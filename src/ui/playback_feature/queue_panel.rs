use std::sync::Arc;
use iced::{Element, Length, Task};
use iced::widget::{button, column, container, scrollable, space, text};
use crate::JETBRAINS_MONO;
use crate::model::audio_tech::PlayableTrack;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::widgets::track_row::queue_track_row;

#[derive(Debug, Clone)]
pub enum QueueMessage {
    Toggle,
    Hovered(usize),
    Unhovered,
    DeleteHovered(usize),
    DeleteUnhovered,
    UiPlayClicked(usize),
    UiRemoveClicked(usize),
    UiMoveClicked(usize, usize),
}

#[derive(Debug, Clone)]
pub enum QueueOutMessage {
    Idle,
    RequestPlay(usize),
    RequestRemove(usize),
    RequestMove(usize, usize),
}

pub struct QueuePanel {
    pub show: bool,
    queue: Vec<Arc<PlayableTrack>>,
    hovered_row: Option<usize>,
    hovered_delete: Option<usize>,
}

impl Default for QueuePanel {
    fn default() -> Self {
        Self {
            show: false,
            queue: Vec::new(),
            hovered_row: None,
            hovered_delete: None,
        }
    }
}

impl QueuePanel {
    pub fn update(&mut self, msg: QueueMessage) -> (Task<QueueMessage>, QueueOutMessage) {
        match msg {
            QueueMessage::Toggle => {
                self.show = !self.show;
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::Hovered(index) => {
                self.hovered_row = Some(index);
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::Unhovered => {
                self.hovered_row = None;
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DeleteHovered(index) => {
                self.hovered_delete = Some(index);
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::DeleteUnhovered => {
                self.hovered_delete = None;
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::UiPlayClicked(index) => (Task::none(), QueueOutMessage::RequestPlay(index)),
            QueueMessage::UiRemoveClicked(index) => (Task::none(), QueueOutMessage::RequestRemove(index)),
            QueueMessage::UiMoveClicked(from, to) => (Task::none(), QueueOutMessage::RequestMove(from, to)),
        }
    }

    pub fn view(&self, cache: &ThumbnailCache) -> Element<'_, QueueMessage> {
        if !self.show {
            return space().into();
        }

        let track_list = column(
            self.queue
                .iter()
                .enumerate()
                .map(|(index, track)| {
                    let thumbnail = cache.peek(&track.track.id);
                    queue_track_row(
                        track.as_ref(),
                        thumbnail,
                        QueueMessage::UiPlayClicked(index),
                        QueueMessage::UiRemoveClicked(index),
                        self.hovered_row == Some(index),
                        QueueMessage::Hovered(index),
                        QueueMessage::Unhovered,

                        self.hovered_delete == Some(index),
                        QueueMessage::DeleteHovered(index),
                        QueueMessage::DeleteUnhovered,
                    )
                })
                .collect::<Vec<_>>(),
        )
            .spacing(4)
            .padding(8);

        container(scrollable(track_list).height(Length::Fill))
            .padding(16)
            .width(Length::Fixed(350.0))
            .height(Length::Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.12, 0.12, 0.12).into()),
                border: iced::border::rounded(12),
                ..Default::default()
            })
            .into()
    }

    pub fn view_toggle_button(&self) -> Element<'_, QueueMessage> {
        let can_show_queue = !self.queue.is_empty();
        let btn = button(text("󰲸").font(JETBRAINS_MONO).size(18))
            .style(transparent_button);

        if can_show_queue {
            btn.on_press(QueueMessage::Toggle)
        } else {
            btn
        }.into()
    }

    pub fn queue_update(&mut self, queue: Vec<Arc<PlayableTrack>>) {
        self.queue = queue;
        if self.queue.is_empty() {
            self.show = false;
        }
    }
}