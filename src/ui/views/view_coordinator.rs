use std::sync::Arc;
use iced::{Element, Size, Task};

use crate::audio::manager::manager::TrackManager;
use crate::db::playlist_manager::PlaylistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::search::SearchQuery;
use crate::ui::views::catalog_store::{CatalogStore, CatalogStoreMessage};
use crate::ui::views::home_view::{HomeView, HomeViewMessage};
use crate::ui::views::explorer_view_v2::{ExplorerView, ExplorerMessage, ExplorerExtra};
use crate::ui::views::favorite_view::{FavoritesView, FavoritesMessage};
use crate::ui::views::playlist_view::{PlaylistView, PlaylistMessage, PlaylistExtra};
use crate::ui::views::view_data::NavId;
use crate::ui::widgets::context_menu_V2::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::track_context_builder::TrackContextAction;
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;

/// Misma semántica que `ActiveRoute` en sidebar_feature_v2 — vive acá
/// duplicado a propósito en vez de importado: el ViewCoordinator es el
/// dueño real de "qué vista de contenido está activa" (Home/Explorer/
/// Favorites/Playlist), mientras que sidebar_feature_v2 solo necesita
/// saber la variante para pintar el highlight del botón de nav — así que
/// se re-exporta desde acá y sidebar_feature_v2 usa este tipo directo,
/// no al revés.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveRoute {
    Nav(NavId),
    Playlist(String),
}

#[derive(Debug, Clone)]
pub enum CoordinatorMessage {
    SelectNav(NavId),
    SelectPlaylist(String),

    Catalog(CatalogStoreMessage),
    Home(HomeViewMessage),
    Explorer(ExplorerMessage),
    Favorites(FavoritesMessage),
    PlaylistDetail(PlaylistMessage),

    // ─── Menú contextual de track (Explorer/Favorites/Playlist) ────
    TrackContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),

    // ─── Miniaturas ──────────────────────────────────────────────
    ThumbnailLoaded(String, Vec<u8>),
    WindowResized(Size)
}

/// Dueño de todo lo que las 3 vistas de tracks (Explorer/Favorites/
/// Playlist) más Home comparten y necesitan para funcionar: el
/// catalog_store, el manager de reproducción, el cache de thumbnails y
/// el menú contextual de track. sidebar_feature_v2 lo contiene como
/// campo, pero no conoce ni toca nada de lo que hay acá adentro salvo a
/// través de esta API pública — así queda liviano y solo con lo que es
/// genuinamente "sidebar" (expand/collapse, nav, menú de fila de
/// playlist, crear playlist).
pub struct ViewCoordinator {
    pub active_route: ActiveRoute,

    pub catalog_store: CatalogStore,
    pub manager: Arc<TrackManager>,

    pub thumbnails: AsyncThumbnail,

    pub home_view: HomeView,
    pub explorer_view: ExplorerView,
    pub favorites_view: FavoritesView,
    pub playlist_view: Option<PlaylistView>,

    /// ContextMenu<String> solo trackea el id abierto y el punto de
    /// anclaje — no los `items` a mostrar, porque esos dependen del
    /// track puntual (¿está likeado?, ¿en qué playlist estamos?) y ya
    /// vienen armados por la vista que emitió ContextMenuRightClicked
    /// (ver TrackContextMenuBuilder).
    track_context_menu: ContextMenu<String>,
    track_context_menu_items: Vec<ContextMenuItem<TrackContextAction>>,
}

impl ViewCoordinator {
    pub fn new(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        manager: Arc<TrackManager>,
    ) -> (Self, Task<CoordinatorMessage>) {
        let (catalog_store, catalog_task) = CatalogStore::load(client, playlist_manager);

        let coordinator = Self {
            active_route: ActiveRoute::Nav(NavId::Home),
            catalog_store,
            manager,
            thumbnails: AsyncThumbnail::new(),
            home_view: HomeView::new(),
            explorer_view: ExplorerView::new(),
            favorites_view: FavoritesView::new(),
            playlist_view: None,
            track_context_menu: ContextMenu::new(),
            track_context_menu_items: Vec::new(),
        };

        (coordinator, catalog_task.map(CoordinatorMessage::Catalog))
    }

