use std::collections::HashSet;

use iced::{Color, Element, Font, Length, Task};
use iced::widget::scrollable::Viewport;
use iced::widget::{column, space, text, Id};
use iced::widget::operation::snap_to;
use iced::widget::scrollable;

use crate::JETBRAINS_MONO;
use crate::model::Track;
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
use crate::ui::widgets::views::sort_state::SortState;
use crate::ui::widgets::views::track_sort;
use crate::ui::widgets::views::catalog_filter;
use crate::ui::widgets::views::track_context_menu::{self, LikeSlot};
use crate::impl_sortable_column;

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
const CONTEXT_MENU_ITEM_COUNT: usize = 7;
const SCROLLABLE_ID: &str = "favorites_catalog_scroll";
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

impl_sortable_column! {
    SortColumn, default = DefaultOrder;
    DefaultOrder => SORT_KEY_DEFAULT_ORDER,
    Title => SORT_KEY_TITLE,
    Artist => SORT_KEY_ARTIST,
    Album => SORT_KEY_ALBUM,
    Bpm => SORT_KEY_BPM,
    Key => SORT_KEY_KEY,
    Duration => SORT_KEY_DURATION,
}

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
    sort: SortState<SortColumn>,
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
            sort: SortState::new(),
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
        self.filtered_indices = catalog_filter::search_indices(&self.liked_tracks(store), &self.search_query);
    }

    fn apply_sort(&mut self, store: &CatalogStore) {
        self.apply_search(store);

        let liked = self.liked_tracks(store);
        let asc = self.sort.is_asc();

        match self.sort.column() {
            SortColumn::DefaultOrder => {
                // Bypass: la indexación natural devuelta por apply_search
                // ya representa el orden cronológico de SQLite.
            }
            SortColumn::Title => track_sort::by_title(&mut self.filtered_indices, &liked),
            SortColumn::Artist => track_sort::by_artist(&mut self.filtered_indices, &liked),
            SortColumn::Album => track_sort::by_album(&mut self.filtered_indices, &liked),
            SortColumn::Bpm => track_sort::by_bpm(&mut self.filtered_indices, &liked),
            SortColumn::Key => track_sort::by_camelot_key(&mut self.filtered_indices, &liked),
            SortColumn::Duration => track_sort::by_duration(&mut self.filtered_indices, &liked),
        }

        if !asc {
            self.filtered_indices.reverse();
        }
    }

    fn current_window(&self) -> crate::ui::utils::virtual_list::VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        catalog_filter::visible_keys(&self.current_window(), |idx| self.track_at(store, idx))
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
                if !self.sort.click(key) {
                    return (Task::none(), FavoritesViewOutMessage::Idle);
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
                active_sort_key: self.sort.active_sort_key(),
                sort_direction_asc: self.sort.is_asc(),
                row_height: ROW_HEIGHT,
                buffer_rows: BUFFER_ROWS,
            };

            let overlay = self.context_menu.render_target(|id| store.track_by_id(id)).map(|(anchor, track)| {
                let items = track_context_menu::build(
                    track,
                    store,
                    LikeSlot::None,
                    ContextMenuAction::AddToPlaylist,
                    None,
                    SUBMENU_ADD_TO_PLAYLIST,
                    ContextMenuAction::PlayNow,
                    ContextMenuAction::AddToQueue,
                    ContextMenuAction::AddToFrontQueue,
                    ContextMenuAction::StartRadio,
                    ContextMenuAction::CopyId,
                    ContextMenuItem::new("Quitar de Me gusta", ContextMenuAction::Unlike).icon(Icon::HeartBroken),
                );

                self.context_menu.view(
                    anchor,
                    items,
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