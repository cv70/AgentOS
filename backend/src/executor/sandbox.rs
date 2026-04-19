use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::config::config::{SandboxConfig, SandboxProfileConfig};
use crate::domain::task::{AgentTask, ExecutionStatus, TaskExecutionRecord, TaskStatus};
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct SandboxExecutor {
    policy: Arc<SandboxConfig>,
    cancellations: Arc<Mutex<HashMap<Uuid, oneshot::Sender<()>>>>,
}

impl SandboxExecutor {
    pub fn new(policy: SandboxConfig) -> Self {
        Self {
            policy: Arc::new(policy),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_run(&self, task_id: Uuid) -> AppResult<oneshot::Receiver<()>> {
        let mut registry = self.cancellations.lock().await;
        if registry.contains_key(&task_id) {
            return Err(AppError::Runtime(format!(
                "task {} is already running",
                task_id
            )));
        }
        let (tx, rx) = oneshot::channel();
        registry.insert(task_id, tx);
        Ok(rx)
    }

    pub async fn finish_run(&self, task_id: Uuid) {
        self.cancellations.lock().await.remove(&task_id);
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> AppResult<()> {
        let sender = self.cancellations.lock().await.remove(&task_id);
        match sender {
            Some(tx) => {
                let _ = tx.send(());
                Ok(())
            }
            None => Err(AppError::Runtime(format!(
                "task {} is not running",
                task_id
            ))),
        }
    }

    pub async fn run_task(
        &self,
        task: &AgentTask,
        cancel_rx: oneshot::Receiver<()>,
    ) -> AppResult<TaskExecutionRecord> {
        let profile = self.validate_task(task)?;
        let working_dir = resolve_working_dir(&self.policy, profile, &task.working_dir)?;
        let started_at = Utc::now();
        let mut audit_log = vec![
            format!(
                "task={} entering sandbox profile={}",
                task.id, task.sandbox_profile
            ),
            format!("cwd={}", working_dir.display()),
            format!("allowed_program={}", task.command.program),
            format!("profile_writable={}", profile.writable),
            format!(
                "resources=cpu:{} mem:{}MB timeout:{}s max_output:{}B",
                task.resources.cpu,
                task.resources.memory_mb,
                task.resources.timeout_secs,
                self.policy.max_output_bytes
            ),
        ];

        let mut command = Command::new(&task.command.program);
        command
            .env_clear()
            .args(&task.command.args)
            .current_dir(&working_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        copy_allowed_env(&self.policy, &mut command, &mut audit_log);
        audit_log.push(format!(
            "exec={} {}",
            task.command.program,
            task.command.args.join(" ")
        ));

        let mut child = command
            .spawn()
            .map_err(|error| AppError::Runtime(format!("spawn sandbox command: {error}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Runtime("stdout pipe unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Runtime("stderr pipe unavailable".to_string()))?;

        let stdout_handle = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut data = Vec::new();
            let _ = reader.read_to_end(&mut data).await;
            data
        });
        let stderr_handle = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr);
            let mut data = Vec::new();
            let _ = reader.read_to_end(&mut data).await;
            data
        });

        let status = wait_for_child(
            &mut child,
            cancel_rx,
            task.resources.timeout_secs,
            &mut audit_log,
        )
        .await?;
        let finished_at = Utc::now();

        let stdout_bytes = stdout_handle
            .await
            .map_err(|error| AppError::Runtime(format!("join stdout collector: {error}")))?;
        let stderr_bytes = stderr_handle
            .await
            .map_err(|error| AppError::Runtime(format!("join stderr collector: {error}")))?;
        let (stdout, stdout_truncated) = trim_output(stdout_bytes, self.policy.max_output_bytes);
        let (stderr, stderr_truncated) = trim_output(stderr_bytes, self.policy.max_output_bytes);
        if stdout_truncated || stderr_truncated {
            audit_log.push("output truncated to sandbox max_output_bytes".to_string());
        }

        let (execution_status, exit_code) = match status {
            ChildFinish::Exited(code, success) => {
                if success {
                    audit_log.push("process exited successfully".to_string());
                    (ExecutionStatus::Succeeded, code)
                } else {
                    audit_log.push(format!("process exited with {:?}", code));
                    (ExecutionStatus::Failed, code)
                }
            }
            ChildFinish::TimedOut => {
                audit_log.push("timeout reached; process killed".to_string());
                (ExecutionStatus::TimedOut, None)
            }
            ChildFinish::Cancelled => {
                audit_log.push("task cancelled by user request".to_string());
                (ExecutionStatus::Cancelled, None)
            }
        };

        Ok(TaskExecutionRecord {
            id: Uuid::new_v4(),
            task_id: task.id,
            sandbox_profile: task.sandbox_profile.clone(),
            command_line: command_line(task),
            status: execution_status,
            exit_code,
            stdout,
            stderr,
            duration_ms: (finished_at - started_at).num_milliseconds().max(0) as u128,
            started_at,
            finished_at,
            working_dir: working_dir.display().to_string(),
            audit_log,
        })
    }

    pub fn validate_task<'a>(&'a self, task: &AgentTask) -> AppResult<&'a SandboxProfileConfig> {
        let profile = resolve_profile(&self.policy, &task.sandbox_profile)?;
        ensure_program_allowed(&self.policy, profile, &task.command.program)?;
        resolve_working_dir(&self.policy, profile, &task.working_dir)?;
        Ok(profile)
    }
}

