use std::time::Instant;
use iced::{Element, Task};
use iced::widget::Id;
use crate::model::{Track};
use crate::ui::assets::icons::Icon;
use crate::ui::views::playlist_adder::{AdderMessage, AdderOutMessage, PlaylistAdder};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::widgets::playlist_header::{play_button, playlist_header, HeaderCover, PlaylistHeaderData, TitleEdit, EDGE_HEADER_HEIGHT, RENAME_INPUT_ID};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::row_animator::RowAnimator;
use crate::ui::cover_palette;
use crate::ui::widgets::collection_page::{action_bar, empty_page, filter_input, icon_toggle, ACTION_BAR_HEIGHT, CONTENT_PADDING_X, ROWS_OFFSET};
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

    /// Abre o cierra el panel lateral para agregar canciones.
    ToggleAdder,
    /// Lupa de la barra de acciones: muestra u oculta el filtro de la playlist.
    ToggleFilter,
    /// Activa/desactiva el modo aleatorio de la reproducción (el mismo del reproductor).
    ToggleShuffle,
    Adder(AdderMessage),


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
    /// Mover varias canciones juntas: el bloque empieza en `to` de la lista sin ellas.
    RequestMoveBlock { playlist_id: String, track_ids: Vec<String>, to: usize },
    RequestCoverChange { playlist_id: String },
    /// Panel de agregar canciones: buscar en YouTube (el resultado vuelve como `AdderMessage::RemoteLoaded`).
    SearchSongs { query: String },
    AddTrack { playlist_id: String, track_id: String },
    /// Descargar una canción de YouTube y agregarla al terminar.
    DownloadAndAdd { playlist_id: String, track: Track },
    ToggleShuffle,
    RequestRename { playlist_id: String, new_name: String },
}

pub type PlaylistOutMessage = TrackListOutMessage<PlaylistExtra>;

// ─── ESTADO DE LA VISTA ─────────────────────────────────────────

const DRAG_ROW_HEIGHT: f32 = 60.0;
const FILTER_INPUT_ID: &str = "playlist_filter_input";
const DRAG_THRESHOLD_PX: f32 = 5.0;

/// `source_index` es la fila agarrada en la lista; `current_index`, dónde empieza el bloque
/// en la lista sin él; `grab_offset`, la altura del cursor desde el borde de arriba del bloque.
#[derive(Debug, Clone)]
pub struct DragState {
    pub source_index: usize,
    pub current_index: usize,
    pub grab_offset: f32,
    /// Canciones que se mueven juntas (la selección si se agarró una seleccionada), en orden visible.
    pub block: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PendingDrag {
    pub source_index: usize,
    pub grab_offset: f32,
    pub start_y: f32,
    pub block: Vec<String>,
}

impl DragState {
    /// Filas que quedan fuera del bloque.
    fn rest_len(&self, total: usize) -> usize {
        total.saturating_sub(self.block.len())
    }

    /// Mueve el destino del bloque según la altura del cursor en el contenido de la lista.
    fn follow(&mut self, rows_y: f32, total: usize) {
        let top_row = ((rows_y - self.grab_offset) / DRAG_ROW_HEIGHT).round().max(0.0) as usize;
        self.current_index = top_row.min(self.rest_len(total));
    }

    /// Orden a pintar: el resto de la lista con el bloque entero (en su orden) en `current_index`.
    fn display_order<'a>(&self, rendered_tracks: &[&'a Track]) -> Vec<&'a Track> {
        let (block, mut order): (Vec<&'a Track>, Vec<&'a Track>) =
            rendered_tracks.iter().copied().partition(|t| self.block.contains(&t.id));
        let at = self.current_index.min(order.len());
        order.splice(at..at, block);
        order
    }
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
    /// Panel lateral para agregar canciones, si está abierto.
    adder: Option<PlaylistAdder>,
    /// Filtro de la playlist visible en la barra de acciones.
    filter_open: bool,
}

