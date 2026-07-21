//! # Field — especificación única de "columna" para el catálogo de tracks
//!
//! ## Por qué existe
//!
//! `ExplorerView`, `FavoritesView` y `PlaylistsView` cada una mantiene
//! DOS listas que deben coincidir en orden y longitud: `columns()` (el
//! header — label, sort_key, ancho) y `row_cells()` (el contenido por
//! track). Hoy están sincronizadas a mano: agregar una columna nueva
//! (p. ej. "agregado", "bpm", "key") significa tocar ambas listas en
//! cada vista que la quiera, y es fácil que se desincronicen en
//! longitud/orden sin que el compilador avise (`track_list::render_row`
//! hace `zip`, así que un desfase simplemente descarta celdas en
//! silencio en vez de dar un error).
//!
//! Este módulo colapsa las dos listas en una: un `Field` describe AMBAS
//! cosas a la vez (cómo se ve su header + cómo se extrae su celda de un
//! `Track`). Las vistas arman su tabla con una sola llamada encadenada:
//!
//! ```ignore
//! use crate::ui::widgets::track_fields::Field;
//!
//! let fields = Field::index(30.0)
//!     .thumbnail(THUMBNAIL_SIZE + 12.0, THUMBNAIL_SIZE)
//!     .title(SORT_KEY_TITLE)
//!     .artist(SORT_KEY_ARTIST)
//!     .album(SORT_KEY_ALBUM)
//!     .duration(SORT_KEY_DURATION)
//!     .bpm(SORT_KEY_BPM)
//!     .camelot_key(SORT_KEY_KEY)
//!     .added_at(SORT_KEY_ADDED_AT);
//!
//! // en vez de Self::columns() + Self::row_cells():
//! let config = TrackListConfig { columns: fields.columns(), ...};
//! track_list(config, &tracks, ..., |track, idx| fields.row_cells(track, idx), overlay)
//! ```
//!
//! Favorites (sin AGREGADO) simplemente no encadena `.added_at(...)`;
//! Playlists (sin BPM/KEY) no encadena esas dos. Cada vista sigue
//! decidiendo QUÉ columnas quiere y en QUÉ orden, pero ya no mantiene
//! dos listas — una sola fuente de verdad por columna.
//!
//! ## Extensibilidad
//!
//! Los campos "de catálogo" (title, artist, album, duration, bpm,
//! camelot_key, added_at) son atajos con formato ya resuelto (mismo
//! `format_duration`/`format_added_at` que antes). Para algo que no
//! encaje ahí — un campo completamente custom, o una columna con lógica
//! de formato específica de una sola vista — `Field::custom` acepta
//! cualquier closure `Fn(&Track) -> Cell` sin tener que crecer este
//! enum por cada capricho de una vista puntual.

use iced::{Color, Length};

use crate::model::Track;
use crate::ui::widgets::track_list::{format_added_at, format_duration, Cell, Column};

/// Un campo = un header (`Column`) + una forma de extraer su `Cell` de
/// un `Track`. `extract` es un `Box<dyn Fn>` (no genérico por campo)
/// para poder guardar una `Vec<Field<Message>>` homogénea — el costo de
/// la indirección es irrelevante frente al de reconstruir la vista
/// completa cada frame.
pub struct Field<'a, Message> {
    column: Column,
    extract: Box<dyn Fn(&Track) -> Cell<'a, Message> + 'a>,
}

/// Punto de entrada. Cada método `Field::xxx(...)` crea la lista con un
/// primer campo; los siguientes se encadenan con los métodos de
/// instancia de abajo. Se separa de `FieldList::new()` vacío porque en
/// la práctica toda tabla arranca con `index` o `index_sortable`, y
/// encadenar desde ahí lee mejor que `FieldList::new().index(...)`.
impl<'a, Message: 'a> Field<'a, Message> {
    pub fn index(width: f32) -> FieldList<'a, Message> {
        FieldList(vec![Field {
            column: Column::index(width),
            extract: Box::new(|_track| Cell::Index(0)), // el índice real lo inyecta track_list en tiempo de render; ver FieldList::row_cells
        }])
    }

    pub fn index_sortable(width: f32, sort_key: usize) -> FieldList<'a, Message> {
        FieldList(vec![Field {
            column: Column::index_sortable(width, sort_key),
            extract: Box::new(|_track| Cell::Index(0)),
        }])
    }
}

