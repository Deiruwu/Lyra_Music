use std::sync::Arc;
use std::time::{Duration, Instant};
use iced::{Alignment, Element, Length, Padding, Point, Size, Subscription, Task};
use iced::alignment::{Horizontal, Vertical};
use iced::Event::Mouse;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, stack, text, text_input};

use crate::audio::manager::manager::TrackManager;
use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::db::artist_tag_manager::ArtistTagManager;
use crate::db::playlist_color_manager::PlaylistColorManager; // [playlist-color]
use crate::microservices::client::MicroserviceClient;
use crate::model::Mix;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::styles::button as button_style;
use crate::ui::utils::cover_manager::CoverVariant;
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::views::view_coordinator::{ActiveRoute, CoordinatorMessage, CoordinatorOutMessage, ViewCoordinator};
use crate::ui::views::home_view;
use crate::ui::views::explorer_view_v2;
use crate::ui::views::favorite_view;
use crate::ui::views::artists_view;
use crate::ui::views::remix_view;
use crate::ui::views::playlist_view::PlaylistMessage;
use crate::ui::views::artists_view::ArtistsMessage;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::playlist_row::{playlist_row, PlaylistRowData};
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::theme::theme;
use crate::ui::styles::text_input as text_input_style;
use crate::ui::styles::scrollable as scrollable_style;

const COLLAPSED_WIDTH: f32 = 60.0;
const EXPANDED_WIDTH: f32 = 200.0;
const LERP_FACTOR: f32 = 0.25;
const SNAP_EPSILON: f32 = 0.5;

/// Distancia vertical entre filas de playlist (igual expandido y colapsado).
const PLAYLIST_ROW_PITCH: f32 = 54.0;
/// Movimiento mínimo antes de que un press sobre una fila cuente como arrastre.
const PLAYLIST_DRAG_THRESHOLD_PX: f32 = 5.0;
/// Ventana tras soltar un arrastre en la que se ignora el click de selección.
const PLAYLIST_DRAG_CLICK_GUARD: Duration = Duration::from_millis(300);

const PRIMARY_VIEWS: &[ViewData] = &[
    home_view::VIEW_DATA,
    explorer_view_v2::VIEW_DATA,
    favorite_view::VIEW_DATA,
    artists_view::VIEW_DATA,
    remix_view::VIEW_DATA,
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

    // ─── Reordenar playlists (drag & drop) ──────────────────────────
    PlaylistRowHovered(String),
    PlaylistRowUnhovered(String),
    GlobalLeftPressed,
    GlobalLeftReleased,
    /// Repinta mientras las filas de playlist se deslizan a su lugar.
    PlaylistAnimationFrame,

    GlobalWindowResized(Size),
}

/// Arrastre de una fila de playlist; `active` cuando superó el umbral.
#[derive(Debug, Clone)]
struct PlaylistDrag {
    source_index: usize,
    target_index: usize,
    start_y: f32,
    active: bool,
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
    RequestOpenArtist(String),
    RequestOpenAlbum(String),
    RequestOpenMix(Mix),
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

