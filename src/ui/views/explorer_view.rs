use std::collections::HashSet;

use chrono::{DateTime, Utc};
use iced::{Color, Element, Font, Length, Task};
use iced::widget::scrollable::Viewport;
use iced::widget::{column, space, text, Id};
use iced::widget::operation::snap_to;
use iced::widget::scrollable;
use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::utils::search::SearchQuery;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuItem};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use iced::clipboard;
use crate::ui::assets::icons::Icon;
use crate::ui::widgets::track_list::{track_list, TrackListCallbacks, TrackListConfig};
use crate::ui::widgets::track_fields::{Field, FieldList};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    Icon::Explorer,
    "Explorar",
    JETBRAINS_MONO,
);

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;
// El menú ahora tiene 7 filas de "primer nivel" (Reproducir ahora,
// Agregar a cola, Reproducir después, Iniciar radio, Like/Dislike,
// Agregar a playlist, Copiar id, Eliminar): 8 en total. Se usa para
// estimar la altura del menú y clampearlo contra el viewport (ver
// `ContextMenu::clamp_anchor`) — el submenú desplegado NO cuenta aquí,
// tiene su propio flyout independiente.
const CONTEXT_MENU_ITEM_COUNT: usize = 8;
const SCROLLABLE_ID: &str = "explorer_catalog_scroll";
/// Id fijo del (único) submenú de este menú: "Agregar a playlist".
const SUBMENU_ADD_TO_PLAYLIST: usize = 0;

// ── Claves opacas de columna para el widget compartido (`Column::sort_key`) ──
// El widget no conoce `SortColumn`; estas constantes son la traducción
// hacia/desde el `usize` opaco que sí conoce.
const SORT_KEY_TITLE: usize = 0;
const SORT_KEY_ARTIST: usize = 1;
const SORT_KEY_ALBUM: usize = 2;
const SORT_KEY_BPM: usize = 3;
const SORT_KEY_KEY: usize = 4;
const SORT_KEY_DURATION: usize = 5;
const SORT_KEY_ADDED_AT: usize = 6;

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

impl SortColumn {
    fn sort_key(self) -> usize {
        match self {
            SortColumn::Title => SORT_KEY_TITLE,
            SortColumn::Artist => SORT_KEY_ARTIST,
            SortColumn::Album => SORT_KEY_ALBUM,
            SortColumn::Bpm => SORT_KEY_BPM,
            SortColumn::Key => SORT_KEY_KEY,
            SortColumn::Duration => SORT_KEY_DURATION,
            SortColumn::AddedAt => SORT_KEY_ADDED_AT,
        }
    }

