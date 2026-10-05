//! Píldora (ícono + texto) que sigue al cursor mientras se arrastra algo.

use iced::widget::{container, pin, row, text};
use iced::{Alignment, Element, Padding, Point, Theme};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::theme::theme;

/// Separación entre la punta del cursor y la píldora.
const CURSOR_OFFSET: Point = Point::new(14.0, 10.0);

/// Píldora posicionada junto a `cursor` (coordenadas de ventana).
pub fn drag_pill<'a, Message: 'a>(icon: Icon, label: &'a str, cursor: Point) -> Element<'a, Message> {
    let pill = container(
        row![
            icons::icon(icon, typography::TEXT_14).color(theme().accent.primary),
            text(label).font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary),
        ]
            .spacing(spacing::SP_8)
            .align_y(Alignment::Center),
    )
        .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_12, right: spacing::SP_12 })
        .style(|_theme: &Theme| container::Style {
            background: Some(theme().surface.control.into()),
            border: iced::border::rounded(radii::R_12).color(theme().border.subtle).width(1.0),
            shadow: theme().elevation.shadow,
            ..Default::default()
        });

    pin(pill).x(cursor.x + CURSOR_OFFSET.x).y(cursor.y + CURSOR_OFFSET.y).into()
}
