//! # TrackListWidget — renderizado puro de una tabla virtualizada de tracks
//!
//! ## Por qué existe
//!
//! `ExplorerView` y `FavoritesView` son visualmente casi idénticas: mismo
//! layout de fila (índice, thumbnail, título, artista, álbum, duración,
//! bpm, key, [agregado]), mismo header clickeable con flecha de orden,
//! mismo scroll virtualizado, mismo overlay de menú contextual. La única
//! diferencia real es **qué** se muestra (todo el catálogo vs. la
//! playlist de likes), **cómo se ordena** (columnas distintas, una vista
//! tiene "AGREGADO" y la otra no, Favorites tiene "orden por defecto"
//! clickeable como columna "#"), y **qué acciones** ofrece el menú
//! contextual (Eliminar vs. Quitar de Me gusta).
//!
//! Este módulo NO decide nada de eso. Es deliberadamente un widget de
//! renderizado puro:
//!
//! - No tiene estado propio (no se guarda entre frames).
//! - No sabe qué es "buscar" ni "ordenar": recibe los tracks ya
//!   filtrados/ordenados como `&[&Track]`.
//! - No conoce el enum `SortColumn` de cada vista: cada columna del
//!   header es una `ColumnSpec` con un `sort_key: usize` opaco que la
//!   vista interpreta como quiera al recibir el mensaje de click.
//! - El menú contextual y el confirm dialog NO viven aquí: cada vista
//!   sigue siendo dueña de su `ContextMenu<String>`/`ConfirmDialog<Track>`
//!   (mismo patrón ya usado en `ExplorerView`/`FavoritesView`), y este
//!   widget solo los apila como overlay si la vista se los pasa ya
//!   armados (`Element` completo).
//!
//! ## Cómo se integra
//!
//! Cada vista sigue manejando: `search_query`, `filtered_indices`,
//! `sort_column`/`sort_direction`/`sort_click_stage`, `scroll`, `epoch`,
//! `context_menu`, `confirm_dialog`. Lo único que cambia es que en vez de
//! escribir su propio `render_table_header`/`render_row`/
//! `render_virtual_body`, arma un `TrackListConfig` y llama
//! `track_list(config)`.
//!
//! ```ignore
//! let config = TrackListConfig {
//!     columns: vec![
//!         Column::index(30.0),
//!         Column::thumbnail(THUMBNAIL_SIZE + 12.0, THUMBNAIL_SIZE),
//!         Column::sortable("TÍTULO", SORT_TITLE, Length::FillPortion(3)),
//!         Column::sortable("ARTISTA", SORT_ARTIST, Length::FillPortion(2)),
//!         Column::sortable("ÁLBUM", SORT_ALBUM, Length::FillPortion(2)),
//!         Column::sortable("DURACIÓN", SORT_DURATION, Length::Fixed(70.0)),
//!         Column::sortable("BPM", SORT_BPM, Length::Fixed(42.0)),
//!         Column::sortable("KEY", SORT_KEY, Length::Fixed(42.0)),
//!         Column::sortable("AGREGADO", SORT_ADDED_AT, Length::Fixed(80.0)),
//!     ],
//!     active_sort_key: Some(SORT_TITLE),
//!     sort_direction_asc: true,
//!     row_height: ROW_HEIGHT,
//!     buffer_rows: BUFFER_ROWS,
//! };
//!
//! track_list(
//!     config,
//!     &tracks,           // &[&Track], ya filtrado/ordenado por la vista
//!     &self.scroll,
//!     thumbnails,
//!     self.selected_track_id.as_deref(),
//!     scrollable_id,     // Id único por vista, p. ej. "explorer_catalog_scroll"
//!     TrackListCallbacks::new(
//!         ExplorerViewMessage::Scrolled,
//!         ExplorerViewMessage::PlayTrack,
//!         ExplorerViewMessage::SortBy, // recibe el sort_key: usize
//!         ExplorerViewMessage::ViewportMouseMoved,
//!         ExplorerViewMessage::RowRightClicked,
//!     ),
//!     overlay, // Option<Element<'a, Message>>: menú contextual / confirm dialog ya armados
//! )
//! ```

use chrono::{DateTime, Datelike, Utc};
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, stack, text, Id};
use iced::{Alignment, Color, Element, Font, Length, Padding};

use crate::model::Track;
use crate::ui::styles::styles::{minimal_button, selected_row_container, transparent_button};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::utils::virtual_list::ScrollTracker;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