    fn from_sort_key(key: usize) -> Option<Self> {
        match key {
            SORT_KEY_TITLE => Some(SortColumn::Title),
            SORT_KEY_ARTIST => Some(SortColumn::Artist),
            SORT_KEY_ALBUM => Some(SortColumn::Album),
            SORT_KEY_BPM => Some(SortColumn::Bpm),
            SORT_KEY_KEY => Some(SortColumn::Key),
            SORT_KEY_DURATION => Some(SortColumn::Duration),
            SORT_KEY_ADDED_AT => Some(SortColumn::AddedAt),
            _ => None,
        }
    }
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
    AddToQueue,
    AddToFrontQueue,
    StartRadio,
    /// Alterna el like. El label/icono que se muestra en el menú ya
    /// refleja el estado actual del track (ver `view()`), así que este
    /// mismo variant sirve tanto para "Me gusta" como para "Ya no me
    /// gusta" — la vista no necesita distinguir Like/Unlike, el store
    /// ya sabe el estado actual vía `Track::liked`.
    ToggleLike,
    AddToPlaylist(String),
    CopyId,
    Delete,
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
    /// `Some(id)` al pasar el mouse sobre "Agregar a playlist", `None`
    /// al salir. Rutea directo a `ContextMenu::set_open_submenu`.
    ContextMenuSubmenuHover(Option<usize>),
    ContextMenuAction(ContextMenuAction, Track),
    ConfirmDialogConfirm,
    ConfirmDialogCancel,
    /// El widget compartido no conoce `SortColumn`: emite el `usize`
    /// opaco de la columna clickeada y la vista lo traduce de vuelta.
    SortByKey(usize),
    SearchChanged(String),
    ColorThumbnailResult(String, Vec<u8>, u64),
    GrayThumbnailResult(String, Vec<u8>, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExplorerViewOutMessage {
    Idle,
    RequestPlay(Track),
    RequestEnqueue(Track),
    RequestFrontEnqueue(Track),
    RequestPlayRadio(Track),
    RequestDelete(String),
    RequestToggleLike(String),
    RequestAddToPlaylist(String, String), // playlist_id, track_id
}

pub struct ExplorerView {
    search_query: String,
    filtered_indices: Vec<usize>,
    sort_column: SortColumn,
    sort_direction: SortDirection,
    sort_click_stage: u8,
    selected_track_id: Option<String>,
    context_menu: ContextMenu<String>,
    confirm_dialog: ConfirmDialog<Track>,
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
            confirm_dialog: ConfirmDialog::new(),
            scroll: ScrollTracker::default(),
            epoch: 1,
        }
    }

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn track_at<'a>(&self, store: &'a CatalogStore, visible_idx: usize) -> Option<&'a Track> {
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| store.all_tracks().get(idx))
    }

    fn visible_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        self.filtered_indices
            .iter()
            .filter_map(|&idx| store.all_tracks().get(idx))
            .collect()
    }

    fn apply_search(&mut self, store: &CatalogStore) {
        let tracks = store.all_tracks();
        let query = SearchQuery::new(&self.search_query);

        if query.is_empty() {
            self.filtered_indices = (0..tracks.len()).collect();
            return;
        }

        self.filtered_indices = tracks
            .iter()
            .enumerate()
            .filter_map(|(idx, track)| {
                let album_name = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
                let artists = track.format_artists();

                let is_match = query.matches_any(&[&track.title, &artists, album_name]);

                is_match.then_some(idx)
            })
            .collect();
    }

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

    fn current_window(&self) -> crate::ui::utils::virtual_list::VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

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

    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        let window = self.current_window();
        (window.start..window.end)
            .filter_map(|visible_idx| self.track_at(store, visible_idx))
            .map(thumb_key)
            .collect()
    }

    fn invalidate_and_reload_thumbnails(&mut self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(store, thumbnails)
    }

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

            ExplorerViewMessage::SortByKey(key) => {
                let Some(column) = SortColumn::from_sort_key(key) else {
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                };

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
                self.scroll.reset();
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);

                let snap = snap_to(
                    Id::new(SCROLLABLE_ID),
                    scrollable::RelativeOffset::START,
                );

                (Task::batch([snap, thumb_task]), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                self.context_menu.note_viewport_size(viewport.bounds().size());
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
                self.context_menu.toggle(track_id.clone(), CONTEXT_MENU_ITEM_COUNT);
                self.selected_track_id = Some(track_id);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::DismissContextMenu => {
                self.context_menu.dismiss();
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ContextMenuSubmenuHover(id) => {
                self.context_menu.set_open_submenu(id);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ContextMenuAction(action, track) => {
                self.context_menu.dismiss();
                self.selected_track_id = Some(track.id.clone());

                if action == ContextMenuAction::Delete {
                    self.confirm_dialog.request(track, "¿Eliminar esta canción del catálogo?");
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                }

                if action == ContextMenuAction::CopyId {
                    return (clipboard::write(track.id.clone()), ExplorerViewOutMessage::Idle);
                }

                let out = match action {
                    ContextMenuAction::PlayNow => ExplorerViewOutMessage::RequestPlay(track),
                    ContextMenuAction::AddToQueue => ExplorerViewOutMessage::RequestEnqueue(track),
                    ContextMenuAction::AddToFrontQueue => ExplorerViewOutMessage::RequestFrontEnqueue(track),
                    ContextMenuAction::StartRadio => ExplorerViewOutMessage::RequestPlayRadio(track),
                    ContextMenuAction::ToggleLike => ExplorerViewOutMessage::RequestToggleLike(track.id.clone()),
                    ContextMenuAction::AddToPlaylist(playlist_id) => {
                        ExplorerViewOutMessage::RequestAddToPlaylist(playlist_id, track.id.clone())
                    }
                    ContextMenuAction::CopyId | ContextMenuAction::Delete => unreachable!(),
                };
                (Task::none(), out)
            }

            ExplorerViewMessage::ConfirmDialogConfirm => {
                let Some(track) = self.confirm_dialog.take_confirmed() else {
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                };
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::RequestDelete(track.id.clone()))
            }

            ExplorerViewMessage::ConfirmDialogCancel => {
                self.confirm_dialog.cancel();
                (Task::none(), ExplorerViewOutMessage::Idle)
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

    /// Antes: `columns()` + `row_cells()` por separado (30 líneas, dos
    /// listas que debían coincidir en orden/longitud a mano). Ahora es
    /// una sola cadena declarativa — ver `ui::widgets::track_fields`.
    fn fields<'a>() -> FieldList<'a, ExplorerViewMessage> {
        Field::index(30.0)
            .thumbnail(THUMBNAIL_SIZE + 12.0, THUMBNAIL_SIZE)
            .title(SORT_KEY_TITLE)
            .artist(SORT_KEY_ARTIST)
            .album(SORT_KEY_ALBUM)
            .duration(SORT_KEY_DURATION)
            .bpm(SORT_KEY_BPM)
            .camelot_key(SORT_KEY_KEY)
            .added_at(SORT_KEY_ADDED_AT)
    }

    pub fn view<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let title = text("Catálogo de Pistas")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = catalog_search_input(
            "Buscar por título, artista o álbum...",
            &self.search_query,
            ExplorerViewMessage::SearchChanged,
        );

        let fixed_header = column![
            title,
            space().height(12),
            search_bar,
        ];

        let body_content: Element<'_, ExplorerViewMessage> = if store.is_loading() {
            catalog_status_message("Cargando catálogo desde microservicios...", StatusTone::Neutral)
        } else if let Some(err) = store.last_error() {
            catalog_status_message(format!("Error de conexión: {}", err), StatusTone::Error)
        } else if self.visible_count() == 0 {
            catalog_status_message("No se encontraron pistas que coincidan con tu búsqueda.", StatusTone::Muted)
        } else {
            let tracks = self.visible_tracks(store);
            let fields = Self::fields();

            let config = TrackListConfig {
                columns: fields.columns(),
                active_sort_key: {
                    let is_default_state = self.sort_column == DEFAULT_SORT_COLUMN
                        && self.sort_direction == DEFAULT_SORT_DIRECTION
                        && self.sort_click_stage == 1;
                    (!is_default_state).then(|| self.sort_column.sort_key())
                },
                sort_direction_asc: self.sort_direction == SortDirection::Asc,
                row_height: ROW_HEIGHT,
                buffer_rows: BUFFER_ROWS,
            };

            let overlay = self.context_menu.render_target(|id| store.track_by_id(id)).map(|(anchor, track)| {
                let (like_label, like_icon) = if track.liked {
                    ("Ya no me gusta", Icon::HeartBroken)
                } else {
                    ("Me gusta", Icon::HeartFull)
                };

                let playlist_children: Vec<ContextMenuItem<ContextMenuAction>> = store
                    .playlists_metadata()
                    .iter()
                    .map(|(playlist_id, name, _)| {
                        let icon = if store.is_track_in_playlist(playlist_id, &track.id) {
                            ""
                        } else {
                            ""
                        };
                        ContextMenuItem::new(
                            name.clone(),
                            ContextMenuAction::AddToPlaylist(playlist_id.clone()),
                        )
                    })
                    .collect();

                self.context_menu.view(
                    anchor,
                    vec![
                        ContextMenuItem::new("Reproducir ahora", ContextMenuAction::PlayNow).icon(Icon::Play),
                        ContextMenuItem::new("Agregar a cola", ContextMenuAction::AddToQueue).icon(Icon::AddQueue),
                        ContextMenuItem::new("Reproducir después", ContextMenuAction::AddToFrontQueue).icon(Icon::AddQueueFront),
                        ContextMenuItem::new("Iniciar radio", ContextMenuAction::StartRadio).icon(Icon::Radio),
                        ContextMenuItem::new(like_label, ContextMenuAction::ToggleLike).icon(like_icon),
                        ContextMenuItem::submenu("Agregar a playlist", SUBMENU_ADD_TO_PLAYLIST, playlist_children)
                            .icon(Icon::Playlist),
                        ContextMenuItem::new("Copiar id", ContextMenuAction::CopyId).icon(Icon::Copiar),
                        ContextMenuItem::new("Eliminar canción", ContextMenuAction::Delete).icon(Icon::Delete),
                    ],
                    track,
                    ExplorerViewMessage::ContextMenuAction,
                    ExplorerViewMessage::DismissContextMenu,
                    ExplorerViewMessage::ContextMenuSubmenuHover,
                )
            });

            let confirm_overlay = self.confirm_dialog.view(
                ExplorerViewMessage::ConfirmDialogConfirm,
                ExplorerViewMessage::ConfirmDialogCancel,
            );

            let combined_overlay = match (overlay, confirm_overlay) {
                (Some(a), Some(b)) => Some(iced::widget::stack![a, b].into()),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };

            track_list(
                config,
                &tracks,
                &self.scroll,
                thumbnails,
                self.selected_track_id.as_deref(),
                SCROLLABLE_ID,
                TrackListCallbacks::new(
                    ExplorerViewMessage::Scrolled,
                    ExplorerViewMessage::PlayTrack,
                    ExplorerViewMessage::SortByKey,
                    ExplorerViewMessage::ViewportMouseMoved,
                    ExplorerViewMessage::RowRightClicked,
                ),
                |track, idx| fields.row_cells(track, idx),
                combined_overlay,
            )
        };

        column![
            fixed_header,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}