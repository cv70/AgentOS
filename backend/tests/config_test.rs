#[path = "../src/config/mod.rs"]
mod config;

use config::config::AppConfig;

#[test]
fn load_default_config() {
    let config = AppConfig::load_from_path("config.yaml").expect("load config");
    assert_eq!(config.server.port, 8787);
    assert_eq!(config.runtime.max_concurrent_tasks, 3);
    assert_eq!(config.models.default_model, "local-phi4");
    assert_eq!(config.storage.state_file, "agentos.db");
    assert_eq!(
        config.models.providers[0].model_name.as_deref(),
        Some("phi4")
    );
    assert!(
        config
            .sandbox
            .allowed_programs
            .iter()
            .any(|item| item == "sh")
    );
    assert_eq!(config.sandbox.max_output_bytes, 32768);
    assert!(
        config
            .sandbox
            .profiles
            .iter()
            .any(|profile| profile.id == "read-only")
    );
    assert!(
        config
            .sandbox
            .profiles
            .iter()
            .any(|profile| profile.id == "tmp-only")
    );
}
