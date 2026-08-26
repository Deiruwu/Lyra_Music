use iced::Color;

use crate::ui::assets::typography;
use crate::ui::theme::theme;

/// Tamaño y color de una línea de letra según su peso de animación en `[0, 1]`.
///
/// La mezcla es cuadrática, no un lerp lineal entre los dos extremos:
/// `gray` depende de `weight` y vuelve a multiplicarse por él. Sustituirla
/// por una interpolación lineal cambiaría los colores intermedios, así que
/// la expresión se conserva tal cual estaba en `lyrics_panel`.
pub fn lyric_line(weight: f32) -> (f32, Color) {
    let size = typography::TEXT_16 + (typography::TEXT_22 - typography::TEXT_16) * weight;

    let gray = theme().content.muted_alt.r - 0.05 * weight;
    let color = Color::from_rgb(
        gray + (1.0 - gray) * weight,
        gray + (1.0 - gray) * weight,
        (gray + 0.05) + (1.0 - (gray + 0.05)) * weight,
    );

    (size, color)
}
