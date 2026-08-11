use std::borrow::Cow;

use iced::{Alignment, Color, Element, Length, Padding, Point, Size};
use iced::widget::{button, column, container, mouse_area, pin, row, space, stack, text};
use crate::ui::assets::icons::Icon;
use crate::ui::sidebar_feature::sidebar_feature::SF_PRO;
use crate::ui::styles::styles::{context_menu_container, context_menu_item};

const ICON_COLUMN_WIDTH: f32 = 20.0;
const MENU_WIDTH: f32 = 180.0;
const SUBMENU_WIDTH: f32 = 200.0;
const ITEM_HEIGHT: f32 = 34.0;
const MENU_PADDING: f32 = 8.0;
const VIEWPORT_MARGIN: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum ContextMenuItem<Action> {
    Leaf {
        label: Cow<'static, str>,
        icon: Option<&'static str>,
        action: Action,
    },
    Submenu {
        label: Cow<'static, str>,
        icon: Option<&'static str>,
        id: usize,
        children: Vec<ContextMenuItem<Action>>,
    },
}

impl<Action: Clone> ContextMenuItem<Action> {
    pub fn new(label: impl Into<Cow<'static, str>>, action: Action) -> Self {
        ContextMenuItem::Leaf { label: label.into(), icon: None, action }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        match &mut self {
            ContextMenuItem::Leaf { icon: i, .. } => *i = Some(icon.into()),
            ContextMenuItem::Submenu { icon: i, .. } => *i = Some(icon.into()),
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
enum MenuState<Id> {
    Closed,
    Open { id: Id, anchor: Point },
}

impl<Id> Default for MenuState<Id> {
    fn default() -> Self {
        MenuState::Closed
    }
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
        if let MenuState::Open { id: open_id, .. } = &self.state {
            if open_id == &id {
                self.dismiss();
                return;
            }
        }

        let anchor = self.last_mouse_in_viewport.unwrap_or(Point::new(200.0, 40.0));
        self.state = MenuState::Open { id, anchor };
        self.open_submenu = None;
    }

    fn dismiss(&mut self) {
        self.state = MenuState::Closed;
        self.open_submenu = None;
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

    pub fn is_open(&self) -> bool {
        matches!(self.state, MenuState::Open { .. })
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
        let anchor = self.clamp_anchor(raw_anchor, items.len());

        let mut list = column![].spacing(2);
        let mut submenu_flyout: Option<(f32, Vec<ContextMenuItem<Action>>)> = None;
        let mut row_index: f32 = 0.0;

        for entry in items {
            let this_row_index = row_index;
            row_index += 1.0;

            match entry {
                ContextMenuItem::Leaf { label, icon, action } => {
                    let item_clone = item.clone();

                    let icon_cell = container(
                        text(icon.unwrap_or(""))
                            .font(SF_PRO)
                            .size(13)
                            .color(Color::WHITE),
                    )
                        .width(Length::Fixed(ICON_COLUMN_WIDTH))
                        .align_x(Alignment::Center);

                    let label_cell = text(label.clone()).font(SF_PRO).size(13).color(Color::WHITE);

                    let row_content = row![icon_cell, label_cell]
                        .spacing(8)
                        .align_y(Alignment::Center);

                    let leaf_button = button(row_content)
                        .width(Length::Fixed(MENU_WIDTH))
                        .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                        .style(context_menu_item)
                        .on_press(to_msg(action, item_clone));

                    let hoverable_leaf = mouse_area(leaf_button)
                        .on_enter(on_submenu_hover(None));

                    list = list.push(hoverable_leaf);
                }
                ContextMenuItem::Submenu { label, icon, id, children } => {
                    let is_open = self.open_submenu == Some(id);

                    let icon_cell = container(
                        text(icon.unwrap_or(""))
                            .font(SF_PRO)
                            .size(13)
                            .color(Color::WHITE),
                    )
                        .width(Length::Fixed(ICON_COLUMN_WIDTH))
                        .align_x(Alignment::Center);

                    let label_cell = text(label.clone()).font(SF_PRO).size(13).color(Color::WHITE);
                    let chevron = text("›").font(SF_PRO).size(14).color(Color::from_rgb(0.6, 0.6, 0.65));

                    let row_content = row![
                        icon_cell,
                        label_cell,
                        space().width(Length::Fill),
                        chevron,
                    ]
                        .spacing(8)
                        .align_y(Alignment::Center);

                    let submenu_button = button(row_content)
                        .width(Length::Fixed(MENU_WIDTH))
                        .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                        .style(context_menu_item);

                    let hoverable_submenu = mouse_area(submenu_button)
                        .on_enter(on_submenu_hover(Some(id)));

                    list = list.push(hoverable_submenu);

                    if is_open {
                        submenu_flyout = Some((this_row_index, children));
                    }
                }
            }
        }

        let menu = container(list).padding(4).style(context_menu_container);

        let mut layers: Vec<Element<'a, Msg>> = Vec::new();

        let dismiss_layer = mouse_area(
            container(space()).width(Length::Fill).height(Length::Fill)
        )
            .on_press(dismiss_msg.clone())
            .on_right_press(dismiss_msg.clone());
        layers.push(dismiss_layer.into());

        let positioned_menu: Element<'a, Msg> = pin(menu)
            .x(anchor.x + 6.0)
            .y(anchor.y + 4.0)
            .into();
        layers.push(positioned_menu);

        if let Some((submenu_row_index, children)) = submenu_flyout {
            let submenu_id_for_flyout = self.open_submenu;
            let mut sub_list = column![].spacing(2);

            for child in children {
                if let ContextMenuItem::Leaf { label, icon, action } = child {
                    let item_clone = item.clone();

                    let icon_cell = container(
                        text(icon.unwrap_or(""))
                            .font(SF_PRO)
                            .size(13)
                            .color(Color::WHITE),
                    )
                        .width(Length::Fixed(ICON_COLUMN_WIDTH))
                        .align_x(Alignment::Center);

                    let label_cell = text(label.clone()).font(SF_PRO).size(13).color(Color::WHITE);

                    let row_content = row![icon_cell, label_cell]
                        .spacing(8)
                        .align_y(Alignment::Center);

                    sub_list = sub_list.push(
                        button(row_content)
                            .width(Length::Fixed(SUBMENU_WIDTH))
                            .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                            .style(context_menu_item)
                            .on_press(to_msg(action, item_clone)),
                    );
                }
            }

            let submenu_container = container(sub_list).padding(4).style(context_menu_container);

            let submenu_hoverable = mouse_area(submenu_container)
                .on_enter(on_submenu_hover(submenu_id_for_flyout));

            let submenu_anchor_y = anchor.y + 4.0 + MENU_PADDING
                + submenu_row_index * (ITEM_HEIGHT + 2.0);

            let mut submenu_x = anchor.x + 6.0 + MENU_WIDTH + MENU_PADDING;

            if let Some(viewport) = self.viewport_size {
                if submenu_x + SUBMENU_WIDTH > viewport.width - VIEWPORT_MARGIN {
                    submenu_x = anchor.x + 6.0 - SUBMENU_WIDTH - MENU_PADDING;
                }
            }

            let positioned_submenu: Element<'a, Msg> = pin(submenu_hoverable)
                .x(submenu_x)
                .y(submenu_anchor_y)
                .into();

            layers.push(positioned_submenu);
        }

        stack(layers).into()
    }
}