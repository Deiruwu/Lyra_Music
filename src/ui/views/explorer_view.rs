use std::collections::HashMap;
use std::sync::Arc;

use iced::{Alignment, Color, Element, Font, Length, Padding, Task};
use iced::widget::{button, column, container, row, space, text, text_input};
use iced::widget::image::Handle;

use crate::JETBRAINS_MONO;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::track_row::track_thumbnail;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    "\u{f148}",
    "Explorar",
    JETBRAINS_MONO,
);

const PAGE_SIZE: usize = 50;
const CHUNK_SIZE: usize = 250;
const PAGE_WINDOW: usize = 2; // Ventana de páginas visibles en la botonera [1 ... 4 5 (6) 7 8 ... 20]

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
    NextPage,
    PrevPage,
    GoToPage(usize),
    PlayTrack(Track),
    SortBy(SortColumn),
    SearchChanged(String),
    IdsLoaded(Result<Vec<String>, String>),
    ChunkResolved(usize, Result<Vec<Track>, String>),
    /// Incluimos u64 (Epoch) para descartar descargas huérfanas de páginas anteriores
    ThumbnailLoaded(String, Vec<u8>, u64),
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

    // Paginación y Filtrado en RAM
    current_page: usize,
    page_size: usize,
    search_query: String,
    filtered_indices: Vec<usize>,

    // Ordenamiento
    sort_column: SortColumn,
    sort_direction: SortDirection,
    sort_click_stage: u8,

    // UI Lifecycle & Epoch
    is_loading: bool,
    last_error: Option<String>,
    page_generation: u64,
}

impl ExplorerView {
    pub fn new(client: Arc<MicroserviceClient>) -> (Self, Task<ExplorerViewMessage>) {
        let view = Self {
            client: Arc::clone(&client),
            all_tracks: Vec::new(),
            pending_chunks: HashMap::new(),
            total_chunks: 0,
            current_page: 0,
            page_size: PAGE_SIZE,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            sort_column: DEFAULT_SORT_COLUMN,
            sort_direction: DEFAULT_SORT_DIRECTION,
            sort_click_stage: 1,
            is_loading: true,
            last_error: None,
            page_generation: 1,
        };

        let load_ids_task = Task::perform(
            async move { client.get_all_ids().await.map_err(|e| e.to_string()) },
            ExplorerViewMessage::IdsLoaded,
        );

        (view, load_ids_task)
    }

    // ── helpers de paginación y filtrado ─────────────────────────────────────

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn total_pages(&self) -> usize {
        let count = self.visible_count();
        if count == 0 {
            0
        } else {
            (count + self.page_size - 1) / self.page_size
        }
    }

    /// Obtiene únicamente el slice de tracks correspondientes a la página activa en O(1)
    fn current_page_slice(&self) -> Vec<&Track> {
        let start = self.current_page * self.page_size;
        if start >= self.filtered_indices.len() {
            return Vec::new();
        }
        let end = (start + self.page_size).min(self.filtered_indices.len());

        self.filtered_indices[start..end]
            .iter()
            .map(|&idx| &self.all_tracks[idx])
            .collect()
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

        // Al reordenar el array base, reconstruimos el índice filtrado
        self.apply_search();
    }

