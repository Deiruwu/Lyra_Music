use std::borrow::Cow;

use iced::{Alignment, Color, Element, Length, Padding, Point, Size};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, pin, row, scrollable, space, stack, text};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::styles::button as button_style;
use crate::ui::styles::container as container_style;
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;

const ICON_COLUMN_WIDTH: f32 = 20.0;
const MENU_WIDTH: f32 = 210.0;
const SUBMENU_WIDTH: f32 = 210.0;
/// Alto fijo de cada item: el clamp contra el viewport depende de que sea exacto.
const ITEM_HEIGHT: f32 = 34.0;
const ITEM_SPACING: f32 = spacing::SP_2;
/// Padding interno del contenedor del menú/submenú.
const MENU_INSET: f32 = spacing::SP_4;
/// Separación entre el cursor y la esquina del menú.
const CURSOR_OFFSET_X: f32 = 6.0;
const CURSOR_OFFSET_Y: f32 = 4.0;
/// Distancia mínima entre el menú y los bordes de la ventana.
const VIEWPORT_MARGIN: f32 = 16.0;

/// Alto total de una lista de `item_count` items dentro de su contenedor.
fn list_height(item_count: usize) -> f32 {
    let n = item_count as f32;
    n * ITEM_HEIGHT + (n - 1.0).max(0.0) * ITEM_SPACING + 2.0 * MENU_INSET
}

/// Ancho total de una lista cuyos items miden `item_width`.
fn list_width(item_width: f32) -> f32 {
    item_width + 2.0 * MENU_INSET
}

