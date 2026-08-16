use std::time::Instant;
use iced::animation::{Animation, Easing};

struct RowAnim {
    track_id: String,
    pub y: Animation<f32>,
}

/// Animador de posición Y por fila, keyed por track id — misma lógica que
/// `playback_feature::queue::animator::QueueAnimator`, pero parametrizado
/// por `row_height` en vez de importar una constante fija, para que
/// `TrackBuilder` (Explorer/Favorites/Playlist, cada uno con su propio
/// alto de fila) pueda reusarlo. Se duplica en vez de generalizar
/// `QueueAnimator` in-place para no tocar el drag de la cola, que ya
/// funciona.
pub struct RowAnimator {
    row_height: f32,
    anims: Vec<RowAnim>,
}

impl RowAnimator {
    pub fn new(row_height: f32) -> Self {
        Self { row_height, anims: Vec::new() }
    }

    /// True mientras alguna fila no ha llegado a su posición objetivo.
    pub fn is_animating(&self, now: Instant) -> bool {
        self.anims.iter().any(|a| a.y.is_animating(now))
    }

    /// Da de alta o recupera la animación de una fila por id de track,
    /// y actualiza su objetivo a la posición Y correspondiente a `index`.
    pub fn sync_target(&mut self, track_id: &str, index: usize, now: Instant) {
        let target_y = index as f32 * self.row_height;

        if let Some(anim) = self.anims.iter_mut().find(|a| a.track_id == track_id) {
            if (anim.y.value() - target_y).abs() > 0.01 {
                anim.y.go_mut(target_y, now);
            }
        } else {
            self.anims.push(RowAnim {
                track_id: track_id.to_string(),
                y: Animation::new(target_y).easing(Easing::EaseOut).quick(),
            });
        }
    }

    /// Sincroniza de golpe y sin transición visual (utilizado al soltar un drag interactivo).
    pub fn snap_to_target(&mut self, track_id: &str, index: usize) {
        let target_y = index as f32 * self.row_height;
        self.anims.retain(|a| a.track_id != track_id);
        self.anims.push(RowAnim {
            track_id: track_id.to_string(),
            y: Animation::new(target_y).easing(Easing::EaseOut).quick(),
        });
    }

    /// Devuelve la posición Y visual interpolada en el milisegundo actual.
    /// `default_index` es el índice a usar si la fila todavía no tiene
    /// animación registrada (primer frame tras iniciar un drag).
    pub fn visual_y_of(&self, track_id: &str, now: Instant, default_index: usize) -> f32 {
        self.anims
            .iter()
            .find(|a| a.track_id == track_id)
            .map(|a| a.y.interpolate_with(|y| y, now))
            .unwrap_or(default_index as f32 * self.row_height)
    }
}
