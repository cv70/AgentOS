use crate::domain::memory::CreateMemoryRequest;
use crate::domain::session::{AppendMessageRequest, CreateSessionRequest};
use crate::domain::task::CreateTaskRequest;
use crate::error::{AppError, AppResult};

pub fn validate_create_task_request(input: &CreateTaskRequest) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(AppError::Validation(
            "task title must not be empty".to_string(),
        ));
    }
    if input.command.program.trim().is_empty() {
        return Err(AppError::Validation(
            "task command program must not be empty".to_string(),
        ));
    }
    if input.working_dir.trim().is_empty() {
        return Err(AppError::Validation(
            "task working_dir must not be empty".to_string(),
        ));
    }
    if input.sandbox_profile.trim().is_empty() {
        return Err(AppError::Validation(
            "task sandbox_profile must not be empty".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_create_session_request(input: &CreateSessionRequest) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(AppError::Validation(
            "session title must not be empty".to_string(),
        ));
    }
    if input.working_dir.trim().is_empty() {
        return Err(AppError::Validation(
            "session working_dir must not be empty".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_append_message_request(input: &AppendMessageRequest) -> AppResult<()> {
    if input.content.trim().is_empty() {
        return Err(AppError::Validation(
            "session message content must not be empty".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_create_memory_request(input: &CreateMemoryRequest) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(AppError::Validation(
            "memory title must not be empty".to_string(),
        ));
    }
    if input.content.trim().is_empty() {
        return Err(AppError::Validation(
            "memory content must not be empty".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_search_query(query: &str, label: &str) -> AppResult<()> {
    if query.trim().is_empty() {
        return Err(AppError::Validation(format!("{label} must not be empty")));
    }
    Ok(())
}
