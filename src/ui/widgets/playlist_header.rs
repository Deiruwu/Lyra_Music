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
use iced::widget::{button, column, container, image, mouse_area, row, space, stack, text, text_input, Id};
use iced::{Alignment, Border, Color, ContentFit, Element, Length, Padding, Theme};
use iced::border::{rounded, Radius};
use crate::model::Track;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::utils::playlist_metadata::{format_track_count, format_total_duration};
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;
use crate::ui::assets::radii;
use crate::ui::styles::text_input as text_input_style;

const COVER_SIZE: f32 = 176.0;
const COVER_RADIUS: f32 = 16.0;
const PLAY_BUTTON_SIZE: f32 = 52.0;

/// Portada del banner: una imagen, o un mosaico 2×2 de carátulas (Explorar / Me gusta).
pub enum HeaderCover {
    Single(Option<Handle>),
    /// Cuatro carátulas en orden de lectura; con menos de cuatro se usa la primera sola.
    Mosaic(Vec<Option<Handle>>),
}

/// Datos puramente informativos del banner. No incluye el `Handle` de
/// la portada porque resolverlo (pedirlo a `ThumbnailCache`, manejar el
/// estado "aún cargando") es responsabilidad de la vista, igual que ya
/// pasa con `track_thumbnail_sized` en las filas de track.
pub struct PlaylistHeaderData<'a> {
    pub name: &'a str,
    /// Subtítulo pequeño encima del nombre, p. ej. "PLAYLIST" o
    /// "ME GUSTA". `None` para omitirlo.
    pub kicker: Option<&'a str>,
    /// Línea descriptiva bajo el nombre (p. ej. el origen de una mezcla). `None` para omitirla.
    pub description: Option<&'a str>,
    pub track_count: usize,
    pub total_duration_seconds: i64,
    /// [playlist-color] Color de arranque del degradado; `None` usa el del tema.
    pub tint: Option<Color>,
}

/// Id del input de renombre, para darle foco al abrirlo.
pub const RENAME_INPUT_ID: &str = "playlist_rename_input";

/// Edición del nombre: doble click sobre el título para empezar, input
/// mientras se edita (Enter confirma).
pub enum TitleEdit<'a, Message> {
    Idle { on_double_click: Message },
    Editing { value: &'a str, on_input: fn(String) -> Message, on_submit: Message },
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
    cover: HeaderCover,
    on_play: Message,
    on_cover_click: Option<Message>,
    title_edit: Option<TitleEdit<'a, Message>>,
    is_playing: bool,
    corner: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let tint = data.tint.unwrap_or(theme().surface.gradient_start);
    let cover_element = match cover {
        HeaderCover::Single(handle) => single_cover(handle),
        HeaderCover::Mosaic(handles) if handles.len() >= 4 => cover_mosaic(handles),
        HeaderCover::Mosaic(handles) => single_cover(handles.into_iter().next().flatten()),
    };

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
            .color(theme().content.muted)
            .into(),
        None => space().height(0).into(),
    };

    let title_text = text(data.name)
        .font(SF_PRO)
        .size(typography::TEXT_40)
        .color(theme().content.primary);

    let title: Element<'a, Message> = match title_edit {
        None => title_text.into(),
        Some(TitleEdit::Idle { on_double_click }) => mouse_area(title_text).on_double_click(on_double_click).into(),
        Some(TitleEdit::Editing { value, on_input, on_submit }) => text_input("Nombre de la playlist", value)
            .id(Id::new(RENAME_INPUT_ID))
            .on_input(on_input)
            .on_submit(on_submit)
            .font(SF_PRO)
            .size(typography::TEXT_40)
            .padding(Padding { top: spacing::SP_0, bottom: spacing::SP_0, left: spacing::SP_8, right: spacing::SP_8 })
            .style(text_input_style::field)
            .into(),
    };

    let description: Element<'a, Message> = match data.description {
        Some(d) => text(d).font(SF_PRO).size(typography::TEXT_14).color(theme().content.secondary).into(),
        None => space().height(0).into(),
    };

    let metadata = text(format!(
        "{} · {}",
        format_track_count(data.track_count),
        format_total_duration(data.total_duration_seconds),
    ))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.muted);

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
        description,
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

    // Control opcional en la esquina superior derecha (p. ej. el selector de color).
    let content: Element<'a, Message> = match corner {
        Some(corner) => stack![
            content,
            container(corner)
                .width(Length::Fill)
                .align_x(Alignment::End)
                .padding(Padding { top: spacing::SP_16, right: spacing::SP_16, ..Default::default() }),
        ]
            .into(),
        None => content.into(),
    };

    container(content)
        .width(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(
                iced::gradient::Linear::new(std::f32::consts::PI * 1.5)
                    .add_stop(0.0, tint)
                    .add_stop(1.0, theme().surface.base)
                    .into(),
            ),
            ..Default::default()
        })
        .into()
}

