use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ModelProvider {
    pub id: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub is_default: bool,
    pub routing_weight: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteModelRequest {
    pub capability: String,
    #[serde(default)]
    pub prefer_local: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetDefaultModelRequest {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRouteDecision {
    pub selected: ModelProvider,
    pub fallbacks: Vec<ModelProvider>,
    pub reason: String,
}
