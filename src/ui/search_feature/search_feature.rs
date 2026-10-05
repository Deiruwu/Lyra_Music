use iced::{Element, Task};
use crate::microservices::client::MicroserviceClient;
use crate::model::audio_tech::PlayableTrack;
use crate::model::{SearchItem, Track, TrackState};
use crate::ui::search_feature::search_bar::{album_thumb_key, artist_thumb_key, SearchFilter, SearchInput, SearchMessage, SearchOutMessage};
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::track_context_builder::TrackContextAction;

/// Epoch fijo para este feature. La búsqueda no tiene noción de
/// "páginas" ni "scroll" que invalide resultados viejos — un resultado
/// que llega tarde sigue siendo válido (la canción sigue en `results` o
/// siendo reproducida), así que no hay nada que descartar por epoch
/// aquí. Se usa 0 constante solo porque la firma de `request_color`/
/// `request_gray` ahora lo exige.
const EPOCH: u64 = 0;

#[derive(Debug, Clone)]
pub enum SearchFeatureMessage {
    Ui(SearchMessage),
    SearchCompleted(Result<Vec<SearchItem>, String>),
    ThumbnailColorLoaded { key: String, bytes: Vec<u8>, epoch: u64 },
    ThumbnailGrayLoaded  { track_id: String, bytes: Vec<u8>, epoch: u64 },
    DownloadFinished(Result<PlayableTrack, String>),
    ContextMenuEvent(ContextMenuEvent<String>),
    ContextAction(TrackContextAction, String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFeatureOutMessage {
    Idle,
    TrackReadyToPlay(PlayableTrack),
    OpenAlbum(String),
    OpenArtist(String),
    /// Clic derecho en una canción: `main.rs` arma el menú (sabe de likes y playlists).
    RequestContextMenu(Track),
    /// Opción elegida en el menú de una canción.
    TrackContextAction(TrackContextAction, Track),
}

pub struct SearchFeature {
    micro_service: MicroserviceClient,
    pub input: SearchInput,
    pub results: Vec<SearchItem>,
    pub is_searching: bool,
    context_menu: ContextMenu<String>,
    context_menu_items: Vec<ContextMenuItem<TrackContextAction>>,
}

impl SearchFeature {
    pub fn new(micro_service: MicroserviceClient) -> Self {
        Self {
            micro_service,
            input: SearchInput::default(),
            results: Vec::new(),
            is_searching: false,
            context_menu: ContextMenu::new(),
            context_menu_items: Vec::new(),
        }
    }

    pub fn update(
        &mut self,
        msg: SearchFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<SearchFeatureMessage>, SearchFeatureOutMessage) {
        match msg {
            SearchFeatureMessage::Ui(ui_msg) => {
                if matches!(ui_msg, SearchMessage::ToggleOpen | SearchMessage::Dismiss | SearchMessage::Close) {
                    self.dismiss_context_menu();
                }
                let (task, out_msg) = self.input.update(ui_msg);
                let mut extra_task = Task::none();
                let mut feature_out = SearchFeatureOutMessage::Idle;

                match out_msg {
                    SearchOutMessage::RequestSearch(query, filter) => {
                        if query.is_empty() {
                            self.results.clear();
                            self.is_searching = false;
                        } else {
                            self.is_searching = true;
                            self.results.clear();

                            let client = self.micro_service.clone();
                            let limit = if filter == SearchFilter::All { 8 } else { 5 };

                            extra_task = Task::perform(
                                async move {
                                    client.search_items(&query, Some(limit), filter.as_param()).await.map_err(|e| e.to_string())
                                },
                                SearchFeatureMessage::SearchCompleted,
                            );
                        }
                    }

                    SearchOutMessage::RequestDownloadAndPlay(track) => {
                        self.results.clear();
                        let client = self.micro_service.clone();
                        let query = track.id.clone();
                        println!("Iniciando descarga de: {}", track.title);

                        extra_task = Task::perform(
                            async move {
                                let downloaded_track = client.download(&query)
                                    .await
                                    .map_err(|e| e.to_string())?;

                                let playable = tokio::task::spawn_blocking(move || {
                                    PlayableTrack::new(downloaded_track)
                                })
                                    .await
                                    .map_err(|e| format!("Fallo interno del hilo: {}", e))?
                                    .map_err(|e| format!("Fallo decodificando audio: {:?}", e))?;

                                Ok(playable)
                            },
                            SearchFeatureMessage::DownloadFinished,
                        );
                    }

                    SearchOutMessage::RequestContextMenu(track) => feature_out = SearchFeatureOutMessage::RequestContextMenu(track),
                    SearchOutMessage::RequestOpenAlbum(album_id) => feature_out = SearchFeatureOutMessage::OpenAlbum(album_id),
                    SearchOutMessage::RequestOpenArtist(artist_id) => feature_out = SearchFeatureOutMessage::OpenArtist(artist_id),

                    SearchOutMessage::Idle => {}
                }

                (Task::batch(vec![task.map(SearchFeatureMessage::Ui), extra_task]), feature_out)
            }

            // ── Resultados de búsqueda ────────────────────────────────────────

            SearchFeatureMessage::SearchCompleted(Ok(items)) => {
                self.is_searching = false;
                self.results = items.clone();

                let color_message = |key, bytes, epoch| SearchFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch };

                // Cada track recibe gris si es Partial, color si ya está Cached;
                // álbumes y artistas siempre a color.
                let tasks: Vec<Task<_>> = items.into_iter().filter_map(|item| {
                    let t = match item {
                        SearchItem::Track(track) => track,
                        SearchItem::Album(album) => {
                            let url = album.thumbnail_small?;
                            return thumbnails.request_color(album_thumb_key(&album.id), url, EPOCH, color_message);
                        }
                        SearchItem::Artist(artist) => {
                            let url = artist.thumbnail_small?;
                            return thumbnails.request_color(artist_thumb_key(&artist.id), url, EPOCH, color_message);
                        }
                    };
                    match t.state {
                        TrackState::Partial => {
                            let url = t.thumbnail_small.clone()?;
                            thumbnails.request_gray(
                                t.id.clone(),
                                url,
                                EPOCH,
                                |id, bytes, epoch| SearchFeatureMessage::ThumbnailGrayLoaded { track_id: id, bytes, epoch },
                            )
                        }
                        TrackState::Cached => {
                            let url = t.thumbnail_small.clone()?;
                            let key = thumb_key(&t);
                            thumbnails.request_color(
                                key,
                                url,
                                EPOCH,
                                |key, bytes, epoch| SearchFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch },
                            )
                        }
                    }
                }).collect();

                (Task::batch(tasks), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::SearchCompleted(Err(e)) => {
                self.is_searching = false;
                println!("Error de red: {}", e);
                (Task::none(), SearchFeatureOutMessage::Idle)
            }

            // ── Thumbnails recibidos ──────────────────────────────────────────
            // En ambos casos hay que llamar on_*_finished para liberar el
            // slot de concurrencia y dejar que la cola arranque el
            // siguiente pendiente (si lo hay). No comparamos epoch contra
            // nada porque EPOCH es constante aquí — siempre se guarda.

            SearchFeatureMessage::ThumbnailGrayLoaded { track_id, bytes, .. } => {
                thumbnails.insert_gray(track_id.clone(), bytes);
                let next = thumbnails.on_gray_finished(&track_id, |id, bytes, epoch| {
                    SearchFeatureMessage::ThumbnailGrayLoaded { track_id: id, bytes, epoch }
                });
                (next, SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::ThumbnailColorLoaded { key, bytes, .. } => {
                thumbnails.insert_color(key.clone(), bytes);
                let next = thumbnails.on_color_finished(&key, |key, bytes, epoch| {
                    SearchFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch }
                });
                (next, SearchFeatureOutMessage::Idle)
            }

            // ── Descarga de canción completada ────────────────────────────────

            SearchFeatureMessage::DownloadFinished(Ok(playable)) => {
                println!("Descarga completada y lista para sonar.");

                let task = playable.track.thumbnail_small.as_ref()
                    .and_then(|url| {
                        let key = thumb_key(&playable.track);
                        thumbnails.request_color(
                            key,
                            url.clone(),
                            EPOCH,
                            |key, bytes, epoch| SearchFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch },
                        )
                    })
                    .unwrap_or(Task::none());

                (task, SearchFeatureOutMessage::TrackReadyToPlay(playable))
            }

            SearchFeatureMessage::DownloadFinished(Err(e)) => {
                println!("Error descargando/procesando la canción: {}", e);
                (Task::none(), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::ContextMenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.context_menu_items.clear();
                }
                self.context_menu.handle(event);
                (Task::none(), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::ContextAction(action, track_id) => {
                self.dismiss_context_menu();
                match self.result_track(&track_id) {
                    Some(track) => (Task::none(), SearchFeatureOutMessage::TrackContextAction(action, track.clone())),
                    None => (Task::none(), SearchFeatureOutMessage::Idle),
                }
            }
        }
    }

    /// Abre el menú de una canción de los resultados con las opciones ya armadas.
    pub fn open_context_menu(&mut self, track_id: String, items: Vec<ContextMenuItem<TrackContextAction>>) {
        self.context_menu_items = items;
        self.context_menu.handle(ContextMenuEvent::RightClicked(track_id));
    }

    /// Cierra el menú de canción; `true` si estaba abierto.
    pub fn dismiss_context_menu(&mut self) -> bool {
        let was_open = self.context_menu.open_id().is_some();
        self.context_menu.handle(ContextMenuEvent::Dismissed);
        self.context_menu_items.clear();
        was_open
    }

    pub fn set_cursor(&mut self, position: iced::Point) {
        self.context_menu.handle(ContextMenuEvent::MouseMoved(position));
    }

    pub fn set_viewport(&mut self, size: iced::Size) {
        self.context_menu.handle(ContextMenuEvent::ViewportResized(size));
    }

    fn result_track(&self, track_id: &str) -> Option<&Track> {
        self.results.iter().find_map(|item| match item {
            SearchItem::Track(track) if track.id == track_id => Some(track),
            _ => None,
        })
    }

    /// Menú de canción abierto sobre la isla de búsqueda.
    pub fn view_context_menu(&self) -> Option<Element<'_, SearchFeatureMessage>> {
        if !self.input.is_open {
            return None;
        }
        let open_track_id = self.context_menu.open_id();
        let (anchor, track_id) = self.context_menu.render_target(|_| open_track_id)?;
        Some(self.context_menu.view(
            anchor,
            self.context_menu_items.clone(),
            track_id,
            SearchFeatureMessage::ContextAction,
            SearchFeatureMessage::ContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| SearchFeatureMessage::ContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }

    pub fn view_toggle(&self) -> Element<'_, SearchFeatureMessage> {
        self.input.view_toggle().map(SearchFeatureMessage::Ui)
    }

    /// Isla flotante de búsqueda como capa de overlay — `None` mientras
    /// está cerrada, para que `main.rs` no monte la capa de dismiss.
    pub fn view_overlay<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Option<Element<'a, SearchFeatureMessage>> {
        self.input.view_overlay(
            self.is_searching,
            &self.results,
            thumbnails,
        ).map(|el| el.map(SearchFeatureMessage::Ui))
    }
}