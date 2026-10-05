use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, scrollable, space, stack, text, Id};
use iced::{Alignment, Color, Element, Length, Padding, Point};
use strum_macros::AsRefStr;
use crate::model::{Album, Artist, Track};
use crate::ui::assets::fonts::{JETBRAINS_MONO, SF_PRO};
use crate::ui::assets::icons::{self, Icon};
use crate::ui::styles::button as button_style;
use crate::ui::styles::row as row_style;
use crate::ui::styles::RowSelectionShape;
use crate::ui::theme::theme;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::color::lerp_color;
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::widgets::artist_links::{album_link, artist_links, artist_names_text};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail_sized;
use crate::utils::formatting::{format_added_at, format_duration, format_last_played};
use crate::ui::assets::{spacing, typography};
use crate::ui::assets::radii;
use crate::ui::widgets::track_row_simple::{cache_indicator, is_downloaded, mark_slot};
use crate::ui::styles::scrollable as scrollable_style;

const THUMBNAIL_COL_WIDTH: f32 = 56.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const INDEX_COL_WIDTH: f32 = 40.0;
const DEFAULT_ROW_HEIGHT: f32 = 60.0;
const DEFAULT_BUFFER_ROWS: usize = 15;
/// Alto de la fila de títulos de columna cuando va dentro del scroll (con `leading`).
pub const COLUMN_HEADER_HEIGHT: f32 = 36.0;
/// Lo que tarda la banda (`band`) en fundirse con el fondo del panel.
pub const BAND_FADE_HEIGHT: f32 = 240.0;
/// Alto de las líneas verticales entre títulos de columna.
const HEADER_SEPARATOR_HEIGHT: f32 = 14.0;

/// Spacing entre celdas compartido por cabecera, fila y fila fantasma —
/// deben coincidir para que las etiquetas de columna queden sobre sus datos.
const ROW_GRID_SPACING: f32 = spacing::SP_16;

/// Padding lateral compartido por cabecera, fila y fila fantasma; el
/// vertical sí varía legítimamente (la cabecera no está constreñida en
/// altura como las filas).
fn row_grid_padding(vertical: f32) -> Padding {
    Padding { top: vertical, bottom: vertical, left: spacing::SP_10, right: spacing::SP_16 }
}

