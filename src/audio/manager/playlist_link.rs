use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use rand::rng;
use rand::RngExt;
use uuid::Uuid;

use crate::audio::manager::manager::{PlaybackState, QueueSlot, RepeatMode, TrackManager};
use crate::model::Track;

/// Cola ligada a una playlist: lo que falta por sonar se deriva de su orden vigente.
pub(super) struct PlaylistLink {
    pub(super) playlist_id: String,
    /// La playlist tal como está ahora.
    tracks: Vec<Arc<Track>>,
    /// Orden de ids que se aplicó a la cola la última vez.
    applied: Vec<String>,
    /// Último track de la playlist que sonó: la posición dentro de ella.
    anchor: Option<String>,
    /// Quitados a mano de la cola; no vuelven en esta pasada.
    skipped: HashSet<String>,
}

impl PlaylistLink {
    pub(super) fn new(playlist_id: String, tracks: Vec<Arc<Track>>, anchor: Option<String>) -> Self {
        let applied = ids(&tracks);
        Self { playlist_id, tracks, applied, anchor, skipped: HashSet::new() }
    }

    pub(super) fn contains(&self, track_id: &str) -> bool {
        self.tracks.iter().any(|t| t.id == track_id)
    }

    /// Quita `track_ids` de la playlist ligada.
    pub(super) fn forget(&mut self, track_ids: &HashSet<&str>) {
        self.tracks.retain(|t| !track_ids.contains(t.id.as_str()));
    }

    /// Reemplaza los datos de `track` en la playlist ligada.
    pub(super) fn refresh(&mut self, track: &Arc<Track>) {
        for slot in self.tracks.iter_mut().filter(|t| t.id == track.id) {
            *slot = Arc::clone(track);
        }
    }
}

fn ids(tracks: &[Arc<Track>]) -> Vec<String> {
    tracks.iter().map(|t| t.id.clone()).collect()
}

impl PlaybackState {
    /// Hay canciones encoladas a mano: la playlist no toca la cola hasta que se vayan.
    pub(super) fn link_suspended(&self) -> bool {
        self.queue.iter().any(|slot| slot.manual)
    }

    /// Empezó a sonar `track_id`: si viene de la playlist, marca ahí la posición.
    pub(super) fn track_started(&mut self, track_id: &str, manual: bool) {
        if let Some(link) = self.link.as_mut()
            && !manual
            && link.contains(track_id)
        {
            link.anchor = Some(track_id.to_string());
        }
        self.reconcile_link(false);
    }

    /// Recibe la playlist vigente: refresca los tracks en cola y, si cambió su orden, la cola.
    pub(super) fn set_link_tracks(&mut self, tracks: Vec<Arc<Track>>) {
        let Some(link) = self.link.as_mut() else { return };
        let by_id: HashMap<&str, &Arc<Track>> = tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        for slot in self.queue.iter_mut() {
            if let Some(track) = by_id.get(slot.track.id.as_str()) {
                slot.track = Arc::clone(track);
            }
        }
        drop(by_id);
        link.tracks = tracks;
        self.reconcile_link(false);
    }

    /// Quitado a mano de la cola: si venía de la playlist, no vuelve en esta pasada.
    pub(super) fn skip_in_link(&mut self, slot: &QueueSlot) {
        if let Some(link) = self.link.as_mut()
            && !slot.manual
        {
            link.skipped.insert(slot.track.id.clone());
        }
        self.reconcile_link(false);
    }

    /// Rearma lo que falta por sonar desde la playlist si cambió desde la última vez (o `force`).
    pub(super) fn reconcile_link(&mut self, force: bool) {
        if self.link_suspended() {
            return;
        }
        let current_id = self.current_track.as_ref().map(|c| c.track.id.clone());
        let shuffle = self.shuffle_enabled;
        let Some(link) = self.link.as_mut() else { return };

        let new_ids = ids(&link.tracks);
        if !force && new_ids == link.applied {
            return;
        }

        let pending = if shuffle {
            shuffled_pending(link, &new_ids, &self.queue, current_id.as_deref())
        } else {
            ordered_pending(link, &new_ids)
        };
        link.applied = new_ids;

        let by_id: HashMap<&str, &Arc<Track>> = link.tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        let mut reuse: HashMap<&str, Uuid> = HashMap::new();
        for slot in &self.queue {
            reuse.entry(slot.track.id.as_str()).or_insert(slot.id);
        }

        let slots: Vec<QueueSlot> = pending
            .iter()
            .filter_map(|id| {
                let track = by_id.get(id.as_str())?;
                Some(QueueSlot {
                    id: reuse.remove(id.as_str()).unwrap_or_else(Uuid::new_v4),
                    track: Arc::clone(track),
                    manual: false,
                })
            })
            .collect();

        self.original_order = slots.iter().map(|s| s.id).collect();
        self.queue = slots.into();
    }

