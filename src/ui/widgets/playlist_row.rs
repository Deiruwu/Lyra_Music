//! # PlaylistRow — fila de playlist (imagen + nombre) para el sidebar
//!
//! Pinta una fila de playlist estilo Spotify en dos variantes:
//! expandida (cover + nombre) y colapsada (solo cover centrado). Emite
//! los eventos de interacción (click / right-click) como `Message` ya
//! mapeados por el caller.
//!
//! Es un widget puramente presentacional — mismo contrato que
//! `playlist_header`: recibe un `Option<Handle>` ya resuelto y no conoce
//! ni `CoverManager`, ni disco, ni `Playlist`. El caller es dueño del
//! loop sobre `playlists_metadata()` y de resolver `cover_url ->
//! Option<Handle>`.

use iced::alignment::Horizontal;
use iced::widget::{button, column, container, mouse_area, row, space, stack, text};
use iced::widget::image::Handle;
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::styles::button as button_style;
use crate::ui::utils::playlist_metadata::{format_track_count, format_total_duration};
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};

/// Tamaños propios de la fila de playlist — más grande que una fila de
/// track (36px) pero mucho más chica que el header (176px). Constantes
/// propias, no reutilizadas.
const ROW_COVER_SIZE: f32 = 40.0;

/// Datos puramente informativos — mismo criterio que `PlaylistHeaderData`:
/// el widget no resuelve nada, solo pinta lo que le pasan.
pub struct PlaylistRowData<'a> {
    pub name: &'a str,
    pub is_active: bool,
    /// Cantidad de canciones de la playlist. Solo se muestra en la
    /// variante expandida; la colapsada lo ignora.
    pub track_count: usize,
    /// Duración total de la playlist en segundos. Solo se muestra en la
    /// variante expandida; la colapsada lo ignora.
    pub total_duration_seconds: i64,
}

/// `cover` ya resuelto por el caller (mismo patrón que `playlist_header`:
/// `Option<Handle>`, la vista es responsable de pedirlo al gestor de
/// portadas). `on_select`/`on_right_click` son mensajes ya mapeados al tipo
/// del caller.
pub fn playlist_row<'a, Message: Clone + 'a>(
    data: PlaylistRowData<'a>,
    cover: Option<Handle>,
    is_expanded: bool,
    on_select: Message,
    on_right_click: Message,
) -> Element<'a, Message> {
    let cover_state = match cover {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    // Recorte redondeado propio de la fila — a 40px un cuadrado sin
    // suavizar se percibe "duro"/tosco pegado al texto, sobre todo en
    // el sidebar angosto. Un radius chico (proporcional, mismo criterio
    // que el header) alivia eso sin necesitar tocar el widget de
    // pintado ni el pipeline de compresión del archivo en disco.
    const ROW_COVER_RADIUS: f32 = ROW_COVER_SIZE * 0.18;
    // Al quitar el hover en la variante colapsada, `row_color` deja de
    // pintar nada (antes solo coloreaba texto, que acá no existe). Sin
    // esto, colapsado no hay forma de ver cuál playlist está activa —
    // se agrega un borde sutil del mismo color de acento como único
    // indicador de selección para esa variante.
    let cover_element: Element<'a, Message> = container(stack![
        async_thumbnail(cover_state, ROW_COVER_SIZE, 6.0),
        // Molde "cortador" de hover: un botón de 40px encima de la imagen,
        // ceñido al mismo clip redondeado que la portada, para que el
        // oscurecimiento al pasar el mouse no "sangre" hacia el texto de
        // la fila ni las esquinas queden cuadradas. Aplica a ambas
        // variantes (expandida y colapsada), que comparten el mismo cover
        // de 40px.
        {
            let hover_button: Element<'a, Message> = button(
                container(space().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
                .width(Length::Fixed(ROW_COVER_SIZE))
                .height(Length::Fixed(ROW_COVER_SIZE))
                .padding(0)
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered => Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(bg.into()),
                        ..Default::default()
                    }
                })
                .on_press(on_select.clone())
                .into();
            hover_button
        },
    ])
        .width(Length::Fixed(ROW_COVER_SIZE))
        .height(Length::Fixed(ROW_COVER_SIZE))
        .clip(true)
        .style(move |_theme: &iced::Theme| container::Style {
            border: iced::Border {
                radius: ROW_COVER_RADIUS.into(),
                width: if data.is_active { 2.0 } else { 0.0 },
                color: Color::from_rgb(0.74, 0.58, 0.98),
            },
            ..Default::default()
        })
        .into();

    let row_color = if data.is_active {
        Color::from_rgb(0.74, 0.58, 0.98)
    } else {
        Color::WHITE
    };

    let content: Element<'a, Message> = if is_expanded {
        let name = text(data.name.to_string())
            .size(13)
            .font(SF_PRO)
            .color(row_color);

        let metadata = text(format!(
            "{} · {}",
            format_track_count(data.track_count),
            format_total_duration(data.total_duration_seconds),
        ))
            .size(11)
            .font(SF_PRO)
            .color(Color::from_rgb(0.5, 0.53, 0.6));

        let text_column = column![name, metadata].spacing(2).align_x(Alignment::Start);

        row![
            cover_element,
            space().width(10),
            text_column,
        ]
            .align_y(Alignment::Center)
            .into()
    } else {
        container(cover_element)
            .width(Length::Fill)
            .align_x(Horizontal::Center)
            .into()
    };

    if is_expanded {
        // Variante expandida: botón real con hover (estilo transparente
        // ya existente) — acá el hover tiene sentido porque hay texto +
        // suficiente área para que el feedback visual no se sienta
        // apretado.
        let row_button = button(content)
            .width(Length::Fill)
            .padding(Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 12.0 })
            .style(button_style::transparent)
            .on_press(on_select);

        mouse_area(row_button)
            .on_right_press(on_right_click)
            .into()
    } else {
        // Variante colapsada: el feedback de selección vive en el cover
        // de 40px (el molde de hover interno se oscurece al pasar el mouse,
        // y el estado activo se indica con el borde de acento). La capa
        // exterior es un container liso envuelto en mouse_area: área
        // clickeable de toda la celda + .on_right_press para el menú
        // contextual, sin highlight de fondo de fila que se vea "sucio".
        let clickable = container(content)
            .width(Length::Fill)
            .padding(Padding { top: 4.0, bottom: 4.0, left: 0.0, right: 0.0 });

        mouse_area(clickable)
            .on_press(on_select)
            .on_right_press(on_right_click)
            .into()
    }
}