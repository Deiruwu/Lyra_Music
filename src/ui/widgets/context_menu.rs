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
//! //    track desapareció del catálogo mientras el menú estaba abierto).
//! //    El ítem "Agregar a playlist" es un `ContextMenuItem::submenu`:
//! //    no dispara `Action` por sí mismo, sus `children` sí:
//! if let Some((anchor, track)) = self.context_menu.render_target(|id| store.track_by_id(id)) {
//!     let playlist_children = store.playlists_metadata().iter().map(|(id, name, _)| {
//!         ContextMenuItem::new(name, ContextMenuAction::AddToPlaylist(id.clone()))
//!     }).collect();
//!
//!     let menu = self.context_menu.view(
//!         anchor,
//!         vec![
//!             ContextMenuItem::new("Reproducir ahora", ContextMenuAction::PlayNow)
//!                 .icon("▶"),
//!             ContextMenuItem::new("Agregar a cola", ContextMenuAction::Enqueue)
//!                 .icon("＋"),
//!             ContextMenuItem::submenu("Agregar a playlist", 0, playlist_children)
//!                 .icon("󰐕"),
//!             ContextMenuItem::new("Eliminar canción", ContextMenuAction::Delete)
//!                 .icon(""),
//!         ],
//!         track,
//!         MyMsg::ContextMenuAction,
//!         MyMsg::DismissContextMenu,
//!         MyMsg::ContextMenuSubmenuHover,
//!     );
//!     stack![tu_contenido, menu].into()
//! }
//! // y en el update():
//! ContextMenuSubmenuHover(id) => self.context_menu.set_open_submenu(id),
//! ```
//!
//! La confirmación antes de ejecutar una acción (p. ej. eliminar) ya no
//! vive en este widget: usa `ConfirmDialog` (mismo módulo padre) como
//! overlay centrado independiente del menú.

use std::borrow::Cow;

use iced::{Alignment, Color, Element, Length, Padding, Point, Size};
use iced::widget::{button, column, container, mouse_area, row, space, text};
use crate::ui::assets::icons::Icon;
use crate::ui::sidebar_feature::sidebar_feature::SF_PRO;
use crate::ui::styles::styles::{context_menu_container, context_menu_item};

const ICON_COLUMN_WIDTH: f32 = 20.0;
const MENU_WIDTH: f32 = 180.0;
const SUBMENU_WIDTH: f32 = 200.0;
const ITEM_HEIGHT: f32 = 34.0;
const MENU_PADDING: f32 = 8.0;
const VIEWPORT_MARGIN: f32 = 8.0;

/// Entrada de menú. `Leaf` dispara `Action` directo al click, igual que
/// antes. `Submenu` es un caso especial para "Agregar a playlist": no
/// dispara ninguna acción por sí sola — al pasar el mouse por encima
/// despliega sus `children` ancladas a la derecha (estilo
/// Spotify/Tidal). Solo se soporta un nivel de anidamiento.
#[derive(Clone)]
pub enum ContextMenuItem<Action> {
    Leaf {
        /// `Cow` en vez de `&'static str`: la mayoría de los ítems son
        /// literales estáticos ("Reproducir ahora", etc.), pero los
        /// hijos del submenú "Agregar a playlist" son nombres de
        /// playlist reales del usuario (`String` owned, viven en
        /// `CatalogStore::playlists_metadata`) — `Cow` acepta ambos sin
        /// forzar un leak de memoria (`Box::leak`) en cada render.
        label: Cow<'static, str>,
        icon: Option<&'static str>,
        action: Action,
    },
    Submenu {
        label: Cow<'static, str>,
        icon: Option<&'static str>,
        /// Id opaco del submenú. Como hoy solo existe un submenú por
        /// menú (Agregar a playlist), basta un id fijo (p. ej. `0`);
        /// se deja como parámetro para no cerrar la puerta a un
        /// segundo submenú en el futuro sin romper la firma.
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
    /// Id del `Submenu` actualmente desplegado (hover), si hay alguno.
    /// Se resetea cada vez que el menú principal se abre/cierra.
    open_submenu: Option<usize>,
}

impl<Id: PartialEq + Clone> Default for ContextMenu<Id> {
    fn default() -> Self {
        Self { state: MenuState::default(), last_mouse_in_viewport: None, viewport_size: None, open_submenu: None }
    }
}

impl<Id: PartialEq + Clone> ContextMenu<Id> {
    pub fn new() -> Self {
        Self { state: MenuState::Closed, last_mouse_in_viewport: None, viewport_size: None, open_submenu: None }
    }

