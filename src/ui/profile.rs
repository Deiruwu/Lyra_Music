//! Perfil del usuario: foto circular y nombre (la foto vive en `<data_dir>/lyra/profile.jpg`).

use std::path::{Path, PathBuf};

use iced::widget::image::Handle;
use iced::widget::{container, image, text};
use iced::{Alignment, ContentFit, Element, Length};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{icon, Icon};
use crate::ui::theme::theme;
use crate::ui::utils::data_dir::data_dir;
use crate::ui::utils::image::{crop_region_and_encode, CropRegion};

/// Lado al que se guarda la foto de perfil.
const PHOTO_SIDE: u32 = 256;

pub fn photo_path() -> PathBuf {
    data_dir().join("lyra").join("profile.jpg")
}

/// Foto guardada, si hay.
pub fn load_photo() -> Option<Handle> {
    std::fs::read(photo_path()).ok().filter(|bytes| !bytes.is_empty()).map(Handle::from_bytes)
}

/// Recorta `source` y lo guarda como foto de perfil; devuelve los bytes guardados.
pub fn save_photo(source: &Path, region: CropRegion) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    let cropped = crop_region_and_encode(&bytes, region, PHOTO_SIDE).ok_or_else(|| "No se pudo recortar la imagen".to_string())?;
    let path = photo_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, &cropped).map_err(|e| e.to_string())?;
    Ok(cropped)
}

pub fn remove_photo() -> Result<(), String> {
    match std::fs::remove_file(photo_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Foto circular; sin foto, la inicial del nombre (o un ícono) sobre un círculo neutro.
pub fn avatar<'a, Message: 'a>(photo: Option<&Handle>, name: &str, size: f32) -> Element<'a, Message> {
    if let Some(handle) = photo {
        return image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(ContentFit::Cover)
            .border_radius(size / 2.0)
            .into();
    }

    let glyph: Element<'a, Message> = match name.trim().chars().next() {
        Some(initial) => text(initial.to_uppercase().to_string())
            .font(SF_PRO)
            .size(size * 0.45)
            .color(theme().content.secondary)
            .into(),
        None => icon(Icon::Account, size * 0.55).color(theme().content.secondary).into(),
    };

    container(glyph)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(theme().surface.control.into()),
            border: iced::border::rounded(size / 2.0),
            ..Default::default()
        })
        .into()
}
