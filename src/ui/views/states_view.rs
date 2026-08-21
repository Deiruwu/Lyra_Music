use std::cell::RefCell;
use std::collections::HashSet;
use std::time::Instant;
use iced::keyboard::Modifiers;
use iced::Point;
use crate::model::Track;
use crate::ui::utils::search::SearchQuery;
use crate::ui::utils::virtual_list::{ScrollTracker, VirtualWindow};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::selection_state::SelectionState;
use crate::ui::widgets::track_list_builder;
use crate::ui::widgets::track_list_builder::TrackEvent;

const ROW_HEIGHT: f32 = 60.0;
const BUFFER_ROWS: usize = 15;

/// Resultado cacheado de filtrar+ordenar tracks para una vista puntual.
/// Guarda ids, no `&Track`: este cache vive dentro de `TrackViewState`,
/// que persiste entre llamadas a `update()`/`view()`, pero los `&Track`
/// que llegan como parámetro solo viven durante ESA llamada puntual
/// (`CatalogStore::all_tracks()`/`tracks_for_playlist()` devuelven
/// referencias con el lifetime de ese `&CatalogStore` de esa invocación).
/// Guardar `Vec<String>` evita el problema de lifetimes por completo: en
/// un cache-hit se resuelven de nuevo contra `CatalogStore::track_by_id`
/// (O(1), HashMap) — muchísimo más barato que repetir el filtro difuso
/// (`SearchQuery::matches_any` hace fuzzy match con Levenshtein por
/// palabra, no es un `contains()` barato).
#[derive(Debug, Clone, Default)]
struct RenderedTracksCache {
    query: String,
    sort_key: Option<usize>,
    sort_ascending: bool,
    catalog_version: u64,
    /// Ids en el orden final ya filtrado+ordenado.
    ids: Vec<String>,
    /// `false` hasta el primer cálculo real. Sin esto, un cache recién
    /// creado (`catalog_version` en 0 por `Default`) podría coincidir por
    /// casualidad con un `CatalogStore` recién creado (también en 0,
    /// antes de terminar de cargar) y devolver un cache-hit vacío como
    /// si fuera válido.
    primed: bool,
}

/// Filtra `tracks` contra `raw_query` usando la utilidad de búsqueda
/// difusa compartida. Matchea contra título, artistas formateados y
/// álbum. Query vacía → devuelve todo sin tocar el orden
/// (`SearchQuery::is_empty` ya hace early-return interno, pero evitamos
/// incluso construir la query si no hace falta).
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


#[derive(Debug, Clone)]
pub enum ListAction {
    PlayContext(String),
    SortChanged(usize),
    OpenContextMenu { anchor_id: String, selected_ids: HashSet<String> },
    OpenArtist(String),
    OpenAlbum(String),
    None,
}

#[derive(Debug, Clone)]
pub struct TrackViewState {
    pub search_filter: String,
    pub keybinds_press: Modifiers,

    pub tracks_selection: SelectionState,
    pub scroll: ScrollTracker,

    pub mouse_position: Option<Point>,

    pub active_sort_key: Option<usize>,
    pub sort_direction_asc: bool,

    pub last_click: Option<(String, Instant)>,

    /// Cache de `filter_tracks` + `sort_tracks` sobre esta vista. `RefCell`
    /// porque `ViewCoordinator::view_content()` y
    /// `active_view_thumbnail_targets()` son `&self` (obligatorio: `view()`
    /// en iced no puede mutar), y necesitan compartir el mismo cache que
    /// `update_route()` (`&mut self`) sin triplicar la implementación.
    /// `TrackViewState` es propiedad exclusiva de una sola vista, todo
    /// corre single-threaded en el loop de iced — no hay aliasing real.
    cache: RefCell<RenderedTracksCache>,
}

impl Default for TrackViewState {
    fn default() -> Self {
        Self {
            search_filter: String::new(),
            keybinds_press: Modifiers::default(),
            tracks_selection: SelectionState::new(),
            scroll: ScrollTracker::default(),
            mouse_position: None,
            active_sort_key: Some(0),
            sort_direction_asc: true,
            last_click: None,
            cache: RefCell::new(RenderedTracksCache::default()),
        }
    }
}

impl TrackViewState {
    pub fn new() -> Self {
        Self::default()
    }

    fn visible_index_range(&self, total_items: usize) -> VirtualWindow {
        self.scroll.window(ROW_HEIGHT, total_items, BUFFER_ROWS)
    }

    /// Universo `(key, url)` de la ventana visible actual (+buffer),
    /// listo para pasar a `AsyncThumbnail::sync()`. El coordinator llama
    /// esto al final de su `update()`, sin importar qué evento llegó —
    /// no hace falta invocarlo desde cada rama de `process_event`.
    pub fn visible_thumbnail_targets(&self, rendered_tracks: &[&Track]) -> Vec<(String, String)> {
        track_list_builder::visible_thumbnail_targets(
            &self.scroll,
            rendered_tracks,
            ROW_HEIGHT,
            BUFFER_ROWS,
        )
    }