    /// Despliega (o cierra) un submenú por id. Llamado desde `on_enter`
    /// / `on_exit` del `mouse_area` que envuelve cada `Submenu` item.
    pub fn set_open_submenu(&mut self, id: Option<usize>) {
        self.open_submenu = id;
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

    pub fn dismiss(&mut self) {
        self.state = MenuState::Closed;
        self.open_submenu = None;
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

    /// Renderiza el menú. `on_submenu_hover` recibe `Option<usize>`
    /// (`Some(id)` al entrar con el mouse a un `Submenu`, `None` al
    /// salir) — la vista lo rutea a `ContextMenu::set_open_submenu` en
    /// su `update()`, mismo patrón que `note_mouse_position`.
    pub fn view<'a, Item: Clone + 'a, Action: Clone + 'a, Msg: Clone + 'a>(
        &self,
        anchor: Point,
        items: Vec<ContextMenuItem<Action>>,
        item: &'a Item,
        to_msg: impl Fn(Action, Item) -> Msg + Copy + 'a,
        dismiss_msg: Msg,
        on_submenu_hover: impl Fn(Option<usize>) -> Msg + Copy + 'a,
    ) -> Element<'a, Msg> {
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

                    // La fila del submenú se pinta como `button` para
                    // reutilizar exactamente el mismo estilo visual que
                    // los `Leaf (incluye estado hover ya resuelto por
                    // el propio widget de botón). No lleva `on_press`:
                    // el despliegue del submenú lo maneja el
                    // `mouse_area` que lo envuelve, vía hover — un click
                    // sobre la fila del submenú no debe hacer nada
                    // (ni cerrar el menú ni disparar una acción), así
                    // que la interactuamos solo vía enter/exit.
                    let _ = is_open; // el estado hover real lo pinta iced vía :hover del button
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

        let menu = container(list)
            .padding(4)
            .style(context_menu_container);

        let mut layers: Vec<Element<'a, Msg>> = Vec::new();

        let dismiss_layer = mouse_area(
            container(space()).width(Length::Fill).height(Length::Fill)
        )
            .on_press(dismiss_msg.clone())
            .on_right_press(dismiss_msg.clone());
        layers.push(dismiss_layer.into());

        let positioned_menu: Element<'a, Msg> = iced::widget::pin(menu)
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
                // Un submenú anidado dentro de otro submenú no está
                // soportado (ver comentario en `ContextMenuItem`): se
                // ignora silenciosamente en vez de entrar en pánico.
            }

            let submenu_container = container(sub_list)
                .padding(4)
                .style(context_menu_container);

            // Reafirma `on_enter` con el mismo id al entrar al flyout:
            // sin esto, mover el mouse desde la fila "Agregar a
            // playlist" hacia el flyout dispararía `on_exit` de la fila
            // (id -> None) antes de que el mouse llegue al flyout,
            // cerrándolo de inmediato y haciendo imposible hacer click
            // en una playlist.
            // CORRECCIÓN: El contenedor del submenú ya no cierra su propio estado al salir.
            let submenu_hoverable = mouse_area(submenu_container)
                .on_enter(on_submenu_hover(submenu_id_for_flyout));

            // Ancla el flyout a la derecha del menú principal, a la
            // misma altura (aproximada) donde vive la fila del
            // submenú: cada fila ocupa `ITEM_HEIGHT` + el spacing de 2
            // que usa `list`, contados desde el padding interno del
            // contenedor del menú.
            let submenu_anchor_y = anchor.y + 4.0 + MENU_PADDING
                + submenu_row_index * (ITEM_HEIGHT + 2.0);

            let positioned_submenu: Element<'a, Msg> = iced::widget::pin(submenu_hoverable)
                .x(anchor.x + 6.0 + MENU_WIDTH + MENU_PADDING)
                .y(submenu_anchor_y)
                .into();

            layers.push(positioned_submenu);
        }

        iced::widget::stack(layers).into()
    }
}