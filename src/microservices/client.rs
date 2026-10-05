use crate::model::{AlbumDto, ArtistDto, ArtistProfileDto, DownloadEvent, SearchItem, Track};
use musichub_client::{MicroserviceClient as HubClient, Request};
pub use musichub_client::MicroserviceError;


#[derive(Clone)]
pub struct MicroserviceClient {
    inner: HubClient,
}

impl MicroserviceClient {
    pub fn new(host: &str, port: u16) -> Self {
        Self { inner: HubClient::new(host, port) }
    }

    /// `filter`: songs | videos | albums | artists | all.
    pub async fn search_items(&self, query: &str, limit: Option<usize>, filter: &str) -> Result<Vec<SearchItem>, MicroserviceError> {
        self.inner.search_items(query, limit, filter).await
    }

    pub async fn download(&self, query: &str) -> Result<Track, MicroserviceError> {
        self.inner.download(query).await
    }

    pub async fn redownload(&self, track_id: &str) -> Result<Track, MicroserviceError> {
        self.inner.redownload(track_id).await
    }

    pub async fn refresh_metadata(&self, track_id: &str) -> Result<Track, MicroserviceError> {
        self.inner.refresh_metadata(track_id).await
    }

    pub async fn refresh_lyrics(&self, track_id: &str) -> Result<Track, MicroserviceError> {
        self.inner.refresh_lyrics(track_id).await
    }

    pub async fn reanalyze(&self, track_id: &str) -> Result<Track, MicroserviceError> {
        self.inner.reanalyze(track_id).await
    }

    pub async fn subscribe_downloads(&self) -> Result<tokio::sync::mpsc::UnboundedReceiver<DownloadEvent>, MicroserviceError> {
        self.inner.subscribe_downloads().await
    }

    pub async fn delete(&self, query: &str) -> Result<(), MicroserviceError>{
        self.inner.delete_track(query).await
    }

    pub async fn radio(&self, query: &str, limit: Option<usize>) -> Result<Vec<Track>, MicroserviceError> {
        self.inner.radio(query, limit).await
    }

    pub async fn mark_as_played(&self, track_id: &str) -> Result<(), MicroserviceError> {
        self.inner.mark_as_played(track_id).await
    }

    pub async fn get_all_ids(&self) -> Result<Vec<String>, MicroserviceError> {
        self.inner.send(Request::new("get_all_ids")).await
    }

    pub async fn resolve_many(&self, ids: &[String]) -> Result<Vec<Track>, MicroserviceError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.inner.resolve_many(ids).await
    }

    pub async fn album(&self, album_id: &str) -> Result<AlbumDto, MicroserviceError> {
        self.inner.album(album_id).await
    }

    pub async fn artist(&self, channel_id: &str, limit: Option<usize>) -> Result<ArtistDto, MicroserviceError> {
        self.inner.artist(channel_id, limit).await
    }

    pub async fn artist_profile(&self, channel_id: &str) -> Result<ArtistProfileDto, MicroserviceError> {
        self.inner.artist_profile(channel_id).await
    }
}