/// # CatalogStore — fuente de la verdad del catálogo de tracks
///
/// ## Qué es
/// `CatalogStore` es el único dueño de `Vec<Track>` completo (todo lo que
/// existe en `music_center`, resuelto vía `MicroserviceClient` en chunks) y
/// también el dueño de las relaciones locales de playlist/likes, resueltas
/// vía `PlaylistManager` (SQLite). El servicio de reproducción/descarga es
/// agnóstico a playlists — todo lo que es "pertenece a X playlist" o
/// "está likeado" vive aquí, no en el microservicio remoto.
///
/// 1. `SidebarFeature` es dueño de UNA instancia de `CatalogStore` y la
///    construye junto a los demás distritos en `SidebarFeature::new`.
/// 2. Los mensajes de carga (`CatalogStoreMessage::IdsLoaded`,
///    `ChunkResolved`, `LikesLoaded`, `PlaylistOrderLoaded`) se rutean desde
///    `SidebarFeature::update` hacia `catalog_store.update(msg)`, igual que
///    cualquier otro distrito.
/// 3. Cualquier vista (Explorer, Favorites, Playlists) YA NO guarda su
///    propia copia de tracks. En vez de eso, en su `view()`/`update()`
///    recibe `&CatalogStore` como parámetro extra y pide su slice:
///
///    // Todo el catálogo (Explorer aplica su propio filtro/orden encima):
///    let tracks: &[Track] = store.all_tracks();
///
///    // Solo los tracks de una playlist puntual (incluye la de Likes,
///    // identificada internamente por `PlaylistManager::system_playlist_id`):
///    let slice: Vec<&Track> = store.tracks_for_playlist(&playlist_id);
///
///    // Un track puntual por id (útil para refrescar selección/menú):
///    if let Some(track) = store.track_by_id(&id) { ... }
///
/// ## Sobre `liked`
/// No se mantiene un `HashSet` aparte: el estado de like vive directamente
/// en `Track::liked` dentro de `all_tracks`. La vista de Favoritos simplemente
/// filtra `all_tracks.iter().filter(|t| t.liked)`. Al hacer toggle, se muta
/// el campo en memoria de forma optimista y se persiste en SQLite en
/// background vía `PlaylistManager`.
///
/// ## Sobre el orden de playlists
/// `playlist_order` guarda `playlist_id -> Vec<track_id>` para las playlists
/// CUSTOM (la SYSTEM/Likes no necesita orden, se resuelve filtrando `liked`).
/// Se carga de forma eager al arrancar junto con los tracks. El orden por
/// defecto es el de inserción (último agregado al final); reordenar es
/// responsabilidad de quien llame a `PlaylistManager::reorder_tracks` y
/// luego refresque `playlist_order` vía el mensaje correspondiente.

use std::collections::HashMap;
use std::sync::Arc;

use iced::Task;

use crate::db::playlist_manager::PlaylistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

const CHUNK_SIZE: usize = 250;

#[derive(Debug, Clone)]
pub enum CatalogStoreMessage {
    IdsLoaded(Result<Vec<String>, String>),
    ChunkResolved(usize, Result<Vec<Track>, String>),
    TrackDeleted(String, Result<(), String>),

    /// Ids de tracks likeados (contenido de la playlist SYSTEM), llega una
    /// sola vez tras terminar de resolver todos los chunks de `all_tracks`.
    LikesLoaded(Result<Vec<String>, String>),

    /// `(playlist_id, [track_id])` para todas las playlists CUSTOM, cargado
    /// eager junto con los likes.
    PlaylistOrderLoaded(Result<Vec<(String, Vec<String>)>, String>),

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
    /// `(playlist_id, track_id, result)`. Si falla, se revierte la
    /// eliminación optimista de `playlist_order`.
    TrackRemovedFromPlaylist(String, String, Result<(), String>),
}

pub struct CatalogStore {
    client: Arc<MicroserviceClient>,
    playlist_manager: Arc<PlaylistManager>,

    all_tracks: Vec<Track>,
    index_by_id: HashMap<String, usize>,
    pending_chunks: HashMap<usize, Vec<Track>>,
    total_chunks: usize,

    playlist_order: HashMap<String, Vec<String>>,
    liked_order: Vec<String>,
    playlists_metadata: Vec<(String, String, Option<String>)>,

    is_loading: bool,
    last_error: Option<String>,
}

