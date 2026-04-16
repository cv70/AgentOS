use crate::runtime::agent_runtime::AgentRuntime;

#[derive(Clone)]
pub struct AppState {
    pub runtime: AgentRuntime,
}
