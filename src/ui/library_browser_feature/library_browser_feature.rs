use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::space;
use iced::{Element, Task};

use crate::audio::manager::manager::{PlaybackOrigin, TrackManager};
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::{Mix, Track};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::album_view::{AlbumMessage, AlbumOutMessage, AlbumView};
use crate::ui::views::artist_view::{ArtistMessage, ArtistOutMessage, ArtistView};
use crate::ui::views::mix_view::{MixMessage, MixOutMessage, MixView};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::selection_state::SelectionStep;
use crate::ui::widgets::track_context_builder::{youtube_link, TrackContextAction, TrackContextMenuBuilder, TrackTool};

enum LibraryBrowserRoute {
    Artist(ArtistView),
    Album(AlbumView),
    Mix(MixView),
}

pub struct LibraryBrowserFeature {
    active: Option<LibraryBrowserRoute>,
    client: MicroserviceClient,
    manager: Arc<TrackManager>,
    followed_artist_manager: Arc<FollowedArtistManager>,
    context_menu: ContextMenu<String>,
    context_menu_items: Vec<ContextMenuItem<TrackContextAction>>,
    /// Scroll de artistas/álbumes ya visitados en esta sesión, por id —
    /// para restaurarlo si se vuelve a abrir el mismo artista/álbum.
    artist_scroll_cache: HashMap<String, ScrollTracker>,
    album_scroll_cache: HashMap<String, ScrollTracker>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryBrowserLocation {
    Artist(String),
    Album(String),
    Mix(Mix),
}

/// `Task<T>` genérico: estas operaciones de scroll no producen ningún
/// mensaje real, así que `T` se infiere del contexto donde se use
/// (batch/return) sin necesitar `.map(...)`.
fn scroll_to_offset<T>(scrollable_id: &'static str, offset_y: f32) -> Task<T> {
    iced::widget::operation::scroll_to(
        iced::widget::Id::new(scrollable_id),
        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset_y },
    )
}

#[derive(Debug, Clone)]
pub enum LibraryBrowserMessage {
    Artist(ArtistMessage),
    Album(AlbumMessage),
    Mix(MixMessage),
    OpenArtist(String),
    OpenAlbum(String),
    OpenMix(Mix),
    ContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),
}

#[derive(Debug, Clone)]
pub enum LibraryBrowserOutMessage {
    Idle,
    RequestToggleLike(Vec<Track>),
    RequestAddToPlaylist { playlist_id: String, tracks: Vec<Track> },
    RequestToggleFollowArtist(String, String, Option<String>),
    RequestTrackTool(TrackTool, String),
}

impl LibraryBrowserFeature {
    pub fn new(client: MicroserviceClient, manager: Arc<TrackManager>, followed_artist_manager: Arc<FollowedArtistManager>) -> Self {
        Self {
            active: None,
            client,
            manager,
            followed_artist_manager,
            context_menu: ContextMenu::new(),
            context_menu_items: Vec::new(),
            artist_scroll_cache: HashMap::new(),
            album_scroll_cache: HashMap::new(),
        }
    }

    /// Si hay una vista de artista/álbum abierta.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Cierra cualquier vista abierta.
    pub fn close(&mut self) {
        self.stash_active_route_scroll();
        self.active = None;
    }

