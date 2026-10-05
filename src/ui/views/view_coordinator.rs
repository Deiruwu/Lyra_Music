use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use iced::keyboard::Modifiers;
use iced::{Element, Size, Task};

use crate::audio::manager::manager::{PlaybackOrigin, TrackManager};
use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::db::artist_tag_manager::ArtistTagManager;
use crate::db::playlist_color_manager::PlaylistColorManager; // [playlist-color]
use crate::ui::playlist_color::{self, PlaylistColor}; // [playlist-color]
use crate::ui::views::artists_view::{ArtistsMessage, ArtistsOutMessage, ArtistsView};
use crate::ui::views::remix_view::{self, RemixMessage, RemixView};
use crate::microservices::client::MicroserviceClient;
use crate::model::{Mix, SearchItem, Track};
use crate::ui::views::playlist_adder::AdderMessage;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::cover_manager::CoverManager;
use crate::ui::utils::image::load_crop_preview;
use crate::ui::views::catalog_store::{CatalogStore, CatalogStoreMessage};
use crate::ui::views::home_view::{HomeView, HomeViewMessage, HomeViewOutMessage};
use crate::ui::views::explorer_view_v2::{ExplorerView, ExplorerMessage, ExplorerExtra};
use crate::ui::views::favorite_view::{FavoritesView, FavoritesMessage};
use crate::ui::views::playlist_view::{PlaylistView, PlaylistMessage, PlaylistExtra};
use crate::ui::views::states_view::{TrackViewState, ROW_HEIGHT};
use crate::ui::views::view_data::NavId;
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::cover_crop_editor::{CoverCropEditor, CropEditorMessage, CropEditorOutcome};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::track_context_builder::{youtube_link, TrackContextAction};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::selection_state::SelectionStep;

/// Resultados de YouTube en el panel de agregar canciones a una playlist.
const ADDER_SEARCH_LIMIT: usize = 10;

/// Lado máximo de la previsualización que se muestra en el editor de recorte.
const CROP_PREVIEW_MAX_SIDE: u32 = 840;

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
    Artists(ArtistsMessage),
    Remix(RemixMessage),
    /// [playlist-color] Colores guardados `(id, tono, saturación, brillo)`, cargados al arrancar.
    PlaylistColorsLoaded(Result<Vec<(String, f64, Option<f64>, Option<f64>)>, String>),
    /// [playlist-color] Resultado de guardar un color.
    PlaylistColorSaved(Result<(), String>),
    /// Terminó la descarga de una canción pedida desde el panel de agregar de una playlist.
    AdderDownloadFinished { playlist_id: String, track_id: String, result: Result<Track, String> },

    // ─── Menú contextual de track (Explorer/Favorites/Playlist) ────
    TrackContextMenuEvent(ContextMenuEvent<String>),
    TrackContextAction(TrackContextAction, String),

    // ─── Borrado del catálogo (con confirmación) ────────────────
    /// Pide confirmación antes de borrar estos ids (audio + letra + fila en el server).
    RequestDeleteTracks(Vec<String>),
    ConfirmDeleteTracks,
    CancelDeleteTracks,

    // ─── Selección múltiple (shift/ctrl) ────────────────────────
    KeybindsChanged(Modifiers),

    // ─── Miniaturas ──────────────────────────────────────────────
    ThumbnailLoaded(String, Vec<u8>),
    WindowResized(Size),

    // ─── Portadas de playlists ──────────────────────────────────
    /// Resultado del file-picker de portada: `(playlist_id, Option<path>)`.
    /// `None` si el usuario canceló el diálogo.
    CoverPicked { playlist_id: String, path: Option<PathBuf> },
    /// Previsualización de la imagen elegida, lista para abrir el editor de recorte.
    CoverPreviewLoaded { playlist_id: String, path: PathBuf, result: Result<(Vec<u8>, u32, u32), String> },
    CoverCrop(CropEditorMessage),
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
    RequestOpenMix(Mix),
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
    pub artists_view: ArtistsView,
    pub remix_view: RemixView,

    /// `TrackViewState` (selección/scroll/filtro/sort) de playlists que no
    /// están activas ahora mismo, guardado por `playlist_id` — para que
    /// revisitar una playlist ya vista no la resetee (`playlist_view` en
    /// sí solo puede tener una playlist "viva" a la vez).
    playlist_view_cache: HashMap<String, TrackViewState>,

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

    /// Confirmación de borrado de tracks, compartida por Explorer y la cola.
    delete_tracks_dialog: ConfirmDialog<Vec<String>>,

    /// Editor de recorte abierto tras elegir una imagen de portada.
    cover_crop: Option<CoverCropEditor>,

    /// [playlist-color] Persistencia de los tonos de playlist.
    playlist_colors: Arc<PlaylistColorManager>,

    /// Búsqueda y descarga para el panel de agregar canciones a una playlist.
    client: Arc<MicroserviceClient>,
}

