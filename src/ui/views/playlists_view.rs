use std::collections::HashMap;
use iced::{Element, Font, Task};
use iced::widget::container;

use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::views::view_data::{NavId, ViewData};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::PlaylistsOverview,
    "\u{f00b}",
    "Playlists",
    JETBRAINS_MONO,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistsSubView {
    Overview,
    Detail(String),
}

#[derive(Debug, Clone)]
pub enum PlaylistsViewMessage {
    SelectPlaylist(String),
    BackToOverview,
    PlayTrackNow(Track),
    EnqueueTrack(Track),
    CreatePlaylistRequested,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistsViewOutMessage {
    Idle,
    RequestPlayNow(Track),
    RequestEnqueue(Track),
    PlaylistSelected(String),
    CreatePlaylistRequested,
}

pub struct PlaylistsView {
    pub current_subview: PlaylistsSubView,
    pub playlists_metadata: Vec<(String, String, String, usize)>,
    pub tracks_cache: HashMap<String, Vec<Track>>,
}

impl Default for PlaylistsView {
    fn default() -> Self {
        Self {
            current_subview: PlaylistsSubView::Overview,
            playlists_metadata: Vec::new(),
            tracks_cache: HashMap::new(),
        }
    }
}

impl PlaylistsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_playlist(&mut self, id: &str) {
        self.current_subview = PlaylistsSubView::Detail(id.to_string());
    }

    pub fn show_overview(&mut self) {
        self.current_subview = PlaylistsSubView::Overview;
    }

    pub fn update(
        &mut self,
        msg: PlaylistsViewMessage,
    ) -> (Task<PlaylistsViewMessage>, PlaylistsViewOutMessage) {
        match msg {
            PlaylistsViewMessage::SelectPlaylist(id) => {
                self.current_subview = PlaylistsSubView::Detail(id.clone());
                (Task::none(), PlaylistsViewOutMessage::PlaylistSelected(id))
            }
            PlaylistsViewMessage::BackToOverview => {
                self.current_subview = PlaylistsSubView::Overview;
                (Task::none(), PlaylistsViewOutMessage::Idle)
            }
            PlaylistsViewMessage::PlayTrackNow(t) => {
                (Task::none(), PlaylistsViewOutMessage::RequestPlayNow(t))
            }
            PlaylistsViewMessage::EnqueueTrack(t) => {
                (Task::none(), PlaylistsViewOutMessage::RequestEnqueue(t))
            }
            PlaylistsViewMessage::CreatePlaylistRequested => {
                (Task::none(), PlaylistsViewOutMessage::CreatePlaylistRequested)
            }
        }
    }

    pub fn view(&self) -> Element<'_, PlaylistsViewMessage> {
        match &self.current_subview {
            PlaylistsSubView::Overview => self.render_overview(),
            PlaylistsSubView::Detail(id) => self.render_detail(id),
        }
    }

    fn render_overview(&self) -> Element<'_, PlaylistsViewMessage> {
        container(iced::widget::space()).into()
    }

    fn render_detail(&self, _id: &str) -> Element<'_, PlaylistsViewMessage> {
        container(iced::widget::space()).into()
    }
}