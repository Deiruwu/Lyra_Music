use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use iced::keyboard::Modifiers;
use iced::{Element, Size, Task};

use crate::audio::manager::manager::TrackManager;
use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::cover_manager::CoverManager;
use crate::ui::views::catalog_store::{CatalogStore, CatalogStoreMessage};
use crate::ui::views::home_view::{HomeView, HomeViewMessage, HomeViewOutMessage};
use crate::ui::views::explorer_view_v2::{ExplorerView, ExplorerMessage, ExplorerExtra};
use crate::ui::views::favorite_view::{FavoritesView, FavoritesMessage};
use crate::ui::views::playlist_view::{PlaylistView, PlaylistMessage, PlaylistExtra};
use crate::ui::views::view_data::NavId;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
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
    CreatePlaylist(String),

    Catalog(CatalogStoreMessage),
    Home(HomeViewMessage),
    Explorer(ExplorerMessage),
    Favorites(FavoritesMessage),
    PlaylistDetail(PlaylistMessage),

    // ─── Menú contextual de track (Explorer/Favorites/Playlist) ────
    TrackContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),

    // ─── Selección múltiple (shift/ctrl) ────────────────────────
    KeybindsChanged(Modifiers),

    // ─── Miniaturas ──────────────────────────────────────────────
    ThumbnailLoaded(String, Vec<u8>),
    WindowResized(Size),

    // ─── Portadas de playlists ──────────────────────────────────
    /// Resultado del file-picker de portada: `(playlist_id, Option<path>)`.
    /// `None` si el usuario canceló el diálogo.
    CoverPicked { playlist_id: String, path: Option<PathBuf> },
    /// Portada local leída/decodificada por `CoverManager`.
    CoverLoaded(String, Vec<u8>),
}

/// Salida de `ViewCoordinator::update()` hacia quien lo contenga
/// (`SidebarFeatureV2`, y de ahí hasta `App`), para pedidos que el
/// coordinador no puede resolver solo — hoy, navegar a un artista, que vive
/// en `LibraryBrowserFeature`, un sibling de `SidebarFeature` dentro de `App`.
#[derive(Debug, Clone)]
pub enum CoordinatorOutMessage {
    Idle,
    RequestOpenArtist(String),
    RequestOpenAlbum(String),
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

    /// Portadas de playlists (archivos locales, keyed por playlist_id).
    pub covers: CoverManager,

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
    /// Ids seleccionados al momento del click derecho que abrió
    /// `track_context_menu` — permite que una acción del menú (like,
    /// enqueue, agregar a playlist, eliminar) se aplique a toda la
    /// selección múltiple, no solo al track anclado.
    track_context_selected_ids: HashSet<String>,
}

impl ViewCoordinator {
    pub fn new(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        manager: Arc<TrackManager>,
        play_history_manager: Arc<PlayHistoryManager>,
        followed_artist_manager: Arc<FollowedArtistManager>,
    ) -> (Self, Task<CoordinatorMessage>) {
        let (catalog_store, catalog_task) = CatalogStore::load(
            Arc::clone(&client),
            playlist_manager,
            Arc::clone(&followed_artist_manager),
        );
        let (home_view, home_task) = HomeView::new(client, play_history_manager);

        let coordinator = Self {
            active_route: ActiveRoute::Nav(NavId::Home),
            catalog_store,
            manager,
            thumbnails: AsyncThumbnail::new(),
            covers: CoverManager::new(),
            home_view,
            explorer_view: ExplorerView::new(),
            favorites_view: FavoritesView::new(),
            playlist_view: None,
            track_context_menu: ContextMenu::new(),
            track_context_menu_items: Vec::new(),
            track_context_selected_ids: HashSet::new(),
        };

        let init_task = Task::batch([
            catalog_task.map(CoordinatorMessage::Catalog),
            home_task.map(CoordinatorMessage::Home),
        ]);

        (coordinator, init_task)
    }

