use axum::http::StatusCode;
use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorPayload,
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Configuration(_) | Self::Storage(_) | Self::Runtime(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "configuration",
            Self::Validation(_) => "validation",
            Self::Conflict(_) => "conflict",
            Self::Storage(_) => "storage",
            Self::Runtime(_) => "runtime",
            Self::NotFound(_) => "not_found",
        }
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorPayload {
                kind: self.kind(),
                message: self.to_string(),
            },
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Configuration(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::Runtime(value.to_string())
    }
}
