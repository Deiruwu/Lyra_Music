#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Error de I/O en el disco: {0}")]
    Io(#[from] std::io::Error),
    #[error("Formato de contenedor no soportado o corrupto: {0}")]
    Format(String),
    #[error("Fallo del codec interno: {0}")]
    Codec(String),
    #[error("Error matemático en el resampling: {0}")]
    Resample(String),
    #[error("El archivo no contiene un flujo de audio legible")]
    NoAudioStream,
}