/// Tipo de contenido de una columna del header/fila. Deliberadamente
/// pequeño: solo lo que Explorer/Favorites necesitan hoy. Agregar un
/// nuevo tipo de columna (p. ej. una columna de icono de "descargado")
/// es agregar una variante aquí, no bifurcar el widget.
#[derive(Clone)]
pub enum Column {
    /// Columna de índice (posición en la lista visible). No es
    /// ordenable por sí misma salvo que se le pase un `sort_key`
    /// (caso de Favorites: "#" == orden por defecto, clickeable).
    Index { width: f32, sort_key: Option<usize> },
    /// Hueco reservado para el thumbnail (no lleva texto de header).
    /// `width` es el ancho de la celda (columna); `size` es el tamaño
    /// real del thumbnail dentro de ella. Se separan porque Explorer
    /// usa `THUMBNAIL_SIZE + 12.0` de ancho de columna y Favorites usa
    /// `THUMBNAIL_SIZE + 2.0` — cada vista define su propio padding
    /// visual sin que el widget tenga que adivinarlo restando un offset
    /// mágico del ancho.
    Thumbnail { width: f32, size: f32 },
    /// Columna de texto con header clickeable para ordenar.
    Sortable { label: &'static str, sort_key: usize, width: Length },
}

impl Column {
    pub fn index(width: f32) -> Self {
        Column::Index { width, sort_key: None }
    }

    pub fn index_sortable(width: f32, sort_key: usize) -> Self {
        Column::Index { width, sort_key: Some(sort_key) }
    }

    pub fn thumbnail(width: f32, size: f32) -> Self {
        Column::Thumbnail { width, size }
    }

