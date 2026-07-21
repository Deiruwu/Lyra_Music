use std::collections::HashSet;

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
use iced::clipboard;
use crate::ui::assets::icons::Icon;
use crate::ui::widgets::track_list::{track_list, TrackListCallbacks, TrackListConfig};
use crate::ui::widgets::track_fields::{Field, FieldList};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Favorites,
    Icon::HeartFull,
    "Me gusta",
    JETBRAINS_MONO,
);

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;
// 7 filas de primer nivel: Reproducir ahora, Agregar a cola, Reproducir
// después, Iniciar radio, Agregar a playlist, Copiar id, Quitar de Me
// gusta. Ver comentario equivalente en `explorer_view.rs`.
const CONTEXT_MENU_ITEM_COUNT: usize = 7;
const SCROLLABLE_ID: &str = "favorites_catalog_scroll";
/// Id fijo del (único) submenú de este menú: "Agregar a playlist".
const SUBMENU_ADD_TO_PLAYLIST: usize = 0;

const SORT_KEY_DEFAULT_ORDER: usize = 0;
const SORT_KEY_TITLE: usize = 1;
const SORT_KEY_ARTIST: usize = 2;
const SORT_KEY_ALBUM: usize = 3;
const SORT_KEY_BPM: usize = 4;
const SORT_KEY_KEY: usize = 5;
const SORT_KEY_DURATION: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    DefaultOrder,
    Title,
    Artist,
    Album,
    Bpm,
    Key,
    Duration,
}

impl SortColumn {
    fn sort_key(self) -> usize {
        match self {
            SortColumn::DefaultOrder => SORT_KEY_DEFAULT_ORDER,
            SortColumn::Title => SORT_KEY_TITLE,
            SortColumn::Artist => SORT_KEY_ARTIST,
            SortColumn::Album => SORT_KEY_ALBUM,
            SortColumn::Bpm => SORT_KEY_BPM,
            SortColumn::Key => SORT_KEY_KEY,
            SortColumn::Duration => SORT_KEY_DURATION,
        }
    }

    fn from_sort_key(key: usize) -> Option<Self> {
        match key {
            SORT_KEY_DEFAULT_ORDER => Some(SortColumn::DefaultOrder),
            SORT_KEY_TITLE => Some(SortColumn::Title),
            SORT_KEY_ARTIST => Some(SortColumn::Artist),
            SORT_KEY_ALBUM => Some(SortColumn::Album),
            SORT_KEY_BPM => Some(SortColumn::Bpm),
            SORT_KEY_KEY => Some(SortColumn::Key),
            SORT_KEY_DURATION => Some(SortColumn::Duration),
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

const DEFAULT_SORT_COLUMN: SortColumn = SortColumn::DefaultOrder;
const DEFAULT_SORT_DIRECTION: SortDirection = SortDirection::Asc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    PlayNow,
    AddToQueue,
    AddToFrontQueue,
    StartRadio,
    AddToPlaylist(String),
    CopyId,
    Unlike,
}

#[derive(Debug, Clone)]
pub enum FavoritesViewMessage {
    CatalogUpdated,
    Scrolled(Viewport),
    PlayTrack(Track),
    RowSelected(String),
    ViewportMouseMoved(iced::Point),
    RowRightClicked(String),
    DismissContextMenu,
    ContextMenuSubmenuHover(Option<usize>),
    ContextMenuAction(ContextMenuAction, Track),
    SortByKey(usize),
    SearchChanged(String),
    ColorThumbnailResult(String, Vec<u8>, u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FavoritesViewOutMessage {
    Idle,
    RequestPlayContext(Vec<Track>, usize),
    RequestEnqueue(Track),
    RequestFrontEnqueue(Track),
    RequestPlayRadio(Track),
    RequestToggleLike(String),
    RequestAddToPlaylist(String, String), // playlist_id, track_id
}

pub struct FavoritesView {
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

impl FavoritesView {
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

    fn liked_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        store.tracks_for_playlist(store.system_playlist_id())
    }

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn track_at<'a>(&self, store: &'a CatalogStore, visible_idx: usize) -> Option<&'a Track> {
        let liked = self.liked_tracks(store);
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| liked.get(idx).copied())
    }

    fn visible_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        let liked = self.liked_tracks(store);
        self.filtered_indices
            .iter()
            .filter_map(|&idx| liked.get(idx).copied())
            .collect()
    }

