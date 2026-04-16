use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryScope {
    ShortTerm,
    LongTerm,
    Episodic,
    Semantic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub scope: MemoryScope,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMemoryRequest {
    pub scope: MemoryScope,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemorySearchResult {
    pub memory: MemoryEntry,
    pub score: f32,
    pub keyword_hits: usize,
}

impl MemoryEntry {
    pub fn new(input: CreateMemoryRequest) -> Self {
        Self {
            id: Uuid::new_v4(),
            scope: input.scope,
            title: input.title,
            content: input.content,
            tags: input.tags,
            created_at: Utc::now(),
        }
    }
}
