use std::collections::HashSet;
use std::time::Instant;

use iced::{Color, Element, Font, Length, Task};
use iced::keyboard::Modifiers;
use iced::widget::scrollable::Viewport;
use iced::widget::{column, space, text, Id};
use iced::widget::operation::snap_to;
use iced::widget::scrollable;
use iced::clipboard;

use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuItem};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::assets::icons::Icon;
use crate::ui::widgets::track_list::{track_list, TrackListCallbacks, TrackListConfig};
use crate::ui::widgets::track_fields::{Field, FieldList};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::views::sort_state::SortState;
use crate::ui::widgets::views::track_sort;
use crate::ui::widgets::views::catalog_filter;
use crate::ui::widgets::views::track_context_menu::{self, LikeSlot};
use crate::impl_sortable_column;
use crate::ui::widgets::selection_state::SelectionState;

pub const SF_PRO: Font = Font::with_name("SF Pro Display");

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    Icon::Explorer,
    "Explorar",
    JETBRAINS_MONO,
);

const ROW_HEIGHT: f32 = 60.0;
const THUMBNAIL_SIZE: f32 = 44.0;
const BUFFER_ROWS: usize = 15;
const CONTEXT_MENU_ITEM_COUNT: usize = 8;
const SCROLLABLE_ID: &str = "explorer_catalog_scroll";
const SUBMENU_ADD_TO_PLAYLIST: usize = 0;

const SORT_KEY_TITLE: usize = 0;
const SORT_KEY_ARTIST: usize = 1;
const SORT_KEY_ALBUM: usize = 2;
const SORT_KEY_BPM: usize = 3;
const SORT_KEY_KEY: usize = 4;
const SORT_KEY_DURATION: usize = 5;
const SORT_KEY_ADDED_AT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Bpm,
    Key,
    Duration,
    AddedAt,
}

impl_sortable_column! {
    SortColumn, default = Title;
    Title => SORT_KEY_TITLE,
    Artist => SORT_KEY_ARTIST,
    Album => SORT_KEY_ALBUM,
    Bpm => SORT_KEY_BPM,
    Key => SORT_KEY_KEY,
    Duration => SORT_KEY_DURATION,
    AddedAt => SORT_KEY_ADDED_AT,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    PlayNow,
    AddToQueue,
    AddToFrontQueue,
    StartRadio,
    ToggleLike,
    AddToPlaylist(String),
    CopyId,
    Delete,
}

#[derive(Debug, Clone)]
pub enum ExplorerViewMessage {
    CatalogUpdated,
    Scrolled(Viewport),
    RowClicked(Track, usize),
    ViewportMouseMoved(iced::Point),
    RowRightClicked(String),
    DismissContextMenu,
    ContextMenuSubmenuHover(Option<usize>),
    ContextMenuAction(ContextMenuAction, Track), // Recibe el "anchor" donde se abrió el menú
    ConfirmDialogConfirm,
    ConfirmDialogCancel,
    SortByKey(usize),
    SearchChanged(String),
    ColorThumbnailResult(String, Vec<u8>, u64),
    GrayThumbnailResult(String, Vec<u8>, u64),
    ModifiersChanged(Modifiers), // Necesario para recibir si Shift/Ctrl están presionados
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExplorerViewOutMessage {
    Idle,
    RequestPlay(Vec<Track>),
    RequestEnqueue(Vec<Track>),
    RequestFrontEnqueue(Vec<Track>),
    RequestPlayRadio(Track), // La radio usualmente se inicia con una semilla
    RequestDelete(Vec<String>),
    RequestToggleLike(Vec<String>),
    RequestAddToPlaylist(String, Vec<String>),
}

pub struct ExplorerView {
    search_query: String,
    filtered_indices: Vec<usize>,
    sort: SortState<SortColumn>,

    selection: SelectionState,
    last_click: Option<(String, Instant)>,
    pub current_modifiers: Modifiers,

