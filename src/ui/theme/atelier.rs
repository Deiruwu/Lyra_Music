use crate::ui::assets::spacing;

use super::palette::*;
use super::semantic::*;

pub const ATELIER: Semantic = Semantic {
    surface: Surface {
        base: STEEL_10,
        panel: STEEL_14,
        sunken: STEEL_18,
        control: STEEL_24,
        gradient_start: VIOLET_22,
    },
    content: Content {
        primary: STEEL_90,
        secondary: STEEL_74,
        muted: STEEL_56,
        faint: STEEL_40,
        on_accent: BLACK,
        on_banner: WHITE_A75,
        on_control_disabled: WHITE_A30,
    },
    border: Border {
        subtle: WHITE_A12,
        field: STEEL_30,
    },
    accent: Accent {
        primary: VIOLET_62,
        hover: VIOLET_72,
        strong: VIOLET_49,
        strong_hover: VIOLET_55,
    },
    overlay: Overlay {
        hover: WHITE_A06,
        hover_accent: VIOLET_A14,
        selected: VIOLET_A20,
        resting: WHITE_A03,
        control_idle: WHITE_A12,
        control_hover: WHITE_A16,
        toggle_on_idle: WHITE_A22,
        toggle_on_hover: WHITE_A28,
        scrim: BLACK_A55,
    },
    elevation: Elevation {
        shadow: iced::Shadow {
            color: BLACK_A45,
            offset: iced::Vector::new(0.0, spacing::SP_8),
            blur_radius: spacing::SP_32,
        },
    },
    status: Status {
        liked: ROSE_64,
        cached: TEAL_57,
        error: CLAY_57,
    },
};
