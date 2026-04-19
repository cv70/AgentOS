use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub id: String,
    pub title: String,
    pub description: String,
    pub rationale: String,
    pub suggested_trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLearningReport {
    pub id: Uuid,
    pub task_id: Uuid,
    pub execution_id: Uuid,
    pub status: String,
    pub source_strategy_keys: Vec<String>,
    pub recap: String,
    pub lessons: Vec<String>,
    pub memory_ids: Vec<Uuid>,
    pub session_id: Option<Uuid>,
    pub skill_candidates: Vec<SkillCandidate>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningCluster {
    pub key: String,
    pub title: String,
    pub capability: String,
    pub report_count: usize,
    pub source_usage_count: usize,
    pub source_success_rate: f32,
    pub success_rate: f32,
    pub recency_score: f32,
    pub strategic_weight: f32,
    pub suppression_level: String,
    pub pruned_from_planning: bool,
    pub common_lessons: Vec<String>,
    pub example_tasks: Vec<String>,
    pub recommended_commands: Vec<String>,
    pub strategic_skill_candidates: Vec<SkillCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningSummary {
    pub generated_at: DateTime<Utc>,
    pub total_reports: usize,
    pub clusters: Vec<LearningCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyEventKind {
    TaskExecuted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyEvaluationEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub execution_id: Uuid,
    pub strategy_source_key: String,
    pub event_kind: StrategyEventKind,
    pub outcome_status: String,
    pub summary: String,
    pub evidence: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyTimeline {
    pub generated_at: DateTime<Utc>,
    pub total_events: usize,
    pub events: Vec<StrategyEvaluationEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListStrategyTimelineRequest {
    pub working_dir: Option<String>,
    pub limit: Option<usize>,
}
