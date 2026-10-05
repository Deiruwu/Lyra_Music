//! # Paleta de portadas
//!
//! Saca el color predominante de una imagen (portada de playlist, carátulas del
//! mosaico de Explorar/Me gusta, portada de la canción en el modo teatro) y de él
//! deriva los tonos que usa la UI:
//! - `header_tint` / `header_tint_end`: degradado del header, apagado y mezclado
//!   con el panel para que combine con el resto de la app.
//! - `band_tint`: arranque de la banda bajo el header; el salto de tono respecto
//!   del final del header es la división (sin línea), al estilo de Spotify.
//! - `accent`: versión legible sobre fondos oscuros (ícono en menús, punto de
//!   color, resplandor y letra del modo teatro).
//! - `ambient`: fondo del modo teatro.
//!
//! El predominante se calcula sobre una miniatura chica: los píxeles se agrupan
//! por tono (cubetas de 30°) y cada uno pesa más cuanto más saturado y de brillo
//! medio sea, así gana el color que se ve y no un fondo negro o gris. Si la imagen
//! no tiene color (blanco y negro), se usa su promedio.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{LazyLock, RwLock};

use iced::Color;
use image::{GenericImageView, ImageReader};

use crate::ui::theme::theme;
use crate::ui::utils::color::lerp_color;

/// Lado de la miniatura sobre la que se cuentan los píxeles.
const SAMPLE_SIDE: u32 = 32;
const HUE_BUCKETS: usize = 12;
/// Debajo de esto (saturación o brillo) un píxel no aporta color.
const MIN_CHROMA: f32 = 0.12;
/// Si el peso con color es menor a esta fracción de los píxeles, la imagen se toma como gris.
const MIN_COLORFUL_SHARE: f32 = 0.05;

const HEADER_MAX_VALUE: f32 = 0.48;
const HEADER_MIN_VALUE: f32 = 0.26;
const HEADER_SATURATION_FACTOR: f32 = 0.85;
const HEADER_END_PANEL_MIX: f32 = 0.35;
const BAND_PANEL_MIX: f32 = 0.70;
const ACCENT_MIN_VALUE: f32 = 0.72;
const AMBIENT_MAX_VALUE: f32 = 0.40;
const FALLBACK_SATURATION: f32 = 0.45;
const FALLBACK_VALUE: f32 = 0.70;

/// Color predominante de cada playlist (de su portada), por id.
static PLAYLIST_COLORS: LazyLock<RwLock<HashMap<String, Color>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

/// Color base de la playlist: el de su portada o, sin portada, uno derivado del id.
pub fn playlist_color(playlist_id: &str) -> Color {
    PLAYLIST_COLORS
        .read()
        .ok()
        .and_then(|colors| colors.get(playlist_id).copied())
        .unwrap_or_else(|| fallback_color(playlist_id))
}

pub fn set_playlist_color(playlist_id: &str, color: Color) {
    if let Ok(mut colors) = PLAYLIST_COLORS.write() {
        colors.insert(playlist_id.to_string(), color);
    }
}

/// Color predominante de una imagen codificada (JPEG/PNG/...).
pub fn dominant_color(bytes: &[u8]) -> Option<Color> {
    let image = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let sample = image.thumbnail(SAMPLE_SIDE, SAMPLE_SIDE);

    let mut buckets = [(0.0_f32, [0.0_f32; 3]); HUE_BUCKETS];
    let mut average = [0.0_f32; 3];
    let mut pixels = 0.0_f32;

    for (_, _, pixel) in sample.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 128 {
            continue;
        }
        let rgb = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        pixels += 1.0;
        for (sum, channel) in average.iter_mut().zip(rgb) {
            *sum += channel;
        }

        let (hue, saturation, value) = to_hsv(rgb);
        if saturation < MIN_CHROMA || value < MIN_CHROMA {
            continue;
        }
        let weight = saturation * (1.0 - (value - 0.6).abs());
        let bucket = &mut buckets[((hue / 360.0) * HUE_BUCKETS as f32) as usize % HUE_BUCKETS];
        bucket.0 += weight;
        for (sum, channel) in bucket.1.iter_mut().zip(rgb) {
            *sum += channel * weight;
        }
    }

    if pixels == 0.0 {
        return None;
    }

    let (weight, sums) = buckets.iter().copied().fold((0.0, [0.0; 3]), |best, bucket| if bucket.0 > best.0 { bucket } else { best });
    let rgb = if weight >= MIN_COLORFUL_SHARE * pixels {
        sums.map(|sum| sum / weight)
    } else {
        average.map(|sum| sum / pixels)
    };
    Some(Color::from_rgb(rgb[0], rgb[1], rgb[2]))
}

