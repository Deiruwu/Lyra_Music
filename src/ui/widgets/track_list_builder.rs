use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, stack, text, Id};
use iced::{Alignment, Color, Element, Length, Padding, Point};
use strum_macros::AsRefStr;
use crate::model::{Album, Artist, Track};
use crate::ui::assets::fonts::{JETBRAINS_MONO, SF_PRO};
use crate::ui::styles::styles::{minimal_button, selected_row_container, transparent_button, RowSelectionShape};
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::widgets::artist_links::{album_link, artist_links, artist_names_text};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail_sized;
use crate::utils::formatting::{format_added_at, format_duration};

const THUMBNAIL_COL_WIDTH: f32 = 56.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const INDEX_COL_WIDTH: f32 = 40.0;
const DEFAULT_ROW_HEIGHT: f32 = 60.0;
const DEFAULT_BUFFER_ROWS: usize = 15;

#[derive(Debug, Clone)]
pub enum TrackEvent {
    Clicked(Track, usize),
    Scrolled(Viewport),
    Sorted(usize),
    MouseMoved(Point),
    RightClicked(String),
    ViewportExited,
    ArtistClicked(String),
    AlbumClicked(String),
}

// ── La fila de fábrica ────────────────────────────────────────────

/// Único vocabulario de "columna ordenable" en toda la app. Cada
/// variante trae su sort_key FIJO — ninguna vista vuelve a declarar
/// sus propios `SORT_KEY_*`. `Index` cubre tanto la columna visual
/// "#" como el estado "sin ordenar, tal cual llega" (orden por
/// defecto de listas tipo Favoritos/Playlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr)]
#[repr(usize)]
pub enum TrackColumn {
    #[strum(serialize = "#")]
    Index = 0,
    #[strum(serialize = "TÍTULO")]
    Title = 1,
    #[strum(serialize = "ARTISTA")]
    Artist = 2,
    #[strum(serialize = "ÁLBUM")]
    Album = 3,
    #[strum(serialize = "DUR.")]
    Duration = 4,
    #[strum(serialize = "BPM")]
    Bpm = 5,
    #[strum(serialize = "KEY")]
    Key = 6,
    #[strum(serialize = "AGREGADO")]
    AddedAt = 7,
}

impl TrackColumn {
    pub const fn as_usize(self) -> usize {
        self as usize
    }
    fn width(self) -> Length {
        match self {
            TrackColumn::Index => Length::Fixed(INDEX_COL_WIDTH),
            TrackColumn::Title => Length::FillPortion(3),
            TrackColumn::Artist => Length::FillPortion(2),
            TrackColumn::Album => Length::FillPortion(2),
            TrackColumn::Duration => Length::Fixed(70.0),
            TrackColumn::Bpm => Length::Fixed(60.0),
            TrackColumn::Key => Length::Fixed(60.0),
            TrackColumn::AddedAt => Length::Fixed(100.0),
        }
    }

