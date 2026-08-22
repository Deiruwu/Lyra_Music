use std::sync::Arc;

use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::text::Shaping;
use iced::widget::{button, column, container, image, responsive, row, rule, scrollable, space, stack, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Task, Theme};

use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::{AlbumSummary, AlbumType, ArtistDto, Track};
use crate::ui::assets::fonts::{JETBRAINS_MONO, SF_PRO};
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::gallery_thumbnail::{GalleryThumbnail, Treatment};
use crate::ui::widgets::track_row::truncate;
use crate::ui::widgets::track_row_simple::track_row_with_thumbnail;
use crate::utils::formatting::format_views;

const TOP_SONGS_COUNT: usize = 5;
const CARD_THUMBNAIL_SIZE: f32 = 176.0;
const CARD_HOVER_PADDING: f32 = CARD_THUMBNAIL_SIZE * 0.06;
const CARD_HOVER_PADDING_BOTTOM: f32 = CARD_HOVER_PADDING * 1.8;
const CARD_RADIUS: f32 = 8.0;
const CARD_UNIT_WIDTH: f32 = CARD_THUMBNAIL_SIZE + 2.0 * CARD_HOVER_PADDING;
const CARD_SPACING: f32 = 12.0;
const CARD_NAME_MAX_CHARS: usize = 34;
const CARD_NAME_CHARS_PER_LINE: usize = 20;
const CARD_NAME_LINE_HEIGHT: f32 = 18.0;
const CARD_SUBTITLE_LINE_HEIGHT: f32 = 16.0;
const CARD_TEXT_BLOCK_HEIGHT: f32 = 2.0 * CARD_NAME_LINE_HEIGHT + 4.0 + CARD_SUBTITLE_LINE_HEIGHT;
const ARROW_SIZE: f32 = 34.0;
const GALLERY_MAX_SIDE: u32 = 500;
const BANNER_SOURCE_ASPECT_RATIO: f32 = 853.0 / 2048.0;
const BANNER_TOP_CROP_FRACTION: f32 = 0.55;
const BANNER_ASPECT_RATIO: f32 = BANNER_SOURCE_ASPECT_RATIO * BANNER_TOP_CROP_FRACTION;

enum ArtistViewData {
    Loading,
    Loaded(ArtistDto),
    Error(String),
}

pub struct ArtistView {
    artist_id: String,
    client: MicroserviceClient,
    data: ArtistViewData,
    is_followed: bool,
    thumbnails: AsyncThumbnail,
    gallery: GalleryThumbnail,
    albums_page: usize,
    singles_page: usize,
}

#[derive(Debug, Clone)]
pub enum ArtistMessage {
    Loaded(Result<ArtistDto, String>),
    FollowStatusLoaded(bool),
    ThumbnailLoaded(String, Vec<u8>),
    GalleryLoaded(String, Vec<u8>),
    AlbumsPrevPage,
    AlbumsNextPage,
    SinglesPrevPage,
    SinglesNextPage,
    AlbumCardPressed(String),
    TopSongClicked(String),
    TopSongRightClicked(String),
    TopSongArtistPressed(String),
    TopSongAlbumPressed(String),
    FollowPressed,
}

#[derive(Debug, Clone)]
pub enum ArtistOutMessage {
    Idle,
    OpenAlbum(String),
    PlayTopSong(String),
    TrackRightClicked(String),
    OpenTrackArtist(String),
    ToggleFollow(String, String, Option<String>),
}

