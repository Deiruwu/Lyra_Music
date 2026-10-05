//! Remix: habilitas playlists y se arma una mezcla con sus canciones, intercaladas
//! (una de cada playlist por vuelta), barajadas dentro de cada playlist y sin repetidas.

use std::collections::HashSet;

use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Element, Length, Padding, Task, Theme};
use rand::rng;
use rand::seq::SliceRandom;

use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{spacing, typography};
use crate::ui::playlist_color;
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::playlist_header::collection_header;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;

pub const VIEW_DATA: ViewData = ViewData::new(NavId::Remix, Icon::Shuffle, "Remix");

pub const SCROLL_ID: &str = "remix_catalog_scroll";
const CHIP_DOT_SIZE: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum RemixMessage {
    Table(TrackEvent),
    TogglePlaylist(String),
    Reshuffle,
    PlayAll,
    TogglePlayback,
}

#[derive(Debug, Clone)]
pub enum RemixExtra {}

pub type RemixOutMessage = TrackListOutMessage<RemixExtra>;

pub struct RemixView {
    pub list: TrackViewState,
    /// Playlists habilitadas para la mezcla (se guardan en los ajustes).
    pub enabled: Vec<String>,
    /// Orden de la mezcla, por id de track.
    order: Vec<String>,
}

impl RemixView {
    pub fn new(enabled: Vec<String>) -> Self {
        Self { list: TrackViewState::new(), enabled, order: Vec::new() }
    }

