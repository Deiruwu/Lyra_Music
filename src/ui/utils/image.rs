use std::io::Cursor;
use std::path::Path;

use image::{ImageFormat, ImageReader};

/// Carga una imagen desde un archivo LOCAL (sin pasar por el semáforo de
/// thumbnails, que es para URLs remotas): la recorta a cuadrado, la redimensiona
/// a `max_size` px por lado (para que la versión guardada sea liviana) y la
/// re-encodea a JPEG. Devuelve `None` si no existe, no se puede leer o decodificar.
///
/// Se usa para las portadas de playlists, que siempre son un archivo local
/// (`~/.local/share/lyra/covers/<id>.jpg`).
pub fn load_local_cover(path: &str, max_size: u32) -> Option<Vec<u8>> {
    let bytes = std::fs::read(Path::new(path)).ok()?;
    if bytes.is_empty() {
        return None;
    }
    crop_and_encode_cover(&bytes, max_size)
}

/// Recorta a cuadrado, redimensiona a `max_size` y re-encodea a JPEG.
pub fn crop_and_encode_cover(bytes: &[u8], max_size: u32) -> Option<Vec<u8>> {
    let img = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;

    let (w, h) = (img.width(), img.height());
    let size = w.min(h);
    let x = (w - size) / 2;
    let y = (h - size) / 2;
    let cropped = img.crop_imm(x, y, size, size);

    let resized = if size > max_size {
        cropped.resize(max_size, max_size, image::imageops::FilterType::Triangle)
    } else {
        cropped
    };

    let mut out = Vec::new();
    resized.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg).ok()?;
    Some(out)
}

pub async fn download_thumbnail(url: String) -> Result<Vec<u8>, String> {
    let bytes = reqwest::get(&url)
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let img = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;

    let (w, h) = (img.width(), img.height());
    let size = w.min(h);
    let x = (w - size) / 2;
    let y = (h - size) / 2;

    let cropped = img.crop_imm(x, y, size, size);

    let mut out = Vec::new();
    cropped.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;

    Ok(out)
}