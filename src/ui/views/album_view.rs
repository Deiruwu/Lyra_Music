use std::collections::HashSet;

use iced::border::rounded;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, image, row, scrollable, space, text, Id};
use iced::{Alignment, ContentFit, Element, Length, Padding, Task, Theme};

use crate::microservices::client::MicroserviceClient;
use crate::model::{AlbumDto, Artist, Track};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::utils::gallery_thumbnail::{GalleryThumbnail, Treatment};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::utils::playlist_metadata::{format_total_duration, format_track_count, track_stats};
use crate::ui::widgets::artist_links::artist_links;
use crate::ui::widgets::selection_state::{stepped_index, DoubleClickDetector, SelectionStep};
use crate::ui::widgets::track_row_simple::{track_row_numbered, NUMBERED_ROW_HEIGHT};
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;
use crate::ui::styles::scrollable as scrollable_style;

const COVER_SIZE: f32 = 220.0;
const COVER_RADIUS: f32 = 12.0;
const GALLERY_MAX_SIDE: u32 = 500;
const PLAY_BUTTON_SIZE: f32 = 52.0;
const HEADER_PADDING_TOP: f32 = spacing::SP_32;
const HEADER_PADDING_BOTTOM: f32 = spacing::SP_28;
const SECTION_SPACING: f32 = spacing::SP_24;
const TRACK_ROW_SPACING: f32 = spacing::SP_4;

enum AlbumViewData {
    Loading,
    Loaded(AlbumDto),
    Error(String),
}

pub struct AlbumView {
    album_id: String,
    data: AlbumViewData,
    gallery: GalleryThumbnail,
    /// Scroll de esta vista — `pub` para que `LibraryBrowserFeature` lo
    /// guarde/restaure al navegar entre artistas/álbumes (ver
    /// `stash_active_route_scroll`).
    pub scroll: ScrollTracker,
    icon_hovered: bool,
    selected_id: Option<String>,
    clicks: DoubleClickDetector,
}

#[derive(Debug, Clone)]
pub enum AlbumMessage {
    Loaded(Result<AlbumDto, String>),
    ThumbnailLoaded(String, Vec<u8>),
    ArtistPressed(String),
    /// Click en una fila: selecciona; doble click reproduce.
    TrackRowPressed(String),
    TrackRowRightClicked(String),
    TrackArtistPressed(String),
    TogglePlayback,
    TrackIconHover(bool),
    PlayAlbumPressed,
    Scrolled(Viewport),
}

#[derive(Debug, Clone)]
pub enum AlbumOutMessage {
    Idle,
    OpenArtist(String),
    PlayTrack(String),
    PlayAlbum,
    TrackRightClicked(String),
    OpenTrackArtist(String),
    RequestTogglePlayback,
}

impl AlbumView {
    pub fn new(client: MicroserviceClient, album_id: String) -> (Self, Task<AlbumMessage>) {
        let view = Self {
            album_id: album_id.clone(),
            data: AlbumViewData::Loading,
            gallery: GalleryThumbnail::new(),
            scroll: ScrollTracker::default(),
            icon_hovered: false,
            selected_id: None,
            clicks: DoubleClickDetector::default(),
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
            AlbumMessage::TrackRowPressed(id) => {
                if self.clicks.register(&id) {
                    out = AlbumOutMessage::PlayTrack(id);
                } else {
                    self.selected_id = Some(id);
                }
            }
            AlbumMessage::TrackRowRightClicked(id) => {
                self.selected_id = Some(id.clone());
                out = AlbumOutMessage::TrackRightClicked(id);
            }
            AlbumMessage::TrackArtistPressed(id) => out = AlbumOutMessage::OpenTrackArtist(id),
            AlbumMessage::TogglePlayback => out = AlbumOutMessage::RequestTogglePlayback,
            AlbumMessage::TrackIconHover(hovered) => self.icon_hovered = hovered,
            AlbumMessage::PlayAlbumPressed => out = AlbumOutMessage::PlayAlbum,
            AlbumMessage::Scrolled(viewport) => self.scroll.update(viewport),
        }

        let wanted = self.thumbnail_targets();
        let task = self.gallery.sync(&wanted, AlbumMessage::ThumbnailLoaded);

        (task, out)
    }