pub fn apply_execution_result(task: &mut AgentTask, record: &TaskExecutionRecord) {
    task.last_exit_code = record.exit_code;
    match record.status {
        ExecutionStatus::Succeeded => task.set_status(TaskStatus::Done),
        ExecutionStatus::Failed | ExecutionStatus::TimedOut => task.set_status(TaskStatus::Failed),
        ExecutionStatus::Cancelled => task.set_status(TaskStatus::Cancelled),
    }
}

enum ChildFinish {
    Exited(Option<i32>, bool),
    TimedOut,
    Cancelled,
}

async fn wait_for_child(
    child: &mut Child,
    mut cancel_rx: oneshot::Receiver<()>,
    timeout_secs: u32,
    audit_log: &mut Vec<String>,
) -> AppResult<ChildFinish> {
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|error| AppError::Runtime(format!("wait sandbox command: {error}")))?;
            Ok(ChildFinish::Exited(status.code(), status.success()))
        }
        _ = &mut cancel_rx => {
            terminate_child(child);
            let _ = child.wait().await;
            Ok(ChildFinish::Cancelled)
        }
        _ = sleep(Duration::from_secs(timeout_secs as u64)) => {
            audit_log.push("sandbox timeout guard fired".to_string());
            terminate_child(child);
            let _ = child.wait().await;
            Ok(ChildFinish::TimedOut)
        }
    }
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
    let _ = child.start_kill();
}

fn resolve_profile<'a>(
    policy: &'a SandboxConfig,
    profile_id: &str,
) -> AppResult<&'a SandboxProfileConfig> {
    policy
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| AppError::Runtime(format!("unknown sandbox profile: {profile_id}")))
}

fn ensure_program_allowed(
    policy: &SandboxConfig,
    profile: &SandboxProfileConfig,
    program: &str,
) -> AppResult<()> {
    if policy.allowed_programs.iter().any(|item| item == program)
        && profile.allowed_programs.iter().any(|item| item == program)
    {
        Ok(())
    } else {
        Err(AppError::Runtime(format!(
            "program not allowed by sandbox policy: {program}"
        )))
    }
}

fn resolve_working_dir(
    policy: &SandboxConfig,
    profile: &SandboxProfileConfig,
    input: &str,
) -> AppResult<PathBuf> {
    let path = Path::new(input);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| AppError::Runtime(format!("resolve current dir: {error}")))?
            .join(path)
    };

    if !absolute.exists() {
        return Err(AppError::Runtime(format!(
            "working dir does not exist: {}",
            absolute.display()
        )));
    }
    if !absolute.is_dir() {
        return Err(AppError::Runtime(format!(
            "working dir is not a directory: {}",
            absolute.display()
        )));
    }
    if !policy
        .allowed_working_dirs
        .iter()
        .any(|root| absolute.starts_with(root))
        || !profile
            .allowed_working_dirs
            .iter()
            .any(|root| absolute.starts_with(root))
    {
        return Err(AppError::Runtime(format!(
            "working dir not allowed by sandbox policy: {}",
            absolute.display()
        )));
    }
    Ok(absolute)
}

fn copy_allowed_env(policy: &SandboxConfig, command: &mut Command, audit_log: &mut Vec<String>) {
    let mut exported = Vec::new();
    for key in &policy.allowed_env {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
            exported.push(key.clone());
        }
    }
    audit_log.push(format!("env_allowlist={}", exported.join(",")));
}

fn trim_output(mut bytes: Vec<u8>, limit: usize) -> (String, bool) {
    if bytes.len() > limit {
        bytes.truncate(limit);
        (String::from_utf8_lossy(&bytes).to_string(), true)
    } else {
        (String::from_utf8_lossy(&bytes).to_string(), false)
    }
}

fn command_line(task: &AgentTask) -> String {
    std::iter::once(task.command.program.clone())
        .chain(task.command.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use crate::config::config::{SandboxConfig, SandboxProfileConfig};

    use super::{ensure_program_allowed, resolve_profile, resolve_working_dir};

    fn policy() -> SandboxConfig {
        SandboxConfig {
            allowed_programs: vec!["sh".to_string()],
            allowed_working_dirs: vec!["/tmp".to_string(), "/root/space".to_string()],
            allowed_env: vec!["PATH".to_string()],
            max_output_bytes: 128,
            profiles: vec![
                SandboxProfileConfig {
                    id: "workspace-write".to_string(),
                    writable: true,
                    allowed_working_dirs: vec!["/root/space".to_string()],
                    allowed_programs: vec!["sh".to_string()],
                },
                SandboxProfileConfig {
                    id: "tmp-only".to_string(),
                    writable: true,
                    allowed_working_dirs: vec!["/tmp".to_string()],
                    allowed_programs: vec!["sh".to_string()],
                },
            ],
        }
    }

    #[test]
    fn resolve_relative_dir() {
        let policy = policy();
        let profile = resolve_profile(&policy, "workspace-write").expect("resolve profile");
        let dir = resolve_working_dir(&policy, profile, ".").expect("resolve cwd");
        assert!(dir.is_dir());
    }

    #[test]
    fn block_disallowed_program() {
        let policy = policy();
        let profile = resolve_profile(&policy, "workspace-write").expect("resolve profile");
        assert!(ensure_program_allowed(&policy, profile, "python").is_err());
    }

    #[test]
    fn block_profile_directory_escape() {
        let policy = policy();
        let profile = resolve_profile(&policy, "tmp-only").expect("resolve profile");
        assert!(resolve_working_dir(&policy, profile, "/root/space").is_err());
    }
}
