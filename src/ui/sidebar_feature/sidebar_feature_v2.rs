use std::sync::Arc;
use iced::{Alignment, Color, Element, Length, Padding, Point, Size, Subscription, Task};
use iced::alignment::{Horizontal, Vertical};
use iced::Event::Mouse;
use iced::mouse::Event::CursorMoved;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, text_input};

use crate::audio::manager::manager::TrackManager;
use crate::db::playlist_manager::PlaylistManager;
use crate::microservices::client::MicroserviceClient;
use crate::ui::assets::fonts::{JETBRAINS_MONO, SF_PRO};
use crate::ui::assets::icons::Icon;
use crate::ui::styles::styles::{minimal_button, transparent_button};
use crate::ui::views::view_coordinator::{ActiveRoute, CoordinatorMessage, ViewCoordinator};
use crate::ui::views::home_view;
use crate::ui::views::explorer_view_v2;
use crate::ui::views::favorite_view;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::context_menu_V2::{ContextMenu, ContextMenuEvent, ContextMenuItem};

const COLLAPSED_WIDTH: f32 = 60.0;
const EXPANDED_WIDTH: f32 = 200.0;
const LERP_FACTOR: f32 = 0.25;
const SNAP_EPSILON: f32 = 0.5;

const PRIMARY_VIEWS: &[ViewData] = &[
    home_view::VIEW_DATA,
    explorer_view_v2::VIEW_DATA,
    favorite_view::VIEW_DATA,
];

// ─── MENSAJES ────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum SidebarMessage {
    ToggleExpanded,
    AnimationTick,
    SelectNav(NavId),
    SelectPlaylist(String),

    Content(CoordinatorMessage),

    ShowCreatePlaylistInput,
    NewPlaylistNameChanged(String),
    SubmitNewPlaylist,
    CancelNewPlaylist,

    // ─── Menú contextual de fila de playlist (sidebar) ──────────────
    PlaylistRowRightClicked(String),
    PlaylistContextMenuEvent(ContextMenuEvent<String>),
    PlaylistContextAction(PlaylistContextAction, String),
    ConfirmDeletePlaylist,
    CancelDeletePlaylist,

    GlobalMouseMoved(Point),
    GlobalWindowResized(Size),
}

/// El menú de fila de playlist es mucho más chico que el de tracks (una
/// sola opción, "Eliminar playlist", sin submenús) — no vale la pena
/// reutilizar el enum completo `TrackContextAction` (que trae variantes
/// de audio/likes que no aplican acá). Se deja como su propio enum en
/// vez de forzar TrackContextAction a cubrir dos dominios distintos.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistContextAction {
    Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarOutMessage {
    Idle,
}

// ─── ESTADO (EL STRUCT) ──────────────────────────────────────────────
pub struct SidebarFeatureV2 {
    pub is_expanded: bool,
    pub sidebar_width: f32,

    /// Dueño de catalog_store, manager, thumbnails y las 3 vistas de
    /// tracks — ver ui::view_coordinator. sidebar_feature_v2 solo
    /// necesita leer `coordinator.active_route` para pintar el highlight
    /// de nav y las filas de playlist; todo lo demás relacionado a
    /// contenido vive y se resuelve adentro del coordinator.
    pub coordinator: ViewCoordinator,

    // Estado local
    pub new_playlist_input: Option<String>,

    // ─── Menú contextual de fila de playlist ────────────────────────
    playlist_context_menu: ContextMenu<String>,
    /// Confirmación antes de borrar una playlist — separado del menú en
    /// sí (mismo criterio que ConfirmDialog en ExplorerView para borrar
    /// tracks): el menú se cierra al elegir "Eliminar", y este diálogo
    /// centrado pide confirmación antes de llamar al coordinator.
    delete_playlist_dialog: ConfirmDialog<String>,
}

impl SidebarFeatureV2 {
    /// Misma firma que SidebarFeature::new() (v1) — construye
    /// ViewCoordinator internamente en vez de recibirlo ya armado, para
    /// que main.rs pueda intercambiar v1 por v2 sin tocar cómo arma sus
    /// dependencias.
    pub fn new(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        manager: Arc<TrackManager>,
    ) -> (Self, Task<SidebarMessage>) {
        let (coordinator, coordinator_task) = ViewCoordinator::new(client, playlist_manager, manager);

        let sidebar = Self {
            is_expanded: false,
            sidebar_width: COLLAPSED_WIDTH,
            coordinator,
            new_playlist_input: None,
            playlist_context_menu: ContextMenu::new(),
            delete_playlist_dialog: ConfirmDialog::new(),
        };

        (sidebar, coordinator_task.map(SidebarMessage::Content))
    }

