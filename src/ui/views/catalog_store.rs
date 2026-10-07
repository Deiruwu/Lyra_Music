//! # CatalogStore — fuente de la verdad del catálogo de tracks
//!
//! ## Qué es
//! `CatalogStore` es el único dueño de `Vec<Track>` completo (todo lo que
//! existe en `music_center`, resuelto vía `MicroserviceClient` en chunks) y
//! también el dueño de las relaciones locales de playlist/likes, resueltas
//! vía `PlaylistManager` (SQLite). El servicio de reproducción/descarga es
//! agnóstico a playlists — todo lo que es "pertenece a X playlist" o
//! "está likeado" vive aquí, no en el microservicio remoto.
//!
//! 1. `SidebarFeature` es dueño de UNA instancia de `CatalogStore` y la
//!    construye junto a los demás distritos en `SidebarFeature::new`.
//! 2. Los mensajes de carga (`CatalogStoreMessage::IdsLoaded`,
//!    `ChunkResolved`, `LikesLoaded`, `PlaylistOrderLoaded`) se rutean desde
//!    `SidebarFeature::update` hacia `catalog_store.update(msg)`, igual que
//!    cualquier otro distrito.
//! 3. Cualquier vista (Explorer, Favorites, Playlists) YA NO guarda su
//!    propia copia de tracks. En vez de eso, en su `view()`/`update()`
//!    recibe `&CatalogStore` como parámetro extra y pide su slice:
//!
//!    // Todo el catálogo (Explorer aplica su propio filtro/orden encima):
//!    let tracks: &[Track] = store.all_tracks();
//!
//!    // Solo los tracks de una playlist puntual (incluye la de Likes,
//!    // identificada internamente por `PlaylistManager::system_playlist_id`):
//!    let slice: Vec<&Track> = store.tracks_for_playlist(&playlist_id);
//!
//!    // Un track puntual por id (útil para refrescar selección/menú):
//!    if let Some(track) = store.track_by_id(&id) { ... }
//!
//! ## Sobre `liked`
//! No se mantiene un `HashSet` aparte: el estado de like vive directamente
//! en `Track::liked` dentro de `all_tracks`. La vista de Favoritos simplemente
//! filtra `all_tracks.iter().filter(|t| t.liked)`. Al hacer toggle, se muta
//! el campo en memoria de forma optimista y se persiste en SQLite en
//! background vía `PlaylistManager`.
//!
//! ## Sobre el orden de playlists
//! `playlist_order` guarda `playlist_id -> Vec<(track_id, position)>` para
//! las playlists CUSTOM (la SYSTEM/Likes no necesita orden, se resuelve
//! filtrando `liked`). Se carga de forma eager al arrancar junto con los
//! tracks, ordenado ascendente por `position`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::Task;

use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::{FollowedArtist, Track, TrackPlayCount};
use crate::ui::widgets::track_context_builder::TrackTool;

const CHUNK_SIZE: usize = 250;
const FOLLOWED_ARTISTS_LIMIT: i64 = 500;

#[derive(Debug, Clone)]
pub enum CatalogStoreMessage {
    IdsLoaded(Result<Vec<String>, String>),
    ChunkResolved(usize, Result<Vec<Track>, String>),
    TrackDeleted(String, Result<(), String>),

    /// Ids de tracks likeados (contenido de la playlist SYSTEM), llega una
    /// sola vez tras terminar de resolver todos los chunks de `all_tracks`.
    LikesLoaded(Result<Vec<String>, String>),

    /// `(playlist_id, [(track_id, position)])` para todas las playlists
    /// CUSTOM, cargado eager junto con los likes.
    PlaylistOrderLoaded(Result<Vec<(String, Vec<(String, f64)>)>, String>),

    /// `(id, name, cover_url)` de todas las playlists CUSTOM, cargado eager junto con
    /// `playlist_order`. Es la fuente para pintar la lista en el sidebar y covers.
    PlaylistsMetadataLoaded(Result<Vec<(String, String, Option<String>)>, String>),

    /// Resultado de persistir un toggle de like en SQLite. Si falla, se
    /// revierte la mutación optimista sobre `all_tracks`.
    LikeToggled(String, bool, Result<(), String>),

    /// Resultado de crear una playlist nueva. Si tuvo éxito, trae
    /// `(id, name, cover_url)` para insertarla directo en memoria sin round-trip.
    PlaylistCreated(Result<(String, String, Option<String>), String>),

    /// Resultado de eliminar una playlist. El id viaja siempre, exista o
    /// no error, para poder revertir/limpiar el estado correspondiente.
    PlaylistDeleted(String, Result<(), String>),

    /// Resultado de agregar un track a una playlist CUSTOM.
    /// `(playlist_id, track_id, result)`. Si falla, se revierte la
    /// inserción optimista en `playlist_order`.
    TrackAddedToPlaylist(String, String, Result<(), String>),

    /// Resultado de quitar un track de una playlist CUSTOM.
    TrackRemovedFromPlaylist(String, String, Result<(), String>),

    /// Resultado de reordenar tracks en una playlist CUSTOM.
    TrackReordered(String, Result<(), String>),

    /// Resultado de persistir un cambio de portada en SQLite.
    /// `(playlist_id, cover_url, result)`.
    PlaylistCoverChanged(String, Option<String>, Result<(), String>),

