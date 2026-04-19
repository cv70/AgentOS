#[path = "../src/error.rs"]
mod error;

use axum::http::StatusCode;
use error::AppError;

#[test]
fn display_error_variants() {
    assert_eq!(
        AppError::Configuration("bad yaml".into()).to_string(),
        "configuration error: bad yaml"
    );
    assert_eq!(
        AppError::Validation("missing title".into()).to_string(),
        "validation error: missing title"
    );
    assert_eq!(
        AppError::Conflict("task-1".into()).to_string(),
        "conflict: task-1"
    );
    assert_eq!(
        AppError::Storage("disk full".into()).to_string(),
        "storage error: disk full"
    );
    assert_eq!(
        AppError::Conflict("task-1".into()).status_code(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        AppError::Runtime("scheduler stopped".into()).to_string(),
        "runtime error: scheduler stopped"
    );
    assert_eq!(
        AppError::NotFound("task-1".into()).to_string(),
        "not found: task-1"
    );
}