/// Encaja `start` (con tamaño `size`) dentro de `[margin, limit - margin]`.
fn clamp_axis(start: f32, size: f32, limit: f32) -> f32 {
    let max = (limit - size - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
    start.clamp(VIEWPORT_MARGIN, max)
}

#[derive(Debug, Clone)]
pub enum ContextMenuItem<Action> {
    Leaf {
        label: Cow<'static, str>,
        icon: Option<Icon>,
        /// Color del ícono; `None` usa el del texto.
        tint: Option<Color>,
        action: Action,
    },
    Submenu {
        label: Cow<'static, str>,
        icon: Option<Icon>,
        id: usize,
        children: Vec<ContextMenuItem<Action>>,
    },
}

impl<Action: Clone> ContextMenuItem<Action> {
    pub fn new(label: impl Into<Cow<'static, str>>, action: Action) -> Self {
        ContextMenuItem::Leaf { label: label.into(), icon: None, tint: None, action }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        match &mut self {
            ContextMenuItem::Leaf { icon: i, .. } => *i = Some(icon),
            ContextMenuItem::Submenu { icon: i, .. } => *i = Some(icon),
        }
        self
    }

    /// Pinta el ícono de este color (p. ej. el color de una playlist).
    pub fn tint(mut self, color: Color) -> Self {
        if let ContextMenuItem::Leaf { tint, .. } = &mut self {
            *tint = Some(color);
        }
        self
    }

    pub fn submenu(
        label: impl Into<Cow<'static, str>>,
        id: usize,
        children: Vec<ContextMenuItem<Action>>,
    ) -> Self {
        ContextMenuItem::Submenu { label: label.into(), icon: None, id, children }
    }
}

#[derive(Debug, Clone)]
pub enum ContextMenuEvent<Id> {
    MouseMoved(Point),
    ViewportResized(Size),
    RightClicked(Id),
    SubmenuHovered(Option<usize>),
    Dismissed,
}

#[derive(Debug, Clone)]
#[derive(Default)]
enum MenuState<Id> {
    #[default]
    Closed,
    Open { id: Id, anchor: Point },
}


pub struct ContextMenu<Id: PartialEq + Clone> {
    state: MenuState<Id>,
    last_mouse_in_viewport: Option<Point>,
    open_submenu: Option<usize>,
    viewport_size: Option<Size>,
}

impl<Id: PartialEq + Clone> Default for ContextMenu<Id> {
    fn default() -> Self {
        Self {
            state: MenuState::default(),
            last_mouse_in_viewport: None,
            viewport_size: None,
            open_submenu: None,
        }
    }
}

impl<Id: PartialEq + Clone> ContextMenu<Id> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, event: ContextMenuEvent<Id>) {
        match event {
            ContextMenuEvent::MouseMoved(p) => {
                self.last_mouse_in_viewport = Some(p);
            }
            ContextMenuEvent::ViewportResized(size) => {
                self.viewport_size = Some(size);
            }
            ContextMenuEvent::RightClicked(id) => self.toggle(id),
            ContextMenuEvent::SubmenuHovered(id) => {
                self.open_submenu = id;
            }
            ContextMenuEvent::Dismissed => self.dismiss(),
        }
    }

    fn toggle(&mut self, id: Id) {
        if let MenuState::Open { id: open_id, .. } = &self.state
            && open_id == &id {
                self.dismiss();
                return;
            }

        let anchor = self.last_mouse_in_viewport.unwrap_or(Point::new(200.0, 40.0));
        self.state = MenuState::Open { id, anchor };
        self.open_submenu = None;
    }

    fn dismiss(&mut self) {
        self.state = MenuState::Closed;
        self.open_submenu = None;
    }

    /// Esquina superior izquierda del menú: a la derecha/abajo del cursor,
    /// o del otro lado si no entra, siempre dentro del viewport con margen.
    fn menu_origin(&self, raw: Point, item_count: usize) -> Point {
        let x = raw.x + CURSOR_OFFSET_X;
        let y = raw.y + CURSOR_OFFSET_Y;

        let Some(viewport) = self.viewport_size else {
            return Point::new(x, y);
        };

        let width = list_width(MENU_WIDTH);
        let height = list_height(item_count);

        let x = if x + width + VIEWPORT_MARGIN > viewport.width { raw.x - width } else { x };
        let y = if y + height + VIEWPORT_MARGIN > viewport.height { raw.y - height } else { y };

        Point::new(clamp_axis(x, width, viewport.width), clamp_axis(y, height, viewport.height))
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
        raw_anchor: Point,
        items: Vec<ContextMenuItem<Action>>,
        item: &'a Item,
        to_msg: impl Fn(Action, Item) -> Msg + Copy + 'a,
        dismiss_msg: Msg,
        on_submenu_hover: impl Fn(Option<usize>) -> Msg + Copy + 'a,
    ) -> Element<'a, Msg> {
        let origin = self.menu_origin(raw_anchor, items.len());

        let mut list = column![].spacing(ITEM_SPACING);
        let mut submenu_flyout: Option<(usize, Vec<ContextMenuItem<Action>>)> = None;

        for (row_index, entry) in items.into_iter().enumerate() {
            match entry {
                ContextMenuItem::Leaf { label, icon, tint, action } => {
                    let leaf_button = button(item_content(icon, tint, label, None))
                        .width(Length::Fixed(MENU_WIDTH))
                        .height(Length::Fixed(ITEM_HEIGHT))
                        .padding(item_padding())
                        .style(button_style::context_menu_item)
                        .on_press(to_msg(action, item.clone()));

                    list = list.push(mouse_area(leaf_button).on_enter(on_submenu_hover(None)));
                }
                ContextMenuItem::Submenu { label, icon, id, children } => {
                    let chevron = text("›").font(SF_PRO).size(typography::TEXT_14).color(theme().content.muted);

                    let submenu_button = button(item_content(icon, None, label, Some(chevron.into())))
                        .width(Length::Fixed(MENU_WIDTH))
                        .height(Length::Fixed(ITEM_HEIGHT))
                        .padding(item_padding())
                        .style(button_style::context_menu_item);

                    list = list.push(mouse_area(submenu_button).on_enter(on_submenu_hover(Some(id))));

                    if self.open_submenu == Some(id) {
                        submenu_flyout = Some((row_index, children));
                    }
                }
            }
        }

        let menu = container(list).padding(MENU_INSET).style(container_style::context_menu);

        let mut layers: Vec<Element<'a, Msg>> = Vec::new();

        let dismiss_layer = mouse_area(
            container(space()).width(Length::Fill).height(Length::Fill)
        )
            .on_press(dismiss_msg.clone())
            .on_right_press(dismiss_msg.clone());
        layers.push(dismiss_layer.into());

        layers.push(pin(menu).x(origin.x).y(origin.y).into());

        if let Some((submenu_row_index, children)) = submenu_flyout {
            layers.push(self.view_submenu(origin, submenu_row_index, children, item, to_msg, on_submenu_hover));
        }

        stack(layers).into()
    }

    /// Flyout de un submenú, alineado con la fila `row_index` del menú
    /// principal (que empieza en `menu_origin`). Si no entra en alto, scrollea.
    fn view_submenu<'a, Item: Clone + 'a, Action: Clone + 'a, Msg: Clone + 'a>(
        &self,
        menu_origin: Point,
        row_index: usize,
        children: Vec<ContextMenuItem<Action>>,
        item: &'a Item,
        to_msg: impl Fn(Action, Item) -> Msg + Copy + 'a,
        on_submenu_hover: impl Fn(Option<usize>) -> Msg + Copy + 'a,
    ) -> Element<'a, Msg> {
        let child_count = children.len();
        let mut sub_list = column![].spacing(ITEM_SPACING);

        for child in children {
            if let ContextMenuItem::Leaf { label, icon, tint, action } = child {
                sub_list = sub_list.push(
                    button(item_content(icon, tint, label, None))
                        .width(Length::Fixed(SUBMENU_WIDTH))
                        .height(Length::Fixed(ITEM_HEIGHT))
                        .padding(item_padding())
                        .style(button_style::context_menu_item)
                        .on_press(to_msg(action, item.clone())),
                );
            }
        }

        let width = list_width(SUBMENU_WIDTH);
        let mut height = list_height(child_count);

        // La primera fila del submenú queda a la altura de la fila que lo abrió.
        let mut x = menu_origin.x + list_width(MENU_WIDTH);
        let mut y = menu_origin.y + row_index as f32 * (ITEM_HEIGHT + ITEM_SPACING);

        let body: Element<'a, Msg> = match self.viewport_size {
            Some(viewport) => {
                if x + width + VIEWPORT_MARGIN > viewport.width {
                    x = menu_origin.x - width;
                }
                x = clamp_axis(x, width, viewport.width);

                let max_height = (viewport.height - 2.0 * VIEWPORT_MARGIN).max(ITEM_HEIGHT);
                if height > max_height {
                    height = max_height;
                    y = VIEWPORT_MARGIN;
                    scrollable(sub_list)
                        .height(Length::Fixed(height - 2.0 * MENU_INSET))
                        .style(scrollable_style::discreet)
                        .into()
                } else {
                    y = clamp_axis(y, height, viewport.height);
                    sub_list.into()
                }
            }
            None => sub_list.into(),
        };

        let submenu = container(body).padding(MENU_INSET).style(container_style::context_menu);
        let hoverable = mouse_area(submenu).on_enter(on_submenu_hover(self.open_submenu));

        pin(hoverable).x(x).y(y).into()
    }
}

fn item_padding() -> Padding {
    Padding { top: spacing::SP_0, bottom: spacing::SP_0, left: spacing::SP_12, right: spacing::SP_12 }
}

/// Icono + etiqueta (una sola línea) + trailing opcional, centrado en el alto fijo del item.
fn item_content<'a, Msg: 'a>(
    icon: Option<Icon>,
    tint: Option<Color>,
    label: Cow<'static, str>,
    trailing: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let icon_cell = container(
        match icon {
            Some(i) => icons::icon(i, typography::TEXT_13).color(tint.unwrap_or(theme().content.primary)),
            None => text("").color(theme().content.primary),
        },
    )
        .width(Length::Fixed(ICON_COLUMN_WIDTH))
        .align_x(Alignment::Center);

    let label_cell = text(label)
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.primary)
        .wrapping(Wrapping::None);

    let mut content = row![icon_cell, label_cell].spacing(spacing::SP_8).align_y(Alignment::Center);

    if let Some(trailing) = trailing {
        content = content.push(space().width(Length::Fill)).push(trailing);
    }

    container(content).height(Length::Fill).align_y(Alignment::Center).into()
}
