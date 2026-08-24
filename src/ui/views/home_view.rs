use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use futures::future::join_all;
use iced::border::rounded;
use iced::widget::text::Shaping;
use iced::widget::{button, column, container, image, mouse_area, responsive, row, rule, scrollable, space, text};
use iced::{Alignment, Border, Color, ContentFit, Element, Length, Padding, Task, Theme};

use crate::JETBRAINS_MONO;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::artist_links::{album_link, artist_links};
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::truncate;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Home,
    Icon::Home,
    "Home",
    JETBRAINS_MONO,
);

const WINDOW_DAYS: i64 = 14;
const TRACK_POOL_LIMIT: i64 = 60;
const RECENT_CARDS_SHOWN: usize = 8;
const TOP_ARTISTS_LIMIT: usize = 12;
// Margen de candidatos para "Top artistas": algunos ids no tienen perfil de
// artista musical en YT Music (canal común, no músico) y su fetch falla —
// se saltean y se sigue con el siguiente candidato en orden de ranking.
const ARTIST_CANDIDATE_LIMIT: usize = TOP_ARTISTS_LIMIT * 3;
const TOP_ALBUMS_LIMIT: usize = 12;

// Carruseles paginados de "Top artistas"/"Top álbumes" (mismo formato que
// artist_view::view_album_section: artistas circulares, álbumes cuadrados).
const CARD_THUMBNAIL_SIZE: f32 = 156.0;
const CARD_HOVER_PADDING: f32 = CARD_THUMBNAIL_SIZE * 0.06;
const CARD_HOVER_PADDING_BOTTOM: f32 = CARD_HOVER_PADDING * 1.8;
const CARD_UNIT_WIDTH: f32 = CARD_THUMBNAIL_SIZE + 2.0 * CARD_HOVER_PADDING;
const CARD_SPACING: f32 = 12.0;
const CARD_NAME_MAX_CHARS: usize = 20;
const CARD_NAME_LINE_HEIGHT: f32 = 18.0;
const CARD_SUBTITLE_LINE_HEIGHT: f32 = 15.0;
const ALBUM_CARD_RADIUS: f32 = 8.0;
const ARROW_SIZE: f32 = 34.0;

// Fichas de "Escuchar ahora" (versión grande del widget de canción actual del reproductor).
const BANNER_THUMBNAIL_SIZE: f32 = 64.0;
const RECENT_CARDS_PER_ROW: usize = 4;
const BANNER_CARD_SPACING: f32 = 12.0;
const BANNER_CARD_RADIUS: f32 = 10.0;

#[derive(Debug, Clone)]
pub struct TopArtistCard {
    artist_id: String,
    name: String,
    photo_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TopAlbumCard {
    album_id: String,
    album_name: String,
    thumbnail_url: Option<String>,
    play_count: i64,
}

struct HomeLoadResult {
    top_tracks: Vec<Track>,
    top_artists: Vec<TopArtistCard>,
    top_albums: Vec<TopAlbumCard>,
}

#[derive(Debug, Clone)]
pub enum HomeViewMessage {
    HomeDataLoaded(Result<(Vec<Track>, Vec<TopArtistCard>, Vec<TopAlbumCard>), String>),
    ThumbnailLoaded(String, Vec<u8>),
    TopTrackClicked(String),
    TopTrackRightClicked(String),
    TopTrackArtistClicked(String),
    TopTrackAlbumClicked(String),
    TopArtistClicked(String),
    TopArtistsPrevPage,
    TopArtistsNextPage,
    TopAlbumClicked(String),
    TopAlbumsPrevPage,
    TopAlbumsNextPage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HomeViewOutMessage {
    Idle,
    PlayTrack(String),
    OpenArtist(String),
    OpenAlbum(String),
}

pub struct HomeView {
    top_tracks: Vec<Track>,
    top_artists: Vec<TopArtistCard>,
    top_albums: Vec<TopAlbumCard>,
    top_artists_page: usize,
    top_albums_page: usize,
    thumbnails: AsyncThumbnail,
}

impl HomeView {
    pub fn new(
        client: Arc<MicroserviceClient>,
        play_history: Arc<PlayHistoryManager>,
    ) -> (Self, Task<HomeViewMessage>) {
        let view = Self {
            top_tracks: Vec::new(),
            top_artists: Vec::new(),
            top_albums: Vec::new(),
            top_artists_page: 0,
            top_albums_page: 0,
            thumbnails: AsyncThumbnail::new(),
        };

        let load_task = Task::perform(
            async move {
                let result = load_home_data(client, play_history).await?;
                Ok((result.top_tracks, result.top_artists, result.top_albums))
            },
            HomeViewMessage::HomeDataLoaded,
        );

        (view, load_task)
    }

