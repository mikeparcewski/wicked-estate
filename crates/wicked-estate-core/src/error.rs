//! The engine error type. Crates return [`Result`] so failures are typed, not stringly-panicked.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("extraction error: {0}")]
    Extraction(String),
    #[error("resolution error: {0}")]
    Resolution(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid argument: {0}")]
    Invalid(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
