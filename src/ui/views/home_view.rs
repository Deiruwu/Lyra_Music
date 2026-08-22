use std::convert::Into;
use std::sync::Arc;

use iced::widget::{button, column, container, image, row, scrollable, space, text};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding, Task, Theme};

use crate::JETBRAINS_MONO;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::{FollowedArtist, Track};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::track_row_simple::track_row_with_thumbnail;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Home,
    Icon::Home,
    "Home",
    JETBRAINS_MONO,
);

const RECENT_LIMIT: i64 = 20;
const FOLLOWED_LIMIT: i64 = 20;
const ARTIST_CARD_SIZE: f32 = 140.0;

#[derive(Debug, Clone)]
pub enum HomeViewMessage {
    RecentTracksLoaded(Result<Vec<Track>, String>),
    FollowedArtistsLoaded(Result<Vec<FollowedArtist>, String>),
    ThumbnailLoaded(String, Vec<u8>),
    RecentTrackClicked(String),
    RecentTrackRightClicked(String),
    RecentTrackArtistClicked(String),
    RecentTrackAlbumClicked(String),
    FollowedArtistClicked(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HomeViewOutMessage {
    Idle,
    PlayTrack(String),
    OpenArtist(String),
    OpenAlbum(String),
}

pub struct HomeView {
    recent_tracks: Vec<Track>,
    followed_artists: Vec<FollowedArtist>,
    thumbnails: AsyncThumbnail,
}

impl HomeView {
    pub fn new(
        client: Arc<MicroserviceClient>,
        play_history: Arc<PlayHistoryManager>,
        followed_artists: Arc<FollowedArtistManager>,
    ) -> (Self, Task<HomeViewMessage>) {
        let view = Self {
            recent_tracks: Vec::new(),
            followed_artists: Vec::new(),
            thumbnails: AsyncThumbnail::new(),
        };

        let recent_task = Task::perform(
            async move {
                let recents = play_history.recent_plays(RECENT_LIMIT).await.map_err(|e| e.to_string())?;
                let ids: Vec<String> = recents.into_iter().map(|r| r.track_id).collect();
                let tracks = client.resolve_many(&ids).await.map_err(|e| e.to_string())?;

                let mut by_id: std::collections::HashMap<String, Track> =
                    tracks.into_iter().map(|t| (t.id.clone(), t)).collect();
                Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
            },
            HomeViewMessage::RecentTracksLoaded,
        );

        let followed_task = Task::perform(
            async move { followed_artists.list_followed(FOLLOWED_LIMIT).await.map_err(|e| e.to_string()) },
            HomeViewMessage::FollowedArtistsLoaded,
        );

        (view, Task::batch([recent_task, followed_task]))
    }

    pub fn recent_tracks(&self) -> &[Track] {
        &self.recent_tracks
    }

    pub fn update(&mut self, message: HomeViewMessage) -> (Task<HomeViewMessage>, HomeViewOutMessage) {
        let mut out = HomeViewOutMessage::Idle;

        match message {
            HomeViewMessage::RecentTracksLoaded(Ok(tracks)) => self.recent_tracks = tracks,
            HomeViewMessage::RecentTracksLoaded(Err(_)) => {}
            HomeViewMessage::FollowedArtistsLoaded(Ok(artists)) => self.followed_artists = artists,
            HomeViewMessage::FollowedArtistsLoaded(Err(_)) => {}
            HomeViewMessage::ThumbnailLoaded(key, bytes) => self.thumbnails.on_loaded(key, bytes),
            HomeViewMessage::RecentTrackClicked(id) => out = HomeViewOutMessage::PlayTrack(id),
            HomeViewMessage::RecentTrackRightClicked(_) => {}
            HomeViewMessage::RecentTrackArtistClicked(id) => out = HomeViewOutMessage::OpenArtist(id),
            HomeViewMessage::RecentTrackAlbumClicked(id) => out = HomeViewOutMessage::OpenAlbum(id),
            HomeViewMessage::FollowedArtistClicked(id) => out = HomeViewOutMessage::OpenArtist(id),
        }

        let sync_task = self.thumbnails.sync(&self.thumbnail_targets(), HomeViewMessage::ThumbnailLoaded);
        (sync_task, out)
    }

    pub fn view(&self) -> Element<'_, HomeViewMessage> {
        if self.recent_tracks.is_empty() && self.followed_artists.is_empty() {
            return status_message("Todavía no hay nada por acá — arrancá escuchando algo.");
        }

        let mut children: Vec<Element<'_, HomeViewMessage>> = Vec::new();

        if !self.recent_tracks.is_empty() {
            children.push(self.view_recent_tracks());
        }

        if !self.followed_artists.is_empty() {
            children.push(self.view_followed_artists());
        }

        scrollable(
            column(children)
                .spacing(28)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 32.0, left: 24.0 }),
        )
        .width(Length::Fill)
        .into()
    }

    fn view_recent_tracks(&self) -> Element<'_, HomeViewMessage> {
        let rows: Vec<Element<'_, HomeViewMessage>> = self.recent_tracks
            .iter()
            .map(|track| {
                let thumbnail = self.thumbnails.get(&thumb_key(track)).cloned();
                track_row_with_thumbnail(
                    track,
                    thumbnail,
                    HomeViewMessage::RecentTrackClicked(track.id.clone()),
                    HomeViewMessage::RecentTrackRightClicked(track.id.clone()),
                    HomeViewMessage::RecentTrackArtistClicked,
                    HomeViewMessage::RecentTrackAlbumClicked,
                )
            })
            .collect();

        column![section_title("Escuchado recientemente"), column(rows).spacing(4)]
            .spacing(12)
            .into()
    }

    fn view_followed_artists(&self) -> Element<'_, HomeViewMessage> {
        let cards: Vec<Element<'_, HomeViewMessage>> = self.followed_artists
            .iter()
            .map(|artist| self.view_followed_artist_card(artist))
            .collect();

        column![
            section_title("Artistas seguidos"),
            scrollable(row(cards).spacing(16))
                .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4))),
        ]
        .spacing(12)
        .into()
    }

    fn view_followed_artist_card<'a>(&'a self, artist: &'a FollowedArtist) -> Element<'a, HomeViewMessage> {
        let thumbnail: Element<'a, HomeViewMessage> = match self.thumbnails.get(&followed_artist_key(&artist.artist_id)) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(ARTIST_CARD_SIZE))
                .height(Length::Fixed(ARTIST_CARD_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(ARTIST_CARD_SIZE / 2.0)
                .into(),
            None => container(space())
                .width(Length::Fixed(ARTIST_CARD_SIZE))
                .height(Length::Fixed(ARTIST_CARD_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.18, 0.18).into()),
                    border: iced::border::rounded(ARTIST_CARD_SIZE / 2.0),
                    ..Default::default()
                })
                .into(),
        };

        let name = text(artist.name.as_str())
            .font(SF_PRO)
            .size(13)
            .color(Color::WHITE)
            .width(Length::Fixed(ARTIST_CARD_SIZE))
            .align_x(Alignment::Center);

        button(column![thumbnail, name].spacing(8).align_x(Alignment::Center))
            .padding(6)
            .style(|_theme: &Theme, status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                    _ => None,
                },
                text_color: Color::WHITE,
                border: iced::border::rounded(8),
                ..Default::default()
            })
            .on_press(HomeViewMessage::FollowedArtistClicked(artist.artist_id.clone()))
            .into()
    }

    fn thumbnail_targets(&self) -> Vec<(String, String)> {
        let mut targets: Vec<(String, String)> = self.recent_tracks
            .iter()
            .filter_map(|track| track.thumbnail_small.clone().map(|url| (thumb_key(track), url)))
            .collect();

        targets.extend(self.followed_artists.iter().filter_map(|artist| {
            artist.photo_url.clone().map(|url| (followed_artist_key(&artist.artist_id), url))
        }));

        targets
    }
}

fn followed_artist_key(artist_id: &str) -> String {
    format!("followed_artist:{artist_id}")
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
