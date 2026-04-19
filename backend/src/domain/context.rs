use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContextFile {
    pub kind: String,
    pub path: String,
    pub title: String,
    pub excerpt: String,
    pub guidance: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListWorkspaceContextsRequest {
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListLearningSummaryRequest {
    pub working_dir: Option<String>,
}