    /// Un nuevo track fue descargado exitosamente desde el buscador.
    /// Se inyecta en el catálogo en tiempo real para evitar recargar todo.
    TrackDownloadedAndCached(Track),

    /// Artistas seguidos, cargado eager junto con likes/playlists.
    FollowedArtistsLoaded(Result<Vec<FollowedArtist>, String>),

    /// Resultado de persistir un toggle de "seguir artista" en SQLite. Si
    /// falla, se revierte la mutación optimista.
    FollowToggled(String, bool, Result<(), String>),
    /// Respuesta del track_manager a una herramienta del menú contextual
    /// (metadatos / análisis / descarga): trae el track actualizado.
    TrackToolFinished(TrackTool, String, Result<Track, String>),
    /// Resultado de persistir el renombre de una playlist.
    PlaylistRenamed(String, String, Result<(), String>),
    /// Resultado de persistir el nuevo orden de playlists.
    PlaylistsReordered(Result<(), String>),
    /// Reproducciones y última vez de cada track según `play_history` (columnas de depuración).
    PlayStatsLoaded(Result<Vec<TrackPlayCount>, String>),
}

pub struct CatalogStore {
    client: Arc<MicroserviceClient>,
    playlist_manager: Arc<PlaylistManager>,
    followed_artist_manager: Arc<FollowedArtistManager>,
    play_history_manager: Arc<PlayHistoryManager>,

    all_tracks: Vec<Track>,
    index_by_id: HashMap<String, usize>,
    pending_chunks: HashMap<usize, Vec<Track>>,
    total_chunks: usize,

    playlist_order: HashMap<String, Vec<(String, f64)>>,
    liked_order: Vec<String>,
    playlists_metadata: Vec<(String, String, Option<String>)>,
    followed_artists: HashMap<String, FollowedArtist>,

    is_loading: bool,
    last_error: Option<String>,
    /// Si ya llegó (bien o mal) la carga de Me gusta desde SQLite.
    likes_loaded: bool,

    /// Se bumpea en cualquier mutación que pueda cambiar qué tracks (o en
    /// qué orden) debe ver una vista: altas/bajas del catálogo, likes,
    /// membresía/orden de playlists. Es la señal barata que usa
    /// `TrackViewState::rendered()` para saber si su cache de
    /// filtrado+orden sigue siendo válido, sin comparar tracks uno a uno.
    version: u64,

    /// Cache de `explorer_tracks()`: índices en `all_tracks`, no `Track`.
    /// Se recalcula solo cuando cambia `version`. Antes se re-filtraba todo
    /// el catálogo en cada llamada, y se la llama 4-5 veces por ciclo de
    /// update+view. `RefCell` porque los callers son `&self` (`view()` en
    /// iced no puede mutar) — todo corre single-threaded en el loop de iced.
    explorer_cache: RefCell<(u64, Vec<u32>)>,

    /// Cache de `(track_count, total_duration_seconds)` por playlist, para
    /// la línea secundaria de cada fila del sidebar. Se recalculaba en cada
    /// frame para TODAS las playlists (un lookup de HashMap por track
    /// miembro), y solo cambia cuando cambia `version`.
    playlist_stats_cache: RefCell<(u64, HashMap<String, (usize, i64)>)>,
}

impl CatalogStore {
    pub fn load(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
        followed_artist_manager: Arc<FollowedArtistManager>,
        play_history_manager: Arc<PlayHistoryManager>,
    ) -> (Self, Task<CatalogStoreMessage>) {
        let store = Self {
            client: Arc::clone(&client),
            playlist_manager,
            followed_artist_manager,
            play_history_manager,
            all_tracks: Vec::new(),
            index_by_id: HashMap::new(),
            pending_chunks: HashMap::new(),
            total_chunks: 0,
            playlist_order: HashMap::new(),
            liked_order: Vec::new(),
            playlists_metadata: Vec::new(),
            followed_artists: HashMap::new(),
            is_loading: true,
            last_error: None,
            likes_loaded: false,
            version: 0,
            explorer_cache: RefCell::new((u64::MAX, Vec::new())),
            playlist_stats_cache: RefCell::new((u64::MAX, HashMap::new())),
        };

        let load_ids_task = Task::perform(
            async move { client.get_all_ids().await.map_err(|e| e.to_string()) },
            CatalogStoreMessage::IdsLoaded,
        );

        (store, load_ids_task)
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn likes_loaded(&self) -> bool {
        self.likes_loaded
    }

    fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    // ── Métodos de acceso ────────────────────────────────

    pub fn all_tracks(&self) -> &[Track] {
        &self.all_tracks
    }

    /// Como `all_tracks()`, pero sin los tracks "stub" que el track_manager
    /// precarga en la DB al abrir un álbum (para tenerlos listos para una
    /// futura descarga): sin bpm, sin camelot_key y sin file_path todavía,
    /// no aportan nada al Explorer y solo lo ensucian.
    pub fn explorer_tracks(&self) -> Vec<&Track> {
        let mut cache = self.explorer_cache.borrow_mut();

        if cache.0 != self.version {
            cache.1.clear();
            cache.1.extend(
                self.all_tracks()
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.bpm.is_some() || t.camelot_key.is_some() || t.file_path.is_some())
                    .map(|(i, _)| i as u32),
            );
            cache.0 = self.version;
        }

        cache.1.iter().map(|&i| &self.all_tracks[i as usize]).collect()
    }

