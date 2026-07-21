use std::sync::Arc;

use iced::{Alignment, Color, Element, Font, Length, Padding, Point, Size, Task};
use iced::widget::{button, column, container, row, scrollable, space, stack, text, text_input};

use crate::JETBRAINS_MONO;
use crate::audio::manager::manager::TrackManager;
use crate::db::playlist_manager::PlaylistManager;
use crate::microservices::client::MicroserviceClient;
use crate::ui::assets::icons::Icon;
use crate::ui::styles::styles::{minimal_button, transparent_button};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuItem};

use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::views::catalog_store::{CatalogStore, CatalogStoreMessage};
use crate::ui::views::home_view::{self, HomeView, HomeViewMessage, HomeViewOutMessage};
use crate::ui::views::explorer_view::{self, ExplorerView, ExplorerViewMessage, ExplorerViewOutMessage};
use crate::ui::views::favorites_view::{self, FavoritesView, FavoritesViewMessage, FavoritesViewOutMessage};
use crate::ui::views::playlists_view::{PlaylistsView, PlaylistsViewMessage, PlaylistsViewOutMessage};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

// ── PARÁMETROS DE LA ANIMACIÓN DEL SIDEBAR ──────────────────────────────────
const COLLAPSED_WIDTH: f32 = 60.0;
const EXPANDED_WIDTH: f32 = 200.0;
const ANIMATION_SPEED: f32 = 12.0;
const SNAP_EPSILON: f32 = 0.5;

// ── REGISTRO ESTÁTICO DE VISTAS PRIMARIAS ───────────────────────────────────
const PRIMARY_VIEWS: &[ViewData] = &[
    home_view::VIEW_DATA,
    explorer_view::VIEW_DATA,
    favorites_view::VIEW_DATA,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveSelection {
    Nav(NavId),
    PlaylistDetail(String),
}

/// Acción disponible en el menú contextual de una fila de playlist.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistContextAction {
    Delete,
}

#[derive(Debug, Clone)]
pub enum SidebarFeatureMessage {
    ToggleExpanded,
    AnimationTick,
    SelectNav(NavId),
    SelectPlaylist(String),
    CreatePlaylistRequested,
    Catalog(CatalogStoreMessage),
    Home(HomeViewMessage),
    Explorer(ExplorerViewMessage),
    Favorites(FavoritesViewMessage),
    Playlists(PlaylistsViewMessage),

    // ── Crear playlist (input inline en el sidebar) ─────────────────────
    /// El "+" fue presionado: muestra el campo de texto inline.
    ShowCreatePlaylistInput,
    /// El usuario tipea en el campo inline.
    NewPlaylistNameChanged(String),
    /// Enter o click en confirmar: crea la playlist si el nombre no está vacío.
    SubmitNewPlaylist,
    /// Esc o click fuera: cierra el campo sin crear nada.
    CancelNewPlaylist,

    // ── Eliminar playlist (click derecho + confirmación) ────────────────
    /// Right-click sobre una fila de playlist. La posición real se toma
    /// del último `ViewportMouseMoved` recibido (ver `ContextMenu::toggle`).
    PlaylistRowRightClicked(String),
    /// Tracking de mouse/viewport requerido por `ContextMenu`. Debe venir
    /// de un `mouse_area` que envuelva TODO `view_sidebar` (no cada fila),
    /// para que las coordenadas ya sean relativas al viewport.
    ViewportMouseMoved(Point),
    /// TODO: `mouse_area` no expone tamaño de viewport directamente. Falta
    /// conectar esto a un evento real (p. ej. `window::resize_events()` o
    /// un `on_resize` de `scrollable`) para que `ContextMenu::clamp_anchor`
    /// tenga datos reales; por ahora el menú puede quedar sin clamping.
    ViewportResized(Size),
    PlaylistContextMenuAction(PlaylistContextAction, String),
    DismissPlaylistContextMenu,
    /// El menú de playlist del sidebar no tiene ningún `Submenu` hoy
    /// (solo "Eliminar playlist"), pero `ContextMenu::view` ahora pide
    /// siempre un callback de hover de submenú — este mensaje solo
    /// existe para satisfacer esa firma; su `update()` es un no-op real
    /// (`set_open_submenu` nunca tendrá efecto visible sin un
    /// `ContextMenuItem::submenu` en la lista).
    DismissPlaylistContextMenuSubmenuHover(Option<usize>),
    ConfirmDeletePlaylist,
    CancelDeletePlaylist,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarFeatureOutMessage {
    Idle,
}

pub struct SidebarFeature {
    pub is_expanded: bool,
    pub sidebar_width: f32,
    pub target_width: f32,
    pub active_selection: ActiveSelection,
    pub catalog_store: CatalogStore,
    manager: Arc<TrackManager>,

