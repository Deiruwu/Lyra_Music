//! Fondo del modo teatro: la portada de la canción actual desenfocada y el
//! color predominante para el resplandor de la carátula, con un fundido al
//! cambiar de canción.

use std::io::Cursor;
use std::time::{Duration, Instant};

use iced::animation::{Animation, Easing};
use iced::widget::image::Handle;
use iced::Color;
use image::imageops::{self, FilterType};
use image::ImageReader;

use crate::ui::cover_palette;
use crate::ui::theme::theme;
use crate::ui::utils::color::lerp_color;

const TRANSITION: Duration = Duration::from_millis(900);
/// Lado de la miniatura que se desenfoca; la GPU la estira al tamaño del panel.
const BACKDROP_SIDE: u32 = 48;
const BACKDROP_BLUR_SIGMA: f32 = 3.5;
/// Opacidad del resplandor de la carátula.
const GLOW_ALPHA: f32 = 0.45;

/// Portada desenfocada lista para pintar de fondo.
pub fn blurred_backdrop(bytes: &[u8]) -> Option<Handle> {
    let image = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let small = image.resize_exact(BACKDROP_SIDE, BACKDROP_SIDE, FilterType::Triangle).to_rgba8();
    let blurred = imageops::blur(&small, BACKDROP_BLUR_SIGMA);
    Some(Handle::from_rgba(blurred.width(), blurred.height(), blurred.into_raw()))
}

/// Tonos que pinta el teatro en un instante.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TheaterTones {
    /// Fondo detrás de la portada desenfocada (y el único fondo si no hay portada).
    pub ambient: Color,
    /// Sombra de color alrededor de la carátula.
    pub glow: Color,
}

impl TheaterTones {
    /// Tonos de una portada, o los neutros del panel si no hay color.
    fn from_base(base: Option<Color>) -> Self {
        match base {
            Some(base) => Self {
                ambient: cover_palette::ambient(base),
                glow: Color { a: GLOW_ALPHA, ..cover_palette::accent(base) },
            },
            None => Self { ambient: theme().surface.panel, glow: theme().elevation.shadow.color },
        }
    }

    fn lerp(self, other: Self, t: f32) -> Self {
        Self { ambient: lerp_color(self.ambient, other.ambient, t), glow: lerp_color(self.glow, other.glow, t) }
    }
}

/// Fondo animado: funde la portada y los tonos anteriores con los de la canción nueva.
pub struct TheaterBackdrop {
    current: Option<Handle>,
    previous: Option<Handle>,
    from: TheaterTones,
    to: TheaterTones,
    progress: Animation<f32>,
}

impl Default for TheaterBackdrop {
    fn default() -> Self {
        let neutral = TheaterTones::from_base(None);
        Self { current: None, previous: None, from: neutral, to: neutral, progress: Animation::new(1.0) }
    }
}

impl TheaterBackdrop {
    /// Arranca el fundido hacia la portada `image` y su color `base`, desde lo que se ve ahora.
    pub fn set(&mut self, image: Option<Handle>, base: Option<Color>, now: Instant) {
        self.from = self.tones(now);
        self.to = TheaterTones::from_base(base);
        self.previous = self.current.take();
        self.current = image;
        self.progress = Animation::new(0.0).easing(Easing::EaseInOut).duration(TRANSITION).go(1.0, now);
    }

    pub fn tones(&self, now: Instant) -> TheaterTones {
        self.from.lerp(self.to, self.progress(now))
    }

    /// Avance del fundido, de 0 a 1.
    pub fn progress(&self, now: Instant) -> f32 {
        self.progress.interpolate_with(|t| t, now)
    }

    pub fn current(&self) -> Option<&Handle> {
        self.current.as_ref()
    }

    pub fn previous(&self) -> Option<&Handle> {
        self.previous.as_ref()
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.progress.is_animating(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurs_a_cover_down_to_the_backdrop_side() {
        let cover = image::RgbImage::from_fn(200, 200, |x, _| if x < 100 { image::Rgb([220, 40, 40]) } else { image::Rgb([30, 30, 200]) });
        let mut png = Vec::new();
        cover.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();

        let handle = blurred_backdrop(&png).expect("backdrop");
        assert!(matches!(handle, Handle::Rgba { width: BACKDROP_SIDE, height: BACKDROP_SIDE, .. }));
        assert!(blurred_backdrop(&[]).is_none());
    }
}