impl CatalogStore {
    pub fn load(
        client: Arc<MicroserviceClient>,
        playlist_manager: Arc<PlaylistManager>,
    ) -> (Self, Task<CatalogStoreMessage>) {
        let store = Self {
            client: Arc::clone(&client),
            playlist_manager,
            all_tracks: Vec::new(),
            index_by_id: HashMap::new(),
            pending_chunks: HashMap::new(),
            total_chunks: 0,
            playlist_order: HashMap::new(),
            liked_order: Vec::new(),
            playlists_metadata: Vec::new(),
            is_loading: true,
            last_error: None,
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

    // ── Métodos de acceso ────────────────────────────────

    pub fn all_tracks(&self) -> &[Track] {
        &self.all_tracks
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
            .filter_map(|id| self.track_by_id(id))
            .collect()
    }

    pub fn delete_track(&mut self, track_id: &str) {
        self.all_tracks.retain(|t| t.id != track_id);
        self.rebuild_index();

        let client = Arc::clone(&self.client);
        let id_clone = track_id.to_string();

        tokio::spawn(async move {
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
            self.liked_order.insert(0, track_id.to_string());
        } else {
            self.liked_order.retain(|id| id != track_id);
        }

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

    /// Indica si un track ya pertenece a una playlist CUSTOM puntual.
    /// No aplica a la playlist SYSTEM (Likes) — para eso se usa
    /// `Track::liked` directo. Útil para que el submenú "Agregar a
    /// playlist" pueda deshabilitar/marcar las playlists que ya lo
    /// contienen en vez de permitir duplicados silenciosos.
    pub fn is_track_in_playlist(&self, playlist_id: &str, track_id: &str) -> bool {
        self.playlist_order
            .get(playlist_id)
            .map(|ids| ids.iter().any(|id| id == track_id))
            .unwrap_or(false)
    }

    /// Agrega un track al FINAL de una playlist CUSTOM. La posición
    /// (`f64` en `PlaylistManager::add_tracks`, pensada para poder
    /// insertar entre dos existentes en el futuro) se calcula sola acá:
    /// simplemente "la cantidad de tracks que ya tiene la playlist" —
    /// como las posiciones son 0-based y consecutivas mientras solo se
    /// agregue al final, eso siempre cae después de la última. El
    /// caller (la vista) no necesita saber nada de posiciones.
    ///
    /// Muta `playlist_order` de forma optimista y persiste en SQLite en
    /// background. No aplica a la playlist SYSTEM (Likes) — usar
    /// `toggle_like` para esa. Si el track ya está en la playlist, no
    /// hace nada (evita duplicados y un round-trip innecesario).
    pub fn add_track_to_playlist(&mut self, playlist_id: &str, track_id: &str) -> Task<CatalogStoreMessage> {
        if self.is_track_in_playlist(playlist_id, track_id) {
            return Task::none();
        }

        let next_position = self.playlist_order
            .get(playlist_id)
            .map(|ids| ids.len() as f64)
            .unwrap_or(0.0);

        self.playlist_order
            .entry(playlist_id.to_string())
            .or_default()
            .push(track_id.to_string());

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id_clone = playlist_id.to_string();
        let track_id_clone = track_id.to_string();

        Task::perform(
            async move {
                let result = manager
                    .add_tracks(&playlist_id_clone, &[(track_id_clone.clone(), next_position)])
                    .await;
                (playlist_id_clone, track_id_clone, result.map_err(|e| e.to_string()))
            },
            |(playlist_id, track_id, result)| {
                CatalogStoreMessage::TrackAddedToPlaylist(playlist_id, track_id, result)
            },
        )
    }

    /// Quita un track de una playlist CUSTOM: muta `playlist_order` de
    /// forma optimista y persiste en SQLite en background. No aplica a
    /// la playlist SYSTEM (Likes) — usar `toggle_like` para esa
    /// (`PlaylistManager::remove_tracks` rechaza explícitamente el id
    /// de la playlist SYSTEM).
    pub fn remove_track_from_playlist(&mut self, playlist_id: &str, track_id: &str) -> Task<CatalogStoreMessage> {
        if let Some(ids) = self.playlist_order.get_mut(playlist_id) {
            ids.retain(|id| id != track_id);
        }

        let manager = Arc::clone(&self.playlist_manager);
        let playlist_id_clone = playlist_id.to_string();
        let track_id_clone = track_id.to_string();

        Task::perform(
            async move {
                let result = manager
                    .remove_tracks(&playlist_id_clone, &[track_id_clone.clone()])
                    .await;
                (playlist_id_clone, track_id_clone, result.map_err(|e| e.to_string()))
            },
            |(playlist_id, track_id, result)| {
                CatalogStoreMessage::TrackRemovedFromPlaylist(playlist_id, track_id, result)
            },
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
                        ids.retain(|id| id != &track_id);
                    }
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
                    self.playlist_order
                        .entry(playlist_id)
                        .or_default()
                        .push(track_id);
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
                    .get_all_playlist_track_ids()
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

        Task::batch(vec![likes_task, order_task, metadata_task])
    }

    fn rebuild_index(&mut self) {
        self.index_by_id = self
            .all_tracks
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.id.clone(), idx))
            .collect();
    }
}