    pub fn subscription(&self) -> Subscription<SidebarMessage> {
        let animation_sub = if (self.sidebar_width - self.target_width()).abs() > SNAP_EPSILON {
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| SidebarMessage::AnimationTick)
        } else {
            Subscription::none()
        };

        let global_events_sub = iced::event::listen_with(|event, _status, _window| {
            match event {
                Mouse(CursorMoved { position }) => {
                    Some(SidebarMessage::GlobalMouseMoved(position))
                }
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(SidebarMessage::GlobalWindowResized(size))
                }
                _ => None
            }
        });

        Subscription::batch(vec![animation_sub, global_events_sub])
    }

    fn target_width(&self) -> f32 {
        if self.is_expanded { EXPANDED_WIDTH } else { COLLAPSED_WIDTH }
    }

    pub fn update(&mut self, msg: SidebarMessage) -> Task<SidebarMessage> {
        match msg {
            SidebarMessage::ToggleExpanded => {
                self.is_expanded = !self.is_expanded;
                Task::none()
            }

            SidebarMessage::AnimationTick => {
                let target = if self.is_expanded { EXPANDED_WIDTH } else { COLLAPSED_WIDTH };
                let diff = target - self.sidebar_width;

                if diff.abs() <= SNAP_EPSILON {
                    self.sidebar_width = target;
                } else {
                    self.sidebar_width += diff * LERP_FACTOR;
                }
                Task::none()
            }

            SidebarMessage::SelectNav(nav_id) => {
                self.coordinator.update(CoordinatorMessage::SelectNav(nav_id)).map(SidebarMessage::Content)
            }

            SidebarMessage::SelectPlaylist(id) => {
                self.coordinator.update(CoordinatorMessage::SelectPlaylist(id)).map(SidebarMessage::Content)
            }

            SidebarMessage::Content(inner) => {
                self.coordinator.update(inner).map(SidebarMessage::Content)
            }

            SidebarMessage::ShowCreatePlaylistInput => {
                self.new_playlist_input = Some(String::new());
                Task::none()
            }

            SidebarMessage::NewPlaylistNameChanged(name) => {
                self.new_playlist_input = Some(name);
                Task::none()
            }

            SidebarMessage::SubmitNewPlaylist => {
                if let Some(name) = self.new_playlist_input.take() {
                    if !name.trim().is_empty() {
                        self.coordinator.create_playlist(&name);
                    }
                }
                Task::none()
            }

            SidebarMessage::CancelNewPlaylist => {
                self.new_playlist_input = None;
                Task::none()
            }

            SidebarMessage::PlaylistRowRightClicked(id) => {
                self.playlist_context_menu.handle(ContextMenuEvent::RightClicked(id));
                Task::none()
            }
            SidebarMessage::PlaylistContextMenuEvent(event) => {
                self.playlist_context_menu.handle(event);
                Task::none()
            }
            SidebarMessage::PlaylistContextAction(action, playlist_id) => {
                match action {
                    // Único item del menú por ahora — pide confirmación
                    // antes de mutar, mismo patrón que ExplorerView usa
                    // para borrar tracks (ConfirmDialog aparte del menú).
                    PlaylistContextAction::Delete => {
                        let name = self.coordinator
                            .playlists_metadata()
                            .iter()
                            .find(|(pid, _, _)| pid == &playlist_id)
                            .map(|(_, name, _)| name.clone())
                            .unwrap_or_else(|| "esta playlist".to_string());
                        self.delete_playlist_dialog.request(
                            playlist_id,
                            &format!("¿Eliminar la playlist \"{name}\"?"),
                        );
                    }
                }
                // El menú ya cumplió su función al emitir la acción —
                // se cierra igual que en v1, independientemente de si la
                // acción termina confirmándose o cancelándose después.
                self.playlist_context_menu.handle(ContextMenuEvent::Dismissed);
                Task::none()
            }
            SidebarMessage::ConfirmDeletePlaylist => {
                if let Some(playlist_id) = self.delete_playlist_dialog.take_confirmed() {
                    return self.coordinator
                        .delete_playlist(&playlist_id)
                        .map(SidebarMessage::Content);
                }
                Task::none()
            }
            SidebarMessage::CancelDeletePlaylist => {
                self.delete_playlist_dialog.cancel();
                Task::none()
            },
            SidebarMessage::GlobalMouseMoved(position) => {
                self.playlist_context_menu.handle(ContextMenuEvent::MouseMoved(position));

                self.coordinator.update(CoordinatorMessage::TrackContextMenuEvent(
                    ContextMenuEvent::MouseMoved(position)
                )).map(SidebarMessage::Content)
            }

            SidebarMessage::GlobalWindowResized(size) => {
                self.playlist_context_menu.handle(ContextMenuEvent::ViewportResized(size));

                self.coordinator.update(CoordinatorMessage::WindowResized(size)).map(SidebarMessage::Content)
            }
        }
    }

    pub fn view_sidebar(&self) -> Element<'_, SidebarMessage> {
        let width = self.sidebar_width;
        let is_expanded_visual = width > (COLLAPSED_WIDTH + EXPANDED_WIDTH) / 2.0;

        let mut sidebar_colum = column![].spacing(4).width(Length::Fill);

        for view_data in PRIMARY_VIEWS {
            let is_active = self.coordinator.active_route == ActiveRoute::Nav(view_data.id);

            sidebar_colum = sidebar_colum.push(self.render_nav_button(
                view_data,
                is_active,
                is_expanded_visual,
                SidebarMessage::SelectNav(view_data.id),
            ));
        }

        let playlist_header_section: Element<'_, SidebarMessage> = if is_expanded_visual {
            let add_playlist_button = button(
                text("+").font(SF_PRO).size(14).color(Color::from_rgb(0.5, 0.53, 0.6)))
                .padding(Padding { top: 2.0, bottom: 2.0, left: 6.0, right: 6.0 })
                .style(minimal_button)
                .on_press(SidebarMessage::ShowCreatePlaylistInput);

            row![
                text("PLAYLISTS").size(11).font(SF_PRO)
                    .color(Color::from_rgb(0.4, 0.43, 0.5)),
                space().width(Length::Fill),
                add_playlist_button,
            ]
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(Padding { top: 20.0, bottom: 0.0, left: 14.0, right: 10.0 })
                .into()
        } else {
            space().height(20).into()
        };

        let separator_line = container(
            space().height(1).width(Length::Fill)
        ).padding(Padding { top: 8.0, bottom: 12.0, left: 14.0, right: 14.0 });

        let playlists_section: Element<'_, SidebarMessage> = if is_expanded_visual {
            self.render_expanded_playlists()
        } else {
            space().into()
        };

        let sidebar_scroll = scrollable(column![
            space().height(10),
            sidebar_colum,
            playlist_header_section,
            separator_line,
            playlists_section,
            space().height(20),
        ]);

        let base = container(sidebar_scroll)
            .width(Length::Fixed(width))
            .height(Length::Fill);

        base.into()
    }

    // TODO: Moverlo a otro lado después y mejorarlo
    fn render_nav_button(
        &self,
        data: &ViewData,
        is_active: bool,
        is_expanded_visual: bool,
        on_press_msg: SidebarMessage,
    ) -> Element<'_, SidebarMessage> {
        let text_color = if is_active {
            Color::from_rgb(0.74, 0.58, 0.98)
        } else {
            Color::from_rgb(0.5, 0.53, 0.6)
        };

        let icon_elem = text(data.icon.as_str())
            .font(data.icon_font)
            .size(16)
            .color(text_color);

        let content: Element<'_, SidebarMessage> = if is_expanded_visual {
            let label_elem = text(data.label)
                .size(13)
                .font(SF_PRO)
                .color(text_color);

            iced::widget::row![icon_elem, space().width(14), label_elem]
                .align_y(Alignment::Center)
                .into()
        } else {
            container(icon_elem)
                .width(Length::Fill)
                .align_x(Horizontal::Center)
                .into()
        };

        button(content)
            .width(Length::Fill)
            .padding(Padding { top: 9.0, bottom: 9.0, left: 12.0, right: 12.0 })
            .style(transparent_button)
            .on_press(on_press_msg)
            .into()
    }

    /// Compone la barra lateral + el contenido activo en un solo árbol,
    /// con los overlays (menú de fila de playlist, diálogo de
    /// confirmación de borrado) — SIN el menú de track, que vive anidado
    /// dentro de `view_content()` (ver docstring ahí sobre por qué).
    ///
    /// `main.rs` apila estas capas sobre su propio `content_layer`
    /// (sidebar + center + queue ya compuestos), no reemplaza ese árbol —
    /// por eso esto devuelve solo las capas condicionales, no
    /// `view_sidebar()`/`view_content()` de nuevo.
    pub fn view_overlays(&self) -> Vec<Element<'_, SidebarMessage>> {
        let mut layers: Vec<Element<'_, SidebarMessage>> = Vec::new();

        let open_playlist_id = self.playlist_context_menu.open_id();
        if let Some((anchor, playlist_id)) = self.playlist_context_menu.render_target(|_| open_playlist_id) {
            let menu = self.playlist_context_menu.view(
                anchor,
                vec![ContextMenuItem::new("Eliminar playlist", PlaylistContextAction::Delete)
                    .icon(Icon::Delete)],
                playlist_id,
                |action, id: String| SidebarMessage::PlaylistContextAction(action, id),
                SidebarMessage::PlaylistContextMenuEvent(ContextMenuEvent::Dismissed),
                // ¡ESTA ES LA LÍNEA QUE DEBES CAMBIAR!
                // Antes decía: |_sub| ... (None)
                // Ahora pasamos la variable 'sub' intacta:
                |sub| SidebarMessage::PlaylistContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
            );
            layers.push(menu);
        }

        // ─── Confirmación de borrado de playlist ───────────────────────
        if let Some(dialog) = self.delete_playlist_dialog.view(
            SidebarMessage::ConfirmDeletePlaylist,
            SidebarMessage::CancelDeletePlaylist,
        ) {
            layers.push(dialog);
        }

        // ─── NUEVO: Extraemos el menú de Pistas del Coordinador ─────────
        if let Some(track_menu) = self.coordinator.view_track_context_menu() {
            layers.push(track_menu.map(SidebarMessage::Content));
        }

        layers
    }

    /// Ya no recibe un ThumbnailCache externo — el ViewCoordinator es su
    /// dueño; sidebar_feature_v2 solo delega el pintado de contenido.
    ///
    /// Compone el contenido de la vista activa con el menú contextual de
    /// TRACK apilado encima, en el mismo nivel del árbol donde vive el
    /// `mouse_area` que capturó el `Point` original — ver el docstring
    /// de `ViewCoordinator::view_content`/`view_track_context_menu` para
    /// el porqué de las coordenadas relativas.
    pub fn view_content(&self) -> Element<'_, SidebarMessage> {
        self.coordinator.view_content().map(SidebarMessage::Content)
    }

    /// Portado de SidebarFeature::render_expanded_playlists (v1) — mismo
    /// layout (input de nueva playlist arriba si está activo + una fila
    /// por playlist), adaptado a los tipos de v2 (SidebarMessage,
    /// ActiveRoute::Playlist en vez de ActiveSelection::PlaylistDetail).
    fn render_expanded_playlists(&self) -> Element<'_, SidebarMessage> {
        let mut section = column![].spacing(2).width(Length::Fill);

        if let Some(current_value) = &self.new_playlist_input {
            let input = text_input("Nombre de la playlist", current_value)
                .size(12)
                .padding(Padding { top: 6.0, bottom: 6.0, left: 8.0, right: 8.0 })
                .on_input(SidebarMessage::NewPlaylistNameChanged)
                .on_submit(SidebarMessage::SubmitNewPlaylist);

            let input_row = container(input)
                .width(Length::Fill)
                .padding(Padding { top: 2.0, bottom: 6.0, left: 12.0, right: 12.0 });

            section = section.push(input_row);
        }

        for (playlist_id, playlist_name, _cover) in self.coordinator.playlists_metadata() {
            let is_row_active = matches!(
                &self.coordinator.active_route,
                ActiveRoute::Playlist(active_id) if active_id == playlist_id
            );
            let row_color = if is_row_active {
                Color::from_rgb(0.74, 0.58, 0.98)
            } else {
                Color::WHITE
            };

            let id_for_click = playlist_id.clone();
            let id_for_right_click = playlist_id.clone();

            let row_button = button(
                text(playlist_name.as_str())
                    .size(13)
                    .font(SF_PRO)
                    .color(row_color),
            )
                .width(Length::Fill)
                .padding(Padding { top: 8.0, bottom: 8.0, left: 26.0, right: 12.0 })
                .style(transparent_button)
                .on_press(SidebarMessage::SelectPlaylist(id_for_click));

            let row_area = mouse_area(row_button)
                .on_right_press(SidebarMessage::PlaylistRowRightClicked(id_for_right_click));

            section = section.push(row_area);
        }

        section.into()
    }

    pub fn view_toggle(&self) -> Element<'_, SidebarMessage> {
        let icon_char = if self.is_expanded { Icon::Return } else { Icon::BurgerMenu };
        let btn = button(text(icon_char.as_str()).font(JETBRAINS_MONO).size(18))
            .style(minimal_button)
            .on_press(SidebarMessage::ToggleExpanded)
            .padding(8);

        container(btn)
            .width(Length::Fixed(60.0))
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }
}