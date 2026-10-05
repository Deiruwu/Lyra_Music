use std::io::Cursor;
use std::path::Path;

use image::{ImageFormat, ImageReader};

use crate::ui::utils::image_fetch::fetch_image_bytes;

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

/// Recuadro cuadrado en píxeles de la imagen original.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRegion {
    pub x: u32,
    pub y: u32,
    pub side: u32,
}

/// Recorta a cuadrado (centrado), redimensiona a `max_size` y re-encodea a JPEG.
pub fn crop_and_encode_cover(bytes: &[u8], max_size: u32) -> Option<Vec<u8>> {
    let img = decode(bytes)?;
    let side = img.width().min(img.height());
    let region = CropRegion { x: (img.width() - side) / 2, y: (img.height() - side) / 2, side };
    encode_region(&img, region, max_size)
}

/// Recorta `region` (ajustada a los bordes), redimensiona a `max_size` y re-encodea a JPEG.
pub fn crop_region_and_encode(bytes: &[u8], region: CropRegion, max_size: u32) -> Option<Vec<u8>> {
    encode_region(&decode(bytes)?, region, max_size)
}

/// Previsualización para el editor de recorte: `(jpeg, ancho, alto)` originales, reducida a `max_side`.
pub fn load_crop_preview(path: &Path, max_side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = decode(&bytes).ok_or_else(|| "No se pudo leer la imagen".to_string())?;
    let (width, height) = (img.width(), img.height());

    let preview = if width.max(height) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    let mut out = Vec::new();
    preview.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg).map_err(|e| e.to_string())?;
    Ok((out, width, height))
}

fn decode(bytes: &[u8]) -> Option<image::DynamicImage> {
    ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()
}

fn encode_region(img: &image::DynamicImage, region: CropRegion, max_size: u32) -> Option<Vec<u8>> {
    let side = region.side.min(img.width()).min(img.height()).max(1);
    let x = region.x.min(img.width() - side);
    let y = region.y.min(img.height() - side);
    let cropped = img.crop_imm(x, y, side, side);

    let resized = if side > max_size {
        cropped.resize(max_size, max_size, image::imageops::FilterType::Triangle)
    } else {
        cropped
    };

    let mut out = Vec::new();
    resized.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg).ok()?;
    Some(out)
}

/// Descarga (con reintentos), recorta a cuadrado, reescala a `max_side` y re-encodea a JPEG.
/// El reescalado importa: las fuentes vienen a resolución completa y se
/// pintaban a 60px (barra del player) o ~544px (teatro).
pub async fn download_thumbnail(url: String, max_side: u32) -> Result<Vec<u8>, String> {
    let bytes = fetch_image_bytes(&url, None).await?;
    crop_and_encode_cover(&bytes, max_side).ok_or_else(|| format!("imagen inválida: {url}"))
}

/// Reescribe el sufijo `=wN-hN` de una URL del CDN de Google para pedir la
/// imagen cuadrada de `side` px. Sin ese sufijo, la devuelve tal cual.
pub fn square_image_url(url: &str, side: u32) -> String {
    let Some(start) = url.rfind("=w") else { return url.to_string() };

    let after_w = &url[start + 2..];
    let width_len = after_w.chars().take_while(char::is_ascii_digit).count();
    let Some(after_h) = after_w[width_len..].strip_prefix("-h") else { return url.to_string() };
    let height_len = after_h.chars().take_while(char::is_ascii_digit).count();

    if width_len == 0 || height_len == 0 {
        return url.to_string();
    }

    format!("{}=w{side}-h{side}{}", &url[..start], &after_h[height_len..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn jpeg_of(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::new(width, height));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg).unwrap();
        bytes
    }

    fn dims(bytes: &[u8]) -> (u32, u32) {
        let img = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format().unwrap().decode().unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn recorta_a_cuadrado_y_topea_el_lado() {
        // 1200x800 -> recorte cuadrado 800x800 -> tope 128.
        let out = crop_and_encode_cover(&jpeg_of(1200, 800), 128).unwrap();
        assert_eq!(dims(&out), (128, 128));
    }

    #[test]
    fn no_agranda_una_fuente_mas_chica_que_el_tope() {
        // 64x64 con tope 128 debe quedarse en 64: reescalar hacia arriba
        // solo gastaría memoria y se vería igual de blando.
        let out = crop_and_encode_cover(&jpeg_of(64, 64), 128).unwrap();
        assert_eq!(dims(&out), (64, 64));
    }

    #[test]
    fn recorta_la_region_elegida_y_la_ajusta_a_los_bordes() {
        // Un recuadro de 500 que se sale por la derecha de una imagen 600x400
        // queda en el lado menor (400) y pegado al borde.
        let region = CropRegion { x: 550, y: 0, side: 500 };
        let out = crop_region_and_encode(&jpeg_of(600, 400), region, 1024).unwrap();
        assert_eq!(dims(&out), (400, 400));
    }

    #[test]
    fn codifica_imagenes_con_transparencia() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::new(200, 100));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        assert_eq!(dims(&crop_and_encode_cover(&png, 512).unwrap()), (100, 100));
    }

    #[test]
    fn el_recorte_cuadrado_usa_el_lado_menor() {
        // 300x900 -> cuadrado de 300, por debajo del tope de 512.
        let out = crop_and_encode_cover(&jpeg_of(300, 900), 512).unwrap();
        assert_eq!(dims(&out), (300, 300));
    }
}
