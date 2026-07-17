//! # ContextMenu — menú contextual genérico anclado a click derecho
//!
//!
//! ## Cómo se consume
//!
//! ```ignore
//! // 1. Un campo en tu vista:
//! context_menu: ContextMenu<String>,
//!
//! // 2. El tracking de mouse Y el tamaño del viewport deben ir en un
//! //    mouse_area/container que envuelva TODO el área visible con
//! //    scroll (no cada fila por separado), para que el punto que
//! //    llega ya sea relativo al viewport y el clamping funcione:
//! mouse_area(scroll_area)
//!     .on_move(ExplorerViewMessage::ViewportMouseMoved)
//! // y en el update:
//! ViewportMouseMoved(point) => self.context_menu.note_mouse_position(point),
//! // El tamaño del viewport se puede obtener de un on_resize / Viewport
//! // de scrollable, o simplemente del tamaño de ventana:
//! ViewportResized(size) => self.context_menu.note_viewport_size(size),
//!
//! // 3. Al hacer right-click sobre una fila (el mensaje de click puede
//! //    seguir viniendo de un mouse_area por fila, solo el tracking de
//! //    posición necesita ser a nivel viewport):
//! self.context_menu.toggle(track_id, item_count);
//!
//! // 4. En tu update(), delegar dismiss:
//! ContextMenuMessage::Dismiss => self.context_menu.dismiss(),
//!
//! // 5. En tu view(), resolver el id abierto a un ítem completo y pedir
//! //    el render. `render_target` regresa `None` si no hay nada
//! //    abierto O si el id abierto ya no resuelve a nada (p. ej. el
//! //    track desapareció del catálogo mientras el menú estaba abierto):
//! if let Some((anchor, track)) = self.context_menu.render_target(|id| store.track_by_id(id)) {
//!     let menu = self.context_menu.view(
//!         anchor,
//!         vec![
//!             ContextMenuItem::new("Reproducir ahora", ContextMenuAction::PlayNow)
//!                 .icon("▶"),
//!             ContextMenuItem::new("Agregar a cola", ContextMenuAction::Enqueue)
//!                 .icon("＋"),
//!             ContextMenuItem::new("Eliminar canción", ContextMenuAction::Delete)
//!                 .icon(""),
//!         ],
//!         track,
//!         MyMsg::ContextMenuAction,
//!         MyMsg::DismissContextMenu,
//!     );
//!     stack![tu_contenido, menu].into()
//! }
//! ```
//!
//! La confirmación antes de ejecutar una acción (p. ej. eliminar) ya no
//! vive en este widget: usa `ConfirmDialog` (mismo módulo padre) como
//! overlay centrado independiente del menú.

use iced::{Alignment, Color, Element, Length, Padding, Point, Size};
use iced::widget::{button, column, container, mouse_area, row, space, text};

use crate::ui::sidebar_feature::sidebar_feature::SF_PRO;
use crate::ui::styles::styles::{context_menu_container, context_menu_item};

const ICON_COLUMN_WIDTH: f32 = 20.0;
const MENU_WIDTH: f32 = 180.0;
const ITEM_HEIGHT: f32 = 34.0;
const MENU_PADDING: f32 = 8.0;
const VIEWPORT_MARGIN: f32 = 8.0;

#[derive(Clone)]
pub struct ContextMenuItem<Action> {
    label: &'static str,
    icon: Option<&'static str>,
    action: Action,
}

impl<Action: Clone> ContextMenuItem<Action> {
    pub fn new(label: &'static str, action: Action) -> Self {
        Self { label, icon: None, action }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
}

#[derive(Debug, Clone)]
enum MenuState<Id> {
    Closed,
    Open { id: Id, anchor: Point },
}

impl<Id> Default for MenuState<Id> {
    fn default() -> Self {
        MenuState::Closed
    }
}

#[derive(Clone)]
pub struct ContextMenu<Id: PartialEq + Clone> {
    state: MenuState<Id>,
    last_mouse_in_viewport: Option<Point>,
    viewport_size: Option<Size>,
}

impl<Id: PartialEq + Clone> Default for ContextMenu<Id> {
    fn default() -> Self {
        Self { state: MenuState::default(), last_mouse_in_viewport: None, viewport_size: None }
    }
}

impl<Id: PartialEq + Clone> ContextMenu<Id> {
    pub fn new() -> Self {
        Self { state: MenuState::Closed, last_mouse_in_viewport: None, viewport_size: None }
    }