    /// Índice de un track en `all_tracks`. Lo usa `TrackViewState::rendered`
    /// para cachear su lista por índice en vez de por id: resolver la lista
    /// cacheada pasa de N hashes de UUID a N accesos directos al vector.
    pub fn index_of(&self, id: &str) -> Option<u32> {
        self.index_by_id.get(id).map(|&i| i as u32)
    }

    pub fn track_at(&self, index: u32) -> Option<&Track> {
        self.all_tracks.get(index as usize)
    }

    pub fn track_by_id(&self, id: &str) -> Option<&Track> {
        self.index_by_id.get(id).and_then(|&idx| self.all_tracks.get(idx))
    }

    /// `(id, name, cover_url)` de todas las playlists CUSTOM, listas para pintar en
    /// el sidebar y encabezados. No incluye la playlist SYSTEM (Likes).
    pub fn playlists_metadata(&self) -> &[(String, String, Option<String>)] {
        &self.playlists_metadata
    }

    pub fn system_playlist_id(&self) -> &str {
        self.playlist_manager.system_playlist_id()
    }

    /// Devuelve los tracks de una playlist, en orden, como slice lógico
    /// sobre `all_tracks` (sin duplicar los `Track`, solo referencias).
    ///
    /// Caso especial: si `playlist_id` es la playlist SYSTEM (Likes), no se
    /// usa `playlist_order` — se filtra directo por `Track::liked`, ya que
    /// esa playlist no tiene orden propio.
    pub fn tracks_for_playlist(&self, playlist_id: &str) -> Vec<&Track> {
        if playlist_id == self.playlist_manager.system_playlist_id() {
            return self.liked_order
                .iter()
                .filter_map(|id| self.track_by_id(id))
                .collect();
        }

        let Some(ids) = self.playlist_order.get(playlist_id) else {
            return Vec::new();
        };

        ids.iter()
            .filter_map(|(id, _pos)| self.track_by_id(id))
            .collect()
    }

    /// `(cantidad de canciones, duración total en segundos)` de una playlist,
    /// memoizado contra `version()`.
    pub fn playlist_track_stats(&self, playlist_id: &str) -> (usize, i64) {
        let mut cache = self.playlist_stats_cache.borrow_mut();

        if cache.0 != self.version {
            cache.1.clear();
            cache.0 = self.version;
        }

        if let Some(&stats) = cache.1.get(playlist_id) {
            return stats;
        }

        let stats = crate::ui::utils::playlist_metadata::track_stats(
            self.tracks_for_playlist(playlist_id),
        );
        cache.1.insert(playlist_id.to_string(), stats);
        stats
    }

    pub fn playlists_containing_track(&self, track_id: &str) -> HashSet<String> {
        let mut result: HashSet<String> = self.playlist_order
            .iter()
            .filter(|(_, tracks)| tracks.iter().any(|(id, _)| id == track_id))
            .map(|(playlist_id, _)| playlist_id.clone())
            .collect();

        if self.liked_order.iter().any(|id| id == track_id) {
            result.insert(self.playlist_manager.system_playlist_id().to_string());
        }

        result
    }

    pub fn delete_track(&mut self, track_id: &str) {
        self.all_tracks.retain(|t| t.id != track_id);
        for ids in self.playlist_order.values_mut() {
            ids.retain(|(id, _)| id != track_id);
        }
        self.liked_order.retain(|id| id != track_id);
        self.rebuild_index();

        let client = Arc::clone(&self.client);
        let playlist_manager = Arc::clone(&self.playlist_manager);
        let id_clone = track_id.to_string();

        tokio::spawn(async move {
            if let Err(e) = playlist_manager.remove_track_everywhere(&id_clone).await {
                eprintln!("[CatalogStore] ERROR: No se pudo sacar {} de las playlists: {}", id_clone, e);
            }
            match client.delete(&id_clone).await {
                Ok(_) => {
                    println!("[CatalogStore] Pista {} eliminada de la BD remota.", id_clone);
                }
                Err(e) => {
                    eprintln!("[CatalogStore] ERROR: Fallo al eliminar {} de la BD remota: {}", id_clone, e);

                }
            }
        });
    }

    /// Corre una herramienta del track_manager sobre un track; al terminar,
    /// `TrackToolFinished` reemplaza el track en el catálogo.
    pub fn run_track_tool(&self, tool: TrackTool, track_id: &str) -> Task<CatalogStoreMessage> {
        let client = Arc::clone(&self.client);
        let id = track_id.to_string();

        Task::perform(
            async move {
                let result = match tool {
                    TrackTool::RefreshMetadata => client.refresh_metadata(&id).await,
                    TrackTool::RefreshLyrics => client.refresh_lyrics(&id).await,
                    TrackTool::Reanalyze => client.reanalyze(&id).await,
                    TrackTool::Redownload => client.redownload(&id).await,
                };
                (id, result.map_err(|e| e.to_string()))
            },
            move |(id, result)| CatalogStoreMessage::TrackToolFinished(tool, id, result),
        )
    }

