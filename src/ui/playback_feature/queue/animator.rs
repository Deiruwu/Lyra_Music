use std::collections::HashSet;
use std::time::Instant;
use iced::animation::{Animation, Easing};
use super::queue_panel::ROW_STRIDE;

struct RowAnim {
    track_id: String,
    pub y: Animation<f32>,
}

#[derive(Default)]
pub struct QueueAnimator {
    anims: Vec<RowAnim>,
}

impl QueueAnimator {
    /// True mientras alguna fila no ha llegado a su posición objetivo.
    pub fn is_animating(&self, now: Instant) -> bool {
        self.anims.iter().any(|a| a.y.is_animating(now))
    }

    pub fn clear(&mut self) {
        self.anims.clear();
    }

    /// Retiene únicamente las animaciones cuyos IDs siguen existiendo en la cola (búsqueda O(1)).
    pub fn retain_valid_ids(&mut self, valid_ids: &HashSet<String>) {
        self.anims.retain(|anim| valid_ids.contains(&anim.track_id));
    }

    /// Da de alta o recupera la animación de una fila por id de track,
    /// y actualiza su objetivo a la posición Y correspondiente a `index`.
    pub fn sync_target(&mut self, track_id: &str, index: usize, now: Instant) {
        let target_y = index as f32 * ROW_STRIDE;

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
        let target_y = index as f32 * ROW_STRIDE;
        self.anims.retain(|a| a.track_id != track_id);
        self.anims.push(RowAnim {
            track_id: track_id.to_string(),
            y: Animation::new(target_y).easing(Easing::EaseOut).quick(),
        });
    }

    /// Devuelve la posición Y visual interpolada en el milisegundo actual.
    pub fn visual_y_of(&self, track_id: &str, now: Instant) -> f32 {
        self.anims
            .iter()
            .find(|a| a.track_id == track_id)
            .map(|a| a.y.interpolate_with(|y| y, now))
            .unwrap_or(0.0)
    }

    /// Calcula la altura dinámica del contenedor para evitar el clipping del scrollable
    /// cuando las filas inferiores se están animando y ascendiendo hacia su nuevo índice.
    pub fn calculate_dynamic_height(&self, item_count: usize, now: Instant) -> f32 {
        let base_height = item_count as f32 * ROW_STRIDE;
        if item_count == 0 {
            return 0.0;
        }

        let max_visual_y = self
            .anims
            .iter()
            .map(|a| a.y.interpolate_with(|y| y, now))
            .fold(0.0_f32, f32::max);

        let dynamic_height = max_visual_y + ROW_STRIDE;
        base_height.max(dynamic_height).max(1.0)
    }
}