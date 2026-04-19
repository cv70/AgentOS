use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::domain::context::WorkspaceContextFile;

#[derive(Clone, Default)]
pub struct WorkspaceContextProvider {
    cache: Arc<Mutex<HashMap<String, CachedWorkspaceContexts>>>,
}

#[derive(Clone)]
struct CachedWorkspaceContexts {
    fingerprint: Vec<ContextFingerprint>,
    contexts: Vec<WorkspaceContextFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextFingerprint {
    path: String,
    modified: Option<SystemTime>,
}

impl WorkspaceContextProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self, working_dir: &str) -> Vec<WorkspaceContextFile> {
        let fingerprint = collect_fingerprint(working_dir);
        let cache_key = working_dir.to_string();

        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(&cache_key) {
                if entry.fingerprint == fingerprint {
                    return entry.contexts.clone();
                }
            }
        }

        let contexts = discover_workspace_contexts(working_dir);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                cache_key,
                CachedWorkspaceContexts {
                    fingerprint,
                    contexts: contexts.clone(),
                },
            );
        }
        contexts
    }
}

pub fn discover_workspace_contexts(working_dir: &str) -> Vec<WorkspaceContextFile> {
    let base = PathBuf::from(working_dir);
    let candidates = [
        ("codex", "AGENTS.md"),
        ("hermes", "SOUL.md"),
        ("memory", "MEMORY.md"),
        ("user", "USER.md"),
        ("workspace", "README.md"),
    ];

    let mut contexts = candidates
        .iter()
        .filter_map(|(kind, file_name)| {
            let path = base.join(file_name);
            let raw = fs::read_to_string(&path).ok()?;
            Some(parse_workspace_context(*kind, &path, &raw))
        })
        .collect::<Vec<_>>();

    contexts.sort_by(|left, right| left.path.cmp(&right.path));
    contexts
}

fn collect_fingerprint(working_dir: &str) -> Vec<ContextFingerprint> {
    let base = PathBuf::from(working_dir);
    let candidates = ["AGENTS.md", "SOUL.md", "MEMORY.md", "USER.md", "README.md"];
    let mut fingerprint = candidates
        .iter()
        .filter_map(|file_name| {
            let path = base.join(file_name);
            let metadata = fs::metadata(&path).ok()?;
            Some(ContextFingerprint {
                path: path.display().to_string(),
                modified: metadata.modified().ok(),
            })
        })
        .collect::<Vec<_>>();
    fingerprint.sort_by(|left, right| left.path.cmp(&right.path));
    fingerprint
}

fn parse_workspace_context(kind: &str, path: &Path, raw: &str) -> WorkspaceContextFile {
    let mut title = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("context")
        .to_string();
    let mut excerpt_lines = Vec::new();
    let mut guidance = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#')
            && title
                == path
                    .file_name()
                    .and_then(|item| item.to_str())
                    .unwrap_or("context")
        {
            title = trimmed.trim_start_matches('#').trim().to_string();
            continue;
        }
        if excerpt_lines.len() < 3 && !trimmed.starts_with("- ") && !trimmed.starts_with("* ") {
            excerpt_lines.push(trimmed.to_string());
        }
        if guidance.len() < 4 {
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                guidance.push(item.trim().to_string());
                continue;
            }
            if let Some((_, item)) = trimmed.split_once(':') {
                if trimmed.len() < 120 {
                    guidance.push(item.trim().to_string());
                }
            }
        }
    }

    if guidance.is_empty() && !excerpt_lines.is_empty() {
        guidance.extend(excerpt_lines.iter().take(2).cloned());
    }

    WorkspaceContextFile {
        kind: kind.to_string(),
        path: path.display().to_string(),
        title,
        excerpt: excerpt_lines.join(" "),
        guidance,
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    use uuid::Uuid;

    use super::{WorkspaceContextProvider, discover_workspace_contexts};

    #[test]
    fn discovery_detects_agents_file() {
        let temp_dir = env::temp_dir().join(format!("agentos-context-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        fs::write(
            temp_dir.join("AGENTS.md"),
            "# Repo Rules\n- Use Rust\n- Prefer local execution\n",
        )
        .expect("write context file");

        let contexts = discover_workspace_contexts(&temp_dir.to_string_lossy());
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].kind, "codex");
        assert!(
            contexts[0]
                .guidance
                .iter()
                .any(|item| item.contains("Prefer local"))
        );

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn provider_refreshes_cache_when_context_changes() {
        let temp_dir = env::temp_dir().join(format!("agentos-context-cache-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let agents_path = temp_dir.join("AGENTS.md");
        fs::write(&agents_path, "# Repo Rules\n- First version\n").expect("write initial context");

        let provider = WorkspaceContextProvider::new();
        let first = provider.list(&temp_dir.to_string_lossy());
        assert!(
            first[0]
                .guidance
                .iter()
                .any(|item| item.contains("First version"))
        );

        thread::sleep(Duration::from_millis(5));
        fs::write(&agents_path, "# Repo Rules\n- Second version\n").expect("update context");

        let second = provider.list(&temp_dir.to_string_lossy());
        assert!(
            second[0]
                .guidance
                .iter()
                .any(|item| item.contains("Second version"))
        );

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }
}