impl ArtistView {
    pub fn new(
        client: MicroserviceClient,
        artist_id: String,
        followed_artist_manager: Arc<FollowedArtistManager>,
    ) -> (Self, Task<ArtistMessage>) {
        let view = Self {
            artist_id: artist_id.clone(),
            client: client.clone(),
            data: ArtistViewData::Loading,
            is_followed: false,
            thumbnails: AsyncThumbnail::new(),
            gallery: GalleryThumbnail::new(),
            albums_page: 0,
            singles_page: 0,
        };

        let load_task = Task::perform(
            async move { client.artist(&artist_id, Some(TOP_SONGS_COUNT)).await.map_err(|e| e.to_string()) },
            ArtistMessage::Loaded,
        );

        let follow_status_task = Task::perform(
            {
                let artist_id = view.artist_id.clone();
                async move { followed_artist_manager.is_followed(&artist_id).await.unwrap_or(false) }
            },
            ArtistMessage::FollowStatusLoaded,
        );

        (view, Task::batch([load_task, follow_status_task]))
    }

    pub fn update(&mut self, message: ArtistMessage) -> (Task<ArtistMessage>, ArtistOutMessage) {
        let mut out = ArtistOutMessage::Idle;

        match message {
            ArtistMessage::Loaded(Ok(dto)) => self.data = ArtistViewData::Loaded(dto),
            ArtistMessage::Loaded(Err(error)) => self.data = ArtistViewData::Error(error),
            ArtistMessage::FollowStatusLoaded(is_followed) => self.is_followed = is_followed,
            ArtistMessage::ThumbnailLoaded(key, bytes) => self.thumbnails.on_loaded(key, bytes),
            ArtistMessage::GalleryLoaded(key, bytes) => self.gallery.on_loaded(key, bytes),
            ArtistMessage::AlbumsPrevPage => self.albums_page = self.albums_page.saturating_sub(1),
            ArtistMessage::AlbumsNextPage => self.albums_page += 1,
            ArtistMessage::SinglesPrevPage => self.singles_page = self.singles_page.saturating_sub(1),
            ArtistMessage::SinglesNextPage => self.singles_page += 1,
            ArtistMessage::AlbumCardPressed(id) => out = ArtistOutMessage::OpenAlbum(id),
            ArtistMessage::TopSongClicked(id) => out = ArtistOutMessage::PlayTopSong(id),
            ArtistMessage::TopSongRightClicked(id) => out = ArtistOutMessage::TrackRightClicked(id),
            ArtistMessage::TopSongArtistPressed(id) => out = ArtistOutMessage::OpenTrackArtist(id),
            ArtistMessage::TopSongAlbumPressed(id) => out = ArtistOutMessage::OpenAlbum(id),
            ArtistMessage::FollowPressed => {
                self.is_followed = !self.is_followed;
                if let ArtistViewData::Loaded(artist) = &self.data {
                    out = ArtistOutMessage::ToggleFollow(artist.id.clone(), artist.name.clone(), artist.banner.clone());
                }
            }
        }

        let sync_task = self.thumbnails.sync(&self.thumbnail_targets(), ArtistMessage::ThumbnailLoaded);
        let gallery_task = self.gallery.sync(&self.gallery_targets(), ArtistMessage::GalleryLoaded);

        (Task::batch([sync_task, gallery_task]), out)
    }