    /// Único punto de entrada público. Delega el manejo del mensaje a
    /// `update_route`, y al final — sin importar qué rama corrió —
    /// sincroniza el caché de thumbnails contra la ventana visible de la
    /// vista activa ahora mismo. No depende de que cada rama se acuerde
    /// de pedir/podar thumbnails: corre siempre.
    pub fn update(&mut self, msg: CoordinatorMessage) -> (Task<CoordinatorMessage>, CoordinatorOutMessage) {
        let (route_task, out) = self.update_route(msg);

        let wanted = self.active_view_thumbnail_targets();
        let sync_task = self.thumbnails.sync(&wanted, CoordinatorMessage::ThumbnailLoaded);

        let wanted_covers = self.active_cover_targets();
        let cover_task = self.covers.sync(&wanted_covers, CoordinatorMessage::CoverLoaded);

        (Task::batch([route_task, sync_task, cover_task]), out)
    }

    fn update_route(&mut self, msg: CoordinatorMessage) -> (Task<CoordinatorMessage>, CoordinatorOutMessage) {
        match msg {
            CoordinatorMessage::SelectNav(nav_id) => {
                self.active_route = ActiveRoute::Nav(nav_id);
                self.playlist_view = None;
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::SelectPlaylist(id) => {
                self.active_route = ActiveRoute::Playlist(id.clone());
                self.playlist_view = Some(PlaylistView::new(id));
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::CreatePlaylist(name) => {
                let task = self.catalog_store
                    .create_playlist(&name)
                    .map(CoordinatorMessage::Catalog);
                (task, CoordinatorOutMessage::Idle)
            }

            // ─── EXPLORER ────────────────────────────────────────────────
            CoordinatorMessage::Explorer(inner) => {
                let all_refs = self.catalog_store.explorer_tracks();
                let rendered_refs = self.explorer_view.list.rendered(&all_refs, &self.catalog_store);
                let play_context: Vec<Track> = rendered_refs.iter().map(|t| (*t).clone()).collect();
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.explorer_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::Explorer);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, &play_context, |store, extra| match extra {
                    ExplorerExtra::RequestDelete(ids) => {
                        // TODO: delete_track es singular; se batchea id a id
                        // hasta que CatalogStore soporte borrado múltiple.
                        for id in ids {
                            store.delete_track(&id);
                        }
                        Task::none()
                    }
                });

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            // ─── FAVORITES ───────────────────────────────────────────────
            CoordinatorMessage::Favorites(inner) => {
                let liked = self.catalog_store.tracks_for_playlist(self.catalog_store.system_playlist_id());
                let rendered_refs = self.favorites_view.list.rendered(&liked, &self.catalog_store);
                let play_context: Vec<Track> = rendered_refs.iter().map(|t| (*t).clone()).collect();
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.favorites_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::Favorites);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, &play_context, |_store, extra| match extra {});

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            // ─── PLAYLIST DETAIL ─────────────────────────────────────────
            CoordinatorMessage::PlaylistDetail(inner) => {
                let Some(playlist_view) = &mut self.playlist_view else {
                    return (Task::none(), CoordinatorOutMessage::Idle);
                };
                let playlist_id = playlist_view.playlist_id.clone();

                let all_tracks = self.catalog_store.tracks_for_playlist(&playlist_id);
                let rendered_refs = playlist_view.list.rendered(&all_tracks, &self.catalog_store);
                let play_context: Vec<Track> = rendered_refs.iter().map(|t| (*t).clone()).collect();
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = playlist_view.update(inner, &rendered_refs, &playlists);
                let view_task = task.map(CoordinatorMessage::PlaylistDetail);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, &play_context, |store, extra| match extra {
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
                    PlaylistExtra::RequestCoverChange { playlist_id } => {
                        Task::perform(
                            crate::ui::utils::cover_picker::pick_cover_image(),
                            move |path| CoordinatorMessage::CoverPicked { playlist_id: playlist_id.clone(), path },
                        )
                    }
                });

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            CoordinatorMessage::Catalog(inner) => {
                let task = self.catalog_store.update(inner).map(CoordinatorMessage::Catalog);
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::Home(inner) => {
                let (task, out) = self.home_view.update(inner);
                let task = task.map(CoordinatorMessage::Home);

                match out {
                    HomeViewOutMessage::Idle => (task, CoordinatorOutMessage::Idle),
                    HomeViewOutMessage::PlayTrack(id) => {
                        let tracks = self.home_view.top_tracks();
                        if let Some(index) = tracks.iter().position(|t| t.id == id) {
                            self.manager.play_context(tracks.to_vec(), index);
                        }
                        (task, CoordinatorOutMessage::Idle)
                    }
                    HomeViewOutMessage::OpenArtist(id) => (task, CoordinatorOutMessage::RequestOpenArtist(id)),
                    HomeViewOutMessage::OpenAlbum(id) => (task, CoordinatorOutMessage::RequestOpenAlbum(id)),
                }
            }

            CoordinatorMessage::TrackContextMenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.track_context_menu_items.clear();
                    self.track_context_selected_ids.clear();
                }
                self.track_context_menu.handle(event);
                (Task::none(), CoordinatorOutMessage::Idle)
            }
            CoordinatorMessage::TrackContextAction(action, track_id) => {
                let task = self.handle_track_context_action(action, track_id);
                self.track_context_menu.handle(ContextMenuEvent::Dismissed);
                self.track_context_menu_items.clear();
                self.track_context_selected_ids.clear();
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::KeybindsChanged(modifiers) => {
                self.explorer_view.list.keybinds_press = modifiers;
                self.favorites_view.list.keybinds_press = modifiers;
                if let Some(view) = &mut self.playlist_view {
                    view.list.keybinds_press = modifiers;
                }
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::ThumbnailLoaded(key, bytes) => {
                self.thumbnails.on_loaded(key, bytes);
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::CoverLoaded(key, bytes) => {
                self.covers.on_loaded(key, bytes);
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::CoverPicked { playlist_id, path } => {
                let task = if let Some(path) = path {
                    match self.covers.import_cover(&playlist_id, &path) {
                        Ok(cover_path) => {
                            let cover_path_str = cover_path.to_string_lossy().into_owned();
                            // 1. Persistir en DB + actualizar playlists_metadata
                            //    en memoria para que el sidebar/header lo reflejen
                            //    ya mismo (asincrónico, vía CatalogStore).
                            self.catalog_store
                                .update_playlist_cover(&playlist_id, &cover_path_str)
                                .map(CoordinatorMessage::Catalog)
                        }
                        Err(e) => {
                            eprintln!("No se pudo importar la portada de {playlist_id}: {e}");
                            Task::none()
                        }
                    }
                } else {
                    Task::none()
                };
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::WindowResized(size) => {
                self.track_context_menu.handle(ContextMenuEvent::ViewportResized(size));
                (Task::none(), CoordinatorOutMessage::Idle)
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
    ) -> (Task<CoordinatorMessage>, CoordinatorOutMessage) {
        let coordinator_out = match &out {
            TrackListOutMessage::RequestOpenArtist(id) => CoordinatorOutMessage::RequestOpenArtist(id.clone()),
            TrackListOutMessage::RequestOpenAlbum(id) => CoordinatorOutMessage::RequestOpenAlbum(id.clone()),
            _ => CoordinatorOutMessage::Idle,
        };

        let task = match out {
            TrackListOutMessage::Idle => Task::none(),
            TrackListOutMessage::RequestOpenArtist(_) => Task::none(),
            TrackListOutMessage::RequestOpenAlbum(_) => Task::none(),

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

            TrackListOutMessage::ContextMenuRightClicked { track_id, items, selected_ids } => {
                self.track_context_menu.handle(ContextMenuEvent::RightClicked(track_id));
                self.track_context_menu_items = items;
                self.track_context_selected_ids = selected_ids;
                Task::none()
            }

            TrackListOutMessage::Extra(extra) => on_extra(&mut self.catalog_store, extra),
        };

        (task, coordinator_out)
    }

    /// Traduce una acción elegida en el menú contextual de track a la
    /// llamada real correspondiente. Cuando el click derecho se hizo sobre
    /// una selección múltiple, `track_context_selected_ids` trae todos los
    /// ids involucrados (no solo `track_id`, el track anclado) — cada
    /// acción decide si aplica a toda esa selección o solo al ancla.
    fn handle_track_context_action(&mut self, action: TrackContextAction, track_id: String) -> Task<CoordinatorMessage> {
        let selected_ids: HashSet<String> = if self.track_context_selected_ids.is_empty() {
            HashSet::from([track_id.clone()])
        } else {
            self.track_context_selected_ids.clone()
        };

        match action {
            TrackContextAction::PlayNow => {
                let rendered = self.active_route_rendered_tracks();
                let tracks: Vec<Track> = rendered
                    .iter()
                    .filter(|t| selected_ids.contains(&t.id))
                    .map(|t| (*t).clone())
                    .collect();
                if !tracks.is_empty() {
                    let start_index = tracks.iter().position(|t| t.id == track_id).unwrap_or(0);
                    self.manager.play_context(tracks, start_index);
                }
                Task::none()
            }
            TrackContextAction::Enqueue => {
                for id in &selected_ids {
                    if let Some(track) = self.catalog_store.track_by_id(id) {
                        self.manager.enqueue(track.clone());
                    }
                }
                Task::none()
            }
            TrackContextAction::FrontEnqueue => {
                for id in &selected_ids {
                    if let Some(track) = self.catalog_store.track_by_id(id) {
                        self.manager.enqueue_front(track.clone());
                    }
                }
                Task::none()
            }
            TrackContextAction::ToggleLike => {
                let target_liked = self.catalog_store.track_by_id(&track_id).map(|t| t.liked).unwrap_or(false);
                let ids_to_toggle: Vec<String> = selected_ids
                    .iter()
                    .filter(|id| self.catalog_store.track_by_id(id).map(|t| t.liked) != Some(target_liked))
                    .cloned()
                    .collect();
                let tasks: Vec<_> = ids_to_toggle
                    .into_iter()
                    .map(|id| self.catalog_store.toggle_like(&id).map(CoordinatorMessage::Catalog))
                    .collect();
                Task::batch(tasks)
            }
            TrackContextAction::AddToPlaylist(target_playlist_id) => {
                let tasks: Vec<_> = selected_ids
                    .iter()
                    .map(|id| {
                        self.catalog_store
                            .add_track_to_playlist(&target_playlist_id, id)
                            .map(CoordinatorMessage::Catalog)
                    })
                    .collect();
                Task::batch(tasks)
            }
            TrackContextAction::CopyId => {
                iced::clipboard::write(track_id)
            }
            TrackContextAction::DeleteFromCatalog => {
                for id in &selected_ids {
                    self.catalog_store.delete_track(id);
                }
                Task::none()
            }
            TrackContextAction::RemoveFromPlaylist => {
                let ActiveRoute::Playlist(playlist_id) = &self.active_route else {
                    return Task::none();
                };
                let playlist_id = playlist_id.clone();
                let tasks: Vec<_> = selected_ids
                    .iter()
                    .map(|id| {
                        self.catalog_store
                            .remove_track_from_playlist(&playlist_id, id)
                            .map(CoordinatorMessage::Catalog)
                    })
                    .collect();
                Task::batch(tasks)
            }
        }
    }

    /// Tracks ya filtrados+ordenados (todo el universo, no la ventana con
    /// buffer) de la vista de la ruta activa ahora mismo — mismo `match` de
    /// rutas que `active_view_thumbnail_targets`, pero sin recortar al
    /// viewport visible: una selección múltiple puede incluir ids que están
    /// scrolleados fuera de pantalla.
    fn active_route_rendered_tracks(&self) -> Vec<&Track> {
        match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => {
                let all_refs = self.catalog_store.explorer_tracks();
                self.explorer_view.list.rendered(&all_refs, &self.catalog_store)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                self.favorites_view.list.rendered(&liked, &self.catalog_store)
            }
            ActiveRoute::Playlist(id) => {
                let Some(view) = &self.playlist_view else {
                    return Vec::new();
                };
                let all = self.catalog_store.tracks_for_playlist(id);
                view.list.rendered(&all, &self.catalog_store)
            }
            _ => Vec::new(),
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
                let all_refs = self.catalog_store.explorer_tracks();
                let tracks = self.explorer_view.list.rendered(&all_refs, &self.catalog_store);

                self.explorer_view.list.visible_thumbnail_targets(&tracks)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let tracks = self.favorites_view.list.rendered(&liked, &self.catalog_store);

                self.favorites_view.list.visible_thumbnail_targets(&tracks)
            }
            ActiveRoute::Playlist(id) => {
                let Some(view) = &self.playlist_view else {
                    return Vec::new();
                };
                let all = self.catalog_store.tracks_for_playlist(id);
                let tracks = view.list.rendered(&all, &self.catalog_store);

                // Las portadas de playlists ya viven pre-codificadas en
                // `playlists_metadata` (son locales; no pasan por el caché de
                // thumbnails ni el semáforo), así que aquí solo entran las
                // miniaturas de track.
                view.list.visible_thumbnail_targets(&tracks)
            }
            _ => Vec::new(),
        }
    }

    /// Universo `(key, local_path)` de portadas que la UI quiere tener vivas
    /// ahora mismo. Se resuelve siempre contra `playlists_metadata()` (todos
    /// los covers del sidebar + el de la vista activa). Cada playlist pide
    /// DOS variantes con claves separadas: la `Large` (header/detalle) y la
    /// `Small` (sidebar ~40px), para que `CoverManager` las cachee por
    /// separado a su propia resolución. `CoverManager` se encarga de retener
    /// solo estas y cargar de disco las que falten.
    fn active_cover_targets(&self) -> Vec<(String, String)> {
        use crate::ui::utils::cover_manager::{covers_path, CoverVariant};
        self.catalog_store
            .playlists_metadata()
            .iter()
            .flat_map(|(id, _name, cover_url)| {
                let large_path = cover_url.clone().unwrap_or_else(|| {
                    covers_path(id, CoverVariant::Large).to_string_lossy().into_owned()
                });
                let small_path = covers_path(id, CoverVariant::Small).to_string_lossy().into_owned();
                vec![
                    (CoverVariant::Large.key(id), large_path),
                    (CoverVariant::Small.key(id), small_path),
                ]
            })
            .collect()
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
                let all_refs = self.catalog_store.explorer_tracks();
                let rendered_tracks = self.explorer_view.list.rendered(&all_refs, &self.catalog_store);

                self.explorer_view
                    .view(rendered_tracks, &self.thumbnails)
                    .map(CoordinatorMessage::Explorer)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked_tracks = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let liked_tracks = self.favorites_view.list.rendered(&liked_tracks, &self.catalog_store);

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
                        let tracks_refs = view.list.rendered(&tracks_refs, &self.catalog_store);

                        let cover_handle = self.covers.get(&crate::ui::utils::cover_manager::CoverVariant::Large.key(id)).cloned();

                        view.view(&meta.1, cover_handle, tracks_refs, &self.thumbnails)
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

    /// Handle de la portada de una playlist si ya terminó de cargar/decodificar.
    /// `None` mientras carga o si no hay memoria viva para esa clave.
    /// `variant` elige si pedimos la grande (header/detalle) o la pequeña
    /// (sidebar).
    pub fn cover_handle(
        &self,
        playlist_id: &str,
        variant: crate::ui::utils::cover_manager::CoverVariant,
    ) -> Option<iced::widget::image::Handle> {
        self.covers.get(&variant.key(playlist_id)).cloned()
    }

    /// Metadata de una playlist a partir de sus tracks: `(cantidad de
    /// canciones, duración total en segundos)`. Lo usa el sidebar para
    /// pintar la línea secundaria de cada fila.
    pub fn playlist_track_stats(&self, playlist_id: &str) -> (usize, i64) {
        let tracks = self.catalog_store.tracks_for_playlist(playlist_id);
        crate::ui::utils::playlist_metadata::track_stats(tracks)
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

/// Las vistas (Explorer/Favorites/Playlist) esperan `&[(String, String)]`
/// (id, nombre) para armar el submenú "Agregar a playlist". CatalogStore
/// devuelve metadata completa, que estas vistas no necesitan — se descarta acá
/// en vez de cambiar la firma de las 3 vistas por un dato que no usan.
pub(crate) fn playlist_pairs(metadata: &[(String, String, Option<String>)]) -> Vec<(String, String)> {
    metadata
        .iter()
        .map(|(id, name, _cover)| (id.clone(), name.clone()))
        .collect()
}