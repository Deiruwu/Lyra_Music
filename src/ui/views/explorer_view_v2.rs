use iced::{Element, Length, Task};
use iced::widget::{column, space, text};
use crate::ui::assets::fonts::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackColumn, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::assets::typography;
use crate::ui::theme::theme;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Explorer,
    Icon::Explorer,
    "Explorar",
    JETBRAINS_MONO,
);

#[derive(Debug, Clone)]
pub enum ExplorerMessage {
    SearchInputChanged(String),
    Table(TrackEvent),
    ConfirmDialogConfirm,
    ConfirmDialogCancel,
}

#[derive(Debug, Clone)]
pub enum ExplorerExtra {
    RequestDelete(Vec<String>),
}

pub type ExplorerOutMessage = TrackListOutMessage<ExplorerExtra>;

#[derive(Debug, Clone)]
pub struct ExplorerView {
    pub list: TrackViewState,

    confirm_dialog: ConfirmDialog<Vec<Track>>,
}

impl ExplorerView {
    pub fn new() -> Self {
        let mut list = TrackViewState::new();
        list.default_sort_key = Some(TrackColumn::AddedAt.as_usize());
        list.default_sort_ascending = true;

        Self {
            list,
            confirm_dialog: ConfirmDialog::new(),
        }
    }

    pub fn update(
        &mut self,
        msg: ExplorerMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
    ) -> (Task<ExplorerMessage>, ExplorerOutMessage) {
        let mut out = ExplorerOutMessage::Idle;

        match &msg {
            ExplorerMessage::Table(event) => {
                let action = self.list.process_event(event.clone(), rendered_tracks);

                out = match action {
                    ListAction::PlayContext(id) => ExplorerOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::SortChanged(key) => ExplorerOutMessage::RequestChangeSort(key),
                    ListAction::OpenArtist(id) => ExplorerOutMessage::RequestOpenArtist(id),
                    ListAction::OpenAlbum(id) => ExplorerOutMessage::RequestOpenAlbum(id),
                    ListAction::TogglePlayback => ExplorerOutMessage::RequestTogglePlayback,
                    ListAction::None => ExplorerOutMessage::Idle,

                    ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                        let is_liked = rendered_tracks.iter().find(|t| t.id == anchor_id).map(|t| t.liked).unwrap_or(false);

                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, None)
                            .with_delete()
                            .build();

                        ExplorerOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id,
                            items,
                            selected_ids,
                        }
                    }
                };
            }

            ExplorerMessage::SearchInputChanged(query) => {
                self.list.apply_search_filter(query.clone());
                out = ExplorerOutMessage::RequestSearch(query.clone());
            }

            ExplorerMessage::ConfirmDialogConfirm => {
                if let Some(tracks) = self.confirm_dialog.take_confirmed() {
                    let ids = tracks.into_iter().map(|t| t.id).collect();
                    out = ExplorerOutMessage::extra(ExplorerExtra::RequestDelete(ids));
                }
            }
            ExplorerMessage::ConfirmDialogCancel => {
                self.confirm_dialog.cancel();
            }
        };

        (Task::none(), out)
    }

    pub fn request_delete_confirmation(&mut self, tracks: Vec<Track>) {
        let msg = if tracks.len() == 1 {
            "¿Eliminar esta canción del catálogo?".to_string()
        } else {
            format!("¿Eliminar {} canciones del catálogo?", tracks.len())
        };
        self.confirm_dialog.request(tracks, &msg);
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        now_playing_id: Option<String>,
        is_playing: bool,
    ) -> Element<'a, ExplorerMessage> {
        let title = text("Catálogo de Pistas")
            .size(typography::TEXT_28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(theme().content.primary) });

        let search_bar = catalog_search_input(
            "Buscar por título, artista o álbum...",
            &self.list.search_filter,
            ExplorerMessage::SearchInputChanged,
        );

        let fixed_header = column![
            title,
            space().height(Length::Fixed(12.0)),
            search_bar,
        ];

        let body_content: Element<'_, ExplorerMessage> = if rendered_tracks.is_empty() {
            catalog_status_message(
                "No se encontraron pistas que coincidan con tu búsqueda.",
                StatusTone::Muted,
            )
        } else {
            let tracks_refs: Vec<&Track> = rendered_tracks;

            let confirm_overlay = self.confirm_dialog.view(
                ExplorerMessage::ConfirmDialogConfirm,
                ExplorerMessage::ConfirmDialogCancel,
            );

            TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "explorer_catalog_scroll",
            )
                .with_added_at()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .on_event(ExplorerMessage::Table)
                .overlay(confirm_overlay)
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