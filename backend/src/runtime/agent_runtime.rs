use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::config::config::AppConfig;
use crate::domain::memory::{CreateMemoryRequest, MemoryEntry, MemorySearchResult, SearchMemoryRequest};
use crate::domain::model::ModelProvider;
use crate::domain::session::{AgentSession, AppendMessageRequest, CreateSessionRequest};
use crate::domain::task::{
    AgentTask, CreateTaskRequest, TaskExecutionRecord, TaskRunReceipt, TaskStatus, UpdateTaskStatusRequest,
};
use crate::domain::tool::{SkillDescriptor, ToolDescriptor};
use crate::error::{AppError, AppResult};
use crate::executor::sandbox::{SandboxExecutor, apply_execution_result};
use crate::storage::sqlite_store::{PersistedState, SqliteStore};

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeOverview {
    pub node_name: String,
    pub scheduler: SchedulerSnapshot,
    pub sessions: SessionSnapshot,
    pub memory: MemorySnapshot,
    pub tools: ToolSnapshot,
    pub models: Vec<ModelProvider>,
    pub recent_tasks: Vec<AgentTask>,
    pub recent_sessions: Vec<AgentSession>,
    pub recent_memories: Vec<MemoryEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchedulerSnapshot {
    pub max_concurrent_tasks: usize,
    pub queue_depth: usize,
    pub running: usize,
    pub paused: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub total: usize,
    pub window_size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    pub total: usize,
    pub search_limit: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSnapshot {
    pub tools: usize,
    pub skills: usize,
    pub hot_reload_enabled: bool,
}

#[derive(Clone)]
pub struct AgentRuntime {
    config: Arc<AppConfig>,
    store: SqliteStore,
    executor: SandboxExecutor,
}

impl AgentRuntime {
    pub async fn new(config: AppConfig) -> AppResult<Self> {
        let config = Arc::new(config);
        let store = SqliteStore::new(&config.storage).await?;
        let runtime = Self {
            executor: SandboxExecutor::new(config.sandbox.clone()),
            config,
            store,
        };
        runtime.seed_if_empty().await?;
        Ok(runtime)
    }

    async fn seed_if_empty(&self) -> AppResult<()> {
        let tasks = self.store.list_tasks().await?;
        let sessions = self.store.list_sessions().await?;
        let memories = self.store.list_memories().await?;
        if !(tasks.is_empty() && sessions.is_empty() && memories.is_empty()) {
            return Ok(());
        }

        let mut state = PersistedState::default();
        state.tasks = vec![
            AgentTask::new(CreateTaskRequest {
                title: "索引工作区".to_string(),
                description: "扫描当前项目目录并建立本地知识索引。".to_string(),
                priority: crate::domain::task::TaskPriority::High,
                sandbox_profile: "workspace-write".to_string(),
                command: crate::domain::task::TaskCommand {
                    program: "sh".to_string(),
                    args: vec!["-lc".to_string(), "pwd && ls -1".to_string()],
                },
                working_dir: "/root/space".to_string(),
            }),
            AgentTask::new(CreateTaskRequest {
                title: "预热工具插件".to_string(),
                description: "检查 Skill 元数据并启用热加载。".to_string(),
                priority: crate::domain::task::TaskPriority::Normal,
                sandbox_profile: "read-only".to_string(),
                command: crate::domain::task::TaskCommand {
                    program: "sh".to_string(),
                    args: vec!["-lc".to_string(), "find . -maxdepth 2 -type f | head".to_string()],
                },
                working_dir: "/root/space".to_string(),
            }),
        ];

        let mut session = AgentSession::new(CreateSessionRequest {
            title: "默认工作会话".to_string(),
            working_dir: "/workspace".to_string(),
        });
        session.append_message(
            AppendMessageRequest {
                role: crate::domain::session::MessageRole::System,
                content: "AgentOS 已启动，优先使用本地资源并记录可追溯审计信息。".to_string(),
            },
            self.config.runtime.session_window_size,
        );
        state.sessions.push(session);

        state.memories = vec![
            MemoryEntry::new(CreateMemoryRequest {
                scope: crate::domain::memory::MemoryScope::LongTerm,
                title: "用户偏好".to_string(),
                content: "默认使用中文回复，并优先在本地执行任务。".to_string(),
                tags: vec!["preference".to_string(), "locale".to_string()],
            }),
            MemoryEntry::new(CreateMemoryRequest {
                scope: crate::domain::memory::MemoryScope::Semantic,
                title: "系统原则".to_string(),
                content: "工具执行遵循最小权限、可观测性和优雅降级。".to_string(),
                tags: vec!["policy".to_string(), "runtime".to_string()],
            }),
        ];

        for task in state.tasks {
            self.store.upsert_task(task).await?;
        }
        for session in state.sessions {
            self.store.upsert_session(session).await?;
        }
        for memory in state.memories {
            self.store.upsert_memory(memory).await?;
        }
        Ok(())
    }

    pub fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                id: "fs".to_string(),
                category: "filesystem".to_string(),
                display_name: "Workspace FS".to_string(),
                permissions: vec!["read".to_string(), "write".to_string()],
                hot_reload: false,
            },
            ToolDescriptor {
                id: "sandbox".to_string(),
                category: "execution".to_string(),
                display_name: "Code Sandbox".to_string(),
                permissions: vec![
                    "exec".to_string(),
                    "timeout".to_string(),
                    "audit".to_string(),
                    "allow-list".to_string(),
                    "cancel".to_string(),
                ],
                hot_reload: false,
            },
            ToolDescriptor {
                id: "skills".to_string(),
                category: "plugin".to_string(),
                display_name: "Skill Loader".to_string(),
                permissions: vec!["discover".to_string(), "reload".to_string()],
                hot_reload: true,
            },
            ToolDescriptor {
                id: "network".to_string(),
                category: "network".to_string(),
                display_name: "Network Gateway".to_string(),
                permissions: vec!["allow-list".to_string()],
                hot_reload: false,
            },
        ]
    }

    pub fn skills(&self) -> Vec<SkillDescriptor> {
        vec![
            SkillDescriptor {
                id: "code-review".to_string(),
                description: "聚焦代码审查、风险识别与修复建议。".to_string(),
                trigger: "review / audit / bugfix".to_string(),
                installed: true,
            },
            SkillDescriptor {
                id: "workspace-memory".to_string(),
                description: "将项目结构和偏好沉淀为长期记忆。".to_string(),
                trigger: "remember / persist / summarize".to_string(),
                installed: true,
            },
        ]
    }

    pub fn models(&self) -> Vec<ModelProvider> {
        self.config
            .models
            .providers
            .iter()
            .map(|provider| ModelProvider {
                id: provider.id.clone(),
                kind: provider.kind.clone(),
                endpoint: provider.endpoint.clone(),
                capabilities: provider.capabilities.clone(),
                is_default: provider.id == self.config.models.default_model,
            })
            .collect()
    }

    pub async fn overview(&self) -> AppResult<RuntimeOverview> {
        let tasks = self.store.list_tasks().await?;
        let sessions = self.store.list_sessions().await?;
        let memories = self.store.list_memories().await?;
        let running = tasks.iter().filter(|task| task.status == TaskStatus::Running).count();
        let paused = tasks.iter().filter(|task| task.status == TaskStatus::Paused).count();

        Ok(RuntimeOverview {
            node_name: "agentos-local-node".to_string(),
            scheduler: SchedulerSnapshot {
                max_concurrent_tasks: self.config.runtime.max_concurrent_tasks,
                queue_depth: tasks.len().saturating_sub(running),
                running,
                paused,
            },
            sessions: SessionSnapshot {
                total: sessions.len(),
                window_size: self.config.runtime.session_window_size,
            },
            memory: MemorySnapshot {
                total: memories.len(),
                search_limit: self.config.runtime.memory_search_limit,
            },
            tools: ToolSnapshot {
                tools: self.tools().len(),
                skills: self.skills().len(),
                hot_reload_enabled: true,
            },
            models: self.models(),
            recent_tasks: tasks.into_iter().take(5).collect(),
            recent_sessions: sessions.into_iter().take(3).collect(),
            recent_memories: memories.into_iter().take(4).collect(),
        })
    }

    pub async fn list_tasks(&self) -> AppResult<Vec<AgentTask>> {
        self.store.list_tasks().await
    }

    pub async fn create_task(&self, input: CreateTaskRequest) -> AppResult<AgentTask> {
        let task = AgentTask::new(input);
        self.store.upsert_task(task.clone()).await?;
        Ok(task)
    }

    pub async fn update_task_status(&self, task_id: Uuid, input: UpdateTaskStatusRequest) -> AppResult<AgentTask> {
        let mut task = self
            .store
            .get_task(task_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("task {}", task_id)))?;
        task.set_status(input.status);
        self.store.upsert_task(task.clone()).await?;
        Ok(task)
    }

    pub async fn run_task(&self, task_id: Uuid) -> AppResult<TaskRunReceipt> {
        let mut task = self
            .store
            .get_task(task_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("task {}", task_id)))?;
        if task.status == TaskStatus::Running {
            return Err(AppError::Runtime(format!("task {} is already running", task_id)));
        }
        self.executor.validate_task(&task)?;
        let cancel_rx = self.executor.register_run(task_id).await?;
        task.set_status(TaskStatus::Running);
        self.store.upsert_task(task.clone()).await?;

        let store = self.store.clone();
        let executor = self.executor.clone();
        tokio::spawn(async move {
            let mut execution = match executor.run_task(&task, cancel_rx).await {
                Ok(record) => record,
                Err(error) => TaskExecutionRecord {
                    id: Uuid::new_v4(),
                    task_id: task.id,
                    sandbox_profile: task.sandbox_profile.clone(),
                    command_line: format!("{} {}", task.command.program, task.command.args.join(" ")),
                    status: crate::domain::task::ExecutionStatus::Failed,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    duration_ms: 0,
                    started_at: chrono::Utc::now(),
                    finished_at: chrono::Utc::now(),
                    working_dir: task.working_dir.clone(),
                    audit_log: vec!["sandbox execution failed before child completion".to_string()],
                },
            };
            let mut finished_task = task.clone();
            apply_execution_result(&mut finished_task, &execution);
            if matches!(execution.status, crate::domain::task::ExecutionStatus::Failed) && execution.audit_log.is_empty() {
                execution.audit_log.push("execution failed without audit trail".to_string());
            }
            let _ = store.upsert_task(finished_task).await;
            let _ = store.add_task_execution(execution).await;
            executor.finish_run(task_id).await;
        });

        Ok(TaskRunReceipt {
            task_id,
            status: TaskStatus::Running,
            message: "task scheduled in sandbox background runner".to_string(),
        })
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> AppResult<TaskRunReceipt> {
        let mut task = self
            .store
            .get_task(task_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("task {}", task_id)))?;
        self.executor.cancel_task(task_id).await?;
        task.set_status(TaskStatus::Cancelled);
        self.store.upsert_task(task).await?;
        Ok(TaskRunReceipt {
            task_id,
            status: TaskStatus::Cancelled,
            message: "cancellation signal delivered to sandbox runner".to_string(),
        })
    }

    pub async fn list_task_executions(&self, task_id: Uuid) -> AppResult<Vec<TaskExecutionRecord>> {
        self.store.list_task_executions(task_id).await
    }

    pub async fn list_sessions(&self) -> AppResult<Vec<AgentSession>> {
        self.store.list_sessions().await
    }

    pub async fn create_session(&self, input: CreateSessionRequest) -> AppResult<AgentSession> {
        let session = AgentSession::new(input);
        self.store.upsert_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn append_message(&self, session_id: Uuid, input: AppendMessageRequest) -> AppResult<AgentSession> {
        let mut session = self
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("session {}", session_id)))?;
        session.append_message(input, self.config.runtime.session_window_size);
        self.store.upsert_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn list_memories(&self) -> AppResult<Vec<MemoryEntry>> {
        self.store.list_memories().await
    }

    pub async fn create_memory(&self, input: CreateMemoryRequest) -> AppResult<MemoryEntry> {
        let memory = MemoryEntry::new(input);
        self.store.upsert_memory(memory.clone()).await?;
        Ok(memory)
    }

    pub async fn search_memories(&self, input: SearchMemoryRequest) -> AppResult<Vec<MemorySearchResult>> {
        self.store
            .search_memories(&input.query, self.config.runtime.memory_search_limit)
            .await
    }
}
