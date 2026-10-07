//! Virtualización de listas verticales para iced.
//!
//! iced no trae una lista virtualizada nativa: `scrollable` siempre
//! recibe TODO el contenido y solo recorta visualmente lo que no cabe
//! en el viewport, pero igual construye/mide cada widget. Con miles de
//! tracks eso es carísimo.
//!
//! Este módulo NO renderiza nada por sí mismo — es puramente el cálculo
//! de "qué rango de índices debo construir ahora mismo", reusable para
//! el Explorer, playlists, colas, o cualquier lista homogénea de altura
//! de fila fija.
//!
//! ## Cómo funciona
//!
//! 1. Todas las filas tienen la misma altura fija (`row_height`). Esto es
//!    lo que hace el cálculo O(1) en vez de tener que medir contenido.
//! 2. Envuelves tu `column![...]` (con SOLO las filas visibles + buffer)
//!    en un `scrollable`, y le agregas ARRIBA y ABAJO un `space()` cuya
//!    altura simula el contenido no renderizado. Así el scrollbar se ve
//!    y se comporta como si las 10,000 filas existieran, aunque solo
//!    ~40-80 widgets reales estén vivos en el árbol de iced.
//! 3. Escuchas `.on_scroll(Message)` en el `scrollable`, guardas el
//!    `AbsoluteOffset` (o el `RelativeOffset`, tú decides), y en tu
//!    `view()` llamas `VirtualWindow::compute(...)` para saber el rango
//!    a construir en ese frame.
//!
//! Es exactamente la misma idea que ya usaste para centrar la letra
//! actual en lyrics (n-20..n+20), generalizada a "n" = índice derivado
//! del scroll en vez de la canción actual.
//!
//! ## Ejemplo de uso en un `view()`
//!
//! ```ignore
//! let window = VirtualWindow::compute(
//!     self.scroll_offset_y,   // f32, en píxeles, guardado desde on_scroll
//!     self.viewport_height,   // f32, alto visible guardado desde on_scroll
//!     ROW_HEIGHT,
//!     total_items,
//!     BUFFER_ROWS,            // p. ej. 10-20
//! );
//!
//! let mut rows = column![];
//! rows = rows.push(space().height(window.top_spacer_height(ROW_HEIGHT)));
//! for idx in window.start..window.end {
//!     rows = rows.push(render_row(idx, &items[idx]));
//! }
//! rows = rows.push(space().height(window.bottom_spacer_height(ROW_HEIGHT, total_items)));
//!
//! scrollable(rows).on_scroll(Message::Scrolled).into()
//! ```
//!
//! ## Nota sobre `on_scroll`
//!
//! `iced::widget::scrollable::Viewport` te da `.absolute_offset()` (en
//! píxeles, lo que quieres aquí) y `.bounds()` (tamaño del viewport, para
//! saber cuántas filas caben). Guarda ambos en tu estado desde el
//! callback de `on_scroll` y pásalos a `compute`.

/// Resultado del cálculo de ventana visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualWindow {
    /// Índice del primer item a renderizar (inclusive).
    pub start: usize,
    /// Índice del último item a renderizar (exclusivo).
    pub end: usize,
}

impl VirtualWindow {
    /// Calcula el rango `[start, end)` a renderizar.
    ///
    /// - `scroll_offset_y`: offset absoluto en píxeles (0 = arriba del todo).
    /// - `viewport_height`: alto visible del scrollable, en píxeles.
    /// - `row_height`: alto fijo de cada fila, en píxeles.
    /// - `total_items`: cuántos items existen en total en la lista lógica.
    /// - `buffer_rows`: cuántas filas extra renderizar arriba Y abajo de
    ///   lo estrictamente visible (recomendado 10-20 para scroll suave
    ///   sin que se vea "parpadear" contenido en blanco al scrollear rápido).
    pub fn compute(
        scroll_offset_y: f32,
        viewport_height: f32,
        row_height: f32,
        total_items: usize,
        buffer_rows: usize,
    ) -> Self {
        if total_items == 0 || row_height <= 0.0 {
            return Self { start: 0, end: 0 };
        }

        let first_visible = (scroll_offset_y / row_height).floor().max(0.0) as usize;
        let visible_rows = (viewport_height / row_height).ceil() as usize + 1;

        let start = first_visible.saturating_sub(buffer_rows);
        let end = (first_visible + visible_rows + buffer_rows).min(total_items);

        // Si por redondeo start > end (viewport_height == 0 antes del primer
        // layout, por ejemplo), devolvemos una ventana vacía en vez de panic.
        if start >= end {
            return Self { start: 0, end: 0 };
        }

        Self { start, end }
    }

