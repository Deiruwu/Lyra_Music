use std::time::Duration;

/// Una línea de letra con su timestamp de inicio.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub timestamp: Duration,
    pub text: String,
}

/// Letras ya parseadas y listas para sincronizar con la posición de
/// reproducción. Las líneas están ordenadas por timestamp ascendente.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
}

impl SyncedLyrics {
    /// Devuelve el índice de la línea que debería estar resaltada
    /// para una posición de reproducción dada, o `None` si la
    /// reproducción todavía no llegó a la primera línea.
    pub fn current_line_index(&self, position: Duration) -> Option<usize> {
        // Buscamos la última línea cuyo timestamp sea <= position.
        // partition_point asume orden ascendente (lo garantizamos al parsear).
        let idx = self.lines.partition_point(|line| line.timestamp <= position);

        if idx == 0 {
            None
        } else {
            Some(idx - 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Parsea el contenido crudo de un archivo .lrc.
///
/// Formato esperado por línea: `[mm:ss.xx]texto` (también soporta
/// `[mm:ss.xxx]` con milisegundos de 3 dígitos, y múltiples tags de
/// tiempo por línea, ej. `[00:12.00][00:45.00]texto` que LRCLIB a
/// veces genera para coros repetidos).
/// Líneas sin tag de tiempo válido (metadata como [ar:], [ti:], o
/// líneas vacías entre versos) se ignoran silenciosamente.
pub fn parse_lrc(content: &str) -> SyncedLyrics {
    let mut lines = Vec::new();

    for raw_line in content.lines() {
        let mut rest = raw_line;
        let mut timestamps = Vec::new();

        // Una línea puede tener uno o más tags [mm:ss.xx] al principio.
        while let Some(tag_end) = rest.find(']') {
            if !rest.starts_with('[') {
                break;
            }

            let tag = &rest[1..tag_end];

            match parse_timestamp(tag) {
                Some(duration) => {
                    timestamps.push(duration);
                    rest = &rest[tag_end + 1..];
                }
                None => break, // no era un timestamp (ej. [ar:Artista]), dejamos de intentar
            }
        }

        if timestamps.is_empty() {
            continue;
        }

        let text = rest.trim().to_string();

        for timestamp in timestamps {
            lines.push(LyricLine {
                timestamp,
                text: text.clone(),
            });
        }
    }

    lines.sort_by_key(|l| l.timestamp);

    SyncedLyrics { lines }
}

/// Parsea un tag de tiempo tipo "01:23.45" o "01:23.450" a Duration.
/// Devuelve None si el formato no matchea (para poder distinguir de
/// tags de metadata como "ar:Nombre").
fn parse_timestamp(tag: &str) -> Option<Duration> {
    let (minutes_str, rest) = tag.split_once(':')?;
    let (seconds_str, millis_str) = rest.split_once('.')?;

    let minutes: u64 = minutes_str.trim().parse().ok()?;
    let seconds: u64 = seconds_str.parse().ok()?;

    // Los milisegundos pueden venir con 2 o 3 dígitos; normalizamos a 3.
    let millis: u64 = match millis_str.len() {
        2 => millis_str.parse::<u64>().ok()? * 10,
        3 => millis_str.parse().ok()?,
        _ => return None,
    };

    Some(Duration::from_millis(minutes * 60_000 + seconds * 1000 + millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_linea_simple() {
        let lrc = "[00:12.34]Hola mundo";
        let parsed = parse_lrc(lrc);
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].timestamp, Duration::from_millis(12_340));
        assert_eq!(parsed.lines[0].text, "Hola mundo");
    }

    #[test]
    fn ignora_metadata() {
        let lrc = "[ar:Artista]\n[ti:Titulo]\n[00:05.00]Primera linea";
        let parsed = parse_lrc(lrc);
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].text, "Primera linea");
    }

    #[test]
    fn soporta_milisegundos_de_3_digitos() {
        let lrc = "[00:12.345]Texto";
        let parsed = parse_lrc(lrc);
        assert_eq!(parsed.lines[0].timestamp, Duration::from_millis(12_345));
    }

    #[test]
    fn soporta_multiples_timestamps_por_linea() {
        let lrc = "[00:10.00][00:50.00]Coro repetido";
        let parsed = parse_lrc(lrc);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].timestamp, Duration::from_millis(10_000));
        assert_eq!(parsed.lines[1].timestamp, Duration::from_millis(50_000));
    }

    #[test]
    fn current_line_index_encuentra_la_correcta() {
        let lrc = "[00:10.00]Primera\n[00:20.00]Segunda\n[00:30.00]Tercera";
        let parsed = parse_lrc(lrc);

        assert_eq!(parsed.current_line_index(Duration::from_secs(5)), None);
        assert_eq!(parsed.current_line_index(Duration::from_secs(15)), Some(0));
        assert_eq!(parsed.current_line_index(Duration::from_secs(25)), Some(1));
        assert_eq!(parsed.current_line_index(Duration::from_secs(99)), Some(2));
    }
}