    /// "Repetir cola" sin pendientes: vuelve a empezar la playlist ligada o, sin ella, el historial.
    pub(super) fn refill_for_repeat(&mut self) {
        if !self.queue.is_empty() || self.repeat_mode != RepeatMode::Queue {
            return;
        }

        if let Some(link) = self.link.as_mut() {
            link.anchor = None;
            link.skipped.clear();
            link.applied = ids(&link.tracks);
            let slots = link.tracks.iter().cloned().map(QueueSlot::new).collect();
            self.refill_queue(slots);
            return;
        }

        if !self.history.is_empty() {
            let replay: Vec<QueueSlot> = self.history.drain(..).map(|t| QueueSlot::new(Arc::new(t))).collect();
            self.refill_queue(replay);
        }
    }
}

/// Sin shuffle: lo que sigue al anchor en el orden vigente, menos lo quitado a mano.
fn ordered_pending(link: &mut PlaylistLink, new_ids: &[String]) -> Vec<String> {
    let start = match &link.anchor {
        None => 0,
        Some(anchor) => match new_ids.iter().position(|id| id == anchor) {
            Some(pos) => pos + 1,
            None => {
                let start = successor_start(&link.applied, anchor, new_ids);
                link.anchor = start.checked_sub(1).map(|i| new_ids[i].clone());
                start
            }
        },
    };

    new_ids[start..].iter().filter(|id| !link.skipped.contains(*id)).cloned().collect()
}

/// El anchor ya no está: se sigue desde el primero que le seguía y sobrevivió.
fn successor_start(applied: &[String], anchor: &str, new_ids: &[String]) -> usize {
    let Some(old_pos) = applied.iter().position(|id| id == anchor) else {
        return new_ids.len();
    };
    applied[old_pos + 1..]
        .iter()
        .find_map(|id| new_ids.iter().position(|n| n == id))
        .unwrap_or(new_ids.len())
}

/// Con shuffle: se conserva el orden aleatorio, se va lo que salió de la playlist y lo nuevo cae en cualquier lugar.
fn shuffled_pending(
    link: &PlaylistLink,
    new_ids: &[String],
    queue: &VecDeque<QueueSlot>,
    current_id: Option<&str>,
) -> Vec<String> {
    let present: HashSet<&str> = new_ids.iter().map(String::as_str).collect();
    let mut pending: Vec<String> = queue
        .iter()
        .map(|slot| slot.track.id.clone())
        .filter(|id| present.contains(id.as_str()) && !link.skipped.contains(id))
        .collect();

    // Cola restaurada recién ligada: se adopta tal cual.
    if link.applied.is_empty() {
        return pending;
    }

    let known: HashSet<&str> = link.applied.iter().map(String::as_str).collect();
    let mut rng = rng();
    for id in new_ids {
        let is_new = !known.contains(id.as_str())
            && !link.skipped.contains(id)
            && Some(id.as_str()) != current_id
            && !pending.contains(id);
        if is_new {
            let index = rng.random_range(0..=pending.len());
            pending.insert(index, id.clone());
        }
    }
    pending
}

impl TrackManager {
    /// Reproduce la playlist desde `start_index` con la cola ligada a ella.
    pub fn play_playlist(&self, playlist_id: &str, tracks: Vec<Track>, start_index: usize) {
        self.play_context_inner(tracks, start_index, Some(playlist_id.to_string()));
    }

    /// "Reproducir todo" de una playlist: en shuffle arranca en una canción al azar.
    pub fn play_playlist_shuffled(&self, playlist_id: &str, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        let start_index = if self.is_shuffled() { rng().random_range(0..tracks.len()) } else { 0 };
        self.play_playlist(playlist_id, tracks, start_index);
    }

    /// Liga la cola restaurada a su playlist; se completa en el primer `sync_link`.
    pub fn relink_playlist(&self, playlist_id: &str) {
        let mut ps = self.playback.lock().unwrap();
        let anchor = ps.current_track.as_ref().map(|c| c.track.id.clone());
        ps.link = Some(PlaylistLink::new(playlist_id.to_string(), Vec::new(), anchor));
    }

    /// Playlist a la que está ligada la cola.
    pub fn linked_playlist(&self) -> Option<String> {
        self.playback.lock().unwrap().link.as_ref().map(|link| link.playlist_id.clone())
    }

    /// La cola sigue a la playlist y todavía le quedan canciones.
    pub fn has_linked_pending(&self) -> bool {
        let ps = self.playback.lock().unwrap();
        ps.link.is_some() && !ps.link_suspended() && !ps.queue.is_empty()
    }

