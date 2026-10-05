use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::widget::image::Handle;
use iced::Task;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader};

use crate::ui::utils::async_thumbnail::DropGuard;
use crate::ui::utils::image_fetch::fetch_image_bytes;

enum Slot {
    /// Nunca se lee: soltar el guard cancela la descarga si la imagen deja de hacer falta.
    Loading(#[allow(dead_code)] DropGuard),
    Ready(Handle),
}

/// Cómo procesar una imagen al descargarla.
#[derive(Debug, Clone, Copy)]
pub enum Treatment {
    /// Reduce (preservando aspecto) si excede este lado máximo.
    MaxSide(u32),
    /// Recorta manteniendo solo esta fracción superior de la altura (0.0-1.0),
    /// y luego reduce si el lado mayor excede el segundo parámetro. El tope
    /// importa: las fuentes de banner llegan a 2880x1200, y antes se
    /// guardaban recortadas pero SIN reducir.
    TopCrop(f32, u32),
}

/// Gestor de imágenes de tamaño variable (portadas de álbum, banner de
/// artista): a diferencia de `AsyncThumbnail`, no recorta — reduce la
/// imagen si excede el tope pedido por target, preservando su aspecto.
#[derive(Default)]
pub struct GalleryThumbnail {
    slots: HashMap<String, Slot>,
}

impl GalleryThumbnail {
    pub fn new() -> Self {
        Self { slots: HashMap::new() }
    }

    /// `wanted` es `(key, url, tratamiento)`.
    pub fn sync<Msg: 'static + Send>(
        &mut self,
        wanted: &[(String, String, Treatment)],
        to_message: impl Fn(String, Vec<u8>) -> Msg + Send + Sync + 'static + Clone,
    ) -> Task<Msg> {
        let wanted_keys: HashSet<&str> = wanted.iter().map(|(k, _, _)| k.as_str()).collect();

        self.slots.retain(|k, _| wanted_keys.contains(k.as_str()));

        let mut tasks = Vec::new();
        for (key, url, treatment) in wanted {
            if self.slots.contains_key(key) {
                continue;
            }

            let (guard, abort_flag) = DropGuard::new();
            self.slots.insert(key.clone(), Slot::Loading(guard));

            let key = key.clone();
            let url = url.clone();
            let treatment = *treatment;
            let to_message = to_message.clone();

            tasks.push(Task::perform(download_with_abort(url, abort_flag, treatment), move |bytes| {
                to_message(key.clone(), bytes)
            }));
        }

        Task::batch(tasks)
    }

    pub fn on_loaded(&mut self, key: String, bytes: Vec<u8>) {
        if bytes.is_empty() || !self.slots.contains_key(&key) {
            return;
        }
        self.slots.insert(key, Slot::Ready(Handle::from_bytes(bytes)));
    }

    pub fn get(&self, key: &str) -> Option<&Handle> {
        match self.slots.get(key)? {
            Slot::Ready(h) => Some(h),
            Slot::Loading(_) => None,
        }
    }
}

/// Descarga, decodifica y aplica el `Treatment` pedido.
async fn download_with_abort(url: String, aborted: Arc<AtomicBool>, treatment: Treatment) -> Vec<u8> {
    let Ok(bytes) = fetch_image_bytes(&url, Some(&aborted)).await else { return Vec::new() };

    if aborted.load(Ordering::Relaxed) { return Vec::new(); }

    let Ok(reader) = ImageReader::new(Cursor::new(&bytes)).with_guessed_format() else {
        return Vec::new();
    };

    let Ok(img) = reader.decode() else {
        return Vec::new();
    };

    let img = match treatment {
        Treatment::MaxSide(max) if img.width() > max || img.height() > max => {
            img.resize(max, max, FilterType::Lanczos3)
        }
        Treatment::MaxSide(_) => img,
        Treatment::TopCrop(fraction, max_side) => {
            let cropped_height = (img.height() as f32 * fraction.clamp(0.0, 1.0)) as u32;
            let cropped = img.crop_imm(0, 0, img.width(), cropped_height.max(1));

            if cropped.width() > max_side || cropped.height() > max_side {
                cropped.resize(max_side, max_side, FilterType::Lanczos3)
            } else {
                cropped
            }
        }
    };

    let mut out = Vec::new();
    if img.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg).is_err() {
        return Vec::new();
    }

    out
}
