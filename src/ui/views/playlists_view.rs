use std::collections::HashSet;

use iced::{Alignment, Color, Element, Font, Length, Task};
use iced::widget::scrollable::Viewport;
use iced::widget::{column, container, row, space, text, Id, button};
use iced::widget::operation::snap_to;
use iced::widget::scrollable;

use crate::model::Track;
use crate::ui::utils::search::SearchQuery;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuItem};
use crate::ui::widgets::playlist_header::{playlist_header, PlaylistHeaderData};
use crate::ui::widgets::track_list::{track_list, TrackListCallbacks, TrackListConfig};
use crate::ui::widgets::track_fields::{Field, FieldList};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::styles::styles::transparent_button;
use iced::clipboard;
use crate::ui::assets::icons::Icon;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;
const CONTEXT_MENU_ITEM_COUNT: usize = 8;
const SCROLLABLE_ID: &str = "playlists_catalog_scroll";
const SUBMENU_ADD_TO_PLAYLIST: usize = 0;

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

    // --- Mensajes de la tabla virtualizada (Detail) ---
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
pub enum PlaylistsViewOutMessage {
    Idle,
    RequestPlayContext(Vec<Track>, usize),
    RequestEnqueue(Track),
    RequestFrontEnqueue(Track),
    RequestPlayRadio(Track),
    RequestRemoveFromPlaylist(String, String), // playlist_id, track_id
    RequestToggleLike(String),
    RequestAddToPlaylist(String, String), // playlist_id, track_id
    CreatePlaylistRequested,
}

pub struct PlaylistsView {
    pub current_subview: PlaylistsSubView,

    // Estado efímero para la vista de detalle
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

impl Default for PlaylistsView {
    fn default() -> Self {
        Self {
            current_subview: PlaylistsSubView::Overview,
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
}

impl PlaylistsView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resetea el estado para evitar panics por índices desfasados o leaks de memoria
    /// visual al cambiar entre playlists con distintas longitudes.
    fn reset_detail_state(&mut self) {
        self.search_query.clear();
        self.filtered_indices.clear();
        self.sort_column = DEFAULT_SORT_COLUMN;
        self.sort_direction = DEFAULT_SORT_DIRECTION;
        self.sort_click_stage = 1;
        self.selected_track_id = None;
        self.context_menu.dismiss();
        self.scroll.reset();
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

    fn apply_search(&mut self, store: &CatalogStore) {
        let tracks = self.current_playlist_tracks(store);
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
                let is_match = query.matches_any(&[&track.title, &track.format_artists(), album_name]);
                is_match.then_some(idx)
            })
            .collect();
    }

