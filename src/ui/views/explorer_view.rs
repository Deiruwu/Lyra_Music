use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::{Alignment, Color, Element, Font, Length, Padding, Task};
use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;

use crate::JETBRAINS_MONO;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::{ScrollTracker, VirtualWindow};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::track_row::track_thumbnail;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    "\u{f148}",
    "Explorar",
    JETBRAINS_MONO,
);

const CHUNK_SIZE: usize = 250;

/// Alto fijo de cada fila en píxeles. DEBE coincidir con lo que
/// realmente ocupa `render_row` (padding incluido) o el scroll se
/// desincroniza del contenido real. Si cambias el padding/tamaño de
/// fuente de la fila, actualiza esto.
const ROW_HEIGHT: f32 = 64.0;

/// Filas extra a renderizar arriba y abajo de lo estrictamente visible.
/// Más alto = scroll más suave pero más widgets vivos. 15-20 es un buen
/// punto medio para listas de música.
const BUFFER_ROWS: usize = 15;

// ── ORDENAMIENTO ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Bpm,
    Key,
    Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    fn toggled(self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

const DEFAULT_SORT_COLUMN: SortColumn = SortColumn::Title;
const DEFAULT_SORT_DIRECTION: SortDirection = SortDirection::Asc;

// ── MENSAJES ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ExplorerViewMessage {
    Scrolled(Viewport),
    PlayTrack(Track),
    SortBy(SortColumn),
    SearchChanged(String),
    IdsLoaded(Result<Vec<String>, String>),
    ChunkResolved(usize, Result<Vec<Track>, String>),
    /// (key, bytes, epoch). El epoch se compara contra `self.epoch`
    /// antes de escribir al caché — si no coincide, se descarta.
    ColorThumbnailResult(String, Vec<u8>, u64),
    GrayThumbnailResult(String, Vec<u8>, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExplorerViewOutMessage {
    Idle,
    RequestPlay(Track),
}

// ── ESTADO PRINCIPAL ─────────────────────────────────────────────────────────

pub struct ExplorerView {
    client: Arc<MicroserviceClient>,
    all_tracks: Vec<Track>,
    pending_chunks: HashMap<usize, Vec<Track>>,
    total_chunks: usize,

    // Filtrado en RAM (ya no hay paginación: todo el resultado filtrado
    // se scrollea virtualmente).
    search_query: String,
    filtered_indices: Vec<usize>,

    // Ordenamiento
    sort_column: SortColumn,
    sort_direction: SortDirection,
    sort_click_stage: u8,

    // Scroll virtualizado
    scroll: ScrollTracker,

    // UI Lifecycle & Epoch
    is_loading: bool,
    last_error: Option<String>,
    /// Se incrementa en cada evento que invalida lo que estaba en
    /// pantalla (búsqueda, orden). Las descargas de thumbnails llevan
    /// este valor; si al completarse ya no coincide, se descartan y
    /// además se podan de la cola (drop_stale) para no gastar ancho de
    /// banda en algo que ya no se va a mostrar.
    epoch: u64,
}

impl ExplorerView {
    pub fn new(client: Arc<MicroserviceClient>) -> (Self, Task<ExplorerViewMessage>) {
        let view = Self {
            client: Arc::clone(&client),
            all_tracks: Vec::new(),
            pending_chunks: HashMap::new(),
            total_chunks: 0,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            sort_column: DEFAULT_SORT_COLUMN,
            sort_direction: DEFAULT_SORT_DIRECTION,
            sort_click_stage: 1,
            scroll: ScrollTracker::default(),
            is_loading: true,
            last_error: None,
            epoch: 1,
        };

        let load_ids_task = Task::perform(
            async move { client.get_all_ids().await.map_err(|e| e.to_string()) },
            ExplorerViewMessage::IdsLoaded,
        );

        (view, load_ids_task)
    }

    // ── helpers de filtrado ───────────────────────────────────────────────────

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn track_at(&self, visible_idx: usize) -> Option<&Track> {
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| self.all_tracks.get(idx))
    }

    fn apply_search(&mut self) {
        if self.search_query.trim().is_empty() {
            self.filtered_indices = (0..self.all_tracks.len()).collect();
            return;
        }

        let needle = self.search_query.to_lowercase();
        self.filtered_indices = self
            .all_tracks
            .iter()
            .enumerate()
            .filter_map(|(idx, track)| {
                let title_match = track.title.to_lowercase().contains(&needle);
                let artist_match = track.format_artists().to_lowercase().contains(&needle);
                let album_match = track
                    .album
                    .as_ref()
                    .map(|a| a.name.to_lowercase().contains(&needle))
                    .unwrap_or(false);

                if title_match || artist_match || album_match {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
    }

    fn apply_sort(&mut self) {
        let asc = self.sort_direction == SortDirection::Asc;

        match self.sort_column {
            SortColumn::Title => {
                self.all_tracks.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            }
            SortColumn::Artist => {
                self.all_tracks.sort_by(|a, b| {
                    a.format_artists().to_lowercase().cmp(&b.format_artists().to_lowercase())
                });
            }
            SortColumn::Album => {
                self.all_tracks.sort_by(|a, b| {
                    let an = a.album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
                    let bn = b.album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
                    an.cmp(&bn)
                });
            }
            SortColumn::Bpm => {
                self.all_tracks.sort_by_key(|t| t.bpm.unwrap_or(i32::MIN));
            }
            SortColumn::Key => {
                self.all_tracks.sort_by(|a, b| {
                    let ak = a.camelot_key.clone().unwrap_or_default();
                    let bk = b.camelot_key.clone().unwrap_or_default();
                    ak.cmp(&bk)
                });
            }
            SortColumn::Duration => {
                self.all_tracks.sort_by_key(|t| t.duration_seconds);
            }
        }

        if !asc {
            self.all_tracks.reverse();
        }

        self.apply_search();
    }

    /// Ventana actual a renderizar, según el scroll guardado.
    fn current_window(&self) -> VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    /// Dispara descargas de thumbnails SOLO para la ventana actualmente
    /// visible (+buffer), usando la cola LIFO de `ThumbnailCache`. Se
    /// llama en cada `Scrolled` y también tras cargar/filtrar/ordenar.
    ///
    /// Como la cola es LIFO, encolar en orden start..end hace que el
    /// último `push` (el final de la ventana, la parte "más nueva" en la
    /// dirección del scroll) se descargue primero — que es justo lo que
    /// se pidió: prioriza lo último solicitado.
    fn request_visible_thumbnails(&self, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        let window = self.current_window();
        let epoch = self.epoch;

        let tasks: Vec<Task<ExplorerViewMessage>> = (window.start..window.end)
            .filter_map(|visible_idx| {
                let track = self.track_at(visible_idx)?;
                let url = track
                    .thumbnail_small
                    .clone()
                    .or_else(|| track.thumbnail_large.clone())?;

                if url.is_empty() {
                    return None;
                }

                let key = thumb_key(track);
                thumbnails.request_color(key, url, epoch, |k, bytes, e| {
                    ExplorerViewMessage::ColorThumbnailResult(k, bytes, e)
                })
            })
            .collect();

        Task::batch(tasks)
    }

    /// Keys (`thumb_key`) de todos los tracks en la ventana visible
    /// actual (+buffer). Se usa para podar la cola de descargas en cada
    /// `Scrolled` — ver `DownloadQueue::drop_outside_visible` para el
    /// porqué: sin esto, una key que queda fuera de ventana durante un
    /// arrastre rápido se queda enterrada en el stack para siempre y
    /// nunca se re-descarga aunque vuelvas a scrollear sobre ella.
    fn visible_keys(&self) -> HashSet<String> {
        let window = self.current_window();
        (window.start..window.end)
            .filter_map(|visible_idx| self.track_at(visible_idx))
            .map(thumb_key)
            .collect()
    }

    /// Invalida todo lo que estaba en cola (búsqueda/orden cambiaron el
    /// universo visible por completo) y vuelve a pedir la ventana nueva.
    fn invalidate_and_reload_thumbnails(&mut self, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(thumbnails)
    }

    // ── UPDATE ───────────────────────────────────────────────────────────────

    pub fn update(
        &mut self,
        msg: ExplorerViewMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<ExplorerViewMessage>, ExplorerViewOutMessage) {
        match msg {
            ExplorerViewMessage::IdsLoaded(result) => match result {
                Ok(ids) => {
                    let chunks: Vec<Vec<String>> = ids
                        .chunks(CHUNK_SIZE)
                        .map(|c| c.to_vec())
                        .collect();

                    self.total_chunks = chunks.len();
                    self.pending_chunks.clear();

                    if chunks.is_empty() {
                        self.is_loading = false;
                        return (Task::none(), ExplorerViewOutMessage::Idle);
                    }

                    let tasks: Vec<Task<ExplorerViewMessage>> = chunks
                        .into_iter()
                        .enumerate()
                        .map(|(idx, chunk_ids)| {
                            let client = Arc::clone(&self.client);
                            Task::perform(
                                async move { client.resolve_many(&chunk_ids).await.map_err(|e| e.to_string()) },
                                move |res| ExplorerViewMessage::ChunkResolved(idx, res),
                            )
                        })
                        .collect();

                    (Task::batch(tasks), ExplorerViewOutMessage::Idle)
                }
                Err(e) => {
                    self.is_loading = false;
                    self.last_error = Some(e);
                    (Task::none(), ExplorerViewOutMessage::Idle)
                }
            },

            ExplorerViewMessage::ChunkResolved(chunk_index, result) => {
                if let Ok(tracks) = result {
                    self.pending_chunks.insert(chunk_index, tracks);
                    self.last_error = None;
                } else if let Err(e) = result {
                    self.last_error = Some(e);
                }

                if self.pending_chunks.len() == self.total_chunks {
                    let mut joined: Vec<Track> = Vec::with_capacity(self.total_chunks * CHUNK_SIZE);
                    for i in 0..self.total_chunks {
                        if let Some(tracks) = self.pending_chunks.remove(&i) {
                            joined.extend(tracks);
                        }
                    }
                    self.all_tracks = joined;
                    self.apply_sort();
                    self.is_loading = false;

                    let thumb_task = self.request_visible_thumbnails(thumbnails);
                    return (thumb_task, ExplorerViewOutMessage::Idle);
                }

                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SortBy(column) => {
                if self.sort_column == column {
                    if self.sort_click_stage >= 2 {
                        self.sort_column = DEFAULT_SORT_COLUMN;
                        self.sort_direction = DEFAULT_SORT_DIRECTION;
                        self.sort_click_stage = 1;
                    } else {
                        self.sort_direction = self.sort_direction.toggled();
                        self.sort_click_stage += 1;
                    }
                } else {
                    self.sort_column = column;
                    self.sort_direction = SortDirection::Asc;
                    self.sort_click_stage = 1;
                }

                self.apply_sort();
                let thumb_task = self.invalidate_and_reload_thumbnails(thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.apply_search();
                let thumb_task = self.invalidate_and_reload_thumbnails(thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                // No invalidamos epoch aquí: el scroll no cambia qué
                // tracks existen, solo cuáles son visibles.
                //
                // PERO sí hay que podar: durante un arrastre rápido se
                // generan muchos `Scrolled` seguidos, cada uno encolando
                // ~30 keys (ventana + buffer) contra solo 5 workers
                // simultáneos. La mayoría quedan apiladas sin worker.
                // Si la ventana sigue moviéndose, esas keys terminan
                // fuera de vista pero siguen marcadas como "en cola" —
                // si vuelves a pasar por ahí, `enqueue` las ignora
                // creyendo que ya están en curso, y nunca se descargan.
                // `drop_outside_visible` libera esas keys (solo las que
                // NO tienen worker activo todavía) para que un futuro
                // `Scrolled` sobre esa misma zona sí las vuelva a pedir.
                let keys = self.visible_keys();
                thumbnails.drop_outside_visible(&keys, &keys);
                let thumb_task = self.request_visible_thumbnails(thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::PlayTrack(track) => {
                (Task::none(), ExplorerViewOutMessage::RequestPlay(track))
            }

            ExplorerViewMessage::ColorThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_color(key.clone(), bytes);
                }
                let next = thumbnails.on_color_finished(&key, |k, b, e| {
                    ExplorerViewMessage::ColorThumbnailResult(k, b, e)
                });
                (next, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::GrayThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_gray(key.clone(), bytes);
                }
                let next = thumbnails.on_gray_finished(&key, |k, b, e| {
                    ExplorerViewMessage::GrayThumbnailResult(k, b, e)
                });
                (next, ExplorerViewOutMessage::Idle)
            }
        }
    }

    // ── VIEW ─────────────────────────────────────────────────────────────────

    pub fn view<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let title = text("Catálogo de Pistas")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = text_input("Buscar por título, artista o álbum...", &self.search_query)
            .font(SF_PRO)
            .size(14)
            .padding(Padding { top: 10.0, bottom: 10.0, left: 14.0, right: 14.0 })
            .on_input(ExplorerViewMessage::SearchChanged)
            .width(Length::Fill);

        let fixed_header = column![
            title,
            space().height(12),
            search_bar,
            space().height(16),
            self.render_table_header(),
        ];

        let body_content: Element<'_, ExplorerViewMessage> = if self.is_loading {
            container(text("Cargando catálogo desde microservicios...").font(SF_PRO).size(14))
                .width(Length::Fill)
                .padding(40)
                .align_x(Alignment::Center)
                .into()
        } else if let Some(err) = &self.last_error {
            container(text(format!("Error de conexión: {}", err)).font(SF_PRO).size(14))
                .width(Length::Fill)
                .padding(40)
                .align_x(Alignment::Center)
                .style(|_| container::Style {
                    text_color: Some(Color::from_rgb(0.9, 0.4, 0.4)),
                    ..Default::default()
                })
                .into()
        } else if self.visible_count() == 0 {
            container(text("No se encontraron pistas que coincidan con tu búsqueda.").font(SF_PRO).size(14))
                .width(Length::Fill)
                .padding(40)
                .align_x(Alignment::Center)
                .style(|_| container::Style {
                    text_color: Some(Color::from_rgb(0.6, 0.6, 0.65)),
                    ..Default::default()
                })
                .into()
        } else {
            self.render_virtual_body(thumbnails)
        };

        column![
            fixed_header,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn render_table_header(&self) -> Element<'_, ExplorerViewMessage> {
        let header_cell = |label: &'static str, col: SortColumn, width: Length| -> Element<'_, ExplorerViewMessage> {
            let is_active = self.sort_column == col;
            let arrow = if is_active {
                match self.sort_direction {
                    SortDirection::Asc => " ▲",
                    SortDirection::Desc => " ▼",
                }
            } else {
                ""
            };

            let color = if is_active {
                Color::from_rgb(0.74, 0.58, 0.98)
            } else {
                Color::from_rgb(0.55, 0.55, 0.6)
            };

            button(
                text(format!("{}{}", label, arrow))
                    .font(SF_PRO)
                    .size(11)
                    .style(move |_| text::Style { color: Some(color) }),
            )
                .style(transparent_button)
                .width(width)
                .on_press(ExplorerViewMessage::SortBy(col))
                .into()
        };

        row![
            container(text("#").font(JETBRAINS_MONO).size(12).color(Color::from_rgb(0.5, 0.5, 0.55)))
                .width(Length::Fixed(36.0)),
            container(space()).width(Length::Fixed(60.0)),
            header_cell("TÍTULO", SortColumn::Title, Length::FillPortion(3)),
            header_cell("ARTISTA", SortColumn::Artist, Length::FillPortion(2)),
            header_cell("ÁLBUM", SortColumn::Album, Length::FillPortion(2)),
            header_cell("BPM", SortColumn::Bpm, Length::Fixed(55.0)),
            header_cell("KEY", SortColumn::Key, Length::Fixed(55.0)),
            header_cell("DURACIÓN", SortColumn::Duration, Length::Fixed(70.0)),
        ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding(Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 12.0 })
            .into()
    }

    /// Cuerpo virtualizado: solo construye widgets para
    /// `current_window()`, rellenando arriba/abajo con `space()` del
    /// tamaño exacto que ocuparían las filas no renderizadas, para que
    /// el scrollbar se comporte igual que si todo existiera.
    fn render_virtual_body<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let window = self.current_window();

        let mut rows = column![].width(Length::Fill);
        rows = rows.push(space().height(window.top_spacer_height(ROW_HEIGHT)));

        for visible_idx in window.start..window.end {
            if let Some(track) = self.track_at(visible_idx) {
                let handle = thumbnails.peek_for_render(track);
                rows = rows.push(
                    container(self.render_row(visible_idx + 1, track, handle))
                        .height(Length::Fixed(ROW_HEIGHT))
                );
            }
        }

        rows = rows.push(space().height(window.bottom_spacer_height(ROW_HEIGHT, self.visible_count())));

        scrollable(rows)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(ExplorerViewMessage::Scrolled)
            .into()
    }

    fn render_row<'a>(
        &'a self,
        index: usize,
        track: &'a Track,
        thumbnail: Option<Handle>,
    ) -> Element<'a, ExplorerViewMessage> {
        let idx_text = text(index.to_string())
            .font(JETBRAINS_MONO)
            .size(13)
            .color(Color::from_rgb(0.5, 0.5, 0.55));

        let thumb_el = track_thumbnail(thumbnail);

        let title_el = text(&track.title).font(SF_PRO).size(14).color(Color::WHITE);
        let artist_el = text(track.format_artists()).font(SF_PRO).size(13).color(Color::from_rgb(0.7, 0.7, 0.75));
        let album_el = text(track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("-"))
            .font(SF_PRO)
            .size(13)
            .color(Color::from_rgb(0.6, 0.6, 0.65));

        let bpm_el = text(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".into()))
            .font(JETBRAINS_MONO)
            .size(13)
            .color(Color::from_rgb(0.65, 0.65, 0.7));

        let key_el = text(track.camelot_key.as_deref().unwrap_or("-"))
            .font(JETBRAINS_MONO)
            .size(13)
            .color(Color::from_rgb(0.74, 0.58, 0.98));

        let mins = track.duration_seconds / 60;
        let secs = track.duration_seconds % 60;
        let duration_el = text(format!("{:02}:{:02}", mins, secs))
            .font(JETBRAINS_MONO)
            .size(13)
            .color(Color::from_rgb(0.6, 0.6, 0.65));

        let row_content = row![
            container(idx_text).width(Length::Fixed(36.0)),
            container(thumb_el).width(Length::Fixed(60.0)),
            container(title_el).width(Length::FillPortion(3)),
            container(artist_el).width(Length::FillPortion(2)),
            container(album_el).width(Length::FillPortion(2)),
            container(bpm_el).width(Length::Fixed(55.0)),
            container(key_el).width(Length::Fixed(55.0)),
            container(duration_el).width(Length::Fixed(70.0)),
        ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding(Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 12.0 });

        button(row_content)
            .width(Length::Fill)
            .style(transparent_button)
            .on_press(ExplorerViewMessage::PlayTrack(track.clone()))
            .into()
    }
}