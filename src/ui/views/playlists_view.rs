use std::collections::HashSet;
use std::time::Instant;

use iced::{Alignment, Color, Element, Font, Length, Task};
use iced::keyboard::Modifiers;
use iced::widget::scrollable::Viewport;
use iced::widget::{column, container, row, space, text, Id, button};
use iced::widget::operation::{snap_to, scroll_by};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::scrollable;
use iced::clipboard;

use crate::model::Track;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuItem};
use crate::ui::widgets::playlist_header::{playlist_header, PlaylistHeaderData};
use crate::ui::widgets::track_list::{track_list, Cell, Column, TrackListCallbacks, TrackListConfig};
use crate::ui::widgets::track_fields::{Field, FieldList};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::styles::styles::transparent_button;
use crate::ui::assets::icons::Icon;
use crate::ui::widgets::views::sort_state::SortState;
use crate::ui::widgets::views::track_sort;
use crate::ui::widgets::views::catalog_filter;
use crate::ui::widgets::views::track_context_menu::{self, LikeSlot};
use crate::impl_sortable_column;
use crate::ui::widgets::selection_state::SelectionState;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;
const CONTEXT_MENU_ITEM_COUNT: usize = 8;
const SCROLLABLE_ID: &str = "playlists_catalog_scroll";
const SUBMENU_ADD_TO_PLAYLIST: usize = 0;

// ── Auto-scroll durante drag de reordenamiento ──────────────────────────────
/// Alto, en píxeles, de la franja sensible junto a cada borde del viewport
/// donde el drag empieza a autoscrollear.
const AUTOSCROLL_ZONE_PX: f32 = 50.0;
/// Velocidad máxima de scroll por tick (16ms) cuando el mouse está pegado
/// al borde extremo de la zona sensible.
const AUTOSCROLL_MAX_SPEED_PX: f32 = 18.0;

const SORT_KEY_DEFAULT_ORDER: usize = 0;
const SORT_KEY_TITLE: usize = 1;
const SORT_KEY_ARTIST: usize = 2;
const SORT_KEY_ALBUM: usize = 3;
const SORT_KEY_DURATION: usize = 4;
const SORT_KEY_BPM: usize = 5;
const SORT_KEY_KEY: usize = 6;

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
    ToggleLike,
    AddToPlaylist(String),
    CopyId,
    RemoveFromPlaylist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistsSubView {
    Overview,
    Detail(String),
}

#[derive(Debug, Clone)]
pub enum PlaylistsViewMessage {
    SelectPlaylist(String),
    BackToOverview,
    PlayPlaylist(String),
    CreatePlaylistRequested,

    Scrolled(Viewport),
    RowClicked(Track, usize),
    ViewportMouseMoved(iced::Point),
    ViewportMouseExited,
    RowRightClicked(String),
    DismissContextMenu,
    ContextMenuSubmenuHover(Option<usize>),
    ContextMenuAction(ContextMenuAction, Track),
    SortByKey(usize),
    SearchChanged(String),
    ColorThumbnailResult(String, Vec<u8>, u64),
    ModifiersChanged(Modifiers),

    GlobalMousePress,
    GlobalMouseRelease,
    AutoScrollTick,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistsViewOutMessage {
    Idle,
    RequestPlayContext(Vec<Track>, usize),
    RequestEnqueue(Vec<Track>),
    RequestFrontEnqueue(Vec<Track>),
    RequestPlayRadio(Track),
    RequestRemoveFromPlaylist(String, Vec<String>),
    RequestToggleLike(Vec<String>),
    RequestAddToPlaylist(String, Vec<String>),
    CreatePlaylistRequested,
    RequestReorder(String, usize, usize),
}

struct DragState {
    source_index: usize,
    current_index: usize,
    grab_offset: f32,
}

pub struct PlaylistsView {
    pub current_subview: PlaylistsSubView,

    search_query: String,
    filtered_indices: Vec<usize>,
    sort: SortState<SortColumn>,

    selection: SelectionState,
    last_click: Option<(String, Instant)>,
    pub current_modifiers: Modifiers,