    fn apply_sort(&mut self, store: &CatalogStore) {
        self.apply_search(store);

        let tracks = self.current_playlist_tracks(store);
        let asc = self.sort_direction == SortDirection::Asc;

        match self.sort_column {
            SortColumn::DefaultOrder => { /* SQLite native position already kept via index */ },
            SortColumn::Title => {
                self.filtered_indices.sort_by(|&a, &b| tracks[a].title.to_lowercase().cmp(&tracks[b].title.to_lowercase()));
            }
            SortColumn::Artist => {
                self.filtered_indices.sort_by(|&a, &b| tracks[a].format_artists().to_lowercase().cmp(&tracks[b].format_artists().to_lowercase()));
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
        let window = self.current_window();
        (window.start..window.end)
            .filter_map(|visible_idx| self.track_at(store, visible_idx))
            .map(thumb_key)
            .collect()
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
                // Reproduce desde el inicio, respetando filtros y orden actuales
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

            // --- Interacciones de tabla (Detail) ---
            PlaylistsViewMessage::SortByKey(key) => {
                let Some(column) = SortColumn::from_sort_key(key) else {
                    return (Task::none(), PlaylistsViewOutMessage::Idle);
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
                (thumb_task, PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::SearchChanged(query) => {
                self.search_query = query;
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
            PlaylistsViewMessage::PlayTrack(track) => {
                self.selected_track_id = Some(track.id.clone());
                self.context_menu.dismiss();
                let context_tracks = self.context_tracks(store);
                let start_idx = context_tracks.iter().position(|t| t.id == track.id).unwrap_or(0);
                (Task::none(), PlaylistsViewOutMessage::RequestPlayContext(context_tracks, start_idx))
            }
            PlaylistsViewMessage::RowSelected(track_id) => {
                self.selected_track_id = Some(track_id);
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::ViewportMouseMoved(point) => {
                self.context_menu.note_mouse_position(point);
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::RowRightClicked(track_id) => {
                self.context_menu.toggle(track_id.clone(), CONTEXT_MENU_ITEM_COUNT);
                self.selected_track_id = Some(track_id);
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
            PlaylistsViewMessage::ContextMenuAction(action, track) => {
                self.context_menu.dismiss();
                self.selected_track_id = Some(track.id.clone());

                if action == ContextMenuAction::CopyId {
                    return (clipboard::write(track.id.clone()), PlaylistsViewOutMessage::Idle);
                }

                let out = match action {
                    ContextMenuAction::PlayNow => {
                        let context_tracks = self.context_tracks(store);
                        let start_idx = context_tracks.iter().position(|t| t.id == track.id).unwrap_or(0);
                        PlaylistsViewOutMessage::RequestPlayContext(context_tracks, start_idx)
                    },
                    ContextMenuAction::AddToQueue => PlaylistsViewOutMessage::RequestEnqueue(track),
                    ContextMenuAction::AddToFrontQueue => PlaylistsViewOutMessage::RequestFrontEnqueue(track),
                    ContextMenuAction::StartRadio => PlaylistsViewOutMessage::RequestPlayRadio(track),
                    ContextMenuAction::ToggleLike => PlaylistsViewOutMessage::RequestToggleLike(track.id.clone()),
                    ContextMenuAction::AddToPlaylist(playlist_id) => {
                        PlaylistsViewOutMessage::RequestAddToPlaylist(playlist_id, track.id.clone())
                    }
                    ContextMenuAction::CopyId => unreachable!(),
                    ContextMenuAction::RemoveFromPlaylist => {
                        if let PlaylistsSubView::Detail(playlist_id) = &self.current_subview {
                            PlaylistsViewOutMessage::RequestRemoveFromPlaylist(playlist_id.clone(), track.id)
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

        // Extraer id y name ignorando el cover (el tercer elemento de la tupla)
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

    /// Antes: `columns()` + `row_cells()` por separado. Ver comentario
    /// equivalente en `explorer_view.rs`. Solo título/artista/álbum/
    /// duración (sin BPM/KEY/AGREGADO — no relevantes en una playlist).
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
        // Encontramos la playlist en la metadata para sacar su nombre
        let meta = store.playlists_metadata().iter().find(|(pid, _, _)| pid == id);
        let name = meta.map(|(_, n, _)| n.as_str()).unwrap_or("Playlist Desconocida");

        let all_playlist_tracks = store.tracks_for_playlist(id);
        let total_duration: i64 = all_playlist_tracks.iter().map(|t| t.duration_seconds as i64).sum();

        // Obtener cover (primer track) delegando la resolución del Handle al cache
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
        } else {
            let visible_tracks: Vec<&Track> = self.filtered_indices
                .iter()
                .filter_map(|&idx| all_playlist_tracks.get(idx).copied())
                .collect();

            let fields = Self::fields();

            let config = TrackListConfig {
                columns: fields.columns(),
                active_sort_key: {
                    let is_default = self.sort_column == DEFAULT_SORT_COLUMN && self.sort_direction == DEFAULT_SORT_DIRECTION && self.sort_click_stage == 1;
                    (!is_default).then(|| self.sort_column.sort_key())
                },
                sort_direction_asc: self.sort_direction == SortDirection::Asc,
                row_height: ROW_HEIGHT,
                buffer_rows: BUFFER_ROWS,
            };

            let overlay = self.context_menu.render_target(|t_id| store.track_by_id(t_id)).map(|(anchor, track)| {
                let (like_label, like_icon) = if track.liked {
                    ("Ya no me gusta", Icon::HeartBroken)
                } else {
                    ("Me gusta", Icon::HeartFull)
                };

                let playlist_children: Vec<ContextMenuItem<ContextMenuAction>> = store
                    .playlists_metadata()
                    .iter()
                    .filter(|(playlist_id, _, _)| playlist_id.as_str() != id)
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
                        ContextMenuItem::new("Quitar de playlist", ContextMenuAction::RemoveFromPlaylist).icon(Icon::Delete),
                    ],
                    track,
                    PlaylistsViewMessage::ContextMenuAction,
                    PlaylistsViewMessage::DismissContextMenu,
                    PlaylistsViewMessage::ContextMenuSubmenuHover,
                )
            });

            track_list(
                config,
                &visible_tracks,
                &self.scroll,
                thumbnails,
                self.selected_track_id.as_deref(),
                SCROLLABLE_ID,
                TrackListCallbacks::new(
                    PlaylistsViewMessage::Scrolled,
                    PlaylistsViewMessage::PlayTrack,
                    PlaylistsViewMessage::SortByKey,
                    PlaylistsViewMessage::ViewportMouseMoved,
                    PlaylistsViewMessage::RowRightClicked,
                ),
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