    /// Canciones de la mezcla en orden: las ya mezcladas que siguen en alguna
    /// playlist habilitada y, al final, las que se agregaron después.
    pub fn source<'a>(&self, catalog: &'a CatalogStore) -> Vec<&'a Track> {
        let members: Vec<&'a Track> = self.enabled.iter().flat_map(|id| catalog.tracks_for_playlist(id)).collect();
        let member_ids: HashSet<&str> = members.iter().map(|t| t.id.as_str()).collect();

        let mut seen: HashSet<&str> = HashSet::new();
        let mut tracks: Vec<&'a Track> = self.order
            .iter()
            .filter(|id| member_ids.contains(id.as_str()))
            .filter_map(|id| catalog.track_by_id(id))
            .filter(|t| seen.insert(t.id.as_str()))
            .collect();
        tracks.extend(members.into_iter().filter(|t| seen.insert(t.id.as_str())));
        tracks
    }

    /// Mezcla por primera vez cuando ya cargó el catálogo.
    pub fn ensure_mixed(&mut self, catalog: &CatalogStore) {
        if self.order.is_empty() && !self.enabled.is_empty() {
            self.reshuffle(catalog);
        }
    }

    pub fn update(
        &mut self,
        message: RemixMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog: &CatalogStore,
    ) -> (Task<RemixMessage>, RemixOutMessage) {
        let out = match message {
            RemixMessage::Table(event) => match self.list.process_event(event, rendered_tracks) {
                ListAction::PlayContext(id) => RemixOutMessage::RequestPlayContext { start_track_id: id },
                ListAction::SortChanged(key) => RemixOutMessage::RequestChangeSort(key),
                ListAction::OpenArtist(id) => RemixOutMessage::RequestOpenArtist(id),
                ListAction::OpenAlbum(id) => RemixOutMessage::RequestOpenAlbum(id),
                ListAction::TogglePlayback => RemixOutMessage::RequestTogglePlayback,
                ListAction::None => RemixOutMessage::Idle,
                ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                    let member_of = catalog.playlists_containing_track(&anchor_id);
                    let is_downloaded = catalog.track_by_id(&anchor_id).is_some_and(|t| t.file_path.is_some());
                    let items = TrackContextMenuBuilder::new(catalog.is_liked(&anchor_id))
                        .with_playlists(playlists, None, &member_of)
                        .with_tools(is_downloaded)
                        .build();
                    RemixOutMessage::ContextMenuRightClicked { track_id: anchor_id, items, selected_ids }
                }
            },
            RemixMessage::TogglePlaylist(playlist_id) => {
                if let Some(index) = self.enabled.iter().position(|id| id == &playlist_id) {
                    self.enabled.remove(index);
                } else {
                    self.enabled.push(playlist_id);
                }
                self.reshuffle(catalog);
                RemixOutMessage::Idle
            }
            RemixMessage::Reshuffle => {
                self.reshuffle(catalog);
                RemixOutMessage::Idle
            }
            RemixMessage::PlayAll => RemixOutMessage::RequestPlayAll,
            RemixMessage::TogglePlayback => RemixOutMessage::RequestTogglePlayback,
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
        catalog: &'a CatalogStore,
    ) -> Element<'a, RemixMessage> {
        let header = collection_header(
            "REMIX",
            "Remix",
            &rendered_tracks,
            mosaic,
            now_playing_id.is_some(),
            is_playing,
            RemixMessage::PlayAll,
            RemixMessage::TogglePlayback,
        );

        let body: Element<'a, RemixMessage> = if self.enabled.is_empty() {
            catalog_status_message("Habilita una o más playlists para mezclarlas.", StatusTone::Muted)
        } else if rendered_tracks.is_empty() {
            catalog_status_message("Las playlists habilitadas están vacías.", StatusTone::Muted)
        } else {
            TrackBuilder::new(
                rendered_tracks,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                SCROLL_ID,
            )
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .on_event(RemixMessage::Table)
                .build()
        };

        column![header, space().height(Length::Fixed(16.0)), self.view_playlist_chips(catalog), space().height(Length::Fixed(12.0)), body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Una píldora por playlist (con el punto de su color) para habilitarla en la mezcla, y "Volver a mezclar".
    fn view_playlist_chips<'a>(&'a self, catalog: &'a CatalogStore) -> Element<'a, RemixMessage> {
        let mut chips: Vec<Element<'a, RemixMessage>> = catalog
            .playlists_metadata()
            .iter()
            .map(|(id, name, _)| {
                let is_enabled = self.enabled.contains(id);
                let dot_color = playlist_color::accent(playlist_color::color_of(id)); // [playlist-color]
                let dot = container(space().width(Length::Fixed(CHIP_DOT_SIZE)).height(Length::Fixed(CHIP_DOT_SIZE)))
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(dot_color.into()),
                        border: rounded(CHIP_DOT_SIZE / 2.0),
                        ..Default::default()
                    });
                button(
                    row![dot, text(name.as_str()).font(SF_PRO).size(typography::TEXT_12).color(theme().content.primary)]
                        .spacing(spacing::SP_6)
                        .align_y(Alignment::Center),
                )
                    .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_12 })
                    .style(button_style::pill(is_enabled))
                    .on_press(RemixMessage::TogglePlaylist(id.clone()))
                    .into()
            })
            .collect();

        let reshuffle = button(
            row![icons::icon(Icon::Shuffle, typography::TEXT_12), text("Volver a mezclar").font(SF_PRO).size(typography::TEXT_12)]
                .spacing(spacing::SP_6)
                .align_y(Alignment::Center),
        )
            .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_12 })
            .style(button_style::pill(false));
        chips.push(if self.enabled.is_empty() { reshuffle.into() } else { reshuffle.on_press(RemixMessage::Reshuffle).into() });

        row(chips).spacing(spacing::SP_8).wrap().vertical_spacing(spacing::SP_8).into()
    }

    /// Baraja cada playlist habilitada y las intercala sin repetir canciones.
    fn reshuffle(&mut self, catalog: &CatalogStore) {
        let mut pools: Vec<Vec<String>> = self.enabled
            .iter()
            .map(|id| catalog.tracks_for_playlist(id).into_iter().map(|t| t.id.clone()).collect())
            .collect();
        for pool in &mut pools {
            pool.shuffle(&mut rng());
        }

        self.order = interleave_unique(&pools);
        self.list.invalidate_cache();
    }
}

/// Una canción de cada lista por vuelta, saltando las repetidas.
fn interleave_unique(pools: &[Vec<String>]) -> Vec<String> {
    let mut seen = HashSet::new();
    let longest = pools.iter().map(Vec::len).max().unwrap_or(0);
    (0..longest)
        .flat_map(|index| pools.iter().filter_map(move |pool| pool.get(index)))
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn intercala_las_playlists_sin_repetir() {
        let pools = vec![ids(&["a1", "a2", "a3"]), ids(&["b1", "a2"])];
        assert_eq!(interleave_unique(&pools), ids(&["a1", "b1", "a2", "a3"]));
    }
}