    context_menu: ContextMenu<String>,
    scroll: ScrollTracker,
    epoch: u64,

    hovered_point: Option<iced::Point>,
    pending_drag_start_point: Option<iced::Point>,
    drag_state: Option<DragState>,
}

impl Default for PlaylistsView {
    fn default() -> Self {
        Self {
            current_subview: PlaylistsSubView::Overview,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            sort: SortState::new(),
            selection: SelectionState::new(),
            last_click: None,
            current_modifiers: Modifiers::default(),
            context_menu: ContextMenu::new(),
            scroll: ScrollTracker::default(),
            epoch: 1,
            hovered_point: None,
            pending_drag_start_point: None,
            drag_state: None,
        }
    }
}

impl PlaylistsView {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` mientras el usuario está arrastrando una fila para
    /// reordenarla. Usado por el padre para decidir si mantener viva
    /// la subscripción de auto-scroll.
    pub fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    fn reset_detail_state(&mut self) {
        self.search_query.clear();
        self.filtered_indices.clear();
        self.sort.reset();
        self.selection.clear();
        self.last_click = None;
        self.context_menu.dismiss();
        self.scroll.reset();
        self.hovered_point = None;
        self.pending_drag_start_point = None;
        self.drag_state = None;
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn current_playlist_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        match &self.current_subview {
            PlaylistsSubView::Detail(id) => store.tracks_for_playlist(id),
            _ => Vec::new(),
        }
    }

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn track_at<'a>(&self, store: &'a CatalogStore, visible_idx: usize) -> Option<&'a Track> {
        let tracks = self.current_playlist_tracks(store);
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| tracks.get(idx).copied())
    }

    fn visible_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        let tracks = self.current_playlist_tracks(store);
        self.filtered_indices
            .iter()
            .filter_map(|&idx| tracks.get(idx).copied())
            .collect()
    }

    fn get_selected_tracks(&self, store: &CatalogStore) -> Vec<Track> {
        let tracks = self.current_playlist_tracks(store);
        self.filtered_indices
            .iter()
            .filter_map(|&idx| tracks.get(idx).copied())
            .filter(|t| self.selection.is_selected(&t.id))
            .cloned()
            .collect()
    }

    fn apply_search(&mut self, store: &CatalogStore) {
        self.filtered_indices =
            catalog_filter::search_indices(&self.current_playlist_tracks(store), &self.search_query);
    }

    fn apply_sort(&mut self, store: &CatalogStore) {
        self.apply_search(store);

        let tracks = self.current_playlist_tracks(store);
        let asc = self.sort.is_asc();

        match self.sort.column() {
            SortColumn::DefaultOrder => {},
            SortColumn::Title => track_sort::by_title(&mut self.filtered_indices, &tracks),
            SortColumn::Artist => track_sort::by_artist(&mut self.filtered_indices, &tracks),
            SortColumn::Album => track_sort::by_album(&mut self.filtered_indices, &tracks),
            SortColumn::Bpm => track_sort::by_bpm(&mut self.filtered_indices, &tracks),
            SortColumn::Key => track_sort::by_camelot_key(&mut self.filtered_indices, &tracks),
            SortColumn::Duration => track_sort::by_duration(&mut self.filtered_indices, &tracks),
        }

        if !asc {
            self.filtered_indices.reverse();
        }
    }

    fn context_tracks(&self, store: &CatalogStore) -> Vec<Track> {
        let playlist_tracks = self.current_playlist_tracks(store);
        self.filtered_indices
            .iter()
            .filter_map(|&idx| playlist_tracks.get(idx).map(|t| (*t).clone()))
            .collect()
    }

    fn current_window(&self) -> crate::ui::utils::virtual_list::VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        catalog_filter::visible_keys(&self.current_window(), |idx| self.track_at(store, idx))
    }

    fn request_visible_thumbnails(&self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<PlaylistsViewMessage> {
        let window = self.current_window();
        let epoch = self.epoch;

        let tasks: Vec<Task<PlaylistsViewMessage>> = (window.start..window.end)
            .filter_map(|visible_idx| {
                let track = self.track_at(store, visible_idx)?;
                let url = track.thumbnail_small.clone().or_else(|| track.thumbnail_large.clone())?;
                if url.is_empty() { return None; }

                let key = thumb_key(track);
                thumbnails.request_color(key, url, epoch, |k, bytes, e| {
                    PlaylistsViewMessage::ColorThumbnailResult(k, bytes, e)
                })
            })
            .collect();

        Task::batch(tasks)
    }

    fn invalidate_and_reload_thumbnails(&mut self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<PlaylistsViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(store, thumbnails)
    }

    pub fn update(
        &mut self,
        msg: PlaylistsViewMessage,
        store: &CatalogStore,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<PlaylistsViewMessage>, PlaylistsViewOutMessage) {
        match msg {
            PlaylistsViewMessage::ModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers;
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::SelectPlaylist(id) => {
                self.reset_detail_state();
                self.current_subview = PlaylistsSubView::Detail(id);
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::BackToOverview => {
                self.reset_detail_state();
                self.current_subview = PlaylistsSubView::Overview;
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::PlayPlaylist(id) => {
                if let PlaylistsSubView::Detail(current_id) = &self.current_subview {
                    if current_id == &id {
                        let context = self.context_tracks(store);
                        return (Task::none(), PlaylistsViewOutMessage::RequestPlayContext(context, 0));
                    }
                }
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::CreatePlaylistRequested => {
                (Task::none(), PlaylistsViewOutMessage::CreatePlaylistRequested)
            }

            PlaylistsViewMessage::SortByKey(key) => {
                if !self.sort.click(key) {
                    return (Task::none(), PlaylistsViewOutMessage::Idle);
                }
                self.selection.clear();
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.selection.clear();
                self.apply_sort(store);
                self.scroll.reset();
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                let snap = snap_to(Id::new(SCROLLABLE_ID), scrollable::RelativeOffset::START);
                (Task::batch([snap, thumb_task]), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                self.context_menu.note_viewport_size(viewport.bounds().size());
                let keys = self.visible_keys(store);
                thumbnails.drop_outside_visible(&keys, &keys);
                let thumb_task = self.request_visible_thumbnails(store, thumbnails);
                (thumb_task, PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::RowClicked(track, index) => {
                let now = Instant::now();
                let is_double_click = match &self.last_click {
                    Some((last_id, time)) => last_id == &track.id && now.duration_since(*time).as_millis() < 500,
                    None => false,
                };

                self.last_click = Some((track.id.clone(), now));
                self.context_menu.dismiss();

                if is_double_click {
                    self.selection.select_single(track.id.clone(), index);
                    let context_tracks = self.context_tracks(store);
                    let start_idx = context_tracks.iter().position(|t| t.id == track.id).unwrap_or(0);
                    return (Task::none(), PlaylistsViewOutMessage::RequestPlayContext(context_tracks, start_idx));
                }

                if self.current_modifiers.shift() {
                    let visible_ids: Vec<&String> = self.filtered_indices.iter()
                        .filter_map(|&idx| self.current_playlist_tracks(store).get(idx).map(|t| &t.id))
                        .collect();
                    self.selection.select_range(index, &visible_ids);
                } else if self.current_modifiers.command() || self.current_modifiers.control() {
                    self.selection.toggle(track.id, index);
                } else {
                    self.selection.select_single(track.id, index);
                }

                (Task::none(), PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::ViewportMouseMoved(point) => {
                self.hovered_point = Some(point);
                self.context_menu.note_mouse_position(point);

                if let Some(start_pos) = self.pending_drag_start_point {
                    if self.drag_state.is_none() {
                        let dist = (point.y - start_pos.y).abs() + (point.x - start_pos.x).abs();
                        // 5 píxeles de holgura para no iniciar drag por error si te tiembla el click
                        if dist > 5.0 {
                            if self.sort.column() == SortColumn::DefaultOrder && self.search_query.is_empty() {
                                let absolute_y = start_pos.y + self.scroll.offset_y;
                                let source_idx = (absolute_y / ROW_HEIGHT).floor() as isize;
                                let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
                                let clamped = source_idx.clamp(0, max_index) as usize;

                                // Validamos que hayamos clickeado en una zona con pistas reales, no en el padding
                                if (clamped as f32 * ROW_HEIGHT) <= absolute_y && absolute_y <= ((clamped + 1) as f32 * ROW_HEIGHT) {
                                    let row_top = clamped as f32 * ROW_HEIGHT;
                                    self.drag_state = Some(DragState {
                                        source_index: clamped,
                                        current_index: clamped,
                                        grab_offset: absolute_y - row_top,
                                    });
                                }
                            }
                            self.pending_drag_start_point = None;
                        }
                    }
                }

                if let Some(drag) = &mut self.drag_state {
                    let absolute_y = point.y + self.scroll.offset_y;
                    let hovered_index = (absolute_y / ROW_HEIGHT).floor() as isize;
                    let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
                    drag.current_index = hovered_index.clamp(0, max_index) as usize;
                }

                (Task::none(), PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::ViewportMouseExited => {
                self.hovered_point = None;
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::AutoScrollTick => {
                if self.drag_state.is_none() {
                    return (Task::none(), PlaylistsViewOutMessage::Idle);
                }
                let Some(point) = self.hovered_point else {
                    return (Task::none(), PlaylistsViewOutMessage::Idle);
                };

                let Some(delta_y) = self.scroll.autoscroll_delta(
                    point.y,
                    AUTOSCROLL_ZONE_PX,
                    AUTOSCROLL_MAX_SPEED_PX,
                ) else {
                    return (Task::none(), PlaylistsViewOutMessage::Idle);
                };

                // Actualizamos nuestro propio offset optimistamente para que
                // el cálculo de current_index (que depende de scroll.offset_y)
                // no se quede un frame atrás del scroll real; on_scroll lo
                // corrige de todos modos en el próximo evento de scrollable.
                self.scroll.offset_y = (self.scroll.offset_y + delta_y).max(0.0);

                if let Some(drag) = &mut self.drag_state {
                    let absolute_y = point.y + self.scroll.offset_y;
                    let hovered_index = (absolute_y / ROW_HEIGHT).floor() as isize;
                    let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
                    drag.current_index = hovered_index.clamp(0, max_index) as usize;
                }

                let task = scroll_by(
                    Id::new(SCROLLABLE_ID),
                    AbsoluteOffset { x: 0.0, y: delta_y },
                );
                (task, PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::GlobalMousePress => {
                if let Some(point) = self.hovered_point {
                    // Si el clic cayó sobre la franja del scrollbar, lo ignoramos:
                    // de lo contrario un drag del scrollbar se confunde con un
                    // intento de reordenar filas.
                    if self.scroll.is_within_content(point.x) {
                        self.pending_drag_start_point = Some(point);
                    }
                }
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }

            PlaylistsViewMessage::GlobalMouseRelease => {
                self.pending_drag_start_point = None;
                let out = if let Some(drag) = self.drag_state.take() {
                    if drag.source_index != drag.current_index {
                        if let PlaylistsSubView::Detail(playlist_id) = &self.current_subview {
                            PlaylistsViewOutMessage::RequestReorder(
                                playlist_id.clone(),
                                drag.source_index,
                                drag.current_index,
                            )
                        } else {
                            PlaylistsViewOutMessage::Idle
                        }
                    } else {
                        PlaylistsViewOutMessage::Idle
                    }
                } else {
                    PlaylistsViewOutMessage::Idle
                };
                (Task::none(), out)
            }

            PlaylistsViewMessage::RowRightClicked(track_id) => {
                if !self.selection.is_selected(&track_id) {
                    let idx = self.filtered_indices.iter()
                        .position(|&i| self.current_playlist_tracks(store).get(i).map(|t| &t.id) == Some(&track_id))
                        .unwrap_or(0);
                    self.selection.select_single(track_id.clone(), idx);
                }

                self.context_menu.toggle(track_id, CONTEXT_MENU_ITEM_COUNT);
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::DismissContextMenu => {
                self.context_menu.dismiss();
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::ContextMenuSubmenuHover(id) => {
                self.context_menu.set_open_submenu(id);
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::ContextMenuAction(action, anchor_track) => {
                self.context_menu.dismiss();

                let selected = self.get_selected_tracks(store);

                if action == ContextMenuAction::CopyId {
                    let ids: Vec<String> = selected.iter().map(|t| t.id.clone()).collect();
                    return (clipboard::write(ids.join(", ")), PlaylistsViewOutMessage::Idle);
                }

                let out = match action {
                    ContextMenuAction::PlayNow => {
                        let context_tracks = self.context_tracks(store);
                        let anchor_id = selected.first().map(|t| t.id.clone()).unwrap_or(anchor_track.id.clone());
                        let start_idx = context_tracks.iter().position(|t| t.id == anchor_id).unwrap_or(0);
                        PlaylistsViewOutMessage::RequestPlayContext(context_tracks, start_idx)
                    },
                    ContextMenuAction::AddToQueue => PlaylistsViewOutMessage::RequestEnqueue(selected),
                    ContextMenuAction::AddToFrontQueue => PlaylistsViewOutMessage::RequestFrontEnqueue(selected),
                    ContextMenuAction::StartRadio => PlaylistsViewOutMessage::RequestPlayRadio(anchor_track),
                    ContextMenuAction::ToggleLike => PlaylistsViewOutMessage::RequestToggleLike(selected.into_iter().map(|t| t.id).collect()),
                    ContextMenuAction::AddToPlaylist(playlist_id) => {
                        PlaylistsViewOutMessage::RequestAddToPlaylist(playlist_id, selected.into_iter().map(|t| t.id).collect())
                    }
                    ContextMenuAction::CopyId => unreachable!(),
                    ContextMenuAction::RemoveFromPlaylist => {
                        if let PlaylistsSubView::Detail(playlist_id) = &self.current_subview {
                            PlaylistsViewOutMessage::RequestRemoveFromPlaylist(
                                playlist_id.clone(),
                                selected.into_iter().map(|t| t.id).collect(),
                            )
                        } else {
                            PlaylistsViewOutMessage::Idle
                        }
                    }
                };
                (Task::none(), out)
            }
            PlaylistsViewMessage::ColorThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_color(key.clone(), bytes);
                }
                let next = thumbnails.on_color_finished(&key, |k, b, e| {
                    PlaylistsViewMessage::ColorThumbnailResult(k, b, e)
                });
                (next, PlaylistsViewOutMessage::Idle)
            }
        }
    }

    // ── VISTAS (Overview y Detail) ───────────────────────────────────────────

    pub fn view<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, PlaylistsViewMessage> {
        match &self.current_subview {
            PlaylistsSubView::Overview => self.render_overview(store),
            PlaylistsSubView::Detail(id) => self.render_detail(id, store, thumbnails),
        }
    }

    fn render_overview<'a>(&self, store: &'a CatalogStore) -> Element<'a, PlaylistsViewMessage> {
        let title = text("Tus Playlists")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let new_btn = button("Crear Playlist")
            .on_press(PlaylistsViewMessage::CreatePlaylistRequested)
            .padding(10);

        let header = row![title, space().width(Length::Fill), new_btn]
            .align_y(Alignment::Center)
            .padding(20);

        let mut list = column![].spacing(10).padding(20);

        for (id, name, _) in store.playlists_metadata() {
            let tracks = store.tracks_for_playlist(id);
            let count = tracks.len();

            let btn = button(
                row![
                    text(name).font(SF_PRO).size(16),
                    space().width(Length::Fill),
                    text(format!("{} pistas", count)).font(SF_PRO).size(14).color(Color::from_rgb(0.6, 0.6, 0.6))
                ]
                    .align_y(Alignment::Center)
                    .padding(15)
            )
                .style(transparent_button)
                .width(Length::Fill)
                .on_press(PlaylistsViewMessage::SelectPlaylist(id.clone()));

            list = list.push(container(btn).style(|_theme: &iced::Theme| {
                container::Style {
                    background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
                    border: iced::border::rounded(8),
                    ..Default::default()
                }
            }));
        }

        column![header, scrollable(list)].width(Length::Fill).height(Length::Fill).into()
    }

    fn fields<'a>() -> FieldList<'a, PlaylistsViewMessage> {
        Field::index_sortable(40.0, SORT_KEY_DEFAULT_ORDER)
            .thumbnail(THUMBNAIL_SIZE + 2.0, THUMBNAIL_SIZE)
            .title(SORT_KEY_TITLE)
            .artist(SORT_KEY_ARTIST)
            .album(SORT_KEY_ALBUM)
            .duration(SORT_KEY_DURATION)
            .bpm(SORT_KEY_BPM)
            .camelot_key(SORT_KEY_KEY)
    }

    fn render_detail<'a>(
        &'a self,
        id: &str,
        store: &'a CatalogStore,
        thumbnails: &'a ThumbnailCache,
    ) -> Element<'a, PlaylistsViewMessage> {
        let meta = store.playlists_metadata().iter().find(|(pid, _, _)| pid == id);
        let name = meta.map(|(_, n, _)| n.as_str()).unwrap_or("Playlist Desconocida");

        let all_playlist_tracks = store.tracks_for_playlist(id);
        let total_duration: i64 = all_playlist_tracks.iter().map(|t| t.duration_seconds as i64).sum();

        let cover_handle = all_playlist_tracks.first().and_then(|t| {
            thumbnails.peek_for_render(t)
        });

        let header = playlist_header(
            PlaylistHeaderData {
                name,
                kicker: Some("PLAYLIST"),
                track_count: all_playlist_tracks.len(),
                total_duration_seconds: total_duration,
            },
            cover_handle,
            PlaylistsViewMessage::PlayPlaylist(id.to_string()),
        );

        let search_bar = catalog_search_input(
            "Buscar en esta playlist...",
            &self.search_query,
            PlaylistsViewMessage::SearchChanged,
        );

        let back_btn = button("← Volver")
            .on_press(PlaylistsViewMessage::BackToOverview)
            .style(transparent_button);

        let toolbar = row![back_btn, space().width(20), search_bar]
            .align_y(Alignment::Center)
            .padding(iced::Padding { top: 0.0, bottom: 12.0, left: 16.0, right: 16.0 });

        let body_content: Element<'_, PlaylistsViewMessage> = if all_playlist_tracks.is_empty() {
            catalog_status_message("Esta playlist está vacía.", StatusTone::Muted)
        } else if self.visible_count() == 0 {
            catalog_status_message("No se encontraron pistas que coincidan con tu búsqueda.", StatusTone::Muted)
        } else {
            let mut tracks = self.visible_tracks(store);
            let fields = Self::fields();

            // Mientras se arrastra una fila, reordenamos el vector que se
            // renderiza (no el estado real) para que las filas se corran
            // visualmente y quede claro dónde caería la pista al soltar.
            // El reorder real solo ocurre al soltar (GlobalMouseRelease).
            if let Some(drag) = &self.drag_state {
                if drag.source_index < tracks.len() && drag.source_index != drag.current_index {
                    let moved = tracks.remove(drag.source_index);
                    let insert_at = drag.current_index.min(tracks.len());
                    tracks.insert(insert_at, moved);
                }
            }

            let config = TrackListConfig {
                columns: fields.columns(),
                active_sort_key: self.sort.active_sort_key(),
                sort_direction_asc: self.sort.is_asc(),
                row_height: ROW_HEIGHT,
                buffer_rows: BUFFER_ROWS,
                dragging_row_index: self.drag_state.as_ref().map(|d| d.current_index),
            };

            let overlay = if let Some(drag) = &self.drag_state {
                if let Some(track) = self.track_at(store, drag.source_index) {
                    let handle = thumbnails.peek_for_render(track);

                    // 1. Reconstruimos la fila COMPLETA usando la misma definición de las columnas
                    let mut row_children: Vec<Element<'_, PlaylistsViewMessage>> = Vec::with_capacity(fields.columns().len());
                    let cells = fields.row_cells(track, drag.source_index + 1);

                    for (col, cell) in fields.columns().iter().zip(cells.into_iter()) {
                        let width = match col {
                            Column::Index { width, .. } => Length::Fixed(*width),
                            Column::Thumbnail { width, .. } => Length::Fixed(*width),
                            Column::Sortable { width, .. } => *width,
                        };

                        let element: Element<'_, PlaylistsViewMessage> = match cell {
                            Cell::Index(i) => container(text(i.to_string()).font(SF_PRO).size(12).color(Color::from_rgb(0.45, 0.45, 0.5)))
                                .width(width).into(),
                            Cell::Thumbnail(_) => container(crate::ui::widgets::track_row::track_thumbnail_sized(handle.clone(), 44.0))
                                .width(width).align_y(Alignment::Center).into(),
                            Cell::Text(s) => container(text(s).font(SF_PRO).size(13.5).color(Color::from_rgb(0.7, 0.7, 0.75)))
                                .width(width).into(),
                            Cell::ColoredText(s, c) => container(text(s).font(SF_PRO).size(13.5).color(c))
                                .width(width).into(),
                            Cell::Custom(el) => container(el).width(width).into(),
                        };
                        row_children.push(element);
                    }

                    let ghost_row = row(row_children)
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .padding(iced::Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 16.0 });

                    let ghost_content = container(ghost_row)
                        .width(Length::Fill)
                        .height(Length::Fixed(ROW_HEIGHT))
                        .style(|_theme: &iced::Theme| {
                            container::Style {
                                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                                border: iced::border::rounded(6)
                                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.15))
                                    .width(1.0),
                                ..Default::default()
                            }
                        });

                    let raw_mouse_y = self.hovered_point.map(|p| p.y).unwrap_or_default();
                    let y_pos = (raw_mouse_y - drag.grab_offset).max(0.0);

                    Some(
                        container(ghost_content)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .padding(iced::Padding {
                                top: y_pos,
                                bottom: 0.0,
                                left: 0.0,  // Bloqueado a la izquierda para que ocupe todo el ancho real
                                right: 0.0, // Bloqueado a la derecha
                            })
                            .into()
                    )
                } else {
                    None
                }
            } else {
                self.context_menu.render_target(|t_id| store.track_by_id(t_id)).map(|(anchor, track)| {
                    let items = track_context_menu::build(
                        track,
                        store,
                        LikeSlot::Toggle(ContextMenuAction::ToggleLike),
                        ContextMenuAction::AddToPlaylist,
                        Some(id),
                        SUBMENU_ADD_TO_PLAYLIST,
                        ContextMenuAction::PlayNow,
                        ContextMenuAction::AddToQueue,
                        ContextMenuAction::AddToFrontQueue,
                        ContextMenuAction::StartRadio,
                        ContextMenuAction::CopyId,
                        ContextMenuItem::new("Quitar de playlist", ContextMenuAction::RemoveFromPlaylist).icon(Icon::Delete),
                    );

                    self.context_menu.view(
                        anchor,
                        items,
                        track,
                        PlaylistsViewMessage::ContextMenuAction,
                        PlaylistsViewMessage::DismissContextMenu,
                        PlaylistsViewMessage::ContextMenuSubmenuHover,
                    )
                })
            };

            track_list(
                config,
                &tracks,
                &self.scroll,
                thumbnails,
                &self.selection.selected_ids,
                SCROLLABLE_ID,
                TrackListCallbacks::new(
                    PlaylistsViewMessage::Scrolled,
                    PlaylistsViewMessage::RowClicked,
                    PlaylistsViewMessage::SortByKey,
                    PlaylistsViewMessage::ViewportMouseMoved,
                    PlaylistsViewMessage::RowRightClicked,
                )
                    .with_exit(PlaylistsViewMessage::ViewportMouseExited),
                |track, idx| fields.row_cells(track, idx),
                overlay,
            )
        };

        column![
            header,
            space().height(16),
            toolbar,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}