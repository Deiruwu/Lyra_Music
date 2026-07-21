//! # track_sort — comparadores de columna compartidos
//!
//! ## Por qué existe
//!
//! El `match` dentro de `apply_sort` (Título/Artista/Álbum/BPM/Key/
//! Duración) es idéntico carácter por carácter en `ExplorerView`,
//! `FavoritesView` y `PlaylistsView` — solo cambia qué variantes extra
//! tiene cada enum (`AddedAt` en Explorer, `DefaultOrder` en
//! Favorites/Playlists) y de dónde sale el conjunto de tracks. Esas dos
//! diferencias se quedan en cada vista; lo que se comparte es la
//! comparación en sí.
//!
//! ## El detalle que casi se rompe: `&[Track]` vs `&[&Track]`
//!
//! Las tres vistas NO tienen el mismo tipo de "lista de tracks":
//!
//! - `ExplorerView::apply_sort` usa `store.all_tracks()` → `&[Track]`
//!   (slice de tracks propios, viven dentro de `CatalogStore`).
//! - `FavoritesView`/`PlaylistsView` usan `store.tracks_for_playlist(id)`
//!   → `Vec<&Track>` (vector de referencias — la playlist es una vista
//!   filtrada sobre el catálogo, no una copia).
//!
//! Una firma fija a `&[&Track]` compila para Favorites/Playlists (`&Vec<&Track>`
//! decae a `&[&Track]` solo), pero NO para Explorer: `&[Track]` no es
//! `&[&Track]`, son tipos distintos (uno indexa a `Track`, el otro a
//! `&Track`). Por eso estas funciones son genéricas sobre cualquier
//! cosa indexable por `usize` que dé un `&Track` — `Index<usize, Output = Track>`
//! cubre `[Track]` (Explorer) y `Index<usize, Output = &Track>` cubre
//! `[&Track]` (Favorites/Playlists) — con dos impls delgadas en vez de
//! una firma que solo le sirve a dos de las tres vistas.
//!
//! ## Cómo se consume
//!
//! Cada vista sigue teniendo su propio `match self.sort.column() { ... }`
//! (no se puede eliminar del todo sin forzar un enum común — ver
//! `sort_state.rs`), pero cada brazo pasa a ser una sola línea, sin
//! importar si `tracks` es `&[Track]` o `&[&Track]`:
//!
//! ```ignore
//! // Explorer: tracks: &[Track]
//! match self.sort.column() {
//!     SortColumn::Title => track_sort::by_title(&mut self.filtered_indices, tracks),
//!     ...
//!     SortColumn::AddedAt => track_sort::by_added_at(&mut self.filtered_indices, tracks),
//! }
//!
//! // Favorites/Playlists: tracks: Vec<&Track> (se pasa &tracks)
//! match self.sort.column() {
//!     SortColumn::DefaultOrder => {}
//!     SortColumn::Title => track_sort::by_title(&mut self.filtered_indices, &tracks),
//!     ...
//! }
//! if !asc { self.filtered_indices.reverse(); }
//! ```

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use crate::model::Track;

/// Abstrae "algo indexable por `usize` que da un `&Track`" — implementado
/// tanto para `[Track]`/`Vec<Track>` (Explorer, tracks propios) como para
/// `[&Track]`/`Vec<&Track>` (Favorites/Playlists, tracks prestados). Es lo
/// que permite que estas funciones acepten cualquiera de los dos sin
/// duplicarse.
///
/// Nota sobre por qué van los cuatro impls y no solo los dos de slice:
/// el trait bound genérico (`T: TrackSlice`) exige que el tipo CONCRETO
/// que se pasa detrás de la referencia implemente el trait — a
/// diferencia de una función con parámetro `&[Track]` fijo, aquí NO
/// aplica la coerción automática `&Vec<T> -> &[T]` que sí funciona en
/// llamadas normales. Si a la vista le sale `Vec<&Track>` de
/// `store.tracks_for_playlist(..)` y se le pasa `&tracks`, Rust necesita
/// `Vec<&Track>: TrackSlice`, no `[&Track]: TrackSlice` — por eso los
/// impls sobre `Vec<..>` delegan al slice pero deben existir aparte.
pub trait TrackSlice {
    fn track_at(&self, index: usize) -> &Track;
}

impl TrackSlice for [Track] {
    fn track_at(&self, index: usize) -> &Track {
        &self[index]
    }
}

impl TrackSlice for [&Track] {
    fn track_at(&self, index: usize) -> &Track {
        self[index]
    }
}

impl TrackSlice for Vec<Track> {
    fn track_at(&self, index: usize) -> &Track {
        &self[index]
    }
}

impl TrackSlice for Vec<&Track> {
    fn track_at(&self, index: usize) -> &Track {
        self[index]
    }
}

fn sort_by_key<T: TrackSlice + ?Sized, K: Ord>(
    indices: &mut [usize],
    tracks: &T,
    key: impl Fn(&Track) -> K,
) {
    indices.sort_by_key(|&i| key(tracks.track_at(i)));
}

fn sort_by_cmp<T: TrackSlice + ?Sized>(
    indices: &mut [usize],
    tracks: &T,
    cmp: impl Fn(&Track, &Track) -> Ordering,
) {
    indices.sort_by(|&a, &b| cmp(tracks.track_at(a), tracks.track_at(b)));
}

pub fn by_title<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_cmp(indices, tracks, |a, b| {
        a.title.to_lowercase().cmp(&b.title.to_lowercase())
    });
}

pub fn by_artist<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_cmp(indices, tracks, |a, b| {
        a.format_artists().to_lowercase().cmp(&b.format_artists().to_lowercase())
    });
}

pub fn by_album<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_cmp(indices, tracks, |a, b| {
        let an = a.album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
        let bn = b.album.as_ref().map(|x| x.name.to_lowercase()).unwrap_or_default();
        an.cmp(&bn)
    });
}

pub fn by_bpm<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_key(indices, tracks, |t| t.bpm.unwrap_or(i32::MIN));
}

pub fn by_camelot_key<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_cmp(indices, tracks, |a, b| {
        let ak = a.camelot_key.clone().unwrap_or_default();
        let bk = b.camelot_key.clone().unwrap_or_default();
        ak.cmp(&bk)
    });
}

pub fn by_duration<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_key(indices, tracks, |t| t.duration_seconds);
}




pub fn by_added_at<T: TrackSlice + ?Sized>(indices: &mut [usize], tracks: &T) {
    sort_by_key(indices, tracks, |t| t.added_at.unwrap_or(DateTime::<Utc>::MIN_UTC));
}