    /// `Some(texto_actual)` mientras el campo inline de "nueva playlist"
    /// está visible; `None` cuando está oculto (estado por defecto).
    new_playlist_input: Option<String>,

    playlist_context_menu: ContextMenu<String>,
    delete_playlist_dialog: ConfirmDialog<String>,

    pub home_view: HomeView,
    pub explorer_view: ExplorerView,
    pub favorites_view: FavoritesView,
    pub playlists_view: PlaylistsView,
}

impl SidebarFeature {
    pub fn new(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        manager: Arc<TrackManager>,
    ) -> (Self, Task<SidebarFeatureMessage>) {
        let (catalog_store, catalog_task) = CatalogStore::load(client, playlist_manager);

        let feature = Self {
            is_expanded: false,
            sidebar_width: COLLAPSED_WIDTH,
            target_width: COLLAPSED_WIDTH,
            active_selection: ActiveSelection::Nav(NavId::Home),
            catalog_store,
            manager,
            new_playlist_input: None,
            playlist_context_menu: ContextMenu::new(),
            delete_playlist_dialog: ConfirmDialog::new(),
            home_view: HomeView::new(),
            explorer_view: ExplorerView::new(),
            favorites_view: FavoritesView::new(),
            playlists_view: PlaylistsView::new(),
        };

        (feature, catalog_task.map(SidebarFeatureMessage::Catalog))
    }

    pub fn subscription(&self) -> iced::Subscription<SidebarFeatureMessage> {
        if (self.sidebar_width - self.target_width).abs() > SNAP_EPSILON {
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| SidebarFeatureMessage::AnimationTick)
        } else {
            iced::Subscription::none()
        }
    }

