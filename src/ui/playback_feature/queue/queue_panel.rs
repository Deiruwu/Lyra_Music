use std::collections::HashSet;
use std::time::Instant;
use iced::{Element, Length, Padding, Task};
use iced::widget::scrollable::Viewport;
use iced::widget::operation::scroll_by;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{button, container, scrollable, space, stack, text, Id};
use crate::JETBRAINS_MONO;
use crate::audio::manager::manager::QueueSlot;
use crate::ui::styles::styles::{minimal_button};
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::widgets::track_row::{queue_track_row, DragRowParams};
use super::animator::QueueAnimator;

pub(crate) const ROW_HEIGHT: f32 = 66.0;
pub(crate) const ROW_SPACING: f32 = 4.0;
pub(crate) const ROW_STRIDE: f32 = ROW_HEIGHT + ROW_SPACING;

pub(crate) const QUEUE_COLLAPSED_WIDTH: f32 = 0.0;
pub(crate) const QUEUE_EXPANDED_WIDTH: f32 = 450.0;
const ANIMATION_SPEED: f32 = 12.0;
const SNAP_EPSILON: f32 = 0.5;

/// Id único del `scrollable` de la cola. Necesario para que las
/// operations de auto-scroll (`operation::scroll_by`) encuentren al
/// widget durante el drag & drop.
const QUEUE_SCROLL_ID: &str = "queue_scroll";
/// Filas extra a renderizar por arriba y por abajo de la ventana
/// estrictamente visible (`VirtualWindow`). Alineado con TrackBuilder.
const BUFFER_ROWS: usize = 15;
/// Zona caliente, en píxeles, desde cada borde del viewport dentro de la
/// cual arrastrar dispara el auto-scroll (misma semántica que playlists).
const AUTOSCROLL_ZONE_PX: f32 = 50.0;
/// Velocidad máxima de auto-scroll por tick, en píxeles.
const AUTOSCROLL_MAX_SPEED_PX: f32 = 18.0;

// ── FEAT FUTURO: canción en descarga visible en la cola ──────────────────
// Hoy la cola presume que todo track ya está descargado. Para mostrar una
// canción con un símbolo de espera mientras se descarga (sin esperar a
// que el motor la suelte), este era el flujo, hoy comentado/eliminado:
//
//   1. TrackManager emite QueueEvent::DownloadStarted(track) /
//      QueueEvent::DownloadFinished(track) por el canal broadcast.
//   2. queue_events() en playback_feature los traducía a
//      PlaybackFeatureMessage::DownloadingStarted(id) / DownloadingFinished(id).
//   3. La UI guardaba downloading_track_id, y view() mandaba
//      QueueThumbnailState::Downloading(self.spinner_frame) a la fila cuyo
//      track_id coincidía (fila + ghost).
//   4. QueueThumbnailState::Downloading(u8) vivía en track_row.rs y
//      thumbnail_with_overlay pintaba un SPINNER sobre la miniatura.
//   5. El frame avanzaba con QueueMessage::Tick (40ms) vía
//      self.spinner_frame = (self.spinner_frame + 1) % 6.
//
// Para reintroducirlo: volver a añadir `spinner_frame` + `Tick` en
// `QueuePanel`, el parámetro `downloading_track_id` en `view()`, y la
// variante `Downloading(u8)` en `QueueThumbnailState` con su overlay.

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

    // ── Drag & drop ──────────────────────────────────────────────────────
    DragStarted(usize),
    DragOver(usize),
    DragReleased,
    CursorMoved(f32),
    AnimationFrame(Instant),

    // ── Virtualización / scroll ──────────────────────────────────────────
    Scrolled(Viewport),
    AutoScrollTick,
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
    pub queue_width: f32,
    pub target_width: f32,
    queue: Vec<QueueSlot>,
    hovered_row: Option<usize>,
    hovered_delete: Option<usize>,
    drag: Option<DragState>,
    animator: QueueAnimator,
    scroll: ScrollTracker,
}

impl Default for QueuePanel {
    fn default() -> Self {
        Self {
            show: false,
            queue_width: QUEUE_COLLAPSED_WIDTH,
            target_width: QUEUE_COLLAPSED_WIDTH,
            queue: Vec::new(),
            hovered_row: None,
            hovered_delete: None,
            drag: None,
            animator: QueueAnimator::default(),
            scroll: ScrollTracker::default(),
        }
    }
}

