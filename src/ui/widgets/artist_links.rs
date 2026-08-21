use iced::widget::text::{Rich, Wrapping};
use iced::widget::{container, span};
use iced::{alignment::Horizontal, Color, Element, Font, Length};

use crate::model::{Album, Artist};
use crate::ui::widgets::single_line_text::single_line_text_aligned;

/// Como [`single_line_text`], pero para el string de artistas de un track:
/// cada artista es un span clickeable independiente que linkea a su propio
/// id, separados por ", ". Si el track no tiene artistas resueltos, cae al
/// mismo texto plano "Artista Desconocido" que usa `Track::format_artists()`.
pub fn artist_links<'a, Message, F>(
    artists: &[Artist],
    font: Font,
    size: f32,
    color: Color,
    width: Length,
    on_click: F,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(String) -> Message + 'a,
{
    artist_links_aligned(artists, font, size, color, width, Horizontal::Left, on_click)
}

/// Igual que [`artist_links`], pero alineando el bloque de spans dentro de
/// su caja (p.ej. centrado, como en el header del modo teatro).
pub fn artist_links_aligned<'a, Message, F>(
    artists: &[Artist],
    font: Font,
    size: f32,
    color: Color,
    width: Length,
    align_x: Horizontal,
    on_click: F,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(String) -> Message + 'a,
{
    if artists.is_empty() {
        return single_line_text_aligned("Artista Desconocido", font, size, color, width, align_x);
    }

    // Spans dueños de su propio `String` (no `&str` prestado de `artists`):
    // así el slice de entrada puede ser un `Vec<Artist>` temporal (p.ej. un
    // clon armado al vuelo) sin atarse al lifetime `'a` del `Element` devuelto.
    let mut spans = Vec::with_capacity(artists.len() * 2 - 1);
    for (i, artist) in artists.iter().enumerate() {
        if i > 0 {
            spans.push(span(", ".to_string()).font(font).size(size).color(color));
        }

        let mut artist_span = span(artist.name.clone()).font(font).size(size).color(color);
        if let Some(id) = artist.id.clone() {
            artist_span = artist_span.link(id);
        }
        spans.push(artist_span);
    }

    container(
        Rich::with_spans(spans)
            .on_link_click(move |id: String| on_click(id))
            .wrapping(Wrapping::None)
            .width(width)
            .align_x(align_x),
    )
    .width(width)
    .clip(true)
    .into()
}

/// Nombres de artistas unidos con ", " — sin link, para contextos donde ya
/// existe otro camino de navegación (p.ej. modo teatro, que muestra la
/// misma canción que el reproductor; o resultados de búsqueda, que no
/// deberían servir para entrar a una vista de artista).
fn joined_artist_names(artists: &[Artist]) -> String {
    if artists.is_empty() {
        "Artista Desconocido".to_string()
    } else {
        artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }
}

/// Igual que [`artist_links`], pero como texto plano (no clickeable).
pub fn artist_names_text<'a, Message: 'a>(
    artists: &[Artist],
    font: Font,
    size: f32,
    color: Color,
    width: Length,
) -> Element<'a, Message> {
    artist_names_text_aligned(artists, font, size, color, width, Horizontal::Left)
}

/// Igual que [`artist_names_text`], pero alineando el texto dentro de su caja.
pub fn artist_names_text_aligned<'a, Message: 'a>(
    artists: &[Artist],
    font: Font,
    size: f32,
    color: Color,
    width: Length,
    align_x: Horizontal,
) -> Element<'a, Message> {
    single_line_text_aligned(joined_artist_names(artists), font, size, color, width, align_x)
}

/// Nombre del álbum de un track, clickeable si tiene id resuelto — mismo
/// patrón que [`artist_links`] pero para un solo crédito.
pub fn album_link<'a, Message, F>(
    album: Option<&Album>,
    font: Font,
    size: f32,
    color: Color,
    width: Length,
    on_click: F,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(String) -> Message + 'a,
{
    let Some(album) = album else {
        return single_line_text_aligned("-", font, size, color, width, Horizontal::Left);
    };

    let album_span = span(album.name.clone()).font(font).size(size).color(color).link(album.id.clone());

    container(
        Rich::with_spans(vec![album_span])
            .on_link_click(move |id: String| on_click(id))
            .wrapping(Wrapping::None)
            .width(width),
    )
    .width(width)
    .clip(true)
    .into()
}
