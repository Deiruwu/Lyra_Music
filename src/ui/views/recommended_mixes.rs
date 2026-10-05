use std::collections::HashSet;
use std::sync::Arc;

use futures::future::join_all;
use rand::rng;
use rand::seq::SliceRandom;

use crate::microservices::client::MicroserviceClient;
use crate::model::{Mix, Track};

/// Canciones más escuchadas que se usan como semilla (una mezcla cada una).
const TOP_SEEDS: usize = 3;
/// Me gusta al azar que siembran la mezcla "Desde tus Me gusta".
const LIKED_SEEDS: usize = 3;
/// Canciones que se le piden a cada radio.
const RADIO_POOL: usize = 25;
const MIX_LENGTH: usize = 30;
/// Menos que esto no alcanza para una mezcla de descubrimiento.
const MIN_DISCOVERY_TRACKS: usize = 5;

/// Semillas para armar las mezclas: lo más escuchado, los Me gusta y lo que ya
/// está en la biblioteca (descargado o con Me gusta), que el descubrimiento excluye.
pub struct MixSeeds {
    pub top_tracks: Vec<Track>,
    pub liked: Vec<Track>,
    pub known_ids: HashSet<String>,
}

impl MixSeeds {
    pub fn is_empty(&self) -> bool {
        self.top_tracks.is_empty() && self.liked.is_empty()
    }
}

/// Pide las radios de las semillas y arma: una mezcla por canción top, una de
/// Me gusta y una de descubrimiento (solo canciones que no están en la biblioteca).
pub async fn build_mixes(client: Arc<MicroserviceClient>, seeds: MixSeeds) -> Vec<Mix> {
    let top_seeds: Vec<Track> = seeds.top_tracks.into_iter().take(TOP_SEEDS).collect();
    let top_ids: HashSet<&str> = top_seeds.iter().map(|t| t.id.as_str()).collect();

    let mut liked = seeds.liked;
    liked.retain(|t| !top_ids.contains(t.id.as_str()));
    liked.shuffle(&mut rng());
    liked.truncate(LIKED_SEEDS);

    let top_pools = fetch_radios(&client, &top_seeds).await;
    let liked_pools = fetch_radios(&client, &liked).await;

    let mut mixes = Vec::new();

    for (seed, pool) in top_seeds.iter().zip(&top_pools) {
        let tracks = interleave_unique(std::slice::from_ref(pool), |_| true);
        if let Some(mix) = make_mix(format!("mix:top:{}", seed.id), format!("Basado en {}", seed.title), artist_names(seed), tracks) {
            mixes.push(mix);
        }
    }

    let liked_tracks = interleave_unique(&liked_pools, |_| true);
    if let Some(mix) = make_mix("mix:liked".into(), "Desde tus Me gusta".into(), "Inspirada en lo que te gusta".into(), liked_tracks) {
        mixes.push(mix);
    }

    let seed_ids: HashSet<&str> = top_seeds.iter().chain(&liked).map(|t| t.id.as_str()).collect();
    let all_pools: Vec<Vec<Track>> = top_pools.into_iter().chain(liked_pools).collect();
    let discovery = interleave_unique(&all_pools, |t| {
        !seeds.known_ids.contains(&t.id) && !seed_ids.contains(t.id.as_str())
    });
    if discovery.len() >= MIN_DISCOVERY_TRACKS
        && let Some(mix) = make_mix("mix:discovery".into(), "Descubrimiento".into(), "Canciones que todavía no tienes".into(), discovery) {
            mixes.push(mix);
        }

    mixes
}

/// Radio de cada semilla, en paralelo; una semilla que falla aporta una lista vacía.
async fn fetch_radios(client: &Arc<MicroserviceClient>, seeds: &[Track]) -> Vec<Vec<Track>> {
    join_all(seeds.iter().map(|seed| {
        let client = Arc::clone(client);
        let id = seed.id.clone();
        async move { client.radio(&id, Some(RADIO_POOL)).await.unwrap_or_default() }
    }))
    .await
}

/// Intercala las listas (una de cada una por vuelta) sin repetir ids, hasta `MIX_LENGTH`.
fn interleave_unique(pools: &[Vec<Track>], keep: impl Fn(&Track) -> bool) -> Vec<Track> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let longest = pools.iter().map(Vec::len).max().unwrap_or(0);

    for index in 0..longest {
        for pool in pools {
            let Some(track) = pool.get(index) else { continue };
            if result.len() >= MIX_LENGTH {
                return result;
            }
            if keep(track) && seen.insert(track.id.clone()) {
                result.push(track.clone());
            }
        }
    }

    result
}

fn make_mix(id: String, title: String, subtitle: String, tracks: Vec<Track>) -> Option<Mix> {
    let first = tracks.first()?;
    let cover_url = first.thumbnail_large.clone().or_else(|| first.thumbnail_small.clone());
    Some(Mix { id, title, subtitle, cover_url, tracks })
}

fn artist_names(track: &Track) -> String {
    track.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
}
