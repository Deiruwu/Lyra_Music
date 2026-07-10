//! # ContextMenu — menú contextual genérico anclado a click derecho
//!
//!
//! ## Cómo se consume
//!
//! ```ignore
//! // 1. Un campo en tu vista:
//! context_menu: ContextMenu<String>,
//!
//! // 2. El tracking de mouse debe ir en un mouse_area que envuelva TODO
//! //    el área visible con scroll (no cada fila por separado), para
//! //    que el punto que llega ya sea relativo al viewport:
//! mouse_area(scroll_area)
//!     .on_move(ExplorerViewMessage::ViewportMouseMoved)
//! // y en el update:
//! ViewportMouseMoved(point) => self.context_menu.note_mouse_position(point),
//!
//! // 3. Al hacer right-click sobre una fila (el mensaje de click puede
//! //    seguir viniendo de un mouse_area por fila, solo el tracking de
//! //    posición necesita ser a nivel viewport):
//! self.context_menu.toggle(track_id);
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
//!             ("▶  Reproducir ahora", ContextMenuAction::PlayNow),
//!             ("＋  Agregar a cola", ContextMenuAction::Enqueue),
//!         ],
//!         track,
//!         MyMsg::ContextMenuAction,
//!         MyMsg::DismissContextMenu,
//!     );
//!     stack![tu_contenido, menu].into()
//! }

use iced::{Alignment, Color, Element, Length, Padding, Point};
use iced::widget::{button, column, container, mouse_area, row, space, text};

use crate::ui::sidebar_feature::sidebar_feature::SF_PRO;
use crate::ui::styles::styles::{context_menu_container, context_menu_item};

#[derive(Debug, Clone, Default)]
pub struct ContextMenu<Id: PartialEq + Clone> {
    open_for: Option<Id>,
    anchor: Option<Point>,
    last_mouse_in_viewport: Option<Point>,
}

impl<Id: PartialEq + Clone> ContextMenu<Id> {
    pub fn new() -> Self {
        Self { open_for: None, anchor: None, last_mouse_in_viewport: None }
    }

    pub fn note_mouse_position(&mut self, viewport_relative_point: Point) {
        self.last_mouse_in_viewport = Some(viewport_relative_point);
    }

    pub fn toggle(&mut self, id: Id) {
        if self.open_for.as_ref() == Some(&id) {
            self.dismiss();
            return;
        }

        let anchor = self.last_mouse_in_viewport
            .unwrap_or(Point::new(200.0, 40.0));

        self.open_for = Some(id);
        self.anchor = Some(anchor);
    }

    pub fn dismiss(&mut self) {
        self.open_for = None;
        self.anchor = None;
    }

    pub fn open_id(&self) -> Option<&Id> {
        self.open_for.as_ref()
    }

    pub fn render_target<'a, Item>(
        &self,
        resolve: impl FnOnce(&Id) -> Option<&'a Item>,
    ) -> Option<(Point, &'a Item)> {
        let id = self.open_for.as_ref()?;
        let anchor = self.anchor?;
        let item = resolve(id)?;
        Some((anchor, item))
    }

    pub fn view<'a, Item: Clone + 'a, Action: Clone + 'a, Msg: Clone + 'a>(
        &self,
        anchor: Point,
        items: Vec<(&'static str, Action)>,
        item: &'a Item,
        to_msg: impl Fn(Action, Item) -> Msg + Copy + 'a,
        dismiss_msg: Msg,
    ) -> Element<'a, Msg> {
        let mut list = column![].spacing(2);
        for (label, action) in items {
            let item_clone = item.clone();
            list = list.push(
                button(
                    row![text(label).font(SF_PRO).size(13).color(Color::WHITE)]
                        .align_y(Alignment::Center)
                )
                    .width(Length::Fixed(180.0))
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                    .style(context_menu_item)
                    .on_press(to_msg(action, item_clone)),
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