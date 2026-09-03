use std::collections::HashSet;
use std::time::Instant;
use iced::{Element, Length, Padding, Task, Theme};
use iced::widget::scrollable::Viewport;
use iced::widget::operation::{scroll_by, snap_to};
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset};
use iced::widget::{button, container, row, rule, scrollable, space, stack, text, Id};
use uuid::Uuid;
use crate::ui::assets::fonts::JETBRAINS_MONO;
use crate::audio::manager::manager::QueueSlot;
use crate::model::Track;
use crate::ui::playback_feature::player::TrackLink;
use crate::ui::styles::button as button_style;
use crate::ui::styles::container as container_style;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::widgets::track_row::{queue_static_row, queue_track_row, QueueRowVariant};
use super::animator::QueueAnimator;
use crate::ui::assets::{radii, typography};
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::theme::theme;

pub(crate) const ROW_HEIGHT: f32 = 66.0;
pub(crate) const ROW_SPACING: f32 = 4.0;
pub(crate) const ROW_STRIDE: f32 = ROW_HEIGHT + ROW_SPACING;
const DIVIDER_EXTRA_GAP: f32 = 10.0;

pub(crate) const QUEUE_COLLAPSED_WIDTH: f32 = 20.0;
pub(crate) const QUEUE_EXPANDED_WIDTH: f32 = 340.0;
const ANIMATION_SPEED: f32 = 12.0;
const SNAP_EPSILON: f32 = 0.5;
const DRAG_THRESHOLD_PX: f32 = 5.0;

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
/// Cuántas canciones ya reproducidas mostrar arriba del track actual.
/// El motor cachea hasta 100 (`PlaybackState::HISTORY_CAP`), pero acá
/// solo mostramos las últimas para no volver la lista fusionada
/// interminable hacia arriba.
const HISTORY_VISIBLE_CAP: usize = 20;

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
    DeleteHoveredHistory(usize),
    DeleteUnhoveredHistory,
    UiPlayClicked(usize),
    UiRemoveClicked(usize),
    UiMoveClicked(usize, usize),
    OpenTrackLink(TrackLink),
    RightClicked(usize),
    RightClickedHistory(usize),
    RightClickedCurrent,
    /// Click en una fila de historial: saltar `n` canciones atrás
    /// (1-based, ver `QueueOutMessage::RequestJumpBack`).
    UiJumpToHistory(usize),
    UiRemoveHistoryClicked(usize),

    // ── Drag & drop ──────────────────────────────────────────────────────
    GlobalPressed,
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
    RequestOpenTrackLink(TrackLink),
    RequestContextMenu(Uuid),
    RequestHistoryContextMenu(usize),
    RequestCurrentContextMenu,
    RequestJumpBack(usize),
    RequestRemoveHistory(usize),
    RequestMoveToHistory(usize),
    RequestMoveToQueue(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum DragOrigin {
    Queue(usize),
    History(usize),
}

struct DragState {
    origin: DragOrigin,
    current_index: usize,
    grab_offset: f32,
    cursor_y: f32,
}

/// Arranca en el mouse press global (ver `PlaybackFeature::subscription`)
/// pero todavía no es un drag real: hasta que el cursor no se mueva más
/// de `DRAG_THRESHOLD_PX`, es indistinguible de un click normal (play,
/// borrar, links, right-click siguen andando igual). Mismo patrón que
/// `PendingDrag` en `playlist_view.rs`.
struct PendingDrag {
    origin: DragOrigin,
    grab_offset: f32,
    start_y: f32,
}

pub struct QueuePanel {
    pub show: bool,
    pub queue_width: f32,
    pub target_width: f32,
    queue: Vec<QueueSlot>,
    /// Últimas `HISTORY_VISIBLE_CAP` canciones ya reproducidas (oldest→newest),
    /// recortadas del snapshot de `TrackManager::get_history_snapshot()`.
    history: Vec<Track>,
    /// Track sonando ahora mismo (`TrackManager::get_current_track()`), si hay.
    current_track: Option<Track>,
    hovered_zone: Option<DragOrigin>,
    hovered_delete: Option<usize>,
    hovered_delete_history: Option<usize>,
    drag: Option<DragState>,
    pending_drag: Option<PendingDrag>,
    last_cursor_y: f32,
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
            history: Vec::new(),
            current_track: None,
            hovered_zone: None,
            hovered_delete: None,
            hovered_delete_history: None,
            drag: None,
            pending_drag: None,
            last_cursor_y: 0.0,
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

    /// Cuántas filas hay antes del inicio de la cola en la lista
    /// fusionada: todo el historial visible + (1 si hay track actual).
    fn queue_start_offset(&self) -> usize {
        self.history.len() + self.current_track.is_some() as usize
    }

    /// Total de filas de la lista fusionada (historial + actual + cola),
    /// la unidad sobre la que virtualiza/scrollea `view()`.
    fn merged_total(&self) -> usize {
        self.queue_start_offset() + self.queue.len()
    }

    fn extra_gap(&self) -> f32 {
        if !self.history.is_empty() && self.current_track.is_some() {
            DIVIDER_EXTRA_GAP
        } else {
            0.0
        }
    }

    fn row_y(&self, merged_index: usize) -> f32 {
        let base = merged_index as f32 * ROW_STRIDE;
        if merged_index >= self.history.len() {
            base + self.extra_gap()
        } else {
            base
        }
    }

    /// Track en la fila `merged_index` de la lista fusionada, sea de
    /// historial, el actual, o de la cola. `None` si el índice cae fuera
    /// de rango.
    fn track_at(&self, merged_index: usize) -> Option<&Track> {
        if merged_index < self.history.len() {
            return self.history.get(merged_index);
        }
        if merged_index == self.history.len() {
            return self.current_track.as_ref();
        }
        self.queue
            .get(merged_index - self.queue_start_offset())
            .map(|s| s.track.as_ref())
    }

    /// Universo `(key, url)` de la ventana visible de la lista fusionada
    /// (+buffer), listo para `AsyncThumbnail::sync`. Espejo de
    /// `track_list_builder::visible_thumbnail_targets` (Explorer/Playlists),
    /// pero sobre la cola con su propio `ROW_STRIDE`.
    pub fn visible_thumbnail_targets(&self) -> Vec<(String, String)> {
        let window = self.scroll.window(ROW_STRIDE, self.merged_total(), BUFFER_ROWS);
        if window.is_empty() {
            return Vec::new();
        }

        (window.start..window.end)
            .filter_map(|i| self.track_at(i))
            .filter_map(|t| t.thumbnail_small.clone().map(|url| (thumb_key(t), url)))
            .collect()
    }

    fn slot_id_of(slot: &QueueSlot) -> String {
        slot.id.to_string()
    }

    /// Busca un track en la cola por id de slot (estable aunque la cola se
    /// reordene) — lo usa `PlaybackFeature` para armar/ejecutar el menú
    /// contextual sin exponer `self.queue` directamente.
    pub fn find_slot_track(&self, slot_id: Uuid) -> Option<Track> {
        self.queue.iter().find(|s| s.id == slot_id).map(|s| (*s.track).clone())
    }

    pub fn history_track(&self, steps_back: usize) -> Option<Track> {
        let index = self.history.len().checked_sub(steps_back)?;
        self.history.get(index).cloned()
    }

    pub fn current_track(&self) -> Option<Track> {
        self.current_track.clone()
    }

    fn move_dragged_item_in_queue(&mut self, hovered_index: usize, now: Instant) {
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
                let was_shown = self.show;
                self.show = !self.show;
                self.target_width = if self.show { QUEUE_EXPANDED_WIDTH } else { QUEUE_COLLAPSED_WIDTH };

                // Al abrir el panel (no en cada toggle), saltar al track
                // actual — de ahí en más el seguimiento es condicional
                // (ver `sync_playback`/`is_row_visible`).
                let task = if self.show && !was_shown {
                    self.scroll_to_current()
                } else {
                    Task::none()
                };
                (task, QueueOutMessage::Idle)
            }
            QueueMessage::Hovered(index) => {
                self.hovered_zone = Some(DragOrigin::Queue(index));
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::Unhovered => {
                self.hovered_zone = None;
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
            QueueMessage::DeleteHoveredHistory(steps_back) => {
                self.hovered_delete_history = Some(steps_back);
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::DeleteUnhoveredHistory => {
                self.hovered_delete_history = None;
                (Task::none(), QueueOutMessage::Idle)
            }
            QueueMessage::UiRemoveHistoryClicked(steps_back) => {
                (Task::none(), QueueOutMessage::RequestRemoveHistory(steps_back))
            }
            QueueMessage::UiPlayClicked(index) => (Task::none(), QueueOutMessage::RequestPlay(index)),
            QueueMessage::UiRemoveClicked(index) => (Task::none(), QueueOutMessage::RequestRemove(index)),
            QueueMessage::UiMoveClicked(from, to) => (Task::none(), QueueOutMessage::RequestMove(from, to)),
            QueueMessage::OpenTrackLink(link) => (Task::none(), QueueOutMessage::RequestOpenTrackLink(link)),
            QueueMessage::UiJumpToHistory(steps_back) => (Task::none(), QueueOutMessage::RequestJumpBack(steps_back)),
            QueueMessage::RightClicked(index) => {
                match self.queue.get(index) {
                    Some(slot) => (Task::none(), QueueOutMessage::RequestContextMenu(slot.id)),
                    None => (Task::none(), QueueOutMessage::Idle),
                }
            }
            QueueMessage::RightClickedHistory(steps_back) => {
                (Task::none(), QueueOutMessage::RequestHistoryContextMenu(steps_back))
            }
            QueueMessage::RightClickedCurrent => {
                (Task::none(), QueueOutMessage::RequestCurrentContextMenu)
            }

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
                let merged_total = self.merged_total();
                let queue_start = self.row_y(self.queue_start_offset());
                let extra_gap = self.extra_gap();

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

                let max_offset = (merged_total as f32 * ROW_STRIDE + extra_gap - self.scroll.viewport_height).max(0.0);
                self.scroll.offset_y = (self.scroll.offset_y + delta_y).clamp(0.0, max_offset);


                drag.cursor_y += delta_y;

                if matches!(drag.origin, DragOrigin::Queue(_))
                    && drag.cursor_y >= queue_start
                    && !self.queue.is_empty()
                {
                    let hovered_index = (((drag.cursor_y - queue_start) / ROW_STRIDE).floor() as isize)
                        .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize;
                    self.move_dragged_item_in_queue(hovered_index, now);
                }

                (
                    scroll_by(Id::new(QUEUE_SCROLL_ID), AbsoluteOffset { x: 0.0, y: delta_y }),
                    QueueOutMessage::Idle,
                )
            }

            // Arma el pending drag a partir de la fila actualmente hovereada
            // (mantenida al día por CursorMoved). Un click normal sobre
            // play/borrar/links/right-click nunca supera el threshold en
            // CursorMoved, así que nunca llega a promoverse a `self.drag`.
            QueueMessage::GlobalPressed => {
                if self.drag.is_none() && self.pending_drag.is_none() {
                    if let Some(origin) = self.hovered_zone {
                        let valid = match origin {
                            DragOrigin::Queue(i) => i < self.queue.len(),
                            DragOrigin::History(sb) => sb >= 1 && sb <= self.history.len(),
                        };
                        if valid {
                            let merged_index = match origin {
                                DragOrigin::Queue(i) => self.queue_start_offset() + i,
                                DragOrigin::History(sb) => self.history.len() - sb,
                            };
                            let row_top = self.row_y(merged_index);
                            self.pending_drag = Some(PendingDrag {
                                origin,
                                grab_offset: self.last_cursor_y - row_top,
                                start_y: self.last_cursor_y,
                            });
                        }
                    }
                }
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::CursorMoved(cursor_y) => {
                self.last_cursor_y = cursor_y;
                let now = Instant::now();

                if let Some(pending) = &self.pending_drag {
                    if (cursor_y - pending.start_y).abs() > DRAG_THRESHOLD_PX {
                        let current_index = match pending.origin {
                            DragOrigin::Queue(i) => i,
                            DragOrigin::History(steps_back) => self.history.len() - steps_back,
                        };
                        self.drag = Some(DragState {
                            origin: pending.origin,
                            current_index,
                            grab_offset: pending.grab_offset,
                            cursor_y,
                        });
                        self.pending_drag = None;
                    }
                }

                let queue_start = self.row_y(self.queue_start_offset());

                let Some(drag) = self.drag.as_mut() else {
                    let history_end = self.history.len() as f32 * ROW_STRIDE;
                    self.hovered_zone = if !self.history.is_empty() && cursor_y < history_end {
                        let history_index = ((cursor_y / ROW_STRIDE).floor() as isize)
                            .clamp(0, self.history.len().saturating_sub(1) as isize) as usize;
                        Some(DragOrigin::History(self.history.len() - history_index))
                    } else if cursor_y < queue_start {
                        None
                    } else if !self.queue.is_empty() {
                        let q_index = (((cursor_y - queue_start) / ROW_STRIDE).floor() as isize)
                            .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize;
                        Some(DragOrigin::Queue(q_index))
                    } else {
                        None
                    };
                    return (Task::none(), QueueOutMessage::Idle);
                };

                drag.cursor_y = cursor_y;

                match drag.origin {
                    DragOrigin::Queue(_) => {
                        if cursor_y >= queue_start && !self.queue.is_empty() {
                            let hovered_index = (((cursor_y - queue_start) / ROW_STRIDE).floor() as isize)
                                .clamp(0, self.queue.len().saturating_sub(1) as isize) as usize;
                            self.move_dragged_item_in_queue(hovered_index, now);
                        }
                    }
                    DragOrigin::History(_) => {}
                }

                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragOver(_index) => {
                (Task::none(), QueueOutMessage::Idle)
            }

            QueueMessage::DragReleased => {
                self.pending_drag = None;

                let Some(drag) = self.drag.take() else {
                    return (Task::none(), QueueOutMessage::Idle);
                };

                let queue_start = self.row_y(self.queue_start_offset());
                let crossed_into_queue = drag.cursor_y >= queue_start;

                match drag.origin {
                    DragOrigin::Queue(source_index) => {
                        if let Some(slot) = self.queue.get(drag.current_index) {
                            let id = Self::slot_id_of(slot);
                            self.animator.snap_to_target(&id, drag.current_index);
                        }
                        if crossed_into_queue {
                            if source_index == drag.current_index {
                                (Task::none(), QueueOutMessage::Idle)
                            } else {
                                (Task::none(), QueueOutMessage::RequestMove(source_index, drag.current_index))
                            }
                        } else {
                            (Task::none(), QueueOutMessage::RequestMoveToHistory(drag.current_index))
                        }
                    }
                    DragOrigin::History(steps_back) => {
                        if crossed_into_queue {
                            (Task::none(), QueueOutMessage::RequestMoveToQueue(steps_back))
                        } else {
                            (Task::none(), QueueOutMessage::Idle)
                        }
                    }
                }
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
        if self.queue_width == 20.0 {
            return space().width(20).into();
        }

        let queue_start = self.queue_start_offset();
        let total_items = self.merged_total();
        let now = Instant::now();
        let is_dragging = self.drag.is_some();

        // 1. Qué rango de índices es realmente visible (+buffer). El stack
        //    ya tiene la altura total fija (list_height), así que el
        //    scrollbar funciona sin espaciadores: la ventana solo FILTRA
        //    qué filas construimos, y el `y` absoluto del animator las
        //    ancla en su posición real dentro del contenido. Los índices
        //    acá son de la lista FUSIONADA: [0, history.len()) = historial,
        //    history.len() = actual (si hay), el resto = cola.
        let window = self.scroll.window(ROW_STRIDE, total_items, BUFFER_ROWS);

        let dynamic_padding = if self.queue_width > 32.0 { 16.0 } else { self.queue_width / 2.0 };

        if window.is_empty() {
            return container(space())
                .padding(dynamic_padding)
                .width(Length::Fixed(self.queue_width))
                .height(Length::Fill)
                .style(container_style::queue_panel)
                .into();
        }

        let mut layers: Vec<Element<'a, QueueMessage>> = Vec::with_capacity(window.len() + 1);

        // 2. Renderizamos SOLO las filas de la ventana.
        for index in window.start..window.end {
            if index < self.history.len() {
                let steps_back = self.history.len() - index;

                let row_is_dragged = self.drag.as_ref().is_some_and(|d| {
                    matches!(d.origin, DragOrigin::History(sb) if sb == steps_back)
                });
                if row_is_dragged {
                    continue;
                }

                let track = &self.history[index];
                let thumbnail = thumbnails.get(&thumb_key(track)).cloned();
                let row = queue_static_row(
                    index + 1,
                    track,
                    thumbnail,
                    QueueRowVariant::History,
                    |id| QueueMessage::OpenTrackLink(TrackLink::Artist(id)),
                    |id| QueueMessage::OpenTrackLink(TrackLink::Album(id)),
                    Some(QueueMessage::UiJumpToHistory(steps_back)),
                    Some(QueueMessage::RightClickedHistory(steps_back)),
                    Some(QueueMessage::UiRemoveHistoryClicked(steps_back)),
                    self.hovered_delete_history == Some(steps_back),
                    Some(QueueMessage::DeleteHoveredHistory(steps_back)),
                    Some(QueueMessage::DeleteUnhoveredHistory),
                    false,
                );
                let y = self.row_y(index);
                layers.push(container(row).width(Length::Fill).padding(Padding::new(0.0).top(y)).into());
                continue;
            }

            if index == self.history.len() && self.current_track.is_some() {
                let track = self.current_track.as_ref().unwrap();
                let thumbnail = thumbnails.get(&thumb_key(track)).cloned();
                let row = queue_static_row(
                    index + 1,
                    track,
                    thumbnail,
                    QueueRowVariant::Current,
                    |id| QueueMessage::OpenTrackLink(TrackLink::Artist(id)),
                    |id| QueueMessage::OpenTrackLink(TrackLink::Album(id)),
                    None,
                    Some(QueueMessage::RightClickedCurrent),
                    None,
                    false,
                    None,
                    None,
                    false,
                );
                let y = self.row_y(index);
                layers.push(container(row).width(Length::Fill).padding(Padding::new(0.0).top(y)).into());

                // Separador fino arriba del actual (solo si hay historial
                // arriba de él) — la única distinción extra que lleva,
                // sin fondo ni borde llamativos. Centrado y corto (~40%
                // del ancho), no de punta a punta.
                if !self.history.is_empty() {
                    let divider = rule::horizontal(1.0).style(|_theme: &Theme| rule::Style {
                        color: theme().border.subtle,
                        radius: radii::R_NONE.into(),
                        fill_mode: rule::FillMode::Full,
                        snap: false,
                    });
                    let inset_divider = row![
                        space().width(Length::FillPortion(1)),
                        container(divider).width(Length::FillPortion(3)),
                        space().width(Length::FillPortion(1)),
                    ];
                    let divider_y = y - (ROW_SPACING + self.extra_gap()) / 2.0 - 0.5;
                    layers.push(
                        container(inset_divider)
                            .width(Length::Fill)
                            .padding(Padding::new(0.0).top(divider_y))
                            .into(),
                    );
                }
                continue;
            }

            let q_index = index - queue_start;
            let slot = &self.queue[q_index];
            let track = &slot.track;
            let slot_id = Self::slot_id_of(slot);
            let thumbnail = thumbnails.get(&thumb_key(track.as_ref())).cloned();

            let row_is_dragged = self.drag.as_ref().is_some_and(|d| {
                matches!(d.origin, DragOrigin::Queue(_)) && d.current_index == q_index
            });

            let row = queue_track_row(
                index + 1,
                track.as_ref(),
                thumbnail,
                QueueMessage::UiPlayClicked(q_index),
                QueueMessage::UiRemoveClicked(q_index),
                !is_dragging && self.hovered_zone == Some(DragOrigin::Queue(q_index)),
                self.hovered_delete == Some(q_index),
                QueueMessage::DeleteHovered(q_index),
                QueueMessage::DeleteUnhovered,
                row_is_dragged,
                |id| QueueMessage::OpenTrackLink(TrackLink::Artist(id)),
                |id| QueueMessage::OpenTrackLink(TrackLink::Album(id)),
                QueueMessage::RightClicked(q_index),
            );

            if row_is_dragged {
                continue;
            }

            let y = self.row_y(queue_start) + self.animator.visual_y_of(&slot_id, now);

            layers.push(
                container(row)
                    .width(Length::Fill)
                    .padding(Padding::new(0.0).top(y))
                    .into(),
            );
        }

        // 3. Ghost del drag & drop (siempre se pinta por encima de la lista).
        if let Some(drag) = &self.drag {
            let ghost_y = (drag.cursor_y - drag.grab_offset).max(0.0);

            match drag.origin {
                DragOrigin::Queue(_) => {
                    if let Some(slot) = self.queue.get(drag.current_index) {
                        let track = &slot.track;
                        let thumbnail = thumbnails.get(&thumb_key(track.as_ref())).cloned();

                        let ghost_row = queue_track_row(
                            queue_start + drag.current_index + 1,
                            track.as_ref(),
                            thumbnail,
                            QueueMessage::UiPlayClicked(drag.current_index),
                            QueueMessage::UiRemoveClicked(drag.current_index),
                            false,
                            false,
                            QueueMessage::DeleteHovered(drag.current_index),
                            QueueMessage::DeleteUnhovered,
                            true,
                            |id| QueueMessage::OpenTrackLink(TrackLink::Artist(id)),
                            |id| QueueMessage::OpenTrackLink(TrackLink::Album(id)),
                            QueueMessage::RightClicked(drag.current_index),
                        );

                        layers.push(
                            container(ghost_row)
                                .width(Length::Fill)
                                .padding(Padding::new(0.0).top(ghost_y))
                                .into(),
                        );
                    }
                }
                DragOrigin::History(steps_back) => {
                    let history_index = self.history.len().checked_sub(steps_back);
                    if let Some(track) = history_index.and_then(|i| self.history.get(i)) {
                        let thumbnail = thumbnails.get(&thumb_key(track)).cloned();

                        let ghost_row = queue_static_row(
                            drag.current_index + 1,
                            track,
                            thumbnail,
                            QueueRowVariant::History,
                            |id| QueueMessage::OpenTrackLink(TrackLink::Artist(id)),
                            |id| QueueMessage::OpenTrackLink(TrackLink::Album(id)),
                            None,
                            None,
                            None,
                            false,
                            None,
                            None,
                            true,
                        );

                        layers.push(
                            container(ghost_row)
                                .width(Length::Fill)
                                .padding(Padding::new(0.0).top(ghost_y))
                                .into(),
                        );
                    }
                }
            }
        }

        // 4. Altura total "teórica" del contenido: mantiene el scrollbar
        //    fiel y deja el `y` absoluto del animator anclando cada fila.
        //    El historial+actual tienen altura fija (no animan reorder);
        //    solo la porción de cola usa la altura dinámica del animator.
        let list_height = self.row_y(queue_start)
            + self.animator.calculate_dynamic_height(self.queue.len(), now);
        let content_stack = stack(layers).height(Length::Fixed(list_height));

        let interactive_area = iced::widget::mouse_area(content_stack)
            .on_move(|point| QueueMessage::CursorMoved(point.y))
            .on_exit(QueueMessage::Unhovered);

        container(
            scrollable(interactive_area)
                .id(Id::new(QUEUE_SCROLL_ID))
                .height(Length::Fill)
                .style(scrollable_style::discreet)
                .on_scroll(QueueMessage::Scrolled),
        )
            .padding(dynamic_padding)
            .width(Length::Fixed(self.queue_width))
            .height(Length::Fill)
            .style(container_style::queue_panel)
            .into()
    }

    pub fn view_toggle_button(&self) -> Element<'_, QueueMessage> {
        let can_show_queue = self.merged_total() > 0;
        let btn = button(text("󰲸").font(JETBRAINS_MONO).size(typography::TEXT_18))
            .style(button_style::minimal);

        if can_show_queue {
            btn.on_press(QueueMessage::Toggle)
        } else {
            btn
        }.into()
    }

    /// Sincroniza historial, track actual y cola en un solo paso — los
    /// tres cambian juntos en la práctica (ver `PlaybackState::advance_to`),
    /// así que se llama tanto en `QueueChanged` como en `TrackChanged`/
    /// `Stopped` (ver `PlaybackFeature`). Devuelve el `Task` de auto-scroll
    /// hacia el track actual cuando éste cambió.
    pub fn sync_playback(
        &mut self,
        current_track: Option<Track>,
        history: Vec<Track>,
        queue: Vec<QueueSlot>,
    ) -> Task<QueueMessage> {
        self.drag = None;
        self.pending_drag = None;
        self.hovered_delete_history = None;

        let track_changed = self.current_track.as_ref().map(|t| &t.id) != current_track.as_ref().map(|t| &t.id);
        // Posición (antes de sobreescribir) de la fila que ERA la actual —
        // para decidir si seguir el avance o no (ver más abajo).
        let was_visible_before = self.show && self.is_row_visible(self.row_y(self.history.len()));

        let start = history.len().saturating_sub(HISTORY_VISIBLE_CAP);
        self.history = history[start..].to_vec();
        self.current_track = current_track;
        self.queue = queue;

        if self.history.is_empty() && self.current_track.is_none() && self.queue.is_empty() {
            self.show = false;
            self.target_width = QUEUE_COLLAPSED_WIDTH;
            self.animator.clear();
            self.scroll.reset();
            return Task::none();
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
        let max_offset = (self.merged_total() as f32 * ROW_STRIDE + self.extra_gap() - self.scroll.viewport_height).max(0.0);
        self.scroll.offset_y = self.scroll.offset_y.min(max_offset);

        // Seguir el avance solo si el track que ERA actual estaba
        // efectivamente visible (el usuario "iba siguiendo" la cola) — si
        // se había scrolleado a otra parte (historial viejo, cola lejana),
        // no lo interrumpimos. La primera vez que se abre el panel siempre
        // salta al actual (ver `QueueMessage::Toggle`), sin pasar por acá.
        if track_changed && was_visible_before {
            self.scroll_to_current()
        } else {
            Task::none()
        }
    }

    /// `true` si la fila que empieza en `row_px` (alto `ROW_STRIDE`) cae,
    /// aunque sea parcialmente, dentro del viewport visible actual.
    fn is_row_visible(&self, row_px: f32) -> bool {
        if self.scroll.viewport_height <= 0.0 {
            return false;
        }
        let viewport_bottom = self.scroll.offset_y + self.scroll.viewport_height;
        row_px + ROW_STRIDE > self.scroll.offset_y && row_px < viewport_bottom
    }

    /// Auto-scrollea para dejar el track actual centrado en el panel
    /// visible (ni pegado arriba ni abajo), mismo mecanismo que usa
    /// `lyrics_panel.rs` para centrar la línea actual.
    fn scroll_to_current(&mut self) -> Task<QueueMessage> {
        if self.current_track.is_none() {
            return Task::none();
        }

        let row_top = self.row_y(self.history.len());
        let row_center = row_top + ROW_HEIGHT / 2.0;
        let content_height = self.merged_total() as f32 * ROW_STRIDE + self.extra_gap();
        let range = (content_height - self.scroll.viewport_height).max(1.0);

        let target_offset = (row_center - self.scroll.viewport_height / 2.0).clamp(0.0, range);

        // Actualización optimista (mismo patrón que AutoScrollTick): no
        // esperar el roundtrip de QueueMessage::Scrolled para que la
        // ventana virtualizada de este mismo frame ya sea la correcta.
        self.scroll.offset_y = target_offset;

        let y = (target_offset / range).clamp(0.0, 1.0);
        snap_to(Id::new(QUEUE_SCROLL_ID), RelativeOffset { x: 0.0, y })
    }
}

