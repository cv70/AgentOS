use axum::{Router, routing::{get, patch, post}};

use crate::api::v1::handlers;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/overview", get(handlers::get_overview))
        .route("/tasks", get(handlers::list_tasks).post(handlers::create_task))
        .route("/tasks/:task_id/status", patch(handlers::update_task_status))
        .route("/tasks/:task_id/run", post(handlers::run_task))
        .route("/tasks/:task_id/cancel", post(handlers::cancel_task))
        .route("/tasks/:task_id/executions", get(handlers::list_task_executions))
        .route("/sessions", get(handlers::list_sessions).post(handlers::create_session))
        .route("/sessions/:session_id/messages", post(handlers::append_message))
        .route("/memories", get(handlers::list_memories).post(handlers::create_memory))
        .route("/memories/search", post(handlers::search_memories))
        .route("/tools", get(handlers::list_tools))
        .route("/models", get(handlers::list_models))
}
