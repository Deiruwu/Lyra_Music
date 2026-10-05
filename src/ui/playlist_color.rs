//! # Color de playlist — `[playlist-color]`
//!
//! Cada playlist tiene un color (tono, saturación y brillo, modelo HSV). Si
//! nunca se eligió uno, el tono sale de un hash de su id (parece aleatorio
//! pero es estable entre sesiones) con saturación y brillo por defecto.
//! Del color elegido se derivan:
//! - `header_tint`: el mismo color con el brillo topado, para el degradado del
//!   header (así el texto blanco se sigue leyendo aunque elijas un color claro).
//! - `accent`: el mismo color con un brillo mínimo, para el ícono de la playlist
//!   en los menús (que no se pierda sobre el fondo oscuro).
//! - `swatch`: el color tal cual, para el selector.
//!
//! Los colores elegidos viven en un registro de proceso (como `theme()`), así
//! los menús contextuales los leen sin que haya que pasarlos por cada vista.
//!
//! ## Para quitar la función
//! 1. Buscar `[playlist-color]` en el código y deshacer cada punto marcado.
//! 2. Borrar este archivo, `ui/widgets/color_picker.rs` y `db/playlist_color_manager.rs`.
//! 3. Agregar una migración con `DROP TABLE playlist_color;` (la crean
//!    `0007_playlist_color.sql` y `0009_playlist_color_hsv.sql`).

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use iced::Color;

pub const DEFAULT_SATURATION: f32 = 0.45;
pub const DEFAULT_VALUE: f32 = 0.70;
/// Brillo máximo del degradado del header.
const HEADER_MAX_VALUE: f32 = 0.45;
/// Brillo mínimo del ícono en los menús.
const ACCENT_MIN_VALUE: f32 = 0.72;

/// Color en HSV: tono en grados (0-360), saturación y brillo en 0-1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaylistColor {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// Colores elegidos a mano, por id de playlist.
static CHOSEN: LazyLock<RwLock<HashMap<String, PlaylistColor>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

/// Color de la playlist: el elegido, o uno derivado de su id.
pub fn color_of(playlist_id: &str) -> PlaylistColor {
    CHOSEN
        .read()
        .ok()
        .and_then(|colors| colors.get(playlist_id).copied())
        .unwrap_or_else(|| PlaylistColor { hue: default_hue(playlist_id), saturation: DEFAULT_SATURATION, value: DEFAULT_VALUE })
}

pub fn set_color(playlist_id: &str, color: PlaylistColor) {
    if let Ok(mut colors) = CHOSEN.write() {
        colors.insert(playlist_id.to_string(), color);
    }
}

/// Reemplaza el registro con lo que hay persistido.
pub fn replace_all(colors: impl IntoIterator<Item = (String, PlaylistColor)>) {
    if let Ok(mut current) = CHOSEN.write() {
        *current = colors.into_iter().collect();
    }
}

/// Color de arranque del degradado del header.
pub fn header_tint(color: PlaylistColor) -> Color {
    hsv(color.hue, color.saturation, color.value.min(HEADER_MAX_VALUE))
}

/// Color legible sobre fondos oscuros (ícono en menús).
pub fn accent(color: PlaylistColor) -> Color {
    hsv(color.hue, color.saturation, color.value.max(ACCENT_MIN_VALUE))
}

/// El color elegido tal cual (muestra del selector).
pub fn swatch(color: PlaylistColor) -> Color {
    hsv(color.hue, color.saturation, color.value)
}

/// Hash FNV-1a del id llevado a grados.
fn default_hue(playlist_id: &str) -> f32 {
    let hash = playlist_id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3));
    (hash % 360) as f32
}

fn hsv(hue: f32, saturation: f32, value: f32) -> Color {
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    Color::from_rgb(r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_tono_por_defecto_es_estable() {
        assert_eq!(default_hue("playlist-a"), default_hue("playlist-a"));
        assert!((0.0..360.0).contains(&default_hue("playlist-a")));
    }

    #[test]
    fn el_color_elegido_reemplaza_al_derivado() {
        let chosen = PlaylistColor { hue: 200.0, saturation: 0.9, value: 0.8 };
        set_color("playlist-test-elegido", chosen);
        assert_eq!(color_of("playlist-test-elegido"), chosen);
    }

    #[test]
    fn hsv_rojo_puro() {
        let red = hsv(0.0, 1.0, 1.0);
        assert_eq!((red.r, red.g, red.b), (1.0, 0.0, 0.0));
    }

    #[test]
    fn el_header_topa_el_brillo() {
        let bright = PlaylistColor { hue: 60.0, saturation: 1.0, value: 1.0 };
        let tint = header_tint(bright);
        assert!(tint.r <= HEADER_MAX_VALUE + 1e-6 && tint.g <= HEADER_MAX_VALUE + 1e-6);
    }
}
