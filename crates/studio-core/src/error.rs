use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("invalid model manifest {path}: {reason}")]
    Manifest { path: PathBuf, reason: String },

    #[error("invalid config: {0}")]
    Config(String),

    #[error("invalid engine kind: {0}")]
    EngineKind(String),

    #[error("invalid task kind: {0}")]
    TaskKind(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("process error: {0}")]
    Process(String),
}