    fn display_value(self, track: &Track, display_index: usize) -> DisplayValue {
        match self {
            TrackColumn::Index => DisplayValue::Index(display_index),
            TrackColumn::Title => DisplayValue::Text(track.title.clone()),
            TrackColumn::Artist => DisplayValue::Artists(track.artists.clone()),
            TrackColumn::Album => DisplayValue::AlbumLink(track.album.clone()),
            TrackColumn::Duration => DisplayValue::Text(format_duration(track.duration_seconds)),
            TrackColumn::Bpm => DisplayValue::Text(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".to_string())),
            TrackColumn::Key => DisplayValue::ColoredText(
                track.camelot_key.clone().unwrap_or_else(|| "-".to_string()),
                Color::from_rgb(0.74, 0.58, 0.98),
            ),
            TrackColumn::AddedAt => DisplayValue::Text(format_added_at(track.added_at)),
        }
    }
}

enum DisplayValue {
    Index(usize),
    Thumbnail(Option<Handle>),
    Text(String),
    ColoredText(String, Color),
    Artists(Vec<Artist>),
    AlbumLink(Option<Album>),
}

fn active_columns(show_added_at: bool) -> Vec<TrackColumn> {
    let mut columns = vec![
        TrackColumn::Title,
        TrackColumn::Artist,
        TrackColumn::Album,
        TrackColumn::Duration,
        TrackColumn::Bpm,
        TrackColumn::Key,
    ];
    if show_added_at {
        columns.push(TrackColumn::AddedAt);
    }
    columns
}

/// Clave de orden para un camelot key tipo "8A"/"12B": número ascendente
/// primero, A antes que B para el mismo número. `cmp` sobre el string crudo
/// ordena lexicográficamente ("10A" < "2B"), que es numéricamente incorrecto.
fn camelot_key_order(key: &str) -> (u32, char) {
    let letter = key.chars().last().unwrap_or_default();
    let number: u32 = key.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0);
    (number, letter)
}

/// Ordena `tracks` in-place según `sort_key` (índice de TrackColumn) y
/// dirección. `sort_key == Index` (0) o `None` deja el orden tal cual llega
/// — es el caso "sin ordenar" de Favoritos/Playlist.
pub fn sort_tracks(tracks: &mut [&Track], sort_key: Option<usize>, ascending: bool) {
    let Some(key) = sort_key else { return };
    if key == TrackColumn::Index.as_usize() {
        return;
    }

    tracks.sort_by(|a, b| {
        let ordering = match key {
            k if k == TrackColumn::Title.as_usize() => a.title.cmp(&b.title),
            k if k == TrackColumn::Artist.as_usize() => a.format_artists().cmp(&b.format_artists()),
            k if k == TrackColumn::Album.as_usize() => {
                let album_a = a.album.as_ref().map(|al| al.name.as_str()).unwrap_or("");
                let album_b = b.album.as_ref().map(|al| al.name.as_str()).unwrap_or("");
                album_a.cmp(album_b)
            }
            k if k == TrackColumn::Duration.as_usize() => a.duration_seconds.cmp(&b.duration_seconds),
            k if k == TrackColumn::Bpm.as_usize() => a.bpm.cmp(&b.bpm),
            k if k == TrackColumn::Key.as_usize() => match (&a.camelot_key, &b.camelot_key) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(ka), Some(kb)) => camelot_key_order(ka).cmp(&camelot_key_order(kb)),
            },
            k if k == TrackColumn::AddedAt.as_usize() => a.added_at.cmp(&b.added_at),
            _ => std::cmp::Ordering::Equal,
        };
        if ascending { ordering } else { ordering.reverse() }
    });
}

/// Dado el ScrollTracker de una vista y los tracks renderizados AHORA,
/// calcula el universo `(key, url)` de la ventana visible (+buffer) que
/// AsyncThumbnail::sync() debe mantener vivo. Reemplaza tanto
/// `pending_thumbnail_requests` como `visible_track_keys` del sistema
/// anterior — sync() ya decide internamente qué falta pedir y qué podar,
/// así que solo hace falta darle el universo deseado completo.
pub fn visible_thumbnail_targets(
    scroll: &ScrollTracker,
    tracks: &[&Track],
    row_height: f32,
    buffer_rows: usize,
) -> Vec<(String, String)> {
    let window = scroll.window(row_height, tracks.len(), buffer_rows);
    let visible = &tracks[window.start..window.end.min(tracks.len())];

    visible
        .iter()
        .filter_map(|t| t.thumbnail_small.clone().map(|url| (thumb_key(t), url)))
        .collect()
}

// ── Drag visual (solo pintar; el estado vive en la vista) ─────────

struct DragVisual {
    hole_index: usize,
    mouse_position: Option<Point>,
    grab_offset: f32,
}

// ── Builder ─────────────────────────────────────────────────────

