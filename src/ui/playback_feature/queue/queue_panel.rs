use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use iced::{Element, Length, Padding, Task};
use iced::widget::{button, container, scrollable, space, stack, text};
use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::widgets::track_row::{queue_track_row, DragRowParams, QueueThumbnailState};
use super::animator::QueueAnimator;

pub(crate) const ROW_HEIGHT: f32 = 66.0;
pub(crate) const ROW_SPACING: f32 = 4.0;
pub(crate) const ROW_STRIDE: f32 = ROW_HEIGHT + ROW_SPACING;

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
    Tick,

    // ── Drag & drop ──────────────────────────────────────────────────────
    DragStarted(usize),
    DragOver(usize),
    DragReleased,
    CursorMoved(f32),
    AnimationFrame(Instant),
}

#[derive(Debug, Clone)]
pub enum QueueOutMessage {
    Idle,
    RequestPlay(usize),
    RequestRemove(usize),
    RequestMove(usize, usize),
}

struct DragState {
    source_index: usize,
    current_index: usize,
    grab_offset: f32,
    cursor_y: f32,
}

pub struct QueuePanel {
    pub show: bool,
    queue: Vec<Arc<Track>>,
    hovered_row: Option<usize>,
    hovered_delete: Option<usize>,
    spinner_frame: u8,
    drag: Option<DragState>,
    animator: QueueAnimator,
}

impl Default for QueuePanel {
    fn default() -> Self {
        Self {
            show: false,
            queue: Vec::new(),
            hovered_row: None,
            hovered_delete: None,
            spinner_frame: 0,
            drag: None,
            animator: QueueAnimator::default(),
        }
    }
}

impl QueuePanel {
    pub fn is_animating(&self) -> bool {
        let now = Instant::now();
        self.animator.is_animating(now)
    }

