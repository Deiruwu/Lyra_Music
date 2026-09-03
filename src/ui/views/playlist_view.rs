use std::time::Instant;
use iced::{Element, Length, Task};
use iced::widget::{column, space};
use crate::model::{Track};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::playlist_header::{playlist_header, PlaylistHeaderData};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;

// ─── MENSAJES INTERNOS ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    SearchInputChanged(String),

    Table(TrackEvent),

    /// El usuario pulsó el overlay "cambiar portada" del header.
    RequestCoverChange,

    /// Botón ▶ del header: reproduce la playlist completa desde el primer track visible.
    PlayAll,
    /// Botón del header cuando ya suena una canción de esta playlist: pausa/reanuda in-place.
    TogglePlayback,

    // Drag & Drop (Pura UI)
    GlobalMousePress,
    GlobalMouseRelease,
    AutoScrollTick,
    AnimationFrame(Instant),
}

// ─── LO PROPIO DE PLAYLIST ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PlaylistExtra {
    RequestReorder { playlist_id: String, from: usize, to: usize },
    RequestRemoveTracks { playlist_id: String, track_ids: Vec<String> },
    RequestCoverChange { playlist_id: String },
}

pub type PlaylistOutMessage = TrackListOutMessage<PlaylistExtra>;

// ─── ESTADO DE LA VISTA ─────────────────────────────────────────

const DRAG_ROW_HEIGHT: f32 = 60.0;
const DRAG_THRESHOLD_PX: f32 = 5.0;

#[derive(Debug, Clone)]
pub struct DragState {
    pub source_index: usize,
    pub current_index: usize,
    pub grab_offset: f32,
    pub track_id: String,
}

#[derive(Debug, Clone)]
pub struct PendingDrag {
    pub source_index: usize,
    pub grab_offset: f32,
    pub start_y: f32,
    pub track_id: String,
}

pub struct PlaylistView {
    // Id de la playlist actual — solo Playlist, no entra en TrackListState.
    pub playlist_id: String,

    // Todo lo compartido con Explorer/Favorites vive acá adentro.
    pub list: TrackViewState,

    pub drag_state: Option<DragState>,
    pending_drag: Option<PendingDrag>,
    pub row_animator: RowAnimator,
}

// ─── IMPLEMENTACIÓN ─────────────────────────────────────────────

