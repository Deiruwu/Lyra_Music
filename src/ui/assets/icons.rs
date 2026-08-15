use strum_macros::{AsRefStr, IntoStaticStr};

#[derive(Debug, Clone, Copy, AsRefStr, IntoStaticStr)]
pub enum Icon {
    // Playback
    #[strum(serialize = "")]
    Play,

    #[strum(serialize = "")]
    Pause,

    #[strum(serialize = "󰒮")]
    SkipPrevious,

    #[strum(serialize = "󰒭")]
    SkipNext,

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

    // Volume
    #[strum(serialize = "...")]
    VolumeOff,

    #[strum(serialize = "...")]
    VolumeDown,

    #[strum(serialize = "...")]
    VolumeUp,

    #[strum(serialize = "")]
    Return,

    #[strum(serialize = "")]
    BurgerMenu,

    // Covers
    #[strum(serialize = "")]
    Camera,
}

impl Icon {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}