// ─── IMPLEMENTACIÓN ─────────────────────────────────────────────

impl PlaylistView {
    /// Restaura (o crea, si `list` es el default) el estado de una
    /// playlist — selección/scroll/filtro/sort — al abrirla. Ver
    /// `ViewCoordinator::playlist_view_cache`. El estado de drag&drop
    /// siempre arranca limpio: es interacción transitoria, no hace
    /// falta preservarlo.
    pub fn with_state(playlist_id: String, mut list: TrackViewState) -> Self {
        list.rows_offset = ROWS_OFFSET;
        Self {
            playlist_id,
            list,
            drag_state: None,
            pending_drag: None,
            row_animator: RowAnimator::new(DRAG_ROW_HEIGHT),
            rename_draft: None,
            adder: None,
            filter_open: false,
        }
    }

    /// Cierra lo que esté abierto (renombre o panel de agregar); `true` si había algo.
    pub fn cancel_rename(&mut self) -> bool {
        self.rename_draft.take().is_some() || self.adder.take().is_some()
    }

    /// Suelta el arrastre de reordenar sin mover nada (la canción se soltó sobre otra playlist).
    pub fn cancel_row_drag(&mut self) {
        self.pending_drag = None;
        if self.drag_state.take().is_some() {
            self.row_animator = RowAnimator::new(DRAG_ROW_HEIGHT);
        }
    }

    pub fn close_adder(&mut self) {
        self.adder = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }

    fn sync_row_animator(&mut self, rendered_tracks: &[&Track], now: Instant) {
        let Some(drag) = &self.drag_state else { return };
        let order = drag.display_order(rendered_tracks);
        for (i, track) in order.iter().enumerate() {
            if drag.block.contains(&track.id) {
                continue;
            }
            self.row_animator.sync_target(&track.id, i, now);
        }
    }

    fn drag_enabled(&self) -> bool {
        matches!(self.list.active_sort_key, None | Some(0)) && self.list.search_filter.trim().is_empty()
    }

