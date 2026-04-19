use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub pinned_decisions: Vec<String>,
    pub compressed_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: Uuid,
    pub title: String,
    pub working_dir: String,
    pub messages: Vec<SessionMessage>,
    pub summary: SessionSummary,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSessionRequest {
    pub title: String,
    pub working_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppendMessageRequest {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchSessionRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSearchResult {
    pub session_id: Uuid,
    pub title: String,
    pub working_dir: String,
    pub role: MessageRole,
    pub excerpt: String,
    pub score: f32,
    pub created_at: DateTime<Utc>,
}

impl AgentSession {
    pub fn new(input: CreateSessionRequest) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: input.title,
            working_dir: input.working_dir,
            messages: Vec::new(),
            summary: SessionSummary {
                pinned_decisions: vec!["默认保留关键任务约束与用户偏好".to_string()],
                compressed_context: "会话刚创建，尚未触发摘要压缩。".to_string(),
            },
            created_at: now,
            updated_at: now,
        }
    }

    pub fn append_message(&mut self, input: AppendMessageRequest, window_size: usize) {
        self.messages.push(SessionMessage {
            id: Uuid::new_v4(),
            role: input.role,
            content: input.content,
            created_at: Utc::now(),
        });
        self.updated_at = Utc::now();

        if self.messages.len() > window_size {
            let dropped = self.messages.len().saturating_sub(window_size);
            self.summary.compressed_context = format!(
                "已压缩 {} 条历史消息，保留最近 {} 条上下文。",
                dropped, window_size
            );
            self.messages.drain(0..dropped);
        }
    }
}