#[derive(Debug, Clone)]
pub enum TrackEvent {
    Clicked(String, usize),
    Scrolled(Viewport),
    Sorted(usize),
    MouseMoved(Point),
    RightClicked(String),
    ViewportExited,
    ArtistClicked(String),
    AlbumClicked(String),
    TogglePlayback,
    PlayingIconHover(bool),
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
    #[strum(serialize = "REPR.")]
    PlayCount = 8,
    #[strum(serialize = "ÚLTIMA VEZ")]
    LastPlayed = 9,
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
            TrackColumn::PlayCount => Length::Fixed(60.0),
            TrackColumn::LastPlayed => Length::Fixed(140.0),
        }
    }

    fn display_value(self, track: &Track, display_index: usize) -> DisplayValue {
        match self {
            TrackColumn::Index => DisplayValue::Index(display_index),
            TrackColumn::Title => DisplayValue::ColoredText(track.title.clone(), theme().content.primary),
            TrackColumn::Artist => DisplayValue::Artists(track.artists.clone()),
            TrackColumn::Album => DisplayValue::AlbumLink(track.album.clone()),
            TrackColumn::Duration => DisplayValue::Text(format_duration(track.duration_seconds)),
            TrackColumn::Bpm => DisplayValue::Text(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".to_string())),
            TrackColumn::Key => DisplayValue::ColoredText(
                track.camelot_key.clone().unwrap_or_else(|| "-".to_string()),
                theme().accent.primary,
            ),
            TrackColumn::AddedAt => DisplayValue::Text(format_added_at(track.added_at)),
            TrackColumn::PlayCount => DisplayValue::Text(track.play_count.map(|c| c.to_string()).unwrap_or_else(|| "-".to_string())),
            TrackColumn::LastPlayed => DisplayValue::Text(format_last_played(track.last_played_at)),
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

fn active_columns(show_added_at: bool, show_play_stats: bool) -> Vec<TrackColumn> {
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
    if show_play_stats {
        columns.push(TrackColumn::PlayCount);
        columns.push(TrackColumn::LastPlayed);
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

    // La columna Artista se ordena aparte (decorate-sort-undecorate):
    // `format_artists()` alloca un String, y dentro del comparador eso son
    // 2 allocaciones por comparación — O(n log n) Strings por click en la
    // cabecera. Materializando la clave una vez por track queda en N.
    // Se compara exactamente el mismo String que antes, así que el orden
    // resultante (empates incluidos: ambos sorts son estables) no cambia.
    if key == TrackColumn::Artist.as_usize() {
        let mut decorated: Vec<(String, &Track)> =
            tracks.iter().map(|t| (t.format_artists(), *t)).collect();

        decorated.sort_by(|a, b| {
            let ordering = a.0.cmp(&b.0);
            if ascending { ordering } else { ordering.reverse() }
        });

        for (slot, (_, track)) in tracks.iter_mut().zip(decorated) {
            *slot = track;
        }
        return;
    }

    tracks.sort_by(|a, b| {
        let ordering = match key {
            k if k == TrackColumn::Title.as_usize() => a.title.cmp(&b.title),
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
            k if k == TrackColumn::PlayCount.as_usize() => a.play_count.cmp(&b.play_count),
            k if k == TrackColumn::LastPlayed.as_usize() => a.last_played_at.cmp(&b.last_played_at),
            _ => std::cmp::Ordering::Equal,
        };
        if ascending { ordering } else { ordering.reverse() }
    });
}

/// Hueco entre títulos de columna, del mismo ancho que el espaciado de las
/// filas; con `with_line`, una línea vertical fina centrada.
fn header_gap<'a, Message: 'a>(with_line: bool) -> Element<'a, Message> {
    let gap = container(if with_line {
        container(space())
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(HEADER_SEPARATOR_HEIGHT))
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(theme().border.subtle.into()),
                ..Default::default()
            })
            .into()
    } else {
        Element::from(space())
    })
        .width(Length::Fixed(ROW_GRID_SPACING))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    gap.into()
}

