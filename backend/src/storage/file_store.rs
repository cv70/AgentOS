use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::RwLock;

use crate::config::config::StorageConfig;
use crate::domain::memory::MemoryEntry;
use crate::domain::session::AgentSession;
use crate::domain::task::AgentTask;
use crate::error::AppResult;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub tasks: Vec<AgentTask>,
    pub sessions: Vec<AgentSession>,
    pub memories: Vec<MemoryEntry>,
}

#[derive(Clone)]
pub struct FileStore {
    path: PathBuf,
    state: Arc<RwLock<PersistedState>>,
}

impl FileStore {
    pub async fn new(config: &StorageConfig) -> AppResult<Self> {
        let data_dir = Path::new(&config.data_dir);
        fs::create_dir_all(data_dir).await?;
        let path = data_dir.join(&config.state_file);

        let state = if fs::try_exists(&path).await? {
            let raw = fs::read_to_string(&path).await?;
            serde_json::from_str(&raw)?
        } else {
            PersistedState::default()
        };

        Ok(Self {
            path,
            state: Arc::new(RwLock::new(state)),
        })
    }

    pub async fn read(&self) -> PersistedState {
        self.state.read().await.clone()
    }

    pub async fn write(&self, state: PersistedState) -> AppResult<()> {
        let raw = serde_json::to_string_pretty(&state)?;
        fs::write(&self.path, raw).await?;
        *self.state.write().await = state;
        Ok(())
    }
}