    pub fn top_tracks(&self) -> &[Track] {
        &self.top_tracks
    }

    pub fn update(&mut self, message: HomeViewMessage) -> (Task<HomeViewMessage>, HomeViewOutMessage) {
        let mut out = HomeViewOutMessage::Idle;

        match message {
            HomeViewMessage::HomeDataLoaded(Ok((tracks, artists, albums))) => {
                self.top_tracks = tracks;
                self.top_artists = artists;
                self.top_albums = albums;
            }
            HomeViewMessage::HomeDataLoaded(Err(_)) => {}
            HomeViewMessage::ThumbnailLoaded(key, bytes) => self.thumbnails.on_loaded(key, bytes),
            HomeViewMessage::TopTrackClicked(id) => out = HomeViewOutMessage::PlayTrack(id),
            HomeViewMessage::TopTrackRightClicked(_) => {}
            HomeViewMessage::TopTrackArtistClicked(id) => out = HomeViewOutMessage::OpenArtist(id),
            HomeViewMessage::TopTrackAlbumClicked(id) => out = HomeViewOutMessage::OpenAlbum(id),
            HomeViewMessage::TopArtistClicked(id) => out = HomeViewOutMessage::OpenArtist(id),
            HomeViewMessage::TopArtistsPrevPage => self.top_artists_page = self.top_artists_page.saturating_sub(1),
            HomeViewMessage::TopArtistsNextPage => self.top_artists_page += 1,
            HomeViewMessage::TopAlbumClicked(id) => out = HomeViewOutMessage::OpenAlbum(id),
            HomeViewMessage::TopAlbumsPrevPage => self.top_albums_page = self.top_albums_page.saturating_sub(1),
            HomeViewMessage::TopAlbumsNextPage => self.top_albums_page += 1,
        }

        let sync_task = self.thumbnails.sync(&self.thumbnail_targets(), HomeViewMessage::ThumbnailLoaded);
        (sync_task, out)
    }

    pub fn view(&self) -> Element<'_, HomeViewMessage> {
        if self.top_tracks.is_empty() && self.top_artists.is_empty() && self.top_albums.is_empty() {
            return status_message("Todavía no hay nada por acá — arrancá escuchando algo.");
        }

        let mut children: Vec<Element<'_, HomeViewMessage>> = Vec::new();

        if !self.top_tracks.is_empty() {
            children.push(self.view_top_tracks());
        }

        if !self.top_artists.is_empty() {
            children.push(view_carousel(
                "Top artistas",
                &self.top_artists,
                self.top_artists_page,
                HomeViewMessage::TopArtistsPrevPage,
                HomeViewMessage::TopArtistsNextPage,
                |artist| self.view_top_artist_card(artist),
            ));
        }

        if !self.top_albums.is_empty() {
            children.push(view_carousel(
                "Top álbumes",
                &self.top_albums,
                self.top_albums_page,
                HomeViewMessage::TopAlbumsPrevPage,
                HomeViewMessage::TopAlbumsNextPage,
                |album| self.view_top_album_card(album),
            ));
        }