    /// Dispara la descarga de thumbnails ÚNICAMENTE para las 50 pistas de la página actual.
    /// Inyecta `page_generation` al closure para invalidación de caché por época.
    fn request_page_thumbnails(&self, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        let page_gen = self.page_generation;

        let tasks: Vec<Task<ExplorerViewMessage>> = self
            .current_page_slice()
            .into_iter()
            .filter_map(|track| {
                let url = track
                    .thumbnail_small
                    .clone()
                    .or_else(|| track.thumbnail_large.clone())?;

                if url.is_empty() {
                    return None;
                }

                let key = thumb_key(track);

                // No modificamos ThumbnailCache; capturamos `gen` en el closure del mensaje
                thumbnails.request_color(key, url, move |k, bytes| {
                    ExplorerViewMessage::ThumbnailLoaded(k, bytes, page_gen)
                })
            })
            .collect();

        Task::batch(tasks)
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

                    let thumb_task = self.request_page_thumbnails(thumbnails);
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
                self.current_page = 0;
                self.page_generation = self.page_generation.wrapping_add(1);

                let thumb_task = self.request_page_thumbnails(thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.apply_search();
                self.current_page = 0;
                self.page_generation = self.page_generation.wrapping_add(1);

                let thumb_task = self.request_page_thumbnails(thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::NextPage => {
                if self.current_page + 1 < self.total_pages() {
                    self.current_page += 1;
                    self.page_generation = self.page_generation.wrapping_add(1);
                    let thumb_task = self.request_page_thumbnails(thumbnails);
                    return (thumb_task, ExplorerViewOutMessage::Idle);
                }
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::PrevPage => {
                if self.current_page > 0 {
                    self.current_page -= 1;
                    self.page_generation = self.page_generation.wrapping_add(1);
                    let thumb_task = self.request_page_thumbnails(thumbnails);
                    return (thumb_task, ExplorerViewOutMessage::Idle);
                }
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::GoToPage(page) => {
                if page < self.total_pages() && page != self.current_page {
                    self.current_page = page;
                    self.page_generation = self.page_generation.wrapping_add(1);
                    let thumb_task = self.request_page_thumbnails(thumbnails);
                    return (thumb_task, ExplorerViewOutMessage::Idle);
                }
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::PlayTrack(track) => {
                (Task::none(), ExplorerViewOutMessage::RequestPlay(track))
            }

            ExplorerViewMessage::ThumbnailLoaded(key, bytes, task_generation) => {
                // CONTROL DE ÉPOCA: Si el usuario ya cambió de página o filtró,
                // descartamos los bytes para no saturar el LRU ni causar re-renders innecesarios.
                if task_generation == self.page_generation {
                    thumbnails.insert_color(key, bytes);
                }
                (Task::none(), ExplorerViewOutMessage::Idle)
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
            self.render_page_body(thumbnails)
        };

        column![
            fixed_header,
            body_content,
            space().height(12),
            self.render_pagination(),
            space().height(20),
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
            container(space()).width(Length::Fixed(60.0)), // Espacio exacto del thumbnail (60px)
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

    fn render_page_body<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let mut rows = column![].spacing(4).width(Length::Fill);
        let start_idx = self.current_page * self.page_size;

        for (i, track) in self.current_page_slice().into_iter().enumerate() {
            let handle = thumbnails.peek_for_render(track);
            rows = rows.push(self.render_row(start_idx + i + 1, track, handle));
        }

        // En un modelo paginado clásico, no necesitamos scrollable para el body si el alto de la app lo soporta,
        // pero lo mantenemos por si la ventana del usuario es verticalmente pequeña.
        iced::widget::scrollable(rows)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Construcción limpia de la fila con proporciones idénticas al header
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

    fn render_pagination(&self) -> Element<'_, ExplorerViewMessage> {
        let total = self.total_pages();
        if total <= 1 {
            return space().height(0).into();
        }

        let current = self.current_page;

        let prev_btn = button(text("◄").font(JETBRAINS_MONO).size(12))
            .style(transparent_button)
            .padding([6, 12])
            .on_press_maybe(if current > 0 { Some(ExplorerViewMessage::PrevPage) } else { None });

        let next_btn = button(text("►").font(JETBRAINS_MONO).size(12))
            .style(transparent_button)
            .padding([6, 12])
            .on_press_maybe(if current + 1 < total { Some(ExplorerViewMessage::NextPage) } else { None });

        let mut pages_row = row![].spacing(6).align_y(Alignment::Center);
        let mut last_shown: Option<usize> = None;

        for page in 0..total {
            let is_edge = page == 0 || page == total - 1;
            let is_near_current = (page as isize - current as isize).abs() as usize <= PAGE_WINDOW;

            if !is_edge && !is_near_current {
                continue;
            }

            if let Some(last) = last_shown {
                if page > last + 1 {
                    pages_row = pages_row.push(
                        text("···").font(JETBRAINS_MONO).size(14).color(Color::from_rgb(0.4, 0.4, 0.45))
                    );
                }
            }

            let is_active = page == current;
            let (bg_color, text_color) = if is_active {
                (Some(Color::from_rgb(0.74, 0.58, 0.98)), Color::BLACK)
            } else {
                (None, Color::from_rgb(0.7, 0.7, 0.75))
            };

            let page_btn = button(
                text((page + 1).to_string())
                    .font(JETBRAINS_MONO)
                    .size(13)
                    .style(move |_| text::Style { color: Some(text_color) })
            )
                .padding([4, 10])
                .style(move |_theme, _status| button::Style {
                    background: bg_color.map(iced::Background::Color),
                    border: iced::border::rounded(4),
                    ..Default::default()
                })
                .on_press(ExplorerViewMessage::GoToPage(page));

            pages_row = pages_row.push(page_btn);
            last_shown = Some(page);
        }

        row![prev_btn, pages_row, next_btn]
            .spacing(16)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    }
}