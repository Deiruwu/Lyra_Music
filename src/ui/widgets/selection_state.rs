use crate::model::Track;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// Cuánto mueve la selección una tecla: filas (flechas) o páginas (RePág/AvPág).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStep {
    Rows(isize),
    Pages(isize),
}

impl SelectionStep {
    /// Filas equivalentes, dado cuántas caben en una página.
    pub fn rows(self, rows_per_page: usize) -> isize {
        match self {
            SelectionStep::Rows(rows) => rows,
            SelectionStep::Pages(pages) => pages * rows_per_page as isize,
        }
    }

    pub fn is_page(self) -> bool {
        matches!(self, SelectionStep::Pages(_))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub selected_ids: HashSet<String>,
    pub anchor_index: Option<usize>,
    /// Extremo móvil de la selección (el que mueven las flechas).
    pub cursor_index: Option<usize>,
}

impl SelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select_single(&mut self, id: String, index: usize) {
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.anchor_index = Some(index);
        self.cursor_index = Some(index);
    }

    pub fn toggle(&mut self, id: String, index: usize) {
        if self.selected_ids.contains(&id) {
            self.selected_ids.remove(&id);
        } else {
            self.selected_ids.insert(id);
            self.anchor_index = Some(index);
        }
        self.cursor_index = Some(index);
    }

    pub fn select_range(&mut self, target_index: usize, visible_ids: &[&String]) {
        let start = self.anchor_index.unwrap_or(target_index);
        let end = target_index;

        let (min, max) = if start < end { (start, end) } else { (end, start) };

        if max >= visible_ids.len() { return; }

        self.selected_ids.clear();
        for id in &visible_ids[min..=max] {
            self.selected_ids.insert((*id).clone());
        }
        self.cursor_index = Some(target_index);
    }

    /// Mueve el cursor `delta` filas dentro de `visible_ids`; con `extend`
    /// agranda la selección desde el ancla (Shift), si no selecciona solo esa fila.
    /// Devuelve el índice nuevo del cursor.
    pub fn move_cursor(&mut self, delta: isize, extend: bool, visible_ids: &[&String]) -> Option<usize> {
        if visible_ids.is_empty() {
            return None;
        }

        let target = stepped_index(self.cursor_index.or(self.anchor_index), delta, visible_ids.len());

        if extend && self.anchor_index.is_some() {
            self.select_range(target, visible_ids);
        } else {
            self.select_single(visible_ids[target].clone(), target);
        }

        Some(target)
    }

    /// Clic en la fila `id` de `ids`: Shift agranda desde el ancla, Ctrl suma o quita, si no selecciona solo esa.
    pub fn click(&mut self, id: &str, modifiers: iced::keyboard::Modifiers, ids: &[String]) {
        let Some(index) = ids.iter().position(|candidate| candidate == id) else { return };
        if modifiers.shift() {
            let visible: Vec<&String> = ids.iter().collect();
            self.select_range(index, &visible);
        } else if modifiers.control() || modifiers.command() {
            self.toggle(id.to_string(), index);
        } else {
            self.select_single(id.to_string(), index);
        }
    }

    /// Clic derecho: conserva la selección si la fila ya estaba en ella, si no selecciona solo esa.
    pub fn right_click(&mut self, id: &str, ids: &[String]) {
        if self.is_selected(id) {
            return;
        }
        if let Some(index) = ids.iter().position(|candidate| candidate == id) {
            self.select_single(id.to_string(), index);
        }
    }

    pub fn clear(&mut self) {
        self.selected_ids.clear();
        self.anchor_index = None;
        self.cursor_index = None;
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected_ids.contains(id)
    }
}

/// Las de `tracks` a las que aplica una acción sobre `anchor_id`: toda la selección
/// (en el orden de `tracks`) si lo incluye, si no solo el ancla.
pub fn selected_or<'a>(tracks: Vec<&'a Track>, selection: &SelectionState, anchor_id: &str) -> Vec<&'a Track> {
    if selection.is_selected(anchor_id) {
        tracks.into_iter().filter(|t| selection.is_selected(&t.id)).collect()
    } else {
        tracks.into_iter().filter(|t| t.id == anchor_id).collect()
    }
}

/// Índice tras mover `delta` desde `current` en una lista de `len` (> 0)
/// elementos; sin posición previa arranca por el primero (o el último si sube).
pub fn stepped_index(current: Option<usize>, delta: isize, len: usize) -> usize {
    let last = len.saturating_sub(1);
    match current {
        Some(current) => current.min(last).saturating_add_signed(delta).min(last),
        None if delta < 0 => last,
        None => 0,
    }
}

/// Detecta doble click sobre el mismo id dentro de `DOUBLE_CLICK_WINDOW`.
#[derive(Debug, Clone, Default)]
pub struct DoubleClickDetector {
    last_click: Option<(String, Instant)>,
}

impl DoubleClickDetector {
    /// Registra un click y dice si completa un doble click.
    pub fn register(&mut self, id: &str) -> bool {
        let now = Instant::now();
        let is_double_click = self
            .last_click
            .as_ref()
            .is_some_and(|(last_id, time)| last_id == id && now.duration_since(*time) < DOUBLE_CLICK_WINDOW);

        self.last_click = (!is_double_click).then(|| (id.to_string(), now));
        is_double_click
    }
}
