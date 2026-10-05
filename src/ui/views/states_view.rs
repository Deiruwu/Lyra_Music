use std::cell::RefCell;
use std::collections::HashSet;
use iced::keyboard::Modifiers;
use iced::Point;
use crate::model::Track;
use crate::ui::utils::search::SearchQuery;
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::selection_state::{DoubleClickDetector, SelectionState};
use crate::ui::widgets::track_list_builder;
use crate::ui::widgets::track_list_builder::{TrackEvent, COLUMN_HEADER_HEIGHT};

pub(crate) const ROW_HEIGHT: f32 = 60.0;
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
    /// Índices en `CatalogStore::all_tracks`, en el orden final ya
    /// filtrado+ordenado. Índices y no ids: resolver la lista cacheada es
    /// un acceso directo al vector en vez de un hash de UUID por track.
    indices: Vec<u32>,
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
pub(crate) fn filter_tracks<'a>(tracks: &[&'a Track], raw_query: &str) -> Vec<&'a Track> {
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
    TogglePlayback,
    None,
}

#[derive(Debug, Clone)]
pub struct TrackViewState {
    pub search_filter: String,
    pub keybinds_press: Modifiers,

    pub tracks_selection: SelectionState,
    pub scroll: ScrollTracker,

    pub mouse_position: Option<Point>,

    /// Alto de lo que va arriba de las filas dentro del mismo scroll (0 si
    /// la tabla scrollea sola). Lo usan miniaturas, teclado y arrastre.
    pub rows_offset: f32,

    pub playing_icon_hovered: bool,

    /// Override explícito del usuario (click en un header). `None` =
    /// sin override, se usa `default_sort_key`/`default_sort_ascending`.
    pub active_sort_key: Option<usize>,
    pub sort_direction_asc: bool,

    /// Orden implícito de esta vista (fijado una sola vez en su
    /// constructor, nunca mutado después). No se muestra flecha para
    /// este orden — solo para un `active_sort_key` explícito.
    pub default_sort_key: Option<usize>,
    pub default_sort_ascending: bool,

    pub clicks: DoubleClickDetector,

    /// `true` mientras hay un recentrado de scroll pendiente sobre la
    /// selección actual (filtro recién limpiado/cambiado con algo
    /// seleccionado) — lo consume `take_pending_scroll_target`.
    pending_scroll_to_selection: bool,

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
            rows_offset: 0.0,
            playing_icon_hovered: false,
            active_sort_key: None,
            sort_direction_asc: true,
            default_sort_key: Some(0),
            default_sort_ascending: true,
            clicks: DoubleClickDetector::default(),
            pending_scroll_to_selection: false,
            cache: RefCell::new(RenderedTracksCache::default()),
        }
    }
}

