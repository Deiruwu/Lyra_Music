use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub selected_ids: HashSet<String>,
    pub anchor_index: Option<usize>,
}

impl SelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select_single(&mut self, id: String, index: usize) {
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.anchor_index = Some(index);
    }

    pub fn toggle(&mut self, id: String, index: usize) {
        if self.selected_ids.contains(&id) {
            self.selected_ids.remove(&id);
        } else {
            self.selected_ids.insert(id);
            self.anchor_index = Some(index);
        }
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
    }

    pub fn clear(&mut self) {
        self.selected_ids.clear();
        self.anchor_index = None;
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected_ids.contains(id)
    }
}