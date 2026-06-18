use iced::{border, Alignment, Color, Element, Length, Padding, Task, Theme};
use iced::widget::{button, column, container, row, space, text, text_input};
use crate::model::Track;
use crate::ui::widgets::icon_toggle::IconToggle;
use crate::ui::widgets::track_row::track_row;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    Songs,
    Videos,
}

#[derive(Debug, Clone)]
pub enum SearchMessage {
    InputChanged(String),
    Submit,
    TrackClicked(Track),
    FilterChanged(SearchFilter),
    Tick,
}

#[derive(Debug, Clone)]
pub enum SearchOutMessage {
    Idle,
    RequestSearch(String, SearchFilter),
    RequestDownloadAndPlay(Track),
}

pub struct SearchInput {
    pub input_value: String,
    pub filter: SearchFilter,
    pub thumb_offset: f32,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            input_value: String::new(),
            filter: SearchFilter::Songs,
            thumb_offset: 0.0,
        }
    }
}

impl SearchInput {
    pub fn update(&mut self, msg: SearchMessage) -> (Task<SearchMessage>, SearchOutMessage) {
        match msg {
            SearchMessage::Tick => {
                let target = if self.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
                let diff = target - self.thumb_offset;

                if diff.abs() > 0.5 {
                    self.thumb_offset += diff * 0.3;
                } else {
                    self.thumb_offset = target;
                }
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::InputChanged(value) => {
                self.input_value = value;
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::RequestSearch(String::new(), self.filter.clone()))
                } else {
                    (Task::none(), SearchOutMessage::Idle)
                }
            }

            SearchMessage::FilterChanged(filter) => {
                self.filter = filter.clone();
                if !self.input_value.is_empty() {
                    return (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), filter));
                }
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::Submit => {
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::Idle)
                } else {
                    (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), self.filter.clone()))
                }
            }

            SearchMessage::TrackClicked(track) => {
                self.input_value.clear();
                (Task::none(), SearchOutMessage::RequestDownloadAndPlay(track))
            }
        }
    }

    pub fn view(&self) -> Element<'_, SearchMessage> {
        let is_videos = self.filter == SearchFilter::Videos;
        let search_bar = row![
            text_input("Buscar canción, álbum, artista...", &self.input_value)
                .on_input(SearchMessage::InputChanged)
                .on_submit(SearchMessage::Submit)
                .padding(10)
                .width(Length::Fill),

            IconToggle::new(
                is_videos,
                self.thumb_offset,
                |next_state| {
                    SearchMessage::FilterChanged(if next_state {
                        SearchFilter::Videos
                    } else {
                        SearchFilter::Songs
                    })
                }
            ).build(),

            button("Buscar")
                .on_press(SearchMessage::Submit)
                .padding(10)
        ]
            .spacing(10)
            .align_y(Alignment::Center);

        container(search_bar)
            .padding(20)
            .width(Length::Fill)
            .into()
    }

    pub fn view_dropdown<'a>(
        &'a self,
        is_searching: bool,
        results: &'a [Track],
        thumbnails: &'a ThumbnailCache
    ) -> Element<'a, SearchMessage> {

        let show_dropdown = !results.is_empty() || is_searching;

        if !show_dropdown {
            return space().into();
        }

        let mut results_column = column![].spacing(10);

        if is_searching {
            results_column = results_column.push(text("Buscando...").size(16));
        } else {
            for track in results {
                let thumbnail = thumbnails.peek(&track.id);
                results_column = results_column.push(
                    track_row(track, thumbnail, SearchMessage::TrackClicked(track.clone()))
                );
            }
        }

        column![
            space().height(70),
            container(results_column)
                .padding(18)
                .width(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color::from_rgb(0.15, 0.15, 0.15).into()),
                    border: border::color(Color::from_rgb(0.3, 0.3, 0.3)).width(1.0).rounded(8),
                    ..Default::default()
                }),
        ]
            .padding(Padding::default().left(20).right(20))
            .into()
    }
}