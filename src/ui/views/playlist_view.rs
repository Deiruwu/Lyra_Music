use iced::{Element, Length, Task};
use iced::widget::{column, space};
use iced::keyboard::Modifiers;
use crate::model::{Track};
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::playlist_header::{playlist_header, PlaylistHeaderData};
use crate::ui::widgets::track_list_builder::{sort_tracks, TrackBuilder, TrackEvent};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;

// ─── MENSAJES INTERNOS ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    SearchInputChanged(String),
    KeybindsChanged(Modifiers),

    Table(TrackEvent),

    // Drag & Drop (Pura UI)
    GlobalMousePress,
    GlobalMouseRelease,
    AutoScrollTick,
}

// ─── LO PROPIO DE PLAYLIST ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PlaylistExtra {
    RequestReorder { playlist_id: String, from: usize, to: usize },
    RequestRemoveTracks { playlist_id: String, track_ids: Vec<String> },
}

pub type PlaylistOutMessage = TrackListOutMessage<PlaylistExtra>;

// ─── ESTADO DE LA VISTA ─────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct DragState {
    pub source_index: usize,
    pub current_index: usize,
    pub grab_offset: f32,
}

pub struct PlaylistView {
    // Id de la playlist actual — solo Playlist, no entra en TrackListState.
    pub playlist_id: String,

    // Todo lo compartido con Explorer/Favorites vive acá adentro.
    pub list: TrackViewState,

    pub drag_state: Option<DragState>,
}

// ─── IMPLEMENTACIÓN ─────────────────────────────────────────────

impl PlaylistView {
    pub fn new(playlist_id: String) -> Self {
        Self {
            playlist_id,
            list: TrackViewState::new(),
            drag_state: None,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    fn drag_enabled(&self) -> bool {
        matches!(self.list.active_sort_key, None | Some(0))
    }

    pub fn update(
        &mut self,
        msg: PlaylistMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        thumbnails: &ThumbnailCache,
    ) -> (Task<PlaylistMessage>, PlaylistOutMessage) {
        let mut out = PlaylistOutMessage::Idle;

        match &msg {
            // ─── EVENTOS DE LA TABLA (TrackBuilder) ────────────────────────
            PlaylistMessage::Table(event) => {
                let action = self.list.process_event(event.clone(), rendered_tracks, thumbnails);

                out = match action {
                    ListAction::PlayContext(id) => PlaylistOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::ThumbnailsNeeded(missing) => PlaylistOutMessage::ThumbnailsNeeded(missing),
                    ListAction::None => PlaylistOutMessage::Idle,

                    ListAction::SortChanged(key) => {
                        self.drag_state = None;
                        PlaylistOutMessage::RequestChangeSort(key)
                    }

                    ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                        let is_liked = rendered_tracks
                            .iter()
                            .find(|t| t.id == anchor_id)
                            .map(|t| t.liked)
                            .unwrap_or(false);

                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, Some(self.playlist_id.as_str()))
                            .with_remove_from_playlist()
                            .build();

                        PlaylistOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id,
                            items,
                        }
                    }
                };
            }
            // ─── EVENTOS INTERNOS ───────────────────────────────
            PlaylistMessage::SearchInputChanged(query) => {
                self.drag_state = None;
                self.list.apply_search_filter(query.clone());
                out = PlaylistOutMessage::RequestSearch(query.clone());
            }

            PlaylistMessage::KeybindsChanged(modifiers) => {
                self.list.keybinds_press = *modifiers;
            }

            // ─── DRAG & DROP GLOBALES ──────────────────────────────────────
            PlaylistMessage::GlobalMousePress => {
                if !self.drag_enabled() {
                    // Sort de columna activo — ver drag_enabled().
                } else if let Some(pos) = self.list.mouse_position {
                    if self.list.scroll.is_within_content(pos.x) {
                        let absolute_y = pos.y + self.list.scroll.offset_y;
                        let clicked_index = (absolute_y / 60.0).floor() as usize;

                        if clicked_index < rendered_tracks.len() {
                            let row_top_y = (clicked_index as f32 * 60.0) - self.list.scroll.offset_y;

                            self.drag_state = Some(DragState {
                                source_index: clicked_index,
                                current_index: clicked_index,
                                grab_offset: pos.y - row_top_y,
                            });
                        }
                    }
                }
            }
            PlaylistMessage::GlobalMouseRelease => {
                if let Some(drag) = self.drag_state.take() {
                    if drag.source_index != drag.current_index {
                        out = PlaylistOutMessage::extra(PlaylistExtra::RequestReorder {
                            playlist_id: self.playlist_id.clone(),
                            from: drag.source_index,
                            to: drag.current_index,
                        });
                    }
                }
            }
            PlaylistMessage::AutoScrollTick => {
                if self.drag_state.is_some() {
                    if let Some(pos) = self.list.mouse_position {
                        if let Some(delta_y) = self.list.scroll.autoscroll_delta(pos.y, 50.0, 18.0) {
                            self.list.scroll.offset_y = (self.list.scroll.offset_y + delta_y).max(0.0);

                            if let Some(drag) = &mut self.drag_state {
                                let absolute_y = pos.y + self.list.scroll.offset_y;
                                let hovered_index = (absolute_y / 60.0).floor() as usize;
                                let max_index = rendered_tracks.len().saturating_sub(1);
                                drag.current_index = hovered_index.clamp(0, max_index);
                            }

                            return (
                                iced::widget::operation::scroll_by(
                                    iced::widget::Id::new("playlists_catalog_scroll"),
                                    iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: delta_y },
                                ),
                                out,
                            );
                        }
                    }
                }
            }
        };

        (Task::none(), out)
    }

    pub fn view<'a>(
        &'a self,
        playlist_name: &'a str,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a ThumbnailCache,
    ) -> Element<'a, PlaylistMessage> {
        let track_count = rendered_tracks.len();
        let total_duration_seconds: i64 =
            rendered_tracks.iter().map(|t| t.duration_seconds as i64).sum();

        let header = playlist_header(
            PlaylistHeaderData {
                name: &playlist_name,
                kicker: Some("PLAYLIST"),
                track_count,
                total_duration_seconds,
            },
            None,
            PlaylistMessage::GlobalMouseRelease,
        );

        let search_bar = catalog_search_input(
            "Buscar en esta playlist...",
            &self.list.search_filter,
            PlaylistMessage::SearchInputChanged,
        );

        let body_content: Element<'_, PlaylistMessage> = if rendered_tracks.is_empty() {
            catalog_status_message("Esta playlist está vacía.", StatusTone::Muted)
        } else {
            let mut tracks_refs: Vec<&Track> = rendered_tracks.iter().copied().collect();
            sort_tracks(&mut tracks_refs, self.list.active_sort_key, self.list.sort_direction_asc);

            let mut builder = TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "playlists_catalog_scroll",
            )
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .on_event(PlaylistMessage::Table);

            if let Some(drag) = &self.drag_state {
                if self.drag_enabled() {
                    builder = builder.dragging(drag.source_index, self.list.mouse_position, drag.grab_offset);
                }
            }

            builder.build()
        };

        column![
            header,
            space().height(Length::Fixed(16.0)),
            search_bar,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}