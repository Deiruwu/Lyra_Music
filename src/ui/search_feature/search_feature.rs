use iced::{Element, Subscription, Task};
use crate::microservices::client::MicroserviceClient;
use crate::model::audio_tech::PlayableTrack;
use crate::model::{Track, TrackState};
use crate::ui::search_feature::search_bar::{SearchFilter, SearchInput, SearchMessage, SearchOutMessage};
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};

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
    SearchCompleted(Result<Vec<Track>, String>),
    ThumbnailColorLoaded { key: String, bytes: Vec<u8>, epoch: u64 },
    ThumbnailGrayLoaded  { track_id: String, bytes: Vec<u8>, epoch: u64 },
    DownloadFinished(Result<PlayableTrack, String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFeatureOutMessage {
    Idle,
    TrackReadyToPlay(PlayableTrack),
}

pub struct SearchFeature {
    micro_service: MicroserviceClient,
    pub input: SearchInput,
    pub results: Vec<Track>,
    pub is_searching: bool,
}

impl SearchFeature {
    pub fn new() -> Self {
        let host = std::env::var("TRACK_MANAGER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port: u16 = std::env::var("TRACK_MANAGER_PORT")
            .unwrap_or_else(|_| "7878".to_string())
            .parse()
            .expect("TRACK_MANAGER_PORT debe ser un número válido");

        Self {
            micro_service: MicroserviceClient::new(&host, port),
            input: SearchInput::default(),
            results: Vec::new(),
            is_searching: false,
        }
    }

    pub fn subscription(&self) -> Subscription<SearchFeatureMessage> {
        let target = if self.input.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
        if (self.input.thumb_offset - target).abs() > 0.5 {
            iced::window::frames().map(|_| SearchFeatureMessage::Ui(SearchMessage::Tick))
        } else {
            Subscription::none()
        }
    }

    pub fn update(
        &mut self,
        msg: SearchFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<SearchFeatureMessage>, SearchFeatureOutMessage) {
        match msg {
            SearchFeatureMessage::Ui(ui_msg) => {
                let (task, out_msg) = self.input.update(ui_msg);
                let mut extra_task = Task::none();

                match out_msg {
                    SearchOutMessage::RequestSearch(query, filter) => {
                        if query.is_empty() {
                            self.results.clear();
                            self.is_searching = false;
                        } else {
                            self.is_searching = true;
                            self.results.clear();

                            let client = self.micro_service.clone();
                            let filter_str = match filter {
                                SearchFilter::Songs  => Some("songs"),
                                SearchFilter::Videos => Some("videos"),
                            };

                            extra_task = Task::perform(
                                async move {
                                    client.search(&query, Some(5), filter_str).await.map_err(|e| e.to_string())
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

                    SearchOutMessage::Idle => {}
                }

                (Task::batch(vec![task.map(SearchFeatureMessage::Ui), extra_task]), SearchFeatureOutMessage::Idle)
            }

            // ── Resultados de búsqueda ────────────────────────────────────────

            SearchFeatureMessage::SearchCompleted(Ok(tracks)) => {
                self.is_searching = false;
                self.results = tracks.clone();

                // Cada track recibe gris si es Partial, color si ya está Cached.
                let tasks: Vec<Task<_>> = tracks.into_iter().filter_map(|t| {
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
        }
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