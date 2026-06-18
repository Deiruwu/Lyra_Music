use iced::{Element, Subscription, Task};
use crate::microservices::client::MicroserviceClient;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::ui::search_feature::search_bar::{SearchFilter, SearchInput, SearchMessage, SearchOutMessage};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

#[derive(Debug, Clone)]
pub enum SearchFeatureMessage {
    Ui(SearchMessage),
    SearchCompleted(Result<Vec<Track>, String>),
    ThumbnailLoaded { track_id: String, bytes: Vec<u8> },
    DownloadFinished(Result<PlayableTrack, String>),
}

#[derive(Debug, Clone)]
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

    // INYECCIÓN: Recibimos el ThumbnailCache mutable
    pub fn update(
        &mut self,
        msg: SearchFeatureMessage,
        thumbnails: &mut ThumbnailCache
    ) -> (Task<SearchFeatureMessage>, SearchFeatureOutMessage) {
        match msg {
            SearchFeatureMessage::Ui(ui_msg) => {
                let (task, out_msg) = self.input.update(ui_msg);
                let mut district_task = Task::none();

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
                                SearchFilter::Songs => Some("songs"),
                                SearchFilter::Videos => Some("videos"),
                            };

                            district_task = Task::perform(
                                async move {
                                    client.search(&query, 5, filter_str).await.map_err(|e| e.to_string())
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

                        district_task = Task::perform(
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

                (Task::batch(vec![task.map(SearchFeatureMessage::Ui), district_task]), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::SearchCompleted(Ok(tracks)) => {
                self.is_searching = false;
                self.results = tracks.clone();

                // Usamos el caché inyectado para pedir las imágenes
                let tasks: Vec<Task<_>> = tracks.into_iter().filter_map(|t| {
                    let id = t.id.clone();
                    let url = t.thumbnail_small.clone()?;

                    thumbnails.request_download(id, url, |id, bytes| {
                        SearchFeatureMessage::ThumbnailLoaded { track_id: id, bytes }
                    })
                }).collect();

                (Task::batch(tasks), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::SearchCompleted(Err(e)) => {
                self.is_searching = false;
                println!("Error de red: {}", e);
                (Task::none(), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::ThumbnailLoaded { track_id, bytes } => {
                // 1. Buscamos si la pista ya estaba descargada
                let is_downloaded = self.results.iter()
                    .find(|t| t.id == track_id)
                    .map_or(false, |t| t.file_path.is_some());

                // 2. Inyectamos al caché con el método correcto
                if is_downloaded {
                    thumbnails.insert(track_id, bytes);
                } else {
                    thumbnails.insert_grayscale(track_id, bytes);
                }

                (Task::none(), SearchFeatureOutMessage::Idle)
            }

            SearchFeatureMessage::DownloadFinished(Ok(playable)) => {
                println!("Descarga completada y lista para sonar.");
                (Task::none(), SearchFeatureOutMessage::TrackReadyToPlay(playable))
            }

            SearchFeatureMessage::DownloadFinished(Err(e)) => {
                println!("Error descargando/procesando la canción: {}", e);
                (Task::none(), SearchFeatureOutMessage::Idle)
            }
        }
    }

    pub fn view(&self) -> Element<'_, SearchFeatureMessage> {
        self.input.view().map(SearchFeatureMessage::Ui)
    }
    pub fn view_dropdown<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, SearchFeatureMessage> {
        self.input.view_dropdown(
            self.is_searching,
            &self.results,
            thumbnails
        ).map(SearchFeatureMessage::Ui)
    }
}