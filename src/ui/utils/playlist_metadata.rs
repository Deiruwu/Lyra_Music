use crate::model::Track;

/// Formatea segundos totales al estilo Spotify: "3 h 24 min" si hay
/// horas, o "42 m" si dura menos de una hora. Nunca muestra segundos
/// sueltos en el total (a diferencia de la duración por track, que sí
/// usa mm:ss) — así se ve la convención habitual de "duración de
/// colección" en vez de "duración de una canción".
pub fn format_total_duration(total_seconds: i64) -> String {
    let total_minutes = total_seconds / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;

    if hours > 0 {
        format!("{} h {} m", hours, minutes)
    } else {
        format!("{} m", minutes)
    }
}

/// Formatea la cantidad de canciones de una playlist: "1 canción" o
/// "N canciones".
pub fn format_track_count(count: usize) -> String {
    if count == 1 {
        "1 canción".to_string()
    } else {
        format!("{} canciones", count)
    }
}

/// Obtiene la metadata de una playlist a partir de sus tracks:
/// `(cantidad de canciones, duración total en segundos)`.
pub fn track_stats<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> (usize, i64) {
    let mut count = 0usize;
    let mut total = 0i64;
    for track in tracks {
        count += 1;
        total += track.duration_seconds as i64;
    }
    (count, total)
}
