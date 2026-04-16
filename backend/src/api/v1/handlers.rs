use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::domain::{
    memory::{CreateMemoryRequest, SearchMemoryRequest},
    session::{AppendMessageRequest, CreateSessionRequest},
    task::{CreateTaskRequest, UpdateTaskStatusRequest},
};
use crate::state::AppState;

pub async fn get_overview(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .overview()
        .await
        .map(|payload| Json(serde_json::to_value(payload).expect("serialize overview")))
        .map_err(internal_error)
}

pub async fn list_tasks(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .list_tasks()
        .await
        .map(|tasks| Json(serde_json::json!(tasks)))
        .map_err(internal_error)
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    state
        .runtime
        .create_task(payload)
        .await
        .map(|task| (StatusCode::CREATED, Json(serde_json::json!(task))))
        .map_err(internal_error)
}

pub async fn run_task(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .run_task(task_id)
        .await
        .map(|receipt| Json(serde_json::json!(receipt)))
        .map_err(internal_error)
}

pub async fn cancel_task(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .cancel_task(task_id)
        .await
        .map(|receipt| Json(serde_json::json!(receipt)))
        .map_err(internal_error)
}

pub async fn list_task_executions(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .list_task_executions(task_id)
        .await
        .map(|items| Json(serde_json::json!(items)))
        .map_err(internal_error)
}

pub async fn update_task_status(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(payload): Json<UpdateTaskStatusRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .update_task_status(task_id, payload)
        .await
        .map(|task| Json(serde_json::json!(task)))
        .map_err(internal_error)
}

pub async fn list_sessions(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .list_sessions()
        .await
        .map(|sessions| Json(serde_json::json!(sessions)))
        .map_err(internal_error)
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    state
        .runtime
        .create_session(payload)
        .await
        .map(|session| (StatusCode::CREATED, Json(serde_json::json!(session))))
        .map_err(internal_error)
}

pub async fn append_message(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(payload): Json<AppendMessageRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .append_message(session_id, payload)
        .await
        .map(|session| Json(serde_json::json!(session)))
        .map_err(internal_error)
}

pub async fn list_memories(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .list_memories()
        .await
        .map(|memories| Json(serde_json::json!(memories)))
        .map_err(internal_error)
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(payload): Json<CreateMemoryRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    state
        .runtime
        .create_memory(payload)
        .await
        .map(|memory| (StatusCode::CREATED, Json(serde_json::json!(memory))))
        .map_err(internal_error)
}

pub async fn search_memories(
    State(state): State<AppState>,
    Json(payload): Json<SearchMemoryRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .runtime
        .search_memories(payload)
        .await
        .map(|results| Json(serde_json::json!(results)))
        .map_err(internal_error)
}

pub async fn list_tools(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "tools": state.runtime.tools(),
        "skills": state.runtime.skills(),
    }))
}

pub async fn list_models(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!(state.runtime.models()))
}

fn internal_error(error: crate::error::AppError) -> (StatusCode, String) {
    let status = match error {
        crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}