impl ViewCoordinator {
    pub fn new(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        manager: Arc<TrackManager>,
        play_history_manager: Arc<PlayHistoryManager>,
        followed_artist_manager: Arc<FollowedArtistManager>,
        artist_tag_manager: Arc<ArtistTagManager>,
        playlist_colors: Arc<PlaylistColorManager>,
    ) -> (Self, Task<CoordinatorMessage>) {
        let (catalog_store, catalog_task) = CatalogStore::load(
            Arc::clone(&client),
            playlist_manager,
            Arc::clone(&followed_artist_manager),
            Arc::clone(&play_history_manager),
        );
        let (home_view, home_task) = HomeView::new(Arc::clone(&client), play_history_manager);
        let (artists_view, artists_task) = ArtistsView::new(artist_tag_manager);

        let coordinator = Self {
            active_route: ActiveRoute::Nav(NavId::Home),
            catalog_store,
            manager,
            thumbnails: AsyncThumbnail::new(128),
            covers: CoverManager::new(),
            home_view,
            explorer_view: ExplorerView::new(),
            favorites_view: FavoritesView::new(),
            playlist_view: None,
            artists_view,
            remix_view: RemixView::new(Vec::new()),
            playlist_view_cache: HashMap::new(),
            track_context_menu: ContextMenu::new(),
            track_context_menu_items: Vec::new(),
            track_context_selected_ids: HashSet::new(),
            delete_tracks_dialog: ConfirmDialog::new(),
            cover_crop: None,
            playlist_colors: Arc::clone(&playlist_colors),
            client,
        };

        // [playlist-color]
        let colors_task = Task::perform(
            async move { playlist_colors.load_all().await.map_err(|e| e.to_string()) },
            CoordinatorMessage::PlaylistColorsLoaded,
        );

        let init_task = Task::batch([
            catalog_task.map(CoordinatorMessage::Catalog),
            home_task.map(CoordinatorMessage::Home),
            artists_task.map(CoordinatorMessage::Artists),
            colors_task,
        ]);

        (coordinator, init_task)
    }

    /// Actualiza solo la posición de cursor que el menú contextual usa para
    /// anclarse. Deliberadamente NO pasa por `update()`: el cursor se mueve
    /// cientos de veces por segundo y `update()` re-sincroniza thumbnails y
    /// portadas de toda la vista activa.
    pub fn set_cursor(&mut self, position: iced::Point) {
        self.track_context_menu.handle(ContextMenuEvent::MouseMoved(position));
        self.artists_view.set_cursor(position);
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

        let scroll_task = self.resolve_pending_scroll_recenter();

        let home_task = if self.active_route == ActiveRoute::Nav(NavId::Home) {
            self.home_view.sync().map(CoordinatorMessage::Home)
        } else {
            Task::none()
        };

        let mixes_task = self.request_mixes_if_ready().map(CoordinatorMessage::Home);

        if self.active_route == ActiveRoute::Nav(NavId::Remix) {
            self.remix_view.ensure_mixed(&self.catalog_store);
        }

        let artists_task = if self.active_route == ActiveRoute::Nav(NavId::Artists) {
            self.artists_view.sync(self.catalog_store.followed_artists()).map(CoordinatorMessage::Artists)
        } else {
            Task::none()
        };

        (Task::batch([route_task, sync_task, cover_task, scroll_task, home_task, mixes_task, artists_task]), out)
    }

    /// Pide las mezclas de Home una sola vez, cuando ya cargaron el historial y los Me gusta.
    fn request_mixes_if_ready(&mut self) -> Task<HomeViewMessage> {
        if !self.home_view.needs_mixes() || !self.catalog_store.likes_loaded() {
            return Task::none();
        }

        let liked: Vec<Track> = self.catalog_store
            .tracks_for_playlist(self.catalog_store.system_playlist_id())
            .into_iter()
            .cloned()
            .collect();
        let known_ids: HashSet<String> = self.catalog_store
            .all_tracks()
            .iter()
            .filter(|t| t.liked || t.file_path.is_some())
            .map(|t| t.id.clone())
            .collect();

        self.home_view.load_mixes(liked, known_ids)
    }

    /// Si la vista activa tiene un recentrado de scroll pendiente (ver
    /// `TrackViewState::apply_search_filter`), lo resuelve acá: recalcula
    /// la lista ya filtrada+ordenada, ubica el track seleccionado y
    /// emite el `Task` que mueve el `scrollable` nativo hasta él. Corre
    /// siempre, sin importar qué rama de `update_route` disparó — mismo
    /// motivo que `sync_task`/`cover_task` arriba.
    fn resolve_pending_scroll_recenter(&mut self) -> Task<CoordinatorMessage> {
        match self.active_route.clone() {
            ActiveRoute::Nav(NavId::Explorer) => {
                let Some(target_id) = self.explorer_view.list.take_pending_scroll_target() else {
                    return Task::none();
                };
                let all_refs = self.catalog_store.explorer_tracks();
                let rendered = self.explorer_view.list.rendered(&all_refs, &self.catalog_store);
                scroll_to_selected(&mut self.explorer_view.list, &rendered, &target_id, "explorer_catalog_scroll")
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let Some(target_id) = self.favorites_view.list.take_pending_scroll_target() else {
                    return Task::none();
                };
                let liked = self.catalog_store.tracks_for_playlist(self.catalog_store.system_playlist_id());
                let rendered = self.favorites_view.list.rendered(&liked, &self.catalog_store);
                scroll_to_selected(&mut self.favorites_view.list, &rendered, &target_id, "favorites_catalog_scroll")
            }
            ActiveRoute::Playlist(id) => {
                let Some(view) = &mut self.playlist_view else {
                    return Task::none();
                };
                let Some(target_id) = view.list.take_pending_scroll_target() else {
                    return Task::none();
                };
                let all = self.catalog_store.tracks_for_playlist(&id);
                let rendered = view.list.rendered(&all, &self.catalog_store);
                scroll_to_selected(&mut view.list, &rendered, &target_id, "playlists_catalog_scroll")
            }
            _ => Task::none(),
        }
    }

