pub mod atelier;
pub mod palette;
pub mod semantic;

use std::collections::HashMap;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{LazyLock, Mutex};

use iced::Color;

pub use semantic::Semantic;

/// Tema con el acento de fábrica (violeta).
static DEFAULT: Semantic = atelier::ATELIER;

/// Tema activo. Siempre apunta a un `Semantic` que vive para siempre: el de
/// fábrica o uno derivado de un acento elegido (ver `set_accent`).
static ACTIVE: AtomicPtr<Semantic> = AtomicPtr::new(std::ptr::addr_of!(DEFAULT) as *mut Semantic);

/// Temas ya derivados, por acento (RGB de 8 bits). Nunca se liberan: los estilos
/// del árbol de widgets guardan `&'static Semantic`, así que reutilizar uno por
/// color acota lo que queda en memoria a los colores que de verdad se probaron.
static DERIVED: LazyLock<Mutex<HashMap<[u8; 3], &'static Semantic>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cuánto se aclara el acento hacia el blanco para el hover.
const HOVER_TOWARD_WHITE: f32 = 0.27;
/// Cuánto se le resta a cada canal para los tonos fuertes.
const STRONG_DROP: f32 = 0.13;
const STRONG_HOVER_DROP: f32 = 0.07;
/// Fracción del acento que queda en el arranque del degradado de los headers.
const GRADIENT_START_SCALE: f32 = 0.34;
const HOVER_ACCENT_ALPHA: f32 = 0.14;
const SELECTED_ALPHA: f32 = 0.20;
/// Debajo de esta luminancia el texto sobre el acento pasa a blanco.
const DARK_ACCENT_LUMINANCE: f32 = 0.45;

/// Tema activo de la aplicación.
pub fn theme() -> &'static Semantic {
    // SAFETY: `ACTIVE` solo guarda punteros a `DEFAULT` o a temas filtrados con
    // `Box::leak`, que viven hasta el final del programa.
    unsafe { &*ACTIVE.load(Ordering::Acquire) }
}

/// Acento de fábrica.
pub fn default_accent() -> Color {
    DEFAULT.accent.primary
}

/// Cambia el acento de toda la app y deriva de él los demás tonos de acento.
pub fn set_accent(accent: Color) {
    let key = rgb8(accent);
    let semantic: &'static Semantic = if key == rgb8(default_accent()) {
        &DEFAULT
    } else {
        let mut derived = DERIVED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        derived.entry(key).or_insert_with(|| Box::leak(Box::new(derive(accent))))
    };
    ACTIVE.store(semantic as *const Semantic as *mut Semantic, Ordering::Release);
}

/// El tema de fábrica con el acento y sus derivados cambiados.
fn derive(accent: Color) -> Semantic {
    let mut semantic = atelier::ATELIER;
    let shifted = |drop: f32| Color::from_rgb((accent.r - drop).max(0.0), (accent.g - drop).max(0.0), (accent.b - drop).max(0.0));
    let with_alpha = |a: f32| Color { a, ..accent };

    semantic.accent.primary = accent;
    semantic.accent.hover = Color::from_rgb(
        accent.r + (1.0 - accent.r) * HOVER_TOWARD_WHITE,
        accent.g + (1.0 - accent.g) * HOVER_TOWARD_WHITE,
        accent.b + (1.0 - accent.b) * HOVER_TOWARD_WHITE,
    );
    semantic.accent.strong = shifted(STRONG_DROP);
    semantic.accent.strong_hover = shifted(STRONG_HOVER_DROP);
    semantic.overlay.hover_accent = with_alpha(HOVER_ACCENT_ALPHA);
    semantic.overlay.selected = with_alpha(SELECTED_ALPHA);
    semantic.surface.gradient_start =
        Color::from_rgb(accent.r * GRADIENT_START_SCALE, accent.g * GRADIENT_START_SCALE, accent.b * GRADIENT_START_SCALE);

    let luminance = 0.2126 * accent.r + 0.7152 * accent.g + 0.0722 * accent.b;
    semantic.content.on_accent = if luminance < DARK_ACCENT_LUMINANCE { Color::WHITE } else { Color::BLACK };
    semantic
}

fn rgb8(color: Color) -> [u8; 3] {
    let [r, g, b, _] = color.into_rgba8();
    [r, g, b]
}

/// `#rrggbb` del color.
pub fn to_hex(color: Color) -> String {
    let [r, g, b] = rgb8(color);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Color de un `#rrggbb` (o `rrggbb`); `None` si no lo es.
pub fn from_hex(hex: &str) -> Option<Color> {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some(Color::from_rgb8(channel(0)?, channel(2)?, channel(4)?))
}

#[cfg(test)]
mod tests;