    pub fn sortable(label: &'static str, sort_key: usize, width: Length) -> Self {
        Column::Sortable { label, sort_key, width }
    }
}

/// Qué mostrar en cada celda de una fila, en el mismo orden que
/// `TrackListConfig::columns`. Cada vista arma este vector a partir de
/// su `Track` real; el widget solo lo distribuye en el layout.
///
/// Se separa de `Column` porque `Column` describe el HEADER (estático,
/// una vez) y `Cell` describe el CONTENIDO de una fila puntual
/// (dinámico, por track). Mantenerlos separados evita que el widget
/// tenga que re-derivar "¿cómo formateo esta columna para este track?"
/// — eso ya lo decidió la vista al llamar `row_cells(track, index)`.
pub enum Cell<'a, Message> {
    Index(usize),
    Thumbnail(Option<Handle>),
    Text(String),
    /// Texto con color propio (p. ej. la columna KEY en violeta, o
    /// título/artista atenuados cuando el track no está descargado).
    ColoredText(String, Color),
    /// Celda completamente custom, para casos que no encajan en texto
    /// simple (p. ej. un botón de like inline). Poco usado hoy, pero
    /// evita que el widget tenga que crecer una variante por cada
    /// capricho visual futuro.
    Custom(Element<'a, Message>),
}

pub struct TrackListConfig {
    pub columns: Vec<Column>,
    /// `sort_key` de la columna actualmente activa, o `None` si la
    /// lista está en su estado de orden por defecto (sin flecha visible
    /// en ningún header).
    pub active_sort_key: Option<usize>,
    pub sort_direction_asc: bool,
    pub row_height: f32,
    pub buffer_rows: usize,
}

/// Callbacks de la vista hacia el widget. Todos reciben tipos simples
/// (String/usize/Track/Point) y devuelven el `Message` de la vista —
/// mismo patrón que `ContextMenu::view` y `ConfirmDialog::view` ya usan
/// en este código base, así que el widget no necesita un enum de
/// mensajes propio ni la vista necesita convertir nada.
pub struct TrackListCallbacks<Message, FScroll, FPlay, FSort, FMove, FRightClick>
where
    FScroll: Fn(Viewport) -> Message,
    FPlay: Fn(Track) -> Message,
    FSort: Fn(usize) -> Message,
    FMove: Fn(iced::Point) -> Message,
    FRightClick: Fn(String) -> Message,
{
    pub on_scroll: FScroll,
    pub on_play: FPlay,
    pub on_sort: FSort,
    pub on_viewport_moved: FMove,
    pub on_row_right_click: FRightClick,
    _marker: std::marker::PhantomData<Message>,
}

impl<Message, FScroll, FPlay, FSort, FMove, FRightClick>
TrackListCallbacks<Message, FScroll, FPlay, FSort, FMove, FRightClick>
where
    FScroll: Fn(Viewport) -> Message,
    FPlay: Fn(Track) -> Message,
    FSort: Fn(usize) -> Message,
    FMove: Fn(iced::Point) -> Message,
    FRightClick: Fn(String) -> Message,
{
    /// Constructor que evita tener que rellenar `_marker` a mano en cada
    /// vista — el `PhantomData` es un detalle de implementación interno
    /// (necesario porque `Message` no aparece en ningún campo por valor,
    /// solo como tipo de retorno de las closures).
    pub fn new(
        on_scroll: FScroll,
        on_play: FPlay,
        on_sort: FSort,
        on_viewport_moved: FMove,
        on_row_right_click: FRightClick,
    ) -> Self {
        Self {
            on_scroll,
            on_play,
            on_sort,
            on_viewport_moved,
            on_row_right_click,
            _marker: std::marker::PhantomData,
        }
    }
}

/// Construye el header de la tabla: una celda por `Column`, con flecha
/// de orden si `sort_key` coincide con `active_sort_key`.
///
/// `on_sort` se toma POR VALOR y con bound `Clone` (no por referencia
/// prestada): la versión anterior pasaba `&'a FSort` apuntando a una
/// variable local de `track_list` (el resultado de desestructurar
/// `TrackListCallbacks`), y el `Element<'a, ...>` devuelto no puede
/// prestar de algo que muere al final de esa función (E0515 — "returns
/// a value referencing data owned by the current function"). Como acá
/// se necesita invocar `on_sort` una vez POR CADA columna ordenable
/// (potencialmente más de una), y los punteros a función/closures que
/// llegan aquí (p. ej. `ExplorerViewMessage::SortByKey`) son `Copy`,
/// pedir `Clone` es gratis en la práctica y evita el problema de vidas
/// por completo: cada celda se queda con su propia copia del callback.
fn render_header<'a, Message: Clone + 'a, FSort: Fn(usize) -> Message + Clone + 'a>(
    config: &TrackListConfig,
    on_sort: FSort,
) -> Element<'a, Message> {
    let mut cells: Vec<Element<'a, Message>> = Vec::with_capacity(config.columns.len());

    for col in &config.columns {
        let cell: Element<'a, Message> = match col {
            Column::Index { width, sort_key: None } => {
                container(text("#").font(JETBRAINS_MONO_HEADER).size(11).color(Color::from_rgb(0.45, 0.45, 0.5)))
                    .width(Length::Fixed(*width))
                    .into()
            }
            Column::Index { width, sort_key: Some(key) } => {
                render_sortable_cell("#", *key, Length::Fixed(*width), config, on_sort.clone())
            }
            Column::Thumbnail { width, .. } => {
                container(space()).width(Length::Fixed(*width)).into()
            }
            Column::Sortable { label, sort_key, width } => {
                render_sortable_cell(label, *sort_key, *width, config, on_sort.clone())
            }
        };
        cells.push(cell);
    }

    row(cells)
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(Padding { top: 4.0, bottom: 4.0, left: 10.0, right: 16.0 })
        .into()
}

fn render_sortable_cell<'a, Message: Clone + 'a, FSort: Fn(usize) -> Message + 'a>(
    label: &'static str,
    sort_key: usize,
    width: Length,
    config: &TrackListConfig,
    on_sort: FSort,
) -> Element<'a, Message> {
    let is_active = config.active_sort_key == Some(sort_key);
    let arrow = if is_active {
        if config.sort_direction_asc { " " } else { " " }
    } else {
        ""
    };

    let color = if is_active {
        Color::from_rgb(0.74, 0.58, 0.98)
    } else {
        Color::from_rgb(0.5, 0.5, 0.55)
    };

    button(
        text(format!("{}{}", label, arrow))
            .font(SF_PRO)
            .size(10.5)
            .style(move |_| text::Style { color: Some(color) }),
    )
        .style(minimal_button)
        .width(width)
        .padding(0)
        .on_press(on_sort(sort_key))
        .into()
}

