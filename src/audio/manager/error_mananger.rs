use std::io::Error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("No hay tracks anteriores en el historial")]
    NoHistory,

    #[error("Índice de cola inválido: intentó acceder al elemento {index}, pero la cola solo tiene longitud {len}")]
    InvalidQueueIndex { index: usize, len: usize },

    #[error("Índice de historial inválido: intentó retroceder {index} canciones, pero el historial solo tiene longitud {len}")]
    InvalidHistoryIndex { index: usize, len: usize },

    #[error("Índice fuera de rango")]
    IndexOutOfRange,

    #[error("La cola se vació inesperadamente durante la extracción")]
    QueueEmptiedUnexpectedly,

    #[error("No se pudo probear el track")]
    ProbeFailed,

    #[error("Fallo al crear hilo supervisor: {0}")]
    SupervisorSpawnFailed(Error),

    #[error("Fallo al iniciar el motor de audio: {0}")]
    EngineStartFailed(String),
}