impl PlaylistView {
    /// Restaura (o crea, si `list` es el default) el estado de una
    /// playlist — selección/scroll/filtro/sort — al abrirla. Ver
    /// `ViewCoordinator::playlist_view_cache`. El estado de drag&drop
    /// siempre arranca limpio: es interacción transitoria, no hace
    /// falta preservarlo.
    pub fn with_state(playlist_id: String, list: TrackViewState) -> Self {
        Self {
            playlist_id,
            list,
            drag_state: None,
            pending_drag: None,
            row_animator: RowAnimator::new(DRAG_ROW_HEIGHT),
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    fn sync_row_animator(&mut self, rendered_tracks: &[&Track], now: Instant) {
        let Some(drag) = &self.drag_state else { return };
        if drag.source_index >= rendered_tracks.len() {
            return;
        }

        let mut order: Vec<&Track> = rendered_tracks.to_vec();
        if drag.source_index != drag.current_index {
            let item = order.remove(drag.source_index);
            let insert_at = drag.current_index.min(order.len());
            order.insert(insert_at, item);
        }

        let current_index = drag.current_index;
        for (i, track) in order.iter().enumerate() {
            if i == current_index {
                continue;
            }
            self.row_animator.sync_target(&track.id, i, now);
        }
    }

    fn drag_enabled(&self) -> bool {
        matches!(self.list.active_sort_key, None | Some(0)) && self.list.search_filter.trim().is_empty()
    }

    pub fn update(
        &mut self,
        msg: PlaylistMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<PlaylistMessage>, PlaylistOutMessage) {
        let mut out = PlaylistOutMessage::Idle;

        match &msg {
            // ─── EVENTOS DE LA TABLA (TrackBuilder) ────────────────────────
            PlaylistMessage::Table(event) => {
                if let TrackEvent::MouseMoved(p) = event {
                    if let Some(pending) = self.pending_drag.clone() {
                        if (p.y - pending.start_y).abs() > DRAG_THRESHOLD_PX {
                            self.drag_state = Some(DragState {
                                source_index: pending.source_index,
                                current_index: pending.source_index,
                                grab_offset: pending.grab_offset,
                                track_id: pending.track_id,
                            });
                            self.pending_drag = None;
                        }
                    }

                    if let Some(drag) = &mut self.drag_state {
                        let absolute_y = p.y + self.list.scroll.offset_y;
                        let hovered_index = (absolute_y / DRAG_ROW_HEIGHT).floor().max(0.0) as usize;
                        let max_index = rendered_tracks.len().saturating_sub(1);
                        drag.current_index = hovered_index.min(max_index);
                    }
                    self.sync_row_animator(rendered_tracks, Instant::now());
                }

                let action = self.list.process_event(event.clone(), rendered_tracks);

                out = match action {
                    ListAction::PlayContext(id) => PlaylistOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::OpenArtist(id) => PlaylistOutMessage::RequestOpenArtist(id),
                    ListAction::OpenAlbum(id) => PlaylistOutMessage::RequestOpenAlbum(id),
                    ListAction::TogglePlayback => PlaylistOutMessage::RequestTogglePlayback,
                    ListAction::None => PlaylistOutMessage::Idle,

                    ListAction::SortChanged(key) => {
                        self.drag_state = None;
                        self.pending_drag = None;
                        PlaylistOutMessage::RequestChangeSort(key)
                    }

                    ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                        let is_liked = rendered_tracks
                            .iter()
                            .find(|t| t.id == anchor_id)
                            .map(|t| t.liked)
                            .unwrap_or(false);

                        let member_of = catalog_store.playlists_containing_track(&anchor_id);
                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, Some(self.playlist_id.as_str()), &member_of)
                            .with_remove_from_playlist()
                            .build();

                        PlaylistOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id,
                            items,
                            selected_ids,
                        }
                    }
                };
            }
            // ─── EVENTOS INTERNOS ───────────────────────────────
            PlaylistMessage::SearchInputChanged(query) => {
                self.drag_state = None;
                self.pending_drag = None;
                self.list.apply_search_filter(query.clone());
                out = PlaylistOutMessage::RequestSearch(query.clone());
            }

            PlaylistMessage::RequestCoverChange => {
                // El picker (I/O de sistema) no vive en la vista: solo
                // burbujeamos el pedido al coordinator, que es el dueño del
                // import/persistencia (mismo patrón que RequestReorder).
                out = PlaylistOutMessage::extra(PlaylistExtra::RequestCoverChange {
                    playlist_id: self.playlist_id.clone(),
                });
            }

            PlaylistMessage::PlayAll => {
                if !rendered_tracks.is_empty() {
                    out = PlaylistOutMessage::RequestPlayAll;
                }
            }

            PlaylistMessage::TogglePlayback => {
                out = PlaylistOutMessage::RequestTogglePlayback;
            }

            // ─── DRAG & DROP GLOBALES ──────────────────────────────────────
            PlaylistMessage::GlobalMousePress => {
                if !self.drag_enabled() {
                    // Sort de columna activo — ver drag_enabled().
                } else if let Some(pos) = self.list.mouse_position {
                    if self.list.scroll.is_within_content(pos.x) && !rendered_tracks.is_empty() {
                        let absolute_y = pos.y + self.list.scroll.offset_y;
                        let clicked_index = ((absolute_y / DRAG_ROW_HEIGHT).floor() as usize)
                            .min(rendered_tracks.len() - 1);
                        let row_top_y = (clicked_index as f32 * DRAG_ROW_HEIGHT) - self.list.scroll.offset_y;

                        self.pending_drag = Some(PendingDrag {
                            source_index: clicked_index,
                            grab_offset: pos.y - row_top_y,
                            start_y: pos.y,
                            track_id: rendered_tracks[clicked_index].id.clone(),
                        });
                    }
                }
            }
            PlaylistMessage::GlobalMouseRelease => {
                self.pending_drag = None;

                if let Some(drag) = self.drag_state.take() {
                    let safe_current_index = if rendered_tracks.is_empty() {
                        drag.current_index
                    } else {
                        drag.current_index.min(rendered_tracks.len() - 1)
                    };

                    self.row_animator.snap_to_target(&drag.track_id, safe_current_index);

                    if drag.source_index != safe_current_index {
                        out = PlaylistOutMessage::extra(PlaylistExtra::RequestReorder {
                            playlist_id: self.playlist_id.clone(),
                            from: drag.source_index,
                            to: safe_current_index,
                        });
                    }
                }
            }
            PlaylistMessage::AutoScrollTick => {
                if self.drag_state.is_some() {
                    if let Some(pos) = self.list.mouse_position {
                        if let Some(delta_y) = self.list.scroll.autoscroll_delta(pos.y, 50.0, 18.0) {

                            let max_offset = (rendered_tracks.len() as f32 * DRAG_ROW_HEIGHT
                                - self.list.scroll.viewport_height)
                                .max(0.0);
                            self.list.scroll.offset_y = (self.list.scroll.offset_y + delta_y).clamp(0.0, max_offset);

                            if let Some(drag) = &mut self.drag_state {
                                let absolute_y = pos.y + self.list.scroll.offset_y;
                                let hovered_index = (absolute_y / DRAG_ROW_HEIGHT).floor() as usize;
                                let max_index = rendered_tracks.len().saturating_sub(1);
                                drag.current_index = hovered_index.clamp(0, max_index);
                            }
                            self.sync_row_animator(rendered_tracks, Instant::now());

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
            PlaylistMessage::AnimationFrame(_now) => {}
        };

        (Task::none(), out)
    }

    pub fn view<'a>(
        &'a self,
        playlist_name: &'a str,
        cover: Option<iced::widget::image::Handle>,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        now_playing_id: Option<String>,
        is_playing: bool,
    ) -> Element<'a, PlaylistMessage> {
        let track_count = rendered_tracks.len();
        let total_duration_seconds: i64 =
            rendered_tracks.iter().map(|t| t.duration_seconds as i64).sum();

        let this_playlist_is_current = now_playing_id
            .as_deref()
            .is_some_and(|id| rendered_tracks.iter().any(|t| t.id == id));
        let header_is_playing = this_playlist_is_current && is_playing;
        let header_message = if this_playlist_is_current {
            PlaylistMessage::TogglePlayback
        } else {
            PlaylistMessage::PlayAll
        };

        let header = playlist_header(
            PlaylistHeaderData {
                name: &playlist_name,
                kicker: Some("PLAYLIST"),
                track_count,
                total_duration_seconds,
            },
            cover,
            header_message,
            Some(PlaylistMessage::RequestCoverChange),
            header_is_playing,
        );

        let search_bar = catalog_search_input(
            "Buscar en esta playlist...",
            &self.list.search_filter,
            PlaylistMessage::SearchInputChanged,
        );

        let body_content: Element<'_, PlaylistMessage> = if rendered_tracks.is_empty() {
            catalog_status_message("Esta playlist está vacía.", StatusTone::Muted)
        } else {
            let mut tracks_refs: Vec<&Track> = rendered_tracks;

            if let Some(drag) = &self.drag_state {
                if self.drag_enabled() && drag.source_index != drag.current_index && drag.source_index < tracks_refs.len() {
                    let item = tracks_refs.remove(drag.source_index);
                    let insert_at = drag.current_index.min(tracks_refs.len());
                    tracks_refs.insert(insert_at, item);
                }
            }

            let mut builder = TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "playlists_catalog_scroll",
            )
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .on_event(PlaylistMessage::Table);

            if let Some(drag) = &self.drag_state {
                if self.drag_enabled() {
                    builder = builder
                        .dragging(drag.current_index, self.list.mouse_position, drag.grab_offset)
                        .animator(&self.row_animator);
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