    /// Alto en píxeles del `space()` que va ANTES de las filas
    /// renderizadas, para que el scrollbar mida lo mismo que si todas
    /// las filas existieran.
    pub fn top_spacer_height(&self, row_height: f32) -> f32 {
        self.start as f32 * row_height
    }

    /// Alto en píxeles del `space()` que va DESPUÉS de las filas
    /// renderizadas.
    pub fn bottom_spacer_height(&self, row_height: f32, total_items: usize) -> f32 {
        let remaining = total_items.saturating_sub(self.end);
        remaining as f32 * row_height
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

/// Estado mínimo que tu vista necesita guardar para poder llamar a
/// `VirtualWindow::compute` en cada `view()`. No incluye row_height ni
/// buffer porque esos normalmente son constantes de tu vista, no estado
/// dinámico.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollTracker {
    pub offset_y: f32,
    pub viewport_height: f32,
    pub viewport_width: f32,
}

/// Ancho aproximado, en píxeles, de la franja donde vive el scrollbar
/// nativo de iced (thumb + su padding lateral). Se usa para distinguir
/// un click sobre el contenido real de la lista de un click sobre el
/// scrollbar, ya que ambos son reportados por el mismo `mouse_area`
/// que envuelve el `scrollable`.
pub const SCROLLBAR_MARGIN_PX: f32 = 16.0;

impl ScrollTracker {

    pub fn reset(&mut self) {
        self.offset_y = 0.0;
    }
    /// Filas de `row_pitch` que caben completas en el viewport (mínimo 1).
    pub fn rows_per_page(&self, row_pitch: f32) -> usize {
        ((self.viewport_height / row_pitch).floor() as usize).max(1)
    }

    /// Scrollea lo justo para que la fila `[row_top, row_top + row_height]`
    /// quede visible en el scrollable `scrollable_id`.
    pub fn reveal<Message>(&mut self, row_top: f32, row_height: f32, scrollable_id: &'static str) -> iced::Task<Message> {
        if self.viewport_height <= 0.0 {
            return iced::Task::none();
        }

        match self.offset_to_reveal(row_top, row_height) {
            Some(target_offset) => self.scroll_to(target_offset, scrollable_id),
            None => iced::Task::none(),
        }
    }

    /// Desplaza `shift` px (una página de filas) sin pasarse de `content_height`,
    /// y después asegura que la fila quede visible.
    pub fn page_and_reveal<Message>(
        &mut self,
        shift: f32,
        content_height: f32,
        row_top: f32,
        row_height: f32,
        scrollable_id: &'static str,
    ) -> iced::Task<Message> {
        if self.viewport_height <= 0.0 {
            return iced::Task::none();
        }

        let max_offset = (content_height - self.viewport_height).max(0.0);
        self.offset_y = (self.offset_y + shift).clamp(0.0, max_offset);

        let target_offset = self.offset_to_reveal(row_top, row_height).unwrap_or(self.offset_y);
        self.scroll_to(target_offset, scrollable_id)
    }

    /// Desplaza `shift` px sin pasarse de `content_height` (RePág/AvPág sin selección).
    pub fn scroll_by_clamped<Message>(&mut self, shift: f32, content_height: f32, scrollable_id: &'static str) -> iced::Task<Message> {
        if self.viewport_height <= 0.0 {
            return iced::Task::none();
        }
        let max_offset = (content_height - self.viewport_height).max(0.0);
        let target_offset = (self.offset_y + shift).clamp(0.0, max_offset);
        self.scroll_to(target_offset, scrollable_id)
    }

    /// Offset necesario para ver la fila, o `None` si ya está visible.
    fn offset_to_reveal(&self, row_top: f32, row_height: f32) -> Option<f32> {
        let row_bottom = row_top + row_height;
        if row_top < self.offset_y {
            Some(row_top)
        } else if row_bottom > self.offset_y + self.viewport_height {
            Some(row_bottom - self.viewport_height)
        } else {
            None
        }
    }

    fn scroll_to<Message>(&mut self, offset_y: f32, scrollable_id: &'static str) -> iced::Task<Message> {
        self.offset_y = offset_y;
        iced::widget::operation::scroll_to(
            iced::widget::Id::new(scrollable_id),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset_y },
        )
    }

