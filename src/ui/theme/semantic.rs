use iced::Color;

pub struct Semantic {
    pub surface: Surface,
    pub content: Content,
    pub border: Border,
    pub accent: Accent,
    pub overlay: Overlay,
    pub elevation: Elevation,
    pub status: Status,
}

/// Escala de elevación, de lo más hundido a lo más prominente.
pub struct Surface {
    pub base: Color,
    pub panel: Color,
    pub sunken: Color,
    pub control: Color,
    pub gradient_start: Color,
}

/// Escalera de texto: el énfasis baja de `primary` a `faint`.
pub struct Content {
    pub primary: Color,
    pub secondary: Color,
    pub muted: Color,
    pub faint: Color,
    pub on_accent: Color,
    pub on_banner: Color,
    pub on_control_disabled: Color,
}

pub struct Border {
    pub subtle: Color,
    pub field: Color,
}

pub struct Accent {
    pub primary: Color,
    pub hover: Color,
    pub strong: Color,
    pub strong_hover: Color,
}

/// Veladuras de estado. `hover` es efímero y blanco; `selected` persiste y va
/// teñido de acento, para que los dos estados no compitan en el mismo canal.
pub struct Overlay {
    pub hover: Color,
    pub hover_accent: Color,
    pub selected: Color,
    pub resting: Color,
    pub control_idle: Color,
    pub control_hover: Color,
    pub toggle_on_idle: Color,
    pub toggle_on_hover: Color,
    pub scrim: Color,
}

pub struct Elevation {
    pub shadow: iced::Shadow,
}

pub struct Status {
    pub liked: Color,
    pub cached: Color,
    pub error: Color,
}