    /// Ctrl+F: muestra el filtro de la página y le da foco con el texto seleccionado.
    pub fn open_filter(&mut self) -> Task<PlaylistMessage> {
        self.filter_open = true;
        let id = Id::new(FILTER_INPUT_ID);
        Task::batch([iced::widget::operation::focus(id.clone()), iced::widget::operation::select_all(id)])
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
                                block: pending.block,
                            });
                            self.pending_drag = None;
                        }

                    if let Some(drag) = &mut self.drag_state {
                        drag.follow(p.y + self.list.scroll.offset_y - self.list.rows_offset, rendered_tracks.len());
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
            PlaylistMessage::ToggleFilter => {
                self.filter_open = !self.filter_open;
                if self.filter_open {
                    task = iced::widget::operation::focus(Id::new(FILTER_INPUT_ID));
                } else if !self.list.search_filter.is_empty() {
                    // Al cerrar la lupa no queda un filtro escondido.
                    self.list.apply_search_filter(String::new());
                    out = PlaylistOutMessage::RequestSearch(String::new());
                }
            }
            PlaylistMessage::ToggleShuffle => {
                out = PlaylistOutMessage::extra(PlaylistExtra::ToggleShuffle);
            }
            PlaylistMessage::Adder(message) => {
                if let Some(adder) = &mut self.adder {
                    let playlist_id = self.playlist_id.clone();
                    out = match adder.update(message.clone()) {
                        AdderOutMessage::Close => {
                            self.adder = None;
                            PlaylistOutMessage::Idle
                        }
                        AdderOutMessage::Idle => PlaylistOutMessage::Idle,
                        AdderOutMessage::Search(query) => PlaylistOutMessage::extra(PlaylistExtra::SearchSongs { query }),
                        AdderOutMessage::AddTrack(track_id) => PlaylistOutMessage::extra(PlaylistExtra::AddTrack { playlist_id, track_id }),
                        AdderOutMessage::DownloadAndAdd(track) => PlaylistOutMessage::extra(PlaylistExtra::DownloadAndAdd { playlist_id, track }),
                    };
                }
            }


            // ─── DRAG & DROP GLOBALES ──────────────────────────────────────
            PlaylistMessage::GlobalMousePress => {
                if !self.drag_enabled() {
                    // Sort de columna activo — ver drag_enabled().
                } else if let Some(pos) = self.list.mouse_position
                    && self.list.scroll.is_within_content(pos.x) && !rendered_tracks.is_empty()
                    && pos.y + self.list.scroll.offset_y >= self.list.rows_offset {
                        // Solo sobre las filas, no sobre el header o la barra de acciones.
                        let absolute_y = pos.y + self.list.scroll.offset_y - self.list.rows_offset;
                        let clicked_index = ((absolute_y / DRAG_ROW_HEIGHT).floor() as usize)
                            .min(rendered_tracks.len() - 1);
                        let row_top_y = self.list.rows_offset + clicked_index as f32 * DRAG_ROW_HEIGHT - self.list.scroll.offset_y;

                        // Agarrar una fila seleccionada arrastra toda la selección.
                        let track_id = rendered_tracks[clicked_index].id.clone();
                        let selection = &self.list.tracks_selection;
                        let block: Vec<String> = if selection.is_selected(&track_id) {
                            rendered_tracks.iter().filter(|t| selection.is_selected(&t.id)).map(|t| t.id.clone()).collect()
                        } else {
                            vec![track_id]
                        };
                        // El bloque se arma pegado: la fila agarrada queda bajo el cursor.
                        let rows_above_in_block = rendered_tracks[..clicked_index].iter().filter(|t| block.contains(&t.id)).count();

                        self.pending_drag = Some(PendingDrag {
                            source_index: clicked_index,
                            grab_offset: pos.y - row_top_y + rows_above_in_block as f32 * DRAG_ROW_HEIGHT,
                            start_y: pos.y,
                            block,
                        });
                    }
            }
            PlaylistMessage::GlobalMouseRelease => {
                self.pending_drag = None;

                if let Some(drag) = self.drag_state.take() {
                    let safe_current_index = drag.current_index.min(drag.rest_len(rendered_tracks.len()));

                    for (offset, track_id) in drag.block.iter().enumerate() {
                        self.row_animator.snap_to_target(track_id, safe_current_index + offset);
                    }

                    if drag.block.len() > 1 {
                        out = PlaylistOutMessage::extra(PlaylistExtra::RequestMoveBlock {
                            playlist_id: self.playlist_id.clone(),
                            track_ids: drag.block,
                            to: safe_current_index,
                        });
                    } else if drag.source_index != safe_current_index {
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

                            let max_offset = (self.list.rows_offset + rendered_tracks.len() as f32 * DRAG_ROW_HEIGHT
                                - self.list.scroll.viewport_height)
                                .max(0.0);
                            self.list.scroll.offset_y = (self.list.scroll.offset_y + delta_y).clamp(0.0, max_offset);

                            if let Some(drag) = &mut self.drag_state {
                                drag.follow(pos.y + self.list.scroll.offset_y - self.list.rows_offset, rendered_tracks.len());
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
        is_shuffled: bool,
        with_lyrics: &'a std::collections::HashSet<String>,
    ) -> Element<'a, PlaylistMessage> {
        let track_count = rendered_tracks.len();
        let lyrics_count = rendered_tracks.iter().filter(|t| with_lyrics.contains(&t.id)).count();
        let total_duration_seconds: i64 =
            rendered_tracks.iter().map(|t| t.duration_seconds as i64).sum();

        let this_playlist_is_current = now_playing_id
            .as_deref()
            .is_some_and(|id| rendered_tracks.iter().any(|t| t.id == id));
        let play_message = if this_playlist_is_current {
            PlaylistMessage::TogglePlayback
        } else {
            PlaylistMessage::PlayAll
        };

        // Color de la portada (predominante), o uno derivado del id si no tiene.
        let color = cover_palette::playlist_color(&self.playlist_id);
        let header = playlist_header(
            PlaylistHeaderData {
                name: playlist_name,
                kicker: Some("PLAYLIST"),
                description: None,
                track_count,
                total_duration_seconds,
                lyrics_count: Some(lyrics_count),
                tint: Some(cover_palette::header_tint(color)),
                tint_end: Some(cover_palette::header_tint_end(color)),
                edge_to_edge: true,
            },
            HeaderCover::Single(cover),
            None,
            Some(PlaylistMessage::RequestCoverChange),
            Some(match &self.rename_draft {
                Some(draft) => TitleEdit::Editing {
                    value: draft,
                    on_input: PlaylistMessage::RenameInputChanged,
                    on_submit: PlaylistMessage::SubmitRename,
                },
                None => TitleEdit::Idle { on_double_click: PlaylistMessage::StartRename(playlist_name.to_string()) },
            }),
            false,
        );

        // Header y barra de acciones scrollean junto con la tabla. Debajo del
        // header arranca una banda más oscura del mismo color (la división).
        let action_bar = self.view_action_bar(this_playlist_is_current && is_playing, play_message, is_shuffled);
        let band = cover_palette::band_tint(color);

        let main: Element<'a, PlaylistMessage> = if rendered_tracks.is_empty() {
            let message = if self.list.search_filter.trim().is_empty() {
                "Esta playlist está vacía. Usa el botón + para empezar a armarla."
            } else {
                "Nada en esta playlist coincide con el filtro."
            };
            empty_page(header, action_bar, message, band)
        } else {
            let tracks_refs: Vec<&Track> = match &self.drag_state {
                Some(drag) if self.drag_enabled() => drag.display_order(&rendered_tracks),
                _ => rendered_tracks,
            };

            let mut builder = TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "playlists_catalog_scroll",
            )
                .leading(header, EDGE_HEADER_HEIGHT)
                .toolbar(action_bar, ACTION_BAR_HEIGHT)
                .band(band)
                .content_padding_x(CONTENT_PADDING_X)
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .lyrics(with_lyrics)
                .on_event(PlaylistMessage::Table);

            if let Some(drag) = &self.drag_state
                && self.drag_enabled() {
                    builder = builder
                        .dragging(drag.current_index, self.list.mouse_position, drag.grab_offset)
                        .drag_count(drag.block.len())
                        .animator(&self.row_animator);
                }

            builder.build()
        };

        main
    }

    pub fn is_adder_open(&self) -> bool {
        self.adder.is_some()
    }

    /// Panel de agregar canciones; se pinta en la columna de la cola (ver `main.rs`).
    pub fn view_adder<'a>(&'a self, catalog: &'a CatalogStore, thumbnails: &'a AsyncThumbnail) -> Option<Element<'a, PlaylistMessage>> {
        self.adder
            .as_ref()
            .map(|adder| adder.view(&self.playlist_id, catalog, thumbnails).map(PlaylistMessage::Adder))
    }

    /// Barra debajo del header: reproducir, aleatorio y, a la derecha, el
    /// filtro (lupa) y el panel para agregar canciones (+).
    fn view_action_bar(&self, is_playing: bool, play_message: PlaylistMessage, is_shuffled: bool) -> Element<'_, PlaylistMessage> {
        let left = vec![
            play_button(is_playing, play_message),
            icon_toggle(Icon::Shuffle, is_shuffled, PlaylistMessage::ToggleShuffle),
        ];

        let mut right = Vec::new();
        if self.filter_open {
            right.push(filter_input("Filtrar esta playlist…", &self.list.search_filter, PlaylistMessage::SearchInputChanged, FILTER_INPUT_ID));
        }
        right.push(icon_toggle(Icon::Search, self.filter_open, PlaylistMessage::ToggleFilter));
        right.push(icon_toggle(Icon::Add, self.adder.is_some(), PlaylistMessage::ToggleAdder));

        action_bar(left, right)
    }
}
