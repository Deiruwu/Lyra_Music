use std::path::PathBuf;

/// Abre el diálogo nativo de selección de archivos (vía `rfd`) para elegir
/// una imagen que servirá de portada de playlist. Devuelve `Some(path)` si el
/// usuario confirmó una selección, o `None` si canceló.
///
/// Vive como util suelta (no dentro de CoverManager ni de la UI) porque es
/// I/O de sistema puro: no sabe nada de playlists, DB ni iced.
pub async fn pick_cover_image() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Elegir portada de playlist")
        .add_filter("Imágenes", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
