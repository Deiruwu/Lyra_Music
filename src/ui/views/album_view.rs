use std::collections::HashSet;

use iced::border::rounded;
use iced::widget::{button, column, container, image, row, scrollable, space, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Task, Theme};

use crate::microservices::client::MicroserviceClient;
use crate::model::{AlbumDto, Artist, Track};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::utils::gallery_thumbnail::{GalleryThumbnail, Treatment};
use crate::ui::utils::playlist_metadata::{format_total_duration, format_track_count, track_stats};
use crate::ui::widgets::track_row_simple::track_row_numbered;

const COVER_SIZE: f32 = 220.0;
const COVER_RADIUS: f32 = 12.0;
const GALLERY_MAX_SIDE: u32 = 500;

enum AlbumViewData {
    Loading,
    Loaded(AlbumDto),
    Error(String),
}

pub struct AlbumView {
    album_id: String,
    client: MicroserviceClient,
    data: AlbumViewData,
    gallery: GalleryThumbnail,
}

#[derive(Debug, Clone)]
pub enum AlbumMessage {
    Loaded(Result<AlbumDto, String>),
    ThumbnailLoaded(String, Vec<u8>),
    ArtistPressed(String),
    TrackRowPressed(String),
    TrackRowRightClicked(String),
    PlayAlbumPressed,
}

#[derive(Debug, Clone)]
pub enum AlbumOutMessage {
    Idle,
    OpenArtist(String),
    PlayTrack(String),
    PlayAlbum,
    TrackRightClicked(String),
}

impl AlbumView {
    pub fn new(client: MicroserviceClient, album_id: String) -> (Self, Task<AlbumMessage>) {
        let view = Self {
            album_id: album_id.clone(),
            client: client.clone(),
            data: AlbumViewData::Loading,
            gallery: GalleryThumbnail::new(),
        };

        let task = Task::perform(
            async move { client.album(&album_id).await.map_err(|e| e.to_string()) },
            AlbumMessage::Loaded,
        );

        (view, task)
    }

    pub fn update(&mut self, message: AlbumMessage) -> (Task<AlbumMessage>, AlbumOutMessage) {
        let mut out = AlbumOutMessage::Idle;

        match message {
            AlbumMessage::Loaded(Ok(dto)) => self.data = AlbumViewData::Loaded(dto),
            AlbumMessage::Loaded(Err(error)) => self.data = AlbumViewData::Error(error),
            AlbumMessage::ThumbnailLoaded(key, bytes) => self.gallery.on_loaded(key, bytes),
            AlbumMessage::ArtistPressed(id) => out = AlbumOutMessage::OpenArtist(id),
            AlbumMessage::TrackRowPressed(id) => out = AlbumOutMessage::PlayTrack(id),
            AlbumMessage::TrackRowRightClicked(id) => out = AlbumOutMessage::TrackRightClicked(id),
            AlbumMessage::PlayAlbumPressed => out = AlbumOutMessage::PlayAlbum,
        }

        let wanted = self.thumbnail_targets();
        let task = self.gallery.sync(&wanted, AlbumMessage::ThumbnailLoaded);

        (task, out)
    }

