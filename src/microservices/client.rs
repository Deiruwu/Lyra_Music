use crate::model::Track;
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Estructura transitoria para deserializar la respuesta del microservicio
#[derive(Deserialize)]
struct ApiResponse<T> {
    status: String,
    data: Option<T>,
    message: Option<String>,
}

/// Errores posibles al hablar con el microservicio, generados limpios con thiserror.
#[derive(Debug, Error)]
pub enum MicroserviceError {
    #[error("Conexión fallida: {0}")]
    ConnectionFailed(std::io::Error),

    #[error("Error de IO: {0}")]
    IoError(std::io::Error),

    #[error("Error del servicio: {0}")]
    ServiceError(String),

    #[error("Respuesta inválida: {0}")]
    InvalidResponse(String),
}

/// Cliente TCP para el microservicio de música.
#[derive(Clone)]
pub struct MicroserviceClient {
    addr: String,
}

impl MicroserviceClient {
    /// Inicializa el cliente directamente con el host y puerto
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            addr: format!("{}:{}", host, port),
        }
    }

    /// Busca tracks en YouTube (Exploración).
    pub async fn search(&self, query: &str, limit: usize, filter: Option<&str>) -> Result<Vec<Track>, MicroserviceError> {
        let payload = json!({
            "action": "search",
            "query": query,
            "limit": limit,
            "filter": filter.unwrap_or("songs")
        }).to_string() + "\n";

        let raw = self.send_raw(&payload).await?;

        let response: ApiResponse<Vec<Track>> = serde_json::from_str(&raw).map_err(|e| {
            MicroserviceError::InvalidResponse(format!("Fallo parseo search: {}. Raw: {}", e, raw))
        })?;

        if response.status == "ok" {
            response.data.ok_or_else(|| {
                MicroserviceError::ServiceError("El microservicio devolvió ok pero 'data' es null en search".into())
            })
        } else {
            Err(MicroserviceError::ServiceError(
                response.message.unwrap_or_else(|| "Error desconocido en search".into()),
            ))
        }
    }

    /// Fuerza la descarga, guarda en Postgres e inicia análisis asíncrono.
    pub async fn download(&self, query: &str) -> Result<Track, MicroserviceError> {
        let payload = json!({
            "action": "download",
            "query": query
        }).to_string() + "\n";

        let raw = self.send_raw(&payload).await?;

        let response: ApiResponse<Track> = serde_json::from_str(&raw).map_err(|e| {
            MicroserviceError::InvalidResponse(format!("Fallo parseo download: {}. Raw: {}", e, raw))
        })?;

        if response.status == "ok" {
            response.data.ok_or_else(|| {
                MicroserviceError::ServiceError("El microservicio devolvió ok pero 'data' es null en download".into())
            })
        } else {
            Err(MicroserviceError::ServiceError(
                response.message.unwrap_or_else(|| "Error desconocido en download".into()),
            ))
        }
    }

    /// Envia una acción `resolve` (Solo obtiene info, NO descarga).
    pub async fn resolve(&self, query: &str) -> Result<Track, MicroserviceError> {
        let payload = json!({
            "action": "resolve",
            "query": query
        }).to_string() + "\n";

        let raw = self.send_raw(&payload).await?;

        let response: ApiResponse<Track> = serde_json::from_str(&raw).map_err(|e| {
            MicroserviceError::InvalidResponse(format!("Fallo parseo resolve: {}. Raw: {}", e, raw))
        })?;

        if response.status == "ok" {
            response.data.ok_or_else(|| {
                MicroserviceError::ServiceError("El microservicio devolvió ok pero 'data' es null".into())
            })
        } else {
            Err(MicroserviceError::ServiceError(
                response.message.unwrap_or_else(|| "Error desconocido del microservicio".into()),
            ))
        }
    }

    /// Envía una acción `radio` y devuelve una lista de Tracks.
    pub async fn radio(&self, query: &str) -> Result<Vec<Track>, MicroserviceError> {
        let payload = json!({
            "action": "radio",
            "query": query
        }).to_string() + "\n";

        let raw = self.send_raw(&payload).await?;

        let response: ApiResponse<Vec<Track>> = serde_json::from_str(&raw).map_err(|e| {
            MicroserviceError::InvalidResponse(format!("Fallo parseo radio: {}. Raw: {}", e, raw))
        })?;

        if response.status == "ok" {
            response.data.ok_or_else(|| {
                MicroserviceError::ServiceError("El microservicio devolvió ok pero 'data' es null".into())
            })
        } else {
            Err(MicroserviceError::ServiceError(
                response.message.unwrap_or_else(|| "Error al generar la radio desde el microservicio".into()),
            ))
        }
    }

    pub async fn mark_as_played(&self, track_id: &str) -> Result<(), MicroserviceError> {
        let payload = json!({
            "action": "played",
            "query": track_id
        }).to_string() + "\n";

        let raw = self.send_raw(&payload).await?;

        let response: ApiResponse<serde_json::Value> = serde_json::from_str(&raw).map_err(|e| {
            MicroserviceError::InvalidResponse(format!("Fallo parseo en mark_as_played: {}", e))
        })?;

        if response.status == "ok" {
            Ok(())
        } else {
            Err(MicroserviceError::ServiceError(
                response.message.unwrap_or_else(|| "Error al registrar el play en el microservicio".into()),
            ))
        }
    }

    /// Abre conexión, envía payload, lee UNA SOLA LÍNEA de respuesta.
    async fn send_raw(&self, payload: &str) -> Result<String, MicroserviceError> {
        let mut stream = TcpStream::connect(&self.addr)
            .await
            .map_err(MicroserviceError::ConnectionFailed)?;

        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(MicroserviceError::IoError)?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();

        reader
            .read_line(&mut response)
            .await
            .map_err(MicroserviceError::IoError)?;

        Ok(response.trim().to_string())
    }
}