impl TrackViewState {
    pub fn new() -> Self {
        Self::default()
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
            self.rows_offset,
        )
    }

    /// Angostar el filtro (o escribirlo desde cero) sigue reseteando el
    /// scroll a 0, igual que antes. Limpiarlo o cambiarlo a algo que no
    /// sea una extensión del anterior, en cambio, deja el scroll donde
    /// está y — si hay algo seleccionado — marca un recentrado
    /// pendiente para que el coordinator scrollee hasta la selección
    /// una vez recalculada la lista con el filtro nuevo.
    pub fn apply_search_filter(&mut self, query: String) {
        let is_narrowing = query.starts_with(&self.search_filter);
        self.search_filter = query;

        if is_narrowing || self.tracks_selection.selected_ids.is_empty() {
            self.scroll.reset();
            self.pending_scroll_to_selection = false;
        } else {
            self.pending_scroll_to_selection = true;
        }
    }

    /// Consume el pedido de recentrado si hay uno pendiente, devolviendo
    /// un id de la selección actual a partir del cual el coordinator
    /// calcula el offset de scroll objetivo.
    pub fn take_pending_scroll_target(&mut self) -> Option<String> {
        if !self.pending_scroll_to_selection {
            return None;
        }
        self.pending_scroll_to_selection = false;
        self.tracks_selection.selected_ids.iter().next().cloned()
    }

    /// Fuerza a recalcular la lista en el próximo `rendered` (cuando cambió la
    /// fuente sin que cambie el catálogo, p. ej. al rehacer una mezcla).
    pub fn invalidate_cache(&self) {
        self.cache.borrow_mut().primed = false;
    }

    /// `(key, ascending)` a usar realmente para ordenar/pintar: el
    /// override explícito del usuario si hay uno, si no el default de
    /// esta vista.
    pub fn effective_sort(&self) -> (Option<usize>, bool) {
        match self.active_sort_key {
            Some(_) => (self.active_sort_key, self.sort_direction_asc),
            None => (self.default_sort_key, self.default_sort_ascending),
        }
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
            return cache.indices.iter().filter_map(|&i| catalog.track_at(i)).collect();
        }

        let (sort_key, sort_ascending) = self.effective_sort();
        let mut filtered = filter_tracks(source, &self.search_filter);
        track_list_builder::sort_tracks(&mut filtered, sort_key, sort_ascending);

        *cache = RenderedTracksCache {
            query: self.search_filter.clone(),
            sort_key: self.active_sort_key,
            sort_ascending: self.sort_direction_asc,
            catalog_version,
            indices: filtered.iter().filter_map(|t| catalog.index_of(&t.id)).collect(),
            primed: true,
        };

        filtered
    }

    /// Ciclo de 3 clicks sobre una misma columna: ascendente → descendente
    /// → sin override (vuelve al orden default de la vista, oculta la
    /// flecha). Clickear una columna distinta siempre arranca en
    /// ascendente.
    pub fn toggle_sort(&mut self, sort_key: usize) {
        match self.active_sort_key {
            Some(k) if k == sort_key && self.sort_direction_asc => {
                self.sort_direction_asc = false;
            }
            Some(k) if k == sort_key => {
                self.active_sort_key = None;
                self.sort_direction_asc = true;
            }
            _ => {
                self.active_sort_key = Some(sort_key);
                self.sort_direction_asc = true;
            }
        }
    }

    /// Mueve la selección con las flechas; devuelve el índice nuevo del cursor.
    pub fn move_selection(&mut self, delta: isize, extend: bool, rendered_tracks: &[&Track]) -> Option<usize> {
        let visible_ids: Vec<&String> = rendered_tracks.iter().map(|t| &t.id).collect();
        self.tracks_selection.move_cursor(delta, extend, &visible_ids)
    }

    /// Canciones que arrastra apretar la fila bajo el mouse: toda la selección (en el orden
    /// visible) si esa fila está seleccionada, si no solo esa fila.
    pub fn drag_candidate<'a>(&self, rendered_tracks: &[&'a Track]) -> Option<Vec<&'a Track>> {
        let position = self.mouse_position?;
        // Con los títulos de columna fijos arriba, apretarlos no agarra la fila de abajo.
        let header_pinned = self.rows_offset > 0.0 && self.scroll.offset_y + COLUMN_HEADER_HEIGHT >= self.rows_offset;
        if !self.scroll.is_within_content(position.x) || (header_pinned && position.y < COLUMN_HEADER_HEIGHT) {
            return None;
        }
        let content_y = position.y + self.scroll.offset_y - self.rows_offset;
        if content_y < 0.0 {
            return None;
        }
        let pressed = rendered_tracks.get((content_y / ROW_HEIGHT) as usize)?;
        if self.tracks_selection.is_selected(&pressed.id) {
            Some(rendered_tracks.iter().filter(|t| self.tracks_selection.is_selected(&t.id)).copied().collect())
        } else {
            Some(vec![*pressed])
        }
    }

    /// Track bajo el cursor de selección (el que reproduce Enter).
    pub fn cursor_track_id<'a>(&self, rendered_tracks: &[&'a Track]) -> Option<&'a str> {
        let index = self.tracks_selection.cursor_index?;
        rendered_tracks
            .get(index)
            .filter(|t| self.tracks_selection.is_selected(&t.id))
            .map(|t| t.id.as_str())
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
            TrackEvent::Clicked(track_id, index) => {
                if self.clicks.register(&track_id) {
                    ListAction::PlayContext(track_id)
                } else {
                    if self.keybinds_press.shift() {
                        let visible_ids: Vec<&String> = rendered_tracks.iter().map(|t| &t.id).collect();
                        self.tracks_selection.select_range(index, &visible_ids);
                    } else if self.keybinds_press.command() || self.keybinds_press.control() {
                        self.tracks_selection.toggle(track_id, index);
                    } else {
                        self.tracks_selection.select_single(track_id, index);
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
            TrackEvent::TogglePlayback => ListAction::TogglePlayback,
            TrackEvent::PlayingIconHover(hovered) => {
                self.playing_icon_hovered = hovered;
                ListAction::None
            }
        }
    }
}