    pub fn view(&self) -> Element<'_, AlbumMessage> {
        match &self.data {
            AlbumViewData::Loading => status_message("Cargando álbum…"),
            AlbumViewData::Error(error) => status_message(error),
            AlbumViewData::Loaded(album) => scrollable(
                column![self.view_header(album), self.view_track_list(&album.tracks)]
                    .spacing(24)
                    .padding(Padding { top: 0.0, right: 24.0, bottom: 32.0, left: 24.0 }),
            )
            .width(Length::Fill)
            .into(),
        }
    }

    /// Header estilo playlist: gradiente + `thumbnail_large` como portada, nombre,
    /// total de canciones/duración y fecha de salida.
    fn view_header<'a>(&'a self, album: &'a AlbumDto) -> Element<'a, AlbumMessage> {
        let cover: Element<'a, AlbumMessage> = match self.gallery.get(&album.id) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(COVER_SIZE))
                .height(Length::Fixed(COVER_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(COVER_RADIUS)
                .into(),
            None => container(space())
                .width(Length::Fixed(COVER_SIZE))
                .height(Length::Fixed(COVER_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.18, 0.18).into()),
                    border: rounded(COVER_RADIUS),
                    ..Default::default()
                })
                .into(),
        };

        let (track_count, total_duration) = track_stats(&album.tracks);

        let title = text(album.name.as_str()).font(SF_PRO).size(36).color(Color::WHITE);

        let artists_and_type = self.view_artists_and_type(album);

        let metadata = text(format!(
            "{} · {}",
            format_track_count(track_count),
            format_total_duration(total_duration),
        ))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75));

        let release_date = text(format!(
            "Fecha de salida: {}",
            album.year.as_deref().unwrap_or("—"),
        ))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.6, 0.6, 0.65));

        let play_button = button(text("Reproducir").font(SF_PRO).size(14).color(Color::WHITE))
            .padding(Padding { top: 8.0, right: 20.0, bottom: 8.0, left: 20.0 })
            .style(|_theme: &Theme, status| {
                let base = Color::from_rgb(0.55, 0.35, 0.85);
                let background = match status {
                    button::Status::Hovered => Color::from_rgb(0.62, 0.42, 0.92),
                    _ => base,
                };
                button::Style {
                    background: Some(background.into()),
                    text_color: Color::WHITE,
                    border: rounded(20.0),
                    ..Default::default()
                }
            })
            .on_press(AlbumMessage::PlayAlbumPressed);

        let info = column![
            title,
            artists_and_type,
            space().height(4),
            metadata,
            release_date,
            space().height(8),
            play_button,
        ]
        .spacing(2)
        .align_x(Alignment::Start);

        let content = row![cover, info]
            .spacing(24)
            .align_y(Alignment::End)
            .padding(Padding { top: 32.0, bottom: 28.0, left: 8.0, right: 8.0 });

        container(content)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(
                    iced::gradient::Linear::new(std::f32::consts::PI * 1.5)
                        .add_stop(0.0, Color::from_rgb(0.22, 0.16, 0.28))
                        .add_stop(1.0, Color::from_rgb(0.09, 0.09, 0.10))
                        .into(),
                ),
                ..Default::default()
            })
            .into()
    }

    /// Artistas del álbum (clickeables, uno por cada crédito distinto) seguidos del tipo (Álbum/Single/EP).
    fn view_artists_and_type<'a>(&'a self, album: &'a AlbumDto) -> Element<'a, AlbumMessage> {
        let artists = album_artists(&album.tracks);

        let mut children: Vec<Element<'a, AlbumMessage>> = Vec::new();
        for (index, artist) in artists.iter().enumerate() {
            if index > 0 {
                children.push(text(", ").font(SF_PRO).size(14).color(Color::from_rgb(0.7, 0.7, 0.75)).into());
            }
            let name_text = text(artist.name.as_str()).font(SF_PRO).size(14).color(Color::from_rgb(0.85, 0.85, 0.9));
            children.push(match &artist.id {
                Some(id) => button(name_text)
                    .padding(0)
                    .style(|_theme: &Theme, status| button::Style {
                        background: None,
                        text_color: match status {
                            button::Status::Hovered => Color::WHITE,
                            _ => Color::from_rgb(0.85, 0.85, 0.9),
                        },
                        ..Default::default()
                    })
                    .on_press(AlbumMessage::ArtistPressed(id.clone()))
                    .into(),
                None => name_text.into(),
            });
        }

        children.push(
            text(format!(" · {}", album.album_type.label()))
                .font(SF_PRO)
                .size(14)
                .color(Color::from_rgb(0.7, 0.7, 0.75))
                .into(),
        );

        row(children).align_y(Alignment::Center).into()
    }

    /// Lista completa de tracks del álbum: fila numerada, título/artista apilados, caché y duración.
    fn view_track_list<'a>(&'a self, tracks: &'a [Track]) -> Element<'a, AlbumMessage> {
        let rows: Vec<Element<'a, AlbumMessage>> = tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                track_row_numbered(
                    index + 1,
                    track,
                    AlbumMessage::TrackRowPressed(track.id.clone()),
                    AlbumMessage::TrackRowRightClicked(track.id.clone()),
                )
            })
            .collect();

        column(rows).spacing(4).into()
    }

    /// Canciones del álbum, para armar `play_context` desde `LibraryBrowserFeature`.
    pub(crate) fn tracks(&self) -> &[Track] {
        match &self.data {
            AlbumViewData::Loaded(album) => &album.tracks,
            _ => &[],
        }
    }

    /// Busca una canción del álbum por id (para armar el menú contextual).
    pub(crate) fn find_track(&self, id: &str) -> Option<&Track> {
        self.tracks().iter().find(|t| t.id == id)
    }

    fn thumbnail_targets(&self) -> Vec<(String, String, Treatment)> {
        let AlbumViewData::Loaded(album) = &self.data else { return Vec::new() };

        match &album.thumbnail_large {
            Some(url) => vec![(album.id.clone(), url.clone(), Treatment::MaxSide(GALLERY_MAX_SIDE))],
            None => Vec::new(),
        }
    }
}

/// Artistas distintos acreditados en el álbum, en orden de primera aparición.
fn album_artists(tracks: &[Track]) -> Vec<&Artist> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for track in tracks {
        for artist in &track.artists {
            let key = artist.id.as_deref().unwrap_or(artist.name.as_str());
            if seen.insert(key) {
                result.push(artist);
            }
        }
    }

    result
}

fn status_message(message: &str) -> Element<'_, AlbumMessage> {
    container(text(message.to_string()).font(SF_PRO).size(14).color(Color::from_rgb(0.6, 0.6, 0.65)))
        .width(Length::Fill)
        .height(Length::Fixed(200.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}