    fn track_id_of(track: &Track) -> String {
        track.id.to_string()
    }

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
            QueueMessage::Tick => {
                self.spinner_frame = (self.spinner_frame + 1) % 6;
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::UiPlayClicked(index) => (Task::none(), QueueOutMessage::RequestPlay(index)),
            QueueMessage::UiRemoveClicked(index) => (Task::none(), QueueOutMessage::RequestRemove(index)),
            QueueMessage::UiMoveClicked(from, to) => (Task::none(), QueueOutMessage::RequestMove(from, to)),

            QueueMessage::DragStarted(index) => {
                if index < self.queue.len() {
                    let grabbed_top = index as f32 * ROW_STRIDE;
                    self.drag = Some(DragState {
                        source_index: index,
                        current_index: index,
                        grab_offset: 0.0,
                        cursor_y: grabbed_top,
                    });
                }
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::CursorMoved(cursor_y) => {
                let now = Instant::now();

                let Some(drag) = self.drag.as_mut() else {
                    let hovered_index = if self.queue.is_empty() {
                        None
                    } else {
                        Some(
                            ((cursor_y / ROW_STRIDE).floor() as isize)
                                .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize,
                        )
                    };
                    self.hovered_row = hovered_index;
                    return (Task::none(), QueueOutMessage::Idle);
                };

                if drag.grab_offset == 0.0 && drag.cursor_y == drag.current_index as f32 * ROW_STRIDE {
                    let row_top = drag.current_index as f32 * ROW_STRIDE;
                    drag.grab_offset = cursor_y - row_top;
                }
                drag.cursor_y = cursor_y;

                let hovered_index = ((cursor_y / ROW_STRIDE).floor() as isize)
                    .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize;

                if hovered_index != drag.current_index {
                    let item = self.queue.remove(drag.current_index);
                    self.queue.insert(hovered_index, item);
                    drag.current_index = hovered_index;

                    let ids: Vec<String> = self.queue.iter().map(|t| Self::track_id_of(t)).collect();
                    for (i, id) in ids.iter().enumerate() {
                        if i == hovered_index {
                            continue;
                        }
                        self.animator.sync_target(id, i, now);
                    }
                }

                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragOver(_index) => {
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragReleased => {
                let Some(drag) = self.drag.take() else {
                    return (Task::none(), QueueOutMessage::Idle);
                };

                if let Some(track) = self.queue.get(drag.current_index) {
                    let id = Self::track_id_of(track);
                    self.animator.snap_to_target(&id, drag.current_index);
                }

                if drag.source_index == drag.current_index {
                    return (Task::none(), QueueOutMessage::Idle);
                }

                (
                    Task::none(),
                    QueueOutMessage::RequestMove(drag.source_index, drag.current_index),
                )
            }

            QueueMessage::AnimationFrame(_now) => {
                (Task::none(), QueueOutMessage::Idle)
            }
        }
    }

    pub fn view(&self, cache: &ThumbnailCache, downloading_track_id: Option<&str>) -> Element<'_, QueueMessage> {
        if !self.show {
            return space().into();
        }

        let now = Instant::now();
        let is_dragging = self.drag.is_some();

        let mut layers: Vec<Element<'_, QueueMessage>> = Vec::with_capacity(self.queue.len() + 1);

        for (index, track) in self.queue.iter().enumerate() {
            let track_id = Self::track_id_of(track);
            let thumbnail = cache.peek_color(&thumb_key(track.as_ref()));

            let state = if downloading_track_id == Some(track_id.as_str()) {
                QueueThumbnailState::Downloading(self.spinner_frame)
            } else {
                QueueThumbnailState::Normal
            };

            let row_is_dragged = self
                .drag
                .as_ref()
                .is_some_and(|d| d.current_index == index);

            let drag_params = DragRowParams {
                is_dragging: row_is_dragged,
                on_drag_start: QueueMessage::DragStarted(index),
                on_drag_release: QueueMessage::DragReleased,
            };

            let row = queue_track_row(
                track.as_ref(),
                thumbnail,
                QueueMessage::UiPlayClicked(index),
                QueueMessage::UiRemoveClicked(index),
                !is_dragging && self.hovered_row == Some(index),
                self.hovered_delete == Some(index),
                QueueMessage::DeleteHovered(index),
                QueueMessage::DeleteUnhovered,
                state,
                drag_params,
            );

            if row_is_dragged {
                continue;
            }

            let y = self.animator.visual_y_of(&track_id, now);

            layers.push(
                container(row)
                    .width(Length::Fill)
                    .padding(Padding::new(0.0).top(y))
                    .into(),
            );
        }

        if let Some(drag) = &self.drag {
            if let Some(track) = self.queue.get(drag.current_index) {
                let thumbnail = cache.peek_color(&thumb_key(track.as_ref()));
                let ghost_y = (drag.cursor_y - drag.grab_offset).max(0.0);

                let drag_params = DragRowParams {
                    is_dragging: true,
                    on_drag_start: QueueMessage::DragStarted(drag.current_index),
                    on_drag_release: QueueMessage::DragReleased,
                };

                let ghost_row = queue_track_row(
                    track.as_ref(),
                    thumbnail,
                    QueueMessage::UiPlayClicked(drag.current_index),
                    QueueMessage::UiRemoveClicked(drag.current_index),
                    false,
                    false,
                    QueueMessage::DeleteHovered(drag.current_index),
                    QueueMessage::DeleteUnhovered,
                    QueueThumbnailState::Normal,
                    drag_params,
                );

                layers.push(
                    container(ghost_row)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(ghost_y))
                        .into(),
                );
            }
        }

        let list_height = self.animator.calculate_dynamic_height(self.queue.len(), now);

        let content_stack = stack(layers).height(Length::Fixed(list_height));

        let interactive_area = iced::widget::mouse_area(content_stack)
            .on_move(|point| QueueMessage::CursorMoved(point.y));

        container(scrollable(interactive_area).height(Length::Fill))
            .padding(16)
            .width(Length::Fixed(450.0))
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

    pub fn queue_update(&mut self, queue: Vec<Arc<Track>>) {
        self.drag = None;
        self.queue = queue;
        if self.queue.is_empty() {
            self.show = false;
            self.animator.clear();
            return;
        }

        let now = Instant::now();

        let valid_ids: HashSet<String> = self
            .queue
            .iter()
            .map(|t| Self::track_id_of(t))
            .collect();

        self.animator.retain_valid_ids(&valid_ids);

        for (index, track) in self.queue.iter().enumerate() {
            let track_id = Self::track_id_of(track);
            self.animator.sync_target(&track_id, index, now);
        }
    }
}