    /// Relee del historial local las reproducciones y la última vez de cada track.
    pub fn refresh_play_stats(&self) -> Task<CatalogStoreMessage> {
        let manager = Arc::clone(&self.play_history_manager);
        Task::perform(
            async move { manager.all_plays().await.map_err(|e| e.to_string()) },
            CatalogStoreMessage::PlayStatsLoaded,
        )
    }

    /// Alterna el like de un track: muta `Track::liked` de forma optimista
    /// y persiste el cambio en SQLite en background. Si la escritura falla,
    /// `LikeToggled` revierte la mutación.
    pub fn toggle_like(&mut self, track_id: &str) -> Task<CatalogStoreMessage> {
        let Some(&idx) = self.index_by_id.get(track_id) else {
            return Task::none();
        };

        let new_value = !self.all_tracks[idx].liked;
        self.all_tracks[idx].liked = new_value;

        if new_value {
            self.liked_order.push(track_id.to_string());
        } else {
            self.liked_order.retain(|id| id != track_id);
        }
        self.bump_version();

        let manager = Arc::clone(&self.playlist_manager);
        let id_clone = track_id.to_string();

        Task::perform(
            async move {
                let result = if new_value {
                    manager.like_track(&id_clone).await
                } else {
                    manager.dislike_track(&id_clone).await
                };
                (id_clone, new_value, result.map_err(|e| e.to_string()))
            },
            |(id, value, result)| CatalogStoreMessage::LikeToggled(id, value, result),
        )
    }

    /// Estado de like según el catálogo (las copias en cola/DTOs pueden estar desactualizadas).
    pub fn is_liked(&self, track_id: &str) -> bool {
        self.track_by_id(track_id).is_some_and(|t| t.liked)
    }

    /// Como `toggle_like`, pero agrega el track al catálogo si todavía no está.
    pub fn toggle_like_track(&mut self, track: Track) -> Task<CatalogStoreMessage> {
        let id = track.id.clone();
        if !self.index_by_id.contains_key(&id) {
            self.upsert_track(track);
        }
        self.toggle_like(&id)
    }

    /// Como `add_track_to_playlist`, pero agrega el track al catálogo si todavía no está.
    pub fn add_track_object_to_playlist(&mut self, playlist_id: &str, track: Track) -> Task<CatalogStoreMessage> {
        let id = track.id.clone();
        if !self.index_by_id.contains_key(&id) {
            self.upsert_track(track);
        }
        self.add_track_to_playlist(playlist_id, &id)
    }

    pub fn is_artist_followed(&self, artist_id: &str) -> bool {
        self.followed_artists.contains_key(artist_id)
    }

    pub fn followed_artist(&self, artist_id: &str) -> Option<&FollowedArtist> {
        self.followed_artists.get(artist_id)
    }

    pub fn followed_artists(&self) -> impl Iterator<Item = &FollowedArtist> {
        self.followed_artists.values()
    }

    /// Alterna "seguir" a un artista: muta el estado local de forma
    /// optimista y persiste en SQLite en background. Si la escritura
    /// falla, `FollowToggled` revierte la mutación.
    pub fn toggle_follow_artist(&mut self, artist_id: &str, name: &str, photo_url: Option<&str>) -> Task<CatalogStoreMessage> {
        let new_value = !self.is_artist_followed(artist_id);

        if new_value {
            self.followed_artists.insert(artist_id.to_string(), FollowedArtist {
                artist_id: artist_id.to_string(),
                name: name.to_string(),
                photo_url: photo_url.map(str::to_string),
                followed_at: chrono::Utc::now().naive_utc(),
            });
        } else {
            self.followed_artists.remove(artist_id);
        }

        let manager = Arc::clone(&self.followed_artist_manager);
        let id_clone = artist_id.to_string();
        let name_clone = name.to_string();
        let photo_clone = photo_url.map(str::to_string);

        Task::perform(
            async move {
                let result = if new_value {
                    manager.follow(&id_clone, &name_clone, photo_clone.as_deref()).await
                } else {
                    manager.unfollow(&id_clone).await
                };
                (id_clone, new_value, result.map_err(|e| e.to_string()))
            },
            |(id, value, result)| CatalogStoreMessage::FollowToggled(id, value, result),
        )
    }

    /// Crea una playlist nueva en SQLite y la agrega al final de
    /// `playlists_metadata` cuando el server confirme (no optimista, porque
    /// necesitamos el `id` real generado por `PlaylistManager`).
    pub fn create_playlist(&self, name: &str) -> Task<CatalogStoreMessage> {
        let manager = Arc::clone(&self.playlist_manager);
        let name_clone = name.to_string();

        Task::perform(
            async move {
                let result = manager.create_playlist(&name_clone).await;
                result
                    // Al nacer la playlist no tiene cover personalizado
                    .map(|id| (id, name_clone, None))
                    .map_err(|e| e.to_string())
            },
            CatalogStoreMessage::PlaylistCreated,
        )
    }

    /// Elimina una playlist: quita su entrada de `playlists_metadata` y
    /// `playlist_order` de forma optimista, y persiste en SQLite en
    /// background. No aplica a la playlist SYSTEM (Likes) — el caller debe
    /// evitar ofrecer esa opción en la UI para esa playlist.
    pub fn delete_playlist(&mut self, playlist_id: &str) -> Task<CatalogStoreMessage> {
        self.playlists_metadata.retain(|(id, _, _)| id != playlist_id);
        self.playlist_order.remove(playlist_id);
        self.bump_version();

        let manager = Arc::clone(&self.playlist_manager);
        let id_clone = playlist_id.to_string();

        Task::perform(
            async move {
                let result = manager.delete_playlist(&id_clone).await;
                (id_clone, result.map_err(|e| e.to_string()))
            },
            |(id, result)| CatalogStoreMessage::PlaylistDeleted(id, result),
        )
    }