        scrollable(
            column(children)
                .spacing(28)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 32.0, left: 24.0 }),
        )
        .width(Length::Fill)
        .into()
    }

    /// Fichas grandes tipo banner (versión ampliada del widget de canción actual del
    /// reproductor), hasta `RECENT_CARDS_SHOWN` en una grilla de `RECENT_CARDS_PER_ROW` columnas fijas.
    fn view_top_tracks(&self) -> Element<'_, HomeViewMessage> {
        let shown = self.top_tracks.len().min(RECENT_CARDS_SHOWN);
        let rows: Vec<Element<'_, HomeViewMessage>> = self.top_tracks[..shown]
            .chunks(RECENT_CARDS_PER_ROW)
            .map(|chunk| {
                row(chunk.iter().map(|track| self.view_top_track_card(track)).collect::<Vec<_>>())
                    .spacing(BANNER_CARD_SPACING)
                    .into()
            })
            .collect();

        column![section_title("Escuchar ahora"), column(rows).spacing(BANNER_CARD_SPACING)]
            .spacing(12)
            .into()
    }

    /// Ficha individual: thumbnail grande a la izquierda, título/artista/álbum apilados a la
    /// derecha, con borde claro. Clic izquierdo reproduce, clic derecho abre el menú contextual.
    fn view_top_track_card<'a>(&'a self, track: &'a Track) -> Element<'a, HomeViewMessage> {
        let thumbnail_state = match self.thumbnails.get(&thumb_key(track)).cloned() {
            Some(handle) => ThumbnailState::Loaded(handle),
            None => ThumbnailState::Loading,
        };
        let thumb = async_thumbnail(thumbnail_state, BANNER_THUMBNAIL_SIZE, BANNER_THUMBNAIL_SIZE / 2.0);

        let title = single_line_text(track.title.as_str(), SF_PRO, 14.0, Color::WHITE, Length::Fill);

        let artist = artist_links(
            &track.artists,
            SF_PRO,
            12.0,
            Color::from_rgb(0.7, 0.7, 0.75),
            Length::Fill,
            HomeViewMessage::TopTrackArtistClicked,
        );

        let album = album_link(
            track.album.as_ref(),
            SF_PRO,
            12.0,
            Color::from_rgb(0.55, 0.55, 0.6),
            Length::Fill,
            HomeViewMessage::TopTrackAlbumClicked,
        );

        let info = column![title, artist, album].spacing(3).width(Length::Fill);

        let content = row![thumb, info].spacing(12).align_y(Alignment::Center);

        let card = button(content)
            .width(Length::FillPortion(1))
            .padding(10)
            .style(|_theme: &Theme, status| {
                let hovered = status == button::Status::Hovered;
                button::Style {
                    background: Some(Color::from_rgba(1.0, 1.0, 1.0, if hovered { 0.05 } else { 0.025 }).into()),
                    text_color: Color::WHITE,
                    border: Border {
                        radius: BANNER_CARD_RADIUS.into(),
                        color: Color::from_rgba(1.0, 1.0, 1.0, if hovered { 0.16 } else { 0.10 }),
                        width: 1.0,
                    },
                    ..Default::default()
                }
            })
            .on_press(HomeViewMessage::TopTrackClicked(track.id.clone()));

        mouse_area(card).on_right_press(HomeViewMessage::TopTrackRightClicked(track.id.clone())).into()
    }

    fn view_top_artist_card<'a>(&'a self, artist: &'a TopArtistCard) -> Element<'a, HomeViewMessage> {
        let thumbnail: Element<'a, HomeViewMessage> = match self.thumbnails.get(&top_artist_key(&artist.artist_id)) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(CARD_THUMBNAIL_SIZE / 2.0)
                .into(),
            None => container(space())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.18, 0.18).into()),
                    border: rounded(CARD_THUMBNAIL_SIZE / 2.0),
                    ..Default::default()
                })
                .into(),
        };

        let display_name = truncate(artist.name.as_str(), CARD_NAME_MAX_CHARS);

        let name = text(display_name)
            .font(SF_PRO)
            .size(13)
            .color(Color::WHITE)
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_NAME_LINE_HEIGHT))
            .align_x(Alignment::Center);

        let content = column![thumbnail, name].spacing(8).align_x(Alignment::Center);

        button(content)
            .padding(Padding {
                top: CARD_HOVER_PADDING,
                right: CARD_HOVER_PADDING,
                bottom: CARD_HOVER_PADDING_BOTTOM,
                left: CARD_HOVER_PADDING,
            })
            .style(card_hover_style)
            .on_press(HomeViewMessage::TopArtistClicked(artist.artist_id.clone()))
            .into()
    }

    fn view_top_album_card<'a>(&'a self, album: &'a TopAlbumCard) -> Element<'a, HomeViewMessage> {
        let thumbnail: Element<'a, HomeViewMessage> = match self.thumbnails.get(&top_album_key(&album.album_id)) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(ALBUM_CARD_RADIUS)
                .into(),
            None => container(space())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.18, 0.18).into()),
                    border: rounded(ALBUM_CARD_RADIUS),
                    ..Default::default()
                })
                .into(),
        };

        let display_name = truncate(album.album_name.as_str(), CARD_NAME_MAX_CHARS);

        let name = container(text(display_name).font(SF_PRO).size(13).color(Color::WHITE))
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_NAME_LINE_HEIGHT))
            .clip(true);

        let subtitle = text(format!("{} reproducciones", album.play_count))
            .font(SF_PRO)
            .size(12)
            .color(Color::from_rgb(0.6, 0.6, 0.65))
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_SUBTITLE_LINE_HEIGHT));

        let text_block = column![name, subtitle].spacing(4).width(Length::Fixed(CARD_THUMBNAIL_SIZE));
        let content = column![thumbnail, text_block].spacing(8);

        button(content)
            .padding(Padding {
                top: CARD_HOVER_PADDING,
                right: CARD_HOVER_PADDING,
                bottom: CARD_HOVER_PADDING_BOTTOM,
                left: CARD_HOVER_PADDING,
            })
            .style(card_hover_style)
            .on_press(HomeViewMessage::TopAlbumClicked(album.album_id.clone()))
            .into()
    }

    fn thumbnail_targets(&self) -> Vec<(String, String)> {
        let mut targets: Vec<(String, String)> = self.top_tracks
            .iter()
            .take(RECENT_CARDS_SHOWN)
            .filter_map(|track| track.thumbnail_small.clone().map(|url| (thumb_key(track), url)))
            .collect();

        targets.extend(self.top_artists.iter().filter_map(|artist| {
            artist.photo_url.clone().map(|url| (top_artist_key(&artist.artist_id), url))
        }));

        targets.extend(self.top_albums.iter().filter_map(|album| {
            album.thumbnail_url.clone().map(|url| (top_album_key(&album.album_id), url))
        }));

        targets
    }
}

