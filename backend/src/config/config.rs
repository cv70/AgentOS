use std::fs;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub runtime: RuntimeConfig,
    pub sandbox: SandboxConfig,
    pub models: ModelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub state_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub max_concurrent_tasks: usize,
    pub session_window_size: usize,
    pub memory_search_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub allowed_programs: Vec<String>,
    pub allowed_working_dirs: Vec<String>,
    pub allowed_env: Vec<String>,
    pub max_output_bytes: usize,
    pub profiles: Vec<SandboxProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxProfileConfig {
    pub id: String,
    pub writable: bool,
    pub allowed_working_dirs: Vec<String>,
    pub allowed_programs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub default_model: String,
    pub providers: Vec<ModelProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    pub id: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
}

impl AppConfig {
    pub fn load_from_path(path: &str) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }

    pub fn load() -> Result<Self> {
        Self::load_from_path("config.yaml")
    }
}

pub fn parse_config_path_from_args<I>(args: I) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            return iter.next();
        }
    }
    None
}