    /// Renombra una playlist CUSTOM de forma optimista; si SQLite falla,
    /// `PlaylistRenamed` restaura el nombre anterior.
    pub fn rename_playlist(&mut self, playlist_id: &str, new_name: &str) -> Task<CatalogStoreMessage> {
        let new_name = new_name.trim();
        let Some(entry) = self.playlists_metadata.iter_mut().find(|(id, _, _)| id == playlist_id) else {
            return Task::none();
        };
        if new_name.is_empty() || entry.1 == new_name {
            return Task::none();
        }

        let previous_name = std::mem::replace(&mut entry.1, new_name.to_string());

        let manager = Arc::clone(&self.playlist_manager);
        let id_clone = playlist_id.to_string();
        let name_clone = new_name.to_string();

        Task::perform(
            async move {
                let result = manager.rename_playlist(&id_clone, &name_clone).await;
                (id_clone, previous_name, result.map_err(|e| e.to_string()))
            },
            |(id, previous, result)| CatalogStoreMessage::PlaylistRenamed(id, previous, result),
        )
    }

    /// Mueve la playlist `from` a la posición `to` (índices sobre
    /// `playlists_metadata`) y persiste el orden completo.
    pub fn move_playlist(&mut self, from: usize, to: usize) -> Task<CatalogStoreMessage> {
        let len = self.playlists_metadata.len();
        if from >= len || to >= len || from == to {
            return Task::none();
        }

        let moved = self.playlists_metadata.remove(from);
        self.playlists_metadata.insert(to, moved);

        let manager = Arc::clone(&self.playlist_manager);
        let ordered_ids: Vec<String> = self.playlists_metadata.iter().map(|(id, _, _)| id.clone()).collect();

        Task::perform(
            async move { manager.reorder_playlists(&ordered_ids).await.map_err(|e| e.to_string()) },
            CatalogStoreMessage::PlaylistsReordered,
        )
    }

    /// Actualiza la portada de una playlist CUSTOM: muta
    /// `playlists_metadata` de forma optimista (reemplazando el cover_url
    /// en memoria para que el sidebar/header lo reflejen ya mismo) y
    /// persiste en SQLite en background. `cover_url` es el camino local del
    /// archivo de portada recién importado.
    pub fn update_playlist_cover(&mut self, playlist_id: &str, cover_url: &str) -> Task<CatalogStoreMessage> {
        if let Some(entry) = self.playlists_metadata.iter_mut().find(|(id, _, _)| id == playlist_id) {
            entry.2 = Some(cover_url.to_string());
        }

        let manager = Arc::clone(&self.playlist_manager);
        let id_clone = playlist_id.to_string();
        let cover_clone = cover_url.to_string();

        Task::perform(
            async move {
                let result = manager.update_playlist_cover(&id_clone, Some(&cover_clone)).await;
                (id_clone, Some(cover_clone), result.map_err(|e| e.to_string()))
            },
            |(id, cover, result)| CatalogStoreMessage::PlaylistCoverChanged(id, cover, result),
        )
    }

    /// Indica si un track ya pertenece a una playlist CUSTOM puntual.
    /// No aplica a la playlist SYSTEM (Likes) — para eso se usa
    /// `Track::liked` directo. Útil para que el submenú "Agregar a
    /// playlist" pueda deshabilitar/marcar las playlists que ya lo
    /// contienen en vez de permitir duplicados silenciosos.
    pub fn is_track_in_playlist(&self, playlist_id: &str, track_id: &str) -> bool {
        self.playlist_order
            .get(playlist_id)
            .map(|ids| ids.iter().any(|(id, _)| id == track_id))
            .unwrap_or(false)
    }

    /// Agrega un track al final de una playlist CUSTOM. No aplica a la
    /// playlist SYSTEM (Likes) — usar `toggle_like` para esa.
    pub fn add_track_to_playlist(&mut self, playlist_id: &str, track_id: &str) -> Task<CatalogStoreMessage> {
        if self.is_track_in_playlist(playlist_id, track_id) {
            return Task::none();
        }

        let ids = self.playlist_order.entry(playlist_id.to_string()).or_default();
        let new_position = next_append_position(ids);
        ids.push((track_id.to_string(), new_position));
        self.bump_version();

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id_clone = playlist_id.to_string();
        let track_id_clone = track_id.to_string();

        Task::perform(
            async move {
                let result = manager
                    .add_track(&playlist_id_clone, &track_id_clone, new_position)
                    .await;
                (playlist_id_clone, track_id_clone, result.map_err(|e| e.to_string()))
            },
            |(playlist_id, track_id, result)| {
                CatalogStoreMessage::TrackAddedToPlaylist(playlist_id, track_id, result)
            },
        )
    }