    pub fn update(
        &mut self,
        msg: SidebarFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<SidebarFeatureMessage>, SidebarFeatureOutMessage) {
        match msg {
            SidebarFeatureMessage::ToggleExpanded => {
                self.is_expanded = !self.is_expanded;
                self.target_width = if self.is_expanded { EXPANDED_WIDTH } else { COLLAPSED_WIDTH };
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::AnimationTick => {
                let delta = self.target_width - self.sidebar_width;
                if delta.abs() <= SNAP_EPSILON {
                    self.sidebar_width = self.target_width;
                } else {
                    self.sidebar_width += delta * (ANIMATION_SPEED / 60.0).min(1.0);
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::SelectNav(nav_id) => {
                self.active_selection = ActiveSelection::Nav(nav_id);
                let mut tasks = vec![];

                if nav_id == NavId::PlaylistsOverview {
                    if !self.is_expanded {
                        self.is_expanded = true;
                        self.target_width = EXPANDED_WIDTH;
                    }

                    let (task, _out) = self.playlists_view.update(
                        PlaylistsViewMessage::BackToOverview,
                        &self.catalog_store,
                        thumbnails,
                    );
                    tasks.push(task.map(SidebarFeatureMessage::Playlists));
                }
                (Task::batch(tasks), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::SelectPlaylist(id) => {
                self.active_selection = ActiveSelection::PlaylistDetail(id.clone());

                let (task, _out) = self.playlists_view.update(
                    PlaylistsViewMessage::SelectPlaylist(id),
                    &self.catalog_store,
                    thumbnails,
                );

                (task.map(SidebarFeatureMessage::Playlists), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::CreatePlaylistRequested => {
                self.new_playlist_input = Some(String::new());
                if !self.is_expanded {
                    self.is_expanded = true;
                    self.target_width = EXPANDED_WIDTH;
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::ShowCreatePlaylistInput => {
                self.new_playlist_input = Some(String::new());
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::NewPlaylistNameChanged(value) => {
                if let Some(current) = self.new_playlist_input.as_mut() {
                    *current = value;
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::SubmitNewPlaylist => {
                let Some(name) = self.new_playlist_input.take() else {
                    return (Task::none(), SidebarFeatureOutMessage::Idle);
                };
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    return (Task::none(), SidebarFeatureOutMessage::Idle);
                }

                let task = self.catalog_store.create_playlist(trimmed);
                (task.map(SidebarFeatureMessage::Catalog), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::CancelNewPlaylist => {
                self.new_playlist_input = None;
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::PlaylistRowRightClicked(id) => {
                self.playlist_context_menu.toggle(id, 1);
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::ViewportMouseMoved(point) => {
                self.playlist_context_menu.note_mouse_position(point);
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::ViewportResized(size) => {
                self.playlist_context_menu.note_viewport_size(size);
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::PlaylistContextMenuAction(action, playlist_id) => {
                self.playlist_context_menu.dismiss();
                match action {
                    PlaylistContextAction::Delete => {
                        self.delete_playlist_dialog
                            .request(playlist_id, "¿Eliminar esta playlist?");
                    }
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::DismissPlaylistContextMenu => {
                self.playlist_context_menu.dismiss();
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::DismissPlaylistContextMenuSubmenuHover(id) => {
                self.playlist_context_menu.set_open_submenu(id);
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::ConfirmDeletePlaylist => {
                let Some(playlist_id) = self.delete_playlist_dialog.take_confirmed() else {
                    return (Task::none(), SidebarFeatureOutMessage::Idle);
                };

                let mut tasks = vec![];

                if let ActiveSelection::PlaylistDetail(active_id) = &self.active_selection {
                    if active_id == &playlist_id {
                        self.active_selection = ActiveSelection::Nav(NavId::PlaylistsOverview);
                        let (task, _out) = self.playlists_view.update(
                            PlaylistsViewMessage::BackToOverview,
                            &self.catalog_store,
                            thumbnails,
                        );
                        tasks.push(task.map(SidebarFeatureMessage::Playlists));
                    }
                }

                let task = self.catalog_store.delete_playlist(&playlist_id);
                tasks.push(task.map(SidebarFeatureMessage::Catalog));
                (Task::batch(tasks), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::CancelDeletePlaylist => {
                self.delete_playlist_dialog.cancel();
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::Catalog(msg) => {
                let store_task = self.catalog_store.update(msg);

                let (explorer_task, _out) = self.explorer_view.update(
                    ExplorerViewMessage::CatalogUpdated,
                    &self.catalog_store,
                    thumbnails,
                );

                let (favorites_task, _out) = self.favorites_view.update(
                    FavoritesViewMessage::CatalogUpdated,
                    &self.catalog_store,
                    thumbnails,
                );

                let task = Task::batch(vec![
                    store_task.map(SidebarFeatureMessage::Catalog),
                    explorer_task.map(SidebarFeatureMessage::Explorer),
                    favorites_task.map(SidebarFeatureMessage::Favorites),
                ]);

                (task, SidebarFeatureOutMessage::Idle)
            }

            // ── BUBBLE-UP DE MENSAJES ───────────────────────────────────────
            SidebarFeatureMessage::Home(msg) => {
                let (task, out_msg) = self.home_view.update(msg);
                let out = match out_msg {
                    HomeViewOutMessage::Idle => SidebarFeatureOutMessage::Idle,
                };
                (task.map(SidebarFeatureMessage::Home), out)
            }

            SidebarFeatureMessage::Explorer(msg) => {
                let (task, out_msg) = self.explorer_view.update(msg, &self.catalog_store, thumbnails);

                let mut extra_tasks = vec![task.map(SidebarFeatureMessage::Explorer)];

                match out_msg {
                    ExplorerViewOutMessage::RequestPlay(track) => {
                        self.manager.play_now(track);
                    }
                    ExplorerViewOutMessage::RequestEnqueue(track) => {
                        self.manager.enqueue(track);
                    }

                    ExplorerViewOutMessage::RequestFrontEnqueue(track) => {
                        self.manager.enqueue_front(track);
                    }

                    ExplorerViewOutMessage::RequestPlayRadio(track) => {
                        self.manager.play_now(track);
                        self.manager.clear_queue().unwrap();
                    }

                    ExplorerViewOutMessage::RequestDelete(track_id) => {
                        self.catalog_store.delete_track(&track_id)
                    }

                    ExplorerViewOutMessage::RequestToggleLike(track_id) => {
                        let like_task = self.catalog_store.toggle_like(&track_id);
                        extra_tasks.push(like_task.map(SidebarFeatureMessage::Catalog));
                    }

                    ExplorerViewOutMessage::RequestAddToPlaylist(playlist_id, track_id) => {
                        let add_task = self.catalog_store.add_track_to_playlist(&playlist_id, &track_id);
                        extra_tasks.push(add_task.map(SidebarFeatureMessage::Catalog));
                    }

                    ExplorerViewOutMessage::Idle => {}
                }
                (Task::batch(extra_tasks), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::Favorites(msg) => {
                let (task, out_msg) = self.favorites_view.update(msg, &self.catalog_store, thumbnails);

                let mut extra_tasks = vec![task.map(SidebarFeatureMessage::Favorites)];

                match out_msg {
                    FavoritesViewOutMessage::RequestPlayContext(tracks, context_id) => {
                        self.manager.play_context(tracks, context_id);
                    }
                    FavoritesViewOutMessage::RequestEnqueue(track) => {
                        self.manager.enqueue(track);
                    }
                    FavoritesViewOutMessage::RequestFrontEnqueue(track) => {
                        self.manager.enqueue_front(track);
                    }
                    FavoritesViewOutMessage::RequestPlayRadio(track) => {
                        self.manager.play_now(track);
                        self.manager.clear_queue().unwrap();
                    }
                    FavoritesViewOutMessage::RequestToggleLike(track_id) => {
                        let like_task = self.catalog_store.toggle_like(&track_id);
                        extra_tasks.push(like_task.map(SidebarFeatureMessage::Catalog));
                    }
                    FavoritesViewOutMessage::RequestAddToPlaylist(playlist_id, track_id) => {
                        let add_task = self.catalog_store.add_track_to_playlist(&playlist_id, &track_id);
                        extra_tasks.push(add_task.map(SidebarFeatureMessage::Catalog));
                    }
                    FavoritesViewOutMessage::Idle => {}
                }

                (Task::batch(extra_tasks), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::Playlists(msg) => {
                let (task, out_msg) = self.playlists_view.update(
                    msg,
                    &self.catalog_store,
                    thumbnails,
                );

                let mut extra_tasks = vec![task.map(SidebarFeatureMessage::Playlists)];

                match out_msg {
                    PlaylistsViewOutMessage::RequestPlayContext(tracks, context_id) => {
                        self.manager.play_context(tracks, context_id);
                    }
                    PlaylistsViewOutMessage::RequestEnqueue(track) => {
                        self.manager.enqueue(track);
                    }
                    PlaylistsViewOutMessage::RequestFrontEnqueue(track) => {
                        self.manager.enqueue_front(track);
                    }
                    PlaylistsViewOutMessage::RequestPlayRadio(track) => {
                        self.manager.play_now(track);
                        self.manager.clear_queue().unwrap();
                    }
                    PlaylistsViewOutMessage::RequestRemoveFromPlaylist(playlist_id, track_id) => {
                        let remove_task = self.catalog_store.remove_track_from_playlist(&playlist_id, &track_id);
                        extra_tasks.push(remove_task.map(SidebarFeatureMessage::Catalog));
                    }
                    PlaylistsViewOutMessage::RequestToggleLike(track_id) => {
                        let like_task = self.catalog_store.toggle_like(&track_id);
                        extra_tasks.push(like_task.map(SidebarFeatureMessage::Catalog));
                    }
                    PlaylistsViewOutMessage::RequestAddToPlaylist(playlist_id, track_id) => {
                        let add_task = self.catalog_store.add_track_to_playlist(&playlist_id, &track_id);
                        extra_tasks.push(add_task.map(SidebarFeatureMessage::Catalog));
                    }
                    PlaylistsViewOutMessage::CreatePlaylistRequested => {
                        self.new_playlist_input = Some(String::new());
                        if !self.is_expanded {
                            self.is_expanded = true;
                            self.target_width = EXPANDED_WIDTH;
                        }
                    }
                    PlaylistsViewOutMessage::Idle => {}
                };
                (Task::batch(extra_tasks), SidebarFeatureOutMessage::Idle)
            }
        }
    }

    // ── INTERFAZ DESENSAMBLADA PARA MAIN.RS ─────────────────────────────────

    pub fn view_toggle(&self) -> Element<'_, SidebarFeatureMessage> {
        let icon_char = if self.is_expanded { "\u{f060}" } else { "\u{f0c9}" };
        let btn = button(text(icon_char).font(JETBRAINS_MONO).size(18))
            .style(minimal_button)
            .on_press(SidebarFeatureMessage::ToggleExpanded)
            .padding(8);

        container(btn)
            .width(Length::Fixed(60.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    pub fn view_sidebar(&self) -> Element<'_, SidebarFeatureMessage> {
        let width = self.sidebar_width;
        let is_expanded_visual = width > (COLLAPSED_WIDTH + EXPANDED_WIDTH) / 2.0;

        let mut primary_col = column![].spacing(4).width(Length::Fill);

        for view_data in PRIMARY_VIEWS {
            let is_active = self.active_selection == ActiveSelection::Nav(view_data.id);
            primary_col = primary_col.push(self.render_nav_button(
                view_data,
                is_active,
                is_expanded_visual,
                SidebarFeatureMessage::SelectNav(view_data.id),
            ));
        }

        let section_label: Element<'_, SidebarFeatureMessage> = if is_expanded_visual {
            let muted_color = Color::from_rgb(0.5, 0.53, 0.6);

            let add_button = button(
                text("+").font(SF_PRO).size(14)
                    .style(move |_| text::Style { color: Some(muted_color) }),
            )
                .padding(Padding { top: 2.0, bottom: 2.0, left: 6.0, right: 6.0 })
                .style(minimal_button)
                .on_press(SidebarFeatureMessage::ShowCreatePlaylistInput);

            row![
                text("PLAYLISTS").size(10.5).font(SF_PRO)
                    .style(|_| text::Style { color: Some(Color::from_rgb(0.4, 0.43, 0.5)) }),
                space().width(Length::Fill),
                add_button,
            ]
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(Padding { top: 20.0, bottom: 0.0, left: 14.0, right: 10.0 })
                .into()
        } else {
            space().height(20).into()
        };

        let separator = container(
            container(space()).width(Length::Fill).height(1).style(|_| container::Style {
                background: Some(Color::from_rgb(0.25, 0.28, 0.35).into()),
                ..Default::default()
            })
        ).width(Length::Fill).padding(Padding { top: 8.0, bottom: 12.0, left: 14.0, right: 14.0 });

        let playlists_section: Element<'_, SidebarFeatureMessage> = if is_expanded_visual {
            self.render_expanded_playlists()
        } else {
            space().into()
        };

        let content_scroll = scrollable(column![
            space().height(10),
            primary_col,
            section_label,
            separator,
            playlists_section,
            space().height(20),
        ]);

        let base = container(content_scroll)
            .width(Length::Fixed(width))
            .height(Length::Fill);

        let tracked: Element<'_, SidebarFeatureMessage> = iced::widget::mouse_area(base)
            .on_move(SidebarFeatureMessage::ViewportMouseMoved)
            .into();

        self.view_sidebar_with_overlays(tracked)
    }

    /// EL ENRUTADOR DE CONTENIDO: Cada distrito renderiza su propio DOM.
    pub fn view_content<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, SidebarFeatureMessage> {
        match &self.active_selection {
            ActiveSelection::Nav(NavId::Home) => {
                self.home_view.view().map(SidebarFeatureMessage::Home)
            }
            ActiveSelection::Nav(NavId::Explorer) => {
                self.explorer_view.view(&self.catalog_store, thumbnails).map(SidebarFeatureMessage::Explorer)
            }
            ActiveSelection::Nav(NavId::Favorites) => {
                self.favorites_view.view(&self.catalog_store, thumbnails).map(SidebarFeatureMessage::Favorites)
            }
            ActiveSelection::Nav(NavId::PlaylistsOverview) | ActiveSelection::PlaylistDetail(_) => {
                self.playlists_view.view(&self.catalog_store, thumbnails).map(SidebarFeatureMessage::Playlists)
            }
        }
    }

    fn render_nav_button(
        &self,
        data: &ViewData,
        is_active: bool,
        is_expanded_visual: bool,
        on_press_msg: SidebarFeatureMessage,
    ) -> Element<'_, SidebarFeatureMessage> {
        let text_color = if is_active {
            Color::from_rgb(0.74, 0.58, 0.98)
        } else {
            Color::from_rgb(0.5, 0.53, 0.6)
        };

        let icon_elem = text(data.icon.as_str())
            .font(data.icon_font)
            .size(16)
            .style(move |_| text::Style { color: Some(text_color) });

        let content: Element<'_, SidebarFeatureMessage> = if is_expanded_visual {
            let label_elem = text(data.label)
                .size(13)
                .font(SF_PRO)
                .style(move |_| text::Style { color: Some(text_color) });

            row![icon_elem, space().width(14), label_elem].align_y(Alignment::Center).into()
        } else {
            container(icon_elem).width(Length::Fill).align_x(iced::alignment::Horizontal::Center).into()
        };

        button(content)
            .width(Length::Fill)
            .padding(Padding { top: 9.0, bottom: 9.0, left: 12.0, right: 12.0 })
            .style(transparent_button)
            .on_press(on_press_msg)
            .into()
    }

    fn render_expanded_playlists(&self) -> Element<'_, SidebarFeatureMessage> {
        let mut section = column![].spacing(2).width(Length::Fill);

        if let Some(current_value) = &self.new_playlist_input {
            let input = text_input("Nombre de la playlist", current_value)
                .size(12)
                .padding(Padding { top: 6.0, bottom: 6.0, left: 8.0, right: 8.0 })
                .on_input(SidebarFeatureMessage::NewPlaylistNameChanged)
                .on_submit(SidebarFeatureMessage::SubmitNewPlaylist);

            let input_row = container(input)
                .width(Length::Fill)
                .padding(Padding { top: 2.0, bottom: 6.0, left: 12.0, right: 12.0 });

            section = section.push(input_row);
        }

        for (playlist_id, playlist_name, _) in self.catalog_store.playlists_metadata() {
            let is_row_active = matches!(
                &self.active_selection,
                ActiveSelection::PlaylistDetail(active_id) if active_id == playlist_id
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
                    .style(move |_| text::Style { color: Some(row_color) }),
            )
                .width(Length::Fill)
                .padding(Padding { top: 8.0, bottom: 8.0, left: 26.0, right: 12.0 })
                .style(transparent_button)
                .on_press(SidebarFeatureMessage::SelectPlaylist(id_for_click));

            let row_area = iced::widget::mouse_area(row_button)
                .on_right_press(SidebarFeatureMessage::PlaylistRowRightClicked(id_for_right_click));

            section = section.push(row_area);
        }

        section.into()
    }

    pub fn view_sidebar_with_overlays<'a>(
        &'a self,
        content: Element<'a, SidebarFeatureMessage>,
    ) -> Element<'a, SidebarFeatureMessage> {
        let mut layers = vec![content];

        if let Some((anchor, entry)) = self
            .playlist_context_menu
            .render_target(|id| self.catalog_store.playlists_metadata().iter().find(|(pid, _, _)| pid == id))
        {
            let menu = self.playlist_context_menu.view(
                anchor,
                vec![ContextMenuItem::new("Eliminar playlist", PlaylistContextAction::Delete).icon(Icon::Delete)],
                entry,
                |action, (id, _name, _cover)| SidebarFeatureMessage::PlaylistContextMenuAction(action, id),
                SidebarFeatureMessage::DismissPlaylistContextMenu,
                SidebarFeatureMessage::DismissPlaylistContextMenuSubmenuHover,
            );
            layers.push(menu);
        }

        if let Some(dialog) = self.delete_playlist_dialog.view(
            SidebarFeatureMessage::ConfirmDeletePlaylist,
            SidebarFeatureMessage::CancelDeletePlaylist,
        ) {
            layers.push(dialog);
        }

        stack(layers).into()
    }
}