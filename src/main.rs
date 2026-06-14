mod microservices;
mod model;
mod audio;
mod ui;

use std::sync::Arc;
use ui::playback_controls::{PlaybackMessage};
use iced::{border, Background, Border, Color, Element, Font, Length, Theme};
use iced::theme::Style;
use iced::widget::{column, container, row, space, stack};
use crate::audio::discord::DiscordPresence;
use crate::audio::engine::AudioEngine;
use crate::audio::manager::TrackManager;
use crate::audio::mpris::MprisServer;
use crate::ui::playback_controls::{PlaybackControls};
use crate::ui::search_bar::{SearchInput, SearchMessage};

const JETBRAINS_MONO: Font = Font::with_name("JetBrainsMono Nerd Font");

#[derive(Debug, Clone)]
pub enum AppMessage {
    Playback(PlaybackMessage),
    Search(SearchMessage),

}

struct App {
    _engine: AudioEngine,
    playback: PlaybackControls,
    search_bar: SearchInput,
}

impl App {
    pub fn init() -> (Self, iced::Task<AppMessage>) {
        let (manager, engine) = TrackManager::new()
            .expect("Fallo fatal al inicializar el hardware de audio");

        let manager = Arc::new(manager);

        MprisServer::spawn(Arc::clone(&manager));
        DiscordPresence::spawn(Arc::clone(&manager));

        let app = Self {
            _engine: engine,
            playback: PlaybackControls::new(Arc::clone(&manager)),
            search_bar: SearchInput::new(),
        };

        (app, iced::Task::none())
    }

    pub fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        match message {


            AppMessage::Playback(msg) => {
                let playback_task = self.playback.update(msg);

                playback_task.map(AppMessage::Playback)
            }

            AppMessage::Search(msg) => {
                let search_task = self.search_bar.update(msg.clone());
                if let SearchMessage::DownloadFinished(Ok(playable)) = msg {
                    let _ = self.playback.update(PlaybackMessage::Play(playable));
                }

                search_task.map(AppMessage::Search)
            }
        }
    }
    pub fn view(&self) -> Element<'_, AppMessage> {
        let center_view = container(space())
            .width(Length::FillPortion(8))
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.20))),
                border: Border {
                    radius: border::Radius::from(18.0),
                    ..Default::default()
                },
                ..Default::default()
            });

        let layout = row![
            space().width(15),
            center_view.width(Length::Fill),
            space().width(15),
        ]
            .width(Length::Fill)
            .height(Length::Fill);

        let playback_view = self.playback.view().map(AppMessage::Playback);
        let search_view = self.search_bar.view().map(AppMessage::Search);

        let base = column![
            search_view,
            layout,
            playback_view,
        ].height(Length::Fill);

        let overlay = self.search_bar.view_dropdown().map(AppMessage::Search);

        stack![
            base,
            overlay,
        ].into()
    }
}

impl App {
    pub fn subscription(&self) -> iced::Subscription<AppMessage> {
        let progress_sub = iced::time::every(std::time::Duration::from_millis(50))
            .map(|_| AppMessage::Playback(PlaybackMessage::Tick));

        let backend_sub = self.playback.subscription()
            .map(AppMessage::Playback);

        let search_sub = self.search_bar.subscription()
            .map(AppMessage::Search);

        iced::Subscription::batch(vec![
            progress_sub,
            backend_sub,
            search_sub,
        ])
    }
}
// ==========================================
// PUNTO DE ENTRADA
// ==========================================
fn main() -> iced::Result {
    let _ = dotenvy::dotenv();

    iced::application(App::init, App::update, App::view)
        .subscription(App::subscription)
        .title("Atelier Player")
        .window(iced::window::Settings {
            size: iced::Size::new(800.0, 600.0),
            ..Default::default()
        })
        .font(include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf"))
        .theme(Theme::Dracula)
        .style(|_state, _theme| Style {
            background_color: Color::from_rgb(0.1, 0.1, 0.1),
            text_color: Color::WHITE,
        })
        .run()
}