/// Lista encadenable de `Field`s. Cada método de instancia agrega un
/// campo y devuelve `self` para seguir encadenando — es la
/// "concatenación de llamadas" que arma la tabla completa en una sola
/// expresión.
pub struct FieldList<'a, Message>(Vec<Field<'a, Message>>);

impl<'a, Message: 'a> FieldList<'a, Message> {
    pub fn thumbnail(mut self, width: f32, size: f32) -> Self {
        self.0.push(Field {
            column: Column::thumbnail(width, size),
            // El widget reemplaza este placeholder con el Handle real
            // cacheado (ver `track_list::track_list`, match sobre
            // `Cell::Thumbnail(_)`) — igual que hacían las vistas antes.
            extract: Box::new(|_track| Cell::Thumbnail(None)),
        });
        self
    }

    pub fn title(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("TÍTULO", sort_key, Length::FillPortion(3)),
            extract: Box::new(|track| Cell::Text(track.title.clone())),
        });
        self
    }

    pub fn artist(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("ARTISTA", sort_key, Length::FillPortion(2)),
            extract: Box::new(|track| Cell::Text(track.format_artists())),
        });
        self
    }

    pub fn album(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("ÁLBUM", sort_key, Length::FillPortion(2)),
            extract: Box::new(|track| {
                Cell::Text(track.album.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "-".to_string()))
            }),
        });
        self
    }

    pub fn duration(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("DURACIÓN", sort_key, Length::Fixed(70.0)),
            extract: Box::new(|track| Cell::Text(format_duration(track.duration_seconds))),
        });
        self
    }

    pub fn bpm(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("BPM", sort_key, Length::Fixed(42.0)),
            extract: Box::new(|track| Cell::Text(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".to_string()))),
        });
        self
    }

    pub fn camelot_key(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("KEY", sort_key, Length::Fixed(42.0)),
            extract: Box::new(|track| {
                Cell::ColoredText(
                    track.camelot_key.clone().unwrap_or_else(|| "-".to_string()),
                    Color::from_rgb(0.74, 0.58, 0.98),
                )
            }),
        });
        self
    }

    pub fn added_at(mut self, sort_key: usize) -> Self {
        self.0.push(Field {
            column: Column::sortable("AGREGADO", sort_key, Length::Fixed(80.0)),
            extract: Box::new(|track| Cell::Text(format_added_at(track.added_at))),
        });
        self
    }

    /// Escape hatch para columnas que no encajan en los atajos de
    /// arriba (formato específico de una vista, un campo nuevo del
    /// modelo que aún no tiene atajo aquí, etc.) sin tener que crecer
    /// este builder por cada caso puntual.
    pub fn custom(
        mut self,
        column: Column,
        extract: impl Fn(&Track) -> Cell<'a, Message> + 'a,
    ) -> Self {
        self.0.push(Field { column, extract: Box::new(extract) });
        self
    }

    /// Extrae la lista de `Column` para `TrackListConfig::columns`.
    pub fn columns(&self) -> Vec<Column> {
        self.0.iter().map(|f| f.column.clone()).collect()
    }

    /// Construye las `Cell` de una fila en el mismo orden que
    /// `columns()`. Firma calzada con el parámetro `row_cells` que
    /// espera `track_list(...)` (`impl Fn(&'a Track, usize) -> Vec<Cell<'a, Message>>`),
    /// así que se pasa directo: `|track, idx| fields.row_cells(track, idx)`.
    ///
    /// El primer campo de índice recibe `idx` (1-based, igual que antes)
    /// en vez del placeholder `Cell::Index(0)` de su `extract` — el
    /// índice de fila es el único dato que no sale del `Track`, así que
    /// se resuelve aquí en vez de en el closure de cada campo.
    pub fn row_cells(&self, track: &'a Track, index: usize) -> Vec<Cell<'a, Message>> {
        self.0
            .iter()
            .map(|f| match &f.column {
                Column::Index { .. } => Cell::Index(index),
                _ => (f.extract)(track),
            })
            .collect()
    }
}