    pub fn view(&self) -> Element<'_, ArtistMessage> {
        match &self.data {
            ArtistViewData::Loading => status_message("Cargando artista…"),
            ArtistViewData::Error(error) => status_message(error),
            ArtistViewData::Loaded(artist) => {
                let (albums, singles_and_eps) = partition_albums(artist);

                let mut children: Vec<Element<'_, ArtistMessage>> =
                    vec![self.view_header(artist), self.view_top_songs(&artist.songs)];

                if !albums.is_empty() {
                    children.push(self.view_album_section(
                        "Álbumes",
                        albums,
                        self.albums_page,
                        ArtistMessage::AlbumsPrevPage,
                        ArtistMessage::AlbumsNextPage,
                    ));
                }

                if !singles_and_eps.is_empty() {
                    children.push(self.view_album_section(
                        "Singles y EPs",
                        singles_and_eps,
                        self.singles_page,
                        ArtistMessage::SinglesPrevPage,
                        ArtistMessage::SinglesNextPage,
                    ));
                }

                scrollable(
                    column(children)
                        .spacing(28)
                        .padding(Padding { top: 0.0, right: 24.0, bottom: 32.0, left: 24.0 }),
                )
                .width(Length::Fill)
                .into()
            }
        }
    }

    /// Banner de fondo (relación de aspecto fija 2048×853) con el nombre del artista superpuesto.
    fn view_header<'a>(&'a self, artist: &'a ArtistDto) -> Element<'a, ArtistMessage> {
        responsive(move |size| {
            let banner_height = size.width * BANNER_ASPECT_RATIO;

            let background: Element<'a, ArtistMessage> = match self.gallery.get(&banner_key(&artist.id)) {
                Some(handle) => banner_image(handle.clone(), banner_height),
                None => banner_placeholder(banner_height),
            };

            let name = text(artist.name.as_str()).font(SF_PRO).size(44).color(Color::WHITE);

            let views: Element<'a, ArtistMessage> = match artist.views {
                Some(views) => text(format!("{} reproducciones", format_views(views)))
                    .font(SF_PRO)
                    .size(14)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.75))
                    .into(),
                None => space().into(),
            };

            let overlay = row![name, space().width(Length::Fill), views, space().width(12), follow_button(self.is_followed)]
                .align_y(Alignment::End)
                .width(Length::Fill);

            let name = container(overlay)
                .width(Length::Fill)
                .height(Length::Fixed(banner_height))
                .align_y(Alignment::End)
                .padding(24);

            container(stack![background, name])
                .width(Length::Fill)
                .height(Length::Fixed(banner_height))
                .clip(true)
                .into()
        })
        .into()
    }

    /// Top 5 canciones del artista, fila simple sin `TrackBuilder`, con thumbnail.
    fn view_top_songs<'a>(&'a self, songs: &'a [Track]) -> Element<'a, ArtistMessage> {
        let rows: Vec<Element<'a, ArtistMessage>> = songs
            .iter()
            .take(TOP_SONGS_COUNT)
            .map(|track| {
                let thumbnail = self.thumbnails.get(&thumb_key(track)).cloned();
                track_row_with_thumbnail(
                    track,
                    thumbnail,
                    ArtistMessage::TopSongClicked(track.id.clone()),
                    ArtistMessage::TopSongRightClicked(track.id.clone()),
                    ArtistMessage::TopSongArtistPressed,
                    ArtistMessage::TopSongAlbumPressed,
                )
            })
            .collect();

        column![section_title("Top canciones"), column(rows).spacing(4)]
            .spacing(12)
            .into()
    }

    /// Título + separador + flechas, y el carrusel paginado de álbumes/singles/EPs. Nunca se llama con `items` vacío.
    fn view_album_section<'a>(
        &'a self,
        title: &'static str,
        items: Vec<&'a AlbumSummary>,
        page: usize,
        on_prev: ArtistMessage,
        on_next: ArtistMessage,
    ) -> Element<'a, ArtistMessage> {
        responsive(move |size| {
            let per_page = albums_per_page(size.width);
            let total_pages = items.len().div_ceil(per_page);
            let page = page.min(total_pages.saturating_sub(1));

            let divider = rule::horizontal(1.0).style(|_theme: &Theme| rule::Style {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: false,
            });

            let header = row![
                section_title(title),
                divider,
                Self::carousel_arrow(Icon::LeftArrow, (page > 0).then_some(on_prev.clone())),
                Self::carousel_arrow(Icon::RightArrow, (page + 1 < total_pages).then_some(on_next.clone())),
            ]
            .spacing(16)
            .align_y(Alignment::Center);

            let start = page * per_page;
            let end = ((page + 1) * per_page).min(items.len());
            let visible_count = end - start;

            let leftover = (size.width - visible_count as f32 * CARD_UNIT_WIDTH).max(0.0);
            let gap = if visible_count > 1 && leftover < CARD_UNIT_WIDTH {
                leftover / (visible_count - 1) as f32
            } else {
                CARD_SPACING
            };

            let cards: Vec<Element<'a, ArtistMessage>> = items[start..end]
                .iter()
                .map(|item| self.view_album_card(item))
                .collect();

            column![header, row(cards).spacing(gap)].spacing(12).into()
        })
        .into()
    }

    /// Tarjeta individual: thumbnail grande arriba, nombre a 2 líneas fijas, "Año · Tipo" debajo. Clic → abre el álbum.
    fn view_album_card<'a>(&'a self, item: &'a AlbumSummary) -> Element<'a, ArtistMessage> {
        let thumbnail: Element<'a, ArtistMessage> = match self.gallery.get(&item.id) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(CARD_RADIUS)
                .into(),
            None => container(space())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.18, 0.18).into()),
                    border: rounded(CARD_RADIUS),
                    ..Default::default()
                })
                .into(),
        };

        let display_name = truncate(item.name.as_str(), CARD_NAME_MAX_CHARS);
        let name_lines = if display_name.chars().count() > CARD_NAME_CHARS_PER_LINE { 2.0 } else { 1.0 };

        let name = container(text(display_name).font(SF_PRO).size(14).color(Color::WHITE))
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(name_lines * CARD_NAME_LINE_HEIGHT))
            .clip(true);

        let subtitle = text(format!(
            "{} · {}",
            item.year.as_deref().unwrap_or("—"),
            item.album_type.label(),
        ))
        .font(SF_PRO)
        .size(12)
        .color(Color::from_rgb(0.6, 0.6, 0.65))
        .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
        .height(Length::Fixed(CARD_SUBTITLE_LINE_HEIGHT));

        let text_block = column![name, subtitle]
            .spacing(4)
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_TEXT_BLOCK_HEIGHT));
        let content = column![thumbnail, text_block].spacing(6);

        button(content)
            .padding(Padding {
                top: CARD_HOVER_PADDING,
                right: CARD_HOVER_PADDING,
                bottom: CARD_HOVER_PADDING_BOTTOM,
                left: CARD_HOVER_PADDING,
            })
            .style(card_hover_style)
            .on_press(ArtistMessage::AlbumCardPressed(item.id.clone()))
            .into()
    }

    /// Botón circular con flecha centrada; sin `on_press` cuando `on_press` es `None`.
    fn carousel_arrow(icon: Icon, on_press: Option<ArtistMessage>) -> Element<'static, ArtistMessage> {
        let glyph = container(
            text(icon.as_str()).font(JETBRAINS_MONO).shaping(Shaping::Advanced).size(14),
        )
        .width(Length::Fixed(ARROW_SIZE))
        .height(Length::Fixed(ARROW_SIZE))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

        let enabled = on_press.is_some();

        let mut arrow_button = button(glyph).padding(0).style(move |_theme: &Theme, status| {
            let bg = if !enabled {
                Color::from_rgba(1.0, 1.0, 1.0, 0.04)
            } else {
                match status {
                    button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.18),
                    _ => Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                }
            };
            button::Style {
                background: Some(bg.into()),
                text_color: if enabled { Color::WHITE } else { Color::from_rgba(1.0, 1.0, 1.0, 0.3) },
                border: rounded(ARROW_SIZE / 2.0),
                ..Default::default()
            }
        });

        if let Some(message) = on_press {
            arrow_button = arrow_button.on_press(message);
        }

        arrow_button.into()
    }

    /// Top canciones ya truncadas a `TOP_SONGS_COUNT`, para armar `play_context`.
    pub(crate) fn top_songs(&self) -> Vec<Track> {
        let ArtistViewData::Loaded(artist) = &self.data else { return Vec::new() };
        artist.songs.iter().take(TOP_SONGS_COUNT).cloned().collect()
    }

    /// Busca una canción del top por id (para armar el menú contextual).
    pub(crate) fn find_song(&self, id: &str) -> Option<&Track> {
        let ArtistViewData::Loaded(artist) = &self.data else { return None };
        artist.songs.iter().take(TOP_SONGS_COUNT).find(|t| t.id == id)
    }

    /// URLs de thumbnails cuadrados a mantener vivos: top 5 canciones.
    fn thumbnail_targets(&self) -> Vec<(String, String)> {
        let ArtistViewData::Loaded(artist) = &self.data else { return Vec::new() };

        artist.songs.iter()
            .take(TOP_SONGS_COUNT)
            .filter_map(|track| track.thumbnail_small.clone().map(|url| (thumb_key(track), url)))
            .collect()
    }

    /// Banner (recortado a su mitad superior) + portadas de toda la discografía (con tope de tamaño).
    fn gallery_targets(&self) -> Vec<(String, String, Treatment)> {
        let ArtistViewData::Loaded(artist) = &self.data else { return Vec::new() };

        let mut targets = Vec::new();

        if let Some(url) = &artist.banner {
            targets.push((banner_key(&artist.id), url.clone(), Treatment::TopCrop(BANNER_TOP_CROP_FRACTION)));
        }

        for item in &artist.albums {
            if let Some(url) = &item.thumbnail_large {
                targets.push((item.id.clone(), url.clone(), Treatment::MaxSide(GALLERY_MAX_SIDE)));
            }
        }

        targets
    }
}

