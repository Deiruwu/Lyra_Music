 //! Utilidad de búsqueda reutilizable: normalización de texto y coincidencia
//! difusa por subcadena/tokens, con tolerancia a errores de tecleo
//! (distancia de edición) en palabras completas. Pensado para filtrar
//! listas (tracks, álbumes, playlists, etc.) sin depender de
//! mayúsculas/minúsculas, acentos, ni de una ortografía perfecta.
//!
//! Uso típico:
//!
//! ```ignore
//! use crate::ui::utils::search::SearchQuery;
//!
//! let query = SearchQuery::new(&self.search_query);
//! let matches = query.matches_any(&[&track.title, &track.format_artists()]);
//! ```

/// Consulta de búsqueda ya normalizada y pre-tokenizada.
/// Se construye una vez por cada cambio de `search_query` y se reutiliza
/// para evaluar cada elemento de la lista (evita re-normalizar el needle
/// en cada comparación).
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Consulta completa normalizada (para contains simple).
    normalized: String,
    /// Tokens (palabras) normalizados, para coincidencia por palabras en
    /// cualquier orden ("radiohead ok" -> matchea "OK Computer - Radiohead").
    tokens: Vec<String>,
}

impl SearchQuery {
    /// Crea una consulta a partir del texto crudo introducido por el usuario.
    pub fn new(raw: &str) -> Self {
        let normalized = normalize(raw);
        let tokens = normalized
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        Self { normalized, tokens }
    }

    /// True si la consulta está vacía (sin texto útil) — en ese caso el
    /// llamador normalmente debe mostrar todos los elementos sin filtrar.
    pub fn is_empty(&self) -> bool {
        self.normalized.is_empty()
    }

    /// Evalúa un único campo de texto contra la consulta.
    pub fn matches(&self, haystack: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let hay = normalize(haystack);
        self.matches_normalized(&hay)
    }

    /// Evalúa varios campos (título, artista, álbum...) y retorna true si
    /// la consulta matchea contra la concatenación de todos ellos. Esto
    /// permite que una búsqueda como "radiohead ok computer" coincida aunque
    /// "radiohead" esté en el campo artista y "ok computer" en el título.
    pub fn matches_any(&self, haystacks: &[&str]) -> bool {
        if self.is_empty() {
            return true;
        }

        // Coincidencia rápida: si algún campo individual ya contiene la
        // consulta completa, no hace falta combinar campos.
        if haystacks.iter().any(|h| self.matches(h)) {
            return true;
        }

        // Coincidencia por tokens combinando todos los campos: cada
        // palabra de la búsqueda debe aparecer en alguno de los campos.
        let combined = haystacks
            .iter()
            .map(|h| normalize(h))
            .collect::<Vec<_>>()
            .join(" ");

        self.matches_normalized(&combined)
    }

    fn matches_normalized(&self, hay_normalized: &str) -> bool {
        // 1) Subcadena directa de la consulta completa (rápido, cubre la
        //    mayoría de los casos: "radioh" -> "radiohead").
        if hay_normalized.contains(&self.normalized) {
            return true;
        }

        // 2) Todas las palabras de la consulta aparecen, en cualquier
        //    orden: por subcadena exacta, o si no, tolerando un typo
        //    (distancia de edición) contra alguna palabra del haystack.
        let hay_words: Vec<&str> = hay_normalized.split_whitespace().collect();

        self.tokens.iter().all(|tok| {
            hay_normalized.contains(tok.as_str())
                || hay_words.iter().any(|w| fuzzy_word_match(tok, w))
        })
    }
}

/// Compara un token de búsqueda contra una palabra del haystack tolerando
/// errores de tecleo, usando distancia de Levenshtein.
///
/// Si el token es más corto que la palabra (caso típico: el usuario está
/// escribiendo y aún no terminó, ej. "Marcupi" buscando "Marsupials"),
/// compara contra el **prefijo** de la palabra del mismo largo que el
/// token, no contra la palabra completa — si no, la diferencia de
/// longitud infla la distancia y nunca matchea.
fn fuzzy_word_match(token: &str, word: &str) -> bool {
    // Palabras muy cortas (<=2 chars) exigen coincidencia exacta: permitir
    // errores ahí genera demasiados falsos positivos.
    if token.len() <= 2 || word.len() <= 2 {
        return word.starts_with(token) || token == word;
    }

    let token_chars: Vec<char> = token.chars().collect();
    let word_chars: Vec<char> = word.chars().collect();
    let threshold = fuzzy_threshold(token_chars.len());

    if threshold == 0 {
        return word_chars.starts_with(&token_chars) || token == word;
    }

    if token_chars.len() >= word_chars.len() {
        // Token igual o más largo que la palabra completa: comparación normal.
        levenshtein_within(&token_chars, &word_chars, threshold)
    } else {
        // Token más corto (búsqueda parcial mientras se escribe): compara
        // contra el mejor prefijo de "word" de tamaño similar al token.
        prefix_levenshtein_within(&token_chars, &word_chars, threshold)
    }
}

/// Umbral de tolerancia a errores según el largo del token de búsqueda.
fn fuzzy_threshold(token_len: usize) -> usize {
    match token_len {
        0..=4 => 0,
        5..=7 => 1,
        8..=11 => 2,
        _ => 3,
    }
}

/// Distancia de Levenshtein completa (ambas cadenas de principio a fin)
/// con early-exit.
fn levenshtein_within(a: &[char], b: &[char], max_dist: usize) -> bool {
    if a.len().abs_diff(b.len()) > max_dist {
        return false;
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];

    for i in 1..=a.len() {
        curr[0] = i;
        let mut row_min = curr[0];

        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
            row_min = row_min.min(curr[j]);
        }

        if row_min > max_dist {
            return false;
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()] <= max_dist
}

/// Distancia de edición de "prefijo": true si `needle` (más corto) matchea
/// con tolerancia `max_dist` contra **algún prefijo** de `haystack`, sin
/// penalizar el resto de `haystack` que sobra después. Es la variante
/// estándar de Levenshtein donde la última fila puede terminar en
/// cualquier columna, no solo en la última.
fn prefix_levenshtein_within(needle: &[char], haystack: &[char], max_dist: usize) -> bool {
    let mut prev: Vec<usize> = (0..=haystack.len()).collect();
    let mut curr = vec![0usize; haystack.len() + 1];

    for i in 1..=needle.len() {
        curr[0] = i;
        for j in 1..=haystack.len() {
            let cost = if needle[i - 1] == haystack[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    // Cualquier columna de la última fila representa terminar el needle
    // habiendo consumido un prefijo distinto de haystack: basta con que
    // la mejor de todas esté dentro del umbral.
    prev.iter().min().copied().unwrap_or(usize::MAX) <= max_dist
}

/// Normaliza un texto para comparación: minúsculas, sin acentos/diacríticos,
/// espacios colapsados y recortados. No usa dependencias externas.
pub fn normalize(input: &str) -> String {
    let lowered = input.to_lowercase();
    let stripped = strip_diacritics(&lowered);

    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Elimina diacríticos comunes (tildes, diéresis, ñ->n, etc.) sin depender
/// de crates externos como `unicode-normalization`. Cubre el rango latino
/// suficiente para español/inglés/francés/alemán/portugués.
fn strip_diacritics(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}