/// Construye una fila completa a partir de las celdas ya resueltas por
/// la vista (`Cell`s). El widget solo decide anchos/espaciado/estilo,
/// nunca el contenido semántico.
fn render_row<'a, Message: Clone + 'a>(
    config: &TrackListConfig,
    cells: Vec<Cell<'a, Message>>,
    row_height: f32,
    is_selected: bool,
    track_id: String,
    on_play: Message,
    on_right_click_msg: Message,
) -> Element<'a, Message> {
    let mut row_children: Vec<Element<'a, Message>> = Vec::with_capacity(cells.len());

    for (col, cell) in config.columns.iter().zip(cells.into_iter()) {
        let width = match col {
            Column::Index { width, .. } => Length::Fixed(*width),
            Column::Thumbnail { width, .. } => Length::Fixed(*width),
            Column::Sortable { width, .. } => *width,
        };

        let element: Element<'a, Message> = match cell {
            Cell::Index(i) => container(
                text(i.to_string()).font(SF_PRO).size(12).color(Color::from_rgb(0.45, 0.45, 0.5)),
            )
                .width(width)
                .into(),
            Cell::Thumbnail(handle) => container(
                crate::ui::widgets::track_row::track_thumbnail_sized(handle, thumbnail_size_of(col)),
            )
                .width(width)
                .align_y(Alignment::Center)
                .into(),
            Cell::Text(s) => container(
                text(s).font(SF_PRO).size(13.5).color(Color::from_rgb(0.7, 0.7, 0.75)),
            )
                .width(width)
                .into(),
            Cell::ColoredText(s, c) => container(text(s).font(SF_PRO).size(13.5).color(c))
                .width(width)
                .into(),
            Cell::Custom(el) => container(el).width(width).into(),
        };

        row_children.push(element);
    }

    let row_content = row(row_children)
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 16.0 });

    let btn = button(row_content)
        .width(Length::Fill)
        .height(Length::Fixed(row_height))
        .style(transparent_button)
        .on_press(on_play);

    let styled_container = container(btn)
        .width(Length::Fill)
        .height(Length::Fixed(row_height))
        .align_y(Alignment::Center)
        .style(selected_row_container(is_selected));

    let _ = track_id; // el id ya viaja embebido en `on_right_click_msg`

    mouse_area(styled_container)
        .on_right_press(on_right_click_msg)
        .into()
}

// El tamaño real del thumbnail viaja explícito en `Column::Thumbnail.size`
// (ver comentario en la definición del enum): evita adivinar un offset
// de padding a partir del ancho de columna, que difiere entre vistas.
fn thumbnail_size_of(col: &Column) -> f32 {
    match col {
        Column::Thumbnail { size, .. } => *size,
        _ => 44.0,
    }
}

const JETBRAINS_MONO_HEADER: Font = Font::with_name("JetBrainsMono Nerd Font");

