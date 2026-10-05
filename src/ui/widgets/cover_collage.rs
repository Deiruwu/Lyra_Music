//! Portada armada con las portadas de varias playlists: una sola, mitades, una
//! grande y dos chicas, cuadrícula... según cuántas haya (hasta `MAX_TILES`).

use iced::border::Radius;
use iced::widget::image::Handle;
use iced::widget::{column, container, image, row};
use iced::{Border, Color, ContentFit, Element, Length, Theme};

use crate::ui::assets::icons::{self, Icon};
use crate::ui::theme::theme;
use crate::ui::widgets::playlist_header::mirrored_for_image;

pub const MAX_TILES: usize = 9;
/// Separación entre portadas.
const GAP: f32 = 2.0;
/// Lado del ícono de playlist respecto del lado menor de un recuadro sin portada.
const PLACEHOLDER_ICON_RATIO: f32 = 0.32;
const PLACEHOLDER_ICON_ALPHA: f32 = 0.55;

/// Un recuadro: la portada, o el color de la playlist si no tiene.
#[derive(Debug, Clone)]
pub struct CollageTile {
    pub handle: Option<Handle>,
    pub color: Color,
}

/// Cuántas portadas va en cada columna, de izquierda a derecha.
fn columns_for(count: usize) -> &'static [usize] {
    match count {
        0 => &[],
        1 => &[1],
        2 => &[1, 1],
        3 => &[1, 2],
        4 => &[2, 2],
        5 => &[2, 3],
        6 => &[3, 3],
        7 => &[2, 2, 3],
        8 => &[2, 3, 3],
        _ => &[3, 3, 3],
    }
}

/// Collage cuadrado de lado `size` con las esquinas exteriores redondeadas a `radius`.
pub fn cover_collage<'a, Message: 'a>(tiles: Vec<CollageTile>, size: f32, radius: f32) -> Element<'a, Message> {
    if tiles.is_empty() {
        let empty = CollageTile { handle: None, color: theme().surface.sunken };
        return cover_tile(empty, size, size, Radius::new(radius));
    }

    let layout = columns_for(tiles.len().min(MAX_TILES));
    let column_width = (size - GAP * (layout.len() - 1) as f32) / layout.len() as f32;
    let mut tiles = tiles.into_iter();
    let last_column = layout.len() - 1;

    let columns = layout.iter().enumerate().map(|(column_index, &count)| -> Element<'a, Message> {
        let tile_height = (size - GAP * (count - 1) as f32) / count as f32;
        let cells = (0..count).filter_map(|row_index| {
            let tile = tiles.next()?;
            let (top, bottom) = (row_index == 0, row_index == count - 1);
            let (left, right) = (column_index == 0, column_index == last_column);
            let corner = Radius {
                top_left: if top && left { radius } else { 0.0 },
                top_right: if top && right { radius } else { 0.0 },
                bottom_left: if bottom && left { radius } else { 0.0 },
                bottom_right: if bottom && right { radius } else { 0.0 },
            };
            Some(cover_tile(tile, column_width, tile_height, corner))
        });
        column(cells).spacing(GAP).into()
    });

    row(columns.collect::<Vec<_>>()).spacing(GAP).into()
}

/// Un recuadro de `width`×`height`: la portada recortada, o el color con el ícono de playlist.
pub fn cover_tile<'a, Message: 'a>(tile: CollageTile, width: f32, height: f32, corner: Radius) -> Element<'a, Message> {
    match tile.handle {
        Some(handle) => image(handle)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .content_fit(ContentFit::Cover)
            .border_radius(mirrored_for_image(corner))
            .into(),
        None => {
            let icon_color = Color { a: PLACEHOLDER_ICON_ALPHA, ..Color::WHITE };
            let color = tile.color;
            container(icons::icon(Icon::Playlist, width.min(height) * PLACEHOLDER_ICON_RATIO).color(icon_color))
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .center_x(Length::Fixed(width))
                .center_y(Length::Fixed(height))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(color.into()),
                    border: Border { radius: corner, ..Default::default() },
                    ..Default::default()
                })
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_cantidad_reparte_todas_sus_portadas() {
        for count in 1..=MAX_TILES {
            assert_eq!(columns_for(count).iter().sum::<usize>(), count);
        }
    }
}
