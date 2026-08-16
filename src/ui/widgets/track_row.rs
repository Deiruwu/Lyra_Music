use iced::{Alignment, ContentFit, Element, Length, Theme};
use iced::widget::{button, column, container, image, mouse_area, row, space, stack, text};
use iced::widget::image::{Handle};
use crate::model::Track;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::styles::styles::transparent_button;
use crate::JETBRAINS_MONO;
use crate::ui::assets::icons::Icon;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

pub fn track_thumbnail_sized<'a, Message>(thumbnail: Option<Handle>, size: f32) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let state = match thumbnail {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    async_thumbnail(state, size, 6.0)
}

pub fn track_thumbnail<'a, Message>(thumbnail: Option<Handle>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    track_thumbnail_sized(thumbnail, 60.0)
}

pub fn track_info<'a, Message>(track: &'a Track, width: Length) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let is_downloaded = track.file_path.as_ref().map_or(false, |p| !p.is_empty());

    let title = truncate(&track.title, 45);

    let album_name = track.album.as_ref().map_or("".to_string(), |album| album.name.clone());
    let artist_and_album = format!("{}\n{}", track.format_artists(), album_name);
    let subtitle = truncate(&artist_and_album, 55);

    let (title_color, artist_color) = if is_downloaded {
        (iced::Color::WHITE, iced::Color::from_rgb(0.6, 0.6, 0.6))
    } else {
        (iced::Color::from_rgb(0.4, 0.4, 0.4), iced::Color::from_rgb(0.3, 0.3, 0.3))
    };

    column![
        text(title)
            .size(14)
            .color(title_color)
            .width(width),
        text(subtitle)
            .size(11)
            .color(artist_color)
            .width(width),
    ]
        .align_x(Alignment::Start)
        .into()
}

pub fn basic_track_view<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    row![
        track_thumbnail(thumbnail),
        track_info(track, Length::Fixed(260.0))
    ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

pub fn track_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_press: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    button(basic_track_view(track, thumbnail))
        .width(Length::Fill)
        .on_press(on_press)
        .style(transparent_button)
        .into()
}

pub fn currently_playing_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    currently_playing_row_with_trailing(track, thumbnail, None)
}

/// Igual que `currently_playing_row`, pero acepta un elemento opcional
/// (`trailing`) que se agrega dentro del MISMO `row!`, con el mismo
/// spacing que el resto de la fila. Existe para casos como el botón de
/// like en el panel de reproducción: si se arma por fuera con otro
/// `row!` envolvente, queda visualmente despegado del track (spacing
/// del row exterior distinto al interno). Metiéndolo aquí adentro
/// queda pegado igual que thumbnail/info.
pub fn currently_playing_row_with_trailing<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    // Cuando hay trailing (p. ej. el like), `track_info` NO debe pedir
    // Fill: un `text(...).width(Fill)` se estira y absorbe todo el ancho
    // sobrante del row, empujando el trailing hasta el borde derecho del
    // panel en vez de dejarlo pegado justo después del texto. Con Shrink,
    // el texto ocupa solo lo que necesita (hasta su truncamiento normal)
    // y el trailing queda inmediatamente después, con el spacing normal
    // de la fila.
    let info_width = if trailing.is_some() { Length::Shrink } else { Length::Fill };

    let mut content = row![
        track_thumbnail(thumbnail),
        track_info(track, info_width)
    ]
        .spacing(12)
        .align_y(Alignment::Center);

    if let Some(trailing) = trailing {
        content = content.push(trailing);
    }

    content.into()
}

// ── Thumbnail con overlay ─────────────────────────────────────────────────────
//
// Nota: antes existía `QueueThumbnailState::Downloading(u8)`, que pintaba
// un SPINNER sobre la miniatura cuando la canción aún se estaba descargando
// pero ya aparecía en la cola. Se eliminó junto con su flujo (ver el
// comentario "FEAT FUTURO: canción en descarga visible en la cola" en
// queue_panel.rs) para reimplementarse como feature más adelante.

fn thumbnail_with_overlay<'a, Message>(
    thumbnail: Option<Handle>,
    on_play: Message,
    hovered: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let base: Element<'a, Message> = match thumbnail {
        Some(handle) => image(handle)
            .width(Length::Fixed(55.0))
            .height(Length::Fixed(55.0))
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(space().width(Length::Fixed(55.0)).height(Length::Fixed(55.0)))
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.2, 0.2, 0.2).into()),
                border: iced::border::rounded(5),
                ..Default::default()
            })
            .into(),
    };

    if hovered {
        let play_btn = button(
            container(
                text(Icon::Play.as_ref()).font(JETBRAINS_MONO).size(18).color(iced::Color::WHITE)
            )
                .width(Length::Fixed(55.0))
                .height(Length::Fixed(55.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
            .on_press(on_play)
            .width(Length::Fixed(55.0))
            .height(Length::Fixed(55.0))
            .padding(0)
            .style(|_: &Theme, _| button::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                ..Default::default()
            });

        stack![base, play_btn].into()
    } else {
        base
    }
}

fn drag_handle<'a, Message>(
    on_drag_start: Message,
    on_drag_release: Message,
    is_dragging: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let icon_color = if is_dragging {
        iced::Color::WHITE
    } else {
        iced::Color::from_rgb(0.45, 0.45, 0.45)
    };

    let handle = container(
        text("")
            .font(JETBRAINS_MONO)
            .size(16)
            .color(icon_color)
    )
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(44.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    mouse_area(handle)
        .on_press(on_drag_start)
        .on_release(on_drag_release)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

pub fn queue_track_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_play: Message,
    on_delete: Message,
    row_hovered: bool,
    delete_hovered: bool,
    on_delete_hover: Message,
    on_delete_leave: Message,
    drag: DragRowParams<Message>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let thumb = thumbnail_with_overlay(thumbnail, on_play, row_hovered);

    let info = track_info(track, Length::Fill);

    let delete_button = mouse_area(
        button(
            container(
                text(if delete_hovered { "󰛌" } else { "󰆴" })
                    .font(JETBRAINS_MONO)
                    .size(16)
            )
                .width(Length::Fixed(44.0))
                .height(Length::Fixed(44.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
            .on_press(on_delete)
            .style(transparent_button)
            .padding(0)
    )
        .on_enter(on_delete_hover)
        .on_exit(on_delete_leave);

    let handle = drag_handle(drag.on_drag_start, drag.on_drag_release, drag.is_dragging);

    let row_content = mouse_area(
        row![handle, thumb, info]
            .spacing(15)
            .align_y(Alignment::Center)
            .padding([8, 12])
            .width(Length::Fill)
    );

    let content = row![row_content, delete_button]
        .align_y(Alignment::Center)
        .width(Length::Fill);

    container(content)
        .width(Length::Fill)
        .style(move |_theme: &Theme| {
            if drag.is_dragging {
                container::Style {
                    background: Some(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                    border: iced::border::rounded(6)
                        .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.15))
                        .width(1.0),
                    ..Default::default()
                }
            } else {
                container::Style::default()
            }
        })
        .into()
}

pub struct DragRowParams<Message> {
    pub is_dragging: bool,
    pub on_drag_start: Message,
    pub on_drag_release: Message,
}