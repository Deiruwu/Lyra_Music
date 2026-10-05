use std::time::Instant;
use iced::{Alignment, Element, Length, Padding, Task};
use iced::widget::{button, column, row, space, text};
use crate::model::{Track};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;
use crate::ui::views::playlist_adder::{AdderMessage, AdderOutMessage, PlaylistAdder};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::playlist_header::{playlist_header, HeaderCover, PlaylistHeaderData, TitleEdit, RENAME_INPUT_ID};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::playlist_color::{self, PlaylistColor}; // [playlist-color]
use crate::ui::widgets::color_picker::color_picker; // [playlist-color]
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

    /// Doble click en el nombre: abre el input de renombre con el nombre actual.
    StartRename(String),
    RenameInputChanged(String),
    SubmitRename,

    /// Abre o cierra el panel para agregar canciones.
    ToggleAdder,
    Adder(AdderMessage),

    // [playlist-color] Selector de color del header.
    ToggleColorPicker,
    ColorChanged(PlaylistColor),
    ColorReleased,

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
    RequestCoverChange { playlist_id: String },
    /// Panel de agregar canciones: buscar en YouTube (el resultado vuelve como `AdderMessage::RemoteLoaded`).
    SearchSongs { query: String },
    AddTrack { playlist_id: String, track_id: String },
    /// Descargar una canción de YouTube y agregarla al terminar.
    DownloadAndAdd { playlist_id: String, track: Track },
    /// [playlist-color] Guardar el color elegido en el selector.
    RequestColorChange { playlist_id: String, color: PlaylistColor },
    RequestRename { playlist_id: String, new_name: String },
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
    /// Nombre en edición mientras el input de renombre está abierto.
    rename_draft: Option<String>,
    /// [playlist-color] Selector desplegado y color en vivo mientras se arrastra un slider.
    color_picker_open: bool,
    color_draft: Option<PlaylistColor>,
    /// Panel para agregar canciones, si está abierto.
    adder: Option<PlaylistAdder>,
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
            rename_draft: None,
            color_picker_open: false,
            color_draft: None,
            adder: None,
        }
    }

    /// Cierra el input de renombre sin guardar; `true` si estaba abierto.
    pub fn cancel_rename(&mut self) -> bool {
        self.rename_draft.take().is_some()
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
        let mut task = Task::none();

        match &msg {
            // ─── EVENTOS DE LA TABLA (TrackBuilder) ────────────────────────
            PlaylistMessage::Table(event) => {
                if let TrackEvent::MouseMoved(p) = event {
                    if let Some(pending) = self.pending_drag.clone()
                        && (p.y - pending.start_y).abs() > DRAG_THRESHOLD_PX {
                            self.drag_state = Some(DragState {
                                source_index: pending.source_index,
                                current_index: pending.source_index,
                                grab_offset: pending.grab_offset,
                                track_id: pending.track_id,
                            });
                            self.pending_drag = None;
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
                        let is_downloaded = catalog_store.track_by_id(&anchor_id).is_some_and(|t| t.file_path.is_some());
                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, Some(self.playlist_id.as_str()), &member_of)
                            .with_tools(is_downloaded)
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

            PlaylistMessage::StartRename(current_name) => {
                self.rename_draft = Some(current_name.clone());
                let input_id = iced::widget::Id::new(RENAME_INPUT_ID);
                return (
                    Task::batch([
                        iced::widget::operation::focus(input_id.clone()),
                        iced::widget::operation::select_all(input_id),
                    ]),
                    out,
                );
            }

            PlaylistMessage::RenameInputChanged(value) => {
                self.rename_draft = Some(value.clone());
            }

            PlaylistMessage::SubmitRename => {
                if let Some(new_name) = self.rename_draft.take() {
                    out = PlaylistOutMessage::extra(PlaylistExtra::RequestRename {
                        playlist_id: self.playlist_id.clone(),
                        new_name,
                    });
                }
            }

            PlaylistMessage::ToggleAdder => {
                if self.adder.take().is_none() {
                    let (adder, focus) = PlaylistAdder::new();
                    self.adder = Some(adder);
                    task = focus.map(PlaylistMessage::Adder);
                }
            }
            PlaylistMessage::Adder(message) => {
                if let Some(adder) = &mut self.adder {
                    let playlist_id = self.playlist_id.clone();
                    out = match adder.update(message.clone()) {
                        AdderOutMessage::Idle => PlaylistOutMessage::Idle,
                        AdderOutMessage::Search(query) => PlaylistOutMessage::extra(PlaylistExtra::SearchSongs { query }),
                        AdderOutMessage::AddTrack(track_id) => PlaylistOutMessage::extra(PlaylistExtra::AddTrack { playlist_id, track_id }),
                        AdderOutMessage::DownloadAndAdd(track) => PlaylistOutMessage::extra(PlaylistExtra::DownloadAndAdd { playlist_id, track }),
                    };
                }
            }

            // [playlist-color]
            PlaylistMessage::ToggleColorPicker => {
                self.color_picker_open = !self.color_picker_open;
            }
            PlaylistMessage::ColorChanged(color) => {
                self.color_draft = Some(*color);
            }
            PlaylistMessage::ColorReleased => {
                if let Some(color) = self.color_draft.take() {
                    out = PlaylistOutMessage::extra(PlaylistExtra::RequestColorChange {
                        playlist_id: self.playlist_id.clone(),
                        color,
                    });
                }
            }

            // ─── DRAG & DROP GLOBALES ──────────────────────────────────────
            PlaylistMessage::GlobalMousePress => {
                if !self.drag_enabled() {
                    // Sort de columna activo — ver drag_enabled().
                } else if let Some(pos) = self.list.mouse_position
                    && self.list.scroll.is_within_content(pos.x) && !rendered_tracks.is_empty() {
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
                if self.drag_state.is_some()
                    && let Some(pos) = self.list.mouse_position
                        && let Some(delta_y) = self.list.scroll.autoscroll_delta(pos.y, 50.0, 18.0) {

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
            PlaylistMessage::AnimationFrame(_now) => {}
        };

        (task, out)
    }

    /// Terminó la descarga de una canción pedida desde el panel de agregar.
    pub fn adder_download_finished(&mut self, track_id: &str) {
        if let Some(adder) = &mut self.adder {
            adder.download_finished(track_id);
        }
    }

    /// Miniaturas de los resultados del panel de agregar, si está abierto.
    pub fn adder_thumbnail_targets(&self, catalog: &CatalogStore) -> Vec<(String, String)> {
        self.adder.as_ref().map(|adder| adder.thumbnail_targets(catalog)).unwrap_or_default()
    }

    pub fn view<'a>(
        &'a self,
        playlist_name: &'a str,
        cover: Option<iced::widget::image::Handle>,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        now_playing_id: Option<String>,
        is_playing: bool,
        catalog: &'a CatalogStore,
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

        let color = self.color_draft.unwrap_or_else(|| playlist_color::color_of(&self.playlist_id)); // [playlist-color]
        let header = playlist_header(
            PlaylistHeaderData {
                name: playlist_name,
                kicker: Some("PLAYLIST"),
                description: None,
                track_count,
                total_duration_seconds,
                tint: Some(playlist_color::header_tint(color)), // [playlist-color]
            },
            HeaderCover::Single(cover),
            header_message,
            Some(PlaylistMessage::RequestCoverChange),
            Some(match &self.rename_draft {
                Some(draft) => TitleEdit::Editing {
                    value: draft,
                    on_input: PlaylistMessage::RenameInputChanged,
                    on_submit: PlaylistMessage::SubmitRename,
                },
                None => TitleEdit::Idle { on_double_click: PlaylistMessage::StartRename(playlist_name.to_string()) },
            }),
            header_is_playing,
            // [playlist-color]
            Some(color_picker(
                color,
                self.color_picker_open,
                PlaylistMessage::ToggleColorPicker,
                PlaylistMessage::ColorChanged,
                PlaylistMessage::ColorReleased,
            )),
        );

        let search_bar = catalog_search_input(
            "Buscar en esta playlist...",
            &self.list.search_filter,
            PlaylistMessage::SearchInputChanged,
        );

        let body_content: Element<'_, PlaylistMessage> = if rendered_tracks.is_empty() {
            catalog_status_message("Esta playlist está vacía. Usa «Agregar canciones» para empezar a armarla.", StatusTone::Muted)
        } else {
            let mut tracks_refs: Vec<&Track> = rendered_tracks;

            if let Some(drag) = &self.drag_state
                && self.drag_enabled() && drag.source_index != drag.current_index && drag.source_index < tracks_refs.len() {
                    let item = tracks_refs.remove(drag.source_index);
                    let insert_at = drag.current_index.min(tracks_refs.len());
                    tracks_refs.insert(insert_at, item);
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

            if let Some(drag) = &self.drag_state
                && self.drag_enabled() {
                    builder = builder
                        .dragging(drag.current_index, self.list.mouse_position, drag.grab_offset)
                        .animator(&self.row_animator);
                }

            builder.build()
        };

        let adder_label = if self.adder.is_some() { "Cerrar" } else { "Agregar canciones" };
        let adder_toggle = button(
            row![
                icons::icon(if self.adder.is_some() { Icon::ExpandLess } else { Icon::Add }, typography::TEXT_13),
                text(adder_label).font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary),
            ]
                .spacing(spacing::SP_6)
                .align_y(Alignment::Center),
        )
            .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_14, right: spacing::SP_14 })
            .style(button_style::pill(self.adder.is_some()))
            .on_press(PlaylistMessage::ToggleAdder);

        let mut content = column![
            header,
            space().height(Length::Fixed(16.0)),
            row![search_bar, adder_toggle].spacing(spacing::SP_12).align_y(Alignment::Center),
        ]
            .width(Length::Fill)
            .height(Length::Fill);

        if let Some(adder) = &self.adder {
            content = content.push(space().height(Length::Fixed(12.0)));
            content = content.push(adder.view(&self.playlist_id, catalog, thumbnails).map(PlaylistMessage::Adder));
        }

        content.push(body_content).into()
    }
}