use std::collections::HashSet;

use chrono::{DateTime, Datelike, Utc};
use iced::{Alignment, Color, Element, Font, Length, Padding, Task};
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, stack, text, text_input, Id};
use iced::widget::image::Handle;

use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::styles::styles::{selected_row_container, transparent_button};
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::{ScrollTracker, VirtualWindow};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::context_menu::ContextMenu;
use crate::ui::widgets::track_row::track_thumbnail_sized;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    "\u{f148}",
    "Explorar",
    JETBRAINS_MONO,
);

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Bpm,
    Key,
    Duration,
    AddedAt,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    PlayNow,
    Enqueue,
}

#[derive(Debug, Clone)]
pub enum ExplorerViewMessage {
    CatalogUpdated,
    Scrolled(Viewport),
    PlayTrack(Track),
    RowSelected(String),
    ViewportMouseMoved(iced::Point),
    RowRightClicked(String),
    DismissContextMenu,
    ContextMenuAction(ContextMenuAction, Track),
    SortBy(SortColumn),
    SearchChanged(String),
    ColorThumbnailResult(String, Vec<u8>, u64),
    GrayThumbnailResult(String, Vec<u8>, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExplorerViewOutMessage {
    Idle,
    RequestPlay(Track),
    RequestEnqueue(Track),
}

pub struct ExplorerView {
    search_query: String,
    filtered_indices: Vec<usize>,
    sort_column: SortColumn,
    sort_direction: SortDirection,
    sort_click_stage: u8,
    selected_track_id: Option<String>,
    context_menu: ContextMenu<String>,
    scroll: ScrollTracker,
    epoch: u64,
}

impl ExplorerView {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            filtered_indices: Vec::new(),
            sort_column: DEFAULT_SORT_COLUMN,
            sort_direction: DEFAULT_SORT_DIRECTION,
            sort_click_stage: 1,
            selected_track_id: None,
            context_menu: ContextMenu::new(),
            scroll: ScrollTracker::default(),
            epoch: 1,
        }
    }