    /// Llama esto desde tu callback `on_scroll(Viewport)`.
    pub fn update(&mut self, viewport: iced::widget::scrollable::Viewport) {
        let offset = viewport.absolute_offset();
        self.offset_y = offset.y;
        let bounds = viewport.bounds();
        self.viewport_height = bounds.height;
        self.viewport_width = bounds.width;
    }

    /// `true` si el punto (en coordenadas relativas al área del
    /// scrollable) cae dentro del contenido real, es decir, fuera de
    /// la franja donde vive el scrollbar.
    pub fn is_within_content(&self, point_x: f32) -> bool {
        self.viewport_width <= 0.0 || point_x < (self.viewport_width - SCROLLBAR_MARGIN_PX)
    }

    pub fn window(&self, row_height: f32, total_items: usize, buffer_rows: usize) -> VirtualWindow {
        self.window_after(0.0, row_height, total_items, buffer_rows)
    }

    /// Como `window`, cuando las filas empiezan `prefix` px más abajo dentro
    /// del mismo scroll (p. ej. un header que se desplaza junto con la lista).
    pub fn window_after(&self, prefix: f32, row_height: f32, total_items: usize, buffer_rows: usize) -> VirtualWindow {
        VirtualWindow::compute(
            (self.offset_y - prefix).max(0.0),
            self.viewport_height,
            row_height,
            total_items,
            buffer_rows,
        )
    }

    /// Calcula cuántos píxeles habría que desplazar el scroll en este
    /// frame si el mouse (en coordenadas relativas al viewport, `local_y`)
    /// está dentro de la zona "caliente" de auto-scroll cerca del borde
    /// superior o inferior mientras se arrastra algo.
    ///
    /// Devuelve `None` si el punto está fuera de ambas zonas calientes.
    /// El signo indica dirección: negativo = subir, positivo = bajar.
    ///
    /// `zone_px`: alto de la franja sensible desde cada borde.
    /// `max_speed_px_per_tick`: velocidad máxima de scroll por tick,
    /// alcanzada cuando el mouse está pegado al borde extremo.
    /// `top_inset`: alto de lo que tapa el borde superior (títulos fijos); la zona empieza debajo.
    pub fn autoscroll_delta(&self, local_y: f32, zone_px: f32, max_speed_px_per_tick: f32, top_inset: f32) -> Option<f32> {
        if self.viewport_height <= 0.0 || zone_px <= 0.0 {
            return None;
        }

        let top_y = local_y - top_inset;
        if top_y < zone_px {
            // Cerca del borde superior: entre más pegado a y=0, más rápido.
            let intensity = ((zone_px - top_y) / zone_px).clamp(0.0, 1.0);
            Some(-max_speed_px_per_tick * intensity)
        } else if local_y > self.viewport_height - zone_px {
            let intensity = ((local_y - (self.viewport_height - zone_px)) / zone_px).clamp(0.0, 1.0);
            Some(max_speed_px_per_tick * intensity)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_at_top() {
        let w = VirtualWindow::compute(0.0, 500.0, 50.0, 1000, 10);
        // visible_rows = ceil(500/50)+1 = 11; start = max(0-10,0) = 0
        assert_eq!(w.start, 0);
        assert_eq!(w.end, 21.min(1000));
    }

    #[test]
    fn window_in_middle() {
        let w = VirtualWindow::compute(5000.0, 500.0, 50.0, 1000, 10);
        // first_visible = 100; visible_rows = 11
        // start = 100 - 10 = 90; end = 100 + 11 + 10 = 121
        assert_eq!(w.start, 90);
        assert_eq!(w.end, 121);
    }

    #[test]
    fn window_clamped_at_end() {
        let w = VirtualWindow::compute(49000.0, 500.0, 50.0, 1000, 10);
        // first_visible = 980; end = min(980+11+10, 1000) = 1000
        assert_eq!(w.end, 1000);
        assert!(w.start < 1000);
    }

    #[test]
    fn empty_list() {
        let w = VirtualWindow::compute(0.0, 500.0, 50.0, 0, 10);
        assert!(w.is_empty());
    }

    #[test]
    fn spacer_heights_sum_correctly() {
        let w = VirtualWindow::compute(5000.0, 500.0, 50.0, 1000, 10);
        let top = w.top_spacer_height(50.0);
        let bottom = w.bottom_spacer_height(50.0, 1000);
        let rendered = w.len() as f32 * 50.0;
        assert_eq!(top + rendered + bottom, 1000.0 * 50.0);
    }
}