/// Renderiza la tabla completa (header fijo por fuera + scrollable
/// virtualizado con overlay opcional). Se usa desde el `view()` de cada
/// vista, después de decidir title/search bar/estados vacíos.
///
/// - `tracks`: ya filtrados/ordenados por la vista, uno por posición
///   visible (índice 0 = primera fila visible).
/// - `row_cells`: closure que, dado un track y su índice visible
///   (1-based, para la columna "#"), devuelve las `Cell` en el mismo
///   orden que `config.columns`. La vista decide formato (duración
///   mm:ss, fecha "9 jul 2026", color de KEY, etc.) — el widget no.
/// - `overlay`: `Element` ya armado por la vista (menú contextual y/o
///   confirm dialog apilados), o `None`. El widget solo lo apila encima
///   del scroll con `stack!`.
#[allow(clippy::too_many_arguments)]
pub fn track_list<'a, Message, FScroll, FPlay, FSort, FMove, FRightClick>(
    config: TrackListConfig,
    tracks: &[&'a Track],
    scroll: &ScrollTracker,
    thumbnails: &'a ThumbnailCache,
    selected_track_id: Option<&str>,
    scrollable_id: &'static str,
    callbacks: TrackListCallbacks<Message, FScroll, FPlay, FSort, FMove, FRightClick>,
    row_cells: impl Fn(&'a Track, usize) -> Vec<Cell<'a, Message>>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
    FScroll: Fn(Viewport) -> Message + 'a,
    FPlay: Fn(Track) -> Message + 'a,
// `Clone` porque `render_header` necesita llamar a `on_sort` una vez
// POR CADA columna ordenable, y no puede tomarlo prestado con vida
// `'a` sin producir un E0515 (ver comentario en `render_header`).
// En la práctica es gratis: los callbacks que llegan aquí son
// variantes de enum (`ExplorerViewMessage::SortByKey`), que son
// punteros a función y por lo tanto ya `Copy`/`Clone`.
    FSort: Fn(usize) -> Message + Clone + 'a,
    FMove: Fn(iced::Point) -> Message + 'a,
    FRightClick: Fn(String) -> Message + 'a,
{
    let window = scroll.window(config.row_height, tracks.len(), config.buffer_rows);
    let row_height = config.row_height;

    // Desestructuramos los callbacks ANTES del loop: son campos `Fn`
    // independientes tomados por valor, lo que evita mover `callbacks`
    // completo repetidamente dentro del loop (algunos se llaman una vez,
    // otros N veces por fila — todos son `Fn`, no `FnOnce`, así que esto
    // es válido).
    let TrackListCallbacks { on_scroll, on_play, on_sort, on_viewport_moved, on_row_right_click, .. } = callbacks;

    let header = render_header(&config, on_sort);

    let mut rows = column![].width(Length::Fill);
    rows = rows.push(space().height(window.top_spacer_height(row_height)));

    for visible_idx in window.start..window.end {
        if let Some(track) = tracks.get(visible_idx) {
            let track = *track;
            let handle = thumbnails.peek_for_render(track);
            let is_selected = selected_track_id == Some(track.id.as_str());

            // Sustituye la celda Thumbnail placeholder por el handle real:
            // la vista no conoce ThumbnailCache (eso es responsabilidad del
            // widget), así que arma sus celdas con `Cell::Thumbnail(None)`
            // y aquí se reemplaza por el valor cacheado real.
            let cells = row_cells(track, visible_idx + 1)
                .into_iter()
                .map(|c| match c {
                    Cell::Thumbnail(_) => Cell::Thumbnail(handle.clone()),
                    other => other,
                })
                .collect();

            let track_id = track.id.clone();
            let on_play_msg = on_play(track.clone());
            let on_right_click_msg = on_row_right_click(track_id.clone());

            rows = rows.push(render_row(
                &config,
                cells,
                row_height,
                is_selected,
                track_id,
                on_play_msg,
                on_right_click_msg,
            ));
        }
    }

    rows = rows.push(space().height(window.bottom_spacer_height(row_height, tracks.len())));

    let scroll_area: Element<'a, Message> = scrollable(rows)
        .id(Id::new(scrollable_id))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(move |v| on_scroll(v))
        .into();

    let scroll_area: Element<'a, Message> = mouse_area(scroll_area)
        .on_move(move |p| on_viewport_moved(p))
        .into();

    // IMPORTANTE: el árbol de widgets alrededor del `scrollable` debe
    // mantener SIEMPRE la misma forma (stack de 2 capas), sin importar
    // si hay overlay o no. Antes, cuando `overlay` era `None`, `body`
    // era directamente `scroll_area` (sin stack), y al abrir el menú
    // contextual pasaba a ser `stack![scroll_area, overlay]`: ese
    // cambio estructural del padre invalidaba el estado interno del
    // `scrollable` en el runtime de iced (aunque `ScrollTracker` seguía
    // con el offset correcto), produciendo el salto visual a la
    // posición 0 al hacer right-click. Usar `space()` como segunda capa
    // "vacía" cuando no hay overlay mantiene la forma del árbol
    // idéntica entre frames y elimina el reset visual.
    let overlay_layer: Element<'a, Message> = overlay.unwrap_or_else(|| space().into());
    let body: Element<'a, Message> = stack![scroll_area, overlay_layer].into();

    column![header, body].width(Length::Fill).height(Length::Fill).into()
}

/// Helper de formato reutilizado por ambas vistas para la columna
/// duración (mm:ss). Vive aquí porque es puramente de presentación.
pub fn format_duration(seconds: i32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{:02}:{:02}", mins, secs)
}

/// Helper de formato reutilizado por Explorer para "AGREGADO" (Favorites
/// no tiene esta columna). Vive aquí para no duplicar el arreglo de
/// meses en español en cada vista.
pub fn format_added_at(added_at: Option<DateTime<Utc>>) -> String {
    const MESES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun",
        "jul", "ago", "sep", "oct", "nov", "dic",
    ];
    match added_at {
        Some(dt) => format!("{} {} {}", dt.day(), MESES[dt.month0() as usize], dt.year()),
        None => "-".to_string(),
    }
}