use std::sync::Arc;

use iced::{Alignment, Color, Element, Font, Length, Padding, Task};
use iced::widget::{button, column, container, row, scrollable, space, text};

use crate::JETBRAINS_MONO;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

// Importamos los distritos y sus descriptores (ViewData)
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::views::home_view::{self, HomeView, HomeViewMessage, HomeViewOutMessage};
use crate::ui::views::explorer_view::{self, ExplorerView, ExplorerViewMessage, ExplorerViewOutMessage};
use crate::ui::views::favorites_view::{self, FavoritesView, FavoritesViewMessage, FavoritesViewOutMessage};
use crate::ui::views::playlists_view::{self, PlaylistsView, PlaylistsViewMessage, PlaylistsViewOutMessage};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

// ── PARÁMETROS DE LA ANIMACIÓN DEL SIDEBAR ──────────────────────────────────
const COLLAPSED_WIDTH: f32 = 60.0;
const EXPANDED_WIDTH: f32 = 200.0;
const ANIMATION_SPEED: f32 = 12.0; // qué tan rápido converge el lerp (mayor = más rápido)
const SNAP_EPSILON: f32 = 0.5; // debajo de esto, se considera "llegó" y para el tick

// ── REGISTRO ESTÁTICO DE VISTAS PRIMARIAS ───────────────────────────────────
// Conforme metas archivos a `src/ui/views/`, solo agregas su `VIEW_DATA` aquí
// y el menú lateral se armará solo en orden.
const PRIMARY_VIEWS: &[ViewData] = &[
    home_view::VIEW_DATA,
    explorer_view::VIEW_DATA,
    favorites_view::VIEW_DATA,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveSelection {
    Nav(NavId),
    PlaylistDetail(String), // Caso especial para cuando estás dentro de una playlist
}

#[derive(Debug, Clone)]
pub enum SidebarFeatureMessage {
    ToggleExpanded,
    AnimationTick,
    SelectNav(NavId),
    SelectPlaylist(String),
    CreatePlaylistRequested,
    // Ruteo local de mensajes hacia los distritos:
    Home(HomeViewMessage),
    Explorer(ExplorerViewMessage),
    Favorites(FavoritesViewMessage),
    Playlists(PlaylistsViewMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarFeatureOutMessage {
    Idle,
    RequestPlay(Track),
    RequestEnqueue(Track),
    CreatePlaylistRequested,
}

pub struct SidebarFeature {
    pub is_expanded: bool,
    pub sidebar_width: f32,
    pub target_width: f32,
    pub active_selection: ActiveSelection,
    pub playlists_metadata: Vec<(String, String)>,

    // Instancias soberanas del distrito:
    pub home_view: HomeView,
    pub explorer_view: ExplorerView,
    pub favorites_view: FavoritesView,
    pub playlists_view: PlaylistsView,
}

impl SidebarFeature {
    /// Construye el feature completo. Necesita el cliente de microservicios
    /// porque ExplorerView lo usa para resolver su catálogo al arrancar.
    /// Regresa también el Task inicial (la carga del catálogo de Explorer)
    /// para que main.rs lo mezcle en su Task de init.
    pub fn new(client: Arc<MicroserviceClient>) -> (Self, Task<SidebarFeatureMessage>) {
        let (explorer_view, explorer_task) = ExplorerView::new(client);

        let feature = Self {
            is_expanded: false,
            sidebar_width: COLLAPSED_WIDTH,
            target_width: COLLAPSED_WIDTH,
            active_selection: ActiveSelection::Nav(NavId::Home),
            playlists_metadata: Vec::new(),
            home_view: HomeView::new(),
            explorer_view,
            favorites_view: FavoritesView::new(),
            playlists_view: PlaylistsView::new(),
        };

        (feature, explorer_task.map(SidebarFeatureMessage::Explorer))
    }

    /// La animación solo corre mientras el ancho actual no ha alcanzado el target.
    /// El main.rs debe mezclar esto en su `Subscription::batch`.
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
                    // Lerp exponencial: converge rápido al inicio, suave al final.
                    self.sidebar_width += delta * (ANIMATION_SPEED / 60.0).min(1.0);
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::SelectNav(nav_id) => {
                self.active_selection = ActiveSelection::Nav(nav_id);
                if nav_id == NavId::PlaylistsOverview {
                    if !self.is_expanded {
                        self.is_expanded = true;
                        self.target_width = EXPANDED_WIDTH;
                    }
                    self.playlists_view.show_overview();
                }
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::SelectPlaylist(id) => {
                self.active_selection = ActiveSelection::PlaylistDetail(id.clone());
                self.playlists_view.open_playlist(&id);
                (Task::none(), SidebarFeatureOutMessage::Idle)
            }

            SidebarFeatureMessage::CreatePlaylistRequested => {
                (Task::none(), SidebarFeatureOutMessage::CreatePlaylistRequested)
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
                let (task, out_msg) = self.explorer_view.update(msg, thumbnails);
                let out = match out_msg {
                    ExplorerViewOutMessage::RequestPlay(t) => SidebarFeatureOutMessage::RequestPlay(t),
                    ExplorerViewOutMessage::Idle => SidebarFeatureOutMessage::Idle,
                };
                (task.map(SidebarFeatureMessage::Explorer), out)
            }

            SidebarFeatureMessage::Favorites(msg) => {
                let (task, out_msg) = self.favorites_view.update(msg);
                let out = match out_msg {
                    FavoritesViewOutMessage::Idle => SidebarFeatureOutMessage::Idle,
                };
                (task.map(SidebarFeatureMessage::Favorites), out)
            }

            SidebarFeatureMessage::Playlists(msg) => {
                let (task, out_msg) = self.playlists_view.update(msg);
                let out = match out_msg {
                    PlaylistsViewOutMessage::RequestPlayNow(t) => SidebarFeatureOutMessage::RequestPlay(t),
                    PlaylistsViewOutMessage::RequestEnqueue(t) => SidebarFeatureOutMessage::RequestEnqueue(t),
                    PlaylistsViewOutMessage::PlaylistSelected(id) => {
                        self.active_selection = ActiveSelection::PlaylistDetail(id);
                        SidebarFeatureOutMessage::Idle
                    }
                    PlaylistsViewOutMessage::Idle => SidebarFeatureOutMessage::Idle,
                    PlaylistsViewOutMessage::CreatePlaylistRequested => {
                        SidebarFeatureOutMessage::CreatePlaylistRequested
                    }
                };
                (task.map(SidebarFeatureMessage::Playlists), out)
            }
        }
    }

    // ── INTERFAZ DESENSAMBLADA PARA MAIN.RS ─────────────────────────────────

    /// El botón hamburguesa/toggle que vive en la top bar.
    pub fn view_toggle(&self) -> Element<'_, SidebarFeatureMessage> {
        let icon_char = if self.is_expanded { "\u{f060}" } else { "\u{f0c9}" };
        let btn = button(text(icon_char).font(JETBRAINS_MONO).size(18))
            .style(transparent_button)
            .on_press(SidebarFeatureMessage::ToggleExpanded)
            .padding(8);

        container(btn)
            .width(Length::Fixed(60.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    /// El menú lateral en sí: armando botones dinámicamente usando ViewData.
    /// El ancho usa `self.sidebar_width` (el valor animado), no el destino,
    /// así el contenedor se desliza frame a frame en vez de saltar.
    pub fn view_sidebar(&self) -> Element<'_, SidebarFeatureMessage> {
        let width = self.sidebar_width;
        let is_expanded_visual = width > (COLLAPSED_WIDTH + EXPANDED_WIDTH) / 2.0;

        let mut primary_col = column![].spacing(4).width(Length::Fill);

        // 1. RECORREMOS EL ARREGLO DE METADATA PARA CREAR BOTONES PRIMARIOS
        for view_data in PRIMARY_VIEWS {
            let is_active = self.active_selection == ActiveSelection::Nav(view_data.id);
            primary_col = primary_col.push(self.render_nav_button(
                view_data,
                is_active,
                is_expanded_visual,
                SidebarFeatureMessage::SelectNav(view_data.id),
            ));
        }

        // 2. SEPARADOR DE 1PX
        let separator = container(
            container(space()).width(Length::Fill).height(1).style(|_| container::Style {
                background: Some(Color::from_rgb(0.25, 0.28, 0.35).into()),
                ..Default::default()
            })
        ).width(Length::Fill).padding(Padding { top: 12.0, bottom: 12.0, left: 14.0, right: 14.0 });

        // 3. SECCIÓN DE PLAYLISTS (su propio bloque bajo la línea divisora)
        let playlists_section: Element<'_, SidebarFeatureMessage> = if is_expanded_visual {
            self.render_expanded_playlists()
        } else {
            let is_active = matches!(
                self.active_selection,
                ActiveSelection::Nav(NavId::PlaylistsOverview) | ActiveSelection::PlaylistDetail(_)
            );
            let pl_data = ViewData::new(NavId::PlaylistsOverview, "\u{f00b}", "Playlists", JETBRAINS_MONO);
            self.render_nav_button(
                &pl_data,
                is_active,
                is_expanded_visual,
                SidebarFeatureMessage::SelectNav(NavId::PlaylistsOverview),
            )
        };

        let content_scroll = scrollable(column![
            space().height(10),
            primary_col,
            separator,
            playlists_section,
            space().height(20),
        ]);

        container(content_scroll)
            .width(Length::Fixed(width))
            .height(Length::Fill)
            .into()
    }

    /// EL ENRUTADOR DE CONTENIDO: Cada distrito renderiza su propio DOM
    pub fn view_content<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, SidebarFeatureMessage> {
        match &self.active_selection {
            ActiveSelection::Nav(NavId::Home) => {
                self.home_view.view().map(SidebarFeatureMessage::Home)
            }
            ActiveSelection::Nav(NavId::Explorer) => {
                self.explorer_view.view(thumbnails).map(SidebarFeatureMessage::Explorer)
            }
            ActiveSelection::Nav(NavId::Favorites) => {
                self.favorites_view.view().map(SidebarFeatureMessage::Favorites)
            }
            ActiveSelection::Nav(NavId::PlaylistsOverview) | ActiveSelection::PlaylistDetail(_) => {
                self.playlists_view.view().map(SidebarFeatureMessage::Playlists)
            }
        }
    }

    // ── HELPER GENÉRICO PARA DIBUJAR UN BOTÓN USANDO VIEW_DATA ──────────────

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

        let icon_elem = text(data.icon)
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
        let is_active = matches!(
            self.active_selection,
            ActiveSelection::Nav(NavId::PlaylistsOverview) | ActiveSelection::PlaylistDetail(_)
        );
        let text_color = if is_active {
            Color::from_rgb(0.74, 0.58, 0.98)
        } else {
            Color::from_rgb(0.5, 0.53, 0.6)
        };

        let header = button(
            row![
                text("\u{f00b}").font(JETBRAINS_MONO).size(16)
                    .style(move |_| text::Style { color: Some(text_color) }),
                space().width(14),
                text("Playlists").size(13).font(SF_PRO)
                    .style(move |_| text::Style { color: Some(text_color) }),
            ]
                .align_y(Alignment::Center),
        )
            .width(Length::Fill)
            .padding(Padding { top: 9.0, bottom: 9.0, left: 12.0, right: 12.0 })
            .style(transparent_button)
            .on_press(SidebarFeatureMessage::SelectNav(NavId::PlaylistsOverview));

        let list_col = column![].spacing(2).width(Length::Fill);

        column![header, list_col].spacing(2).width(Length::Fill).into()
    }
}