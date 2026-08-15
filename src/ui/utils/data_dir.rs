use std::env;
use std::path::PathBuf;

/// Directorio base de datos de la aplicación según la especificación XDG:
/// `$XDG_DATA_HOME` si está definido, o `~/.local/share` por defecto.
/// `$HOME` es el fallback si no se puede resolver el home del usuario.
pub fn data_dir() -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".local").join("share");
    }
    PathBuf::from(".")
}

/// Directorio donde viven las portadas de playlists recortadas a cuadrado,
/// una por playlist: `<data_dir>/lyra/covers/<playlist_id>.jpg`.
///
/// Se crea on-demand ([`ensure_covers_dir`]) en el momento de escribir la
/// primera portada; el dir en sí se resuelve siempre igual.
pub fn covers_dir() -> PathBuf {
    data_dir().join("lyra").join("covers")
}

/// Crea el directorio de portadas (y sus padres) si no existe. Devuelve
/// `Ok(dir)` si quedó listo, o el error de I/O si no se pudo crear.
pub fn ensure_covers_dir() -> std::io::Result<PathBuf> {
    let dir = covers_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