    pub fn current_location(&self) -> Option<LibraryBrowserLocation> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => Some(LibraryBrowserLocation::Artist(view.artist_id().to_string())),
            Some(LibraryBrowserRoute::Album(view)) => Some(LibraryBrowserLocation::Album(view.album_id().to_string())),
            Some(LibraryBrowserRoute::Mix(view)) => Some(LibraryBrowserLocation::Mix(view.mix().clone())),
            None => None,
        }
    }

    /// Guarda el scroll de la ruta activa (artista o álbum) en el cache
    /// correspondiente, por id, antes de que `active` se reemplace o se
    /// descarte — para poder restaurarlo si se vuelve a visitar el mismo
    /// artista/álbum más adelante, incluso tras varios saltos que ya
    /// pisaron `previous` (que solo cubre un nivel de historial).
    fn stash_active_route_scroll(&mut self) {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => {
                self.artist_scroll_cache.insert(view.artist_id().to_string(), view.scroll);
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                self.album_scroll_cache.insert(view.album_id().to_string(), view.scroll);
            }
            Some(LibraryBrowserRoute::Mix(view)) => {
                self.album_scroll_cache.insert(view.mix().id.clone(), view.list.scroll);
            }
            None => {}
        }
    }

    /// Busca un track por id en la ruta actualmente activa (canciones del artista o tracks del álbum).
    fn find_track<'a>(&'a self, id: &str, catalog_store: &'a CatalogStore) -> Option<&'a Track> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.find_song(id, catalog_store),
            Some(LibraryBrowserRoute::Album(view)) => view.find_track(id),
            Some(LibraryBrowserRoute::Mix(view)) => view.find_track(id),
            None => None,
        }
    }

    /// Ctrl/Shift sostenidos, para la selección múltiple con el mouse.
    pub fn set_modifiers(&mut self, modifiers: iced::keyboard::Modifiers) {
        match &mut self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.set_modifiers(modifiers),
            Some(LibraryBrowserRoute::Album(view)) => view.set_modifiers(modifiers),
            Some(LibraryBrowserRoute::Mix(view)) => view.list.keybinds_press = modifiers,
            None => {}
        }
    }

    /// Canciones a las que aplica una acción del menú sobre `anchor_id` (la selección, si lo incluye).
    fn selected_or(&self, anchor_id: &str, catalog_store: &CatalogStore) -> Vec<Track> {
        let tracks = match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.selected_or(anchor_id, catalog_store),
            Some(LibraryBrowserRoute::Album(view)) => view.selected_or(anchor_id),
            Some(LibraryBrowserRoute::Mix(view)) => view.selected_or(anchor_id),
            None => Vec::new(),
        };
        tracks.into_iter().cloned().collect()
    }

    /// Canciones bajo el mouse en la vista abierta, para arrastrarlas a una playlist.
    pub fn drag_candidate(&self, catalog_store: &CatalogStore) -> Option<Vec<Track>> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.drag_candidate(catalog_store).map(|tracks| tracks.into_iter().cloned().collect()),
            Some(LibraryBrowserRoute::Album(view)) => view.drag_candidate().map(|tracks| tracks.into_iter().cloned().collect()),
            Some(LibraryBrowserRoute::Mix(view)) => view.drag_candidate(),
            None => None,
        }
    }

    /// Mueve la selección de la lista abierta (flechas, RePág/AvPág).
    pub fn move_selection(&mut self, step: SelectionStep, extend: bool, catalog_store: &CatalogStore) -> Task<LibraryBrowserMessage> {
        match &mut self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.move_selection(step, extend, catalog_store).map(LibraryBrowserMessage::Artist),
            Some(LibraryBrowserRoute::Album(view)) => view.move_selection(step, extend).map(LibraryBrowserMessage::Album),
            Some(LibraryBrowserRoute::Mix(view)) => view.move_selection(step).map(LibraryBrowserMessage::Mix),
            None => Task::none(),
        }
    }

    /// Reproduce la lista abierta desde la canción seleccionada (Enter).
    pub fn play_selection(&self, catalog_store: &CatalogStore) {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => {
                if let Some(id) = view.selected_song_id(catalog_store) {
                    self.play_artist_songs(view, id, catalog_store);
                }
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                if let Some(id) = view.selected_track_id() {
                    self.play_album_tracks(view, id);
                }
            }
            Some(LibraryBrowserRoute::Mix(view)) => {
                if let Some(id) = view.selected_track_id() {
                    self.play_mix_tracks(view, id);
                }
            }
            None => {}
        }
    }

    /// Reproduce las canciones visibles del artista empezando por `start_id`.
    fn play_artist_songs(&self, view: &ArtistView, start_id: &str, catalog_store: &CatalogStore) {
        let songs = view.shown_songs(catalog_store);
        if let Some(index) = songs.iter().position(|t| t.id == start_id) {
            self.manager.set_playback_origin(PlaybackOrigin::Artist(view.artist_id().to_string()));
            self.manager.play_context(songs, index);
        }
    }

    /// Reproduce el álbum empezando por `start_id`.
    fn play_album_tracks(&self, view: &AlbumView, start_id: &str) {
        let tracks = view.tracks();
        if let Some(index) = tracks.iter().position(|t| t.id == start_id) {
            self.manager.set_playback_origin(PlaybackOrigin::Album(view.album_id().to_string()));
            self.manager.play_context(tracks.to_vec(), index);
        }
    }

    /// Reproduce la mezcla (en el orden de la tabla) empezando por `start_id`.
    fn play_mix_tracks(&self, view: &MixView, start_id: &str) {
        let tracks = view.tracks_in_order();
        if let Some(index) = tracks.iter().position(|t| t.id == start_id) {
            self.manager.set_playback_origin(PlaybackOrigin::Mix(view.mix().id.clone()));
            self.manager.play_context(tracks, index);
        }
    }

    /// Reemplaza en el lugar, en la ruta activa (si la hay y contiene ese
    /// track), el `Track` recién descargado/analizado — para que Álbum/
    /// Artista dejen de mostrar/reproducir el stub congelado que trajo el
    /// fetch inicial de la vista. Se llama desde `App` en el mismo punto
    /// donde ya se empuja el track a `CatalogStore` (ver `main.rs`).
    pub fn patch_track(&mut self, track: &Track) {
        match &mut self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.patch_track(track),
            Some(LibraryBrowserRoute::Album(view)) => view.patch_track(track),
            Some(LibraryBrowserRoute::Mix(view)) => view.patch_track(track),
            None => {}
        }
    }

    /// Ver `ViewCoordinator::set_cursor`.
    pub fn set_cursor(&mut self, position: iced::Point) {
        self.context_menu.handle(ContextMenuEvent::MouseMoved(position));
    }

    /// Tamaño de la ventana, para encajar el menú contextual.
    pub fn set_viewport(&mut self, size: iced::Size) {
        self.context_menu.handle(ContextMenuEvent::ViewportResized(size));
    }

    pub fn update(
        &mut self,
        message: LibraryBrowserMessage,
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<LibraryBrowserMessage>, LibraryBrowserOutMessage) {
        match message {
            LibraryBrowserMessage::OpenArtist(id) => {
                self.stash_active_route_scroll();
                let (mut view, task) = ArtistView::new(self.client.clone(), id.clone(), Arc::clone(&self.followed_artist_manager));
                let mut out_task = task.map(LibraryBrowserMessage::Artist);
                if let Some(scroll) = self.artist_scroll_cache.remove(&id) {
                    view.scroll = scroll;
                    out_task = Task::batch([out_task, scroll_to_offset("artist_view_scroll", scroll.offset_y)]);
                }
                self.active = Some(LibraryBrowserRoute::Artist(view));
                (out_task, LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::OpenAlbum(id) => {
                self.stash_active_route_scroll();
                let (mut view, task) = AlbumView::new(self.client.clone(), id.clone());
                let mut out_task = task.map(LibraryBrowserMessage::Album);
                if let Some(scroll) = self.album_scroll_cache.remove(&id) {
                    view.scroll = scroll;
                    out_task = Task::batch([out_task, scroll_to_offset("album_view_scroll", scroll.offset_y)]);
                }
                self.active = Some(LibraryBrowserRoute::Album(view));
                (out_task, LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::OpenMix(mix) => {
                self.stash_active_route_scroll();
                let id = mix.id.clone();
                let (mut view, task) = MixView::new(mix);
                let mut out_task = task.map(LibraryBrowserMessage::Mix);
                if let Some(scroll) = self.album_scroll_cache.remove(&id) {
                    view.list.scroll = scroll;
                    out_task = Task::batch([out_task, scroll_to_offset(MixView::scroll_id(), scroll.offset_y)]);
                }
                self.active = Some(LibraryBrowserRoute::Mix(view));
                (out_task, LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::Mix(msg) => match &mut self.active {
                Some(LibraryBrowserRoute::Mix(view)) => {
                    let (task, out) = view.update(msg);
                    let task = task.map(LibraryBrowserMessage::Mix);

                    let follow_up = match out {
                        MixOutMessage::Idle => Task::none(),
                        MixOutMessage::PlayTrack(id) => {
                            let tracks = view.tracks_in_order();
                            if let Some(index) = tracks.iter().position(|t| t.id == id) {
                                self.manager.set_playback_origin(PlaybackOrigin::Mix(view.mix().id.clone()));
                                self.manager.play_context(tracks, index);
                            }
                            Task::none()
                        }
                        MixOutMessage::PlayAll => {
                            self.manager.set_playback_origin(PlaybackOrigin::Mix(view.mix().id.clone()));
                            self.manager.play_context_shuffled(view.tracks_in_order());
                            Task::none()
                        }
                        MixOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_track(&id) {
                                let member_of = catalog_store.playlists_containing_track(&id);
                                let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                    .with_playlists(playlists, None, &member_of)
                                    .with_tools(track.file_path.is_some())
                                    .build();
                                self.context_menu.handle(ContextMenuEvent::RightClicked(id));
                                self.context_menu_items = items;
                            }
                            Task::none()
                        }
                        MixOutMessage::OpenArtist(id) => Task::done(LibraryBrowserMessage::OpenArtist(id)),
                        MixOutMessage::OpenAlbum(id) => Task::done(LibraryBrowserMessage::OpenAlbum(id)),
                        MixOutMessage::ToggleShuffle => {
                            self.manager.toggle_shuffle();
                            Task::none()
                        }
                        MixOutMessage::RequestTogglePlayback => {
                            if self.manager.state.is_playing() { self.manager.pause(); } else { self.manager.resume(); }
                            Task::none()
                        }
                    };

                    (Task::batch([task, follow_up]), LibraryBrowserOutMessage::Idle)
                }
                _ => (Task::none(), LibraryBrowserOutMessage::Idle),
            },

            LibraryBrowserMessage::ContextMenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.context_menu_items.clear();
                }
                self.context_menu.handle(event);
                (Task::none(), LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::TrackContextAction(action, track_id) => {
                self.context_menu.handle(ContextMenuEvent::Dismissed);
                self.context_menu_items.clear();

                let Some(track) = self.find_track(&track_id, catalog_store) else {
                    return (Task::none(), LibraryBrowserOutMessage::Idle);
                };
                // Reproducir, encolar, me gusta y agregar a playlist van a toda la selección.
                let tracks = self.selected_or(&track_id, catalog_store);

                match action {
                    TrackContextAction::PlayNow => {
                        self.manager.play_context(tracks, 0);
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::Enqueue => {
                        self.manager.enqueue_many(tracks);
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::StartRadio => {
                        self.manager.start_radio(track.clone());
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::FrontEnqueue => {
                        self.manager.enqueue_front_many(tracks);
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::CopyId => {
                        (iced::clipboard::write(track_id), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::CopyYoutubeLink => {
                        (iced::clipboard::write(youtube_link(&track_id)), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::Tool(tool) => {
                        (Task::none(), LibraryBrowserOutMessage::RequestTrackTool(tool, track_id))
                    }
                    TrackContextAction::ToggleLike => {
                        (Task::none(), LibraryBrowserOutMessage::RequestToggleLike(tracks))
                    }
                    TrackContextAction::AddToPlaylist(playlist_id) => {
                        (Task::none(), LibraryBrowserOutMessage::RequestAddToPlaylist { playlist_id, tracks })
                    }
                    TrackContextAction::DeleteFromCatalog | TrackContextAction::RemoveFromPlaylist => {
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                }
            }

            LibraryBrowserMessage::Artist(msg) => match &mut self.active {
                Some(LibraryBrowserRoute::Artist(view)) => {
                    let (task, out) = view.update(msg, catalog_store);
                    let task = task.map(LibraryBrowserMessage::Artist);

                    match out {
                        ArtistOutMessage::OpenAlbum(id) => {
                            self.stash_active_route_scroll();
                            let (mut album_view, album_task) = AlbumView::new(self.client.clone(), id.clone());
                            let mut out_task = Task::batch([task, album_task.map(LibraryBrowserMessage::Album)]);
                            if let Some(scroll) = self.album_scroll_cache.remove(&id) {
                                album_view.scroll = scroll;
                                out_task = Task::batch([out_task, scroll_to_offset("album_view_scroll", scroll.offset_y)]);
                            }
                            self.active = Some(LibraryBrowserRoute::Album(album_view));
                            (out_task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::PlaySong(id) => {
                            let songs = view.shown_songs(catalog_store);
                            if let Some(index) = songs.iter().position(|t| t.id == id) {
                                self.manager.set_playback_origin(PlaybackOrigin::Artist(view.artist_id().to_string()));
                                self.manager.play_context(songs, index);
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_song(&id, catalog_store) {
                                let member_of = catalog_store.playlists_containing_track(&id);
                                let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                    .with_playlists(playlists, None, &member_of)
                                    .with_tools(track.file_path.is_some())
                                    .build();
                                self.context_menu.handle(ContextMenuEvent::RightClicked(id));
                                self.context_menu_items = items;
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::OpenTrackArtist(id) => {
                            self.stash_active_route_scroll();
                            let (mut artist_view, artist_task) = ArtistView::new(self.client.clone(), id.clone(), Arc::clone(&self.followed_artist_manager));
                            let mut out_task = Task::batch([task, artist_task.map(LibraryBrowserMessage::Artist)]);
                            if let Some(scroll) = self.artist_scroll_cache.remove(&id) {
                                artist_view.scroll = scroll;
                                out_task = Task::batch([out_task, scroll_to_offset("artist_view_scroll", scroll.offset_y)]);
                            }
                            self.active = Some(LibraryBrowserRoute::Artist(artist_view));
                            (out_task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::Idle => (task, LibraryBrowserOutMessage::Idle),
                        ArtistOutMessage::ToggleFollow(id, name, photo) => {
                            (task, LibraryBrowserOutMessage::RequestToggleFollowArtist(id, name, photo))
                        }
                        ArtistOutMessage::RequestTogglePlayback => {
                            if self.manager.state.is_playing() { self.manager.pause(); } else { self.manager.resume(); }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                    }
                }
                _ => (Task::none(), LibraryBrowserOutMessage::Idle),
            },

            LibraryBrowserMessage::Album(msg) => match &mut self.active {
                Some(LibraryBrowserRoute::Album(view)) => {
                    let (task, out) = view.update(msg);
                    let task = task.map(LibraryBrowserMessage::Album);

                    match out {
                        AlbumOutMessage::OpenArtist(id) => {
                            self.stash_active_route_scroll();
                            let (mut artist_view, artist_task) = ArtistView::new(self.client.clone(), id.clone(), Arc::clone(&self.followed_artist_manager));
                            let mut out_task = Task::batch([task, artist_task.map(LibraryBrowserMessage::Artist)]);
                            if let Some(scroll) = self.artist_scroll_cache.remove(&id) {
                                artist_view.scroll = scroll;
                                out_task = Task::batch([out_task, scroll_to_offset("artist_view_scroll", scroll.offset_y)]);
                            }
                            self.active = Some(LibraryBrowserRoute::Artist(artist_view));
                            (out_task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::PlayTrack(id) => {
                            let tracks = view.tracks();
                            if let Some(index) = tracks.iter().position(|t| t.id == id) {
                                self.manager.set_playback_origin(PlaybackOrigin::Album(view.album_id().to_string()));
                                self.manager.play_context(tracks.to_vec(), index);
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::PlayAlbum => {
                            self.manager.set_playback_origin(PlaybackOrigin::Album(view.album_id().to_string()));
                            self.manager.play_context_shuffled(view.tracks().to_vec());
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_track(&id) {
                                let member_of = catalog_store.playlists_containing_track(&id);
                                let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                    .with_playlists(playlists, None, &member_of)
                                    .with_tools(track.file_path.is_some())
                                    .build();
                                self.context_menu.handle(ContextMenuEvent::RightClicked(id));
                                self.context_menu_items = items;
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::OpenTrackArtist(id) => {
                            self.stash_active_route_scroll();
                            let (mut artist_view, artist_task) = ArtistView::new(self.client.clone(), id.clone(), Arc::clone(&self.followed_artist_manager));
                            let mut out_task = Task::batch([task, artist_task.map(LibraryBrowserMessage::Artist)]);
                            if let Some(scroll) = self.artist_scroll_cache.remove(&id) {
                                artist_view.scroll = scroll;
                                out_task = Task::batch([out_task, scroll_to_offset("artist_view_scroll", scroll.offset_y)]);
                            }
                            self.active = Some(LibraryBrowserRoute::Artist(artist_view));
                            (out_task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::Idle => (task, LibraryBrowserOutMessage::Idle),
                        AlbumOutMessage::RequestTogglePlayback => {
                            if self.manager.state.is_playing() { self.manager.pause(); } else { self.manager.resume(); }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                    }
                }
                _ => (Task::none(), LibraryBrowserOutMessage::Idle),
            },
        }
    }

    pub fn view<'a>(&'a self, catalog_store: &'a CatalogStore) -> Element<'a, LibraryBrowserMessage> {
        let is_playing = self.manager.state.is_playing();

        let origin_matches_active_route = match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => {
                self.manager.get_playback_origin() == Some(PlaybackOrigin::Artist(view.artist_id().to_string()))
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                self.manager.get_playback_origin() == Some(PlaybackOrigin::Album(view.album_id().to_string()))
            }
            Some(LibraryBrowserRoute::Mix(view)) => {
                self.manager.get_playback_origin() == Some(PlaybackOrigin::Mix(view.mix().id.clone()))
            }
            None => false,
        };
        let now_playing_id = if origin_matches_active_route {
            self.manager.get_current_track().map(|t| t.track.id.clone())
        } else {
            None
        };

        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => {
                view.view(now_playing_id, is_playing, catalog_store).map(LibraryBrowserMessage::Artist)
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                view.view(now_playing_id, is_playing).map(LibraryBrowserMessage::Album)
            }
            Some(LibraryBrowserRoute::Mix(view)) => {
                view.view(now_playing_id, is_playing, self.manager.is_shuffled()).map(LibraryBrowserMessage::Mix)
            }
            None => space().into(),
        }
    }

    /// Overlay del menú contextual de track, si hay uno abierto.
    pub fn view_context_menu(&self) -> Option<Element<'_, LibraryBrowserMessage>> {
        let open_track_id = self.context_menu.open_id();
        let (anchor, track_id) = self.context_menu.render_target(|_| open_track_id)?;

        Some(self.context_menu.view(
            anchor,
            self.context_menu_items.clone(),
            track_id,
            LibraryBrowserMessage::TrackContextAction,
            LibraryBrowserMessage::ContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| LibraryBrowserMessage::ContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }
}