/// Banner de una colección sin portada propia (Explorar, Me gusta): mosaico de
/// carátulas, conteo/duración de `tracks` y botón que reproduce todo o, si ya
/// suena algo de esta colección, pausa/reanuda.
pub fn collection_header<'a, Message: Clone + 'a>(
    kicker: &'a str,
    name: &'a str,
    tracks: &[&Track],
    mosaic: Vec<Option<Handle>>,
    is_current: bool,
    is_playing: bool,
    on_play_all: Message,
    on_toggle: Message,
) -> Element<'a, Message> {
    playlist_header(
        PlaylistHeaderData {
            name,
            kicker: Some(kicker),
            description: None,
            track_count: tracks.len(),
            total_duration_seconds: tracks.iter().map(|t| t.duration_seconds as i64).sum(),
            tint: None,
        },
        HeaderCover::Mosaic(mosaic),
        if is_current { on_toggle } else { on_play_all },
        None,
        None,
        is_current && is_playing,
        None,
    )
}

fn single_cover<'a, Message: Clone + 'a>(handle: Option<Handle>) -> Element<'a, Message> {
    let state = match handle {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    async_thumbnail(state, COVER_SIZE, COVER_RADIUS)
}

/// Mosaico 2×2 con las esquinas exteriores redondeadas como una portada normal.
fn cover_mosaic<'a, Message: 'a>(handles: Vec<Option<Handle>>) -> Element<'a, Message> {
    let half = COVER_SIZE / 2.0;
    let corners = [
        Radius::new(0.0).top_left(COVER_RADIUS),
        Radius::new(0.0).top_right(COVER_RADIUS),
        Radius::new(0.0).bottom_left(COVER_RADIUS),
        Radius::new(0.0).bottom_right(COVER_RADIUS),
    ];

    let mut tiles = handles.into_iter().zip(corners).map(|(handle, corner)| -> Element<'a, Message> {
        match handle {
            Some(handle) => image(handle)
                .width(Length::Fixed(half))
                .height(Length::Fixed(half))
                .content_fit(ContentFit::Cover)
                .border_radius(mirrored_for_image(corner))
                .into(),
            None => container(space())
                .width(Length::Fixed(half))
                .height(Length::Fixed(half))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(theme().surface.sunken.into()),
                    border: Border { radius: corner, ..Default::default() },
                    ..Default::default()
                })
                .into(),
        }
    });

    let mut next = || tiles.next().unwrap_or_else(|| space().into());
    column![row![next(), next()], row![next(), next()]].into()
}

/// El shader de imágenes de iced 0.14 (wgpu) aplica los radios por esquina en
/// espejo (el de arriba a la izquierda redondea abajo a la derecha, etc.); los
/// contenedores no tienen ese problema. Se invierte para que la esquina pedida
/// sea la que se redondea.
fn mirrored_for_image(radius: Radius) -> Radius {
    Radius {
        top_left: radius.bottom_right,
        top_right: radius.bottom_left,
        bottom_right: radius.top_left,
        bottom_left: radius.top_right,
    }
}