/// Línea horizontal fina de separación.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    rule::horizontal(1.0)
        .style(|_theme: &iced::Theme| rule::Style {
            color: theme().border.subtle,
            radius: radii::R_NONE.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
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
    rows_offset: f32,
) -> Vec<(String, String)> {
    let window = scroll.window_after(rows_offset, row_height, tracks.len(), buffer_rows);
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
    /// Filas arrastradas juntas: el hueco y el fantasma van de `hole_index` a `hole_index + count`.
    count: usize,
}

// ── Builder ─────────────────────────────────────────────────────

pub struct TrackBuilder<'a, Message> {
    tracks: Vec<&'a Track>,
    scroll: &'a ScrollTracker,
    thumbnails: &'a AsyncThumbnail,
    selected_ids: &'a HashSet<String>,
    scrollable_id: &'static str,

    playing_id: Option<String>,
    is_playing: bool,
    icon_hovered: bool,

    row_height: f32,
    buffer_rows: usize,

    show_added_at: bool,
    show_play_stats: bool,
    /// Ids con letra: si está, el título lleva la marca de letra (solo playlists).
    lyrics: Option<&'a HashSet<String>>,
    /// El título lleva el punto de descargada / sin descargar (mezclas recomendadas).
    cache_dots: bool,
    index_sortable: bool,

    active_sort_key: Option<usize>,
    sort_direction_asc: bool,

    drag: Option<DragVisual>,
    animator: Option<&'a RowAnimator>,

    on_event: Option<Rc<dyn Fn(TrackEvent) -> Message + 'a>>,

    /// Contenido de alto fijo arriba de la tabla que scrollea junto con ella.
    leading: Option<(Element<'a, Message>, f32)>,
    /// Barra de alto fijo entre `leading` y los títulos de columna (también scrollea).
    toolbar: Option<(Element<'a, Message>, f32)>,
    /// Color con el que arranca, debajo de `leading`, una banda que se funde con el fondo.
    band: Option<Color>,
    /// Margen lateral de títulos y filas (el `leading` va de borde a borde).
    content_padding_x: f32,
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
            playing_id: None,
            is_playing: false,
            icon_hovered: false,
            row_height: DEFAULT_ROW_HEIGHT,
            buffer_rows: DEFAULT_BUFFER_ROWS,
            show_added_at: false,
            show_play_stats: false,
            lyrics: None,
            cache_dots: false,
            index_sortable: false,
            active_sort_key: Some(TrackColumn::Index.as_usize()),
            sort_direction_asc: true,
            drag: None,
            animator: None,
            on_event: None,
            leading: None,
            toolbar: None,
            band: None,
            content_padding_x: 0.0,
        }
    }

    // ── Diffs sobre la fila base ─────────────────────────────────

    /// Agrega la columna "AGREGADO" (fecha) al final de la fila.
    pub fn with_added_at(mut self) -> Self {
        self.show_added_at = true;
        self
    }

    /// Pone `content` (de `height` px) arriba de la tabla, dentro del mismo
    /// scroll: al bajar, se va junto con los títulos de columna.
    pub fn leading(mut self, content: impl Into<Element<'a, Message>>, height: f32) -> Self {
        self.leading = Some((content.into(), height));
        self
    }

    /// Barra de `height` px entre el `leading` y los títulos de columna.
    pub fn toolbar(mut self, content: impl Into<Element<'a, Message>>, height: f32) -> Self {
        self.toolbar = Some((content.into(), height));
        self
    }

    /// Banda de fondo que arranca en `color` justo debajo del `leading` (detrás de
    /// la barra, los títulos y las primeras filas) y se funde con el panel.
    pub fn band(mut self, color: Color) -> Self {
        self.band = Some(color);
        self
    }

    /// Margen lateral de los títulos de columna y las filas.
    pub fn content_padding_x(mut self, padding: f32) -> Self {
        self.content_padding_x = padding;
        self
    }

    /// Agrega las columnas de depuración "REPR." y "ÚLTIMA VEZ" si `show` es `true`.
    pub fn with_play_stats(mut self, show: bool) -> Self {
        self.show_play_stats = show;
        self
    }

    /// Marca con un ícono de letra el título de las canciones de `with_lyrics`.
    pub fn lyrics(mut self, with_lyrics: &'a HashSet<String>) -> Self {
        self.lyrics = Some(with_lyrics);
        self
    }

    /// Agrega al título el punto de descargada (verde) o sin descargar, como en artista/álbum.
    pub fn cache_dots(mut self) -> Self {
        self.cache_dots = true;
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

    /// Fija cuál track (si alguno) está sonando ahora mismo.
    pub fn playing(mut self, playing_id: Option<String>, is_playing: bool) -> Self {
        self.playing_id = playing_id;
        self.is_playing = is_playing;
        self
    }

    /// Si el mouse está sobre el icono de la fila que suena.
    pub fn icon_hovered(mut self, hovered: bool) -> Self {
        self.icon_hovered = hovered;
        self
    }

    /// Activa el pintado del ghost row de reordenamiento.
    /// `hole_index` es la fila (dentro del vector visual ya
    /// reordenado que se pasó al builder) que se está arrastrando.
    /// Puramente visual — la vista decide CUÁNDO llamar esto.
    pub fn dragging(mut self, hole_index: usize, mouse_position: Option<Point>, grab_offset: f32) -> Self {
        self.drag = Some(DragVisual { hole_index, mouse_position, grab_offset, count: 1 });
        self
    }

    /// Cuántas filas consecutivas desde el hueco se arrastran juntas (solo junto con `.dragging(...)`).
    pub fn drag_count(mut self, count: usize) -> Self {
        if let Some(drag) = &mut self.drag {
            drag.count = count.max(1);
        }
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

    // ── Render: header ────────────────────────────────────────────

    /// Títulos de columna con una línea fina entre cada uno y otra debajo.
    /// Las líneas ocupan el mismo hueco que el espaciado de las filas, así
    /// que los títulos siguen alineados con sus datos.
    fn render_header(&self, fields: &[TrackColumn], emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>) -> Element<'a, Message> {
        let mut cells: Vec<Element<'a, Message>> = Vec::with_capacity(2 * fields.len() + 4);

        cells.push(if self.index_sortable {
            self.sortable_header_cell(TrackColumn::Index, emit)
        } else {
            container(text("#").font(JETBRAINS_MONO).size(typography::TEXT_11).color(theme().content.faint))
                .width(Length::Fixed(INDEX_COL_WIDTH))
                .into()
        });

        // La columna de la miniatura no tiene título: sin líneas a sus lados.
        cells.push(header_gap(false));
        cells.push(container(space()).width(Length::Fixed(THUMBNAIL_COL_WIDTH)).into());

        for (i, &field) in fields.iter().enumerate() {
            cells.push(header_gap(i > 0));
            cells.push(self.sortable_header_cell(field, emit));
        }

        let titles = row(cells)
            .align_y(Alignment::Center)
            .padding(row_grid_padding(spacing::SP_4));

        column![titles, divider()].width(Length::Fill).into()
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
            theme().accent.primary
        } else {
            theme().content.muted
        };

        button(
            text(format!("{}{}", field.as_ref(), arrow))
                .font(SF_PRO)
                .size(typography::TEXT_10_5)
                .style(move |_| text::Style { color: Some(color) }),
        )
            .style(button_style::minimal)
            .width(field.width())
            .padding(spacing::SP_0)
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
                text(i.to_string()).font(SF_PRO).size(typography::TEXT_12).color(theme().content.faint),
            )
                .width(width)
                .into(),
            DisplayValue::Thumbnail(handle) => container(track_thumbnail_sized(handle, THUMBNAIL_SIZE))
                .width(width)
                .align_y(Alignment::Center)
                .into(),
            DisplayValue::Text(s) => single_line_text(s, SF_PRO, typography::TEXT_13_5, theme().content.muted, width),
            DisplayValue::ColoredText(s, c) => single_line_text(s, SF_PRO, typography::TEXT_13_5, c, width),
            DisplayValue::Artists(artists) => match emit {
                Some(emit) => {
                    let emit = Rc::clone(emit);
                    artist_links(&artists, SF_PRO, typography::TEXT_13_5, theme().content.muted, width, move |id| {
                        emit(TrackEvent::ArtistClicked(id))
                    })
                }
                None => artist_names_text(&artists, SF_PRO, typography::TEXT_13_5, theme().content.muted, width),
            },
            DisplayValue::AlbumLink(album) => match emit {
                Some(emit) => {
                    let emit = Rc::clone(emit);
                    album_link(album.as_ref(), SF_PRO, typography::TEXT_13_5, theme().content.muted, width, move |id| {
                        emit(TrackEvent::AlbumClicked(id))
                    })
                }
                None => {
                    let name = album.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "-".to_string());
                    single_line_text(name, SF_PRO, typography::TEXT_13_5, theme().content.muted, width)
                }
            },
        }
    }

    /// Celda líder de la fila: número de orden normalmente; si es la fila
    /// que está sonando, el ecualizador animado o el icono de play/pausa
    /// al pasar el mouse por encima de la fila.
    fn render_leading_cell(&self, display_index: usize, is_current_row: bool) -> Element<'a, Message> {
        if !is_current_row {
            return self.render_cell(DisplayValue::Index(display_index), Length::Fixed(INDEX_COL_WIDTH), None);
        }

        let show_toggle_icon = !self.is_playing || self.icon_hovered;

        let glyph: Element<'a, Message> = if show_toggle_icon {
            let icon_variant = if self.is_playing { Icon::Pause } else { Icon::Play };
            icons::icon(icon_variant, typography::TEXT_13)
                .color(theme().accent.primary)
                .into()
        } else {
            icons::glyph(Icon::equalizer_frame(), typography::TEXT_11).color(theme().accent.primary).into()
        };

        container(glyph)
            .width(Length::Fixed(INDEX_COL_WIDTH))
            .height(Length::Fixed(self.row_height))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    }

    fn render_row(
        &self,
        fields: &[TrackColumn],
        track: &'a Track,
        display_index: usize,
        handle: Option<Handle>,
        shape: RowSelectionShape,
        is_current_row: bool,
        emit: &Rc<dyn Fn(TrackEvent) -> Message + 'a>,
    ) -> Element<'a, Message> {
        let mut row_children: Vec<Element<'a, Message>> = Vec::with_capacity(fields.len() + 2);

        row_children.push(self.render_leading_cell(display_index, is_current_row));
        row_children.push(self.render_cell(DisplayValue::Thumbnail(handle), Length::Fixed(THUMBNAIL_COL_WIDTH), None));

        for &field in fields {
            let display = if field == TrackColumn::Title && is_current_row {
                DisplayValue::ColoredText(track.title.clone(), theme().accent.primary)
            } else {
                field.display_value(track, display_index)
            };
            let has_marks = self.lyrics.is_some() || self.cache_dots;
            let cell = if field == TrackColumn::Title && has_marks {
                let mut title = row![self.render_cell(display, Length::Fill, Some(emit))]
                    .spacing(spacing::SP_8)
                    .align_y(Alignment::Center)
                    .width(field.width());
                if let Some(with_lyrics) = self.lyrics {
                    title = title.push(mark_slot(with_lyrics.contains(&track.id).then(|| lyrics_mark(is_current_row))));
                }
                if self.cache_dots {
                    title = title.push(mark_slot(Some(cache_indicator(is_downloaded(track)))));
                }
                title.into()
            } else {
                self.render_cell(display, field.width(), Some(emit))
            };
            row_children.push(cell);
        }

        let row_content = row(row_children)
            .spacing(ROW_GRID_SPACING)
            .align_y(Alignment::Center)
            .padding(row_grid_padding(spacing::SP_0));

        // display_index es 1-based (para mostrar "#1, #2..."); el índice
        // real dentro del vector visible es display_index - 1.
        let visible_idx = display_index - 1;
        let click_id = track.id.clone();
        let right_click_id = track.id.clone();

        let centered_content = container(row_content)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .align_y(Alignment::Center);

        let on_press = if is_current_row {
            emit(TrackEvent::TogglePlayback)
        } else {
            emit(TrackEvent::Clicked(click_id, visible_idx))
        };

        // Durante un arrastre las filas no se resaltan ni reciben el clic al
        // soltar (igual que en la cola): el único resaltado es el fantasma.
        let btn = button(centered_content)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .padding(spacing::SP_0);
        let btn = if self.drag.is_some() {
            btn.style(button_style::inert)
        } else {
            btn.style(button_style::transparent).on_press(on_press)
        };

        let styled = container(btn)
            .width(Length::Fill)
            .height(Length::Fixed(self.row_height))
            .align_y(Alignment::Center)
            .style(row_style::selected(shape));

        let area = mouse_area(styled).on_right_press(emit(TrackEvent::RightClicked(right_click_id)));

        if is_current_row {
            area.on_enter(emit(TrackEvent::PlayingIconHover(true)))
                .on_exit(emit(TrackEvent::PlayingIconHover(false)))
                .into()
        } else {
            area.into()
        }
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
        let is_current_row = self.playing_id.as_deref() == Some(track.id.as_str());

        let display_index = visible_idx + 1;
        Some(self.render_row(fields, track, display_index, handle, shape, is_current_row, emit))
    }

    // ── Ghost row (drag) ──────────────────────────────────────────

    fn ghost_overlay(&self, fields: &[TrackColumn]) -> Option<Element<'a, Message>> {
        let drag = self.drag.as_ref()?;
        let end = (drag.hole_index + drag.count).min(self.tracks.len());

        // Todas las filas arrastradas, pegadas, en una sola tarjeta.
        let ghost_rows = (drag.hole_index..end).map(|index| -> Element<'a, Message> {
            let track = self.tracks[index];
            let handle = self.thumbnails.get(&thumb_key(track)).cloned();
            let display_index = index + 1;

            let mut row_children: Vec<Element<'a, Message>> = Vec::with_capacity(fields.len() + 2);
            row_children.push(self.render_cell(DisplayValue::Index(display_index), Length::Fixed(INDEX_COL_WIDTH), None));
            row_children.push(self.render_cell(DisplayValue::Thumbnail(handle), Length::Fixed(THUMBNAIL_COL_WIDTH), None));
            for &field in fields {
                row_children.push(self.render_cell(field.display_value(track, display_index), field.width(), None));
            }

            container(
                row(row_children)
                    .spacing(ROW_GRID_SPACING)
                    .align_y(Alignment::Center)
                    .padding(row_grid_padding(spacing::SP_0)),
            )
                .width(Length::Fill)
                .height(Length::Fixed(self.row_height))
                .align_y(Alignment::Center)
                .into()
        });

        let ghost_content = container(column(ghost_rows.collect::<Vec<_>>()))
            .width(Length::Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(theme().overlay.hover.into()),
                border: iced::border::rounded(radii::R_6)
                    .color(theme().border.subtle)
                    .width(1.0),
                ..Default::default()
            });

        let raw_mouse_y = drag.mouse_position.map(|p| p.y).unwrap_or_default();
        let y_pos = (raw_mouse_y - drag.grab_offset).max(0.0);

        Some(
            container(ghost_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: y_pos,
                    bottom: spacing::SP_0,
                    left: self.content_padding_x,
                    right: self.content_padding_x,
                })
                .into(),
        )
    }

    // ── Build ──────────────────────────────────────────────────────

    /// # Panics
    /// Si no se llamó `.on_event(...)`. No es un "olvido tolerable":
    /// una tabla sin conexión de eventos no tiene sentido en ninguna
    /// vista real.
    pub fn build(mut self) -> Element<'a, Message> {
        let fields = active_columns(self.show_added_at, self.show_play_stats);
        let emit = self.on_event.clone().expect("TrackBuilder: falta .on_event(...)");

        let header = self.render_header(&fields, &emit);
        let leading = self.leading.take();
        let toolbar = self.toolbar.take();
        let toolbar_height = toolbar.as_ref().map_or(0.0, |(_, height)| *height);
        // Hasta acá llega lo que se va con el scroll antes de que los títulos queden fijos.
        let pinned_from = leading.as_ref().map(|(_, height)| height + toolbar_height);
        let rows_offset = pinned_from.map_or(0.0, |offset| offset + COLUMN_HEADER_HEIGHT);

        let window = self.scroll.window_after(rows_offset, self.row_height, self.tracks.len(), self.buffer_rows);
        let dragging_rows = self.drag.as_ref().map(|d| d.hole_index..d.hole_index + d.count);
        let is_dragged_row = |index: usize| dragging_rows.as_ref().is_some_and(|rows| rows.contains(&index));

        let body_rows: Element<'a, Message> = if let Some(animator) = self.animator {
            // Layout absoluto/animado: cada fila visible se posiciona vía
            // padding-top interpolado por el animator, en vez de fluir
            // secuencialmente — así puede DESLIZAR a su nueva posición en
            // vez de saltar de golpe (mismo mecanismo que QueuePanel).
            let now = Instant::now();
            let mut layers: Vec<Element<'a, Message>> = Vec::with_capacity(window.len());

            for visible_idx in window.start..window.end {
                if is_dragged_row(visible_idx) {
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
                let is_dragging_this_row = is_dragged_row(visible_idx);

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

        let side_padding = Padding { left: self.content_padding_x, right: self.content_padding_x, ..Padding::ZERO };
        let body_rows = container(body_rows).width(Length::Fill).padding(side_padding);

        // Con `leading`, al pasar ese contenido los títulos quedan fijos arriba.
        let sticky_header: Option<Element<'a, Message>> = pinned_from
            .filter(|offset| self.scroll.offset_y >= *offset)
            .map(|_| {
                // Mismo tono que tiene la banda a esta altura, para que la franja fija no se note.
                let leading_height = pinned_from.unwrap_or_default() - toolbar_height;
                let fade = ((self.scroll.offset_y - leading_height) / BAND_FADE_HEIGHT).clamp(0.0, 1.0);
                let background = self.band.map_or(theme().surface.panel, |band| lerp_color(band, theme().surface.panel, fade));
                let titles = container(self.render_header(&fields, &emit))
                    .width(Length::Fill)
                    .height(Length::Fixed(COLUMN_HEADER_HEIGHT))
                    .align_y(Alignment::End)
                    .padding(side_padding)
                    .style(move |_theme: &iced::Theme| container::Style {
                        background: Some(background.into()),
                        ..Default::default()
                    });
                // `opaque`: un clic fuera de los títulos no cae en la fila tapada.
                column![opaque(titles)].width(Length::Fill).into()
            });

        // Con `leading`, títulos y filas van dentro del scroll, debajo de ese contenido.
        let (header, scroll_content): (Option<Element<'a, Message>>, Element<'a, Message>) = match leading {
            Some((leading, _)) => {
                let below = column![
                    toolbar.map(|(bar, height)| container(bar).width(Length::Fill).height(Length::Fixed(height))),
                    container(header).width(Length::Fill).height(Length::Fixed(COLUMN_HEADER_HEIGHT)).align_y(Alignment::End).padding(side_padding),
                    body_rows,
                ]
                    .width(Length::Fill);

                let below: Element<'a, Message> = match self.band {
                    // La banda dura `BAND_FADE_HEIGHT` px sin importar cuántas filas haya.
                    Some(band) => {
                        let total = toolbar_height + COLUMN_HEADER_HEIGHT + self.tracks.len() as f32 * self.row_height;
                        let fade_end = (BAND_FADE_HEIGHT / total.max(1.0)).min(1.0);
                        container(below)
                            .width(Length::Fill)
                            .style(move |_theme: &iced::Theme| container::Style {
                                background: Some(
                                    iced::gradient::Linear::new(std::f32::consts::PI)
                                        .add_stop(0.0, band)
                                        .add_stop(fade_end, theme().surface.panel)
                                        .into(),
                                ),
                                ..Default::default()
                            })
                            .into()
                    }
                    None => below.into(),
                };

                (None, column![leading, below].width(Length::Fill).into())
            }
            None => (Some(container(header).width(Length::Fill).padding(side_padding).into()), body_rows.into()),
        };

        let scroll_area = scrollable(scroll_content)
            .id(Id::new(self.scrollable_id))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(move |v| emit_scroll(TrackEvent::Scrolled(v)));
        // Con `leading` la tabla va de borde a borde: la barra que se ve deja un hueco arriba y
        // abajo para no tocar las esquinas del panel (la nativa queda invisible, para arrastrarla).
        let scroll_area: Element<'a, Message> = if pinned_from.is_some() {
            scroll_area.style(scrollable_style::invisible).into()
        } else {
            scroll_area.style(scrollable_style::discreet).into()
        };
        let inset_scrollbar: Option<Element<'a, Message>> = pinned_from.map(|_| {
            let content_height = rows_offset + self.tracks.len() as f32 * self.row_height;
            scrollable_style::inset_scrollbar(self.scroll.offset_y, self.scroll.viewport_height, content_height)
        });

        let area = mouse_area(scroll_area)
            .on_move(move |p| emit_move(TrackEvent::MouseMoved(p)))
            .on_exit(emit_exit(TrackEvent::ViewportExited));
        let scroll_area: Element<'a, Message> = area.into();

        let overlay_layer: Element<'a, Message> = self.ghost_overlay(&fields).unwrap_or_else(|| space().into());

        let mut layers = vec![scroll_area];
        layers.extend(sticky_header);
        layers.extend(inset_scrollbar);
        layers.push(overlay_layer);
        let body: Element<'a, Message> = stack(layers).into();

        match header {
            Some(header) => column![header, body].width(Length::Fill).height(Length::Fill).into(),
            None => body,
        }
    }
}

/// Ícono de "tiene letra": tenue, y en el acento en la fila que suena.
fn lyrics_mark<'a, Message: 'a>(is_current_row: bool) -> Element<'a, Message> {
    let color = if is_current_row { theme().accent.primary } else { theme().content.faint };
    icons::icon(Icon::Lyrics, typography::TEXT_12).color(color).into()
}

