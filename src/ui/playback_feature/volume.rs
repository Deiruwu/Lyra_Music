use iced::Element;
use iced::widget::{row, slider, text};
use iced::Task;
use crate::JETBRAINS_MONO;

#[derive(Debug, Clone)]
pub enum VolumeMessage {
    UiSliderChanged(f32),
}

#[derive(Debug, Clone)]
pub enum VolumeOutMessage {
    Idle,
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
            text(volume_icon).font(JETBRAINS_MONO),
            slider(0.0..=1.0, volume, VolumeMessage::UiSliderChanged)
                .step(0.01)
                .width(150)
        ]
            .spacing(20)
            .padding(20)
            .into()
    }
}