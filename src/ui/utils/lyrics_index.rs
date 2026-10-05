//! Qué canciones descargadas tienen letra: un `.lrc` junto al audio (el mismo que
//! lee el modo teatro). El disco se revisa en segundo plano, una vez por canción,
//! y se recuerda; los avisos de "letra encontrada" del servidor lo actualizan.

use std::collections::HashSet;

use iced::Task;

use crate::model::audio_tech::local_audio_path;
use crate::model::Track;

#[derive(Default)]
pub struct LyricsIndex {
    /// Ya revisadas (con o sin letra) o en revisión.
    checked: HashSet<String>,
    with_lyrics: HashSet<String>,
}

impl LyricsIndex {
    /// Ids de las canciones que tienen letra.
    pub fn with_lyrics(&self) -> &HashSet<String> {
        &self.with_lyrics
    }

    /// Revisa en segundo plano las canciones descargadas de `tracks` que todavía no se miraron.
    pub fn sync<Msg: Send + 'static>(
        &mut self,
        tracks: &[&Track],
        to_message: impl Fn(Vec<(String, bool)>) -> Msg + Send + 'static,
    ) -> Task<Msg> {
        let pending: Vec<(String, String)> = tracks
            .iter()
            .filter(|t| !self.checked.contains(&t.id))
            .filter_map(|t| Some((t.id.clone(), t.file_path.clone().filter(|p| !p.is_empty())?)))
            .collect();
        if pending.is_empty() {
            return Task::none();
        }
        self.checked.extend(pending.iter().map(|(id, _)| id.clone()));

        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    pending
                        .into_iter()
                        .map(|(id, path)| (id, local_audio_path(&path).with_extension("lrc").exists()))
                        .collect()
                })
                    .await
                    .unwrap_or_default()
            },
            to_message,
        )
    }

    pub fn on_checked(&mut self, results: Vec<(String, bool)>) {
        for (id, has_lyrics) in results {
            if has_lyrics {
                self.with_lyrics.insert(id);
            } else {
                self.with_lyrics.remove(&id);
            }
        }
    }

    /// El servidor avisó que encontró la letra de `id`.
    pub fn mark_found(&mut self, id: &str) {
        self.checked.insert(id.to_string());
        self.with_lyrics.insert(id.to_string());
    }

    /// Volver a revisar `id` la próxima vez (p. ej. se descargó de nuevo).
    pub fn forget(&mut self, id: &str) {
        self.checked.remove(id);
    }
}