/// Pide el pool de tracks más escuchados en `WINDOW_DAYS`, y deriva de ahí (sin más
/// queries) tanto el top de canciones como los agregados de artistas y álbumes.
async fn load_home_data(
    client: Arc<MicroserviceClient>,
    play_history: Arc<PlayHistoryManager>,
) -> Result<HomeLoadResult, String> {
    let since = (Utc::now() - Duration::days(WINDOW_DAYS)).naive_utc();

    let pool = play_history.top_tracks_recent(since, TRACK_POOL_LIMIT).await.map_err(|e| e.to_string())?;
    let ordered_ids: Vec<String> = pool.iter().map(|p| p.track_id.clone()).collect();
    let play_counts: HashMap<String, i64> = pool.into_iter().map(|p| (p.track_id, p.play_count)).collect();

    let resolved = client.resolve_many(&ordered_ids).await.map_err(|e| e.to_string())?;
    let mut by_id: HashMap<String, Track> = resolved.into_iter().map(|t| (t.id.clone(), t)).collect();
    let tracks: Vec<Track> = ordered_ids.iter().filter_map(|id| by_id.remove(id)).collect();

    let mut albums: HashMap<String, TopAlbumCard> = HashMap::new();
    let mut artists: HashMap<String, (String, i64)> = HashMap::new();

    for track in &tracks {
        let count = play_counts.get(&track.id).copied().unwrap_or(0);

        if let Some(album) = &track.album {
            let entry = albums.entry(album.id.clone()).or_insert_with(|| TopAlbumCard {
                album_id: album.id.clone(),
                album_name: album.name.clone(),
                thumbnail_url: track.thumbnail_large.clone().or_else(|| track.thumbnail_small.clone()),
                play_count: 0,
            });
            entry.play_count += count;
        }

        for artist in &track.artists {
            let Some(artist_id) = artist.id.clone() else { continue };
            let entry = artists.entry(artist_id).or_insert_with(|| (artist.name.clone(), 0));
            entry.1 += count;
        }
    }

    let mut top_albums: Vec<TopAlbumCard> = albums.into_values().collect();
    top_albums.sort_by(|a, b| b.play_count.cmp(&a.play_count));
    top_albums.truncate(TOP_ALBUMS_LIMIT);

    let mut ranked_artists: Vec<(String, String, i64)> = artists
        .into_iter()
        .map(|(id, (name, count))| (id, name, count))
        .collect();
    ranked_artists.sort_by(|a, b| b.2.cmp(&a.2));
    ranked_artists.truncate(ARTIST_CANDIDATE_LIMIT);

    // Algunos ids no tienen perfil de artista musical en YT Music y
    // artist_profile falla — esos se saltean (None) en vez de mostrarse sin
    // foto, así el siguiente candidato en el ranking ocupa su lugar.
    let resolved_candidates = join_all(ranked_artists.into_iter().map(|(artist_id, name, _count)| {
        let client = Arc::clone(&client);
        async move {
            client.artist_profile(&artist_id).await.ok().map(|profile| TopArtistCard {
                artist_id,
                name,
                photo_url: profile.thumbnail_large.or(profile.thumbnail_small),
            })
        }
    }))
    .await;

    let top_artists: Vec<TopArtistCard> = resolved_candidates.into_iter().flatten().take(TOP_ARTISTS_LIMIT).collect();

    let top_tracks = tracks.into_iter().take(RECENT_CARDS_SHOWN).collect();

    Ok(HomeLoadResult { top_tracks, top_artists, top_albums })
}

