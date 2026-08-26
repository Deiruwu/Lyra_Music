use iced::Color;

// Los sufijos `_alt` marcan deriva: valores que juegan el mismo rol que el
// token base pero que hoy difieren unas centésimas. Se conservan distintos
// para no alterar la apariencia; unificarlos es una decisión de diseño y
// consiste en apuntarlos al mismo color de la paleta.

pub struct Semantic {
    pub background: Background,
    pub surface: Surface,
    pub content: Content,
    pub border: Border,
    pub accent: Accent,
    pub overlay: Overlay,
    pub status: Status,
}

pub struct Background {
    pub app: Color,
    pub surface: Color,
}

pub struct Surface {
    pub elevated: Color,
    pub panel: Color,
    pub raised: Color,
    pub field: Color,
    pub control: Color,
    pub placeholder: Color,
    pub gradient_start: Color,
    pub gradient_end: Color,
}

pub struct Content {
    pub primary: Color,
    pub primary_alt: Color,
    pub active: Color,
    pub secondary: Color,
    pub secondary_alt: Color,
    pub secondary_alt2: Color,
    pub tertiary: Color,
    pub tertiary_alt: Color,
    pub tertiary_alt2: Color,
    pub muted: Color,
    pub muted_alt: Color,
    pub muted_alt2: Color,
    pub faint: Color,
    pub faint_alt: Color,
    pub disabled: Color,
    pub disabled_alt: Color,
    pub disabled_alt2: Color,
    pub on_accent: Color,
    pub on_banner: Color,
    pub on_control_disabled: Color,
}

pub struct Border {
    pub subtle: Color,
    pub field: Color,
    pub drag: Color,
}

pub struct Accent {
    pub primary: Color,
    pub hover: Color,
    pub strong: Color,
    pub strong_hover: Color,
    pub control_active: Color,
    pub tint: Color,
}

pub struct Overlay {
    pub hover_subtle: Color,
    pub hover_row: Color,
    pub hover_item: Color,
    pub selected: Color,
    pub card_idle: Color,
    pub card_hover: Color,
    pub card_border_idle: Color,
    pub card_border_hover: Color,
    pub control_idle: Color,
    pub control_hover: Color,
    pub control_disabled: Color,
    pub toggle_idle: Color,
    pub toggle_hover: Color,
    pub toggle_on_idle: Color,
    pub toggle_on_hover: Color,
    pub scrim_cover: Color,
    pub scrim_strong: Color,
    pub scrim_play: Color,
    pub shadow: Color,
}

pub struct Status {
    pub liked: Color,
    pub cached: Color,
    pub error: Color,
}