    fn apply_search(&mut self, store: &CatalogStore) {
        let tracks = self.liked_tracks(store);
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

        let tracks = self.liked_tracks(store);
        let asc = self.sort_direction == SortDirection::Asc;

        match self.sort_column {
            SortColumn::DefaultOrder => {
                // Bypass: la indexación natural devuelta por apply_search
                // ya representa el orden cronológico de SQLite.
            }
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
        }

        if !asc {
            self.filtered_indices.reverse();
        }
    }

    fn current_window(&self) -> crate::ui::utils::virtual_list::VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        let window = self.current_window();
        (window.start..window.end)
            .filter_map(|visible_idx| self.track_at(store, visible_idx))
            .map(thumb_key)
            .collect()
    }

    fn request_visible_thumbnails(&self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<FavoritesViewMessage> {
        let window = self.current_window();
        let epoch = self.epoch;

        let tasks: Vec<Task<FavoritesViewMessage>> = (window.start..window.end)
            .filter_map(|visible_idx| {
                let track = self.track_at(store, visible_idx)?;
                let url = track
                    .thumbnail_small
                    .clone()
                    .or_else(|| track.thumbnail_large.clone())?;

                if url.is_empty() { return None; }

                let key = thumb_key(track);
                thumbnails.request_color(key, url, epoch, |k, bytes, e| {
                    FavoritesViewMessage::ColorThumbnailResult(k, bytes, e)
                })
            })
            .collect();

        Task::batch(tasks)
    }

    fn invalidate_and_reload_thumbnails(&mut self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<FavoritesViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(store, thumbnails)
    }

    /// Reconstruye la lista de tracks en el orden filtrado actual como
    /// `Vec<Track>` clonados, para armar el "contexto de reproducción"
    /// (necesario porque a diferencia de Explorer, tocar cualquier track
    /// en Favoritos reproduce la playlist completa desde ese punto, no
    /// solo el track individual).
    fn context_tracks(&self, store: &CatalogStore) -> Vec<Track> {
        let liked = self.liked_tracks(store);
        self.filtered_indices
            .iter()
            .filter_map(|&idx| liked.get(idx).map(|t| (*t).clone()))
            .collect()
    }

    pub fn update(
        &mut self,
        msg: FavoritesViewMessage,
        store: &CatalogStore,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<FavoritesViewMessage>, FavoritesViewOutMessage) {
        match msg {
            FavoritesViewMessage::CatalogUpdated => {
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::SortByKey(key) => {
                let Some(column) = SortColumn::from_sort_key(key) else {
                    return (Task::none(), FavoritesViewOutMessage::Idle);
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
                (thumb_task, FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.apply_sort(store);
                self.scroll.reset();
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);

                let snap = snap_to(
                    Id::new(SCROLLABLE_ID),
                    scrollable::RelativeOffset::START,
                );

                (Task::batch([snap, thumb_task]), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                self.context_menu.note_viewport_size(viewport.bounds().size());

                let keys = self.visible_keys(store);
                thumbnails.drop_outside_visible(&keys, &keys);

                let thumb_task = self.request_visible_thumbnails(store, thumbnails);
                (thumb_task, FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::PlayTrack(track) => {
                self.selected_track_id = Some(track.id.clone());
                self.context_menu.dismiss();

                let context_tracks = self.context_tracks(store);
                let start_idx = context_tracks.iter().position(|t| t.id == track.id).unwrap_or(0);

                (Task::none(), FavoritesViewOutMessage::RequestPlayContext(context_tracks, start_idx))
            }

            FavoritesViewMessage::RowSelected(track_id) => {
                self.selected_track_id = Some(track_id);
                (Task::none(), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::ViewportMouseMoved(point) => {
                self.context_menu.note_mouse_position(point);
                (Task::none(), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::RowRightClicked(track_id) => {
                self.context_menu.toggle(track_id.clone(), CONTEXT_MENU_ITEM_COUNT);
                self.selected_track_id = Some(track_id);
                (Task::none(), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::DismissContextMenu => {
                self.context_menu.dismiss();
                (Task::none(), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::ContextMenuSubmenuHover(id) => {
                self.context_menu.set_open_submenu(id);
                (Task::none(), FavoritesViewOutMessage::Idle)
            }

            FavoritesViewMessage::ContextMenuAction(action, track) => {
                self.context_menu.dismiss();
                self.selected_track_id = Some(track.id.clone());

                if action == ContextMenuAction::CopyId {
                    return (clipboard::write(track.id.clone()), FavoritesViewOutMessage::Idle);
                }

                let out = match action {
                    ContextMenuAction::PlayNow => {
                        let context_tracks = self.context_tracks(store);
                        let start_idx = context_tracks.iter().position(|t| t.id == track.id).unwrap_or(0);
                        FavoritesViewOutMessage::RequestPlayContext(context_tracks, start_idx)
                    },
                    ContextMenuAction::AddToQueue => FavoritesViewOutMessage::RequestEnqueue(track),
                    ContextMenuAction::AddToFrontQueue => FavoritesViewOutMessage::RequestFrontEnqueue(track),
                    ContextMenuAction::StartRadio => FavoritesViewOutMessage::RequestPlayRadio(track),
                    ContextMenuAction::AddToPlaylist(playlist_id) => {
                        FavoritesViewOutMessage::RequestAddToPlaylist(playlist_id, track.id.clone())
                    }
                    ContextMenuAction::CopyId => unreachable!(),
                    ContextMenuAction::Unlike => FavoritesViewOutMessage::RequestToggleLike(track.id.clone()),
                };
                (Task::none(), out)
            }

            FavoritesViewMessage::ColorThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_color(key.clone(), bytes);
                }
                let next = thumbnails.on_color_finished(&key, |k, b, e| {
                    FavoritesViewMessage::ColorThumbnailResult(k, b, e)
                });
                (next, FavoritesViewOutMessage::Idle)
            }
        }
    }

    fn fields<'a>() -> FieldList<'a, FavoritesViewMessage> {
        Field::index_sortable(40.0, SORT_KEY_DEFAULT_ORDER)
            .thumbnail(THUMBNAIL_SIZE + 2.0, THUMBNAIL_SIZE)
            .title(SORT_KEY_TITLE)
            .artist(SORT_KEY_ARTIST)
            .album(SORT_KEY_ALBUM)
            .duration(SORT_KEY_DURATION)
            .bpm(SORT_KEY_BPM)
            .camelot_key(SORT_KEY_KEY)
    }

    pub fn view<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, FavoritesViewMessage> {
        let title = text("Me gusta")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = catalog_search_input(
            "Buscar en tus favoritos...",
            &self.search_query,
            FavoritesViewMessage::SearchChanged,
        );

        let fixed_header = column![
            title,
            space().height(12),
            search_bar,
        ];

        let body_content: Element<'_, FavoritesViewMessage> = if store.is_loading() {
            catalog_status_message("Cargando catálogo desde microservicios...", StatusTone::Neutral)
        } else if let Some(err) = store.last_error() {
            catalog_status_message(format!("Error de conexión: {}", err), StatusTone::Error)
        } else if self.liked_tracks(store).is_empty() {
            catalog_status_message("Aún no has marcado ninguna canción con \"Me gusta\".", StatusTone::Muted)
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
                        ContextMenuItem::submenu("Agregar a playlist", SUBMENU_ADD_TO_PLAYLIST, playlist_children)
                            .icon(Icon::Playlist),
                        ContextMenuItem::new("Copiar id", ContextMenuAction::CopyId).icon(Icon::Copiar),
                        ContextMenuItem::new("Quitar de Me gusta", ContextMenuAction::Unlike).icon(Icon::HeartBroken),
                    ],
                    track,
                    FavoritesViewMessage::ContextMenuAction,
                    FavoritesViewMessage::DismissContextMenu,
                    FavoritesViewMessage::ContextMenuSubmenuHover,
                )
            });

            track_list(
                config,
                &tracks,
                &self.scroll,
                thumbnails,
                self.selected_track_id.as_deref(),
                SCROLLABLE_ID,
                TrackListCallbacks::new(
                    FavoritesViewMessage::Scrolled,
                    FavoritesViewMessage::PlayTrack,
                    FavoritesViewMessage::SortByKey,
                    FavoritesViewMessage::ViewportMouseMoved,
                    FavoritesViewMessage::RowRightClicked,
                ),
                |track, idx| fields.row_cells(track, idx),
                overlay,
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