/// Header (título + separador + flechas) + fila de tarjetas de un carrusel paginado.
/// Compartido por "Top artistas" (circular) y "Top álbumes" (cuadrado) — misma
/// paginación/distribución, solo difiere la tarjeta que arma `card_fn`.
fn view_carousel<'a, T>(
    title: &'static str,
    items: &'a [T],
    page: usize,
    on_prev: HomeViewMessage,
    on_next: HomeViewMessage,
    card_fn: impl Fn(&'a T) -> Element<'a, HomeViewMessage> + 'a,
) -> Element<'a, HomeViewMessage> {
    responsive(move |size| {
        let per_page = cards_per_page(size.width);
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
            carousel_arrow(Icon::LeftArrow, (page > 0).then_some(on_prev.clone())),
            carousel_arrow(Icon::RightArrow, (page + 1 < total_pages).then_some(on_next.clone())),
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

        let cards: Vec<Element<'a, HomeViewMessage>> = items[start..end].iter().map(&card_fn).collect();

        column![header, row(cards).spacing(gap)].spacing(12).into()
    })
    .into()
}

/// Botón circular con flecha centrada; sin `on_press` cuando `on_press` es `None`.
fn carousel_arrow(icon: Icon, on_press: Option<HomeViewMessage>) -> Element<'static, HomeViewMessage> {
    let glyph = container(text(icon.as_str()).font(JETBRAINS_MONO).shaping(Shaping::Advanced).size(14))
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

fn card_hover_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        _ => None,
    };
    button::Style { background, text_color: Color::WHITE, border: rounded(12.0), ..Default::default() }
}

/// Cuántas tarjetas de `CARD_UNIT_WIDTH` (+ `CARD_SPACING` entre ellas) caben en `width`.
fn cards_per_page(width: f32) -> usize {
    let unit = CARD_UNIT_WIDTH + CARD_SPACING;
    (((width + CARD_SPACING) / unit).floor() as usize).max(1)
}

fn top_artist_key(artist_id: &str) -> String {
    format!("top_artist:{artist_id}")
}

fn top_album_key(album_id: &str) -> String {
    format!("top_album:{album_id}")
}

fn section_title<'a, Message: 'a>(title: &'a str) -> Element<'a, Message> {
    text(title).font(SF_PRO).size(18).color(Color::WHITE).into()
}

fn status_message(message: &str) -> Element<'_, HomeViewMessage> {
    container(text(message.to_string()).font(SF_PRO).size(14).color(Color::from_rgb(0.6, 0.6, 0.65)))
        .width(Length::Fill)
        .height(Length::Fixed(200.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}
