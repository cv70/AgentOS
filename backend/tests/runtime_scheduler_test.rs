#[path = "../src/config/mod.rs"]
mod config;
#[path = "../src/domain/mod.rs"]
mod domain;
#[path = "../src/error.rs"]
mod error;
#[path = "../src/executor/mod.rs"]
mod executor;
#[path = "../src/runtime/mod.rs"]
mod runtime;
#[path = "../src/storage/mod.rs"]
mod storage;

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use config::config::{
    AppConfig, ModelConfig, ModelProviderConfig, RuntimeConfig, SandboxConfig,
    SandboxProfileConfig, ServerConfig, StorageConfig,
};
use domain::task::{CreateTaskRequest, TaskCommand, TaskPriority, TaskStatus};
use runtime::agent_runtime::AgentRuntime;

fn test_config(data_dir: &str, working_dir: &str) -> AppConfig {
    AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 8787,
        },
        storage: StorageConfig {
            data_dir: data_dir.to_string(),
            state_file: "agentos.db".to_string(),
        },
        runtime: RuntimeConfig {
            max_concurrent_tasks: 1,
            session_window_size: 8,
            memory_search_limit: 4,
        },
        sandbox: SandboxConfig {
            allowed_programs: vec!["sh".to_string(), "python3".to_string()],
            allowed_working_dirs: vec![working_dir.to_string(), "/tmp".to_string()],
            allowed_env: vec!["PATH".to_string(), "HOME".to_string()],
            max_output_bytes: 16 * 1024,
            profiles: vec![SandboxProfileConfig {
                id: "workspace-write".to_string(),
                writable: true,
                allowed_working_dirs: vec![working_dir.to_string()],
                allowed_programs: vec!["sh".to_string(), "python3".to_string()],
            }],
        },
        models: ModelConfig {
            default_model: "local-test".to_string(),
            providers: vec![ModelProviderConfig {
                id: "local-test".to_string(),
                kind: "local".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                capabilities: vec!["chat".to_string(), "code".to_string()],
                model_name: Some("phi4".to_string()),
                api_key_env: None,
            }],
        },
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("agentos-{label}-{nanos}"))
}

async fn wait_for_status(runtime: &AgentRuntime, task_id: uuid::Uuid, expected: TaskStatus) {
    for _ in 0..40 {
        let task = runtime
            .list_tasks()
            .await
            .expect("list tasks")
            .into_iter()
            .find(|task| task.id == task_id)
            .expect("task exists");
        if task.status == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let snapshot = runtime
        .list_tasks()
        .await
        .expect("list tasks for panic")
        .into_iter()
        .map(|task| (task.id, task.title, task.status))
        .collect::<Vec<_>>();
    panic!("task {task_id} did not reach expected status {expected:?}; snapshot: {snapshot:?}");
}

async fn wait_for_scheduler_idle(runtime: &AgentRuntime) {
    for _ in 0..40 {
        let scheduler = runtime.scheduler_status().await.expect("scheduler status");
        if scheduler.running_task_ids.is_empty() && scheduler.queued_task_ids.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let scheduler = runtime
        .scheduler_status()
        .await
        .expect("scheduler status for panic");
    panic!(
        "scheduler did not become idle: {:?}",
        (scheduler.running_task_ids, scheduler.queued_task_ids)
    );
}

#[tokio::test]
async fn scheduler_enforces_capacity_and_dispatches_queued_task() {
    let working_dir = unique_temp_dir("workspace");
    let data_dir = unique_temp_dir("data");
    std::fs::create_dir_all(&working_dir).expect("create working dir");
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let runtime = AgentRuntime::new(test_config(
        data_dir.to_str().expect("data dir utf8"),
        working_dir.to_str().expect("working dir utf8"),
    ))
    .await
    .expect("create runtime");

    let task_one = runtime
        .create_task(CreateTaskRequest {
            title: "slow task".to_string(),
            description: "holds the only scheduler slot briefly".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "workspace-write".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    "i=0; while [ \"$i\" -lt 400000 ]; do i=$((i+1)); done; printf task-one"
                        .to_string(),
                ],
            },
            working_dir: working_dir.display().to_string(),
            strategy_sources: Vec::new(),
        })
        .await
        .expect("create task one");

    let task_two = runtime
        .create_task(CreateTaskRequest {
            title: "queued task".to_string(),
            description: "should wait until the running task finishes".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "workspace-write".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "printf queued-two".to_string()],
            },
            working_dir: working_dir.display().to_string(),
            strategy_sources: Vec::new(),
        })
        .await
        .expect("create task two");

    let receipt_one = runtime.run_task(task_one.id).await.expect("run task one");
    assert_eq!(receipt_one.status, TaskStatus::Running);

    let receipt_two = runtime.run_task(task_two.id).await.expect("run task two");
    assert_eq!(receipt_two.status, TaskStatus::Pending);
    assert!(receipt_two.message.contains("queued"));

    let scheduler = runtime.scheduler_status().await.expect("scheduler status");
    assert_eq!(scheduler.max_concurrent_tasks, 1);
    assert_eq!(scheduler.running_task_ids, vec![task_one.id]);
    assert_eq!(scheduler.queued_task_ids, vec![task_two.id]);
    assert_eq!(scheduler.available_slots, 0);

    wait_for_status(&runtime, task_one.id, TaskStatus::Done).await;
    wait_for_status(&runtime, task_two.id, TaskStatus::Done).await;
    wait_for_scheduler_idle(&runtime).await;

    let scheduler = runtime
        .scheduler_status()
        .await
        .expect("scheduler status after drain");
    assert!(scheduler.running_task_ids.is_empty());
    assert!(scheduler.queued_task_ids.is_empty());
    assert_eq!(scheduler.available_slots, 1);

    let insights = runtime
        .task_execution_insights(task_two.id)
        .await
        .expect("execution insights");
    assert_eq!(insights.executions.len(), 1);
    assert_eq!(insights.executions[0].stdout, "queued-two");

    let fetched = runtime
        .get_execution(insights.executions[0].id)
        .await
        .expect("fetch execution by id");
    assert_eq!(fetched.id, insights.executions[0].id);
}
