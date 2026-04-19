use crate::domain::task::{AgentTask, TaskExecutionRecord};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: String,
    pub category: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub hot_reload: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDescriptor {
    pub id: String,
    pub description: String,
    pub trigger: String,
    pub installed: bool,
    pub source: String,
    pub path: String,
    pub scripts: Vec<SkillScriptDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillScriptDescriptor {
    pub name: String,
    pub path: String,
    pub runner: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunSkillRequest {
    pub script_name: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub sandbox_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillExecutionResult {
    pub skill: SkillDescriptor,
    pub selected_script: SkillScriptDescriptor,
    pub task: AgentTask,
    pub execution: TaskExecutionRecord,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromoteSkillCandidateRequest {
    pub task_id: Option<uuid::Uuid>,
    pub cluster_key: Option<String>,
    pub candidate_id: String,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromotedSkillResult {
    pub skill: SkillDescriptor,
    pub files: Vec<String>,
    pub source_task: AgentTask,
}