    // Cantidad de tracks visibles tras el filtro de búsqueda actual.
    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    // Traduce un índice "visible" (post-filtro) al Track real en el store.
    fn track_at<'a>(&self, store: &'a CatalogStore, visible_idx: usize) -> Option<&'a Track> {
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| store.all_tracks().get(idx))
    }

    // Recalcula filtered_indices según search_query (título/artista/álbum).
    fn apply_search(&mut self, store: &CatalogStore) {
        let tracks = store.all_tracks();

        if self.search_query.trim().is_empty() {
            self.filtered_indices = (0..tracks.len()).collect();
            return;
        }

        let needle = self.search_query.to_lowercase();
        self.filtered_indices = tracks
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

    // Re-filtra y luego ordena filtered_indices según sort_column/sort_direction.
    fn apply_sort(&mut self, store: &CatalogStore) {
        self.apply_search(store);

        let tracks = store.all_tracks();
        let asc = self.sort_direction == SortDirection::Asc;

        match self.sort_column {
            SortColumn::Title => {
                self.filtered_indices.sort_by(|&a, &b| {
                    tracks[a].title.to_lowercase().cmp(&tracks[b].title.to_lowercase())
                });
            }
            SortColumn::Artist => {
                self.filtered_indices.sort_by(|&a, &b| {
                    tracks[a].format_artists().to_lowercase().cmp(&tracks[b].format_artists().to_lowercase())
                });
            }
            SortColumn::Album => {
                self.filtered_indices.sort_by(|&a, &b| {
                    let an = tracks[a].album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
                    let bn = tracks[b].album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
                    an.cmp(&bn)
                });
            }
            SortColumn::Bpm => {
                self.filtered_indices.sort_by_key(|&i| tracks[i].bpm.unwrap_or(i32::MIN));
            }
            SortColumn::Key => {
                self.filtered_indices.sort_by(|&a, &b| {
                    let ak = tracks[a].camelot_key.clone().unwrap_or_default();
                    let bk = tracks[b].camelot_key.clone().unwrap_or_default();
                    ak.cmp(&bk)
                });
            }
            SortColumn::Duration => {
                self.filtered_indices.sort_by_key(|&i| tracks[i].duration_seconds);
            }
            SortColumn::AddedAt => {
                self.filtered_indices.sort_by_key(|&i| {
                    tracks[i].added_at.unwrap_or(DateTime::<Utc>::MIN_UTC)
                });
            }
        }

        if !asc {
            self.filtered_indices.reverse();
        }
    }

    // Calcula qué rango de filas debe renderizarse dado el scroll actual (virtual list).
    fn current_window(&self) -> VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    // Dispara tasks de carga de thumbnail (color) solo para las filas dentro de la ventana visible.
    fn request_visible_thumbnails(&self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        let window = self.current_window();
        let epoch = self.epoch;

        let tasks: Vec<Task<ExplorerViewMessage>> = (window.start..window.end)
            .filter_map(|visible_idx| {
                let track = self.track_at(store, visible_idx)?;
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

    // Set de thumb_keys de los tracks actualmente en pantalla (usado para invalidar cache fuera de rango).
    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        let window = self.current_window();
        (window.start..window.end)
            .filter_map(|visible_idx| self.track_at(store, visible_idx))
            .map(thumb_key)
            .collect()
    }

    // Sube el epoch (invalida thumbnails viejos en vuelo) y vuelve a pedir los de la ventana visible.
    fn invalidate_and_reload_thumbnails(&mut self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(store, thumbnails)
    }

    // Maneja todos los mensajes de la vista: búsqueda, orden, scroll, clicks y resultados de thumbnails.
    pub fn update(
        &mut self,
        msg: ExplorerViewMessage,
        store: &CatalogStore,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<ExplorerViewMessage>, ExplorerViewOutMessage) {
        match msg {
            ExplorerViewMessage::CatalogUpdated => {
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
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

                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                let keys = self.visible_keys(store);
                thumbnails.drop_outside_visible(&keys, &keys);
                let thumb_task = self.request_visible_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::PlayTrack(track) => {
                self.selected_track_id = Some(track.id.clone());
                self.context_menu.dismiss();
                (Task::none(), ExplorerViewOutMessage::RequestPlay(track))
            }

            ExplorerViewMessage::RowSelected(track_id) => {
                self.selected_track_id = Some(track_id);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ViewportMouseMoved(point) => {
                self.context_menu.note_mouse_position(point);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::RowRightClicked(track_id) => {
                self.context_menu.toggle(track_id.clone());
                self.selected_track_id = Some(track_id);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::DismissContextMenu => {
                self.context_menu.dismiss();
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ContextMenuAction(action, track) => {
                self.context_menu.dismiss();
                self.selected_track_id = Some(track.id.clone());
                let out = match action {
                    ContextMenuAction::PlayNow => ExplorerViewOutMessage::RequestPlay(track),
                    ContextMenuAction::Enqueue => ExplorerViewOutMessage::RequestEnqueue(track),
                };
                (Task::none(), out)
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

    // Arma el layout completo: título, buscador, header de tabla y el cuerpo (loading/error/vacío/lista).
    pub fn view<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let title = text("Catálogo de Pistas")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = text_input("Buscar por título, artista o álbum...", &self.search_query)
            .font(SF_PRO)
            .size(14)
            .padding(Padding { top: 10.0, bottom: 10.0, left: 14.0, right: 14.0 })
            .style(|theme, status| {
                let mut style = text_input::default(theme, status);
                style.border.radius = 8.0.into();
                style
            })
            .on_input(ExplorerViewMessage::SearchChanged)
            .width(Length::Fill);

        let fixed_header = column![
            title,
            space().height(12),
            search_bar,
            space().height(16),
            self.render_table_header(),
        ];

        let body_content: Element<'_, ExplorerViewMessage> = if store.is_loading() {
            container(text("Cargando catálogo desde microservicios...").font(SF_PRO).size(14))
                .width(Length::Fill)
                .padding(40)
                .align_x(Alignment::Center)
                .into()
        } else if let Some(err) = store.last_error() {
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
            self.render_virtual_body(store, thumbnails)
        };

        column![
            fixed_header,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // Formatea la fecha "agregado" al estilo "9 jul 2026", o "-" si no hay fecha.
    fn format_added_at(added_at: Option<DateTime<Utc>>) -> String {
        const MESES: [&str; 12] = [
            "ene", "feb", "mar", "abr", "may", "jun",
            "jul", "ago", "sep", "oct", "nov", "dic",
        ];
        match added_at {
            Some(dt) => {
                format!("{} {} {}", dt.day(), MESES[dt.month0() as usize], dt.year())
            }
            None => "-".to_string(),
        }
    }

    // Construye la fila de encabezados clickeables (con flecha de orden activo) para cada columna.
    fn render_table_header(&self) -> Element<'_, ExplorerViewMessage> {
        let is_default_state = self.sort_column == DEFAULT_SORT_COLUMN
            && self.sort_direction == DEFAULT_SORT_DIRECTION
            && self.sort_click_stage == 1;

        let header_cell = |label: &'static str, col: SortColumn, width: Length| -> Element<'_, ExplorerViewMessage> {
            let is_active = self.sort_column == col && !is_default_state;
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
                Color::from_rgb(0.5, 0.5, 0.55)
            };

            button(
                text(format!("{}{}", label, arrow))
                    .font(SF_PRO)
                    .size(10.5)
                    .style(move |_| text::Style { color: Some(color) }),
            )
                .style(transparent_button)
                .width(width)
                .padding(0)
                .on_press(ExplorerViewMessage::SortBy(col))
                .into()
        };

        row![
            container(text("#").font(JETBRAINS_MONO).size(11).color(Color::from_rgb(0.45, 0.45, 0.5)))
                .width(Length::Fixed(30.0)),
            container(space()).width(Length::Fixed(THUMBNAIL_SIZE + 12.0)),
            header_cell("TÍTULO", SortColumn::Title, Length::FillPortion(3)),
            header_cell("ARTISTA", SortColumn::Artist, Length::FillPortion(2)),
            header_cell("ÁLBUM", SortColumn::Album, Length::FillPortion(2)),
            header_cell("DURACIÓN", SortColumn::Duration, Length::Fixed(60.0))  ,
            header_cell("BPM", SortColumn::Bpm, Length::Fixed(42.0)),
            header_cell("KEY", SortColumn::Key, Length::Fixed(42.0)),
            header_cell("AGREGADO", SortColumn::AddedAt, Length::Fixed(80.0)),
        ]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding(Padding { top: 4.0, bottom: 4.0, left: 10.0, right: 16.0 })
            .into()
    }

    // Renderiza una fila de track: thumbnail + columnas + botón (play) + área de click derecho (menú contextual).
    fn render_row<'a>(
        &self,
        index: usize,
        track: &'a Track,
        thumbnail: Option<Handle>,
        is_selected: bool,
    ) -> Element<'a, ExplorerViewMessage> {
        let mins = track.duration_seconds / 60;
        let secs = track.duration_seconds % 60;

        let row_content = row![
            container(text(index.to_string()).font(SF_PRO).size(12).color(Color::from_rgb(0.45, 0.45, 0.5)))
                .width(Length::Fixed(30.0)),
            container(track_thumbnail_sized(thumbnail, THUMBNAIL_SIZE))
                .width(Length::Fixed(THUMBNAIL_SIZE + 12.0)),
            container(text(&track.title).font(SF_PRO).size(14.5).color(Color::WHITE))
                .width(Length::FillPortion(3)),
            container(text(track.format_artists()).font(SF_PRO).size(12.5).color(Color::from_rgb(0.7, 0.7, 0.75)))
                .width(Length::FillPortion(2)),
            container(text(track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("-")).font(SF_PRO).size(12.5).color(Color::from_rgb(0.6, 0.6, 0.65)))
                .width(Length::FillPortion(2)),
            container(text(Self::format_added_at(track.added_at)).font(SF_PRO).size(12).color(Color::from_rgb(0.55, 0.55, 0.6)))
                .width(Length::Fixed(80.0)),
            container(text(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".into())).font(SF_PRO).size(12.5).color(Color::from_rgb(0.65, 0.65, 0.7)))
                .width(Length::Fixed(42.0)),
            container(text(track.camelot_key.as_deref().unwrap_or("-")).font(JETBRAINS_MONO).size(12.5).color(Color::from_rgb(0.74, 0.58, 0.98)))
                .width(Length::Fixed(42.0)),
            container(text(format!("{:02}:{:02}", mins, secs)).font(SF_PRO).size(12.5).color(Color::from_rgb(0.6, 0.6, 0.65)))
                .width(Length::Fixed(60.0)),
        ]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding(Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 16.0 });

        let track_clone = track.clone();
        let track_id = track.id.clone();

        let btn = button(row_content)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .style(transparent_button)
            .on_press(ExplorerViewMessage::PlayTrack(track_clone));

        let styled_container = container(btn)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .align_y(Alignment::Center)
            .style(selected_row_container(is_selected));

        mouse_area(styled_container)
            .on_right_press(ExplorerViewMessage::RowRightClicked(track_id))
            .into()
    }

    // Renderiza la lista virtualizada (spacers + filas visibles) dentro de un scrollable, y superpone el menú contextual si está abierto.
    fn render_virtual_body<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let window = self.current_window();

        let mut rows = column![].width(Length::Fill);
        rows = rows.push(space().height(window.top_spacer_height(ROW_HEIGHT)));

        for visible_idx in window.start..window.end {
            if let Some(track) = self.track_at(store, visible_idx) {
                let handle = thumbnails.peek_for_render(track);
                let is_selected = self.selected_track_id.as_deref() == Some(track.id.as_str());
                rows = rows.push(self.render_row(visible_idx + 1, track, handle, is_selected));
            }
        }

        rows = rows.push(space().height(window.bottom_spacer_height(ROW_HEIGHT, self.visible_count())));

        let scroll_area: Element<'_, ExplorerViewMessage> = scrollable(rows)
            .id(Id::new("explorer_catalog_scroll"))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(ExplorerViewMessage::Scrolled)
            .into();

        let scroll_area: Element<'_, ExplorerViewMessage> = mouse_area(scroll_area)
            .on_move(ExplorerViewMessage::ViewportMouseMoved)
            .into();

        let mut layers = stack![scroll_area];

        if let Some((anchor, track)) = self.context_menu.render_target(|id| store.track_by_id(id)) {
            let menu = self.context_menu.view(
                anchor,
                vec![
                    ("▶  Reproducir ahora", ContextMenuAction::PlayNow),
                    ("＋  Agregar a cola", ContextMenuAction::Enqueue),
                ],
                track,
                ExplorerViewMessage::ContextMenuAction,
                ExplorerViewMessage::DismissContextMenu,
            );
            layers = layers.push(menu);
        }

        layers.into()
    }
}