    pub fn note_mouse_position(&mut self, viewport_relative_point: Point) {
        self.last_mouse_in_viewport = Some(viewport_relative_point);
    }

    pub fn note_viewport_size(&mut self, size: Size) {
        self.viewport_size = Some(size);
    }

    /// Abre (o cierra si ya estaba abierto para este `id`) el menú.
    /// `item_count` es la cantidad de opciones que se van a mostrar,
    /// usada para estimar la altura del menú y clampear su posición
    /// contra el tamaño del viewport, de modo que nunca quede cortado
    /// fuera de la pantalla.
    pub fn toggle(&mut self, id: Id, item_count: usize) {
        if let MenuState::Open { id: open_id, .. } = &self.state {
            if open_id == &id {
                self.dismiss();
                return;
            }
        }

        let raw_anchor = self.last_mouse_in_viewport
            .unwrap_or(Point::new(200.0, 40.0));

        let anchor = self.clamp_anchor(raw_anchor, item_count);

        self.state = MenuState::Open { id, anchor };
    }

    fn clamp_anchor(&self, raw: Point, item_count: usize) -> Point {
        let Some(viewport) = self.viewport_size else {
            return raw;
        };

        let menu_width = MENU_WIDTH + MENU_PADDING;
        let menu_height = (item_count as f32 * ITEM_HEIGHT) + MENU_PADDING;

        let max_x = (viewport.width - menu_width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
        let max_y = (viewport.height - menu_height - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);

        Point::new(raw.x.min(max_x), raw.y.min(max_y))
    }

    pub fn dismiss(&mut self) {
        self.state = MenuState::Closed;
    }

    pub fn open_id(&self) -> Option<&Id> {
        match &self.state {
            MenuState::Open { id, .. } => Some(id),
            MenuState::Closed => None,
        }
    }

    pub fn render_target<'a, Item>(
        &self,
        resolve: impl FnOnce(&Id) -> Option<&'a Item>,
    ) -> Option<(Point, &'a Item)> {
        let MenuState::Open { id, anchor } = &self.state else {
            return None;
        };
        let item = resolve(id)?;
        Some((*anchor, item))
    }

    pub fn view<'a, Item: Clone + 'a, Action: Clone + 'a, Msg: Clone + 'a>(
        &self,
        anchor: Point,
        items: Vec<ContextMenuItem<Action>>,
        item: &'a Item,
        to_msg: impl Fn(Action, Item) -> Msg + Copy + 'a,
        dismiss_msg: Msg,
    ) -> Element<'a, Msg> {
        let mut list = column![].spacing(2);

        for entry in items {
            let item_clone = item.clone();

            let icon_cell = container(
                text(entry.icon.unwrap_or(""))
                    .font(SF_PRO)
                    .size(13)
                    .color(Color::WHITE),
            )
                .width(Length::Fixed(ICON_COLUMN_WIDTH))
                .align_x(Alignment::Center);

            let label_cell = text(entry.label).font(SF_PRO).size(13).color(Color::WHITE);

            let row_content = row![icon_cell, label_cell]
                .spacing(8)
                .align_y(Alignment::Center);

            list = list.push(
                button(row_content)
                    .width(Length::Fixed(MENU_WIDTH))
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                    .style(context_menu_item)
                    .on_press(to_msg(entry.action, item_clone)),
            );
        }

        let menu = container(list)
            .padding(4)
            .style(context_menu_container);

        let positioned_menu: Element<'a, Msg> = iced::widget::pin(menu)
            .x(anchor.x + 6.0)
            .y(anchor.y + 4.0)
            .into();

        let dismiss_layer = mouse_area(
            container(space()).width(Length::Fill).height(Length::Fill)
        )
            .on_press(dismiss_msg.clone())
            .on_right_press(dismiss_msg);

        iced::widget::stack![dismiss_layer, positioned_menu].into()
    }
}