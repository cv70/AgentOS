use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::learning::TaskLearningReport;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskPriority {
    High,
    Normal,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Pending,
    Running,
    Paused,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionStatus {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceProfile {
    pub cpu: u8,
    pub memory_mb: u32,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub status: TaskStatus,
    pub sandbox_profile: String,
    pub resources: ResourceProfile,
    #[serde(default)]
    pub command: TaskCommand,
    pub working_dir: String,
    #[serde(default)]
    pub strategy_sources: Vec<String>,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub sandbox_profile: String,
    pub command_line: String,
    pub status: ExecutionStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub working_dir: String,
    pub audit_log: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskExecutionInsights {
    pub task_id: Uuid,
    pub executions: Vec<TaskExecutionRecord>,
    pub learning_reports: Vec<TaskLearningReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRunReceipt {
    pub task_id: Uuid,
    pub status: TaskStatus,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub sandbox_profile: String,
    pub command: TaskCommand,
    pub working_dir: String,
    #[serde(default)]
    pub strategy_sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTaskStatusRequest {
    pub status: TaskStatus,
}

impl AgentTask {
    pub fn new(input: CreateTaskRequest) -> Self {
        let now = Utc::now();
        let resources = match input.priority {
            TaskPriority::High => ResourceProfile {
                cpu: 4,
                memory_mb: 2048,
                timeout_secs: 1800,
            },
            TaskPriority::Normal => ResourceProfile {
                cpu: 2,
                memory_mb: 1024,
                timeout_secs: 1200,
            },
            TaskPriority::Low => ResourceProfile {
                cpu: 1,
                memory_mb: 512,
                timeout_secs: 600,
            },
        };

        Self {
            id: Uuid::new_v4(),
            title: input.title,
            description: input.description,
            priority: input.priority,
            status: TaskStatus::Pending,
            sandbox_profile: input.sandbox_profile,
            resources,
            command: input.command,
            working_dir: input.working_dir,
            strategy_sources: input.strategy_sources,
            last_exit_code: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }
}
