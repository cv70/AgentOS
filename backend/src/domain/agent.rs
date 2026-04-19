use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{
    context::WorkspaceContextFile,
    learning::LearningCluster,
    memory::MemoryEntry,
    model::ModelRouteDecision,
    session::{AgentSession, SessionSearchResult},
    task::AgentTask,
    tool::SkillDescriptor,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HermesActionKind {
    MemoryWrite,
    SkillSuggestion,
    TaskSuggestion,
    SessionRecall,
    ModelRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesToolEvent {
    pub tool: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesStrategyTrace {
    pub response_sources: Vec<String>,
    pub task_sources: Vec<HermesTaskStrategyTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesTaskStrategyTrace {
    pub task_title: String,
    pub strategy_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesSuggestedAction {
    pub kind: HermesActionKind,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HermesAgentRequest {
    pub session_id: Option<Uuid>,
    pub title: Option<String>,
    pub working_dir: Option<String>,
    pub message: String,
    #[serde(default)]
    pub auto_persist_memory: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HermesAgentResponse {
    pub session: AgentSession,
    pub assistant_message: String,
    pub routed_model: ModelRouteDecision,
    pub workspace_contexts: Vec<WorkspaceContextFile>,
    pub strategic_clusters: Vec<LearningCluster>,
    pub strategy_trace: HermesStrategyTrace,
    pub memory_hits: Vec<MemoryEntry>,
    pub session_hits: Vec<SessionSearchResult>,
    pub suggested_skills: Vec<SkillDescriptor>,
    pub suggested_tasks: Vec<AgentTask>,
    pub actions: Vec<HermesSuggestedAction>,
    pub tool_trace: Vec<HermesToolEvent>,
    pub memory_written: Option<MemoryEntry>,
}