impl QueuePanel {
    pub fn is_animating(&self) -> bool {
        let now = Instant::now();
        self.animator.is_animating(now)
    }

    pub fn is_animating_width(&self) -> bool {
        (self.queue_width - self.target_width).abs() > SNAP_EPSILON
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Universo `(key, url)` de la ventana visible de la cola (+buffer),
    /// listo para `AsyncThumbnail::sync`. Espejo de
    /// `track_list_builder::visible_thumbnail_targets` (Explorer/Playlists),
    /// pero sobre la cola con su propio `ROW_STRIDE`.
    pub fn visible_thumbnail_targets(&self) -> Vec<(String, String)> {
        let window = self.scroll.window(ROW_STRIDE, self.queue.len(), BUFFER_ROWS);
        if window.is_empty() {
            return Vec::new();
        }

        self.queue[window.start..window.end]
            .iter()
            .filter_map(|s| s.track.thumbnail_small.clone().map(|url| (thumb_key(&s.track), url)))
            .collect()
    }

    fn slot_id_of(slot: &QueueSlot) -> String {
        slot.id.to_string()
    }

    fn move_dragged_item(&mut self, hovered_index: usize, now: Instant) {
        let Some(drag) = self.drag.as_mut() else { return };
        if hovered_index == drag.current_index {
            return;
        }

        let item = self.queue.remove(drag.current_index);
        self.queue.insert(hovered_index, item);
        drag.current_index = hovered_index;

        let ids: Vec<String> = self.queue.iter().map(Self::slot_id_of).collect();
        for (i, id) in ids.iter().enumerate() {
            if i == hovered_index {
                continue;
            }
            self.animator.sync_target(id, i, now);
        }
    }

    pub fn update(&mut self, msg: QueueMessage) -> (Task<QueueMessage>, QueueOutMessage) {
        match msg {
            QueueMessage::Toggle => {
                self.show = !self.show;
                self.target_width = if self.show { QUEUE_EXPANDED_WIDTH } else { QUEUE_COLLAPSED_WIDTH };
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

            QueueMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                (Task::none(), QueueOutMessage::Idle)
            }

            // Auto-scroll mientras se arrastra cerca de los bordes del viewport.
            // OJO: el `mouse_area` de la cola envuelve el CONTENIDO (dentro del
            // `scrollable`), así que `drag.cursor_y` ya viene en coordenadas de
            // contenido (absolutas). Para `autoscroll_delta` necesitamos la
            // posición local al viewport, de ahí restar `offset_y`.
            QueueMessage::AutoScrollTick => {
                let now = Instant::now();

                let Some(drag) = self.drag.as_mut() else {
                    return (Task::none(), QueueOutMessage::Idle);
                };

                let local_y = drag.cursor_y - self.scroll.offset_y;
                let Some(delta_y) = self.scroll.autoscroll_delta(
                    local_y,
                    AUTOSCROLL_ZONE_PX,
                    AUTOSCROLL_MAX_SPEED_PX,
                ) else {
                    return (Task::none(), QueueOutMessage::Idle);
                };

                let max_offset = (self.queue.len() as f32 * ROW_STRIDE - self.scroll.viewport_height).max(0.0);
                self.scroll.offset_y = (self.scroll.offset_y + delta_y).clamp(0.0, max_offset);


                drag.cursor_y += delta_y;

                let hovered_index = ((drag.cursor_y / ROW_STRIDE).floor() as isize)
                    .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize;

                self.move_dragged_item(hovered_index, now);

                (
                    scroll_by(Id::new(QUEUE_SCROLL_ID), AbsoluteOffset { x: 0.0, y: delta_y }),
                    QueueOutMessage::Idle,
                )
            }

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

                self.move_dragged_item(hovered_index, now);

                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragOver(_index) => {
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragReleased => {
                let Some(drag) = self.drag.take() else {
                    return (Task::none(), QueueOutMessage::Idle);
                };

                if let Some(slot) = self.queue.get(drag.current_index) {
                    let id = Self::slot_id_of(slot);
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
                // Interpolar ancho de panel en el frame
                let delta = self.target_width - self.queue_width;
                if delta.abs() <= SNAP_EPSILON {
                    self.queue_width = self.target_width;
                } else {
                    self.queue_width += delta * (ANIMATION_SPEED / 60.0).min(1.0);
                }
                (Task::none(), QueueOutMessage::Idle)
            }
        }
    }

    pub fn view<'a>(&'a self, thumbnails: &'a AsyncThumbnail) -> Element<'a, QueueMessage> {
        if self.queue_width == 0.0 {
            return space().into();
        }

        let total_items = self.queue.len();
        let now = Instant::now();
        let is_dragging = self.drag.is_some();

        // 1. Qué rango de índices es realmente visible (+buffer). El stack
        //    ya tiene la altura total fija (list_height), así que el
        //    scrollbar funciona sin espaciadores: la ventana solo FILTRA
        //    qué filas construimos, y el `y` absoluto del animator las
        //    ancla en su posición real dentro del contenido.
        let window = self.scroll.window(ROW_STRIDE, total_items, BUFFER_ROWS);

        let dynamic_padding = if self.queue_width > 32.0 { 16.0 } else { self.queue_width / 2.0 };

        if window.is_empty() {
            return container(space())
                .padding(dynamic_padding)
                .width(Length::Fixed(self.queue_width))
                .height(Length::Fill)
                .style(queue_panel_style)
                .into();
        }

        let mut layers: Vec<Element<'a, QueueMessage>> = Vec::with_capacity(window.len() + 1);

        // 2. Renderizamos SOLO las filas de la ventana.
        for index in window.start..window.end {
            let slot = &self.queue[index];
            let track = &slot.track;
            let slot_id = Self::slot_id_of(slot);
            let thumbnail = thumbnails.get(&thumb_key(track.as_ref())).cloned();

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
                drag_params,
            );

            if row_is_dragged {
                continue;
            }

            let y = self.animator.visual_y_of(&slot_id, now);

            layers.push(
                container(row)
                    .width(Length::Fill)
                    .padding(Padding::new(0.0).top(y))
                    .into(),
            );
        }

        // 3. Ghost del drag & drop (siempre se pinta por encima de la lista).
        if let Some(drag) = &self.drag {
            if let Some(slot) = self.queue.get(drag.current_index) {
                let track = &slot.track;
                let thumbnail = thumbnails.get(&thumb_key(track.as_ref())).cloned();
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

        // 4. Altura total "teórica" del contenido: mantiene el scrollbar
        //    fiel y deja el `y` absoluto del animator anclando cada fila.
        let list_height = self.animator.calculate_dynamic_height(total_items, now);
        let content_stack = stack(layers).height(Length::Fixed(list_height));

        let interactive_area = iced::widget::mouse_area(content_stack)
            .on_move(|point| QueueMessage::CursorMoved(point.y))
            .on_exit(QueueMessage::Unhovered);

        container(
            scrollable(interactive_area)
                .id(Id::new(QUEUE_SCROLL_ID))
                .height(Length::Fill)
                .on_scroll(QueueMessage::Scrolled),
        )
            .padding(dynamic_padding)
            .width(Length::Fixed(self.queue_width))
            .height(Length::Fill)
            .style(queue_panel_style)
            .into()
    }

    pub fn view_toggle_button(&self) -> Element<'_, QueueMessage> {
        let can_show_queue = !self.queue.is_empty();
        let btn = button(text("󰲸").font(JETBRAINS_MONO).size(18))
            .style(minimal_button);

        if can_show_queue {
            btn.on_press(QueueMessage::Toggle)
        } else {
            btn
        }.into()
    }

    pub fn queue_update(&mut self, queue: Vec<QueueSlot>) {
        self.drag = None;
        self.queue = queue;
        if self.queue.is_empty() {
            self.show = false;
            self.target_width = QUEUE_COLLAPSED_WIDTH;
            self.animator.clear();
            self.scroll.reset();
            return;
        }

        let now = Instant::now();

        let valid_ids: HashSet<String> = self
            .queue
            .iter()
            .map(Self::slot_id_of)
            .collect();

        self.animator.retain_valid_ids(&valid_ids);

        for (index, slot) in self.queue.iter().enumerate() {
            let slot_id = Self::slot_id_of(slot);
            self.animator.sync_target(&slot_id, index, now);
        }

        // Reclampa el offset de scroll por si la lista se encogió.
        let max_offset = (self.queue.len() as f32 * ROW_STRIDE - self.scroll.viewport_height).max(0.0);
        self.scroll.offset_y = self.scroll.offset_y.min(max_offset);
    }
}

/// Fondo común del panel de la cola (se usa tanto en el caso vacío como
/// en el render normal).
fn queue_panel_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Color::from_rgb(0.12, 0.12, 0.12).into()),
        border: iced::border::rounded(12),
        ..Default::default()
    }
}