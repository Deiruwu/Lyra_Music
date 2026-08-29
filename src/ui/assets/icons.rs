use std::time::{SystemTime, UNIX_EPOCH};
use iced::widget::text::{self, Shaping, Text};
use iced::{Renderer, Theme};
use strum_macros::{AsRefStr, IntoStaticStr};

use crate::ui::assets::fonts::JETBRAINS_MONO;

#[derive(Debug, Clone, Copy, AsRefStr, IntoStaticStr)]
pub enum Icon {
    // Playback
    #[strum(serialize = "")]
    Play,

    #[strum(serialize = "")]
    Pause,

    // Niveles del ecualizador animado (ver `Icon::equalizer_frame`).
    #[strum(serialize = "▂")]
    EqualizerLow,

    #[strum(serialize = "▅")]
    EqualizerMid,

    #[strum(serialize = "█")]
    EqualizerHigh,

    #[strum(serialize = "󰒮")]
    SkipPrevious,

    #[strum(serialize = "󰒭")]
    SkipNext,

    #[strum(serialize = "")]
    Shuffle,

    #[strum(serialize = "󰑖")]
    Repeat,

    #[strum(serialize = "󰑘")]
    RepeatOne,

    // Library
    #[strum(serialize = "")]
    Home,

    #[strum(serialize = "")]
    Explorer,

    #[strum(serialize = "󰐹")]
    Radio,

    #[strum(serialize = "")]
   Playlist,

    #[strum(serialize = "󰲸")]
    QueueMusic,

    #[strum(serialize = "")]
    AddQueue,

    #[strum(serialize = "󰐒")]
    AddQueueFront,

    #[strum(serialize = "")]
    Copiar,

    // Favorites
    #[strum(serialize = "")]
    HeartFull,

    #[strum(serialize = "")]
    Heart,

    #[strum(serialize = "󰋔")]
    HeartBroken,

    // Delete
    #[strum(serialize = "󰆴")]
    Delete,

    #[strum(serialize = "󰛌")]
    DeleteOpen,

    // Navigation
    #[strum(serialize = "")]
    ExpandLess,

    #[strum(serialize = "")]
    ExpandMore,

    #[strum(serialize = "")]
    LeftArrow,

    #[strum(serialize = "")]
    RightArrow,

    // Volume
    #[strum(serialize = "")]
    VolumeMuted,

    #[strum(serialize = "")]
    VolumeOff,

    #[strum(serialize = "")]
    VolumeDown,

    #[strum(serialize = "")]
    VolumeUp,

    #[strum(serialize = "")]
    Return,

    #[strum(serialize = "")]
    BurgerMenu,

    // Covers
    #[strum(serialize = "")]
    Camera,

    #[strum(serialize = "󰋩")]
    ImagePlaceholder,
}

impl Icon {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Fotograma actual del ecualizador animado.
    pub fn equalizer_frame() -> String {
        const LEVELS: [Icon; 3] = [Icon::EqualizerLow, Icon::EqualizerMid, Icon::EqualizerHigh];
        const BAR_COUNT: u128 = 5;
        const STEP_MS: u128 = 110;

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let step = millis / STEP_MS;
        let level_count = LEVELS.len() as u128;

        (0..BAR_COUNT)
            .map(|bar| LEVELS[((step + bar) % level_count) as usize].as_str())
            .collect()
    }
}

/// Único punto de renderizado de un `Icon`: fija la fuente Nerd Font y el
/// shaping avanzado que sus glifos PUA necesitan, para que sea imposible
/// construir un icono roto por olvidar uno de los dos.
pub fn icon<'a>(icon: Icon, size: f32) -> Text<'a, Theme, Renderer> {
    glyph(icon.as_str(), size)
}

/// Igual que [`icon`], para contenido de glifos que no es un único `Icon`
/// (p.ej. `Icon::equalizer_frame`).
pub fn glyph<'a>(s: impl text::IntoFragment<'a>, size: f32) -> Text<'a, Theme, Renderer> {
    Text::new(s)
        .font(JETBRAINS_MONO)
        .shaping(Shaping::Advanced)
        .size(size)
}