/// Botón chico "Seguir"/"Siguiendo" junto al nombre del artista en el banner.
fn follow_button(is_followed: bool) -> Element<'static, ArtistMessage> {
    let label = if is_followed { "Siguiendo" } else { "Seguir" };

    let (idle, hovered) = if is_followed {
        (Color::from_rgba(1.0, 1.0, 1.0, 0.22), Color::from_rgba(1.0, 1.0, 1.0, 0.28))
    } else {
        (Color::from_rgba(1.0, 1.0, 1.0, 0.08), Color::from_rgba(1.0, 1.0, 1.0, 0.14))
    };

    button(text(label).font(SF_PRO).size(13).color(Color::WHITE))
        .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(match status {
                button::Status::Hovered => hovered,
                _ => idle,
            }.into()),
            text_color: Color::WHITE,
            border: rounded(14.0),
            ..Default::default()
        })
        .on_press(ArtistMessage::FollowPressed)
        .into()
}

fn card_hover_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: Color::WHITE,
        border: rounded(CARD_RADIUS),
        ..Default::default()
    }
}

/// Cuántas tarjetas de `CARD_UNIT_WIDTH` (+ `CARD_SPACING` entre ellas) caben en `width`.
fn albums_per_page(width: f32) -> usize {
    let unit = CARD_UNIT_WIDTH + CARD_SPACING;
    (((width + CARD_SPACING) / unit).floor() as usize).max(1)
}

