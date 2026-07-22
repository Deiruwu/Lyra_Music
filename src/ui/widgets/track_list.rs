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
//!   sigue siendo dueña de su `ContextMenu<String>`/`ConfirmDialog<Vec<Track>>`
//!   (mismo patrón ya usado en `ExplorerView`/`FavoritesView`), y este
//!   widget solo los apila como overlay si la vista se los pasa ya
//!   armados (`Element` completo).
use std::collections::HashSet;
use chrono::{DateTime, Datelike, Utc};
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, stack, text, Id};
use iced::{Alignment, Color, Element, Font, Length, Padding};

use crate::model::Track;
use crate::ui::styles::styles::{minimal_button, selected_row_container, transparent_button, RowSelectionShape};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::utils::virtual_list::ScrollTracker;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");
const JETBRAINS_MONO_HEADER: Font = Font::with_name("JetBrainsMono Nerd Font");

#[derive(Clone)]
pub enum Column {
    Index { width: f32, sort_key: Option<usize> },
    Thumbnail { width: f32, size: f32 },
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

pub enum Cell<'a, Message> {
    Index(usize),
    Thumbnail(Option<Handle>),
    Text(String),
    ColoredText(String, Color),
    Custom(Element<'a, Message>),
}

pub struct TrackListConfig {
    pub columns: Vec<Column>,
    pub active_sort_key: Option<usize>,
    pub sort_direction_asc: bool,
    pub row_height: f32,
    pub buffer_rows: usize,
    /// Índice (dentro del vector ya reordenado visualmente que se le
    /// pasa a `track_list`) de la fila que se está arrastrando en este
    /// momento, si la hay. Esa fila se atenúa porque su representación
    /// "real" es el ghost que sigue al mouse.
    ///
    /// `Default::default()` es `None`, así que las vistas que no usan
    /// reordenamiento (Explorer, Favorites) no necesitan tocar este
    /// campo: `TrackListConfig { columns, ..., ..Default::default() }`
    /// o simplemente inicializándolo en `None` explícitamente.
    pub dragging_row_index: Option<usize>,
}

impl Default for TrackListConfig {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            active_sort_key: None,
            sort_direction_asc: true,
            row_height: 60.0,
            buffer_rows: 15,
            dragging_row_index: None,
        }
    }
}

pub struct TrackListCallbacks<Message, FScroll, FClick, FSort, FMove, FRightClick>
where
    FScroll: Fn(Viewport) -> Message,
    FClick: Fn(Track, usize) -> Message,
    FSort: Fn(usize) -> Message,
    FMove: Fn(iced::Point) -> Message,
    FRightClick: Fn(String) -> Message,
{
    pub on_scroll: FScroll,
    pub on_click: FClick,
    pub on_sort: FSort,
    pub on_viewport_moved: FMove,
    pub on_row_right_click: FRightClick,
    pub on_viewport_exited: Option<Message>, // <--- Nuevo para limpiar tracking global
    _marker: std::marker::PhantomData<Message>,
}

