use std::sync::Arc;

use iced::widget::space;
use iced::{Element, Task};

use crate::audio::manager::manager::TrackManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
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
    previous: Option<LibraryBrowserRoute>,
    client: MicroserviceClient,
    manager: Arc<TrackManager>,
    context_menu: ContextMenu<String>,
    context_menu_items: Vec<ContextMenuItem<TrackContextAction>>,
}

#[derive(Debug, Clone)]
pub enum LibraryBrowserMessage {
    Artist(ArtistMessage),
    Album(AlbumMessage),
    OpenArtist(String),
    OpenAlbum(String),
    Close,
    Back,
    ContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),
}

#[derive(Debug, Clone)]
pub enum LibraryBrowserOutMessage {
    Idle,
    RequestToggleLike(String),
    RequestAddToPlaylist { playlist_id: String, track_id: String },
}

impl LibraryBrowserFeature {
    pub fn new(client: MicroserviceClient, manager: Arc<TrackManager>) -> Self {
        Self {
            active: None,
            previous: None,
            client,
            manager,
            context_menu: ContextMenu::new(),
            context_menu_items: Vec::new(),
        }
    }

    /// Si hay una vista de artista/álbum abierta.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Cierra cualquier vista abierta y descarta el historial de navegación.
    pub fn close(&mut self) {
        self.active = None;
        self.previous = None;
    }

    /// Busca un track por id en la ruta actualmente activa (top 5 del artista o tracks del álbum).
    fn find_track(&self, id: &str) -> Option<&Track> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.find_song(id),
            Some(LibraryBrowserRoute::Album(view)) => view.find_track(id),
            None => None,
        }
    }

    pub fn update(
        &mut self,
        message: LibraryBrowserMessage,
        playlists: &[(String, String)],
    ) -> (Task<LibraryBrowserMessage>, LibraryBrowserOutMessage) {
        match message {
            LibraryBrowserMessage::OpenArtist(id) => {
                let (view, task) = ArtistView::new(self.client.clone(), id);
                self.active = Some(LibraryBrowserRoute::Artist(view));
                self.previous = None;
                (task.map(LibraryBrowserMessage::Artist), LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::OpenAlbum(id) => {
                let (view, task) = AlbumView::new(self.client.clone(), id);
                self.active = Some(LibraryBrowserRoute::Album(view));
                self.previous = None;
                (task.map(LibraryBrowserMessage::Album), LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::Close => {
                self.close();
                (Task::none(), LibraryBrowserOutMessage::Idle)
            }

            LibraryBrowserMessage::Back => {
                self.active = self.previous.take();
                (Task::none(), LibraryBrowserOutMessage::Idle)
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
                            let (album_view, album_task) = AlbumView::new(self.client.clone(), id);
                            self.previous = self.active.take();
                            self.active = Some(LibraryBrowserRoute::Album(album_view));
                            (Task::batch([task, album_task.map(LibraryBrowserMessage::Album)]), LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::PlayTopSong(id) => {
                            let songs = view.top_songs();
                            if let Some(index) = songs.iter().position(|t| t.id == id) {
                                self.manager.play_context(songs, index);
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_song(&id) {
                                let items = TrackContextMenuBuilder::new(track.liked)
                                    .with_playlists(playlists, None)
                                    .build();
                                self.context_menu.handle(ContextMenuEvent::RightClicked(id));
                                self.context_menu_items = items;
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        ArtistOutMessage::Idle => (task, LibraryBrowserOutMessage::Idle),
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
                            let (artist_view, artist_task) = ArtistView::new(self.client.clone(), id);
                            self.previous = self.active.take();
                            self.active = Some(LibraryBrowserRoute::Artist(artist_view));
                            (Task::batch([task, artist_task.map(LibraryBrowserMessage::Artist)]), LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::PlayTrack(id) => {
                            let tracks = view.tracks();
                            if let Some(index) = tracks.iter().position(|t| t.id == id) {
                                self.manager.play_context(tracks.to_vec(), index);
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::PlayAlbum => {
                            self.manager.play_context(view.tracks().to_vec(), 0);
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::TrackRightClicked(id) => {
                            if let Some(track) = view.find_track(&id) {
                                let items = TrackContextMenuBuilder::new(track.liked)
                                    .with_playlists(playlists, None)
                                    .build();
                                self.context_menu.handle(ContextMenuEvent::RightClicked(id));
                                self.context_menu_items = items;
                            }
                            (task, LibraryBrowserOutMessage::Idle)
                        }
                        AlbumOutMessage::Idle => (task, LibraryBrowserOutMessage::Idle),
                    }
                }
                _ => (Task::none(), LibraryBrowserOutMessage::Idle),
            },
        }
    }

    pub fn view(&self) -> Element<'_, LibraryBrowserMessage> {
        match &self.active {
            Some(LibraryBrowserRoute::Artist(view)) => view.view().map(LibraryBrowserMessage::Artist),
            Some(LibraryBrowserRoute::Album(view)) => view.view().map(LibraryBrowserMessage::Album),
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
