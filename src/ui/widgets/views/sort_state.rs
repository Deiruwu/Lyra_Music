//! # SortState — estado + ciclo de click de ordenamiento, compartido
//!
//! ## Por qué existe
//!
//! `ExplorerView`, `FavoritesView` y `PlaylistsView` cada una repetía:
//! - Tres campos: `sort_column`, `sort_direction`, `sort_click_stage`.
//! - El mismo ciclo de 3 estados al hacer click en una columna (asc →
//!   desc → default, o cambiar de columna → asc).
//! - El mismo cálculo de `active_sort_key` para `TrackListConfig`
//!   (`None` cuando estamos en el estado default, para que el widget no
//!   pinte flecha de sort en ninguna columna).
//!
//! Cada vista tiene su propio enum `SortColumn` (no son intercambiables:
//! Explorer tiene `AddedAt`, Favorites/Playlists tienen `DefaultOrder`),
//! así que este módulo no fuerza un enum común — pide que cada vista
//! implemente el trait `SortableColumn` una vez (mapeo hacia/desde el
//! `usize` opaco que espera el widget `track_list`) y a cambio obtiene
//! gratis el estado y el ciclo de click.
//!
//! ## Cómo se consume
//!
//! ```ignore
//! // 1. El enum de columnas de la vista implementa el trait (una vez):
//! impl SortableColumn for SortColumn {
//!     const DEFAULT: Self = SortColumn::Title;
//!
//!     fn sort_key(self) -> usize { /* ...igual que antes... */ }
//!     fn from_sort_key(key: usize) -> Option<Self> { /* ...igual que antes... */ }
//! }
//!
//! // 2. Un campo en tu vista, en vez de sort_column + sort_direction + sort_click_stage:
//! sort: SortState<SortColumn>,
//!
//! // 3. En vez del match de 15 líneas en SortByKey:
//! SortByKey(key) => {
//!     if !self.sort.click(key) {
//!         return (Task::none(), OutMessage::Idle);
//!     }
//!     self.apply_sort(store);
//!     ...
//! }
//!
//! // 4. En vez del bloque `is_default_state` en TrackListConfig:
//! let config = TrackListConfig {
//!     active_sort_key: self.sort.active_sort_key(),
//!     sort_direction_asc: self.sort.is_asc(),
//!     ...
//! };
//!
//! // 5. En apply_sort, en vez de leer sort_column/sort_direction por
//! //    separado:
//! let column = self.sort.column();
//! let asc = self.sort.is_asc();
//! ```

/// Traducción hacia/desde el `usize` opaco que conoce el widget
/// `track_list` (ver comentario equivalente que existía en cada vista,
/// junto a las constantes `SORT_KEY_*`). `DEFAULT` reemplaza las
/// constantes sueltas `DEFAULT_SORT_COLUMN` de cada vista.
pub trait SortableColumn: Copy + PartialEq {
    const DEFAULT: Self;

    fn sort_key(self) -> usize;
    fn from_sort_key(key: usize) -> Option<Self>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    fn toggled(self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

/// Estado de ordenamiento de una tabla + el ciclo de click compartido.
/// El "estado default" es columna `C::DEFAULT`, dirección `Asc`, stage
/// `1` — igual que en cada vista antes de este refactor.
#[derive(Debug, Clone, Copy)]
pub struct SortState<C: SortableColumn> {
    column: C,
    direction: SortDirection,
    /// `1` = primer click en esta columna (o recién cambiada de
    /// columna), `>=2` = segundo click consecutivo en la misma columna
    /// → el próximo click vuelve al estado default.
    click_stage: u8,
}

impl<C: SortableColumn> Default for SortState<C> {
    fn default() -> Self {
        Self {
            column: C::DEFAULT,
            direction: SortDirection::Asc,
            click_stage: 1,
        }
    }
}

impl<C: SortableColumn> SortState<C> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn column(&self) -> C {
        self.column
    }

    pub fn is_asc(&self) -> bool {
        self.direction == SortDirection::Asc
    }

    /// `None` cuando estamos en el estado default (columna default +
    /// asc + stage 1) — así `track_list` no resalta ninguna columna
    /// como "activa", igual que el comportamiento original.
    pub fn active_sort_key(&self) -> Option<usize> {
        let is_default_state = self.column == C::DEFAULT
            && self.direction == SortDirection::Asc
            && self.click_stage == 1;
        (!is_default_state).then(|| self.column.sort_key())
    }

    /// Resetea al estado default (usado p. ej. al cambiar de playlist
    /// en `PlaylistsView::reset_detail_state`).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Aplica el click sobre la columna identificada por `key` (el
    /// `usize` opaco que emite `track_list::SortByKey`). Regresa
    /// `false` si `key` no resuelve a ninguna columna conocida — la
    /// vista debe ignorar el mensaje en ese caso, igual que antes.
    ///
    /// Ciclo: click en columna nueva → Asc, stage 1. Click de nuevo en
    /// la misma columna con stage 1 → toggla dirección, stage 2. Click
    /// de nuevo (stage >= 2) → vuelve al estado default.
    pub fn click(&mut self, key: usize) -> bool {
        let Some(column) = C::from_sort_key(key) else {
            return false;
        };

        if self.column == column {
            if self.click_stage >= 2 {
                self.column = C::DEFAULT;
                self.direction = SortDirection::Asc;
                self.click_stage = 1;
            } else {
                self.direction = self.direction.toggled();
                self.click_stage += 1;
            }
        } else {
            self.column = column;
            self.direction = SortDirection::Asc;
            self.click_stage = 1;
        }

        true
    }
}

/// Genera el `impl SortableColumn for $Enum` a partir de la lista
/// `Variante => CONSTANTE_USIZE`. Reemplaza el par de `match` (28
/// líneas) que estaba duplicado literalmente en `ExplorerView`,
/// `FavoritesView` y `PlaylistsView` — solo cambiaba qué variantes
/// tenía cada enum. La macro no fuerza un enum común (cada vista sigue
/// con su propio `SortColumn`, con o sin `AddedAt`/`DefaultOrder`); solo
/// evita reescribir a mano los dos `match` espejo, que es justo el
/// punto donde un desalineamiento manual (agregar una variante y
/// olvidar uno de los dos matches) no lo agarra el compilador salvo por
/// el `_ => None` de `from_sort_key`.
///
/// Uso:
/// ```ignore
/// impl_sortable_column! {
///     SortColumn, default = Title;
///     Title => SORT_KEY_TITLE,
///     Artist => SORT_KEY_ARTIST,
///     Album => SORT_KEY_ALBUM,
///     Bpm => SORT_KEY_BPM,
///     Key => SORT_KEY_KEY,
///     Duration => SORT_KEY_DURATION,
///     AddedAt => SORT_KEY_ADDED_AT,
/// }
/// ```
#[macro_export]
macro_rules! impl_sortable_column {
    ($Enum:ident, default = $default_variant:ident; $($variant:ident => $key:ident),+ $(,)?) => {
        impl $crate::ui::widgets::views::sort_state::SortableColumn for $Enum {
            const DEFAULT: Self = $Enum::$default_variant;

            fn sort_key(self) -> usize {
                match self {
                    $($Enum::$variant => $key,)+
                }
            }

            fn from_sort_key(key: usize) -> Option<Self> {
                match key {
                    $($key => Some($Enum::$variant),)+
                    _ => None,
                }
            }
        }
    };
}