impl<Message, FScroll, FClick, FSort, FMove, FRightClick>
TrackListCallbacks<Message, FScroll, FClick, FSort, FMove, FRightClick>
where
    FScroll: Fn(Viewport) -> Message,
    FClick: Fn(Track, usize) -> Message,
    FSort: Fn(usize) -> Message,
    FMove: Fn(iced::Point) -> Message,
    FRightClick: Fn(String) -> Message,
{
    pub fn new(
        on_scroll: FScroll,
        on_click: FClick,
        on_sort: FSort,
        on_viewport_moved: FMove,
        on_row_right_click: FRightClick,
    ) -> Self {
        Self {
            on_scroll,
            on_click,
            on_sort,
            on_viewport_moved,
            on_row_right_click,
            on_viewport_exited: None,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn with_exit(mut self, on_exit: Message) -> Self {
        self.on_viewport_exited = Some(on_exit);
        self
    }
}

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

fn render_row<'a, Message: Clone + 'a>(
    config: &TrackListConfig,
    cells: Vec<Cell<'a, Message>>,
    row_height: f32,
    shape: RowSelectionShape,
    track_id: String,
    on_click: Message,
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
        .on_press(on_click);

    let styled_container = container(btn)
        .width(Length::Fill)
        .height(Length::Fixed(row_height))
        .align_y(Alignment::Center)
        .style(selected_row_container(shape));

    let _ = track_id;

    mouse_area(styled_container)
        .on_right_press(on_right_click_msg)
        .into()
}

fn thumbnail_size_of(col: &Column) -> f32 {
    match col {
        Column::Thumbnail { size, .. } => *size,
        _ => 44.0,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn track_list<'a, Message, FScroll, FClick, FSort, FMove, FRightClick>(
    config: TrackListConfig,
    tracks: &[&'a Track],
    scroll: &ScrollTracker,
    thumbnails: &'a ThumbnailCache,
    selected_ids: &'a HashSet<String>,
    scrollable_id: &'static str,
    callbacks: TrackListCallbacks<Message, FScroll, FClick, FSort, FMove, FRightClick>,
    row_cells: impl Fn(&'a Track, usize) -> Vec<Cell<'a, Message>>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
    FScroll: Fn(Viewport) -> Message + 'a,
    FClick: Fn(Track, usize) -> Message + 'a,
    FSort: Fn(usize) -> Message + Clone + 'a,
    FMove: Fn(iced::Point) -> Message + 'a,
    FRightClick: Fn(String) -> Message + 'a,
{
    let window = scroll.window(config.row_height, tracks.len(), config.buffer_rows);
    let row_height = config.row_height;

    let on_scroll = callbacks.on_scroll;
    let on_click = callbacks.on_click;
    let on_sort = callbacks.on_sort;
    let on_viewport_moved = callbacks.on_viewport_moved;
    let on_row_right_click = callbacks.on_row_right_click;
    let on_viewport_exited = callbacks.on_viewport_exited;

    let header = render_header(&config, on_sort);

    let mut rows = column![].width(Length::Fill);
    rows = rows.push(space().height(window.top_spacer_height(row_height)));

    for visible_idx in window.start..window.end {
        if let Some(track) = tracks.get(visible_idx) {
            let track = *track;
            let handle = thumbnails.peek_for_render(track);

            let is_selected = selected_ids.contains(track.id.as_str());
            let prev_selected = visible_idx > 0
                && tracks.get(visible_idx - 1).is_some_and(|t| selected_ids.contains(t.id.as_str()));
            let next_selected = tracks.get(visible_idx + 1).is_some_and(|t| selected_ids.contains(t.id.as_str()));
            let shape = RowSelectionShape::from_neighbors(is_selected, prev_selected, next_selected);

            let cells = row_cells(track, visible_idx + 1)
                .into_iter()
                .map(|c| match c {
                    Cell::Thumbnail(_) => Cell::Thumbnail(handle.clone()),
                    other => other,
                })
                .collect();

            let track_id = track.id.clone();
            let on_click_msg = on_click(track.clone(), visible_idx);
            let on_right_click_msg = on_row_right_click(track_id.clone());
            let is_dragging_this_row = config.dragging_row_index == Some(visible_idx);

            let rendered_row: Element<'a, Message> = if is_dragging_this_row {
                container(space().height(Length::Fixed(row_height)))
                    .width(Length::Fill)
                    .height(Length::Fixed(row_height))
                    .into()
            } else {
                render_row(
                    &config,
                    cells,
                    row_height,
                    shape,
                    track_id,
                    on_click_msg,
                    on_right_click_msg,
                )
            };

            rows = rows.push(rendered_row);
        }
    }

    rows = rows.push(space().height(window.bottom_spacer_height(row_height, tracks.len())));

    let scroll_area: Element<'a, Message> = scrollable(rows)
        .id(Id::new(scrollable_id))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(move |v| on_scroll(v))
        .into();

    let mut area = mouse_area(scroll_area)
        .on_move(move |p| on_viewport_moved(p));

    if let Some(exit_msg) = on_viewport_exited {
        area = area.on_exit(exit_msg);
    }

    let scroll_area: Element<'a, Message> = area.into();

    let overlay_layer: Element<'a, Message> = overlay.unwrap_or_else(|| space().into());
    let body: Element<'a, Message> = stack![scroll_area, overlay_layer].into();

    column![header, body].width(Length::Fill).height(Length::Fill).into()
}

pub fn format_duration(seconds: i32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{:02}:{:02}", mins, secs)
}

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