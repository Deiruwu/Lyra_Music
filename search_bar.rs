use std::collections::HashMap;
use iced::{border, Color, Element, Length, Padding, Theme};
use iced::widget::{button, column, container, row, space, text, text_input};
use iced::widget::image::Handle;
use crate::microservices::client::MicroserviceClient;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::ui::utils::image::download_thumbnail;
use crate::ui::widgets::icon_toggle::IconToggle;
use crate::ui::widgets::track_row::track_row;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    Songs,
    Videos,
}

#[derive(Debug, Clone)]
pub enum SearchMessage {
    InputChanged(String),
    SubmitSearch,
    SearchCompleted(Result<Vec<Track>, String>),
    TrackSelected(Track),
    DownloadFinished(Result<PlayableTrack, String>),
    ThumbnailLoaded(String, Vec<u8>),
    FilterChanged(SearchFilter),
    Tick,
}

pub struct SearchInput {
    micro_service: MicroserviceClient,
    pub input_value: String,
    pub results: Vec<Track>,
    pub is_searching: bool,
    pub thumbnails: HashMap<String, Handle>,
    pub filter: SearchFilter,
    pub thumb_offset: f32,
}

impl SearchInput {
    pub fn new() -> Self {
        let host = std::env::var("TRACK_MANAGER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port: u16 = std::env::var("TRACK_MANAGER_PORT")
            .unwrap_or_else(|_| "7878".to_string())
            .parse()
            .expect("TRACK_MANAGER_PORT debe ser un número válido");

        Self {
            micro_service: MicroserviceClient::new(&host, port),
            input_value: String::new(),
            results: Vec::new(),
            is_searching: false,
            thumbnails: HashMap::new(),
            filter: SearchFilter::Songs,
            thumb_offset: 0.0,
        }
    }

    pub fn subscription(&self) -> iced::Subscription<SearchMessage> {
        let target = if self.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
        if (self.thumb_offset - target).abs() > 0.5 {
            iced::window::frames().map(|_| SearchMessage::Tick)
        } else {
            iced::Subscription::none()
        }
    }

    pub fn update(&mut self, msg: SearchMessage) -> iced::Task<SearchMessage> {
        match msg {
            SearchMessage::Tick => {
                let target = if self.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
                let diff = target - self.thumb_offset;

                if diff.abs() > 0.5 {
                    self.thumb_offset += diff * 0.3;
                } else {
                    self.thumb_offset = target;
                }

                iced::Task::none()
            }

            SearchMessage::InputChanged(value) => {
                if self.input_value.is_empty() {
                    self.results.clear();
                    self.thumbnails.clear();
                }

                self.input_value = value;
                iced::Task::none()
            }

            SearchMessage::FilterChanged(filter) => {
                self.filter = filter;
                if !self.input_value.is_empty() {
                    return self.update(SearchMessage::SubmitSearch);
                }
                iced::Task::none()
            }

            SearchMessage::SubmitSearch => {
                if self.input_value.is_empty() {
                    return iced::Task::none();
                }
                self.is_searching = true;
                self.results.clear();
                self.thumbnails.clear();

                let query = self.input_value.clone();
                let client = self.micro_service.clone();

                let filter = match &self.filter {
                    SearchFilter::Songs => Some("songs"),
                    SearchFilter::Videos => Some("videos"),
                };

                iced::Task::perform(
                    async move {
                        client.search(&query, 5, filter).await.map_err(|e| e.to_string())
                    },
                    SearchMessage::SearchCompleted,
                )
            }

            SearchMessage::SearchCompleted(Ok(tracks)) => {
                self.is_searching = false;

                let tasks: Vec<_> = tracks.iter()
                    .filter_map(|t| {
                        let url = t.thumbnail_small.clone()?;
                        let id = t.id.clone();
                        Some(iced::Task::perform(
                            download_thumbnail(url),
                            move |result| match result {
                                Ok(bytes) => SearchMessage::ThumbnailLoaded(id, bytes),
                                Err(_) => SearchMessage::ThumbnailLoaded(id, vec![]),
                            },
                        ))
                    })
                    .collect();

                self.results = tracks;
                iced::Task::batch(tasks)
            }

            SearchMessage::SearchCompleted(Err(e)) => {
                self.is_searching = false;
                println!("Error de red: {}", e);
                iced::Task::none()
            }

            SearchMessage::ThumbnailLoaded(id, bytes) => {
                if !bytes.is_empty() {
                    // Determinamos si está descargada
                    let is_downloaded = self.results.iter()
                        .find(|t| t.id == id)
                        .map_or(false, |t| t.file_path.is_some());

                    let handle = if is_downloaded {
                        Handle::from_bytes(bytes)
                    } else {
                        // Convertir a escala de grises
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let gray = img.grayscale();
                            let mut buf = std::io::Cursor::new(Vec::new());
                            gray.write_to(&mut buf, image::ImageFormat::Png).unwrap();
                            Handle::from_bytes(buf.into_inner())
                        } else {
                            Handle::from_bytes(bytes)
                        }
                    };

                    self.thumbnails.insert(id, handle);
                }
                iced::Task::none()
            }

            SearchMessage::TrackSelected(track) => {
                self.results.clear();
                self.input_value.clear();
                self.thumbnails.clear();

                let query = track.id.clone();
                let client = self.micro_service.clone();

                println!("Iniciando descarga de: {}", track.title);

                iced::Task::perform(
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
                    SearchMessage::DownloadFinished,
                )
            }

            SearchMessage::DownloadFinished(Ok(_)) => {
                println!("Descarga completada y lista para sonar.");
                iced::Task::none()
            }

            SearchMessage::DownloadFinished(Err(e)) => {
                println!("Error descargando/procesando la canción: {}", e);
                iced::Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, SearchMessage> {
        let is_videos = self.filter == SearchFilter::Videos;
        let search_bar = row![
            text_input("Buscar canción, álbum, artista...", &self.input_value)
                .on_input(SearchMessage::InputChanged)
                .on_submit(SearchMessage::SubmitSearch)
                .padding(10)
                .width(Length::Fill),

            IconToggle::new(
                        is_videos,
                        self.thumb_offset,
                        |next_state| {
                            SearchMessage::FilterChanged(if next_state {
                                SearchFilter::Videos
                            } else {
                                SearchFilter::Songs
                            })
                        }
                    )
                    .build(),

            button("Buscar")
                .on_press(SearchMessage::SubmitSearch)
                .padding(10)
        ]
            .spacing(10)
            .align_y(iced::Alignment::Center);

        container(search_bar)
            .padding(20)
            .width(Length::Fill)
            .into()
    }

    pub fn view_dropdown(&self) -> Element<'_, SearchMessage> {
        let show_dropdown = !self.results.is_empty() || self.is_searching;

        if !show_dropdown {
            return space().into();
        }

        let mut results_column = column![].spacing(10);

        if self.is_searching {
            results_column = results_column.push(text("Buscando...").size(16));
        } else {
            for track in &self.results {
                let thumbnail = self.thumbnails.get(&track.id).;
                results_column = results_column.push(
                    track_row(track, thumbnail, SearchMessage::TrackSelected(track.clone()))
                );
            }
        }

        column![
        space().height(70),
        container(results_column)
            .padding(18)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(Color::from_rgb(0.15, 0.15, 0.15).into()),
                border: border::color(Color::from_rgb(0.3, 0.3, 0.3)).width(1.0).rounded(8),
                ..Default::default()
            }),
    ]
            .padding(Padding::default().left(20).right(20))
            .into()
    }
}