    // ─── Reordenar playlists ────────────────────────────────────────
    hovered_playlist: Option<String>,
    playlist_drag: Option<PlaylistDrag>,
    playlist_drag_ended_at: Option<Instant>,
    /// Posición animada de cada fila mientras se reordena (igual que la cola).
    playlist_animator: RowAnimator,
    cursor: Point,
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
        play_history_manager: Arc<PlayHistoryManager>,
        followed_artist_manager: Arc<FollowedArtistManager>,
        artist_tag_manager: Arc<ArtistTagManager>,
        playlist_colors: Arc<PlaylistColorManager>,
    ) -> (Self, Task<SidebarMessage>) {
        let (coordinator, coordinator_task) = ViewCoordinator::new(
            client,
            playlist_manager,
            manager,
            play_history_manager,
            followed_artist_manager,
            artist_tag_manager,
            playlist_colors,
        );

        let sidebar = Self {
            is_expanded: false,
            sidebar_width: COLLAPSED_WIDTH,
            coordinator,
            new_playlist_input: None,
            playlist_context_menu: ContextMenu::new(),
            delete_playlist_dialog: ConfirmDialog::new(),
            hovered_playlist: None,
            playlist_drag: None,
            playlist_drag_ended_at: None,
            playlist_animator: RowAnimator::new(PLAYLIST_ROW_PITCH),
            cursor: Point::ORIGIN,
        };

        (sidebar, coordinator_task.map(SidebarMessage::Content))
    }

    /// Ver `ViewCoordinator::set_cursor`. Cubre su propio menú de playlists
    /// y el de tracks que vive en el coordinator.
    pub fn set_cursor(&mut self, position: iced::Point) {
        self.playlist_context_menu.handle(ContextMenuEvent::MouseMoved(position));
        self.coordinator.set_cursor(position);
        self.cursor = position;

        let playlist_count = self.coordinator.playlists_metadata().len();
        let mut order_changed = false;
        if let Some(drag) = &mut self.playlist_drag {
            let dy = position.y - drag.start_y;
            if !drag.active && dy.abs() > PLAYLIST_DRAG_THRESHOLD_PX {
                drag.active = true;
                order_changed = true;
            }
            if drag.active {
                let offset = (dy / PLAYLIST_ROW_PITCH).round() as isize;
                let target = drag.source_index.saturating_add_signed(offset).min(playlist_count.saturating_sub(1));
                order_changed |= target != drag.target_index;
                drag.target_index = target;
            }
        }
        if order_changed {
            self.sync_playlist_animator();
        }
    }

    /// Lleva cada fila hacia su posición en el orden que se está pintando.
    fn sync_playlist_animator(&mut self) {
        let now = Instant::now();
        let ids: Vec<String> = self.playlists_in_display_order().iter().map(|(id, _, _)| id.clone()).collect();
        for (index, id) in ids.iter().enumerate() {
            self.playlist_animator.sync_target(id, index, now);
        }
    }

    /// `true` si el click de selección de playlist viene de un arrastre
    /// (en curso o recién soltado) y no debe navegar.
    pub fn is_playlist_drag_click(&self) -> bool {
        self.playlist_drag.as_ref().is_some_and(|drag| drag.active)
            || self.playlist_drag_ended_at.is_some_and(|at| at.elapsed() < PLAYLIST_DRAG_CLICK_GUARD)
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
                Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                    Some(SidebarMessage::GlobalLeftPressed)
                }
                Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(SidebarMessage::GlobalLeftReleased)
                }
                iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                    Some(SidebarMessage::Content(CoordinatorMessage::KeybindsChanged(modifiers)))
                }
                _ => None
            }
        });

        let mut subs = vec![animation_sub, global_events_sub];

        if self.playlist_animator.is_animating(Instant::now()) {
            subs.push(iced::window::frames().map(|_| SidebarMessage::PlaylistAnimationFrame));
        }
        
        if self.coordinator.playlist_view.as_ref().is_some_and(|v| v.is_dragging()) {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(16))
                    .map(|_| SidebarMessage::Content(CoordinatorMessage::PlaylistDetail(
                        PlaylistMessage::AutoScrollTick,
                    ))),
            );
        }

        if self.coordinator.playlist_view.as_ref().is_some_and(|v| v.row_animator.is_animating(std::time::Instant::now())) {
            subs.push(
                iced::window::frames()
                    .map(|instant| SidebarMessage::Content(CoordinatorMessage::PlaylistDetail(
                        PlaylistMessage::AnimationFrame(instant),
                    ))),
            );
        }

        if self.coordinator.home_view.is_animating() {
            subs.push(
                self.coordinator.home_view.subscription()
                    .map(|inner| SidebarMessage::Content(CoordinatorMessage::Home(inner))),
            );
        }

        Subscription::batch(subs)
    }

    fn target_width(&self) -> f32 {
        if self.is_expanded { EXPANDED_WIDTH } else { COLLAPSED_WIDTH }
    }

    /// Fija el estado expandido/colapsado sin animar — para restaurar el
    /// estado guardado al arrancar, en vez de animar desde COLLAPSED_WIDTH.
    pub fn set_expanded_immediate(&mut self, expanded: bool) {
        self.is_expanded = expanded;
        self.sidebar_width = self.target_width();
    }

    pub fn update(&mut self, msg: SidebarMessage) -> (Task<SidebarMessage>, SidebarOutMessage) {
        /// Azúcar local: corre `self.coordinator.update(...)`, mapea el Task
        /// y traduce `CoordinatorOutMessage` a `SidebarOutMessage`.
        fn from_coordinator(
            (task, out): (Task<CoordinatorMessage>, CoordinatorOutMessage),
        ) -> (Task<SidebarMessage>, SidebarOutMessage) {
            let out = match out {
                CoordinatorOutMessage::Idle => SidebarOutMessage::Idle,
                CoordinatorOutMessage::RequestOpenArtist(id) => SidebarOutMessage::RequestOpenArtist(id),
                CoordinatorOutMessage::RequestOpenAlbum(id) => SidebarOutMessage::RequestOpenAlbum(id),
                CoordinatorOutMessage::RequestOpenMix(mix) => SidebarOutMessage::RequestOpenMix(mix),
            };
            (task.map(SidebarMessage::Content), out)
        }

        match msg {
            SidebarMessage::ToggleExpanded => {
                self.is_expanded = !self.is_expanded;
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::AnimationTick => {
                let target = if self.is_expanded { EXPANDED_WIDTH } else { COLLAPSED_WIDTH };
                let diff = target - self.sidebar_width;

                if diff.abs() <= SNAP_EPSILON {
                    self.sidebar_width = target;
                } else {
                    self.sidebar_width += diff * LERP_FACTOR;
                }
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::SelectNav(nav_id) => {
                from_coordinator(self.coordinator.update(CoordinatorMessage::SelectNav(nav_id)))
            }

            SidebarMessage::SelectPlaylist(id) => {
                from_coordinator(self.coordinator.update(CoordinatorMessage::SelectPlaylist(id)))
            }

            SidebarMessage::Content(inner) => {
                from_coordinator(self.coordinator.update(inner))
            }

            SidebarMessage::ShowCreatePlaylistInput => {
                self.new_playlist_input = Some(String::new());
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::NewPlaylistNameChanged(name) => {
                self.new_playlist_input = Some(name);
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::SubmitNewPlaylist => {
                if let Some(name) = self.new_playlist_input.take()
                    && !name.trim().is_empty() {
                        return from_coordinator(
                            self.coordinator.update(CoordinatorMessage::CreatePlaylist(name)),
                        );
                    }
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::CancelNewPlaylist => {
                self.new_playlist_input = None;
                (Task::none(), SidebarOutMessage::Idle)
            }

            SidebarMessage::PlaylistRowRightClicked(id) => {
                self.playlist_context_menu.handle(ContextMenuEvent::RightClicked(id));
                (Task::none(), SidebarOutMessage::Idle)
            }
            SidebarMessage::PlaylistContextMenuEvent(event) => {
                self.playlist_context_menu.handle(event);
                (Task::none(), SidebarOutMessage::Idle)
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
                            .find(|(id, _, _)| id == &playlist_id)
                            .map(|(_, name, _)| name.clone())
                            .unwrap_or_else(|| "esta playlist".to_string());
                        self.delete_playlist_dialog.request(
                            playlist_id,
                            format!("¿Eliminar la playlist \"{name}\"?"),
                        );
                    }
                }
                // El menú ya cumplió su función al emitir la acción —
                // se cierra igual que en v1, independientemente de si la
                // acción termina confirmándose o cancelándose después.
                self.playlist_context_menu.handle(ContextMenuEvent::Dismissed);
                (Task::none(), SidebarOutMessage::Idle)
            }
            SidebarMessage::ConfirmDeletePlaylist => {
                if let Some(playlist_id) = self.delete_playlist_dialog.take_confirmed() {
                    let task = self.coordinator
                        .delete_playlist(&playlist_id)
                        .map(SidebarMessage::Content);
                    return (task, SidebarOutMessage::Idle);
                }
                (Task::none(), SidebarOutMessage::Idle)
            }
            SidebarMessage::CancelDeletePlaylist => {
                self.delete_playlist_dialog.cancel();
                (Task::none(), SidebarOutMessage::Idle)
            },
            SidebarMessage::PlaylistRowHovered(id) => {
                self.hovered_playlist = Some(id);
                (Task::none(), SidebarOutMessage::Idle)
            }
            SidebarMessage::PlaylistRowUnhovered(id) => {
                if self.hovered_playlist.as_ref() == Some(&id) {
                    self.hovered_playlist = None;
                }
                (Task::none(), SidebarOutMessage::Idle)
            }
            SidebarMessage::GlobalLeftPressed => {
                self.playlist_drag = self.hovered_playlist.as_ref().and_then(|hovered| {
                    let index = self.coordinator.playlists_metadata().iter().position(|(id, _, _)| id == hovered)?;
                    Some(PlaylistDrag { source_index: index, target_index: index, start_y: self.cursor.y, active: false })
                });
                let (playlist_task, out) = from_coordinator(self.coordinator.update(CoordinatorMessage::PlaylistDetail(PlaylistMessage::GlobalMousePress)));
                let (artists_task, _) = from_coordinator(self.coordinator.update(CoordinatorMessage::Artists(ArtistsMessage::GlobalPressed)));
                (Task::batch([playlist_task, artists_task]), out)
            }
            SidebarMessage::PlaylistAnimationFrame => (Task::none(), SidebarOutMessage::Idle),
            SidebarMessage::GlobalLeftReleased => {
                let reorder_task = match self.playlist_drag.take() {
                    Some(drag) if drag.active => {
                        self.playlist_drag_ended_at = Some(Instant::now());
                        let dragged_id = self.coordinator.playlists_metadata().get(drag.source_index).map(|(id, _, _)| id.clone());
                        let task = self.coordinator
                            .catalog_store
                            .move_playlist(drag.source_index, drag.target_index)
                            .map(|m| SidebarMessage::Content(CoordinatorMessage::Catalog(m)));
                        // La arrastrada aterriza donde se soltó; el resto ya va camino a su lugar.
                        if let Some(id) = dragged_id {
                            self.playlist_animator.snap_to_target(&id, drag.target_index);
                        }
                        self.sync_playlist_animator();
                        task
                    }
                    _ => Task::none(),
                };
                let (task, out) = from_coordinator(
                    self.coordinator.update(CoordinatorMessage::PlaylistDetail(PlaylistMessage::GlobalMouseRelease)),
                );
                let (artists_task, _) = from_coordinator(self.coordinator.update(CoordinatorMessage::Artists(ArtistsMessage::GlobalReleased)));
                (Task::batch([reorder_task, task, artists_task]), out)
            }
            SidebarMessage::GlobalWindowResized(size) => {
                self.playlist_context_menu.handle(ContextMenuEvent::ViewportResized(size));

                from_coordinator(self.coordinator.update(CoordinatorMessage::WindowResized(size)))
            }
        }
    }

    pub fn view_sidebar(&self) -> Element<'_, SidebarMessage> {
        let width = self.sidebar_width;
        let is_expanded_visual = width > (COLLAPSED_WIDTH + EXPANDED_WIDTH) / 2.0;

        let mut sidebar_colum = column![].spacing(spacing::SP_4).width(Length::Fill);

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
                text("+").font(SF_PRO).size(typography::TEXT_14).color(theme().content.muted))
                .padding(Padding { top: spacing::SP_2, bottom: spacing::SP_2, left: spacing::SP_6, right: spacing::SP_6 })
                .style(button_style::minimal)
                .on_press(SidebarMessage::ShowCreatePlaylistInput);

            row![
                text("PLAYLISTS").size(typography::TEXT_11).font(SF_PRO)
                    .color(theme().content.muted),
                space().width(Length::Fill),
                add_playlist_button,
            ]
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(Padding { top: spacing::SP_20, bottom: spacing::SP_0, left: spacing::SP_14, right: spacing::SP_10 })
                .into()
        } else {
            space().height(20).into()
        };

        let separator_line = container(
            space().height(1).width(Length::Fill)
        ).padding(Padding { top: spacing::SP_8, bottom: spacing::SP_12, left: spacing::SP_14, right: spacing::SP_14 });

        let playlists_section: Element<'_, SidebarMessage> = if is_expanded_visual {
            self.render_expanded_playlists()
        } else {
            self.render_collapsed_playlists()
        };

        let sidebar_scroll = scrollable(column![
            space().height(10),
            sidebar_colum,
            playlist_header_section,
            separator_line,
            playlists_section,
            space().height(20),
        ])
        .style(scrollable_style::discreet);

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
            theme().accent.primary
        } else {
            theme().content.secondary
        };

        let icon_elem = icons::icon(data.icon, typography::TEXT_16).color(text_color);

        let content: Element<'_, SidebarMessage> = if is_expanded_visual {
            let label_elem = text(data.label)
                .size(typography::TEXT_13)
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
            .padding(Padding { top: spacing::SP_9, bottom: spacing::SP_9, left: spacing::SP_12, right: spacing::SP_12 })
            .style(button_style::sidebar_item)
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

        // ─── Menú y diálogo de la vista de Artistas ────────────────────
        layers.extend(self.coordinator.view_artists_overlays().into_iter().map(|layer| layer.map(SidebarMessage::Content)));

        // ─── Editor de recorte de portada ──────────────────────────────
        if let Some(editor) = self.coordinator.view_cover_crop() {
            layers.push(editor.map(SidebarMessage::Content));
        }

        // ─── Confirmación de borrado de tracks ─────────────────────────
        if let Some(dialog) = self.coordinator.view_delete_dialog() {
            layers.push(dialog.map(SidebarMessage::Content));
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
        let mut section = column![].spacing(spacing::SP_2).width(Length::Fill);

        if let Some(current_value) = &self.new_playlist_input {
            let input = text_input("Nombre de la playlist", current_value)
                .size(typography::TEXT_12)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_8, right: spacing::SP_8 })
                .on_input(SidebarMessage::NewPlaylistNameChanged)
                .on_submit(SidebarMessage::SubmitNewPlaylist)
                .style(text_input_style::field);

            let input_row = container(input)
                .width(Length::Fill)
                .padding(Padding { top: spacing::SP_2, bottom: spacing::SP_6, left: spacing::SP_12, right: spacing::SP_12 });

            section = section.push(input_row);
        }

        section = section.push(self.render_playlist_rows(true));

        section.into()
    }

    /// Playlists en el orden a pintar: con un arrastre activo, la arrastrada
    /// ya aparece en su posición de destino.
    fn playlists_in_display_order(&self) -> Vec<&(String, String, Option<String>)> {
        let mut playlists: Vec<_> = self.coordinator.playlists_metadata().iter().collect();

        if let Some(drag) = self.playlist_drag.as_ref().filter(|drag| drag.active)
            && drag.source_index < playlists.len() {
                let moved = playlists.remove(drag.source_index);
                playlists.insert(drag.target_index.min(playlists.len()), moved);
            }

        playlists
    }

    /// Envuelve una fila de playlist para seguir el hover (inicio de arrastre).
    fn draggable_playlist_row<'a>(&self, playlist_id: &str, row: Element<'a, SidebarMessage>) -> Element<'a, SidebarMessage> {
        mouse_area(row)
            .on_enter(SidebarMessage::PlaylistRowHovered(playlist_id.to_string()))
            .on_exit(SidebarMessage::PlaylistRowUnhovered(playlist_id.to_string()))
            .into()
    }

    fn render_collapsed_playlists(&self) -> Element<'_, SidebarMessage> {
        self.render_playlist_rows(false)
    }

    /// Fila de playlist (expandida con nombre y metadata, o colapsada solo con portada).
    fn playlist_row_for<'a>(&'a self, playlist_id: &'a str, playlist_name: &'a str, expanded: bool) -> Element<'a, SidebarMessage> {
        let is_row_active = matches!(
            &self.coordinator.active_route,
            ActiveRoute::Playlist(active_id) if active_id == playlist_id
        );
        let cover_handle = self.coordinator.cover_handle(playlist_id, CoverVariant::Small);
        let (track_count, total_duration_seconds) = if expanded {
            self.coordinator.playlist_track_stats(playlist_id)
        } else {
            (0, 0)
        };

        playlist_row(
            PlaylistRowData {
                name: playlist_name,
                is_active: is_row_active,
                track_count,
                total_duration_seconds,
            },
            cover_handle,
            expanded,
            SidebarMessage::SelectPlaylist(playlist_id.to_string()),
            SidebarMessage::PlaylistRowRightClicked(playlist_id.to_string()),
        )
    }

    /// Filas de playlist en columna; mientras se arrastra o se acomodan, cada
    /// fila va en la posición que le da el animador, con un hueco donde caerá
    /// la arrastrada y su fantasma siguiendo al cursor (igual que la cola).
    fn render_playlist_rows(&self, expanded: bool) -> Element<'_, SidebarMessage> {
        let playlists = self.playlists_in_display_order();
        let row_spacing = if expanded { spacing::SP_2 } else { spacing::SP_6 };
        let row_align = if expanded { Horizontal::Left } else { Horizontal::Center };
        let now = Instant::now();
        let drag = self.playlist_drag.as_ref().filter(|drag| drag.active);

        if drag.is_none() && !self.playlist_animator.is_animating(now) {
            let rows = playlists
                .iter()
                .map(|(id, name, _)| self.draggable_playlist_row(id, self.playlist_row_for(id, name, expanded)));
            return column(rows).spacing(row_spacing).width(Length::Fill).align_x(row_align).into();
        }

        let dragged = drag.and_then(|drag| self.coordinator.playlists_metadata().get(drag.source_index).map(|p| (drag, p)));
        let mut layers: Vec<Element<'_, SidebarMessage>> = Vec::with_capacity(playlists.len() + 1);

        for (index, (id, name, _)) in playlists.iter().enumerate() {
            if dragged.is_some_and(|(_, (dragged_id, _, _))| dragged_id == id) {
                continue;
            }
            let y = self.playlist_animator.visual_y_of(id, now, index);
            layers.push(
                container(self.draggable_playlist_row(id, self.playlist_row_for(id, name, expanded)))
                    .width(Length::Fill)
                    .align_x(row_align)
                    .padding(Padding::new(0.0).top(y))
                    .into(),
            );
        }

        if let Some((drag, (id, name, _))) = dragged {
            let max_y = playlists.len().saturating_sub(1) as f32 * PLAYLIST_ROW_PITCH;
            let y = (drag.source_index as f32 * PLAYLIST_ROW_PITCH + self.cursor.y - drag.start_y).clamp(0.0, max_y);
            let ghost = container(self.playlist_row_for(id, name, expanded)).style(|_theme| container::Style {
                background: Some(theme().overlay.hover.into()),
                border: iced::border::rounded(radii::R_8).color(theme().border.subtle).width(1.0),
                ..Default::default()
            });
            layers.push(
                container(ghost)
                    .width(Length::Fill)
                    .align_x(row_align)
                    .padding(Padding::new(0.0).top(y))
                    .into(),
            );
        }

        let height = (playlists.len() as f32 * PLAYLIST_ROW_PITCH - row_spacing).max(0.0);
        stack(layers).width(Length::Fill).height(Length::Fixed(height)).into()
    }

    pub fn view_toggle(&self) -> Element<'_, SidebarMessage> {
        let icon_char = if self.is_expanded { Icon::Return } else { Icon::BurgerMenu };
        let btn = button(icons::icon(icon_char, typography::TEXT_18))
            .style(button_style::minimal)
            .on_press(SidebarMessage::ToggleExpanded)
            .padding(spacing::SP_8);

        container(btn)
            .width(Length::Fixed(60.0))
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }
}