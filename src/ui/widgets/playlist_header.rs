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
use iced::widget::{button, column, container, row, space, stack, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use iced::border::rounded;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::utils::playlist_metadata::{format_track_count, format_total_duration};
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;
use crate::ui::assets::radii;

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

/// Construye el banner completo. `on_play` es el mensaje disparado al
/// presionar el botón grande — la vista decide qué significa eso (p. ej.
/// `RequestPlayContext(tracks, 0)`, mismo patrón que ya usa
/// `FavoritesView::PlayTrack`). `is_playing` controla el glifo (▶/⏸).
///
/// `on_cover_click` es opcional: si es `Some`, la portada se envuelve en
/// un overlay de "cambiar portada" que aparece al hacer hover; si es
/// `None` (p. ej. para "Me gusta", que no tiene portada editable) el
/// comportamiento es idéntico a no tener overlay.
pub fn playlist_header<'a, Message: Clone + 'a>(
    data: PlaylistHeaderData<'a>,
    cover: Option<Handle>,
    on_play: Message,
    on_cover_click: Option<Message>,
    is_playing: bool,
) -> Element<'a, Message> {
    let cover_state = match cover {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    let cover_element = async_thumbnail(cover_state, COVER_SIZE, 16.0);

    let cover_element: Element<'a, Message> = if let Some(on_click) = on_cover_click {
        let hover_button = button(
            container(
                container(
                    icons::icon(Icon::Camera, typography::TEXT_24),
                )
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
                .width(Length::Fixed(COVER_SIZE))
                .height(Length::Fixed(COVER_SIZE)),
        )
            .width(Length::Fixed(COVER_SIZE))
            .height(Length::Fixed(COVER_SIZE))
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme().overlay.scrim,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: match status {
                        button::Status::Hovered => theme().content.primary,
                        _ => Color::TRANSPARENT,
                    },
                    border: rounded(radii::R_13_5),
                    ..Default::default()
                }
            })
            .on_press(on_click);


        let hover_overlay = container(hover_button)
            .width(Length::Fixed(COVER_SIZE))
            .height(Length::Fixed(COVER_SIZE));


        container(
            stack![
                cover_element,
                hover_overlay,
            ],
        )
            .width(Length::Fixed(COVER_SIZE))
            .height(Length::Fixed(COVER_SIZE))
            .into()
    } else {
        cover_element
    };

    let kicker: Element<'a, Message> = match data.kicker {
        Some(k) => text(k)
            .font(SF_PRO)
            .size(typography::TEXT_12)
            .color(theme().content.secondary)
            .into(),
        None => space().height(0).into(),
    };

    let title = text(data.name)
        .font(SF_PRO)
        .size(typography::TEXT_40)
        .color(theme().content.primary);

    let metadata = text(format!(
        "{} · {}",
        format_track_count(data.track_count),
        format_total_duration(data.total_duration_seconds),
    ))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.secondary);

    let play_icon = if is_playing { Icon::Pause } else { Icon::Play };
    let play_button = button(
        container(
            icons::icon(play_icon, typography::TEXT_20).color(theme().content.on_accent),
        )
            .width(Length::Fixed(PLAY_BUTTON_SIZE))
            .height(Length::Fixed(PLAY_BUTTON_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
        .padding(spacing::SP_0)
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered => theme().accent.hover,
                _ => theme().accent.primary,
            };
            button::Style {
                background: Some(bg.into()),
                border: rounded(PLAY_BUTTON_SIZE / 2.0),
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
        .spacing(spacing::SP_2);

    let content = row![
        cover_element,
        info_column,
    ]
        .spacing(spacing::SP_24)
        .align_y(Alignment::End)
        .padding(Padding { top: spacing::SP_32, bottom: spacing::SP_28, left: spacing::SP_8, right: spacing::SP_8 });

    container(content)
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(
                iced::gradient::Linear::new(std::f32::consts::PI * 1.5)
                    .add_stop(0.0, theme().surface.gradient_start)
                    .add_stop(1.0, theme().surface.base)
                    .into(),
            ),
            ..Default::default()
        })
        .into()
}