pub struct TrackBuilder<'a, Message> {
    tracks: Vec<&'a Track>,
    scroll: &'a ScrollTracker,
    thumbnails: &'a AsyncThumbnail,
    selected_ids: &'a HashSet<String>,
    scrollable_id: &'static str,

    row_height: f32,
    buffer_rows: usize,

    show_added_at: bool,
    index_sortable: bool,

    active_sort_key: Option<usize>,
    sort_direction_asc: bool,

    drag: Option<DragVisual>,
    animator: Option<&'a RowAnimator>,

    on_event: Option<Rc<dyn Fn(TrackEvent) -> Message + 'a>>,

    overlay: Option<Element<'a, Message>>,
}

impl<'a, Message> TrackBuilder<'a, Message>
where
    Message: Clone + 'a,
{
    pub fn new(
        tracks: Vec<&'a Track>,
        scroll: &'a ScrollTracker,
        thumbnails: &'a AsyncThumbnail,
        selected_ids: &'a HashSet<String>,
        scrollable_id: &'static str,
    ) -> Self {
        Self {
            tracks,
            scroll,
            thumbnails,
            selected_ids,
            scrollable_id,
            row_height: DEFAULT_ROW_HEIGHT,
            buffer_rows: DEFAULT_BUFFER_ROWS,
            show_added_at: false,
            index_sortable: false,
            active_sort_key: Some(TrackColumn::Index.as_usize()),
            sort_direction_asc: true,
            drag: None,
            animator: None,
            on_event: None,
            overlay: None,
        }
    }

    // ── Diffs sobre la fila base ─────────────────────────────────

    /// Agrega la columna "AGREGADO" (fecha) al final de la fila.
    pub fn with_added_at(mut self) -> Self {
        self.show_added_at = true;
        self
    }

    /// Vuelve el índice ("#") clickeable/ordenable en vez de decorativo.
    pub fn index_sortable(mut self) -> Self {
        self.index_sortable = true;
        self
    }

    /// Fija el estado de orden ACTUAL (columna activa + dirección).
    /// Si no se llama, ordena por índice (sin reordenar).
    pub fn sort(mut self, active_key: Option<usize>, asc: bool) -> Self {
        self.active_sort_key = active_key;
        self.sort_direction_asc = asc;
        self
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height;
        self
    }

    pub fn buffer_rows(mut self, rows: usize) -> Self {
        self.buffer_rows = rows;
        self
    }

    /// Activa el pintado del ghost row de reordenamiento.
    /// `hole_index` es la fila (dentro del vector visual ya
    /// reordenado que se pasó al builder) que se está arrastrando.
    /// Puramente visual — la vista decide CUÁNDO llamar esto.
    pub fn dragging(mut self, hole_index: usize, mouse_position: Option<Point>, grab_offset: f32) -> Self {
        self.drag = Some(DragVisual { hole_index, mouse_position, grab_offset });
        self
    }

    /// Activa el renderizado animado/absoluto (filas deslizan a su nueva
    /// posición en vez de saltar de golpe), igual que `QueuePanel`. Solo
    /// tiene sentido junto con `.dragging(...)` — sin drag activo no hay
    /// nada que animar, así que las vistas sin reordenamiento (Explorer,
    /// Favorites) nunca la llaman y siguen con el `column!` simple.
    pub fn animator(mut self, animator: &'a RowAnimator) -> Self {
        self.animator = Some(animator);
        self
    }

    pub fn on_event(mut self, f: impl Fn(TrackEvent) -> Message + 'a) -> Self {
        self.on_event = Some(Rc::new(f));
        self
    }

    pub fn overlay(mut self, overlay: Option<Element<'a, Message>>) -> Self {
        self.overlay = overlay;
        self
    }

    // ── Render: header ────────────────────────────────────────────

    fn render_header(&self, fields: &[TrackColumn], emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>) -> Element<'a, Message> {
        let mut cells: Vec<Element<'a, Message>> = Vec::with_capacity(fields.len() + 2);

        cells.push(if self.index_sortable {
            self.sortable_header_cell(TrackColumn::Index, emit)
        } else {
            container(text("#").font(JETBRAINS_MONO).size(11).color(Color::from_rgb(0.45, 0.45, 0.5)))
                .width(Length::Fixed(INDEX_COL_WIDTH))
                .into()
        });

        cells.push(container(space()).width(Length::Fixed(THUMBNAIL_COL_WIDTH)).into());

        for &field in fields {
            cells.push(self.sortable_header_cell(field, emit));
        }

        row(cells)
            .spacing(10)
            .align_y(Alignment::Center)
            .padding(Padding { top: 4.0, bottom: 4.0, left: 10.0, right: 16.0 })
            .into()
    }

    fn sortable_header_cell(&self, field: TrackColumn, emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>) -> Element<'a, Message> {
        let sort_key = field.as_usize();
        let is_active = self.active_sort_key == Some(sort_key);
        let arrow = if is_active {
            if self.sort_direction_asc { " ↑" } else { " ↓" }
        } else {
            ""
        };
        let color = if is_active {
            Color::from_rgb(0.74, 0.58, 0.98)
        } else {
            Color::from_rgb(0.5, 0.5, 0.55)
        };

        button(
            text(format!("{}{}", field.as_ref(), arrow))
                .font(SF_PRO)
                .size(10.5)
                .style(move |_| text::Style { color: Some(color) }),
        )
            .style(minimal_button)
            .width(field.width())
            .padding(0)
            .on_press(emit(TrackEvent::Sorted(sort_key)))
            .into()
    }
    // Nota: emit: &Rc<dyn Fn(...)->Message> se puede llamar como emit(...)
    // directo — Rc<dyn Fn> implementa Fn vía auto-deref, no hace falta
    // (*emit)(...) ni emit.as_ref()(...).

    // ── Render: celda → Element ──────────────────────────────────

    fn render_cell(
        &self,
        cell: DisplayValue,
        width: Length,
        emit: Option<&Rc<dyn Fn(TrackEvent) -> Message + 'a>>,
    ) -> Element<'a, Message> {
        match cell {
            DisplayValue::Index(i) => container(
                text(i.to_string()).font(SF_PRO).size(12).color(Color::from_rgb(0.45, 0.45, 0.5)),
            )
                .width(width)
                .into(),
            DisplayValue::Thumbnail(handle) => container(track_thumbnail_sized(handle, THUMBNAIL_SIZE))
                .width(width)
                .align_y(Alignment::Center)
                .into(),
            DisplayValue::Text(s) => single_line_text(s, SF_PRO, 13.5, Color::from_rgb(0.7, 0.7, 0.75), width),
            DisplayValue::ColoredText(s, c) => single_line_text(s, SF_PRO, 13.5, c, width),
            DisplayValue::Artists(artists) => match emit {
                Some(emit) => {
                    let emit = Rc::clone(emit);
                    artist_links(&artists, SF_PRO, 13.5, Color::from_rgb(0.7, 0.7, 0.75), width, move |id| {
                        emit(TrackEvent::ArtistClicked(id))
                    })
                }
                None => artist_names_text(&artists, SF_PRO, 13.5, Color::from_rgb(0.7, 0.7, 0.75), width),
            },
            DisplayValue::AlbumLink(album) => match emit {
                Some(emit) => {
                    let emit = Rc::clone(emit);
                    album_link(album.as_ref(), SF_PRO, 13.5, Color::from_rgb(0.7, 0.7, 0.75), width, move |id| {
                        emit(TrackEvent::AlbumClicked(id))
                    })
                }
                None => {
                    let name = album.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "-".to_string());
                    single_line_text(name, SF_PRO, 13.5, Color::from_rgb(0.7, 0.7, 0.75), width)
                }
            },
        }
    }

    fn render_row(
        &self,
        fields: &[TrackColumn],
        track: &'a Track,
        display_index: usize,
        handle: Option<Handle>,
        shape: RowSelectionShape,
        emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>,
    ) -> Element<'a, Message> {
        let mut row_children: Vec<Element<'a, Message>> = Vec::with_capacity(fields.len() + 2);

        row_children.push(self.render_cell(DisplayValue::Index(display_index), Length::Fixed(INDEX_COL_WIDTH), None));
        row_children.push(self.render_cell(DisplayValue::Thumbnail(handle), Length::Fixed(THUMBNAIL_COL_WIDTH), None));

        for &field in fields {
            row_children.push(self.render_cell(field.display_value(track, display_index), field.width(), Some(emit)));
        }

        let row_content = row(row_children)
            .spacing(16)
            .align_y(Alignment::Center)
            .padding(Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 16.0 });

        // display_index es 1-based (para mostrar "#1, #2..."); el índice
        // real dentro del vector visible es display_index - 1.
        let visible_idx = display_index - 1;
        let click_track = track.clone();
        let right_click_id = track.id.clone();

        let btn = button(row_content)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .style(transparent_button)
            .on_press(emit(TrackEvent::Clicked(click_track, visible_idx)));

        let styled = container(btn)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .align_y(Alignment::Center)
            .style(selected_row_container(shape));

        mouse_area(styled)
            .on_right_press(emit(TrackEvent::RightClicked(right_click_id)))
            .into()
    }

    /// Resuelve thumbnail/selección/vecinos y arma la fila en `visible_idx`
    /// — compartido entre el layout `column!` (Explorer/Favorites/Playlist
    /// sin drag) y el `stack!` animado (Playlist arrastrando), que solo
    /// difieren en CÓMO envuelven este elemento, no en cómo se arma.
    fn render_visible_row(
        &self,
        fields: &[TrackColumn],
        emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>,
        visible_idx: usize,
    ) -> Option<Element<'a, Message>> {
        let track = self.tracks.get(visible_idx).copied()?;
        let handle = self.thumbnails.get(&thumb_key(track)).cloned();
        let is_selected = self.selected_ids.contains(track.id.as_str());
        let prev_selected = visible_idx > 0
            && self.tracks.get(visible_idx - 1).is_some_and(|t| self.selected_ids.contains(t.id.as_str()));
        let next_selected = self.tracks.get(visible_idx + 1).is_some_and(|t| self.selected_ids.contains(t.id.as_str()));
        let shape = RowSelectionShape::from_neighbors(is_selected, prev_selected, next_selected);

        let display_index = visible_idx + 1;
        Some(self.render_row(fields, track, display_index, handle, shape, emit))
    }

    // ── Ghost row (drag) ──────────────────────────────────────────

    fn ghost_overlay(&self, fields: &[TrackColumn]) -> Option<Element<'a, Message>> {
        let drag = self.drag.as_ref()?;
        let track = *self.tracks.get(drag.hole_index)?;
        let handle = self.thumbnails.get(&thumb_key(track)).cloned();
        let display_index = drag.hole_index + 1;

        let mut row_children: Vec<Element<'a, Message>> = Vec::with_capacity(fields.len() + 2);
        row_children.push(self.render_cell(DisplayValue::Index(display_index), Length::Fixed(INDEX_COL_WIDTH), None));
        row_children.push(self.render_cell(DisplayValue::Thumbnail(handle), Length::Fixed(THUMBNAIL_COL_WIDTH), None));
        for &field in fields {
            row_children.push(self.render_cell(field.display_value(track, display_index), field.width(), None));
        }

        let ghost_row = row(row_children)
            .spacing(10)
            .align_y(Alignment::Center)
            .padding(Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 16.0 });

        let ghost_content = container(ghost_row)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                border: iced::border::rounded(6)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.15))
                    .width(1.0),
                ..Default::default()
            });

        let raw_mouse_y = drag.mouse_position.map(|p| p.y).unwrap_or_default();
        let y_pos = (raw_mouse_y - drag.grab_offset).max(0.0);

        Some(
            container(ghost_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding { top: y_pos, bottom: 0.0, left: 0.0, right: 0.0 })
                .into(),
        )
    }

    // ── Build ──────────────────────────────────────────────────────

    /// # Panics
    /// Si no se llamó `.on_event(...)`. No es un "olvido tolerable":
    /// una tabla sin conexión de eventos no tiene sentido en ninguna
    /// vista real.
    pub fn build(self) -> Element<'a, Message> {
        let fields = active_columns(self.show_added_at);
        let emit = self.on_event.clone().expect("TrackBuilder: falta .on_event(...)");

        let header = self.render_header(&fields, &emit);

        let window = self.scroll.window(self.row_height, self.tracks.len(), self.buffer_rows);
        let dragging_index = self.drag.as_ref().map(|d| d.hole_index);

        let body_rows: Element<'a, Message> = if let Some(animator) = self.animator {
            // Layout absoluto/animado: cada fila visible se posiciona vía
            // padding-top interpolado por el animator, en vez de fluir
            // secuencialmente — así puede DESLIZAR a su nueva posición en
            // vez de saltar de golpe (mismo mecanismo que QueuePanel).
            let now = Instant::now();
            let mut layers: Vec<Element<'a, Message>> = Vec::with_capacity(window.len());

            for visible_idx in window.start..window.end {
                if dragging_index == Some(visible_idx) {
                    continue;
                }
                let Some(row_el) = self.render_visible_row(&fields, &emit, visible_idx) else { continue };
                let track_id = self.tracks[visible_idx].id.as_str();
                let y = animator.visual_y_of(track_id, now, visible_idx);

                layers.push(
                    container(row_el)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(y))
                        .into(),
                );
            }

            let total_height = self.tracks.len() as f32 * self.row_height;
            stack(layers).width(Length::Fill).height(Length::Fixed(total_height)).into()
        } else {
            let mut rows = column![].width(Length::Fill);
            rows = rows.push(space().height(window.top_spacer_height(self.row_height)));

            for visible_idx in window.start..window.end {
                let is_dragging_this_row = dragging_index == Some(visible_idx);

                let rendered_row: Element<'a, Message> = if is_dragging_this_row {
                    container(space().height(Length::Fixed(self.row_height)))
                        .width(Length::Fill)
                        .height(Length::Fixed(self.row_height))
                        .into()
                } else {
                    match self.render_visible_row(&fields, &emit, visible_idx) {
                        Some(el) => el,
                        None => continue,
                    }
                };

                rows = rows.push(rendered_row);
            }

            rows = rows.push(space().height(window.bottom_spacer_height(self.row_height, self.tracks.len())));
            rows.into()
        };

        let emit_scroll = emit.clone();
        let emit_move = emit.clone();
        let emit_exit = emit.clone();

        let scroll_area: Element<'a, Message> = scrollable(body_rows)
            .id(Id::new(self.scrollable_id))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(move |v| emit_scroll(TrackEvent::Scrolled(v)))
            .into();

        let area = mouse_area(scroll_area)
            .on_move(move |p| emit_move(TrackEvent::MouseMoved(p)))
            .on_exit(emit_exit(TrackEvent::ViewportExited));
        let scroll_area: Element<'a, Message> = area.into();

        let ghost = self.ghost_overlay(&fields);
        let combined_overlay = match (self.overlay, ghost) {
            (Some(a), Some(b)) => Some(stack![a, b].into()),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let overlay_layer: Element<'a, Message> = combined_overlay.unwrap_or_else(|| space().into());

        let body: Element<'a, Message> = stack![scroll_area, overlay_layer].into();

        column![header, body].width(Length::Fill).height(Length::Fill).into()
    }
}