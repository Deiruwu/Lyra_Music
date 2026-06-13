use image::ImageReader;
use std::io::Cursor;

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