    pub fn view(&self, now_playing_id: Option<String>, is_playing: bool) -> Element<'_, AlbumMessage> {
        match &self.data {
            AlbumViewData::Loading => status_message("Cargando álbum…"),
            AlbumViewData::Error(error) => status_message(error),
            AlbumViewData::Loaded(album) => {
                let this_album_is_current = now_playing_id
                    .as_deref()
                    .is_some_and(|id| album.tracks.iter().any(|t| t.id == id));
                let header_is_playing = this_album_is_current && is_playing;

                scrollable(
                    column![
                        self.view_header(album, this_album_is_current, header_is_playing),
                        self.view_track_list(&album.tracks, now_playing_id, is_playing),
                    ]
                        .spacing(SECTION_SPACING)
                        .padding(Padding { top: spacing::SP_0, right: spacing::SP_24, bottom: spacing::SP_32, left: spacing::SP_24 }),
                )
                .width(Length::Fill)
                .style(scrollable_style::discreet)
                .id(Id::new("album_view_scroll"))
                .on_scroll(AlbumMessage::Scrolled)
                .into()
            }
        }
    }

    /// Header estilo playlist: gradiente + `thumbnail_large` como portada, nombre,
    /// total de canciones/duración y fecha de salida.
    fn view_header<'a>(&'a self, album: &'a AlbumDto, this_album_is_current: bool, header_is_playing: bool) -> Element<'a, AlbumMessage> {
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
                    background: Some(theme().surface.sunken.into()),
                    border: rounded(COVER_RADIUS),
                    ..Default::default()
                })
                .into(),
        };

        let (track_count, total_duration) = track_stats(&album.tracks);

        let title = text(album.name.as_str()).font(SF_PRO).size(typography::TEXT_36).color(theme().content.primary);

        let artists_and_type = self.view_artists_and_type(album);

        let metadata = text(format!(
            "{} · {}",
            format_track_count(track_count),
            format_total_duration(total_duration),
        ))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.muted);

        let release_date = text(format!("Fecha de salida: {}", album.year.as_deref().unwrap_or("—")))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.muted);

        let play_icon = if header_is_playing { Icon::Pause } else { Icon::Play };
        let play_message = if this_album_is_current { AlbumMessage::TogglePlayback } else { AlbumMessage::PlayAlbumPressed };

        let play_button = button(
            container(
                icons::icon(play_icon, typography::TEXT_20).color(theme().content.on_accent),
            )
                .width(Length::Fixed(PLAY_BUTTON_SIZE))
                .height(Length::Fixed(PLAY_BUTTON_SIZE))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
            .padding(spacing::SP_0)
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme().accent.hover,
                    _ => theme().accent.primary,
                };
                button::Style {
                    background: Some(bg.into()),
                    border: rounded(PLAY_BUTTON_SIZE / 2.0),
                    ..Default::default()
                }
            })
            .on_press(play_message);

        let info = column![
            title,
            artists_and_type,
            space().height(4),
            metadata,
            release_date,
            space().height(8),
            play_button,
        ]
        .spacing(spacing::SP_2)
        .align_x(Alignment::Start);

        let content = row![cover, info]
            .spacing(spacing::SP_24)
            .align_y(Alignment::End)
            .padding(Padding { top: HEADER_PADDING_TOP, bottom: HEADER_PADDING_BOTTOM, left: spacing::SP_8, right: spacing::SP_8 });

        container(content)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(
                    iced::gradient::Linear::new(std::f32::consts::PI * 1.5)
                        .add_stop(0.0, theme().surface.gradient_start)
                        .add_stop(1.0, theme().surface.base)
                        .into(),
                ),
                ..Default::default()
            })
            .into()
    }

    /// Artistas del álbum (clickeables, uno por cada crédito distinto) seguidos del tipo (Álbum/Single/EP).
    /// Usa el mismo widget `artist_links` que el resto de la app en vez de
    /// armar los spans a mano, para no perder su garantía de una sola
    /// línea sin wrap (la fila del resto de créditos de artista sí la
    /// tiene; esta no la tenía, rompiendo la consistencia visual).
    fn view_artists_and_type<'a>(&'a self, album: &'a AlbumDto) -> Element<'a, AlbumMessage> {
        let derived;
        let artists: &[Artist] = if !album.artists.is_empty() {
            &album.artists
        } else {
            derived = album_artists(&album.tracks).into_iter().cloned().collect::<Vec<_>>();
            &derived
        };

        let artist_line = artist_links(
            artists,
            SF_PRO,
            14.0,
            theme().content.muted,
            Length::Shrink,
            AlbumMessage::ArtistPressed,
        );

        let type_label = text(format!(" · {}", album.album_type.label()))
            .font(SF_PRO)
            .size(typography::TEXT_14)
            .color(theme().content.muted);

        row![artist_line, type_label].align_y(Alignment::Center).into()
    }

    /// Lista completa de tracks del álbum: fila numerada, título/artista apilados, caché y duración.
    fn view_track_list<'a>(&'a self, tracks: &'a [Track], now_playing_id: Option<String>, is_playing: bool) -> Element<'a, AlbumMessage> {
        let rows: Vec<Element<'a, AlbumMessage>> = tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let is_playing_row = now_playing_id.as_deref() == Some(track.id.as_str());
                let is_selected = self.selected_id.as_deref() == Some(track.id.as_str());
                track_row_numbered(
                    index + 1,
                    track,
                    is_selected,
                    AlbumMessage::TrackRowPressed(track.id.clone()),
                    AlbumMessage::TrackRowRightClicked(track.id.clone()),
                    AlbumMessage::TrackArtistPressed,
                    is_playing_row,
                    is_playing,
                    self.icon_hovered,
                    AlbumMessage::TogglePlayback,
                    AlbumMessage::TrackIconHover(true),
                    AlbumMessage::TrackIconHover(false),
                )
            })
            .collect();

        column(rows).spacing(TRACK_ROW_SPACING).into()
    }

    /// Mueve la selección (filas o páginas) y scrollea para mantenerla a la vista.
    pub(crate) fn move_selection(&mut self, step: SelectionStep) -> Task<AlbumMessage> {
        let tracks = self.tracks();
        if tracks.is_empty() {
            return Task::none();
        }

        let row_pitch = NUMBERED_ROW_HEIGHT + TRACK_ROW_SPACING;
        let current = self.selected_id.as_deref().and_then(|id| tracks.iter().position(|t| t.id == id));
        let index = stepped_index(current, step.rows(self.scroll.rows_per_page(row_pitch)), tracks.len());
        let track_count = tracks.len();
        self.selected_id = Some(tracks[index].id.clone());

        let list_top = HEADER_PADDING_TOP + COVER_SIZE + HEADER_PADDING_BOTTOM + SECTION_SPACING;
        let row_top = list_top + index as f32 * row_pitch;

        if step.is_page() {
            let moved_rows = index as f32 - current.unwrap_or(index) as f32;
            let content_height = list_top + track_count as f32 * row_pitch + spacing::SP_32;
            self.scroll.page_and_reveal(moved_rows * row_pitch, content_height, row_top, NUMBERED_ROW_HEIGHT, "album_view_scroll")
        } else {
            self.scroll.reveal(row_top, NUMBERED_ROW_HEIGHT, "album_view_scroll")
        }
    }

    /// Id del track seleccionado, si sigue estando en el álbum.
    pub(crate) fn selected_track_id(&self) -> Option<&str> {
        let id = self.selected_id.as_deref()?;
        self.tracks().iter().any(|t| t.id == id).then_some(id)
    }

    /// Id del álbum mostrado — para cachear/restaurar el scroll por id
    /// (ver `LibraryBrowserFeature::stash_active_route_scroll`).
    pub(crate) fn album_id(&self) -> &str {
        &self.album_id
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

    /// Reemplaza en el lugar el track cuyo id matchea (mismo `Track` que
    /// acaba de terminar de descargarse/analizarse en otra parte de la app,
    /// vía `LibraryBrowserFeature::patch_track`) — sin esto, esta vista
    /// sigue mostrando/reproduciendo el stub congelado que trajo el fetch
    /// inicial del álbum, aunque `CatalogStore` ya tenga el dato fresco.
    pub(crate) fn patch_track(&mut self, track: &Track) {
        if let AlbumViewData::Loaded(album) = &mut self.data
            && let Some(existing) = album.tracks.iter_mut().find(|t| t.id == track.id) {
                *existing = track.clone();
            }
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
    container(text(message.to_string()).font(SF_PRO).size(typography::TEXT_14).color(theme().content.muted))
        .width(Length::Fill)
        .height(Length::Fixed(200.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}
