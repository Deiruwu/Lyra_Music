use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::space;
use iced::{Element, Task};

use crate::audio::manager::manager::{PlaybackOrigin, TrackManager};
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::album_view::{AlbumMessage, AlbumOutMessage, AlbumView};
use crate::ui::views::artist_view::{ArtistMessage, ArtistOutMessage, ArtistView};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::track_context_builder::{TrackContextAction, TrackContextMenuBuilder};

enum LibraryBrowserRoute {
    Artist(ArtistView),
    Album(AlbumView),
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
    OpenArtist(String),
    OpenAlbum(String),
    ContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),
}

#[derive(Debug, Clone)]
pub enum LibraryBrowserOutMessage {
    Idle,
    RequestToggleLike(String),
    RequestAddToPlaylist { playlist_id: String, track_id: String },
    RequestToggleFollowArtist(String, String, Option<String>),
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
            None => {}
        }
    }

    /// Busca un track por id en la ruta actualmente activa (top 5 del artista o tracks del álbum).
    fn find_track(&self, id: &str) -> Option<&Track> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.find_song(id),
            Some(LibraryBrowserRoute::Album(view)) => view.find_track(id),
            None => None,
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
            None => {}
        }
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

                let Some(track) = self.find_track(&track_id) else {
                    return (Task::none(), LibraryBrowserOutMessage::Idle);
                };

                match action {
                    TrackContextAction::PlayNow => {
                        self.manager.play_context(vec![track.clone()], 0);
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::Enqueue => {
                        self.manager.enqueue(track.clone());
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::FrontEnqueue => {
                        self.manager.enqueue_front(track.clone());
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::CopyId => {
                        (iced::clipboard::write(track_id), LibraryBrowserOutMessage::Idle)
                    }
                    TrackContextAction::ToggleLike => {
                        (Task::none(), LibraryBrowserOutMessage::RequestToggleLike(track_id))
                    }
                    TrackContextAction::AddToPlaylist(playlist_id) => {
                        (Task::none(), LibraryBrowserOutMessage::RequestAddToPlaylist { playlist_id, track_id })
                    }
                    TrackContextAction::DeleteFromCatalog | TrackContextAction::RemoveFromPlaylist => {
                        (Task::none(), LibraryBrowserOutMessage::Idle)
                    }
                }
            }

            LibraryBrowserMessage::Artist(msg) => match &mut self.active {
                Some(LibraryBrowserRoute::Artist(view)) => {
                    let (task, out) = view.update(msg);
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
                        ArtistOutMessage::PlayTopSong(id) => {
                            let songs = view.top_songs();
                            if let Some(index) = songs.iter().position(|t| t.id == id) {
                                self.manager.set_playback_origin(PlaybackOrigin::Artist(view.artist_id().to_string()));
                                self.manager.play_context(songs, index);
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_song(&id) {
                                let member_of = catalog_store.playlists_containing_track(&id);
                                let items = TrackContextMenuBuilder::new(track.liked)
                                    .with_playlists(playlists, None, &member_of)
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
                                let items = TrackContextMenuBuilder::new(track.liked)
                                    .with_playlists(playlists, None, &member_of)
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

    pub fn view(&self) -> Element<'_, LibraryBrowserMessage> {
        let is_playing = self.manager.state.is_playing();

        let origin_matches_active_route = match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => {
                self.manager.get_playback_origin() == Some(PlaybackOrigin::Artist(view.artist_id().to_string()))
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                self.manager.get_playback_origin() == Some(PlaybackOrigin::Album(view.album_id().to_string()))
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
                view.view(now_playing_id, is_playing).map(LibraryBrowserMessage::Artist)
            }
            Some(LibraryBrowserRoute::Album(view)) => {
                view.view(now_playing_id, is_playing).map(LibraryBrowserMessage::Album)
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
            |action, id| LibraryBrowserMessage::TrackContextAction(action, id),
            LibraryBrowserMessage::ContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| LibraryBrowserMessage::ContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }
}