    context_menu: ContextMenu<String>,
    confirm_dialog: ConfirmDialog<Vec<Track>>, // Mutado a Vec<Track> para deletes masivos
    scroll: ScrollTracker,
    epoch: u64,
}

impl ExplorerView {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            filtered_indices: Vec::new(),
            sort: SortState::new(),
            selection: SelectionState::new(),
            last_click: None,
            current_modifiers: Modifiers::default(),
            context_menu: ContextMenu::new(),
            confirm_dialog: ConfirmDialog::new(),
            scroll: ScrollTracker::default(),
            epoch: 1,
        }
    }

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn track_at<'a>(&self, store: &'a CatalogStore, visible_idx: usize) -> Option<&'a Track> {
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&idx| store.all_tracks().get(idx))
    }

    fn visible_tracks<'a>(&self, store: &'a CatalogStore) -> Vec<&'a Track> {
        self.filtered_indices
            .iter()
            .filter_map(|&idx| store.all_tracks().get(idx))
            .collect()
    }

    /// Recupera los Tracks reales seleccionados manteniendo el orden del filtro actual (vital para Enqueue masivo)
    fn get_selected_tracks(&self, store: &CatalogStore) -> Vec<Track> {
        let all = store.all_tracks();
        self.filtered_indices
            .iter()
            .filter_map(|&idx| all.get(idx))
            .filter(|t| self.selection.is_selected(&t.id))
            .cloned()
            .collect()
    }

    fn apply_search(&mut self, store: &CatalogStore) {
        self.filtered_indices = catalog_filter::search_indices(store.all_tracks(), &self.search_query);
    }

    fn apply_sort(&mut self, store: &CatalogStore) {
        self.apply_search(store);

        let tracks = store.all_tracks();
        let asc = self.sort.is_asc();

        match self.sort.column() {
            SortColumn::Title => track_sort::by_title(&mut self.filtered_indices, tracks),
            SortColumn::Artist => track_sort::by_artist(&mut self.filtered_indices, tracks),
            SortColumn::Album => track_sort::by_album(&mut self.filtered_indices, tracks),
            SortColumn::Bpm => track_sort::by_bpm(&mut self.filtered_indices, tracks),
            SortColumn::Key => track_sort::by_camelot_key(&mut self.filtered_indices, tracks),
            SortColumn::Duration => track_sort::by_duration(&mut self.filtered_indices, tracks),
            SortColumn::AddedAt => track_sort::by_added_at(&mut self.filtered_indices, tracks),
        }

        if !asc {
            self.filtered_indices.reverse();
        }
    }

    fn current_window(&self) -> crate::ui::utils::virtual_list::VirtualWindow {
        self.scroll.window(ROW_HEIGHT, self.visible_count(), BUFFER_ROWS)
    }

    fn request_visible_thumbnails(&self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        let window = self.current_window();
        let epoch = self.epoch;

        let tasks: Vec<Task<ExplorerViewMessage>> = (window.start..window.end)
            .filter_map(|visible_idx| {
                let track = self.track_at(store, visible_idx)?;
                let url = track
                    .thumbnail_small
                    .clone()
                    .or_else(|| track.thumbnail_large.clone())?;

                if url.is_empty() {
                    return None;
                }

                let key = thumb_key(track);
                thumbnails.request_color(key, url, epoch, |k, bytes, e| {
                    ExplorerViewMessage::ColorThumbnailResult(k, bytes, e)
                })
            })
            .collect();

        Task::batch(tasks)
    }

    fn visible_keys(&self, store: &CatalogStore) -> HashSet<String> {
        catalog_filter::visible_keys(&self.current_window(), |idx| self.track_at(store, idx))
    }

    fn invalidate_and_reload_thumbnails(&mut self, store: &CatalogStore, thumbnails: &mut ThumbnailCache) -> Task<ExplorerViewMessage> {
        self.epoch = self.epoch.wrapping_add(1);
        let current = self.epoch;
        thumbnails.drop_stale(move |e| e == current);
        self.request_visible_thumbnails(store, thumbnails)
    }

    pub fn update(
        &mut self,
        msg: ExplorerViewMessage,
        store: &CatalogStore,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<ExplorerViewMessage>, ExplorerViewOutMessage) {
        match msg {
            ExplorerViewMessage::ModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers;
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::CatalogUpdated => {
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SortByKey(key) => {
                if !self.sort.click(key) {
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                }

                self.selection.clear(); // Limpiamos para evitar Shift+Clicks inválidos
                self.apply_sort(store);
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::SearchChanged(query) => {
                self.search_query = query;
                self.selection.clear();
                self.apply_sort(store);
                self.scroll.reset();
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);

                let snap = snap_to(
                    Id::new(SCROLLABLE_ID),
                    scrollable::RelativeOffset::START,
                );

                (Task::batch([snap, thumb_task]), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::Scrolled(viewport) => {
                self.scroll.update(viewport);
                self.context_menu.note_viewport_size(viewport.bounds().size());
                let keys = self.visible_keys(store);
                thumbnails.drop_outside_visible(&keys, &keys);
                let thumb_task = self.request_visible_thumbnails(store, thumbnails);
                (thumb_task, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::RowClicked(track, index) => {
                let now = Instant::now();
                let is_double_click = match &self.last_click {
                    Some((last_id, time)) => last_id == &track.id && now.duration_since(*time).as_millis() < 500,
                    None => false,
                };

                self.last_click = Some((track.id.clone(), now));
                self.context_menu.dismiss();

                if is_double_click {
                    // Doble clic: destuimos la selección múltiple (convención SO) y reproducimos
                    self.selection.select_single(track.id.clone(), index);
                    return (Task::none(), ExplorerViewOutMessage::RequestPlay(vec![track]));
                }

                if self.current_modifiers.shift() {
                    let visible_ids: Vec<&String> = self.filtered_indices.iter()
                        .filter_map(|&idx| store.all_tracks().get(idx).map(|t| &t.id))
                        .collect();
                    self.selection.select_range(index, &visible_ids);
                } else if self.current_modifiers.command() || self.current_modifiers.control() {
                    self.selection.toggle(track.id, index);
                } else {
                    self.selection.select_single(track.id, index);
                }

                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ViewportMouseMoved(point) => {
                self.context_menu.note_mouse_position(point);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::RowRightClicked(track_id) => {
                // Si haces clic derecho en algo que NO está en la selección, se descarta lo anterior
                if !self.selection.is_selected(&track_id) {
                    let idx = self.filtered_indices.iter()
                        .position(|&i| store.all_tracks().get(i).map(|t| &t.id) == Some(&track_id))
                        .unwrap_or(0);
                    self.selection.select_single(track_id.clone(), idx);
                }

                self.context_menu.toggle(track_id, CONTEXT_MENU_ITEM_COUNT);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::DismissContextMenu => {
                self.context_menu.dismiss();
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ContextMenuSubmenuHover(id) => {
                self.context_menu.set_open_submenu(id);
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ContextMenuAction(action, anchor_track) => {
                self.context_menu.dismiss();

                let selected = self.get_selected_tracks(store);

                if action == ContextMenuAction::Delete {
                    let msg = if selected.len() == 1 {
                        "¿Eliminar esta canción del catálogo?".to_string()
                    } else {
                        format!("¿Eliminar {} canciones del catálogo?", selected.len())
                    };
                    self.confirm_dialog.request(selected, &msg);
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                }

                if action == ContextMenuAction::CopyId {
                    let ids: Vec<String> = selected.iter().map(|t| t.id.clone()).collect();
                    return (clipboard::write(ids.join(", ")), ExplorerViewOutMessage::Idle);
                }

                let out = match action {
                    ContextMenuAction::PlayNow => ExplorerViewOutMessage::RequestPlay(selected),
                    ContextMenuAction::AddToQueue => ExplorerViewOutMessage::RequestEnqueue(selected),
                    ContextMenuAction::AddToFrontQueue => ExplorerViewOutMessage::RequestFrontEnqueue(selected),
                    ContextMenuAction::StartRadio => ExplorerViewOutMessage::RequestPlayRadio(anchor_track),
                    ContextMenuAction::ToggleLike => ExplorerViewOutMessage::RequestToggleLike(selected.into_iter().map(|t| t.id).collect()),
                    ContextMenuAction::AddToPlaylist(playlist_id) => {
                        ExplorerViewOutMessage::RequestAddToPlaylist(playlist_id, selected.into_iter().map(|t| t.id).collect())
                    }
                    ContextMenuAction::CopyId | ContextMenuAction::Delete => unreachable!(),
                };
                (Task::none(), out)
            }

            ExplorerViewMessage::ConfirmDialogConfirm => {
                let Some(tracks) = self.confirm_dialog.take_confirmed() else {
                    return (Task::none(), ExplorerViewOutMessage::Idle);
                };
                self.selection.clear();
                let thumb_task = self.invalidate_and_reload_thumbnails(store, thumbnails);

                let ids = tracks.into_iter().map(|t| t.id).collect();
                (thumb_task, ExplorerViewOutMessage::RequestDelete(ids))
            }

            ExplorerViewMessage::ConfirmDialogCancel => {
                self.confirm_dialog.cancel();
                (Task::none(), ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::ColorThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_color(key.clone(), bytes);
                }
                let next = thumbnails.on_color_finished(&key, |k, b, e| {
                    ExplorerViewMessage::ColorThumbnailResult(k, b, e)
                });
                (next, ExplorerViewOutMessage::Idle)
            }

            ExplorerViewMessage::GrayThumbnailResult(key, bytes, result_epoch) => {
                if result_epoch == self.epoch {
                    thumbnails.insert_gray(key.clone(), bytes);
                }
                let next = thumbnails.on_gray_finished(&key, |k, b, e| {
                    ExplorerViewMessage::GrayThumbnailResult(k, b, e)
                });
                (next, ExplorerViewOutMessage::Idle)
            }
        }
    }

    fn fields<'a>() -> FieldList<'a, ExplorerViewMessage> {
        Field::index(30.0)
            .thumbnail(THUMBNAIL_SIZE + 12.0, THUMBNAIL_SIZE)
            .title(SORT_KEY_TITLE)
            .artist(SORT_KEY_ARTIST)
            .album(SORT_KEY_ALBUM)
            .duration(SORT_KEY_DURATION)
            .bpm(SORT_KEY_BPM)
            .camelot_key(SORT_KEY_KEY)
            .added_at(SORT_KEY_ADDED_AT)
    }

    pub fn view<'a>(&'a self, store: &'a CatalogStore, thumbnails: &'a ThumbnailCache) -> Element<'a, ExplorerViewMessage> {
        let title = text("Catálogo de Pistas")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = catalog_search_input(
            "Buscar por título, artista o álbum...",
            &self.search_query,
            ExplorerViewMessage::SearchChanged,
        );

        let fixed_header = column![
            title,
            space().height(12),
            search_bar,
        ];

        let body_content: Element<'_, ExplorerViewMessage> = if store.is_loading() {
            catalog_status_message("Cargando catálogo desde microservicios...", StatusTone::Neutral)
        } else if let Some(err) = store.last_error() {
            catalog_status_message(format!("Error de conexión: {}", err), StatusTone::Error)
        } else if self.visible_count() == 0 {
            catalog_status_message("No se encontraron pistas que coincidan con tu búsqueda.", StatusTone::Muted)
        } else {
            let tracks = self.visible_tracks(store);
            let fields = Self::fields();

            let config = TrackListConfig {
                columns: fields.columns(),
                active_sort_key: self.sort.active_sort_key(),
                sort_direction_asc: self.sort.is_asc(),
                row_height: ROW_HEIGHT,
                buffer_rows: BUFFER_ROWS,
            };

            let overlay = self.context_menu.render_target(|id| store.track_by_id(id)).map(|(anchor, track)| {
                let items = track_context_menu::build(
                    track,
                    store,
                    LikeSlot::Toggle(ContextMenuAction::ToggleLike),
                    ContextMenuAction::AddToPlaylist,
                    None,
                    SUBMENU_ADD_TO_PLAYLIST,
                    ContextMenuAction::PlayNow,
                    ContextMenuAction::AddToQueue,
                    ContextMenuAction::AddToFrontQueue,
                    ContextMenuAction::StartRadio,
                    ContextMenuAction::CopyId,
                    ContextMenuItem::new("Eliminar canción", ContextMenuAction::Delete).icon(Icon::Delete),
                );

                self.context_menu.view(
                    anchor,
                    items,
                    track,
                    ExplorerViewMessage::ContextMenuAction,
                    ExplorerViewMessage::DismissContextMenu,
                    ExplorerViewMessage::ContextMenuSubmenuHover,
                )
            });

            let confirm_overlay = self.confirm_dialog.view(
                ExplorerViewMessage::ConfirmDialogConfirm,
                ExplorerViewMessage::ConfirmDialogCancel,
            );

            let combined_overlay = match (overlay, confirm_overlay) {
                (Some(a), Some(b)) => Some(iced::widget::stack![a, b].into()),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };

            track_list(
                config,
                &tracks,
                &self.scroll,
                thumbnails,
                &self.selection.selected_ids,
                SCROLLABLE_ID,
                TrackListCallbacks::new(
                    ExplorerViewMessage::Scrolled,
                    ExplorerViewMessage::RowClicked,
                    ExplorerViewMessage::SortByKey,
                    ExplorerViewMessage::ViewportMouseMoved,
                    ExplorerViewMessage::RowRightClicked,
                ),
                |track, idx| fields.row_cells(track, idx),
                combined_overlay,
            )
        };

        column![
            fixed_header,
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}