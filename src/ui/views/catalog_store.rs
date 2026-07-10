/// # CatalogStore — fuente de la verdad del catálogo de tracks
///
/// ## Qué es
/// `CatalogStore` es el único dueño de `Vec<Track>` completo (todo lo que
/// existe en `music_center`, resuelto vía `MicroserviceClient` en chunks).
///
/// 1. `SidebarFeature` es dueño de UNA instancia de `CatalogStore` y la
///    construye junto a los demás distritos en `SidebarFeature::new`.
/// 2. Los mensajes de carga (`CatalogStoreMessage::IdsLoaded`,
///    `ChunkResolved`) se rutean desde `SidebarFeature::update` hacia
///    `catalog_store.update(msg)`, igual que cualquier otro distrito.
/// 3. Cualquier vista (Explorer, Favorites, Playlists) YA NO guarda su
///    propia copia de tracks. En vez de eso, en su `view()`/`update()`
///    recibe `&CatalogStore` como parámetro extra y pide su slice:
///
///    // Todo el catálogo (Explorer aplica su propio filtro/orden encima):
///    let tracks: &[Track] = store.all_tracks();
///
///    // Solo los tracks de una playlist puntual:
///    let slide: Vec<&Track> = store.tracks_for_playlist(&playlist_id);
///
///    // Un track puntual por id (útil para refrescar selección/menú):
///    if let Some(track) = store.track_by_id(&id) { ... }
///    ```
///

use std::collections::HashMap;
use std::sync::Arc;

use iced::Task;

use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

const CHUNK_SIZE: usize = 250;

#[derive(Debug, Clone)]
pub enum CatalogStoreMessage {
    IdsLoaded(Result<Vec<String>, String>),
    ChunkResolved(usize, Result<Vec<Track>, String>),
}

pub struct CatalogStore {
    client: Arc<MicroserviceClient>,
    all_tracks: Vec<Track>,
    index_by_id: HashMap<String, usize>,
    pending_chunks: HashMap<usize, Vec<Track>>,
    total_chunks: usize,
    is_loading: bool,
    last_error: Option<String>,
}

impl CatalogStore {
    pub fn load(client: Arc<MicroserviceClient>) -> (Self, Task<CatalogStoreMessage>) {
        let store = Self {
            client: Arc::clone(&client),
            all_tracks: Vec::new(),
            index_by_id: HashMap::new(),
            pending_chunks: HashMap::new(),
            total_chunks: 0,
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

    pub fn tracks_for_playlist(&self, _playlist_id: &str) -> Vec<&Track> {
        // TODO: reemplazar por el filtro real una vez confirmado el
        // campo/relación en `Track` (p. ej. `track.playlist_ids.contains(...)`
        // o una tabla intermedia resuelta por el microservicio).
        Vec::new()
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
                        return Task::none();
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
                }

                Task::none()
            }
        }
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