/// El más colorido de varios (saturación × brillo), p. ej. entre las carátulas de un mosaico.
pub fn most_vivid(colors: impl IntoIterator<Item = Color>) -> Option<Color> {
    colors.into_iter().max_by(|a, b| vividness(*a).total_cmp(&vividness(*b)))
}

fn vividness(color: Color) -> f32 {
    let (_, saturation, value) = to_hsv([color.r, color.g, color.b]);
    saturation * value
}

/// Arriba del header: el color apagado, con el brillo acotado para que se lea el texto blanco.
pub fn header_tint(base: Color) -> Color {
    let (hue, saturation, value) = to_hsv([base.r, base.g, base.b]);
    hsv(hue, saturation * HEADER_SATURATION_FACTOR, value.clamp(HEADER_MIN_VALUE, HEADER_MAX_VALUE))
}

/// Abajo del header: mezclado con el fondo del panel.
pub fn header_tint_end(base: Color) -> Color {
    lerp_color(header_tint(base), theme().surface.panel, HEADER_END_PANEL_MIX)
}

/// Arranque de la banda bajo el header: mucho más mezclado con el panel.
pub fn band_tint(base: Color) -> Color {
    lerp_color(header_tint(base), theme().surface.panel, BAND_PANEL_MIX)
}

/// Legible sobre fondos oscuros (íconos, puntos de color, resplandor, letra).
pub fn accent(base: Color) -> Color {
    let (hue, saturation, value) = to_hsv([base.r, base.g, base.b]);
    hsv(hue, saturation, value.max(ACCENT_MIN_VALUE))
}

/// Fondo del modo teatro.
pub fn ambient(base: Color) -> Color {
    let (hue, saturation, value) = to_hsv([base.r, base.g, base.b]);
    hsv(hue, saturation * HEADER_SATURATION_FACTOR, value.min(AMBIENT_MAX_VALUE))
}

/// Tono estable derivado del id (hash FNV-1a), para lo que no tiene portada.
pub fn fallback_color(id: &str) -> Color {
    let hash = id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3));
    hsv((hash % 360) as f32, FALLBACK_SATURATION, FALLBACK_VALUE)
}

pub(crate) fn to_hsv([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

pub(crate) fn hsv(hue: f32, saturation: f32, value: f32) -> Color {
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
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    fn png(image: RgbImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(image).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
        bytes
    }

    #[test]
    fn gana_el_color_y_no_el_fondo_negro() {
        // 3/4 negro, 1/4 naranja: el predominante es el naranja.
        let image = RgbImage::from_fn(64, 64, |x, _| if x < 48 { Rgb([5, 5, 5]) } else { Rgb([230, 120, 20]) });
        let color = dominant_color(&png(image)).unwrap();
        assert!(color.r > color.g && color.g > color.b && color.r > 0.7);
    }

    #[test]
    fn una_imagen_gris_devuelve_su_promedio() {
        let color = dominant_color(&png(RgbImage::from_pixel(16, 16, Rgb([128, 128, 128])))).unwrap();
        assert!((color.r - color.g).abs() < 0.01 && (color.g - color.b).abs() < 0.01);
    }

    #[test]
    fn hsv_ida_y_vuelta() {
        let (h, s, v) = to_hsv([0.9, 0.45, 0.1]);
        let back = hsv(h, s, v);
        assert!((back.r - 0.9).abs() < 1e-4 && (back.g - 0.45).abs() < 1e-4 && (back.b - 0.1).abs() < 1e-4);
    }

    #[test]
    fn el_color_de_reserva_es_estable() {
        assert_eq!(fallback_color("playlist-a"), fallback_color("playlist-a"));
    }
}
