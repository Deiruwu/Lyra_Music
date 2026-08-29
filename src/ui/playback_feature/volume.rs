use iced::{Alignment, Element};
use iced::widget::{button, row, slider};
use iced::Task;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::styles::slider as slider_style;

#[derive(Debug, Clone)]
pub enum VolumeMessage {
    UiSliderChanged(f32),
    IconClicked,
}

#[derive(Debug, Clone)]
pub enum VolumeOutMessage {
    RequestVolumeChange(f32),
}

pub struct Volume {
    /// Último volumen > 0 antes de mutear, para restaurarlo al desmutear.
    previous_volume: f32,
}

impl Default for Volume {
    fn default() -> Self {
        Self { previous_volume: 1.0 }
    }
}

impl Volume {
    pub fn update(&mut self, msg: VolumeMessage, current_volume: f32) -> (Task<VolumeMessage>, VolumeOutMessage) {
        match msg {
            VolumeMessage::UiSliderChanged(vol) => (Task::none(), VolumeOutMessage::RequestVolumeChange(vol)),
            VolumeMessage::IconClicked => {
                if current_volume > 0.0 {
                    self.previous_volume = current_volume;
                    (Task::none(), VolumeOutMessage::RequestVolumeChange(0.0))
                } else {
                    let restored = if self.previous_volume > 0.0 { self.previous_volume } else { 1.0 };
                    (Task::none(), VolumeOutMessage::RequestVolumeChange(restored))
                }
            }
        }
    }

    pub fn view(&self, volume: f32) -> Element<'_, VolumeMessage> {
        let icon_variant = if volume <= 0.0 {
            Icon::VolumeMuted
        } else if volume <= 0.33 {
            Icon::VolumeOff
        } else if volume <= 0.66 {
            Icon::VolumeDown
        } else {
            Icon::VolumeUp
        };

        let icon_button = button(icons::icon(icon_variant, typography::TEXT_16))
            .style(button_style::minimal)
            .on_press(VolumeMessage::IconClicked);

        row![
            icon_button,
            slider(0.0..=1.0, volume, VolumeMessage::UiSliderChanged)
                .step(0.01)
                .style(slider_style::track)
        ]
            .spacing(spacing::SP_20)
            .padding(spacing::SP_20)
            .align_y(Alignment::Center)
            .into()
    }
}
