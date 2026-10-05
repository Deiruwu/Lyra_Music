use iced::{Color, Element, Padding, Task};
use iced::widget::image::Handle;
use iced::widget::{button, text, Id};
use crate::model::Track;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::collection_page::{action_bar, contains_now_playing, empty_page, filter_input, icon_toggle, mosaic_header, ACTION_BAR_HEIGHT, CONTENT_PADDING_X, ROWS_OFFSET};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackColumn, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::widgets::playlist_header::{play_button, EDGE_HEADER_HEIGHT};
use crate::ui::assets::{spacing, typography};
use crate::ui::cover_palette;
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    Icon::Explorer,
    "Explorar",
);

#[derive(Debug, Clone)]
pub enum ExplorerMessage {
    SearchInputChanged(String),
    Table(TrackEvent),
    TogglePlayStats,
    PlayAll,
    TogglePlayback,
    ToggleFilter,
    ToggleShuffle,
}

#[derive(Debug, Clone)]
pub enum ExplorerExtra {
    /// Se activaron las columnas de depuración: hay que releer el historial.
    RefreshPlayStats,
    ToggleShuffle,
}

const FILTER_INPUT_ID: &str = "explorer_filter_input";

pub type ExplorerOutMessage = TrackListOutMessage<ExplorerExtra>;

#[derive(Debug, Clone)]
pub struct ExplorerView {
    pub list: TrackViewState,
    /// Columnas de depuración "REPR." / "ÚLTIMA VEZ" visibles.
    pub show_play_stats: bool,
    /// Campo de filtro visible (lupa activa).
    filter_open: bool,
}

impl ExplorerView {
    pub fn new() -> Self {
        let mut list = TrackViewState::new();
        list.default_sort_key = Some(TrackColumn::AddedAt.as_usize());
        list.default_sort_ascending = true;
        list.rows_offset = ROWS_OFFSET;

        Self { list, show_play_stats: false, filter_open: false }
    }

    /// Ctrl+F: muestra el filtro de la página y le da foco con el texto seleccionado.
    pub fn open_filter(&mut self) -> Task<ExplorerMessage> {
        self.filter_open = true;
        let id = Id::new(FILTER_INPUT_ID);
        Task::batch([iced::widget::operation::focus(id.clone()), iced::widget::operation::select_all(id)])
    }