    /// Único punto de entrada público. Delega el manejo del mensaje a
    /// `update_route`, y al final — sin importar qué rama corrió —
    /// sincroniza el caché de thumbnails contra la ventana visible de la
    /// vista activa ahora mismo. No depende de que cada rama se acuerde
    /// de pedir/podar thumbnails: corre siempre.
    pub fn update(&mut self, msg: CoordinatorMessage) -> Task<CoordinatorMessage> {
        let route_task = self.update_route(msg);

        let wanted = self.active_view_thumbnail_targets();
        let sync_task = self.thumbnails.sync(&wanted, CoordinatorMessage::ThumbnailLoaded);

        Task::batch([route_task, sync_task])
    }

    fn update_route(&mut self, msg: CoordinatorMessage) -> Task<CoordinatorMessage> {
        match msg {
            CoordinatorMessage::SelectNav(nav_id) => {
                self.active_route = ActiveRoute::Nav(nav_id);
                self.playlist_view = None;
                Task::none()
            }

            CoordinatorMessage::SelectPlaylist(id) => {
                self.active_route = ActiveRoute::Playlist(id.clone());
                self.playlist_view = Some(PlaylistView::new(id));
                Task::none()
            }

            // ─── EXPLORER ────────────────────────────────────────────────
            CoordinatorMessage::Explorer(inner) => {
                let rendered_tracks: Vec<Track> = self.catalog_store.all_tracks().to_vec();
                let all_refs: Vec<&Track> = rendered_tracks.iter().collect();

                let rendered_refs = filter_tracks(&all_refs, &self.explorer_view.list.search_filter);
                let play_context = filter_tracks_owned(&rendered_tracks, &self.explorer_view.list.search_filter);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.explorer_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::Explorer);

                let out_task = self.handle_track_list_out(out, &play_context, |store, extra| match extra {
                    ExplorerExtra::RequestDelete(ids) => {
                        // TODO: delete_track es singular; se batchea id a id
                        // hasta que CatalogStore soporte borrado múltiple.
                        for id in ids {
                            store.delete_track(&id);
                        }
                        Task::none()
                    }
                });

                Task::batch([view_task, out_task])
            }

            // ─── FAVORITES ───────────────────────────────────────────────
            CoordinatorMessage::Favorites(inner) => {
                let rendered_tracks: Vec<Track> = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id())
                    .into_iter()
                    .cloned()
                    .collect();
                let all_refs: Vec<&Track> = rendered_tracks.iter().collect();
                let rendered_refs = filter_tracks(&all_refs, &self.favorites_view.list.search_filter);
                let play_context = filter_tracks_owned(&rendered_tracks, &self.favorites_view.list.search_filter);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.favorites_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::Favorites);

                let out_task = self.handle_track_list_out(out, &play_context, |_store, extra| match extra {});

