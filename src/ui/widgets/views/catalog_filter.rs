//! # catalog_filter — búsqueda y "claves visibles" compartidas
//!
//! ## Por qué existe
//!
//! Dos bloques que eran idénticos carácter por carácter en
//! `ExplorerView`, `FavoritesView` y `PlaylistsView`:
//!
//! - `apply_search`: filtra el slice de tracks de la vista contra
//!   `SearchQuery`, comparando título/artistas/álbum, y devuelve los
//!   índices que matchean (o todos, si la query está vacía).
//! - `visible_keys`: recorre la ventana visible actual (`VirtualWindow`)
//!   y junta las `thumb_key` de los tracks en pantalla, para decirle al
//!   cache de miniaturas cuáles conservar.
//!
//! Ninguno de los dos necesita saber de dónde sale el slice de tracks
//! (`store.all_tracks()`, `liked_tracks(store)`,
//! `current_playlist_tracks(store)`) — por eso `search_indices` es
//! genérico sobre `TrackSlice` (el mismo trait ya usado en
//! `track_sort`, así cubre tanto `[Track]` como `[&Track]` sin impls
//! nuevos) y `visible_keys` recibe `track_at` como closure, ya que
//! `track_at` en sí varía por vista (cada una indexa una fuente
//! distinta) y no es abstraíble sin forzar una interfaz común sobre
//! `CatalogStore`.
//!
//! ## Cómo se consume
//!
//! ```ignore
//! // apply_search, antes 12 líneas repetidas:
//! fn apply_search(&mut self, store: &CatalogStore) {
//!     let tracks = store.all_tracks(); // o el slice que aplique
//!     self.filtered_indices = catalog_filter::search_indices(&tracks, &self.search_query);
//! }
//!
//! // visible_keys, antes 7 líneas repetidas:
//! fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
//!     let window = self.current_window();
//!     catalog_filter::visible_keys(window, |visible_idx| self.track_at(store, visible_idx))
//! }
//! ```

use std::collections::HashSet;

use crate::model::Track;
use crate::ui::utils::search::SearchQuery;
use crate::ui::utils::thumbnail_cache::thumb_key;
use crate::ui::utils::virtual_list::VirtualWindow;
use crate::ui::widgets::views::track_sort::TrackSlice;

/// Extensión mínima sobre `TrackSlice` para poder iterar por índice sin
/// que cada vista tenga que exponer su propio `.len()` por separado
/// (Explorer usa `&[Track]`, Favorites/Playlists `Vec<&Track>` — ambos
/// ya tienen `.len()` nativo, pero no hay forma de pedirlo genéricamente
/// a través de `TrackSlice` tal como está definido en `track_sort.rs`).
pub trait TrackSliceLen: TrackSlice {
    fn track_len(&self) -> usize;
}

impl TrackSliceLen for [Track] {
    fn track_len(&self) -> usize {
        self.len()
    }
}

impl TrackSliceLen for [&Track] {
    fn track_len(&self) -> usize {
        self.len()
    }
}

impl TrackSliceLen for Vec<Track> {
    fn track_len(&self) -> usize {
        self.len()
    }
}

impl TrackSliceLen for Vec<&Track> {
    fn track_len(&self) -> usize {
        self.len()
    }
}

/// Reemplaza el bloque de `apply_search` repetido en las 3 vistas.
/// Compara título, artistas formateados y nombre de álbum contra
/// `query`; si `query` está vacía devuelve todos los índices en orden
/// (0..len), igual que el comportamiento original.
pub fn search_indices<T: TrackSliceLen + ?Sized>(tracks: &T, query_text: &str) -> Vec<usize> {
    let query = SearchQuery::new(query_text);
    let len = tracks.track_len();

    if query.is_empty() {
        return (0..len).collect();
    }

    (0..len)
        .filter(|&idx| {
            let track = tracks.track_at(idx);
            let album_name = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            let artists = track.format_artists();
            query.matches_any(&[&track.title, &artists, album_name])
        })
        .collect()
}

/// Reemplaza el bloque de `visible_keys` repetido en las 3 vistas.
/// `track_at` es el mismo closure que cada vista ya usa para resolver
/// `visible_idx -> &Track` (varía porque cada vista indexa una fuente
/// de tracks distinta). Toma `window` por referencia para no asumir que
/// `VirtualWindow` es `Copy`.
pub fn visible_keys<'a>(
    window: &VirtualWindow,
    mut track_at: impl FnMut(usize) -> Option<&'a Track>,
) -> HashSet<String> {
    (window.start..window.end)
        .filter_map(|visible_idx| track_at(visible_idx))
        .map(thumb_key)
        .collect()
}