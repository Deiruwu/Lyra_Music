use rand::seq::SliceRandom;
use rand::rng;
use crate::model::Track;

// ── Shuffle ───────────────────────────────────────────────────────────────────

/// Shuffle random puro y recorte a `take` elementos.
pub fn shuffle_pool(mut tracks: Vec<Track>, take: usize) -> Vec<Track> {
    tracks.shuffle(&mut rng());
    tracks.truncate(take);
    tracks
}

// ── Camelot ───────────────────────────────────────────────────────────────────

/// Parsea un string tipo "8A" o "12B" en (número, letra).
fn parse_camelot(key: &str) -> Option<(u8, char)> {
    let key = key.trim().to_uppercase();
    let letter = key.chars().last()?;
    if letter != 'A' && letter != 'B' {
        return None;
    }
    let number: u8 = key[..key.len() - 1].parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    Some((number, letter))
}

/// Compatible si: mismo número (cualquier letra) o número vecino (±1, circular) con misma letra.
pub fn is_camelot_compatible(a: &str, b: &str) -> bool {
    let (Some((num_a, let_a)), Some((num_b, let_b))) = (parse_camelot(a), parse_camelot(b)) else {
        return false;
    };

    if num_a == num_b {
        return true;
    }

    if let_a != let_b {
        return false;
    }

    let diff = (num_a as i16 - num_b as i16).abs();
    diff == 1 || diff == 11
}

// ── Orden armónico ──────────────────────────────────────────────────────────

/// Ordena armónicamente las pistas que tienen bpm+camelot_key, preservando
/// los huecos (posiciones) de las que NO tienen esa metadata.
pub fn sort_harmonic(tracks: Vec<Track>) -> Vec<Track> {
    let mut fixed_slots: Vec<(usize, Track)> = Vec::new();
    let mut sortable: Vec<Track> = Vec::new();

    for (i, track) in tracks.into_iter().enumerate() {
        if track.bpm.is_some() && track.camelot_key.is_some() {
            sortable.push(track);
        } else {
            fixed_slots.push((i, track));
        }
    }

    let total_len = sortable.len() + fixed_slots.len();
    let ordered_sortable = greedy_harmonic_chain(sortable);

    let mut result: Vec<Option<Track>> = (0..total_len).map(|_| None).collect();
    for (i, track) in fixed_slots {
        result[i] = Some(track);
    }

    let mut chain_iter = ordered_sortable.into_iter();
    for slot in result.iter_mut() {
        if slot.is_none() {
            *slot = chain_iter.next();
        }
    }

    result.into_iter().flatten().collect()
}

/// Arma una cadena greedy: en cada paso elige la pista restante más
/// compatible en camelot, y entre las compatibles, la de BPM más cercano.
fn greedy_harmonic_chain(mut pool: Vec<Track>) -> Vec<Track> {
    if pool.is_empty() {
        return pool;
    }

    let mut chain = vec![pool.remove(0)];

    while !pool.is_empty() {
        let last = chain.last().unwrap();
        let last_camelot = last.camelot_key.as_deref().unwrap_or_default();
        let last_bpm = last.bpm.unwrap_or(0);

        let best_idx = pool
            .iter()
            .enumerate()
            .min_by_key(|(_, candidate)| {
                let candidate_camelot = candidate.camelot_key.as_deref().unwrap_or_default();
                let candidate_bpm = candidate.bpm.unwrap_or(0);
                let bpm_diff = (candidate_bpm - last_bpm).abs();
                let compatible = is_camelot_compatible(last_camelot, candidate_camelot);

                let compat_rank = if compatible { 0 } else { 1 };
                (compat_rank, bpm_diff)
            })
            .map(|(i, _)| i);

        if let Some(idx) = best_idx {
            chain.push(pool.remove(idx));
        }
    }

    chain
}