                Task::batch([view_task, out_task])
            }

            // ─── PLAYLIST DETAIL ─────────────────────────────────────────
            CoordinatorMessage::PlaylistDetail(inner) => {
                let Some(playlist_view) = &mut self.playlist_view else {
                    return Task::none();
                };
                let playlist_id = playlist_view.playlist_id.clone();

                let rendered_tracks: Vec<Track> = self.catalog_store
                    .tracks_for_playlist(&playlist_id)
                    .into_iter()
                    .cloned()
                    .collect();
                let all_refs: Vec<&Track> = rendered_tracks.iter().collect();
                let rendered_refs = filter_tracks(&all_refs, &playlist_view.list.search_filter);
                let play_context = filter_tracks_owned(&rendered_tracks, &playlist_view.list.search_filter);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = playlist_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::PlaylistDetail);

                let out_task = self.handle_track_list_out(out, &play_context, |store, extra| match extra {
                    PlaylistExtra::RequestReorder { playlist_id, from, to } => {
                        store.reorder_track_in_playlist(&playlist_id, from, to)
                            .map(CoordinatorMessage::Catalog)
                    }
                    PlaylistExtra::RequestRemoveTracks { playlist_id, track_ids } => {
                        // TODO: remove_track_from_playlist es singular; se
                        // batchea id a id hasta soportar remoción múltiple.
                        let tasks: Vec<_> = track_ids
                            .into_iter()
                            .map(|id| {
                                store.remove_track_from_playlist(&playlist_id, &id)
                                    .map(CoordinatorMessage::Catalog)
                            })
                            .collect();
                        Task::batch(tasks)
                    }
                });

                Task::batch([view_task, out_task])
            }

            CoordinatorMessage::Catalog(inner) => {
                self.catalog_store.update(inner).map(CoordinatorMessage::Catalog)
            }

            CoordinatorMessage::Home(inner) => {
                self.home_view.update(inner);
                Task::none()
            }

            CoordinatorMessage::TrackContextMenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.track_context_menu_items.clear();
                }
                self.track_context_menu.handle(event);
                Task::none()
            }
            CoordinatorMessage::TrackContextAction(action, track_id) => {
                let task = self.handle_track_context_action(action, track_id);
                self.track_context_menu.handle(ContextMenuEvent::Dismissed);
                self.track_context_menu_items.clear();
                task
            }

            CoordinatorMessage::ThumbnailLoaded(key, bytes) => {
                self.thumbnails.on_loaded(key, bytes);
                Task::none()
            }

            CoordinatorMessage::WindowResized(size) => {
                self.track_context_menu.handle(ContextMenuEvent::ViewportResized(size));
                Task::none()
            }
        }
    }

    /// Punto único donde se resuelve TrackListOutMessage<Extra> para
    /// cualquiera de las 3 vistas. Las variantes comunes (play/enqueue/
    /// like/context-menu) se manejan acá una sola vez; lo específico de
    /// cada vista se delega al callback `on_extra`, que recibe &mut
    /// CatalogStore porque varias mutaciones de dominio son async
    /// (devuelven Task<CatalogStoreMessage>) y hay que enrutarlas de
    /// vuelta, no tirarlas al piso.
    fn handle_track_list_out<Extra>(
        &mut self,
        out: TrackListOutMessage<Extra>,
        play_context: &[Track],
        on_extra: impl FnOnce(&mut CatalogStore, Extra) -> Task<CoordinatorMessage>,
    ) -> Task<CoordinatorMessage> {
        match out {
            TrackListOutMessage::Idle => Task::none(),

            TrackListOutMessage::RequestPlayContext { start_track_id } => {
                if let Some(start_index) = play_context.iter().position(|t| t.id == start_track_id) {
                    self.manager.play_context(play_context.to_vec(), start_index);
                }
                Task::none()
            }
            TrackListOutMessage::RequestEnqueue(ids) => {
                // TODO: enqueue es de un solo Track; se resuelve id a id.
                for id in ids {
                    if let Some(track) = self.catalog_store.track_by_id(&id) {
                        self.manager.enqueue(track.clone());
                    }
                }
                Task::none()
            }
            TrackListOutMessage::RequestFrontEnqueue(ids) => {
                // TODO: enqueue_front también es de un solo Track.
                for id in ids {
                    if let Some(track) = self.catalog_store.track_by_id(&id) {
                        self.manager.enqueue_front(track.clone());
                    }
                }
                Task::none()
            }
            TrackListOutMessage::RequestPlayRadio(_track_id) => {
                // TODO: TrackManager no expone play_radio en esta lista de
                // firmas; falta decidir cómo se arma el modo radio.
                Task::none()
            }

            TrackListOutMessage::RequestToggleLike(ids) => {
                // TODO: toggle_like es singular; se batchea id a id.
                let tasks: Vec<_> = ids
                    .into_iter()
                    .map(|id| {
                        self.catalog_store.toggle_like(&id).map(CoordinatorMessage::Catalog)
                    })
                    .collect();
                Task::batch(tasks)
            }
            TrackListOutMessage::RequestAddToPlaylist { target_playlist_id, track_ids } => {
                // TODO: add_track_to_playlist es singular; se batchea id a id.
                let tasks: Vec<_> = track_ids
                    .into_iter()
                    .map(|id| {
                        self.catalog_store
                            .add_track_to_playlist(&target_playlist_id, &id)
                            .map(CoordinatorMessage::Catalog)
                    })
                    .collect();
                Task::batch(tasks)
            }

            TrackListOutMessage::RequestSearch(_query) => {
                // No-op intencional: la vista ya guardó `query` en su propio
                // `search_filter` (ver *Message::SearchInputChanged en cada
                // vista) antes de emitir este mensaje.
                Task::none()
            }
            TrackListOutMessage::RequestChangeSort(_sort_key) => {
                // No-op intencional: cada vista ya resuelve su propio
                // (active_sort_key, sort_direction_asc) localmente.
                Task::none()
            }

            TrackListOutMessage::ContextMenuRightClicked { track_id, items, .. } => {
                self.track_context_menu.handle(ContextMenuEvent::RightClicked(track_id));
                self.track_context_menu_items = items;
                Task::none()
            }

            TrackListOutMessage::Extra(extra) => on_extra(&mut self.catalog_store, extra),
        }
    }

    /// Traduce una acción elegida en el menú contextual de track a la
    /// llamada real correspondiente.
    fn handle_track_context_action(&mut self, action: TrackContextAction, track_id: String) -> Task<CoordinatorMessage> {
        match action {
            TrackContextAction::PlayNow => {
                if let Some(track) = self.catalog_store.track_by_id(&track_id) {
                    self.manager.play_context(vec![track.clone()], 0);
                }
                Task::none()
            }
            TrackContextAction::Enqueue => {
                if let Some(track) = self.catalog_store.track_by_id(&track_id) {
                    self.manager.enqueue(track.clone());
                }
                Task::none()
            }
            TrackContextAction::FrontEnqueue => {
                if let Some(track) = self.catalog_store.track_by_id(&track_id) {
                    self.manager.enqueue_front(track.clone());
                }
                Task::none()
            }
            TrackContextAction::ToggleLike => {
                self.catalog_store.toggle_like(&track_id).map(CoordinatorMessage::Catalog)
            }
            TrackContextAction::AddToPlaylist(target_playlist_id) => {
                self.catalog_store
                    .add_track_to_playlist(&target_playlist_id, &track_id)
                    .map(CoordinatorMessage::Catalog)
            }
            TrackContextAction::CopyId => {
                iced::clipboard::write(track_id)
            }
            TrackContextAction::DeleteFromCatalog => {
                self.catalog_store.delete_track(&track_id);
                Task::none()
            }
            TrackContextAction::RemoveFromPlaylist => {
                let ActiveRoute::Playlist(playlist_id) = &self.active_route else {
                    return Task::none();
                };
                let playlist_id = playlist_id.clone();
                self.catalog_store
                    .remove_track_from_playlist(&playlist_id, &track_id)
                    .map(CoordinatorMessage::Catalog)
            }
        }
    }

    /// Universo `(key, url)` de la ventana visible de la vista activa
    /// ahora mismo. Único lugar donde se decide qué debe seguir vivo en
    /// AsyncThumbnail — reemplaza a active_view_color_keys +
    /// active_view_track_ids del sistema anterior (dos HashSet, uno por
    /// tipo de caché); acá solo hay un caché, así que un solo Vec basta.
    fn active_view_thumbnail_targets(&self) -> Vec<(String, String)> {
        match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => {
                let all_tracks = self.catalog_store.all_tracks();
                let all_refs: Vec<&Track> = all_tracks.iter().collect();
                let mut tracks = filter_tracks(&all_refs, &self.explorer_view.list.search_filter);

                crate::ui::widgets::track_list_builder::sort_tracks(
                    &mut tracks,
                    self.explorer_view.list.active_sort_key,
                    self.explorer_view.list.sort_direction_asc,
                );

                self.explorer_view.list.visible_thumbnail_targets(&tracks)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let mut tracks = filter_tracks(&liked, &self.favorites_view.list.search_filter);

                crate::ui::widgets::track_list_builder::sort_tracks(
                    &mut tracks,
                    self.favorites_view.list.active_sort_key,
                    self.favorites_view.list.sort_direction_asc,
                );

                self.favorites_view.list.visible_thumbnail_targets(&tracks)
            }
            ActiveRoute::Playlist(id) => {
                let Some(view) = &self.playlist_view else {
                    return Vec::new();
                };
                let all = self.catalog_store.tracks_for_playlist(id);
                let mut tracks = filter_tracks(&all, &view.list.search_filter);

                crate::ui::widgets::track_list_builder::sort_tracks(
                    &mut tracks,
                    view.list.active_sort_key,
                    view.list.sort_direction_asc,
                );

                // Las portadas ya viven pre-codificadas en `playlists_metadata`
                // (son locales; no pasan por el caché de thumbnails ni el
                // semáforo), así que aquí solo entran las miniaturas de track.
                view.list.visible_thumbnail_targets(&tracks)
            }
            _ => Vec::new(),
        }
    }

    /// Compone el contenido de la vista activa (Home/Explorer/Favorites/
    /// PlaylistDetail) con el menú contextual de TRACK apilado encima, en
    /// el mismo nivel del árbol donde vive el `mouse_area` que capturó el
    /// `Point` original (dentro de `TrackBuilder`, vía `TrackEvent::
    /// MouseMoved` → `ContextMenuRightClicked { position, .. }`).
    ///
    /// Por qué acá y no en un stack de nivel superior en sidebar_feature_v2
    /// / main.rs: `position` llega relativo al `mouse_area` que envuelve
    /// la tabla de tracks, no al viewport global de la ventana. Si el
    /// `pin(menu)` se pinta en un `stack` de nivel superior, ese mismo
    /// número se reinterpreta en OTRO sistema de coordenadas — el menú
    /// queda desplazado por la suma de todo lo que hay "antes" en el
    /// árbol (top_bar, padding de center_view, header de playlist, etc.),
    /// sin que haya ningún cálculo que lo esté compensando.
    ///
    /// Anidando el stack ACÁ, `pin(menu).x(anchor.x)` vuelve a estar en
    /// el mismo contenedor padre que originó el `Point`.
    pub fn view_content(&self) -> Element<'_, CoordinatorMessage> {
        match &self.active_route {
            ActiveRoute::Nav(NavId::Home) => {
                self.home_view.view().map(CoordinatorMessage::Home)
            }
            ActiveRoute::Nav(NavId::Explorer) => {
                let all_tracks = self.catalog_store.all_tracks();
                let all_refs: Vec<&Track> = all_tracks.iter().collect();
                let rendered_tracks = filter_tracks(&all_refs, &self.explorer_view.list.search_filter);

                self.explorer_view
                    .view(rendered_tracks, &self.thumbnails)
                    .map(CoordinatorMessage::Explorer)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked_tracks = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let liked_tracks = filter_tracks(&liked_tracks, &self.favorites_view.list.search_filter);

                self.favorites_view
                    .view(liked_tracks, &self.thumbnails)
                    .map(CoordinatorMessage::Favorites)
            }
            ActiveRoute::Playlist(id) => {
                if let Some(view) = &self.playlist_view {
                    if let Some(meta) = self.catalog_store
                        .playlists_metadata()
                        .iter()
                        .find(|(id_, _, _)| id_ == id)
                    {
                        let tracks_refs = self.catalog_store.tracks_for_playlist(id);
                        let tracks_refs = filter_tracks(&tracks_refs, &view.list.search_filter);

                        view.view(&meta.1, tracks_refs, &self.thumbnails)
                            .map(CoordinatorMessage::PlaylistDetail)
                    } else {
                        iced::widget::space().into()
                    }
                } else {
                    iced::widget::container(iced::widget::text("Cargando playlist..."))
                        .width(iced::Length::Fill)
                        .height(iced::Length::Fill)
                        .center_x(iced::Length::Fill)
                        .center_y(iced::Length::Fill)
                        .into()
                }
            }
            _ => iced::widget::space().into(),
        }
    }

    /// Overlay del menú contextual de TRACK — se expone aparte de
    /// `view_content()` porque sidebar_feature_v2 necesita apilarlo en el
    /// mismo `stack` que envuelve `view_content()` (mismo motivo de
    /// coordenadas relativas documentado arriba), no en su propio
    /// `view_overlays()` de nivel superior junto con el menú de fila de
    /// playlist.
    pub fn view_track_context_menu(&self) -> Option<Element<'_, CoordinatorMessage>> {
        let open_track_id = self.track_context_menu.open_id();
        let (anchor, track_id) = self.track_context_menu.render_target(|_| open_track_id)?;

        Some(self.track_context_menu.view(
            anchor,
            self.track_context_menu_items.clone(),
            track_id,
            |action, id| CoordinatorMessage::TrackContextAction(action, id),
            CoordinatorMessage::TrackContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| CoordinatorMessage::TrackContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }

    /// Metadata de playlists (id, nombre, cover) — sidebar_feature_v2 la
    /// necesita para pintar la sección de playlists y resolver el nombre
    /// al pedir confirmación de borrado.
    pub fn playlists_metadata(&self) -> &[(String, String, Option<String>)] {
        self.catalog_store.playlists_metadata()
    }

    pub fn create_playlist(&mut self, name: &str) {
        let _ = self.catalog_store.create_playlist(name);
    }

    /// Borra una playlist y, si era la que estaba activa, saca al
    /// coordinator de esa ruta — si no, view_content() seguiría buscando
    /// una playlist que ya no existe en playlists_metadata().
    pub fn delete_playlist(&mut self, playlist_id: &str) -> Task<CoordinatorMessage> {
        if self.active_route == ActiveRoute::Playlist(playlist_id.to_string()) {
            self.active_route = ActiveRoute::Nav(NavId::Home);
            self.playlist_view = None;
        }
        self.catalog_store
            .delete_playlist(playlist_id)
            .map(CoordinatorMessage::Catalog)
    }
}

