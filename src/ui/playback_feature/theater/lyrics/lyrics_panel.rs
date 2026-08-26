use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::animation::{Animation, Easing};
use iced::widget::{column, container, mouse_area, scrollable, text, Id as WidgetId};
use iced::widget::scrollable::RelativeOffset;
use iced::{Element, Length, Padding, Task};
use iced::widget::operation::snap_to;
use crate::model::audio_tech::PlayableTrack;
use super::lrc_parser::{parse_lrc, SyncedLyrics};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::text as text_style;
use crate::ui::theme::theme;

const LINE_SHIFT_PX: f32 = 10.0;

#[derive(Debug, Clone)]
pub enum LyricsMessage {
    TrackChanged(Arc<PlayableTrack>),
    Loaded {
        track_id: String,
        lyrics: Option<SyncedLyrics>,
    },
    PositionUpdated(Duration),
    AnimationFrame(Instant),
    LineClicked(Duration),
}

#[derive(Debug, Clone)]
pub enum LyricsOutMessage {
    RequestSeek(Duration),
    Idle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LyricsStatus {
    NoTrack,
    Loading,
    NotFound,
    Synced(SyncedLyrics),
}

struct LineAnim {
    index: usize,
    weight: Animation<f32>,
}

pub struct LyricsPanel {
    status: LyricsStatus,
    current_track_id: Option<String>,
    current_line: Option<usize>,
    anims: Vec<LineAnim>,
    scroll_id: WidgetId,
}

impl Default for LyricsPanel {
    fn default() -> Self {
        Self {
            status: LyricsStatus::NoTrack,
            current_track_id: None,
            current_line: None,
            anims: Vec::new(),
            scroll_id: WidgetId::unique(),
        }
    }
}

impl LyricsPanel {
    pub fn update(&mut self, msg: LyricsMessage) -> (Task<LyricsMessage>, LyricsOutMessage) {
        match msg {
            LyricsMessage::TrackChanged(playable) => {
                let track_id = playable.track.id.clone();
                self.current_track_id = Some(track_id.clone());
                self.current_line = None;
                self.anims.clear();

                let Some(lrc_path) = lrc_path_for(&playable) else {
                    self.status = LyricsStatus::NotFound;
                    return (Task::none(), LyricsOutMessage::Idle);
                };

                self.status = LyricsStatus::Loading;

                let task = Task::perform(load_lrc(lrc_path), move |lyrics| LyricsMessage::Loaded {
                    track_id: track_id.clone(),
                    lyrics,
                });
                (task, LyricsOutMessage::Idle)
            }

            LyricsMessage::Loaded { track_id, lyrics } => {
                if self.current_track_id.as_deref() == Some(track_id.as_str()) {
                    self.status = match lyrics {
                        Some(l) => LyricsStatus::Synced(l),
                        None => LyricsStatus::NotFound,
                    };
                }
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::PositionUpdated(position) => {
                if let LyricsStatus::Synced(lyrics) = &self.status {
                    let new_line = lyrics.current_line_index(position);
                    if new_line != self.current_line {
                        let total = lyrics.lines.len().max(1);

                        self.current_line = new_line;
                        self.retarget_anims(now());

                        if let Some(idx) = new_line {
                            let offset_y = (idx as f32 / total as f32).clamp(0.0, 1.0);
                            let task = snap_to(
                                self.scroll_id.clone(),
                                RelativeOffset { x: 0.0, y: offset_y },
                            );
                            return (task, LyricsOutMessage::Idle);
                        }
                    }
                }
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::AnimationFrame(_) => {
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::LineClicked(timestamp) => {
                (Task::none(), LyricsOutMessage::RequestSeek(timestamp))
            }
        }
    }

    fn retarget_anims(&mut self, now: Instant) {
        let current = self.current_line;

        for anim in self.anims.iter_mut() {
            if Some(anim.index) != current && anim.weight.value() > 0.0 {
                anim.weight.go_mut(0.0, now);
            }
        }

        if let Some(idx) = current {
            if let Some(anim) = self.anims.iter_mut().find(|a| a.index == idx) {
                anim.weight.go_mut(1.0, now);
            } else {
                self.anims.push(LineAnim {
                    index: idx,
                    weight: Animation::new(0.0).easing(Easing::EaseOut).slow().go(1.0, now),
                });
            }
        }

        self.anims.retain(|a| Some(a.index) == current || a.weight.value() > 0.001);
    }

    fn weight_of(&self, index: usize, now: Instant) -> f32 {
        self.anims
            .iter()
            .find(|a| a.index == index)
            .map(|a| a.weight.interpolate_with(|w| w, now))
            .unwrap_or(if Some(index) == self.current_line { 1.0 } else { 0.0 })
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.anims.iter().any(|a| a.weight.is_animating(now))
    }

    pub fn has_lyrics(&self) -> bool {
        matches!(self.status, LyricsStatus::Synced(_))
    }

    pub fn status(&self) -> &LyricsStatus {
        &self.status
    }

    pub fn view(&self) -> Element<'_, LyricsMessage> {
        let content: Element<'_, LyricsMessage> = match &self.status {
            LyricsStatus::NoTrack => status_message("Sin reproducción activa"),

            LyricsStatus::Loading => status_message("Buscando letra..."),

            LyricsStatus::NotFound => status_message("Letra no disponible"),

            LyricsStatus::Synced(lyrics) => {
                let now = now();
                let mut lines_col = column![].spacing(spacing::SP_14).width(Length::Fill);

                for (i, line) in lyrics.lines.iter().enumerate() {
                    let weight = self.weight_of(i, now);

                    let (size, color) = text_style::lyric_line(weight);
                    let offset_y = LINE_SHIFT_PX * (1.0 - weight);

                    let line_text = text(line.text.clone())
                        .font(SF_PRO)
                        .size(size)
                        .color(color);

                    let clickable_text = mouse_area(line_text)
                        .on_press(LyricsMessage::LineClicked(line.timestamp))
                        .interaction(iced::mouse::Interaction::Pointer);

                    let row = container(clickable_text)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(offset_y))
                        .align_x(iced::alignment::Horizontal::Center);

                    lines_col = lines_col.push(row);
                }

                scrollable(
                    container(lines_col)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(24.0).bottom(24.0)),
                )
                    .id(self.scroll_id.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }
}

fn now() -> Instant {
    Instant::now()
}

fn status_message(msg: &str) -> Element<'_, LyricsMessage> {
    text(msg.to_string())
        .font(SF_PRO)
        .size(typography::TEXT_15)
        .color(theme().content.faint)
        .into()
}

fn lrc_path_for(playable: &PlayableTrack) -> Option<PathBuf> {
    let file_path = playable.track.file_path.as_ref()?;
    if file_path.is_empty() {
        return None;
    }

    let path = Path::new(file_path);
    let lrc_path = path.with_extension("lrc");

    if lrc_path.exists() {
        Some(lrc_path)
    } else {
        None
    }
}

async fn load_lrc(path: PathBuf) -> Option<SyncedLyrics> {
    let content = tokio::fs::read_to_string(&path).await.ok()?;
    let parsed = parse_lrc(&content);

    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}