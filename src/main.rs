mod microservices;
mod model;
mod audio;
mod ui;

use std::sync::Arc;
use iced::{border, Background, Border, Color, Element, Font, Length, Padding, Theme};
use iced::theme::Style;
use iced::widget::{column, container, row, space, stack};
use crate::audio::discord::DiscordPresence;
use crate::audio::engine::AudioEngine;
use crate::audio::manager::TrackManager;
use crate::audio::mpris::MprisServer;
use crate::ui::playback_feature::playback_feature::{PlaybackFeature, PlaybackFeatureMessage};
use crate::ui::search_feature::search_feature::{SearchFeature, SearchFeatureMessage, SearchFeatureOutMessage};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

const JETBRAINS_MONO: Font = Font::with_name("JetBrainsMono Nerd Font");

#[derive(Debug, Clone)]
pub enum AppMessage {
    SearchFeature(SearchFeatureMessage),
    PlaybackFeature(PlaybackFeatureMessage),
}

struct App {
    _engine: AudioEngine,
    search_feature: SearchFeature,
    playback_feature: PlaybackFeature,
    thumbnails: ThumbnailCache,
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
            search_feature: SearchFeature::new(),
            playback_feature: PlaybackFeature::new(Arc::clone(&manager)),
            thumbnails: ThumbnailCache::new(100),
        };

        (app, iced::Task::none())
    }

    pub fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        match message {
            AppMessage::PlaybackFeature(msg) => {
                self.playback_feature.update(msg, &mut self.thumbnails).map(AppMessage::PlaybackFeature)
            }

            AppMessage::SearchFeature(msg) => {
                let (search_task, out_msg) = self.search_feature.update(msg, &mut self.thumbnails);

                let mut feature_task = iced::Task::none();

                if let SearchFeatureOutMessage::TrackReadyToPlay(playable) = out_msg {
                    feature_task = self.playback_feature.update(PlaybackFeatureMessage::Play(playable), &mut self.thumbnails);
                }

                iced::Task::batch(vec![
                    search_task.map(AppMessage::SearchFeature),
                    feature_task.map(AppMessage::PlaybackFeature),
                ])
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        let center_view = container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.20))),
                border: Border {
                    radius: border::Radius::from(18.0),
                    ..Default::default()
                },
                ..Default::default()
            });

        let queue_layer = container(
            self.playback_feature.view_queue(&self.thumbnails).map(AppMessage::PlaybackFeature)
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(
                Padding {
                    top: 10.0,
                    right: 20.00,
                    bottom: 10.0,
                    left: 0.0,
                }
            );

        let content_layer = row![
            space().width(15),
            center_view,
            space().width(15),
        ]
            .width(Length::Fill)
            .height(Length::Fill);

        let layout_stack = stack![
            content_layer,
            queue_layer,
        ];

        let playback_view = self.playback_feature.view(&self.thumbnails).map(AppMessage::PlaybackFeature);
        let search_view = self.search_feature.view().map(AppMessage::SearchFeature);
        let search_overlay = self.search_feature.view_dropdown(&self.thumbnails).map(AppMessage::SearchFeature);

        stack![
            column![
                search_view,
                layout_stack,
                playback_view,
            ]
            .height(Length::Fill),
            search_overlay,
        ]
            .into()
    }
}

impl App {
    pub fn subscription(&self) -> iced::Subscription<AppMessage> {
        let search_sub = self.search_feature.subscription()
            .map(AppMessage::SearchFeature);

        let feature_sub = self.playback_feature.subscription()
            .map(AppMessage::PlaybackFeature);

        iced::Subscription::batch(vec![
            search_sub,
            feature_sub,
        ])
    }
}

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