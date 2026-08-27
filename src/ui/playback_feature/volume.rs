use iced::{Element};
use iced::widget::{row, slider, text};
use iced::Task;
use crate::ui::assets::fonts::JETBRAINS_MONO;
use crate::ui::assets::spacing;
use crate::ui::styles::slider as slider_style;
use crate::ui::theme::theme;

#[derive(Debug, Clone)]
pub enum VolumeMessage {
    UiSliderChanged(f32),
}

#[derive(Debug, Clone)]
pub enum VolumeOutMessage {
    RequestVolumeChange(f32),
}

pub struct Volume;

impl Default for Volume {
    fn default() -> Self {
        Self {}
    }
}

impl Volume {
    pub fn update(&mut self, msg: VolumeMessage) -> (Task<VolumeMessage>, VolumeOutMessage) {
        match msg {
            VolumeMessage::UiSliderChanged(vol) => (Task::none(), VolumeOutMessage::RequestVolumeChange(vol)),
        }
    }

    pub fn view(&self, volume: f32) -> Element<'_, VolumeMessage> {
        let volume_icon = if volume < 0.2 { "󰕿" } else if volume > 0.6 { "󰕾" } else { "󰖀" };

        row![
            text(volume_icon).font(JETBRAINS_MONO).color(theme().content.primary),
            slider(0.0..=1.0, volume, VolumeMessage::UiSliderChanged)
                .step(0.01)
                .style(slider_style::track)
        ]
            .spacing(spacing::SP_20)
            .padding(spacing::SP_20)
            .into()
    }
}