    /// Nueva versión de la playlist ligada; la cola la sigue.
    pub fn sync_link(&self, playlist_id: &str, tracks: Vec<Track>) {
        {
            let mut ps = self.playback.lock().unwrap();
            if ps.link.as_ref().is_none_or(|link| link.playlist_id != playlist_id) {
                return;
            }
            ps.set_link_tracks(tracks.into_iter().map(Arc::new).collect());
        }
        self.broadcast_queue_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::audio_tech::{AudioProperties, PlayableTrack};

    fn track(id: &str) -> Track {
        Track {
            id: id.to_string(),
            title: id.to_string(),
            duration_seconds: 0,
            thumbnail_small: None,
            thumbnail_large: None,
            bpm: None,
            camelot_key: None,
            file_path: Some(format!("{id}.mp3")),
            added_at: None,
            state: Default::default(),
            liked: false,
            play_count: None,
            last_played_at: None,
            album: None,
            artists: vec![],
        }
    }

    fn arcs(ids: &[&str]) -> Vec<Arc<Track>> {
        ids.iter().map(|id| Arc::new(track(id))).collect()
    }

    fn playable(id: &str) -> Arc<PlayableTrack> {
        Arc::new(PlayableTrack {
            track: track(id),
            audio: AudioProperties { sample_rate: 44100, channels: 2, bit_depth: None, codec: "test".to_string(), duration_secs: None },
        })
    }

    /// Playlist `ids` sonando en `current`, con el resto en cola.
    fn linked(ids: &[&str], current: &str, shuffle: bool) -> PlaybackState {
        let mut ps = PlaybackState::new();
        ps.shuffle_enabled = shuffle;
        let tracks = arcs(ids);
        let start = ids.iter().position(|id| *id == current).unwrap();
        ps.queue = tracks[start + 1..].iter().cloned().map(QueueSlot::new).collect();
        ps.current_track = Some(playable(current));
        ps.link = Some(PlaylistLink::new("p".into(), tracks, Some(current.to_string())));
        ps
    }

    fn queue_ids(ps: &PlaybackState) -> Vec<String> {
        ps.queue.iter().map(|s| s.track.id.clone()).collect()
    }

    #[test]
    fn reordenar_la_playlist_reordena_lo_pendiente() {
        let mut ps = linked(&["a", "b", "c", "d"], "a", false);
        ps.set_link_tracks(arcs(&["a", "d", "c", "b"]));
        assert_eq!(queue_ids(&ps), vec!["d", "c", "b"]);
    }

    #[test]
    fn agregar_y_quitar_en_la_playlist_se_refleja() {
        let mut ps = linked(&["a", "b", "c"], "a", false);
        ps.set_link_tracks(arcs(&["a", "c", "e"]));
        assert_eq!(queue_ids(&ps), vec!["c", "e"]);
    }

    #[test]
    fn mismo_orden_respeta_los_movimientos_locales() {
        let mut ps = linked(&["a", "b", "c"], "a", false);
        ps.queue.swap(0, 1);
        ps.set_link_tracks(arcs(&["a", "b", "c"]));
        assert_eq!(queue_ids(&ps), vec!["c", "b"]);
    }

    #[test]
    fn lo_manual_suspende_y_al_irse_se_reconcilia() {
        let mut ps = linked(&["a", "b", "c"], "a", false);
        ps.queue.push_front(QueueSlot::manual(Arc::new(track("x"))));
        ps.set_link_tracks(arcs(&["a", "c", "b"]));
        assert_eq!(queue_ids(&ps), vec!["x", "b", "c"]);

        let slot = ps.queue.remove(0).unwrap();
        ps.skip_in_link(&slot);
        assert_eq!(queue_ids(&ps), vec!["c", "b"]);
    }

    #[test]
    fn lo_quitado_a_mano_no_resucita() {
        let mut ps = linked(&["a", "b", "c"], "a", false);
        let slot = ps.queue.remove(0).unwrap();
        ps.skip_in_link(&slot);
        ps.set_link_tracks(arcs(&["a", "c", "b", "d"]));
        assert_eq!(queue_ids(&ps), vec!["c", "d"]);
    }

    #[test]
    fn si_se_borra_el_anchor_sigue_el_siguiente_que_sobrevive() {
        let mut ps = linked(&["a", "b", "c", "d"], "b", false);
        ps.set_link_tracks(arcs(&["a", "d", "c"]));
        assert_eq!(queue_ids(&ps), vec!["c"]);
        ps.set_link_tracks(arcs(&["a", "d", "c", "e"]));
        assert_eq!(queue_ids(&ps), vec!["c", "e"]);
    }

    #[test]
    fn en_shuffle_se_conserva_el_orden_y_entran_los_nuevos() {
        let mut ps = linked(&["a", "b", "c", "d"], "a", true);
        ps.queue = arcs(&["d", "b", "c"]).into_iter().map(QueueSlot::new).collect();
        ps.set_link_tracks(arcs(&["a", "c", "b", "d", "e"]));
        let queue = queue_ids(&ps);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.iter().filter(|id| *id != "e").collect::<Vec<_>>(), vec!["d", "b", "c"]);
    }

    #[test]
    fn repetir_cola_vuelve_a_empezar_la_playlist() {
        let mut ps = linked(&["a", "b"], "b", false);
        ps.repeat_mode = RepeatMode::Queue;
        ps.refill_for_repeat();
        assert_eq!(queue_ids(&ps), vec!["a", "b"]);
    }
}