fn banner_key(artist_id: &str) -> String {
    format!("artist_banner:{artist_id}")
}

/// Separa la discografía del artista en (álbumes, singles + EPs).
fn partition_albums(artist: &ArtistDto) -> (Vec<&AlbumSummary>, Vec<&AlbumSummary>) {
    artist.albums.iter().partition(|item| item.album_type == AlbumType::Album)
}

fn banner_image<'a, Message: 'a>(handle: Handle, height: f32) -> Element<'a, Message> {
    container(
        image(handle)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .opacity(0.8),
    )
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .clip(true)
    .style(|_theme: &Theme| container::Style {
        background: Some(Color::BLACK.into()),
        ..Default::default()
    })
    .into()
}

fn banner_placeholder<'a, Message: 'a>(height: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgb(0.14, 0.14, 0.16).into()),
            ..Default::default()
        })
        .into()
}

fn status_message(message: &str) -> Element<'_, ArtistMessage> {
    container(text(message.to_string()).font(SF_PRO).size(14).color(Color::from_rgb(0.6, 0.6, 0.65)))
        .width(Length::Fill)
        .height(Length::Fixed(200.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn section_title<'a, Message: 'a>(title: &'a str) -> Element<'a, Message> {
    text(title).font(SF_PRO).size(18).color(Color::WHITE).into()
}