    pub fn apply_search_filter(&mut self, query: String) {
        self.search_filter = query;
        self.scroll.reset();
    }

    /// Filtra y ordena `source` contra el estado actual de búsqueda/orden
    /// de esta vista, cacheando el resultado. Si nada relevante cambió
    /// desde la última llamada (mismo texto de búsqueda, mismo sort, y
    /// `catalog.version()` sin bumpear), NO vuelve a correr el filtro
    /// difuso: resuelve los ids ya cacheados contra
    /// `CatalogStore::track_by_id` (O(1) cada uno) en vez de recorrer
    /// `source` entero con Levenshtein por palabra en cada mensaje/frame.
    ///
    /// `source` es el universo elegible para ESTA vista (todo el catálogo
    /// para Explorer, o el subconjunto de una playlist/Favoritos) — lo
    /// resuelve el caller porque `TrackViewState` no sabe de playlists.
    pub fn rendered<'a>(&self, source: &[&'a Track], catalog: &'a CatalogStore) -> Vec<&'a Track> {
        let catalog_version = catalog.version();
        let mut cache = self.cache.borrow_mut();

        let hit = cache.primed
            && cache.query == self.search_filter
            && cache.sort_key == self.active_sort_key
            && cache.sort_ascending == self.sort_direction_asc
            && cache.catalog_version == catalog_version;

        if hit {
            return cache.ids.iter().filter_map(|id| catalog.track_by_id(id)).collect();
        }

        let mut filtered = filter_tracks(source, &self.search_filter);
        track_list_builder::sort_tracks(&mut filtered, self.active_sort_key, self.sort_direction_asc);

        *cache = RenderedTracksCache {
            query: self.search_filter.clone(),
            sort_key: self.active_sort_key,
            sort_ascending: self.sort_direction_asc,
            catalog_version,
            ids: filtered.iter().map(|t| t.id.clone()).collect(),
            primed: true,
        };

        filtered
    }

    pub fn toggle_sort(&mut self, sort_key: usize) -> bool {
        if self.active_sort_key == Some(sort_key) {
            self.sort_direction_asc = !self.sort_direction_asc;
            false
        } else {
            self.active_sort_key = Some(sort_key);
            self.sort_direction_asc = true;
            true
        }
    }

    pub fn register_click(&mut self, track_id: &str) -> bool {
        let now = Instant::now();
        let is_double_click = match &self.last_click {
            Some((last_id, time)) => {
                last_id == track_id && now.duration_since(*time).as_millis() < 500
            }
            None => false,
        };

        self.last_click = Some((track_id.to_string(), now));

        is_double_click
    }

    pub fn process_event(
        &mut self,
        event: TrackEvent,
        rendered_tracks: &[&Track],
    ) -> ListAction {
        match event {
            TrackEvent::MouseMoved(p) => {
                self.mouse_position = Some(p);
                ListAction::None
            }
            TrackEvent::ViewportExited => {
                self.mouse_position = None;
                ListAction::None
            }
            TrackEvent::Scrolled(viewport) => {
                self.scroll.update(viewport);
                ListAction::None
            }
            TrackEvent::Sorted(sort_key) => {
                self.toggle_sort(sort_key);
                ListAction::SortChanged(sort_key)
            }
            TrackEvent::Clicked(track, index) => {
                if self.register_click(&track.id) {
                    ListAction::PlayContext(track.id.clone())
                } else {
                    if self.keybinds_press.shift() {
                        let visible_ids: Vec<&String> = rendered_tracks.iter().map(|t| &t.id).collect();
                        self.tracks_selection.select_range(index, &visible_ids);
                    } else if self.keybinds_press.command() || self.keybinds_press.control() {
                        self.tracks_selection.toggle(track.id.clone(), index);
                    } else {
                        self.tracks_selection.select_single(track.id.clone(), index);
                    }
                    ListAction::None
                }
            }
            TrackEvent::RightClicked(track_id) => {
                if !self.tracks_selection.is_selected(&track_id) {
                    self.tracks_selection.clear();
                    let idx = rendered_tracks.iter().position(|t| t.id == track_id).unwrap_or(0);
                    self.tracks_selection.select_single(track_id.clone(), idx);
                }

                ListAction::OpenContextMenu {
                    anchor_id: track_id,
                    selected_ids: self.tracks_selection.selected_ids.clone(),
                }
            }
            TrackEvent::ArtistClicked(artist_id) => ListAction::OpenArtist(artist_id),
            TrackEvent::AlbumClicked(album_id) => ListAction::OpenAlbum(album_id),
        }
    }
}