    pub fn update(
        &mut self,
        msg: ExplorerMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<ExplorerMessage>, ExplorerOutMessage) {
        let mut task = Task::none();
        let out = match &msg {
            ExplorerMessage::Table(event) => {
                let action = self.list.process_event(event.clone(), rendered_tracks);

                match action {
                    ListAction::PlayContext(id) => ExplorerOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::SortChanged(key) => ExplorerOutMessage::RequestChangeSort(key),
                    ListAction::OpenArtist(id) => ExplorerOutMessage::RequestOpenArtist(id),
                    ListAction::OpenAlbum(id) => ExplorerOutMessage::RequestOpenAlbum(id),
                    ListAction::TogglePlayback => ExplorerOutMessage::RequestTogglePlayback,
                    ListAction::None => ExplorerOutMessage::Idle,

                    ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                        let is_liked = rendered_tracks.iter().find(|t| t.id == anchor_id).map(|t| t.liked).unwrap_or(false);
                        let member_of = catalog_store.playlists_containing_track(&anchor_id);

                        let is_downloaded = catalog_store.track_by_id(&anchor_id).is_some_and(|t| t.file_path.is_some());
                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, None, &member_of)
                            .with_tools(is_downloaded)
                            .with_delete()
                            .build();

                        ExplorerOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id,
                            items,
                            selected_ids,
                        }
                    }
                }
            }

            ExplorerMessage::SearchInputChanged(query) => {
                self.list.apply_search_filter(query.clone());
                ExplorerOutMessage::RequestSearch(query.clone())
            }

            ExplorerMessage::PlayAll => ExplorerOutMessage::RequestPlayAll,
            ExplorerMessage::TogglePlayback => ExplorerOutMessage::RequestTogglePlayback,
            ExplorerMessage::ToggleShuffle => ExplorerOutMessage::extra(ExplorerExtra::ToggleShuffle),

            ExplorerMessage::ToggleFilter => {
                self.filter_open = !self.filter_open;
                if self.filter_open {
                    task = iced::widget::operation::focus(Id::new(FILTER_INPUT_ID));
                    ExplorerOutMessage::Idle
                } else if !self.list.search_filter.is_empty() {
                    // Al cerrar la lupa no queda un filtro escondido.
                    self.list.apply_search_filter(String::new());
                    ExplorerOutMessage::RequestSearch(String::new())
                } else {
                    ExplorerOutMessage::Idle
                }
            }

            ExplorerMessage::TogglePlayStats => {
                self.show_play_stats = !self.show_play_stats;
                if self.show_play_stats {
                    ExplorerOutMessage::extra(ExplorerExtra::RefreshPlayStats)
                } else {
                    // No dejar la lista ordenada por una columna que ya no se ve.
                    let stats_columns = [TrackColumn::PlayCount.as_usize(), TrackColumn::LastPlayed.as_usize()];
                    if self.list.active_sort_key.is_some_and(|key| stats_columns.contains(&key)) {
                        self.list.active_sort_key = None;
                        self.list.sort_direction_asc = true;
                    }
                    ExplorerOutMessage::Idle
                }
            }
        };

        (task, out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        mosaic: Vec<Option<Handle>>,
        base_color: Color,
        now_playing_id: Option<String>,
        is_playing: bool,
        is_shuffled: bool,
    ) -> Element<'a, ExplorerMessage> {
        let header = mosaic_header("EXPLORAR", "Catálogo de pistas", &rendered_tracks, mosaic, base_color);
        let is_current = contains_now_playing(&rendered_tracks, now_playing_id.as_deref());
        let toolbar = self.view_action_bar(is_current, is_current && is_playing, is_shuffled);
        let band = cover_palette::band_tint(base_color);

        if rendered_tracks.is_empty() {
            return empty_page(header, toolbar, "No se encontraron pistas que coincidan con tu búsqueda.", band);
        }

        TrackBuilder::new(
            rendered_tracks,
            &self.list.scroll,
            thumbnails,
            &self.list.tracks_selection.selected_ids,
            "explorer_catalog_scroll",
        )
            .leading(header, EDGE_HEADER_HEIGHT)
            .toolbar(toolbar, ACTION_BAR_HEIGHT)
            .band(band)
            .content_padding_x(CONTENT_PADDING_X)
            .with_added_at()
            .with_play_stats(self.show_play_stats)
            .sort(self.list.active_sort_key, self.list.sort_direction_asc)
            .playing(now_playing_id, is_playing)
            .icon_hovered(self.list.playing_icon_hovered)
            .on_event(ExplorerMessage::Table)
            .build()
    }

    /// Reproducir, aleatorio y depuración; a la derecha el filtro (lupa).
    fn view_action_bar(&self, is_current: bool, is_playing: bool, is_shuffled: bool) -> Element<'_, ExplorerMessage> {
        let play_message = if is_current { ExplorerMessage::TogglePlayback } else { ExplorerMessage::PlayAll };
        let debug_toggle = button(text("Depuración").size(typography::TEXT_12).color(theme().content.primary))
            .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_12, right: spacing::SP_12 })
            .style(button_style::pill(self.show_play_stats))
            .on_press(ExplorerMessage::TogglePlayStats);

        let left = vec![
            play_button(is_playing, play_message),
            icon_toggle(Icon::Shuffle, is_shuffled, ExplorerMessage::ToggleShuffle),
            debug_toggle.into(),
        ];

        let mut right = Vec::new();
        if self.filter_open {
            right.push(filter_input("Buscar por título, artista o álbum…", &self.list.search_filter, ExplorerMessage::SearchInputChanged, FILTER_INPUT_ID));
        }
        right.push(icon_toggle(Icon::Search, self.filter_open, ExplorerMessage::ToggleFilter));

        action_bar(left, right)
    }
}
