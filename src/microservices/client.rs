use crate::model::Track;
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

    pub async fn search(&self, query: &str, limit: Option<usize>, filter: Option<&str>) -> Result<Vec<Track>, MicroserviceError> {
        self.inner.search(query, limit, filter).await
    }

    pub async fn download(&self, query: &str) -> Result<Track, MicroserviceError> {
        self.inner.download(query).await
    }

    pub async fn resolve(&self, query: &str) -> Result<Track, MicroserviceError> {
        self.inner.resolve(query).await
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
}