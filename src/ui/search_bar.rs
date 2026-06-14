use std::collections::HashMap;
use iced::{border, Color, Element, Length, Padding, Theme};
use iced::widget::{button, column, container, row, space, text, text_input};
use iced::widget::image::Handle;
use crate::JETBRAINS_MONO;
use crate::microservices::client::MicroserviceClient;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::image::download_thumbnail;
use crate::ui::widgets::icon_toggle::IconToggle;
use crate::ui::widgets::track_row::track_row;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    Songs,
    Videos,
}

impl SearchFilter {
    fn as_str(&self) -> &'static str {
        match self {
            SearchFilter::Songs => "songs",
            SearchFilter::Videos => "videos",
        }
    }
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
                // Definimos a dónde debe ir la bola según el filtro actual
                let target = if self.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
                let diff = target - self.thumb_offset;

                // Si la distancia es mayor a medio píxel, la movemos un 30% del camino (Ease-out)
                if diff.abs() > 0.5 {
                    self.thumb_offset += diff * 0.3;
                } else {
                    // Si ya está muy cerca, la anclamos para evitar micro-cálculos infinitos
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


    fn custom_icon_toggle(&self) -> Element<'_, SearchMessage> {
        let is_videos = self.filter == SearchFilter::Videos;

        // 1. Definimos el "Thumb" (la bolita que se mueve)
        // Usamos un container vacío con un tamaño fijo y borde completamente redondo
        let thumb = container(space().width(20).height(20))
            .style(|_theme| container::Style {
                background: Some(Color::WHITE.into()), // Color de la bolita
                border: border::rounded(10), // Radio = mitad del ancho para que sea un círculo perfecto
                ..Default::default()
            });

        // 2. Construimos el contenido de la pista dependiendo del estado
        let track_content = if is_videos {
            // Estado Videos: El pulgar está a la derecha. Ponemos el icono de Songs a la izquierda.
            row![
                space().width(4),
                text("").font(JETBRAINS_MONO).size(14).style(|_theme| text::Style {
                    color: Option::from(Color::from_rgb(0.6, 0.6, 0.6)),
                ..Default::default()
                }),
                space().width(Length::Fill),
                thumb,
            ]
                .align_y(iced::Alignment::Center)
        } else {
            // Estado Songs: El pulgar está a la izquierda. Ponemos el icono de Videos a la derecha.
            row![
                thumb,
                space().width(Length::Fill),
                text("").font(JETBRAINS_MONO).size(14).style(|_theme| text::Style {
                    color: Option::from(Color::from_rgb(0.6, 0.6, 0.6)),
                ..Default::default()
                }),
                space().width(11),
            ]
                .align_y(iced::Alignment::Center)
        };

        // 3. Definimos la pista (Track)
        let track = container(track_content)
            .width(60) // Ancho total fijo del toggle
            .padding(4) // Espacio entre el borde de la pista y la bolita
            .style(move |_theme| container::Style {
                // Color de fondo de la pista (puedes hacerlo dinámico según el estado si quieres)
                background: Some(Color::from_rgb(0.2, 0.2, 0.2).into()),
                border: border::rounded(20), // Forma de píldora
                ..Default::default()
            });

        // 4. Envolvemos la pista en un botón transparente para que reaccione al clic
        button(track)
            .padding(0) // Quitamos el padding del botón para que el container defina el tamaño
            .style(transparent_button) // Tu estilo ya existente para que el botón no dibuje fondo
            .on_press(SearchMessage::FilterChanged(if is_videos {
                SearchFilter::Songs
            } else {
                SearchFilter::Videos
            }))
            .into()
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
                            // El closure recibe el nuevo estado booleano tras el clic
                            SearchMessage::FilterChanged(if next_state {
                                SearchFilter::Videos
                            } else {
                                SearchFilter::Songs
                            })
                        }
                    )
                    // Puedes encadenar builders opcionales si cambias de opinión con la estética:
                    // .icons("", "")
                    // .width(70.0)
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
                let thumbnail = self.thumbnails.get(&track.id);
                results_column = results_column.push(
                    track_row(track, thumbnail, SearchMessage::TrackSelected(track.clone()))
                );
            }
        }

        column![
        space().height(70), // altura del search bar + padding
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