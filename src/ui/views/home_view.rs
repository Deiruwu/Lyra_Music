use iced::{Element, Font, Task};
use iced::widget::container;

use crate::JETBRAINS_MONO;
use crate::ui::views::view_data::{NavId, ViewData};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Home,
    "\u{f015}",
    "Home",
    JETBRAINS_MONO,
);

#[derive(Debug, Clone)]
pub enum HomeViewMessage {}

#[derive(Debug, Clone, PartialEq)]
pub enum HomeViewOutMessage {
    Idle,
}

#[derive(Default)]
pub struct HomeView;

impl HomeView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, msg: HomeViewMessage) -> (Task<HomeViewMessage>, HomeViewOutMessage) {
        match msg {}
    }

    pub fn view(&self) -> Element<'_, HomeViewMessage> {
        container(iced::widget::space()).into()
    }
}