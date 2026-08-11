use std::collections::HashSet;
use std::time::Instant;
use iced::keyboard::Modifiers;
use iced::Point;
use crate::model::Track;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::{ScrollTracker, VirtualWindow};
use crate::ui::widgets::selection_state::SelectionState;
use crate::ui::widgets::track_list_builder;
use crate::ui::widgets::track_list_builder::{TrackEvent, VisibleTrackKeys};
use crate::ui::widgets::track_list_out_message::VisibleTrackRef;

const ROW_HEIGHT: f32 = 60.0;
const BUFFER_ROWS: usize = 15;


#[derive(Debug, Clone)]
pub enum ListAction {
    PlayContext(String),
    ThumbnailsNeeded(Vec<VisibleTrackRef>),
    SortChanged(usize),
    OpenContextMenu { anchor_id: String, selected_ids: HashSet<String> },
    None,
}

#[derive(Debug, Clone)]
pub struct TrackViewState {
    pub search_filter: String,
    pub keybinds_press: Modifiers,

    pub tracks_selection: SelectionState,
    pub scroll: ScrollTracker,

    pub mouse_position: Option<Point>,

    pub active_sort_key: Option<usize>,
    pub sort_direction_asc: bool,

    pub last_click: Option<(String, Instant)>,
}

impl Default for TrackViewState {
    fn default() -> Self {
        Self {
            search_filter: String::new(),
            keybinds_press: Modifiers::default(),
            tracks_selection: SelectionState::new(),
            scroll: ScrollTracker::default(),
            mouse_position: None,
            active_sort_key: Some(0),
            sort_direction_asc: true,
            last_click: None,
        }
    }
}

impl TrackViewState {
    pub fn new() -> Self {
        Self::default()
    }

    fn visible_index_range(&self, total_items: usize) -> VirtualWindow {
        self.scroll.window(ROW_HEIGHT, total_items, BUFFER_ROWS)
    }

    pub fn pending_thumbnail_requests(
        &self,
        rendered_tracks: &[&Track],
        thumbnails: &ThumbnailCache,
    ) -> Vec<VisibleTrackRef> {
        let window = self.visible_index_range(rendered_tracks.len());
        rendered_tracks[window.start..window.end.min(rendered_tracks.len())]
            .iter()
            .filter(|t| thumbnails.peek_for_render(t).is_none())
            .map(|t| VisibleTrackRef {
                track_id: t.id.clone(),
                color_key: thumb_key(t),
                artwork_url: t.thumbnail_small.clone(),
                state: t.state.clone(),
            })
            .collect()
    }

    pub fn visible_track_keys(&self, rendered_tracks: &[&Track]) -> VisibleTrackKeys {
        track_list_builder::visible_track_keys(
            &self.scroll,
            rendered_tracks,
            ROW_HEIGHT,
            BUFFER_ROWS,
        )
    }

    pub fn apply_search_filter(&mut self, query: String) {
        self.search_filter = query;
        self.scroll.reset();
    }

    pub fn toggle_sort(&mut self, sort_key: usize) -> bool {
        if self.active_sort_key == Some(sort_key) {
            self.sort_direction_asc = !self.sort_direction_asc;
            false
        } else {
            self.active_sort_key = Some(sort_key);
            self.sort_direction_asc = true;
            true
        }
    }

    pub fn register_click(&mut self, track_id: &str) -> bool {
        let now = Instant::now();
        let is_double_click = match &self.last_click {
            Some((last_id, time)) => {
                last_id == track_id && now.duration_since(*time).as_millis() < 500
            }
            None => false,
        };

        self.last_click = Some((track_id.to_string(), now));

        is_double_click
    }


    pub fn process_event(
        &mut self,
        event: TrackEvent,
        rendered_tracks: &[&Track],
        thumbnails: &ThumbnailCache
    ) -> ListAction {
        match event {
            TrackEvent::MouseMoved(p) => {
                self.mouse_position = Some(p);
                ListAction::None
            }
            TrackEvent::ViewportExited => {
                self.mouse_position = None;
                ListAction::None
            }
            TrackEvent::Scrolled(viewport) => {
                self.scroll.update(viewport);
                let missing = self.pending_thumbnail_requests(rendered_tracks, thumbnails);

                println!("pending thumbs [SCROLLED]: {}", missing.len());

                if !missing.is_empty() {
                    ListAction::ThumbnailsNeeded(missing)
                } else {
                    ListAction::None
                }
            }
            TrackEvent::Sorted(sort_key) => {
                self.toggle_sort(sort_key);

                let mut newly_sorted = rendered_tracks.to_vec();
                track_list_builder::sort_tracks(
                    &mut newly_sorted,
                    self.active_sort_key,
                    self.sort_direction_asc
                );

                let missing = self.pending_thumbnail_requests(&newly_sorted, thumbnails);

                println!("pending thumbs [Sorted]: {}", missing.len());

                if !missing.is_empty() {
                    ListAction::ThumbnailsNeeded(missing)
                } else {
                    ListAction::None
                }
            }
            TrackEvent::Clicked(track, index) => {
                if self.register_click(&track.id) {
                    ListAction::PlayContext(track.id.clone())
                } else {
                    if self.keybinds_press.shift() {
                        let visible_ids: Vec<&String> = rendered_tracks.iter().map(|t| &t.id).collect();
                        self.tracks_selection.select_range(index, &visible_ids);
                    } else if self.keybinds_press.command() || self.keybinds_press.control() {
                        self.tracks_selection.toggle(track.id.clone(), index);
                    } else {
                        self.tracks_selection.select_single(track.id.clone(), index);
                    }
                    ListAction::None
                }
            }
            TrackEvent::RightClicked(track_id) => {
                if !self.tracks_selection.is_selected(&track_id) {
                    self.tracks_selection.clear();
                    let idx = rendered_tracks.iter().position(|t| t.id == track_id).unwrap_or(0);
                    self.tracks_selection.select_single(track_id.clone(), idx);
                }

                ListAction::OpenContextMenu {
                    anchor_id: track_id,
                    selected_ids: self.tracks_selection.selected_ids.clone(),
                }
            }
        }
    }
}