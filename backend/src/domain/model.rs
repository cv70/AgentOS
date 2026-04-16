use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ModelProvider {
    pub id: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub is_default: bool,
}