/// Filtra `tracks` contra `raw_query` (el `search_filter` local de cada
/// vista) usando la utilidad de búsqueda difusa compartida. Matchea contra
/// título, artistas formateados y álbum. Query vacía → devuelve todo sin
/// tocar el orden (SearchQuery::is_empty ya hace early-return internamente,
/// pero evitamos incluso construir la query si no hace falta).
fn filter_tracks<'a>(tracks: &[&'a Track], raw_query: &str) -> Vec<&'a Track> {
    if raw_query.trim().is_empty() {
        return tracks.to_vec();
    }

    let query = SearchQuery::new(raw_query);
    tracks
        .iter()
        .copied()
        .filter(|t| {
            let album_name = t.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            query.matches_any(&[&t.title, &t.format_artists(), album_name])
        })
        .collect()
}

/// Igual que `filter_tracks` pero devuelve `Vec<Track>` clonado en vez de
/// referencias — lo usan los call-sites de `update()` para armar
/// `play_context`, que necesita ownership propio (se lo pasa a
/// `TrackManager::play_context`, que lo consume).
fn filter_tracks_owned(tracks: &[Track], raw_query: &str) -> Vec<Track> {
    let refs: Vec<&Track> = tracks.iter().collect();
    filter_tracks(&refs, raw_query).into_iter().cloned().collect()
}

/// Las vistas (Explorer/Favorites/Playlist) esperan `&[(String, String)]`
/// (id, nombre) para armar el submenú "Agregar a playlist". CatalogStore
/// devuelve metadata completa, que estas vistas no necesitan — se descarta acá
/// en vez de cambiar la firma de las 3 vistas por un dato que no usan.
fn playlist_pairs(metadata: &[(String, String, Option<String>)]) -> Vec<(String, String)> {
    metadata
        .iter()
        .map(|(id, name, _cover)| (id.clone(), name.clone()))
        .collect()
}