    /// Guarda el `TrackViewState` de la playlist activa (si hay una) en
    /// `playlist_view_cache` antes de que `playlist_view` se reemplace o
    /// se descarte, para poder restaurarlo si se vuelve a visitar.
    fn stash_active_playlist_view(&mut self) {
        if let Some(view) = self.playlist_view.take() {
            self.playlist_view_cache.insert(view.playlist_id.clone(), view.list);
        }
    }

    fn update_route(&mut self, msg: CoordinatorMessage) -> (Task<CoordinatorMessage>, CoordinatorOutMessage) {
        match msg {
            CoordinatorMessage::SelectNav(nav_id) => {
                self.artists_view.clear_hover();
                self.active_route = ActiveRoute::Nav(nav_id);
                self.stash_active_playlist_view();
                // Las columnas de depuración leen el historial; se refresca al entrar.
                let task = match nav_id {
                    NavId::Explorer if self.explorer_view.show_play_stats => {
                        self.catalog_store.refresh_play_stats().map(CoordinatorMessage::Catalog)
                    }
                    NavId::Artists => self.artists_view.reload().map(CoordinatorMessage::Artists),
                    _ => Task::none(),
                };
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::SelectPlaylist(id) => {
                self.artists_view.clear_hover();
                self.stash_active_playlist_view();
                self.active_route = ActiveRoute::Playlist(id.clone());

                let restored = self.playlist_view_cache.remove(&id).unwrap_or_default();
                let restored_offset = restored.scroll.offset_y;
                self.playlist_view = Some(PlaylistView::with_state(id, restored));

                let task = if restored_offset > 0.0 {
                    scroll_to_offset("playlists_catalog_scroll", restored_offset)
                } else {
                    Task::none()
                };
                (task, CoordinatorOutMessage::Idle)
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
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.explorer_view.update(inner, &rendered_refs, &playlists, &self.catalog_store);
                let view_task = task.map(CoordinatorMessage::Explorer);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, |s: &Self| {
                    let all = s.catalog_store.explorer_tracks();
                    s.explorer_view.list.rendered(&all, &s.catalog_store).iter().map(|t| (*t).clone()).collect()
                }, PlaybackOrigin::Explorer, |store, extra| match extra {
                    ExplorerExtra::RefreshPlayStats => store.refresh_play_stats().map(CoordinatorMessage::Catalog),
                });

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            // ─── FAVORITES ───────────────────────────────────────────────
            CoordinatorMessage::Remix(inner) => {
                let source = self.remix_view.source(&self.catalog_store);
                let rendered_refs = self.remix_view.list.rendered(&source, &self.catalog_store);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.remix_view.update(inner, &rendered_refs, &playlists, &self.catalog_store);
                let view_task = task.map(CoordinatorMessage::Remix);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, |s: &Self| {
                    s.remix_rendered().into_iter().cloned().collect()
                }, PlaybackOrigin::Remix, |_store, extra| match extra {});

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            CoordinatorMessage::Favorites(inner) => {
                let liked = self.catalog_store.tracks_for_playlist(self.catalog_store.system_playlist_id());
                let rendered_refs = self.favorites_view.list.rendered(&liked, &self.catalog_store);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = self.favorites_view.update(inner, &rendered_refs, &playlists, &self.catalog_store);
                let view_task = task.map(CoordinatorMessage::Favorites);

                let (out_task, coordinator_out) = self.handle_track_list_out(out, |s: &Self| {
                    let liked = s.catalog_store.tracks_for_playlist(s.catalog_store.system_playlist_id());
                    s.favorites_view.list.rendered(&liked, &s.catalog_store).iter().map(|t| (*t).clone()).collect()
                }, PlaybackOrigin::Favorites, |_store, extra| match extra {});

                (Task::batch([view_task, out_task]), coordinator_out)
            }

            // ─── PLAYLIST DETAIL ─────────────────────────────────────────
            CoordinatorMessage::PlaylistDetail(inner) => {
                let Some(playlist_view) = &mut self.playlist_view else {
                    return (Task::none(), CoordinatorOutMessage::Idle);
                };
                let playlist_id = playlist_view.playlist_id.clone();

                let ctx_playlist_id = playlist_id.clone();

                let all_tracks = self.catalog_store.tracks_for_playlist(&playlist_id);
                let rendered_refs = playlist_view.list.rendered(&all_tracks, &self.catalog_store);
                let playlists = playlist_pairs(self.catalog_store.playlists_metadata());

                let (task, out) = playlist_view.update(inner, &rendered_refs, &playlists, &self.catalog_store);
                let view_task = task.map(CoordinatorMessage::PlaylistDetail);

                let playlist_colors = Arc::clone(&self.playlist_colors); // [playlist-color]
                let client = Arc::clone(&self.client);
                let (out_task, coordinator_out) = self.handle_track_list_out(out, |s: &Self| {
                    let Some(view) = &s.playlist_view else { return Vec::new() };
                    let all = s.catalog_store.tracks_for_playlist(&ctx_playlist_id);
                    view.list.rendered(&all, &s.catalog_store).iter().map(|t| (*t).clone()).collect()
                }, PlaybackOrigin::Playlist(playlist_id.clone()), |store, extra| match extra {
                    PlaylistExtra::RequestReorder { playlist_id, from, to } => {
                        store.reorder_track_in_playlist(&playlist_id, from, to)
                            .map(CoordinatorMessage::Catalog)
                    }
                    PlaylistExtra::RequestRename { playlist_id, new_name } => {
                        store.rename_playlist(&playlist_id, &new_name).map(CoordinatorMessage::Catalog)
                    }
                    PlaylistExtra::SearchSongs { query } => Task::perform(
                        async move {
                            client
                                .search_items(&query, Some(ADDER_SEARCH_LIMIT), "songs")
                                .await
                                .map(|items| items.into_iter().filter_map(|item| match item {
                                    SearchItem::Track(track) => Some(track),
                                    _ => None,
                                }).collect())
                                .map_err(|e| e.to_string())
                        },
                        |result| CoordinatorMessage::PlaylistDetail(PlaylistMessage::Adder(AdderMessage::RemoteLoaded(result))),
                    ),
                    PlaylistExtra::AddTrack { playlist_id, track_id } => {
                        store.add_track_to_playlist(&playlist_id, &track_id).map(CoordinatorMessage::Catalog)
                    }
                    PlaylistExtra::DownloadAndAdd { playlist_id, track } => {
                        let track_id = track.id.clone();
                        Task::perform(
                            async move { client.download(&track.id).await.map_err(|e| e.to_string()) },
                            move |result| CoordinatorMessage::AdderDownloadFinished {
                                playlist_id: playlist_id.clone(),
                                track_id: track_id.clone(),
                                result,
                            },
                        )
                    }
                    // [playlist-color]
                    PlaylistExtra::RequestColorChange { playlist_id, color } => {
                        playlist_color::set_color(&playlist_id, color);
                        Task::perform(
                            async move { playlist_colors.set_color(&playlist_id, color).await.map_err(|e| e.to_string()) },
                            CoordinatorMessage::PlaylistColorSaved,
                        )
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
                            self.manager.set_playback_origin(PlaybackOrigin::Home);
                            self.manager.play_context(tracks.to_vec(), index);
                        }
                        (task, CoordinatorOutMessage::Idle)
                    }
                    HomeViewOutMessage::OpenArtist(id) => (task, CoordinatorOutMessage::RequestOpenArtist(id)),
                    HomeViewOutMessage::OpenAlbum(id) => (task, CoordinatorOutMessage::RequestOpenAlbum(id)),
                    HomeViewOutMessage::OpenMix(id) => match self.home_view.mix(&id) {
                        Some(mix) => (task, CoordinatorOutMessage::RequestOpenMix(mix.clone())),
                        None => (task, CoordinatorOutMessage::Idle),
                    },
                }
            }

            CoordinatorMessage::Artists(inner) => {
                let (task, out) = self.artists_view.update(inner);
                let out = match out {
                    ArtistsOutMessage::Idle => CoordinatorOutMessage::Idle,
                    ArtistsOutMessage::OpenArtist(id) => CoordinatorOutMessage::RequestOpenArtist(id),
                };
                (task.map(CoordinatorMessage::Artists), out)
            }

            CoordinatorMessage::AdderDownloadFinished { playlist_id, track_id, result } => {
                if let Some(view) = &mut self.playlist_view {
                    view.adder_download_finished(&track_id);
                }
                let task = match result {
                    Ok(track) => {
                        let cached = self.catalog_store
                            .update(CatalogStoreMessage::TrackDownloadedAndCached(track))
                            .map(CoordinatorMessage::Catalog);
                        let added = self.catalog_store
                            .add_track_to_playlist(&playlist_id, &track_id)
                            .map(CoordinatorMessage::Catalog);
                        Task::batch([cached, added])
                    }
                    Err(e) => {
                        eprintln!("[PLAYLIST] No se pudo descargar {track_id} para agregarla: {e}");
                        Task::none()
                    }
                };
                (task, CoordinatorOutMessage::Idle)
            }

            // [playlist-color]
            CoordinatorMessage::PlaylistColorsLoaded(result) => {
                match result {
                    Ok(colors) => playlist_color::replace_all(colors.into_iter().map(|(id, hue, saturation, value)| {
                        let color = PlaylistColor {
                            hue: hue as f32,
                            saturation: saturation.map_or(playlist_color::DEFAULT_SATURATION, |s| s as f32),
                            value: value.map_or(playlist_color::DEFAULT_VALUE, |v| v as f32),
                        };
                        (id, color)
                    })),
                    Err(e) => eprintln!("[COLOR] No se pudieron cargar los colores de playlist: {e}"),
                }
                (Task::none(), CoordinatorOutMessage::Idle)
            }
            CoordinatorMessage::PlaylistColorSaved(result) => {
                if let Err(e) = result {
                    eprintln!("[COLOR] No se pudo guardar el color de la playlist: {e}");
                }
                (Task::none(), CoordinatorOutMessage::Idle)
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

            CoordinatorMessage::RequestDeleteTracks(ids) => {
                self.request_delete_tracks(ids);
                (Task::none(), CoordinatorOutMessage::Idle)
            }
            CoordinatorMessage::ConfirmDeleteTracks => {
                if let Some(ids) = self.delete_tracks_dialog.take_confirmed() {
                    for id in ids {
                        self.catalog_store.delete_track(&id);
                    }
                }
                (Task::none(), CoordinatorOutMessage::Idle)
            }
            CoordinatorMessage::CancelDeleteTracks => {
                self.delete_tracks_dialog.cancel();
                (Task::none(), CoordinatorOutMessage::Idle)
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
                let Some(path) = path else {
                    return (Task::none(), CoordinatorOutMessage::Idle);
                };
                let task = Task::perform(
                    {
                        let path = path.clone();
                        async move {
                            tokio::task::spawn_blocking(move || load_crop_preview(&path, CROP_PREVIEW_MAX_SIDE))
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()))
                        }
                    },
                    move |result| CoordinatorMessage::CoverPreviewLoaded { playlist_id: playlist_id.clone(), path: path.clone(), result },
                );
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::CoverPreviewLoaded { playlist_id, path, result } => {
                match result {
                    Ok((preview, width, height)) => {
                        self.cover_crop = Some(CoverCropEditor::new(playlist_id, path, preview, width, height));
                    }
                    Err(e) => eprintln!("No se pudo abrir la imagen de portada {}: {e}", path.display()),
                }
                (Task::none(), CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::CoverCrop(message) => {
                let Some(editor) = &mut self.cover_crop else {
                    return (Task::none(), CoordinatorOutMessage::Idle);
                };
                let task = match editor.update(message) {
                    CropEditorOutcome::Editing => Task::none(),
                    CropEditorOutcome::Cancel => {
                        self.cover_crop = None;
                        Task::none()
                    }
                    CropEditorOutcome::Save { playlist_id, source_path, region } => {
                        self.cover_crop = None;
                        match self.covers.import_cover(&playlist_id, &source_path, region) {
                            // Persiste en DB y actualiza playlists_metadata para que sidebar/header lo reflejen ya.
                            Ok(cover_path) => self.catalog_store
                                .update_playlist_cover(&playlist_id, &cover_path.to_string_lossy())
                                .map(CoordinatorMessage::Catalog),
                            Err(e) => {
                                eprintln!("No se pudo importar la portada de {playlist_id}: {e}");
                                Task::none()
                            }
                        }
                    }
                };
                (task, CoordinatorOutMessage::Idle)
            }

            CoordinatorMessage::WindowResized(size) => {
                self.track_context_menu.handle(ContextMenuEvent::ViewportResized(size));
                self.artists_view.set_viewport(size);
                (Task::none(), CoordinatorOutMessage::Idle)
            }
        }
    }

    /// Punto único donde se resuelve TrackListOutMessage<Extra> para
    /// cualquiera de las 3 vistas. Las variantes comunes (reproducir,
    /// navegar, menú contextual) se manejan acá una sola vez; lo específico de
    /// cada vista se delega al callback `on_extra`, que recibe &mut
    /// CatalogStore porque varias mutaciones de dominio son async
    /// (devuelven Task<CatalogStoreMessage>) y hay que enrutarlas de
    /// vuelta, no tirarlas al piso.
    fn handle_track_list_out<Extra>(
        &mut self,
        out: TrackListOutMessage<Extra>,
        play_context: impl FnOnce(&Self) -> Vec<Track>,
        origin: PlaybackOrigin,
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
                let context = play_context(self);
                if let Some(start_index) = context.iter().position(|t| t.id == start_track_id) {
                    self.manager.set_playback_origin(origin);
                    self.manager.play_context(context, start_index);
                }
                Task::none()
            }
            TrackListOutMessage::RequestPlayAll => {
                let context = play_context(self);
                self.manager.set_playback_origin(origin);
                self.manager.play_context_shuffled(context);
                Task::none()
            }
            TrackListOutMessage::RequestTogglePlayback => {
                if self.manager.state.is_playing() { self.manager.pause(); } else { self.manager.resume(); }
                Task::none()
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
        let selection = self.ordered_selection(&track_id);
        let selected_ids: Vec<String> = selection.iter().map(|t| t.id.clone()).collect();

        match action {
            TrackContextAction::PlayNow => {
                if !selection.is_empty() {
                    let start_index = selection.iter().position(|t| t.id == track_id).unwrap_or(0);
                    self.manager.play_context(selection, start_index);
                }
                Task::none()
            }
            TrackContextAction::Enqueue => {
                self.manager.enqueue_many(selection);
                Task::none()
            }
            TrackContextAction::FrontEnqueue => {
                self.manager.enqueue_front_many(selection);
                Task::none()
            }
            TrackContextAction::StartRadio => {
                if let Some(track) = self.catalog_store.track_by_id(&track_id) {
                    self.manager.start_radio(track.clone());
                }
                Task::none()
            }
            TrackContextAction::ToggleLike => {
                let target_liked = self.catalog_store.is_liked(&track_id);
                let tasks: Vec<_> = selection
                    .iter()
                    .filter(|t| t.liked == target_liked)
                    .map(|t| self.catalog_store.toggle_like(&t.id).map(CoordinatorMessage::Catalog))
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
            TrackContextAction::CopyYoutubeLink => {
                iced::clipboard::write(youtube_link(&track_id))
            }
            TrackContextAction::Tool(tool) => {
                let tasks: Vec<_> = selected_ids
                    .iter()
                    .map(|id| self.catalog_store.run_track_tool(tool, id).map(CoordinatorMessage::Catalog))
                    .collect();
                Task::batch(tasks)
            }
            TrackContextAction::DeleteFromCatalog => {
                self.request_delete_tracks(selected_ids);
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

    /// Tracks a los que aplica una acción del menú: la selección múltiple en
    /// el orden visible de la lista, o solo el ancla si no había selección.
    fn ordered_selection(&self, anchor_id: &str) -> Vec<Track> {
        let selected: Vec<Track> = if self.track_context_selected_ids.is_empty() {
            Vec::new()
        } else {
            self.active_route_rendered_tracks()
                .into_iter()
                .filter(|t| self.track_context_selected_ids.contains(&t.id))
                .cloned()
                .collect()
        };

        if !selected.is_empty() {
            return selected;
        }
        self.catalog_store.track_by_id(anchor_id).cloned().into_iter().collect()
    }

    /// Abre el diálogo de confirmación para borrar `ids` del catálogo.
    fn request_delete_tracks(&mut self, ids: Vec<String>) {
        let prompt = match ids.as_slice() {
            [] => return,
            [id] => match self.catalog_store.track_by_id(id) {
                Some(track) => format!("¿Eliminar \"{}\" del catálogo?", track.title),
                None => "¿Eliminar esta canción del catálogo?".to_string(),
            },
            _ => format!("¿Eliminar {} canciones del catálogo?", ids.len()),
        };
        self.delete_tracks_dialog.request(ids, prompt);
    }

    /// Menú contextual y diálogo de la vista de Artistas (a nivel ventana).
    pub fn view_artists_overlays(&self) -> Vec<Element<'_, CoordinatorMessage>> {
        if self.active_route != ActiveRoute::Nav(NavId::Artists) {
            return Vec::new();
        }
        self.artists_view
            .view_overlays(&self.catalog_store)
            .into_iter()
            .map(|layer| layer.map(CoordinatorMessage::Artists))
            .collect()
    }

    /// Cierra el input/renombre/diálogo abierto en Artistas; `true` si había algo.
    pub fn cancel_artists_edit(&mut self) -> bool {
        self.artists_view.cancel_edit()
    }

    /// Editor de recorte de portada, si hay uno abierto.
    pub fn view_cover_crop(&self) -> Option<Element<'_, CoordinatorMessage>> {
        self.cover_crop.as_ref().map(|editor| editor.view().map(CoordinatorMessage::CoverCrop))
    }

    /// Cierra el editor de recorte sin guardar; `true` si había uno abierto.
    pub fn cancel_cover_crop(&mut self) -> bool {
        self.cover_crop.take().is_some()
    }

    /// Diálogo de confirmación de borrado, si hay uno pendiente.
    pub fn view_delete_dialog(&self) -> Option<Element<'_, CoordinatorMessage>> {
        self.delete_tracks_dialog.view(CoordinatorMessage::ConfirmDeleteTracks, CoordinatorMessage::CancelDeleteTracks)
    }

    /// Cierra el diálogo de borrado; `true` si había uno abierto.
    pub fn cancel_delete_dialog(&mut self) -> bool {
        let was_open = self.delete_tracks_dialog.is_open();
        self.delete_tracks_dialog.cancel();
        was_open
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
            ActiveRoute::Nav(NavId::Remix) => self.remix_rendered(),
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

    /// Canciones de Remix filtradas/ordenadas como se ven en su tabla.
    fn remix_rendered(&self) -> Vec<&Track> {
        let source = self.remix_view.source(&self.catalog_store);
        self.remix_view.list.rendered(&source, &self.catalog_store)
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

                let mut targets = self.explorer_view.list.visible_thumbnail_targets(&tracks);
                targets.extend(mosaic_targets(&self.explorer_mosaic_tracks()));
                targets
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let tracks = self.favorites_view.list.rendered(&liked, &self.catalog_store);

                let mut targets = self.favorites_view.list.visible_thumbnail_targets(&tracks);
                targets.extend(mosaic_targets(&self.favorites_mosaic_tracks()));
                targets
            }
            ActiveRoute::Nav(NavId::Remix) => {
                let tracks = self.remix_rendered();
                let mut targets = self.remix_view.list.visible_thumbnail_targets(&tracks);
                targets.extend(mosaic_targets(&mosaic_tracks(tracks)));
                targets
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
                let mut targets = view.list.visible_thumbnail_targets(&tracks);
                targets.extend(view.adder_thumbnail_targets(&self.catalog_store));
                targets
            }
            _ => Vec::new(),
        }
    }

    /// Carátulas del mosaico de Explorar: lo agregado más recientemente.
    fn explorer_mosaic_tracks(&self) -> Vec<&Track> {
        let mut tracks = self.catalog_store.explorer_tracks();
        tracks.sort_by_key(|t| std::cmp::Reverse(t.added_at));
        mosaic_tracks(tracks)
    }

    /// Carátulas del mosaico de Me gusta: lo último que marcaste.
    fn favorites_mosaic_tracks(&self) -> Vec<&Track> {
        let liked = self.catalog_store.tracks_for_playlist(self.catalog_store.system_playlist_id());
        mosaic_tracks(liked.into_iter().rev())
    }

    fn mosaic_handles(&self, tracks: &[&Track]) -> Vec<Option<iced::widget::image::Handle>> {
        tracks.iter().map(|t| self.thumbnails.get(&thumb_key(t)).cloned()).collect()
    }

    /// Universo `(key, local_path)` de portadas que la UI quiere tener vivas
    /// ahora mismo. Todas las playlists piden su `Small` (sidebar ~40px);
    /// la `Large` (512px, header/detalle) solo la pide la playlist ABIERTA,
    /// que es la única que la lee (`view_content` → `covers.get(Large)`).
    /// Antes se pedían las dos para todas: con 100 playlists eso era ~4.7MB
    /// de JPEG residente y una lectura de disco + decode + resize + re-encode
    /// por playlist, para pintar una sola.
    fn active_cover_targets(&self) -> Vec<(String, String)> {
        use crate::ui::utils::cover_manager::{covers_path, CoverVariant};

        let active_playlist = match &self.active_route {
            ActiveRoute::Playlist(id) => Some(id.as_str()),
            _ => None,
        };

        self.catalog_store
            .playlists_metadata()
            .iter()
            .flat_map(|(id, _name, cover_url)| {
                let small_path = covers_path(id, CoverVariant::Small).to_string_lossy().into_owned();
                let mut out = vec![(CoverVariant::Small.key(id), small_path)];

                if active_playlist == Some(id.as_str()) {
                    let large_path = cover_url.clone().unwrap_or_else(|| {
                        covers_path(id, CoverVariant::Large).to_string_lossy().into_owned()
                    });
                    out.push((CoverVariant::Large.key(id), large_path));
                }

                out
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
        let is_playing = self.manager.state.is_playing();

        let origin_matches_active_route = match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => self.manager.get_playback_origin() == Some(PlaybackOrigin::Explorer),
            ActiveRoute::Nav(NavId::Favorites) => self.manager.get_playback_origin() == Some(PlaybackOrigin::Favorites),
            ActiveRoute::Nav(NavId::Remix) => self.manager.get_playback_origin() == Some(PlaybackOrigin::Remix),
            ActiveRoute::Playlist(id) => self.manager.get_playback_origin() == Some(PlaybackOrigin::Playlist(id.clone())),
            _ => false,
        };
        let now_playing_id = if origin_matches_active_route {
            self.manager.get_current_track().map(|t| t.track.id.clone())
        } else {
            None
        };

        match &self.active_route {
            ActiveRoute::Nav(NavId::Home) => {
                self.home_view.view().map(CoordinatorMessage::Home)
            }
            ActiveRoute::Nav(NavId::Artists) => {
                self.artists_view.view(&self.catalog_store).map(CoordinatorMessage::Artists)
            }
            ActiveRoute::Nav(NavId::Explorer) => {
                let all_refs = self.catalog_store.explorer_tracks();
                let rendered_tracks = self.explorer_view.list.rendered(&all_refs, &self.catalog_store);

                let mosaic = self.mosaic_handles(&self.explorer_mosaic_tracks());
                self.explorer_view
                    .view(rendered_tracks, &self.thumbnails, mosaic, now_playing_id, is_playing)
                    .map(CoordinatorMessage::Explorer)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked_tracks = self.catalog_store
                    .tracks_for_playlist(self.catalog_store.system_playlist_id());
                let liked_tracks = self.favorites_view.list.rendered(&liked_tracks, &self.catalog_store);

                let mosaic = self.mosaic_handles(&self.favorites_mosaic_tracks());
                self.favorites_view
                    .view(liked_tracks, &self.thumbnails, mosaic, now_playing_id, is_playing)
                    .map(CoordinatorMessage::Favorites)
            }
            ActiveRoute::Nav(NavId::Remix) => {
                let tracks = self.remix_rendered();
                let mosaic = self.mosaic_handles(&mosaic_tracks(tracks.iter().copied()));
                self.remix_view
                    .view(tracks, &self.thumbnails, mosaic, now_playing_id, is_playing, &self.catalog_store)
                    .map(CoordinatorMessage::Remix)
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

                        view.view(&meta.1, cover_handle, tracks_refs, &self.thumbnails, now_playing_id, is_playing, &self.catalog_store)
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
            CoordinatorMessage::TrackContextAction,
            CoordinatorMessage::TrackContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| CoordinatorMessage::TrackContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }

    /// Mueve la selección de la lista activa (flechas, RePág/AvPág) y la mantiene a la vista.
    pub fn move_selection(&mut self, step: SelectionStep, extend: bool) -> Task<CoordinatorMessage> {
        match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => {
                let all = self.catalog_store.explorer_tracks();
                let rendered = self.explorer_view.list.rendered(&all, &self.catalog_store);
                move_and_reveal(&mut self.explorer_view.list, &rendered, step, extend, "explorer_catalog_scroll")
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                let liked = self.catalog_store.tracks_for_playlist(self.catalog_store.system_playlist_id());
                let rendered = self.favorites_view.list.rendered(&liked, &self.catalog_store);
                move_and_reveal(&mut self.favorites_view.list, &rendered, step, extend, "favorites_catalog_scroll")
            }
            ActiveRoute::Nav(NavId::Remix) => {
                let source = self.remix_view.source(&self.catalog_store);
                let rendered = self.remix_view.list.rendered(&source, &self.catalog_store);
                move_and_reveal(&mut self.remix_view.list, &rendered, step, extend, remix_view::SCROLL_ID)
            }
            ActiveRoute::Playlist(id) => {
                let Some(view) = &mut self.playlist_view else { return Task::none() };
                let all = self.catalog_store.tracks_for_playlist(id);
                let rendered = view.list.rendered(&all, &self.catalog_store);
                move_and_reveal(&mut view.list, &rendered, step, extend, "playlists_catalog_scroll")
            }
            _ => Task::none(),
        }
    }

    /// Cierra el input de renombre de la playlist abierta; `true` si había uno.
    pub fn cancel_rename(&mut self) -> bool {
        self.playlist_view.as_mut().is_some_and(|view| view.cancel_rename())
    }

    /// Reproduce la lista activa desde el track bajo el cursor de selección.
    pub fn play_selection(&mut self) {
        let (list, origin) = match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => (&self.explorer_view.list, PlaybackOrigin::Explorer),
            ActiveRoute::Nav(NavId::Favorites) => (&self.favorites_view.list, PlaybackOrigin::Favorites),
            ActiveRoute::Nav(NavId::Remix) => (&self.remix_view.list, PlaybackOrigin::Remix),
            ActiveRoute::Playlist(id) => match &self.playlist_view {
                Some(view) => (&view.list, PlaybackOrigin::Playlist(id.clone())),
                None => return,
            },
            _ => return,
        };

        let rendered = self.active_route_rendered_tracks();
        let Some(start_index) = list
            .cursor_track_id(&rendered)
            .and_then(|id| rendered.iter().position(|t| t.id == id))
        else {
            return;
        };

        let context: Vec<Track> = rendered.iter().map(|t| (*t).clone()).collect();
        self.manager.set_playback_origin(origin);
        self.manager.play_context(context, start_index);
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
        self.catalog_store.playlist_track_stats(playlist_id)
    }

    /// Borra una playlist y, si era la que estaba activa, saca al
    /// coordinator de esa ruta — si no, view_content() seguiría buscando
    /// una playlist que ya no existe en playlists_metadata().
    pub fn delete_playlist(&mut self, playlist_id: &str) -> Task<CoordinatorMessage> {
        if self.active_route == ActiveRoute::Playlist(playlist_id.to_string()) {
            self.active_route = ActiveRoute::Nav(NavId::Home);
            self.playlist_view = None;
        }
        self.playlist_view_cache.remove(playlist_id);
        self.catalog_store
            .delete_playlist(playlist_id)
            .map(CoordinatorMessage::Catalog)
    }

    /// Reafirma el offset de scroll YA guardado en `TrackViewState` de la
    /// ruta activa contra el `scrollable` nativo de iced. Necesario
    /// porque `App::view()` (`main.rs`) intercala `view_content()` con
    /// otras ramas (library browser, modo teatro) en un mismo `if/else`
    /// — iced identifica el estado interno de un widget por posición+tipo
    /// en el árbol al diffear, no por su `Id`, así que en cuanto
    /// `view_content()` deja de renderizarse un frame, su `scrollable`
    /// desaparece del árbol y vuelve con offset 0 al reaparecer, aunque
    /// `ScrollTracker.offset_y` en memoria siga teniendo el valor
    /// correcto. Llamar esto justo cuando `view_content()` vuelve a
    /// mostrarse arregla eso — no hay nada que recalcular, el offset ya
    /// es el correcto.
    pub fn resync_active_scroll(&self) -> Task<CoordinatorMessage> {
        match &self.active_route {
            ActiveRoute::Nav(NavId::Explorer) => {
                scroll_to_offset("explorer_catalog_scroll", self.explorer_view.list.scroll.offset_y)
            }
            ActiveRoute::Nav(NavId::Favorites) => {
                scroll_to_offset("favorites_catalog_scroll", self.favorites_view.list.scroll.offset_y)
            }
            ActiveRoute::Nav(NavId::Remix) => {
                scroll_to_offset(remix_view::SCROLL_ID, self.remix_view.list.scroll.offset_y)
            }
            ActiveRoute::Playlist(_) => match &self.playlist_view {
                Some(view) => scroll_to_offset("playlists_catalog_scroll", view.list.scroll.offset_y),
                None => Task::none(),
            },
            _ => Task::none(),
        }
    }
}

/// Emite el `Task` que mueve el `scrollable` nativo (por su `Id`) al
/// offset absoluto dado.
fn scroll_to_offset(scrollable_id: &'static str, offset_y: f32) -> Task<CoordinatorMessage> {
    iced::widget::operation::scroll_to(
        iced::widget::Id::new(scrollable_id),
        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset_y },
    )
}

/// Ubica `target_id` en `rendered` y mueve `list.scroll` (más el
/// `scrollable` nativo de iced, vía el `Task` devuelto) hasta que quede a
/// la vista. Si `target_id` ya no está en `rendered` (se filtró afuera o
/// se eliminó), cae al reset a 0 de siempre.
fn scroll_to_selected(
    list: &mut TrackViewState,
    rendered: &[&Track],
    target_id: &str,
    scrollable_id: &'static str,
) -> Task<CoordinatorMessage> {
    let Some(index) = rendered.iter().position(|t| t.id == target_id) else {
        list.scroll.reset();
        return Task::none();
    };

    let max_offset = (rendered.len() as f32 * ROW_HEIGHT - list.scroll.viewport_height).max(0.0);
    let target_offset = (index as f32 * ROW_HEIGHT).min(max_offset);
    list.scroll.offset_y = target_offset;

    scroll_to_offset(scrollable_id, target_offset)
}

/// Mueve la selección de `list` y scrollea para mantener el cursor a la
/// vista; con páginas, la lista se desplaza lo mismo que el cursor.
fn move_and_reveal(
    list: &mut TrackViewState,
    rendered: &[&Track],
    step: SelectionStep,
    extend: bool,
    scrollable_id: &'static str,
) -> Task<CoordinatorMessage> {
    let previous = list.tracks_selection.cursor_index.or(list.tracks_selection.anchor_index);
    let delta = step.rows(list.scroll.rows_per_page(ROW_HEIGHT));

    let Some(index) = list.move_selection(delta, extend, rendered) else {
        return Task::none();
    };
    let row_top = index as f32 * ROW_HEIGHT;

    if step.is_page() {
        let previous = previous.map_or(index, |p| p.min(rendered.len() - 1));
        let moved_rows = index as f32 - previous as f32;
        let content_height = rendered.len() as f32 * ROW_HEIGHT;
        list.scroll.page_and_reveal(moved_rows * ROW_HEIGHT, content_height, row_top, ROW_HEIGHT, scrollable_id)
    } else {
        list.scroll.reveal(row_top, ROW_HEIGHT, scrollable_id)
    }
}

/// Hasta cuatro tracks con carátulas distintas, en el orden dado.
fn mosaic_tracks<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> Vec<&'a Track> {
    let mut seen = HashSet::new();
    tracks
        .into_iter()
        .filter(|t| t.thumbnail_small.is_some() && seen.insert(thumb_key(t)))
        .take(4)
        .collect()
}

fn mosaic_targets(tracks: &[&Track]) -> Vec<(String, String)> {
    tracks
        .iter()
        .filter_map(|t| t.thumbnail_small.clone().map(|url| (thumb_key(t), url)))
        .collect()
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