    /// Quita un track de una playlist CUSTOM. No aplica a la playlist
    /// SYSTEM (Likes) — usar `toggle_like` para esa.
    pub fn remove_track_from_playlist(&mut self, playlist_id: &str, track_id: &str) -> Task<CatalogStoreMessage> {
        if let Some(ids) = self.playlist_order.get_mut(playlist_id) {
            ids.retain(|(id, _)| id != track_id);
        }
        self.bump_version();

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id_clone = playlist_id.to_string();
        let track_id_clone = track_id.to_string();

        Task::perform(
            async move {
                let result = manager
                    .remove_tracks(&playlist_id_clone, std::slice::from_ref(&track_id_clone))
                    .await;
                (playlist_id_clone, track_id_clone, result.map_err(|e| e.to_string()))
            },
            |(playlist_id, track_id, result)| {
                CatalogStoreMessage::TrackRemovedFromPlaylist(playlist_id, track_id, result)
            },
        )
    }

    /// Reordena un track dentro de una playlist CUSTOM (drag-and-drop).
    pub fn reorder_track_in_playlist(&mut self, playlist_id: &str, from_idx: usize, to_idx: usize) -> Task<CatalogStoreMessage> {
        let Some(ids) = self.playlist_order.get_mut(playlist_id) else {
            return Task::none();
        };

        if from_idx >= ids.len() || to_idx >= ids.len() || from_idx == to_idx {
            return Task::none();
        }

        let (track_id, _old_position) = ids.remove(from_idx);

        let prev = to_idx.checked_sub(1).and_then(|i| ids.get(i)).map(|(_, p)| *p);
        let next = ids.get(to_idx).map(|(_, p)| *p);

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id_clone = playlist_id.to_string();

        

        match compute_new_position(prev, next) {
            Some(new_position) => {
                ids.insert(to_idx, (track_id.clone(), new_position));
                self.bump_version();

                Task::perform(
                    async move {
                        let result = manager.update_position(&playlist_id_clone, &track_id, new_position).await;
                        (playlist_id_clone, result.map_err(|e| e.to_string()))
                    },
                    |(pid, res)| CatalogStoreMessage::TrackReordered(pid, res),
                )
            }
            None => {
                // Colisión de redondeo f64 entre prev y next: renumerar todo.
                ids.insert(to_idx, (track_id, 0.0));
                let renumbered: Vec<(String, f64)> = ids
                    .iter()
                    .enumerate()
                    .map(|(i, (id, _))| (id.clone(), i as f64))
                    .collect();
                *ids = renumbered.clone();
                self.bump_version();

                Task::perform(
                    async move {
                        let result = manager.renumber_playlist(&playlist_id_clone, &renumbered).await;
                        (playlist_id_clone, result.map_err(|e| e.to_string()))
                    },
                    |(pid, res)| CatalogStoreMessage::TrackReordered(pid, res),
                )
            }
        }
    }

    /// Mueve juntas las canciones `track_ids` (en su orden dentro de la playlist) para que el
    /// bloque empiece en `to_idx` de la lista sin ellas, y renumera las posiciones.
    pub fn move_tracks_in_playlist(&mut self, playlist_id: &str, track_ids: &[String], to_idx: usize) -> Task<CatalogStoreMessage> {
        let Some(ids) = self.playlist_order.get_mut(playlist_id) else {
            return Task::none();
        };

        let (block, mut rest): (Vec<_>, Vec<_>) = ids.iter().cloned().partition(|(id, _)| track_ids.contains(id));
        let at = to_idx.min(rest.len());
        rest.splice(at..at, block);
        if rest.iter().map(|(id, _)| id).eq(ids.iter().map(|(id, _)| id)) {
            return Task::none();
        }

        let renumbered: Vec<(String, f64)> = rest.into_iter().enumerate().map(|(i, (id, _))| (id, i as f64)).collect();
        *ids = renumbered.clone();
        self.bump_version();

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id = playlist_id.to_string();
        Task::perform(
            async move {
                let result = manager.renumber_playlist(&playlist_id, &renumbered).await;
                (playlist_id, result.map_err(|e| e.to_string()))
            },
            |(pid, res)| CatalogStoreMessage::TrackReordered(pid, res),
        )
    }

    // ── CARGA (chunking) ─────────────────────────────────────────────────────

