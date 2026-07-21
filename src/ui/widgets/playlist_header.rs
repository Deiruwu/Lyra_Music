//! # PlaylistHeader — banner superior de vista de playlist
//!
//! Banner grande estilo Spotify/Tidal: portada + nombre + metadata
//! (cantidad de canciones y duración total), con un botón de reproducir
//! (▶) sobre un fondo con gradiente sutil hacia el color de fondo base
//! de la app, para que se sienta "anclado arriba" y no como una fila más.
//!
//! ## Por qué es un widget aparte (no vive dentro de PlaylistsView)
//!
//! Se usa desde cualquier vista que muestre el contenido de UNA playlist
//! puntual (`PlaylistsView` para las CUSTOM, y potencialmente
//! `FavoritesView` si en el futuro se decide mostrar el mismo banner
//! para "Me gusta" en vez del título simple actual). Mantenerlo separado
//! de `track_list` es intencional: el banner no es parte de la tabla
//! virtualizada, es un header de página fijo que se pinta una sola vez
//! por encima de ella — cosas distintas, ciclos de vida distintos.
//!
//! ## Cómo se consume
//!
//! ```ignore
//! use crate::ui::widgets::playlist_header::{playlist_header, PlaylistHeaderData};
//!
//! let header = playlist_header(
//!     PlaylistHeaderData {
//!         name: &playlist.name,
//!         cover_url: playlist.cover_url.as_deref(),
//!         track_count: tracks.len(),
//!         total_duration_seconds: tracks.iter().map(|t| t.duration_seconds).sum(),
//!     },
//!     cover_handle,           // Option<Handle>, ya resuelto por la vista vía ThumbnailCache
//!     MyMsg::PlayPlaylist,    // mensaje del botón ▶
//! );
//!
//! column![header, resto_del_contenido].into()
//! ```
//!
//! La vista es responsable de pedir/cachear `cover_handle` (mismo patrón
//! que ya usan Explorer/Favorites con `ThumbnailCache` para thumbnails de
//! track) — este widget no descarga nada, solo pinta lo que le pasan.

use iced::widget::image::Handle;
use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Color, Element, Font, Length, Padding};
use crate::ui::assets::icons::Icon;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};

pub const SF_PRO: Font = Font::with_name("SF Pro Display");
pub const JETBRAINS_MONO_ICON: Font = Font::with_name("JetBrainsMono Nerd Font");

const COVER_SIZE: f32 = 176.0;
const PLAY_BUTTON_SIZE: f32 = 52.0;

/// Datos puramente informativos del banner. No incluye el `Handle` de
/// la portada porque resolverlo (pedirlo a `ThumbnailCache`, manejar el
/// estado "aún cargando") es responsabilidad de la vista, igual que ya
/// pasa con `track_thumbnail_sized` en las filas de track.
pub struct PlaylistHeaderData<'a> {
    pub name: &'a str,
    /// Subtítulo pequeño encima del nombre, p. ej. "PLAYLIST" o
    /// "ME GUSTA". `None` para omitirlo.
    pub kicker: Option<&'a str>,
    pub track_count: usize,
    pub total_duration_seconds: i64,
}

/// Formatea segundos totales al estilo Spotify: "3 h 24 min" si hay
/// horas, o "42 min" si dura menos de una hora. Nunca muestra segundos
/// sueltos en el total (a diferencia de la duración por track, que sí
/// usa mm:ss) — así se ve la convención habitual de "duración de
/// colección" en vez de "duración de una canción".
pub fn format_total_duration(total_seconds: i64) -> String {
    let total_minutes = total_seconds / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;

    if hours > 0 {
        format!("{} h {} min", hours, minutes)
    } else {
        format!("{} min", minutes)
    }
}

fn format_track_count(count: usize) -> String {
    if count == 1 {
        "1 canción".to_string()
    } else {
        format!("{} canciones", count)
    }
}

/// Construye el banner completo. `on_play` es el mensaje disparado al
/// presionar el botón ▶ grande (reproducir la playlist completa desde
/// el principio) — la vista decide qué significa eso (p. ej.
/// `RequestPlayContext(tracks, 0)`, mismo patrón que ya usa
/// `FavoritesView::PlayTrack`).
pub fn playlist_header<'a, Message: Clone + 'a>(
    data: PlaylistHeaderData<'a>,
    cover: Option<Handle>,
    on_play: Message,
) -> Element<'a, Message> {
    let cover_state = match cover {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    let cover_element = async_thumbnail(cover_state, COVER_SIZE);

    let kicker: Element<'a, Message> = match data.kicker {
        Some(k) => text(k)
            .font(SF_PRO)
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.8))
            .into(),
        None => space().height(0).into(),
    };

    let title = text(data.name)
        .font(SF_PRO)
        .size(40)
        .color(Color::WHITE);

    let metadata = text(format!(
        "{} · {}",
        format_track_count(data.track_count),
        format_total_duration(data.total_duration_seconds),
    ))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75));

    let play_button = button(
        container(
            text(Icon::Play.as_ref()).font(JETBRAINS_MONO_ICON).size(20).color(Color::BLACK),
        )
            .width(Length::Fixed(PLAY_BUTTON_SIZE))
            .height(Length::Fixed(PLAY_BUTTON_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
        .padding(0)
        .style(|_theme: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => Color::from_rgb(0.85, 0.68, 1.0),
                _ => Color::from_rgb(0.74, 0.58, 0.98),
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::border::rounded(PLAY_BUTTON_SIZE / 2.0),
                ..Default::default()
            }
        })
        .on_press(on_play);

    let info_column = column![
        kicker,
        title,
        space().height(8),
        metadata,
        space().height(16),
        play_button,
    ]
        .align_x(Alignment::Start)
        .spacing(2);

    let content = row![
        cover_element,
        info_column,
    ]
        .spacing(24)
        .align_y(Alignment::End)
        .padding(Padding { top: 32.0, bottom: 28.0, left: 8.0, right: 8.0 });

    container(content)
        .width(Length::Fill)
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(
                iced::gradient::Linear::new(std::f32::consts::PI * 1.5)
                    .add_stop(0.0, Color::from_rgb(0.22, 0.16, 0.28))
                    .add_stop(1.0, Color::from_rgb(0.09, 0.09, 0.10))
                    .into(),
            ),
            ..Default::default()
        })
        .into()
}