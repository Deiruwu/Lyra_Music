use iced::{Element, Font, Task};
use iced::widget::container;

use crate::JETBRAINS_MONO;
use crate::ui::views::view_data::{NavId, ViewData};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Favorites,
    "\u{f004}",
    "Me gusta",
    JETBRAINS_MONO,
);

#[derive(Debug, Clone)]
pub enum FavoritesViewMessage {}

#[derive(Debug, Clone, PartialEq)]
pub enum FavoritesViewOutMessage {
    Idle,
}

#[derive(Default)]
pub struct FavoritesView;

impl FavoritesView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, msg: FavoritesViewMessage) -> (Task<FavoritesViewMessage>, FavoritesViewOutMessage) {
        match msg {}
    }

    pub fn view(&self) -> Element<'_, FavoritesViewMessage> {
        container(iced::widget::space()).into()
    }
}