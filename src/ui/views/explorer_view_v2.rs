use iced::{Element, Length, Task};
use iced::widget::image::Handle;
use iced::widget::{button, column, row, space, text};
use iced::{Alignment, Padding};
use crate::model::Track;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackColumn, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::widgets::playlist_header::collection_header;
use crate::ui::assets::{spacing, typography};
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
}

#[derive(Debug, Clone)]
pub enum ExplorerExtra {
    /// Se activaron las columnas de depuración: hay que releer el historial.
    RefreshPlayStats,
}

pub type ExplorerOutMessage = TrackListOutMessage<ExplorerExtra>;

#[derive(Debug, Clone)]
pub struct ExplorerView {
    pub list: TrackViewState,
    /// Columnas de depuración "REPR." / "ÚLTIMA VEZ" visibles.
    pub show_play_stats: bool,
}

impl ExplorerView {
    pub fn new() -> Self {
        let mut list = TrackViewState::new();
        list.default_sort_key = Some(TrackColumn::AddedAt.as_usize());
        list.default_sort_ascending = true;

        Self { list, show_play_stats: false }
    }

    pub fn update(
        &mut self,
        msg: ExplorerMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<ExplorerMessage>, ExplorerOutMessage) {
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

        (Task::none(), out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        mosaic: Vec<Option<Handle>>,
        now_playing_id: Option<String>,
        is_playing: bool,
    ) -> Element<'a, ExplorerMessage> {
        let header = collection_header(
            "EXPLORAR",
            "Catálogo de pistas",
            &rendered_tracks,
            mosaic,
            now_playing_id.is_some(),
            is_playing,
            ExplorerMessage::PlayAll,
            ExplorerMessage::TogglePlayback,
        );

        let search_bar = catalog_search_input(
            "Buscar por título, artista o álbum...",
            &self.list.search_filter,
            ExplorerMessage::SearchInputChanged,
        );

        let debug_toggle = button(text("Depuración").size(typography::TEXT_12).color(theme().content.primary))
            .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_12, right: spacing::SP_12 })
            .style(button_style::pill(self.show_play_stats))
            .on_press(ExplorerMessage::TogglePlayStats);

        let fixed_header = column![
            header,
            space().height(Length::Fixed(16.0)),
            row![search_bar, debug_toggle].spacing(spacing::SP_12).align_y(Alignment::Center),
        ];

        let body_content: Element<'_, ExplorerMessage> = if rendered_tracks.is_empty() {
            catalog_status_message(
                "No se encontraron pistas que coincidan con tu búsqueda.",
                StatusTone::Muted,
            )
        } else {
            let tracks_refs: Vec<&Track> = rendered_tracks;

            TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "explorer_catalog_scroll",
            )
                .with_added_at()
                .with_play_stats(self.show_play_stats)
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .on_event(ExplorerMessage::Table)
                .build()
        };

        column![
            fixed_header,
            space().height(Length::Fixed(16.0)),
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}