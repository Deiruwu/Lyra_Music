use iced::widget::{column, space};
use iced::{Element, Length, Task};

use crate::model::{Mix, Track};
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::gallery_thumbnail::{GalleryThumbnail, Treatment};
use crate::ui::views::states_view::{ListAction, TrackViewState, ROW_HEIGHT};
use crate::ui::widgets::playlist_header::{playlist_header, HeaderCover, PlaylistHeaderData};
use crate::ui::widgets::selection_state::SelectionStep;
use crate::ui::widgets::track_list_builder::{sort_tracks, TrackBuilder, TrackEvent};

const SCROLL_ID: &str = "mix_view_scroll";
const COVER_KEY: &str = "mix_cover";
const COVER_MAX_SIDE: u32 = 500;

/// Mezcla temporal mostrada como una playlist: header de playlist y la misma tabla con miniaturas.
pub struct MixView {
    mix: Mix,
    /// Selección/scroll/orden de la tabla — `pub` para que `LibraryBrowserFeature`
    /// guarde/restaure el scroll al navegar.
    pub list: TrackViewState,
    thumbnails: AsyncThumbnail,
    cover: GalleryThumbnail,
}

#[derive(Debug, Clone)]
pub enum MixMessage {
    Table(TrackEvent),
    PlayAll,
    TogglePlayback,
    ThumbnailLoaded(String, Vec<u8>),
    CoverLoaded(String, Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum MixOutMessage {
    Idle,
    PlayTrack(String),
    PlayAll,
    TrackRightClicked(String),
    OpenArtist(String),
    OpenAlbum(String),
    RequestTogglePlayback,
}

impl MixView {
    pub fn new(mix: Mix) -> (Self, Task<MixMessage>) {
        let mut view = Self {
            mix,
            list: TrackViewState::new(),
            thumbnails: AsyncThumbnail::new(128),
            cover: GalleryThumbnail::new(),
        };
        let task = view.sync_images();
        (view, task)
    }

    pub fn update(&mut self, message: MixMessage) -> (Task<MixMessage>, MixOutMessage) {
        let out = match message {
            MixMessage::Table(event) => {
                let rendered = ordered(&self.mix.tracks, &self.list);
                match self.list.process_event(event, &rendered) {
                    ListAction::PlayContext(id) => MixOutMessage::PlayTrack(id),
                    ListAction::OpenContextMenu { anchor_id, .. } => MixOutMessage::TrackRightClicked(anchor_id),
                    ListAction::OpenArtist(id) => MixOutMessage::OpenArtist(id),
                    ListAction::OpenAlbum(id) => MixOutMessage::OpenAlbum(id),
                    ListAction::TogglePlayback => MixOutMessage::RequestTogglePlayback,
                    ListAction::SortChanged(_) | ListAction::None => MixOutMessage::Idle,
                }
            }
            MixMessage::PlayAll => MixOutMessage::PlayAll,
            MixMessage::TogglePlayback => MixOutMessage::RequestTogglePlayback,
            MixMessage::ThumbnailLoaded(key, bytes) => {
                self.thumbnails.on_loaded(key, bytes);
                MixOutMessage::Idle
            }
            MixMessage::CoverLoaded(key, bytes) => {
                self.cover.on_loaded(key, bytes);
                MixOutMessage::Idle
            }
        };

        (self.sync_images(), out)
    }

    pub fn view(&self, now_playing_id: Option<String>, is_playing: bool) -> Element<'_, MixMessage> {
        let rendered = ordered(&self.mix.tracks, &self.list);

        let is_current = now_playing_id
            .as_deref()
            .is_some_and(|id| self.mix.tracks.iter().any(|t| t.id == id));

        let header = playlist_header(
            PlaylistHeaderData {
                name: &self.mix.title,
                kicker: Some("MEZCLA"),
                description: Some(&self.mix.subtitle),
                track_count: self.mix.tracks.len(),
                total_duration_seconds: self.mix.tracks.iter().map(|t| t.duration_seconds as i64).sum(),
                tint: None,
            },
            HeaderCover::Single(self.cover.get(COVER_KEY).cloned()),
            if is_current { MixMessage::TogglePlayback } else { MixMessage::PlayAll },
            None,
            None,
            is_current && is_playing,
            None,
        );

        let table = TrackBuilder::new(
            rendered,
            &self.list.scroll,
            &self.thumbnails,
            &self.list.tracks_selection.selected_ids,
            SCROLL_ID,
        )
            .index_sortable()
            .sort(self.list.active_sort_key, self.list.sort_direction_asc)
            .playing(now_playing_id, is_playing)
            .icon_hovered(self.list.playing_icon_hovered)
            .on_event(MixMessage::Table)
            .build();

        column![header, space().height(Length::Fixed(16.0)), table]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn mix(&self) -> &Mix {
        &self.mix
    }

    /// Canciones en el orden que muestra la tabla, para armar `play_context`.
    pub(crate) fn tracks_in_order(&self) -> Vec<Track> {
        ordered(&self.mix.tracks, &self.list).into_iter().cloned().collect()
    }

    pub(crate) fn find_track(&self, id: &str) -> Option<&Track> {
        self.mix.tracks.iter().find(|t| t.id == id)
    }

    /// Id de la canción bajo el cursor de selección (la que reproduce Enter).
    pub(crate) fn selected_track_id(&self) -> Option<&str> {
        self.list.cursor_track_id(&ordered(&self.mix.tracks, &self.list))
    }

    /// Mueve la selección (flechas, RePág/AvPág) y la mantiene a la vista.
    pub(crate) fn move_selection(&mut self, step: SelectionStep) -> Task<MixMessage> {
        let rendered = ordered(&self.mix.tracks, &self.list);
        let delta = step.rows(self.list.scroll.rows_per_page(ROW_HEIGHT));
        let Some(index) = self.list.move_selection(delta, false, &rendered) else {
            return Task::none();
        };
        self.list.scroll.reveal(index as f32 * ROW_HEIGHT, ROW_HEIGHT, SCROLL_ID)
    }

    /// Reemplaza el track recién descargado/analizado en otra parte de la app.
    pub(crate) fn patch_track(&mut self, track: &Track) {
        if let Some(existing) = self.mix.tracks.iter_mut().find(|t| t.id == track.id) {
            *existing = track.clone();
        }
    }

    pub(crate) fn scroll_id() -> &'static str {
        SCROLL_ID
    }

    /// Portada (primera canción) y miniaturas de las filas visibles.
    fn sync_images(&mut self) -> Task<MixMessage> {
        let rendered = ordered(&self.mix.tracks, &self.list);
        let rows_task = self.thumbnails.sync(&self.list.visible_thumbnail_targets(&rendered), MixMessage::ThumbnailLoaded);

        let cover_targets: Vec<(String, String, Treatment)> = self.mix.cover_url
            .iter()
            .map(|url| (COVER_KEY.to_string(), url.clone(), Treatment::MaxSide(COVER_MAX_SIDE)))
            .collect();
        let cover_task = self.cover.sync(&cover_targets, MixMessage::CoverLoaded);

        Task::batch([rows_task, cover_task])
    }
}

/// Las canciones de la mezcla con el orden elegido en la tabla (sin columna = orden original).
fn ordered<'a>(tracks: &'a [Track], list: &TrackViewState) -> Vec<&'a Track> {
    let mut rendered: Vec<&Track> = tracks.iter().collect();
    let (sort_key, ascending) = list.effective_sort();
    sort_tracks(&mut rendered, sort_key, ascending);
    rendered
}