    pub fn update(&mut self, msg: CatalogStoreMessage) -> Task<CatalogStoreMessage> {
        match msg {
            CatalogStoreMessage::IdsLoaded(result) => match result {
                Ok(ids) => {
                    let chunks: Vec<Vec<String>> = ids.chunks(CHUNK_SIZE).map(|c| c.to_vec()).collect();

                    self.total_chunks = chunks.len();
                    self.pending_chunks.clear();

                    if chunks.is_empty() {
                        self.is_loading = false;
                        return self.load_playlist_relations();
                    }

                    let tasks: Vec<Task<CatalogStoreMessage>> = chunks
                        .into_iter()
                        .enumerate()
                        .map(|(idx, chunk_ids)| {
                            let client = Arc::clone(&self.client);
                            Task::perform(
                                async move { client.resolve_many(&chunk_ids).await.map_err(|e| e.to_string()) },
                                move |res| CatalogStoreMessage::ChunkResolved(idx, res),
                            )
                        })
                        .collect();

                    Task::batch(tasks)
                }
                Err(e) => {
                    self.is_loading = false;
                    self.last_error = Some(e);
                    Task::none()
                }
            },

            CatalogStoreMessage::ChunkResolved(chunk_index, result) => {
                match result {
                    Ok(tracks) => {
                        self.pending_chunks.insert(chunk_index, tracks);
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }

                if self.pending_chunks.len() == self.total_chunks {
                    let mut joined: Vec<Track> = Vec::with_capacity(self.total_chunks * CHUNK_SIZE);
                    for i in 0..self.total_chunks {
                        if let Some(tracks) = self.pending_chunks.remove(&i) {
                            joined.extend(tracks);
                        }
                    }
                    self.all_tracks = joined;
                    self.rebuild_index();
                    self.is_loading = false;

                    // Todos los tracks están en memoria: ahora sí podemos
                    // hidratar likes y orden de playlists contra índices
                    // válidos.
                    return self.load_playlist_relations();
                }

                Task::none()
            }

            CatalogStoreMessage::TrackDeleted(deleted_id, result) => {
                match result {
                    Ok(_) => {
                        self.all_tracks.retain(|t| t.id != deleted_id);

                        self.rebuild_index();
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::LikesLoaded(result) => {
                self.likes_loaded = true;
                match result {
                    Ok(liked_ids) => {
                        self.liked_order = liked_ids.clone();

                        for track in self.all_tracks.iter_mut() {
                            track.liked = false;
                        }
                        for id in liked_ids {
                            if let Some(&idx) = self.index_by_id.get(&id) {
                                self.all_tracks[idx].liked = true;
                            }
                        }
                        self.bump_version();
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistOrderLoaded(result) => {
                match result {
                    Ok(pairs) => {
                        self.playlist_order = pairs.into_iter().collect();
                        self.bump_version();
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::LikeToggled(track_id, attempted_value, result) => {
                if let Err(e) = result {
                    if let Some(&idx) = self.index_by_id.get(&track_id) {
                        self.all_tracks[idx].liked = !attempted_value;
                    }

                    if attempted_value {
                        self.liked_order.retain(|id| id != &track_id);
                    } else {
                        self.liked_order.insert(0, track_id.clone());
                    }
                    self.bump_version();

                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistsMetadataLoaded(result) => {
                match result {
                    Ok(metadata) => {
                        self.playlists_metadata = metadata;
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistCreated(result) => {
                match result {
                    Ok((id, name, cover_url)) => {
                        self.playlists_metadata.push((id, name, cover_url));
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistDeleted(_playlist_id, result) => {
                // La mutación optimista ya se aplicó en `delete_playlist`.
                // Si falló, solo dejamos constancia del error; recargar el
                // estado exacto de SQLite ahí es un caso raro (fallo de
                // escritura) que no amerita un round-trip completo aquí.
                if let Err(e) = result {
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::TrackAddedToPlaylist(playlist_id, track_id, result) => {
                if let Err(e) = result {
                    // Revierte la inserción optimista.
                    if let Some(ids) = self.playlist_order.get_mut(&playlist_id) {
                        ids.retain(|(id, _)| id != &track_id);
                    }
                    self.bump_version();
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::TrackRemovedFromPlaylist(playlist_id, track_id, result) => {
                if let Err(e) = result {
                    // Revierte la eliminación optimista reinsertando al
                    // final. No es 100% fiel a la posición original,
                    // pero es un caso raro (fallo de escritura) y evita
                    // tener que guardar el índice previo solo para este
                    // camino de error.
                    let ids = self.playlist_order.entry(playlist_id).or_default();
                    let new_position = next_append_position(ids);
                    ids.push((track_id, new_position));
                    self.bump_version();
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::TrackReordered(_playlist_id, result) => {
                if let Err(e) = result {
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistCoverChanged(_playlist_id, _cover, result) => {
                if let Err(e) = result {
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::TrackDownloadedAndCached(track) => {
                self.upsert_track(track);
                Task::none()
            }

            CatalogStoreMessage::TrackToolFinished(tool, track_id, result) => {
                match result {
                    Ok(track) => {
                        self.upsert_track(track);
                        self.last_error = None;
                    }
                    Err(e) => {
                        eprintln!("[CatalogStore] No se pudo {} {}: {}", tool.label(), track_id, e);
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistRenamed(playlist_id, previous_name, result) => {
                if let Err(e) = result {
                    if let Some(entry) = self.playlists_metadata.iter_mut().find(|(id, _, _)| id == &playlist_id) {
                        entry.1 = previous_name;
                    }
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::PlaylistsReordered(result) => {
                if let Err(e) = result {
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }

            CatalogStoreMessage::PlayStatsLoaded(result) => {
                match result {
                    Ok(stats) => {
                        for track in self.all_tracks.iter_mut() {
                            track.play_count = None;
                            track.last_played_at = None;
                        }
                        for stat in stats {
                            if let Some(&idx) = self.index_by_id.get(&stat.track_id) {
                                self.all_tracks[idx].play_count = Some(stat.play_count);
                                self.all_tracks[idx].last_played_at = Some(stat.last_played_at);
                            }
                        }
                        self.bump_version();
                    }
                    Err(e) => self.last_error = Some(e),
                }
                Task::none()
            }

            CatalogStoreMessage::FollowedArtistsLoaded(result) => {
                match result {
                    Ok(artists) => {
                        self.followed_artists = artists.into_iter().map(|a| (a.artist_id.clone(), a)).collect();
                        self.last_error = None;
                    }
                    Err(e) => {
                        self.last_error = Some(e);
                    }
                }
                Task::none()
            }

            CatalogStoreMessage::FollowToggled(artist_id, attempted_value, result) => {
                if let Err(e) = result {
                    if attempted_value {
                        self.followed_artists.remove(&artist_id);
                    }
                    // Si attempted_value era `false` (un unfollow que
                    // falló), no reinsertamos: no tenemos a mano el
                    // name/photo_url originales para reconstruir la fila.
                    self.last_error = Some(e);
                } else {
                    self.last_error = None;
                }
                Task::none()
            }
        }
    }

    /// Dispara en paralelo la carga de likes y de orden de playlists custom.
    /// Se llama una vez que `all_tracks`/`index_by_id` ya están completos.
    fn load_playlist_relations(&self) -> Task<CatalogStoreMessage> {
        let manager_likes = Arc::clone(&self.playlist_manager);
        let system_id = self.playlist_manager.system_playlist_id().to_string();

        let likes_task = Task::perform(
            async move {
                manager_likes
                    .get_playlist_track_ids(&system_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            CatalogStoreMessage::LikesLoaded,
        );

        let manager_order = Arc::clone(&self.playlist_manager);
        let order_task = Task::perform(
            async move {
                manager_order
                    .get_all_playlist_track_positions()
                    .await
                    .map_err(|e| e.to_string())
            },
            CatalogStoreMessage::PlaylistOrderLoaded,
        );

        let manager_meta = Arc::clone(&self.playlist_manager);
        let system_id_for_meta = self.playlist_manager.system_playlist_id().to_string();
        let metadata_task = Task::perform(
            async move {
                let playlists = manager_meta.get_all_playlists().await.map_err(|e| e.to_string())?;
                // Mapeamos a la tupla (String, String, Option<String>)
                let metadata: Vec<(String, String, Option<String>)> = playlists
                    .into_iter()
                    .filter(|p| p.id != system_id_for_meta)
                    .map(|p| (p.id, p.name, p.cover_url))
                    .collect();
                Ok::<Vec<(String, String, Option<String>)>, String>(metadata)
            },
            CatalogStoreMessage::PlaylistsMetadataLoaded,
        );

        let followed_manager = Arc::clone(&self.followed_artist_manager);
        let followed_task = Task::perform(
            async move { followed_manager.list_followed(FOLLOWED_ARTISTS_LIMIT).await.map_err(|e| e.to_string()) },
            CatalogStoreMessage::FollowedArtistsLoaded,
        );

        Task::batch(vec![likes_task, order_task, metadata_task, followed_task, self.refresh_play_stats()])
    }

    /// Reemplaza (o agrega) un track con la versión fresca del servidor,
    /// conservando el `liked` local.
    fn upsert_track(&mut self, mut track: Track) {
        match self.index_by_id.get(&track.id) {
            Some(&idx) => {
                let previous = &self.all_tracks[idx];
                track.liked = previous.liked;
                track.play_count = previous.play_count;
                track.last_played_at = previous.last_played_at;
                keep_known_fields(&mut track, previous);
                self.all_tracks[idx] = track;
            }
            None => {
                track.liked = self.liked_order.contains(&track.id);
                let next_idx = self.all_tracks.len();
                self.index_by_id.insert(track.id.clone(), next_idx);
                self.all_tracks.push(track);
            }
        }
        self.bump_version();
    }

    fn rebuild_index(&mut self) {
        self.index_by_id = self
            .all_tracks
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.id.clone(), idx))
            .collect();
        self.bump_version();
    }
}

/// Completa con `previous` los campos que `track` trae vacíos.
fn keep_known_fields(track: &mut Track, previous: &Track) {
    fn keep<T: Clone>(field: &mut Option<T>, previous: &Option<T>) {
        if field.is_none() {
            field.clone_from(previous);
        }
    }

    keep(&mut track.added_at, &previous.added_at);
    keep(&mut track.bpm, &previous.bpm);
    keep(&mut track.camelot_key, &previous.camelot_key);
    keep(&mut track.file_path, &previous.file_path);
    keep(&mut track.thumbnail_small, &previous.thumbnail_small);
    keep(&mut track.thumbnail_large, &previous.thumbnail_large);
    keep(&mut track.album, &previous.album);
    if track.artists.is_empty() {
        track.artists.clone_from(&previous.artists);
    }
}

/// Posición para agregar un track al final de `ids`.
fn next_append_position(ids: &[(String, f64)]) -> f64 {
    ids.last().map(|(_, p)| p + 1.0).unwrap_or(0.0)
}

/// Nueva posición para un track movido, dados sus vecinos de destino
/// (`None` si no hay vecino de ese lado). Devuelve `None` si `prev`/`next`
/// están demasiado pegadas para tener un punto medio `f64` distinto.
fn compute_new_position(prev: Option<f64>, next: Option<f64>) -> Option<f64> {
    match (prev, next) {
        (None, None) => Some(0.0),
        (None, Some(next)) => Some(next - 1.0),
        (Some(prev), None) => Some(prev + 1.0),
        (Some(prev), Some(next)) => {
            let mid = prev + (next - prev) / 2.0;
            (mid > prev && mid < next).then_some(mid)
        }
    }
}