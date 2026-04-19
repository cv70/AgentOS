use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::config::AppConfig;
use crate::domain::agent::{
    HermesActionKind, HermesAgentRequest, HermesAgentResponse, HermesStrategyTrace,
    HermesSuggestedAction, HermesTaskStrategyTrace, HermesToolEvent,
};
use crate::domain::context::WorkspaceContextFile;
use crate::domain::learning::{
    LearningCluster, LearningSummary, SkillCandidate, StrategyEvaluationEvent, StrategyEventKind,
    StrategyTimeline, TaskLearningReport,
};
use crate::domain::memory::{
    CreateMemoryRequest, MemoryEntry, MemorySearchResult, SearchMemoryRequest,
};
use crate::domain::model::{
    ModelProvider, ModelRouteDecision, RouteModelRequest, SetDefaultModelRequest,
};
use crate::domain::session::{
    AgentSession, AppendMessageRequest, CreateSessionRequest, MessageRole, SearchSessionRequest,
    SessionSearchResult,
};
use crate::domain::task::{
    AgentTask, CreateTaskRequest, TaskCommand, TaskExecutionInsights, TaskExecutionRecord,
    TaskPriority, TaskRunReceipt, TaskStatus, UpdateTaskStatusRequest,
};
use crate::domain::tool::{
    PromoteSkillCandidateRequest, PromotedSkillResult, RunSkillRequest, SkillDescriptor,
    SkillExecutionResult, SkillScriptDescriptor, ToolDescriptor,
};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmChatMessage {
    role: String,
    content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<LlmToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: LlmFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct LlmChatResponse {
    choices: Vec<LlmChoice>,
}

#[derive(Debug, Deserialize)]
struct LlmChoice {
    message: LlmAssistantMessage,
}

#[derive(Debug, Deserialize)]
struct LlmAssistantMessage {
    content: Option<Value>,
    #[serde(default)]
    tool_calls: Vec<LlmToolCall>,
}

#[derive(Clone)]
pub struct AgentRuntime {
    config: Arc<AppConfig>,
    store: SqliteStore,
    executor: SandboxExecutor,
    llm_client: Client,
    default_model: Arc<RwLock<String>>,
    skill_roots: Arc<Vec<PathBuf>>,
}

impl AgentRuntime {
    pub async fn new(config: AppConfig) -> AppResult<Self> {
        let config = Arc::new(config);
        let store = SqliteStore::new(&config.storage).await?;
        let runtime = Self {
            executor: SandboxExecutor::new(config.sandbox.clone()),
            llm_client: Client::new(),
            default_model: Arc::new(RwLock::new(config.models.default_model.clone())),
            skill_roots: Arc::new(default_skill_roots()),
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
                strategy_sources: Vec::new(),
            }),
            AgentTask::new(CreateTaskRequest {
                title: "扫描 Skills".to_string(),
                description: "发现本地技能目录并刷新 Skill Registry。".to_string(),
                priority: crate::domain::task::TaskPriority::Normal,
                sandbox_profile: "read-only".to_string(),
                command: crate::domain::task::TaskCommand {
                    program: "sh".to_string(),
                    args: vec![
                        "-lc".to_string(),
                        "find /root/space -name SKILL.md | head -20".to_string(),
                    ],
                },
                working_dir: "/root/space".to_string(),
                strategy_sources: Vec::new(),
            }),
        ];

        let mut session = AgentSession::new(CreateSessionRequest {
            title: "默认工作会话".to_string(),
            working_dir: "/root/space".to_string(),
        });
        session.append_message(
            AppendMessageRequest {
                role: crate::domain::session::MessageRole::System,
                content: "AgentOS 已启动，优先使用本地资源，支持会话搜索、Skill 发现和模型路由。"
                    .to_string(),
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
                permissions: vec!["read".to_string(), "write".to_string(), "watch".to_string()],
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
                id: "session-search".to_string(),
                category: "memory".to_string(),
                display_name: "Session Search".to_string(),
                permissions: vec![
                    "fts".to_string(),
                    "excerpt".to_string(),
                    "history".to_string(),
                ],
                hot_reload: false,
            },
            ToolDescriptor {
                id: "skills".to_string(),
                category: "plugin".to_string(),
                display_name: "Skill Loader".to_string(),
                permissions: vec![
                    "discover".to_string(),
                    "reload".to_string(),
                    "metadata".to_string(),
                ],
                hot_reload: true,
            },
            ToolDescriptor {
                id: "model-router".to_string(),
                category: "llm".to_string(),
                display_name: "Model Router".to_string(),
                permissions: vec![
                    "route".to_string(),
                    "fallback".to_string(),
                    "set-default".to_string(),
                ],
                hot_reload: false,
            },
            ToolDescriptor {
                id: "hermes-loop".to_string(),
                category: "agent".to_string(),
                display_name: "Hermes-style Loop".to_string(),
                permissions: vec![
                    "session-recall".to_string(),
                    "memory-nudge".to_string(),
                    "skill-suggest".to_string(),
                    "task-draft".to_string(),
                ],
                hot_reload: false,
            },
        ]
    }

    pub fn skills(&self) -> Vec<SkillDescriptor> {
        let mut discovered = discover_skills(&self.skill_roots);
        if discovered.is_empty() {
            discovered = builtin_skills();
        }
        discovered
    }

    pub async fn models(&self) -> Vec<ModelProvider> {
        let default_model = self.default_model.read().await.clone();
        self.config
            .models
            .providers
            .iter()
            .map(|provider| ModelProvider {
                id: provider.id.clone(),
                kind: provider.kind.clone(),
                endpoint: provider.endpoint.clone(),
                capabilities: provider.capabilities.clone(),
                is_default: provider.id == default_model,
                routing_weight: model_weight(&provider.kind, &provider.capabilities),
            })
            .collect()
    }

    pub async fn set_default_model(
        &self,
        input: SetDefaultModelRequest,
    ) -> AppResult<Vec<ModelProvider>> {
        if !self
            .config
            .models
            .providers
            .iter()
            .any(|provider| provider.id == input.model_id)
        {
            return Err(AppError::NotFound(format!("model {}", input.model_id)));
        }
        *self.default_model.write().await = input.model_id;
        Ok(self.models().await)
    }

    pub async fn hermes_chat(&self, input: HermesAgentRequest) -> AppResult<HermesAgentResponse> {
        let message = input.message.trim();
        if message.is_empty() {
            return Err(AppError::Runtime("empty hermes message".to_string()));
        }

        let mut tool_trace = vec![HermesToolEvent {
            tool: "session".to_string(),
            detail: "loaded or created conversation session".to_string(),
        }];

        let mut session = if let Some(session_id) = input.session_id {
            self.store
                .get_session(session_id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("session {}", session_id)))?
        } else {
            let created = AgentSession::new(CreateSessionRequest {
                title: input.title.unwrap_or_else(|| infer_session_title(message)),
                working_dir: input
                    .working_dir
                    .unwrap_or_else(|| "/root/space".to_string()),
            });
            self.store.upsert_session(created.clone()).await?;
            created
        };

        session.append_message(
            AppendMessageRequest {
                role: MessageRole::User,
                content: message.to_string(),
            },
            self.config.runtime.session_window_size,
        );
        self.store.upsert_session(session.clone()).await?;

        let capability = infer_hermes_loop_capability(&self.config, message);
        let routed_model = self
            .route_model(RouteModelRequest {
                capability: capability.clone(),
                prefer_local: !looks_like_remote_request(message),
            })
            .await?;
        tool_trace.push(HermesToolEvent {
            tool: "model-router".to_string(),
            detail: format!("capability={} -> {}", capability, routed_model.selected.id),
        });

        let workspace_contexts = self
            .list_workspace_contexts(Some(session.working_dir.clone()))
            .await?;
        if !workspace_contexts.is_empty() {
            tool_trace.push(HermesToolEvent {
                tool: "workspace-context".to_string(),
                detail: format!("loaded {} context files", workspace_contexts.len()),
            });
        }
        let learning_summary = self
            .learning_summary(Some(session.working_dir.clone()))
            .await?;
        let strategic_clusters = learning_summary
            .clusters
            .into_iter()
            .take(3)
            .collect::<Vec<_>>();
        if !strategic_clusters.is_empty() {
            tool_trace.push(HermesToolEvent {
                tool: "learning-clusters".to_string(),
                detail: format!("loaded {} strategic clusters", strategic_clusters.len()),
            });
        }

        let mut memory_hits = Vec::new();
        let mut session_hits = Vec::new();
        let mut suggested_skills = Vec::new();
        let mut suggested_tasks = Vec::new();
        let mut memory_written = None;

        let assistant_message = match self
            .run_llm_hermes_loop(
                &session,
                message,
                &routed_model,
                &workspace_contexts,
                &strategic_clusters,
                input.auto_persist_memory,
                &mut tool_trace,
                &mut memory_hits,
                &mut session_hits,
                &mut suggested_skills,
                &mut suggested_tasks,
                &mut memory_written,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tool_trace.push(HermesToolEvent {
                    tool: "llm-fallback".to_string(),
                    detail: format!(
                        "LLM loop unavailable, falling back to heuristic planner: {error}"
                    ),
                });

                memory_hits = self
                    .search_memories(SearchMemoryRequest {
                        query: message.to_string(),
                    })
                    .await?
                    .into_iter()
                    .take(3)
                    .map(|item| item.memory)
                    .collect();
                if !memory_hits.is_empty() {
                    tool_trace.push(HermesToolEvent {
                        tool: "memory".to_string(),
                        detail: format!("recalled {} related memories", memory_hits.len()),
                    });
                }

                session_hits = self
                    .search_sessions(SearchSessionRequest {
                        query: message.to_string(),
                        limit: Some(3),
                    })
                    .await?;
                if !session_hits.is_empty() {
                    tool_trace.push(HermesToolEvent {
                        tool: "session-search".to_string(),
                        detail: format!("recalled {} prior session messages", session_hits.len()),
                    });
                }

                suggested_skills = rank_skills(&self.skills(), message, 3);
                if !suggested_skills.is_empty() {
                    tool_trace.push(HermesToolEvent {
                        tool: "skills".to_string(),
                        detail: format!("matched {} skills", suggested_skills.len()),
                    });
                }

                suggested_tasks =
                    build_task_suggestions(message, &session.working_dir, &strategic_clusters);
                if !suggested_tasks.is_empty() {
                    tool_trace.push(HermesToolEvent {
                        tool: "task-draft".to_string(),
                        detail: format!("drafted {} executable tasks", suggested_tasks.len()),
                    });
                }

                memory_written =
                    maybe_persist_memory(self, message, input.auto_persist_memory).await?;
                if let Some(memory) = &memory_written {
                    tool_trace.push(HermesToolEvent {
                        tool: "memory-write".to_string(),
                        detail: format!("persisted preference memory {}", memory.title),
                    });
                }

                compose_hermes_response(
                    message,
                    &routed_model,
                    &workspace_contexts,
                    &strategic_clusters,
                    &memory_hits,
                    &session_hits,
                    &suggested_skills,
                    &suggested_tasks,
                    memory_written.as_ref(),
                )
            }
        };

        if memory_hits.is_empty() {
            memory_hits = self
                .search_memories(SearchMemoryRequest {
                    query: message.to_string(),
                })
                .await?
                .into_iter()
                .take(3)
                .map(|item| item.memory)
                .collect();
        }
        if session_hits.is_empty() {
            session_hits = self
                .search_sessions(SearchSessionRequest {
                    query: message.to_string(),
                    limit: Some(3),
                })
                .await?;
        }
        if suggested_skills.is_empty() {
            suggested_skills = rank_skills(&self.skills(), message, 3);
        }
        if suggested_tasks.is_empty() {
            suggested_tasks =
                build_task_suggestions(message, &session.working_dir, &strategic_clusters);
        }
        if memory_written.is_none() {
            memory_written = maybe_persist_memory(self, message, input.auto_persist_memory).await?;
        }

        let actions = build_actions(
            &routed_model,
            &workspace_contexts,
            &strategic_clusters,
            &memory_hits,
            &session_hits,
            &suggested_skills,
            &suggested_tasks,
            memory_written.as_ref(),
        );
        let strategy_trace = build_strategy_trace(&strategic_clusters, &suggested_tasks);

        session.append_message(
            AppendMessageRequest {
                role: MessageRole::Assistant,
                content: assistant_message.clone(),
            },
            self.config.runtime.session_window_size,
        );
        self.store.upsert_session(session.clone()).await?;

        Ok(HermesAgentResponse {
            session,
            assistant_message,
            routed_model,
            workspace_contexts,
            strategic_clusters,
            strategy_trace,
            memory_hits,
            session_hits,
            suggested_skills,
            suggested_tasks,
            actions,
            tool_trace,
            memory_written,
        })
    }

    async fn run_llm_hermes_loop(
        &self,
        session: &AgentSession,
        message: &str,
        routed_model: &ModelRouteDecision,
        workspace_contexts: &[WorkspaceContextFile],
        strategic_clusters: &[LearningCluster],
        auto_persist_memory: bool,
        tool_trace: &mut Vec<HermesToolEvent>,
        memory_hits: &mut Vec<MemoryEntry>,
        session_hits: &mut Vec<SessionSearchResult>,
        suggested_skills: &mut Vec<SkillDescriptor>,
        suggested_tasks: &mut Vec<AgentTask>,
        memory_written: &mut Option<MemoryEntry>,
    ) -> AppResult<String> {
        let system_prompt = build_hermes_system_prompt(
            session,
            routed_model,
            workspace_contexts,
            strategic_clusters,
            auto_persist_memory || looks_like_memory_intent(message),
        );
        let mut messages = vec![
            LlmChatMessage {
                role: "system".to_string(),
                content: json!(system_prompt),
                tool_call_id: None,
                tool_calls: None,
            },
            LlmChatMessage {
                role: "user".to_string(),
                content: json!(message),
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        for _ in 0..4 {
            let response = self
                .llm_chat_completion(&routed_model.selected.id, &messages, &hermes_tool_schemas())
                .await?;
            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| AppError::Runtime("llm returned no choices".to_string()))?;

            let assistant_content = choice.message.content.unwrap_or(Value::Null);
            let assistant_text = llm_content_to_string(&assistant_content);
            let tool_calls = choice.message.tool_calls;

            messages.push(LlmChatMessage {
                role: "assistant".to_string(),
                content: assistant_content,
                tool_call_id: None,
                tool_calls: (!tool_calls.is_empty()).then_some(tool_calls.clone()),
            });

            if tool_calls.is_empty() {
                if assistant_text.trim().is_empty() {
                    return Err(AppError::Runtime(
                        "llm finished without tool calls or response text".to_string(),
                    ));
                }
                return Ok(assistant_text);
            }

            for tool_call in tool_calls {
                let tool_message = self
                    .execute_hermes_tool_call(
                        &tool_call,
                        session,
                        message,
                        tool_trace,
                        memory_hits,
                        session_hits,
                        suggested_skills,
                        suggested_tasks,
                        memory_written,
                    )
                    .await?;
                messages.push(tool_message);
            }
        }

        Err(AppError::Runtime(
            "llm tool loop exceeded maximum iterations".to_string(),
        ))
    }

    async fn llm_chat_completion(
        &self,
        provider_id: &str,
        messages: &[LlmChatMessage],
        tools: &[Value],
    ) -> AppResult<LlmChatResponse> {
        let provider = self
            .config
            .models
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| AppError::NotFound(format!("model provider {}", provider_id)))?;

        let endpoint = build_chat_completions_endpoint(&provider.endpoint);
        let model_name = provider
            .model_name
            .clone()
            .unwrap_or_else(|| provider.id.clone());
        let payload = json!({
            "model": model_name,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
            "temperature": 0.2
        });

        let mut request = self.llm_client.post(endpoint).json(&payload);
        if let Some(api_key_env) = &provider.api_key_env {
            if let Ok(value) = std::env::var(api_key_env) {
                if !value.trim().is_empty() {
                    request = request.bearer_auth(value);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|error| AppError::Runtime(format!("send llm request: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read llm error body".to_string());
            return Err(AppError::Runtime(format!(
                "llm request failed with status {}: {}",
                status, body
            )));
        }

        response
            .json::<LlmChatResponse>()
            .await
            .map_err(|error| AppError::Runtime(format!("decode llm response: {error}")))
    }

    async fn execute_hermes_tool_call(
        &self,
        tool_call: &LlmToolCall,
        session: &AgentSession,
        message: &str,
        tool_trace: &mut Vec<HermesToolEvent>,
        memory_hits: &mut Vec<MemoryEntry>,
        session_hits: &mut Vec<SessionSearchResult>,
        suggested_skills: &mut Vec<SkillDescriptor>,
        suggested_tasks: &mut Vec<AgentTask>,
        memory_written: &mut Option<MemoryEntry>,
    ) -> AppResult<LlmChatMessage> {
        let args: Value = if tool_call.function.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&tool_call.function.arguments)
                .map_err(|error| AppError::Runtime(format!("parse tool arguments: {error}")))?
        };

        let result = match tool_call.function.name.as_str() {
            "list_learning_clusters" => {
                let working_dir = args
                    .get("working_dir")
                    .and_then(Value::as_str)
                    .unwrap_or(&session.working_dir);
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
                let results = build_learning_summary(
                    &self.store.list_tasks().await?,
                    &self.store.list_all_task_learning_reports().await?,
                    Some(working_dir),
                )
                .clusters
                .into_iter()
                .take(limit)
                .collect::<Vec<_>>();
                tool_trace.push(HermesToolEvent {
                    tool: "learning-clusters".to_string(),
                    detail: format!("llm recalled {} strategic clusters", results.len()),
                });
                json!(results)
            }
            "read_context_files" => {
                let working_dir = args
                    .get("working_dir")
                    .and_then(Value::as_str)
                    .unwrap_or(&session.working_dir);
                let results = discover_workspace_contexts(working_dir);
                tool_trace.push(HermesToolEvent {
                    tool: "workspace-context".to_string(),
                    detail: format!("llm loaded {} context files", results.len()),
                });
                json!(results)
            }
            "search_memories" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or(message)
                    .to_string();
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
                let results = self
                    .search_memories(SearchMemoryRequest { query })
                    .await?
                    .into_iter()
                    .take(limit)
                    .collect::<Vec<_>>();
                *memory_hits = results.iter().map(|item| item.memory.clone()).collect();
                tool_trace.push(HermesToolEvent {
                    tool: "memory".to_string(),
                    detail: format!("llm recalled {} memories", memory_hits.len()),
                });
                json!(results)
            }
            "search_sessions" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or(message)
                    .to_string();
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
                let results = self
                    .search_sessions(SearchSessionRequest {
                        query,
                        limit: Some(limit),
                    })
                    .await?;
                *session_hits = results.clone();
                tool_trace.push(HermesToolEvent {
                    tool: "session-search".to_string(),
                    detail: format!("llm recalled {} session messages", session_hits.len()),
                });
                json!(results)
            }
            "list_skills" => {
                let query = args.get("query").and_then(Value::as_str).unwrap_or(message);
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3) as usize;
                let results = rank_skills(&self.skills(), query, limit);
                *suggested_skills = results.clone();
                tool_trace.push(HermesToolEvent {
                    tool: "skills".to_string(),
                    detail: format!("llm matched {} skills", suggested_skills.len()),
                });
                json!(results)
            }
            "draft_tasks" => {
                let goal = args.get("goal").and_then(Value::as_str).unwrap_or(message);
                let cluster_hints = build_learning_summary(
                    &self.store.list_tasks().await?,
                    &self.store.list_all_task_learning_reports().await?,
                    Some(&session.working_dir),
                )
                .clusters
                .into_iter()
                .take(3)
                .collect::<Vec<_>>();
                let results = build_task_suggestions(goal, &session.working_dir, &cluster_hints);
                *suggested_tasks = results.clone();
                tool_trace.push(HermesToolEvent {
                    tool: "task-draft".to_string(),
                    detail: format!("llm drafted {} tasks", suggested_tasks.len()),
                });
                json!(results)
            }
            "write_memory" => {
                let title = args
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("Hermes preference capture");
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(message);
                let tags = args
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| vec!["hermes".to_string(), infer_capability(content)]);
                let memory = MemoryEntry::new(CreateMemoryRequest {
                    scope: crate::domain::memory::MemoryScope::LongTerm,
                    title: title.to_string(),
                    content: content.to_string(),
                    tags,
                });
                self.store.upsert_memory(memory.clone()).await?;
                *memory_written = Some(memory.clone());
                tool_trace.push(HermesToolEvent {
                    tool: "memory-write".to_string(),
                    detail: format!("llm persisted memory {}", memory.title),
                });
                json!(memory)
            }
            other => {
                return Err(AppError::Runtime(format!(
                    "unsupported hermes tool call: {}",
                    other
                )));
            }
        };

        Ok(LlmChatMessage {
            role: "tool".to_string(),
            content: json!(result.to_string()),
            tool_call_id: Some(tool_call.id.clone()),
            tool_calls: None,
        })
    }

    pub async fn route_model(&self, input: RouteModelRequest) -> AppResult<ModelRouteDecision> {
        let capability = input.capability.trim().to_lowercase();
        let mut ranked = self.models().await;
        ranked.retain(|model| {
            model
                .capabilities
                .iter()
                .any(|item| item.eq_ignore_ascii_case(&capability))
        });
        if ranked.is_empty() {
            return Err(AppError::NotFound(format!(
                "model capability {}",
                input.capability
            )));
        }

        ranked.sort_by(|left, right| {
            let left_local = left.kind == "local";
            let right_local = right.kind == "local";
            right_local
                .cmp(&left_local)
                .then(right.is_default.cmp(&left.is_default))
                .then(right.routing_weight.cmp(&left.routing_weight))
        });

        if !input.prefer_local {
            ranked.sort_by(|left, right| {
                right
                    .is_default
                    .cmp(&left.is_default)
                    .then(right.routing_weight.cmp(&left.routing_weight))
                    .then((right.kind == "local").cmp(&(left.kind == "local")))
            });
        }

        let selected = ranked.remove(0);
        let reason = if input.prefer_local && selected.kind == "local" {
            format!(
                "capability={} 命中本地优先策略，优先选择可本地运行的模型",
                capability
            )
        } else if selected.is_default {
            format!("capability={} 命中默认模型并满足能力要求", capability)
        } else {
            format!(
                "capability={} 根据能力、默认权重和回退顺序选择最优模型",
                capability
            )
        };

        Ok(ModelRouteDecision {
            selected,
            fallbacks: ranked,
            reason,
        })
    }

    pub async fn overview(&self) -> AppResult<RuntimeOverview> {
        let tasks = self.store.list_tasks().await?;
        let sessions = self.store.list_sessions().await?;
        let memories = self.store.list_memories().await?;
        let skills = self.skills();
        let running = tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Running)
            .count();
        let paused = tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Paused)
            .count();

        Ok(RuntimeOverview {
            node_name: "agentos-local-node".to_string(),
            scheduler: SchedulerSnapshot {
                max_concurrent_tasks: self.config.runtime.max_concurrent_tasks,
                queue_depth: tasks
                    .iter()
                    .filter(|task| task.status == TaskStatus::Pending)
                    .count(),
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
                skills: skills.len(),
                hot_reload_enabled: true,
            },
            models: self.models().await,
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

    pub async fn update_task_status(
        &self,
        task_id: Uuid,
        input: UpdateTaskStatusRequest,
    ) -> AppResult<AgentTask> {
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
            return Err(AppError::Runtime(format!(
                "task {} is already running",
                task_id
            )));
        }
        self.executor.validate_task(&task)?;
        let cancel_rx = self.executor.register_run(task_id).await?;
        task.set_status(TaskStatus::Running);
        self.store.upsert_task(task.clone()).await?;

        let store = self.store.clone();
        let executor = self.executor.clone();
        let session_window_size = self.config.runtime.session_window_size;
        tokio::spawn(async move {
            let mut execution = match executor.run_task(&task, cancel_rx).await {
                Ok(record) => record,
                Err(error) => TaskExecutionRecord {
                    id: Uuid::new_v4(),
                    task_id: task.id,
                    sandbox_profile: task.sandbox_profile.clone(),
                    command_line: format!(
                        "{} {}",
                        task.command.program,
                        task.command.args.join(" ")
                    ),
                    status: crate::domain::task::ExecutionStatus::Failed,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    duration_ms: 0,
                    started_at: Utc::now(),
                    finished_at: Utc::now(),
                    working_dir: task.working_dir.clone(),
                    audit_log: vec!["sandbox execution failed before child completion".to_string()],
                },
            };
            let mut finished_task = task.clone();
            apply_execution_result(&mut finished_task, &execution);
            if matches!(
                execution.status,
                crate::domain::task::ExecutionStatus::Failed
            ) && execution.audit_log.is_empty()
            {
                execution
                    .audit_log
                    .push("execution failed without audit trail".to_string());
            }
            let _ = store.upsert_task(finished_task).await;
            let _ = store.add_task_execution(execution.clone()).await;
            let _ = record_task_learning(&store, &task, &execution, session_window_size).await;
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

    pub async fn task_execution_insights(&self, task_id: Uuid) -> AppResult<TaskExecutionInsights> {
        Ok(TaskExecutionInsights {
            task_id,
            executions: self.store.list_task_executions(task_id).await?,
            learning_reports: self.store.list_task_learning_reports(task_id).await?,
        })
    }

    pub async fn list_sessions(&self) -> AppResult<Vec<AgentSession>> {
        self.store.list_sessions().await
    }

    pub async fn create_session(&self, input: CreateSessionRequest) -> AppResult<AgentSession> {
        let session = AgentSession::new(input);
        self.store.upsert_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn append_message(
        &self,
        session_id: Uuid,
        input: AppendMessageRequest,
    ) -> AppResult<AgentSession> {
        let mut session = self
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("session {}", session_id)))?;
        session.append_message(input, self.config.runtime.session_window_size);
        self.store.upsert_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn search_sessions(
        &self,
        input: SearchSessionRequest,
    ) -> AppResult<Vec<SessionSearchResult>> {
        self.store
            .search_sessions(&input.query, input.limit.unwrap_or(8))
            .await
    }

    pub async fn list_memories(&self) -> AppResult<Vec<MemoryEntry>> {
        self.store.list_memories().await
    }

    pub async fn create_memory(&self, input: CreateMemoryRequest) -> AppResult<MemoryEntry> {
        let memory = MemoryEntry::new(input);
        self.store.upsert_memory(memory.clone()).await?;
        Ok(memory)
    }

    pub async fn search_memories(
        &self,
        input: SearchMemoryRequest,
    ) -> AppResult<Vec<MemorySearchResult>> {
        self.store
            .search_memories(&input.query, self.config.runtime.memory_search_limit)
            .await
    }

    pub async fn run_skill(
        &self,
        skill_id: &str,
        input: RunSkillRequest,
    ) -> AppResult<SkillExecutionResult> {
        let skill = self
            .skills()
            .into_iter()
            .find(|item| item.id == skill_id)
            .ok_or_else(|| AppError::NotFound(format!("skill {}", skill_id)))?;
        let selected_script = resolve_skill_script(&skill, input.script_name.as_deref())?;
        let command = build_skill_command(&selected_script, &input.args)?;

        let mut task = AgentTask::new(CreateTaskRequest {
            title: format!("skill:{}:{}", skill.id, selected_script.name),
            description: format!("run skill {} via {}", skill.id, selected_script.runner),
            priority: crate::domain::task::TaskPriority::Normal,
            sandbox_profile: input
                .sandbox_profile
                .unwrap_or_else(|| "workspace-write".to_string()),
            command,
            working_dir: input
                .working_dir
                .unwrap_or_else(|| "/root/space".to_string()),
            strategy_sources: Vec::new(),
        });

        self.executor.validate_task(&task)?;
        task.set_status(TaskStatus::Running);
        self.store.upsert_task(task.clone()).await?;

        let cancel_rx = self.executor.register_run(task.id).await?;
        let execution_result = self.executor.run_task(&task, cancel_rx).await;
        self.executor.finish_run(task.id).await;
        let execution = execution_result?;

        let mut finished_task = task.clone();
        apply_execution_result(&mut finished_task, &execution);
        self.store.upsert_task(finished_task.clone()).await?;
        self.store.add_task_execution(execution.clone()).await?;
        record_task_learning(
            &self.store,
            &task,
            &execution,
            self.config.runtime.session_window_size,
        )
        .await?;

        Ok(SkillExecutionResult {
            skill,
            selected_script,
            task: finished_task,
            execution,
        })
    }

    pub async fn promote_skill_candidate(
        &self,
        input: PromoteSkillCandidateRequest,
    ) -> AppResult<PromotedSkillResult> {
        let tasks = self.store.list_tasks().await?;
        let reports = match input.task_id {
            Some(task_id) => self.store.list_task_learning_reports(task_id).await?,
            None => self.store.list_all_task_learning_reports().await?,
        };

        let (candidate, source_task, cluster_context) = if let Some(task_id) = input.task_id {
            let task = tasks
                .iter()
                .find(|item| item.id == task_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("task {}", task_id)))?;
            let report = reports
                .iter()
                .find(|item| {
                    item.skill_candidates
                        .iter()
                        .any(|candidate| candidate.id == input.candidate_id)
                })
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "skill candidate {} for task {}",
                        input.candidate_id, task_id
                    ))
                })?;
            let candidate = report
                .skill_candidates
                .iter()
                .find(|candidate| candidate.id == input.candidate_id)
                .cloned()
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "skill candidate {} for task {}",
                        input.candidate_id, task_id
                    ))
                })?;
            (candidate, task, None)
        } else {
            let summary = self.learning_summary(input.working_dir.clone()).await?;
            let cluster_key = input.cluster_key.clone().ok_or_else(|| {
                AppError::Runtime("cluster_key is required when task_id is omitted".to_string())
            })?;
            let cluster = summary
                .clusters
                .into_iter()
                .find(|item| item.key == cluster_key)
                .ok_or_else(|| AppError::NotFound(format!("learning cluster {}", cluster_key)))?;
            let candidate = cluster
                .strategic_skill_candidates
                .iter()
                .find(|candidate| candidate.id == input.candidate_id)
                .cloned()
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "strategic skill candidate {} for cluster {}",
                        input.candidate_id, cluster_key
                    ))
                })?;
            let task = select_representative_task(&tasks, &reports, &cluster.key)?;
            (candidate, task, Some(cluster))
        };

        let working_dir = input
            .working_dir
            .unwrap_or_else(|| source_task.working_dir.clone());
        let generated_root = PathBuf::from(&working_dir).join(".agentos/skills");
        fs::create_dir_all(&generated_root)
            .map_err(|error| AppError::Runtime(format!("create skill root: {error}")))?;
        let skill_dir = unique_skill_dir(&generated_root, &candidate.id)?;
        fs::create_dir_all(skill_dir.join("scripts"))
            .map_err(|error| AppError::Runtime(format!("create skill scripts dir: {error}")))?;

        let skill_md_path = skill_dir.join("SKILL.md");
        let script_path = skill_dir.join("scripts").join("run.sh");
        let skill_md =
            render_generated_skill_markdown(&candidate, &source_task, cluster_context.as_ref());
        let script = render_generated_skill_script(&source_task);

        fs::write(&skill_md_path, skill_md)
            .map_err(|error| AppError::Runtime(format!("write SKILL.md: {error}")))?;
        fs::write(&script_path, script)
            .map_err(|error| AppError::Runtime(format!("write skill script: {error}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path)
                .map_err(|error| AppError::Runtime(format!("stat skill script: {error}")))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms)
                .map_err(|error| AppError::Runtime(format!("chmod skill script: {error}")))?;
        }

        let raw = fs::read_to_string(&skill_md_path)
            .map_err(|error| AppError::Runtime(format!("read generated SKILL.md: {error}")))?;
        let skill = parse_skill_descriptor(&generated_root, &skill_md_path, &raw);
        Ok(PromotedSkillResult {
            skill,
            files: vec![
                skill_md_path.display().to_string(),
                script_path.display().to_string(),
            ],
            source_task,
        })
    }

    pub async fn learning_summary(
        &self,
        working_dir: Option<String>,
    ) -> AppResult<LearningSummary> {
        let tasks = self.store.list_tasks().await?;
        let reports = self.store.list_all_task_learning_reports().await?;
        Ok(build_learning_summary(
            &tasks,
            &reports,
            working_dir.as_deref(),
        ))
    }

    pub async fn strategy_timeline(
        &self,
        working_dir: Option<String>,
        limit: Option<usize>,
    ) -> AppResult<StrategyTimeline> {
        let tasks = self.store.list_tasks().await?;
        let mut events = self
            .store
            .list_strategy_evaluation_events(limit.unwrap_or(24))
            .await?;
        if let Some(dir) = working_dir {
            events.retain(|event| {
                tasks
                    .iter()
                    .find(|task| task.id == event.task_id)
                    .map(|task| task.working_dir == dir)
                    .unwrap_or(false)
            });
        }
        Ok(StrategyTimeline {
            generated_at: Utc::now(),
            total_events: events.len(),
            events,
        })
    }

    pub async fn list_workspace_contexts(
        &self,
        working_dir: Option<String>,
    ) -> AppResult<Vec<WorkspaceContextFile>> {
        let cwd = working_dir.unwrap_or_else(|| "/root/space".to_string());
        Ok(discover_workspace_contexts(&cwd))
    }
}

fn build_chat_completions_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn unique_skill_dir(root: &Path, base_id: &str) -> AppResult<PathBuf> {
    let cleaned = slugify(base_id);
    let first = root.join(&cleaned);
    if !first.exists() {
        return Ok(first);
    }

    for idx in 2..100 {
        let candidate = root.join(format!("{cleaned}-{idx}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(AppError::Runtime(format!(
        "unable to allocate unique skill directory for {}",
        base_id
    )))
}

fn render_generated_skill_markdown(
    candidate: &SkillCandidate,
    task: &AgentTask,
    cluster: Option<&LearningCluster>,
) -> String {
    let cluster_section = cluster
        .map(|item| {
            format!(
                "\n## Strategic Cluster\n\n- key: {}\n- capability: {}\n- report_count: {}\n- success_rate: {:.2}\n- source_success_rate: {:.2}\n- common_lessons: {}\n",
                item.key,
                item.capability,
                item.report_count,
                item.success_rate,
                item.source_success_rate,
                item.common_lessons.join(" | "),
            )
        })
        .unwrap_or_default();
    format!(
        "# {title}\n\nname: {id}\ndescription: {description}\ntrigger: {trigger}\n\n## Purpose\n\n{rationale}\n{cluster_section}\n## Source Task\n\n- title: {task_title}\n- description: {task_description}\n- working_dir: {working_dir}\n- command: {command}\n\n## Usage\n\nRun the generated `scripts/run.sh` to replay the successful task flow inside AgentOS.\n",
        title = candidate.title,
        id = candidate.id,
        description = candidate.description,
        trigger = candidate.suggested_trigger,
        rationale = candidate.rationale,
        cluster_section = cluster_section,
        task_title = task.title,
        task_description = task.description,
        working_dir = task.working_dir,
        command = format!("{} {}", task.command.program, task.command.args.join(" ")),
    )
}

fn render_generated_skill_script(task: &AgentTask) -> String {
    let command = render_shell_command(task);
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\ncd {working_dir}\n{command}\n",
        working_dir = shell_quote(&task.working_dir),
        command = command
    )
}

fn render_shell_command(task: &AgentTask) -> String {
    if matches!(task.command.program.as_str(), "sh" | "bash")
        && task.command.args.len() >= 2
        && matches!(task.command.args[0].as_str(), "-lc" | "-c")
    {
        task.command.args[1].clone()
    } else {
        std::iter::once(shell_quote(&task.command.program))
            .chain(task.command.args.iter().map(|arg| shell_quote(arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn shell_quote(raw: &str) -> String {
    if raw.is_empty() {
        return "''".to_string();
    }
    if raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.' | ':'))
    {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

async fn record_task_learning(
    store: &SqliteStore,
    task: &AgentTask,
    execution: &TaskExecutionRecord,
    session_window_size: usize,
) -> AppResult<TaskLearningReport> {
    let recap = build_task_recap(task, execution);
    let lessons = derive_task_lessons(task, execution);

    let memory = MemoryEntry::new(CreateMemoryRequest {
        scope: crate::domain::memory::MemoryScope::Episodic,
        title: format!("Task learning · {}", task.title),
        content: recap.clone(),
        tags: vec![
            "task-learning".to_string(),
            format!("status:{}", execution_status_label(&execution.status)),
            infer_capability(&format!("{} {}", task.title, task.description)),
        ],
    });
    store.upsert_memory(memory.clone()).await?;

    let mut recap_session = ensure_learning_session(store, &task.working_dir).await?;
    recap_session.append_message(
        AppendMessageRequest {
            role: MessageRole::Assistant,
            content: recap.clone(),
        },
        session_window_size,
    );
    store.upsert_session(recap_session.clone()).await?;

    let report = TaskLearningReport {
        id: Uuid::new_v4(),
        task_id: task.id,
        execution_id: execution.id,
        status: execution_status_label(&execution.status).to_string(),
        source_strategy_keys: task.strategy_sources.clone(),
        recap,
        lessons,
        memory_ids: vec![memory.id],
        session_id: Some(recap_session.id),
        skill_candidates: derive_skill_candidates(task, execution),
        created_at: Utc::now(),
    };
    store.add_task_learning_report(report.clone()).await?;
    for strategy_source_key in &task.strategy_sources {
        let event = StrategyEvaluationEvent {
            id: Uuid::new_v4(),
            task_id: task.id,
            execution_id: execution.id,
            strategy_source_key: strategy_source_key.clone(),
            event_kind: StrategyEventKind::TaskExecuted,
            outcome_status: execution_status_label(&execution.status).to_string(),
            summary: format!(
                "Strategy `{}` was exercised by task `{}`",
                strategy_source_key, task.title
            ),
            evidence: compact_execution_evidence(execution),
            created_at: Utc::now(),
        };
        store.add_strategy_evaluation_event(event).await?;
    }
    Ok(report)
}

async fn ensure_learning_session(
    store: &SqliteStore,
    working_dir: &str,
) -> AppResult<AgentSession> {
    if let Some(existing) = store
        .list_sessions()
        .await?
        .into_iter()
        .find(|session| session.title == "Hermes Autopilot Recap")
    {
        return Ok(existing);
    }

    let session = AgentSession::new(CreateSessionRequest {
        title: "Hermes Autopilot Recap".to_string(),
        working_dir: working_dir.to_string(),
    });
    store.upsert_session(session.clone()).await?;
    Ok(session)
}

fn build_task_recap(task: &AgentTask, execution: &TaskExecutionRecord) -> String {
    let mut parts = vec![
        format!(
            "任务 `{}` 以 `{}` 结束，耗时 {} ms。",
            task.title,
            execution_status_label(&execution.status),
            execution.duration_ms
        ),
        format!("命令：{}", execution.command_line),
    ];

    if let Some(stdout) = extract_signal_text(&execution.stdout) {
        parts.push(format!("stdout 摘要：{}", stdout));
    }
    if let Some(stderr) = extract_signal_text(&execution.stderr) {
        parts.push(format!("stderr 摘要：{}", stderr));
    }
    if !execution.audit_log.is_empty() {
        parts.push(format!(
            "审计线索：{}",
            execution
                .audit_log
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }

    parts.join(" ")
}

fn derive_task_lessons(task: &AgentTask, execution: &TaskExecutionRecord) -> Vec<String> {
    let mut lessons = Vec::new();
    match execution.status {
        crate::domain::task::ExecutionStatus::Succeeded => {
            lessons.push("该任务的当前命令链路可复用，适合沉淀为固定流程。".to_string());
        }
        crate::domain::task::ExecutionStatus::Failed => {
            lessons.push("任务失败，后续应优先检查 stderr 与 sandbox allow-list。".to_string());
        }
        crate::domain::task::ExecutionStatus::TimedOut => {
            lessons.push("任务超时，后续应拆分步骤或提升资源/缩小扫描范围。".to_string());
        }
        crate::domain::task::ExecutionStatus::Cancelled => {
            lessons.push("任务被取消，说明需要更强的中断恢复与阶段性输出。".to_string());
        }
    }

    if task.command.program == "sh" || task.command.program == "bash" {
        lessons.push("Shell 命令适合作为 Skill 脚本候选入口。".to_string());
    }
    if task.title.to_lowercase().contains("scan") || task.description.contains("扫描") {
        lessons.push("扫描类任务适合做成可重复运行的 workspace indexing skill。".to_string());
    }

    lessons.truncate(4);
    lessons
}

fn derive_skill_candidates(
    task: &AgentTask,
    execution: &TaskExecutionRecord,
) -> Vec<SkillCandidate> {
    let mut candidates = Vec::new();
    if matches!(
        execution.status,
        crate::domain::task::ExecutionStatus::Succeeded
    ) {
        candidates.push(SkillCandidate {
            id: format!("skill-{}", slugify(&task.title)),
            title: format!("Promote {}", task.title),
            description: "将当前成功任务提升为可重复调用的 Skill。".to_string(),
            rationale: format!(
                "任务成功执行，命令 `{}` 已有可复用价值。",
                execution.command_line
            ),
            suggested_trigger: format!("run / automate / {}", slugify(&task.title)),
        });
    }

    if task.description.contains("扫描") || task.title.to_lowercase().contains("scan") {
        candidates.push(SkillCandidate {
            id: format!("workspace-index-{}", slugify(&task.title)),
            title: "Workspace Index Refresh".to_string(),
            description: "将扫描与索引动作收敛为固定 Skill。".to_string(),
            rationale: "重复扫描类任务是典型的可沉淀能力。".to_string(),
            suggested_trigger: "scan / index / workspace".to_string(),
        });
    }

    candidates.truncate(3);
    candidates
}

fn build_learning_summary(
    tasks: &[AgentTask],
    reports: &[TaskLearningReport],
    working_dir: Option<&str>,
) -> LearningSummary {
    use std::collections::BTreeMap;

    let filtered_reports = reports
        .iter()
        .filter(|report| {
            if let Some(dir) = working_dir {
                tasks
                    .iter()
                    .find(|task| task.id == report.task_id)
                    .map(|task| task.working_dir == dir)
                    .unwrap_or(false)
            } else {
                true
            }
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut groups: BTreeMap<String, Vec<TaskLearningReport>> = BTreeMap::new();
    for report in &filtered_reports {
        if let Some(task) = tasks.iter().find(|task| task.id == report.task_id) {
            groups
                .entry(learning_cluster_key(task))
                .or_default()
                .push(report.clone());
        }
    }

    let mut clusters = groups
        .into_iter()
        .filter_map(|(key, items)| summarize_learning_cluster(&key, &items, tasks))
        .filter(|cluster| !cluster.pruned_from_planning || cluster.recency_score >= 0.85)
        .collect::<Vec<_>>();
    clusters.sort_by(|left, right| {
        right
            .strategic_weight
            .partial_cmp(&left.strategic_weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.report_count.cmp(&left.report_count))
            .then(left.title.cmp(&right.title))
    });

    LearningSummary {
        generated_at: Utc::now(),
        total_reports: filtered_reports.len(),
        clusters,
    }
}

fn summarize_learning_cluster(
    key: &str,
    reports: &[TaskLearningReport],
    tasks: &[AgentTask],
) -> Option<LearningCluster> {
    if reports.is_empty() {
        return None;
    }

    let capability = key.split("::").next().unwrap_or("chat").to_string();
    let related_tasks = reports
        .iter()
        .filter_map(|report| tasks.iter().find(|task| task.id == report.task_id))
        .cloned()
        .collect::<Vec<_>>();
    let success_count = reports
        .iter()
        .filter(|report| report.status == "succeeded")
        .count();
    let success_rate = success_count as f32 / reports.len() as f32;
    let source_reports = reports
        .iter()
        .filter(|report| {
            report
                .source_strategy_keys
                .iter()
                .any(|source| strategy_key_matches_cluster(source, key))
        })
        .collect::<Vec<_>>();
    let source_usage_count = source_reports.len();
    let source_success_rate = if source_usage_count == 0 {
        0.0
    } else {
        source_reports
            .iter()
            .filter(|report| report.status == "succeeded")
            .count() as f32
            / source_usage_count as f32
    };
    let recency_score = compute_recency_score(reports);
    let base_weight = (reports.len() as f32 * 0.45)
        + (success_rate * 25.0)
        + (source_success_rate * 20.0)
        + (recency_score * 20.0);
    let (suppression_level, pruned_from_planning, suppression_penalty) =
        cluster_suppression_policy(source_usage_count, source_success_rate, recency_score);
    let strategic_weight = (base_weight - suppression_penalty).max(0.0);
    let common_lessons = top_strings(
        reports
            .iter()
            .flat_map(|report| report.lessons.iter().cloned())
            .collect::<Vec<_>>(),
        4,
    );
    let example_tasks = top_strings(
        related_tasks
            .iter()
            .map(|task| task.title.clone())
            .collect::<Vec<_>>(),
        4,
    );
    let recommended_commands = top_strings(
        related_tasks
            .iter()
            .map(render_shell_command)
            .collect::<Vec<_>>(),
        3,
    );
    let title = cluster_title_from_key(key, &example_tasks);
    let strategic_skill_candidates = derive_cluster_skill_candidates(
        key,
        &title,
        &capability,
        reports.len(),
        success_rate,
        recency_score,
        &common_lessons,
    );

    Some(LearningCluster {
        key: key.to_string(),
        title,
        capability,
        report_count: reports.len(),
        source_usage_count,
        source_success_rate,
        success_rate,
        recency_score,
        strategic_weight,
        suppression_level,
        pruned_from_planning,
        common_lessons,
        example_tasks,
        recommended_commands,
        strategic_skill_candidates,
    })
}

fn derive_cluster_skill_candidates(
    key: &str,
    title: &str,
    capability: &str,
    report_count: usize,
    success_rate: f32,
    recency_score: f32,
    common_lessons: &[String],
) -> Vec<SkillCandidate> {
    if report_count < 2 {
        return Vec::new();
    }

    vec![SkillCandidate {
        id: format!("cluster-{}", slugify(key)),
        title: format!("Strategic {}", title),
        description: "将多次任务经验沉淀为长期复用 Skill。".to_string(),
        rationale: format!(
            "该经验簇已累积 {} 次执行，成功率 {:.0}%，时效分 {:.2}，高频经验包括：{}",
            report_count,
            success_rate * 100.0,
            recency_score,
            common_lessons.join(" | ")
        ),
        suggested_trigger: format!("strategy / {} / {}", capability, slugify(title)),
    }]
}

fn learning_cluster_key(task: &AgentTask) -> String {
    let capability = infer_capability(&format!("{} {}", task.title, task.description));
    let lower = format!(
        "{} {}",
        task.title.to_lowercase(),
        task.description.to_lowercase()
    );
    let pattern = if contains_any(&lower, &["scan", "扫描", "index", "索引"]) {
        "scan-index".to_string()
    } else if contains_any(&lower, &["test", "验证", "check", "audit"]) {
        "validate-audit".to_string()
    } else if contains_any(&lower, &["skill", "技能", "plugin", "插件"]) {
        "skill-ops".to_string()
    } else if contains_any(&lower, &["review", "审查", "bug", "fix", "修复"]) {
        "review-fix".to_string()
    } else {
        slugify(&task.title)
    };
    format!("{capability}::{pattern}")
}

fn cluster_title_from_key(key: &str, examples: &[String]) -> String {
    if let Some(example) = examples.first() {
        example.clone()
    } else {
        key.replace("::", " · ")
    }
}

fn top_strings(items: Vec<String>, limit: usize) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut counts = BTreeMap::<String, usize>::new();
    for item in items {
        *counts.entry(item).or_default() += 1;
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(item, _)| item)
        .collect()
}

fn cluster_suppression_policy(
    source_usage_count: usize,
    source_success_rate: f32,
    recency_score: f32,
) -> (String, bool, f32) {
    if source_usage_count >= 3 && source_success_rate < 0.25 && recency_score < 0.6 {
        return ("pruned".to_string(), true, 30.0);
    }
    if source_usage_count >= 2 && source_success_rate < 0.5 {
        return ("degraded".to_string(), false, 12.0);
    }
    if source_usage_count == 0 {
        return ("cold".to_string(), false, 0.0);
    }
    ("healthy".to_string(), false, 0.0)
}

fn strategy_key_matches_cluster(source_key: &str, cluster_key: &str) -> bool {
    if source_key == cluster_key {
        return true;
    }
    let source_suffix = source_key.split("::").nth(1).unwrap_or(source_key);
    let cluster_suffix = cluster_key.split("::").nth(1).unwrap_or(cluster_key);
    source_suffix == cluster_suffix
}

fn compute_recency_score(reports: &[TaskLearningReport]) -> f32 {
    let now = Utc::now();
    let mut weighted = 0.0_f32;
    for report in reports {
        let age_hours = (now - report.created_at).num_hours().max(0) as f32;
        let decay = (-age_hours / 72.0).exp();
        weighted += decay;
    }
    (weighted / reports.len() as f32).clamp(0.0, 1.0)
}

fn select_representative_task(
    tasks: &[AgentTask],
    reports: &[TaskLearningReport],
    cluster_key: &str,
) -> AppResult<AgentTask> {
    reports
        .iter()
        .find_map(|report| {
            tasks
                .iter()
                .find(|task| task.id == report.task_id && learning_cluster_key(task) == cluster_key)
        })
        .cloned()
        .ok_or_else(|| {
            AppError::NotFound(format!("representative task for cluster {}", cluster_key))
        })
}

fn extract_signal_text(raw: &str) -> Option<String> {
    let compact = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    if compact.is_empty() {
        None
    } else {
        Some(truncate_chars(&compact, 220))
    }
}

fn compact_execution_evidence(execution: &TaskExecutionRecord) -> String {
    let mut parts = vec![format!(
        "status={}",
        execution_status_label(&execution.status)
    )];
    if let Some(stdout) = extract_signal_text(&execution.stdout) {
        parts.push(format!("stdout={}", stdout));
    }
    if let Some(stderr) = extract_signal_text(&execution.stderr) {
        parts.push(format!("stderr={}", stderr));
    }
    if let Some(exit_code) = execution.exit_code {
        parts.push(format!("exit={}", exit_code));
    }
    truncate_chars(&parts.join(" | "), 260)
}

fn truncate_chars(input: &str, limit: usize) -> String {
    let mut output = input.chars().take(limit).collect::<String>();
    if input.chars().count() > limit {
        output.push_str("...");
    }
    output
}

fn execution_status_label(status: &crate::domain::task::ExecutionStatus) -> &'static str {
    match status {
        crate::domain::task::ExecutionStatus::Succeeded => "succeeded",
        crate::domain::task::ExecutionStatus::Failed => "failed",
        crate::domain::task::ExecutionStatus::TimedOut => "timed_out",
        crate::domain::task::ExecutionStatus::Cancelled => "cancelled",
    }
}

fn slugify(input: &str) -> String {
    let slug = input
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    slug.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn llm_content_to_string(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| item.as_str().map(ToString::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn build_hermes_system_prompt(
    session: &AgentSession,
    routed_model: &ModelRouteDecision,
    workspace_contexts: &[WorkspaceContextFile],
    strategic_clusters: &[LearningCluster],
    should_persist_memory: bool,
) -> String {
    let context_summary = if workspace_contexts.is_empty() {
        "No workspace context files detected.".to_string()
    } else {
        workspace_contexts
            .iter()
            .take(4)
            .map(|item| {
                format!(
                    "{} [{}] => {}",
                    item.title,
                    item.kind,
                    item.guidance.join(" | ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let cluster_summary = if strategic_clusters.is_empty() {
        "No strategic learning clusters detected.".to_string()
    } else {
        strategic_clusters
            .iter()
            .take(3)
            .map(|item| {
                format!(
                    "{} [{} reports / {:.0}% success / {:.0}% source-success / {}] => {}",
                    item.title,
                    item.report_count,
                    item.success_rate * 100.0,
                    item.source_success_rate * 100.0,
                    item.suppression_level,
                    item.common_lessons.join(" | ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "You are Hermes Replica running inside AgentOS.\n\
Use tools when useful before answering. Prefer grounded responses based on recalled session history, memories, skills, and executable task drafts.\n\
Current working directory: {}.\n\
Selected model route: {}.\n\
Session title: {}.\n\
Workspace context hints:\n{}.\n\
Strategic learning hints:\n{}.\n\
If the user states a stable preference or asks to remember something, call write_memory{}.\n\
Reply in Chinese unless the user clearly asks otherwise.\n\
Keep the final answer concise but actionable, with sections for recalled context, strategic experience, recommended skills, and next executable tasks.",
        session.working_dir,
        routed_model.selected.id,
        session.title,
        context_summary,
        cluster_summary,
        if should_persist_memory {
            " and persist it"
        } else {
            " when truly justified"
        }
    )
}

fn hermes_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "list_learning_clusters",
                "description": "List strategic experience clusters distilled from prior task learning reports.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "working_dir": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5 }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_context_files",
                "description": "Load Codex-style AGENTS.md or Hermes-style workspace context files from the working directory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "working_dir": { "type": "string" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_memories",
                "description": "Search persistent memories relevant to the user's request.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5 }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_sessions",
                "description": "Search prior session messages relevant to the current task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5 }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_skills",
                "description": "Find the most relevant local skills for the request.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5 }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "draft_tasks",
                "description": "Draft executable AgentOS tasks for the user's goal.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string" }
                    },
                    "required": ["goal"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_memory",
                "description": "Persist a stable user preference or durable project fact into long-term memory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "content": { "type": "string" },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["content"]
                }
            }
        }),
    ]
}

async fn maybe_persist_memory(
    runtime: &AgentRuntime,
    message: &str,
    auto_persist_memory: bool,
) -> AppResult<Option<MemoryEntry>> {
    if !auto_persist_memory && !looks_like_memory_intent(message) {
        return Ok(None);
    }

    let content = normalize_memory_content(message);
    let memory = MemoryEntry::new(CreateMemoryRequest {
        scope: crate::domain::memory::MemoryScope::LongTerm,
        title: "Hermes preference capture".to_string(),
        content: content.clone(),
        tags: vec![
            "hermes".to_string(),
            "preference".to_string(),
            infer_capability(message),
        ],
    });
    runtime.store.upsert_memory(memory.clone()).await?;
    Ok(Some(memory))
}

fn infer_session_title(message: &str) -> String {
    let compact = message.trim().replace('\n', " ");
    let prefix: String = compact.chars().take(24).collect();
    if prefix.is_empty() {
        "Hermes Session".to_string()
    } else {
        format!("Hermes · {}", prefix)
    }
}

fn infer_capability(message: &str) -> String {
    let lower = message.to_lowercase();
    if contains_any(
        &lower,
        &[
            "code",
            "编码",
            "实现",
            "修复",
            "bug",
            "rust",
            "typescript",
            "python",
        ],
    ) {
        "code".to_string()
    } else if contains_any(
        &lower,
        &["route", "router", "模型", "工具", "调用", "agent"],
    ) {
        "tools".to_string()
    } else if contains_any(&lower, &["总结", "摘要", "summarize", "总结下", "概括"]) {
        "summarize".to_string()
    } else if contains_any(&lower, &["计划", "规划", "plan", "roadmap"]) {
        "planning".to_string()
    } else {
        "chat".to_string()
    }
}

fn infer_hermes_loop_capability(config: &AppConfig, message: &str) -> String {
    let inferred = infer_capability(message);
    let has_tools_model = config.models.providers.iter().any(|provider| {
        provider
            .capabilities
            .iter()
            .any(|capability| capability.eq_ignore_ascii_case("tools"))
    });
    if has_tools_model {
        "tools".to_string()
    } else {
        inferred
    }
}

fn looks_like_remote_request(message: &str) -> bool {
    let lower = message.to_lowercase();
    contains_any(&lower, &["openai", "gpt", "remote", "cloud", "联网"])
}

fn looks_like_memory_intent(message: &str) -> bool {
    let lower = message.to_lowercase();
    contains_any(
        &lower,
        &[
            "记住",
            "记下来",
            "以后",
            "偏好",
            "默认",
            "always",
            "remember",
            "preference",
        ],
    )
}

fn normalize_memory_content(message: &str) -> String {
    let trimmed = message.trim();
    trimmed
        .trim_start_matches("请")
        .trim_start_matches("帮我")
        .trim_start_matches("记住")
        .trim()
        .to_string()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn rank_skills(skills: &[SkillDescriptor], query: &str, limit: usize) -> Vec<SkillDescriptor> {
    let tokens = tokenize_for_matching(query);
    let mut scored: Vec<(usize, SkillDescriptor)> = skills
        .iter()
        .filter_map(|skill| {
            let searchable = format!(
                "{} {} {} {}",
                skill.id,
                skill.description,
                skill.trigger,
                skill
                    .scripts
                    .iter()
                    .map(|script| script.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            let score = tokens
                .iter()
                .filter(|token| searchable.contains(token.as_str()))
                .count();
            (score > 0).then(|| (score, skill.clone()))
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.id.cmp(&right.1.id)));
    scored.truncate(limit);
    scored.into_iter().map(|(_, skill)| skill).collect()
}

fn tokenize_for_matching(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | '-'))
        .filter(|token| token.len() >= 2)
        .map(ToString::to_string)
        .collect()
}

fn build_task_suggestions(
    message: &str,
    working_dir: &str,
    strategic_clusters: &[LearningCluster],
) -> Vec<AgentTask> {
    let lower = message.to_lowercase();
    let mut tasks = Vec::new();
    let matched_clusters = matched_task_clusters(&lower, strategic_clusters);
    let guidance = cluster_guidance_summary(&matched_clusters);
    let strategy_sources = matched_clusters
        .iter()
        .map(|cluster| cluster.key.clone())
        .collect::<Vec<_>>();

    if contains_any(&lower, &["scan", "扫描", "索引", "index", "workspace"]) {
        tasks.push(AgentTask::new(CreateTaskRequest {
            title: "Hermes workspace scan".to_string(),
            description: with_guidance(
                "扫描当前工作目录，生成 Hermes 风格的本地索引入口。",
                guidance.as_deref(),
            ),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    "pwd && find . -maxdepth 2 -type f | head -80".to_string(),
                ],
            },
            working_dir: working_dir.to_string(),
            strategy_sources: strategy_sources.clone(),
        }));
    }

    if contains_any(&lower, &["skill", "技能", "plugin", "插件"]) {
        tasks.push(AgentTask::new(CreateTaskRequest {
            title: "Hermes skill registry refresh".to_string(),
            description: with_guidance(
                "重新扫描工作区中的 SKILL.md 与可执行脚本。",
                guidance.as_deref(),
            ),
            priority: TaskPriority::Low,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    "find . -name SKILL.md -o -name package.json | head -60".to_string(),
                ],
            },
            working_dir: working_dir.to_string(),
            strategy_sources: strategy_sources.clone(),
        }));
    }

    if contains_any(&lower, &["test", "验证", "检查", "audit"]) {
        tasks.push(AgentTask::new(CreateTaskRequest {
            title: "Hermes validation probe".to_string(),
            description: with_guidance(
                "列出项目测试入口，便于后续在 AgentOS 中执行。",
                guidance.as_deref(),
            ),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    "find . -maxdepth 3 \\( -name package.json -o -name Cargo.toml -o -name pyproject.toml \\)".to_string(),
                ],
            },
            working_dir: working_dir.to_string(),
            strategy_sources: strategy_sources.clone(),
        }));
    }

    tasks.truncate(3);
    tasks
}

fn matched_task_clusters<'a>(
    lower_message: &str,
    strategic_clusters: &'a [LearningCluster],
) -> Vec<&'a LearningCluster> {
    strategic_clusters
        .iter()
        .filter(|cluster| !cluster.pruned_from_planning)
        .filter(|cluster| {
            let key = cluster.key.to_lowercase();
            let title = cluster.title.to_lowercase();
            contains_any(lower_message, &[key.as_str(), title.as_str()])
                || cluster_pattern_matches(lower_message, &key, &cluster.capability)
                || cluster
                    .common_lessons
                    .iter()
                    .any(|lesson| lower_message.contains(&lesson.to_lowercase()))
        })
        .take(2)
        .collect()
}

fn cluster_guidance_summary(matched_clusters: &[&LearningCluster]) -> Option<String> {
    if matched_clusters.is_empty() {
        return None;
    }
    Some(
        matched_clusters
            .iter()
            .map(|cluster| {
                format!(
                    "参考长期经验簇 `{}`：success {:.0}%，source-success {:.0}%，weight {:.1}，state {}，lessons {}",
                    cluster.title,
                    cluster.success_rate * 100.0,
                    cluster.source_success_rate * 100.0,
                    cluster.strategic_weight,
                    cluster.suppression_level,
                    cluster.common_lessons.join(" | ")
                )
            })
            .collect::<Vec<_>>()
            .join("；"),
    )
}

fn cluster_pattern_matches(lower_message: &str, key: &str, capability: &str) -> bool {
    if key.contains("scan-index") {
        return contains_any(
            lower_message,
            &["scan", "扫描", "index", "索引", "workspace"],
        );
    }
    if key.contains("validate-audit") {
        return contains_any(lower_message, &["test", "验证", "check", "audit", "检查"]);
    }
    if key.contains("skill-ops") {
        return contains_any(lower_message, &["skill", "技能", "plugin", "插件"]);
    }
    if key.contains("review-fix") {
        return contains_any(lower_message, &["review", "审查", "bug", "fix", "修复"]);
    }
    contains_any(lower_message, &[capability])
}

fn with_guidance(base: &str, guidance: Option<&str>) -> String {
    match guidance {
        Some(guidance) if !guidance.trim().is_empty() => format!("{base} {guidance}。"),
        _ => base.to_string(),
    }
}

fn build_actions(
    routed_model: &ModelRouteDecision,
    workspace_contexts: &[WorkspaceContextFile],
    strategic_clusters: &[LearningCluster],
    memory_hits: &[MemoryEntry],
    session_hits: &[SessionSearchResult],
    suggested_skills: &[SkillDescriptor],
    suggested_tasks: &[AgentTask],
    memory_written: Option<&MemoryEntry>,
) -> Vec<HermesSuggestedAction> {
    let mut actions = vec![HermesSuggestedAction {
        kind: HermesActionKind::ModelRoute,
        title: format!("使用模型 {}", routed_model.selected.id),
        detail: routed_model.reason.clone(),
    }];

    if let Some(context) = workspace_contexts.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::SessionRecall,
            title: format!("加载工作区上下文 {}", context.title),
            detail: context.excerpt.clone(),
        });
    }

    if let Some(cluster) = strategic_clusters.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::SessionRecall,
            title: format!("参考长期经验 {}", cluster.title),
            detail: cluster.common_lessons.join(" | "),
        });
    }

    if let Some(memory) = memory_written {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::MemoryWrite,
            title: format!("已写入记忆 {}", memory.title),
            detail: memory.content.clone(),
        });
    } else if let Some(memory) = memory_hits.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::SessionRecall,
            title: format!("参考记忆 {}", memory.title),
            detail: memory.content.clone(),
        });
    }

    if let Some(session_hit) = session_hits.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::SessionRecall,
            title: format!("召回会话 {}", session_hit.title),
            detail: session_hit.excerpt.clone(),
        });
    }

    if let Some(skill) = suggested_skills.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::SkillSuggestion,
            title: format!("推荐 Skill {}", skill.id),
            detail: skill.description.clone(),
        });
    }

    if let Some(task) = suggested_tasks.first() {
        actions.push(HermesSuggestedAction {
            kind: HermesActionKind::TaskSuggestion,
            title: format!("建议任务 {}", task.title),
            detail: task.description.clone(),
        });
    }

    actions
}

fn build_strategy_trace(
    strategic_clusters: &[LearningCluster],
    suggested_tasks: &[AgentTask],
) -> HermesStrategyTrace {
    HermesStrategyTrace {
        response_sources: strategic_clusters
            .iter()
            .map(|cluster| cluster.key.clone())
            .collect(),
        task_sources: suggested_tasks
            .iter()
            .map(|task| HermesTaskStrategyTrace {
                task_title: task.title.clone(),
                strategy_sources: task.strategy_sources.clone(),
            })
            .collect(),
    }
}

fn compose_hermes_response(
    message: &str,
    routed_model: &ModelRouteDecision,
    workspace_contexts: &[WorkspaceContextFile],
    strategic_clusters: &[LearningCluster],
    memory_hits: &[MemoryEntry],
    session_hits: &[SessionSearchResult],
    suggested_skills: &[SkillDescriptor],
    suggested_tasks: &[AgentTask],
    memory_written: Option<&MemoryEntry>,
) -> String {
    let mut sections = vec![
        format!(
            "我按 Hermes 风格走了一轮本地 Agent Loop：先做模型路由，再召回记忆/会话，最后给出可执行的 Skill 和任务草案。当前选择 `{}`。",
            routed_model.selected.id
        ),
        format!("你的当前目标是：{}", message.trim()),
    ];

    if !workspace_contexts.is_empty() {
        let contexts = workspace_contexts
            .iter()
            .take(3)
            .map(|item| format!("{}({})", item.title, item.kind))
            .collect::<Vec<_>>()
            .join("、");
        sections.push(format!("工作区上下文：已加载 {}。", contexts));
    }

    if !strategic_clusters.is_empty() {
        let clusters = strategic_clusters
            .iter()
            .take(2)
            .map(|item| {
                format!(
                    "{} -> lessons: {}",
                    item.title,
                    item.common_lessons.join(" | ")
                )
            })
            .collect::<Vec<_>>()
            .join("；");
        sections.push(format!("长期经验：{}。", clusters));
        sections.push(format!(
            "策略来源键：{}。",
            strategic_clusters
                .iter()
                .take(3)
                .map(|cluster| cluster.key.as_str())
                .collect::<Vec<_>>()
                .join("、")
        ));
    }

    if let Some(memory) = memory_written {
        sections.push(format!("我已把这次偏好写入长期记忆：{}。", memory.content));
    } else if !memory_hits.is_empty() {
        let recall = memory_hits
            .iter()
            .take(2)
            .map(|memory| format!("{}：{}", memory.title, memory.content))
            .collect::<Vec<_>>()
            .join("；");
        sections.push(format!("记忆召回：{}。", recall));
    }

    if !session_hits.is_empty() {
        let recall = session_hits
            .iter()
            .take(2)
            .map(|item| format!("{} -> {}", item.title, item.excerpt))
            .collect::<Vec<_>>()
            .join("；");
        sections.push(format!("会话召回：{}。", recall));
    }

    if !suggested_skills.is_empty() {
        let skills = suggested_skills
            .iter()
            .map(|skill| format!("`{}`({})", skill.id, skill.description))
            .collect::<Vec<_>>()
            .join("、");
        sections.push(format!("可优先使用的 Skills：{}。", skills));
    }

    if !suggested_tasks.is_empty() {
        let tasks = suggested_tasks
            .iter()
            .map(|task| {
                if task.strategy_sources.is_empty() {
                    format!("`{}` -> {}", task.title, task.command.args.join(" "))
                } else {
                    format!(
                        "`{}` -> {} [sources: {}]",
                        task.title,
                        task.command.args.join(" "),
                        task.strategy_sources.join(", ")
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("；");
        sections.push(format!(
            "我还为 AgentOS 起草了可直接执行的任务：{}。",
            tasks
        ));
    } else {
        sections
            .push("这轮没有生成任务草案，说明你的请求更像是对话/规划而不是直接执行。".to_string());
    }

    sections.join("\n\n")
}

fn default_skill_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/root/space"),
        PathBuf::from("/root/.codex/skills"),
    ]
}

fn discover_workspace_contexts(working_dir: &str) -> Vec<WorkspaceContextFile> {
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

fn builtin_skills() -> Vec<SkillDescriptor> {
    vec![
        SkillDescriptor {
            id: "code-review".to_string(),
            description: "聚焦代码审查、风险识别与修复建议。".to_string(),
            trigger: "review / audit / bugfix".to_string(),
            installed: true,
            source: "builtin".to_string(),
            path: "builtin://code-review".to_string(),
            scripts: Vec::new(),
        },
        SkillDescriptor {
            id: "workspace-memory".to_string(),
            description: "将项目结构和偏好沉淀为长期记忆。".to_string(),
            trigger: "remember / persist / summarize".to_string(),
            installed: true,
            source: "builtin".to_string(),
            path: "builtin://workspace-memory".to_string(),
            scripts: Vec::new(),
        },
    ]
}

fn discover_skills(skill_roots: &[PathBuf]) -> Vec<SkillDescriptor> {
    let mut found = Vec::new();
    for root in skill_roots {
        scan_skill_dir(root, root, 0, &mut found);
    }
    found.sort_by(|left, right| left.id.cmp(&right.id));
    found
}

fn scan_skill_dir(
    base_root: &Path,
    current: &Path,
    depth: usize,
    found: &mut Vec<SkillDescriptor>,
) {
    if depth > 6 {
        return;
    }
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_skill_dir(base_root, &path, depth + 1, found);
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            found.push(parse_skill_descriptor(base_root, &path, &raw));
        }
    }
}

fn parse_skill_descriptor(base_root: &Path, path: &Path, raw: &str) -> SkillDescriptor {
    let relative = path
        .strip_prefix(base_root)
        .unwrap_or(path)
        .display()
        .to_string();
    let id = raw
        .lines()
        .find_map(|line| line.trim().strip_prefix("name:"))
        .map(|value| value.trim().to_string())
        .or_else(|| {
            path.parent()
                .and_then(|parent| parent.file_name())
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| relative.clone());
    let description = raw
        .lines()
        .find_map(|line| line.trim().strip_prefix("description:"))
        .map(|value| value.trim().to_string())
        .or_else(|| {
            raw.lines()
                .find(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(|line| line.trim().to_string())
        })
        .unwrap_or_else(|| "Workspace skill".to_string());
    let trigger = raw
        .lines()
        .find_map(|line| line.trim().strip_prefix("trigger:"))
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| format!("manual / {}", id));

    SkillDescriptor {
        id,
        description,
        trigger,
        installed: true,
        source: source_label(base_root),
        path: path.display().to_string(),
        scripts: discover_skill_scripts(path.parent().unwrap_or(base_root)),
    }
}

fn source_label(base_root: &Path) -> String {
    if base_root.starts_with("/root/.codex") {
        "codex-skill".to_string()
    } else {
        "workspace".to_string()
    }
}

fn model_weight(kind: &str, capabilities: &[String]) -> u8 {
    let mut weight = 50;
    if kind == "local" {
        weight += 20;
    }
    if capabilities.iter().any(|capability| capability == "code") {
        weight += 15;
    }
    if capabilities.iter().any(|capability| capability == "tools") {
        weight += 10;
    }
    weight
}

fn discover_skill_scripts(skill_dir: &Path) -> Vec<SkillScriptDescriptor> {
    let scripts_dir = skill_dir.join("scripts");
    let entries = match fs::read_dir(scripts_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            continue;
        };
        let Some(runner) = infer_script_runner(&path) else {
            continue;
        };
        scripts.push(SkillScriptDescriptor {
            name: name.to_string(),
            path: path.display().to_string(),
            runner: runner.to_string(),
        });
    }
    scripts.sort_by(|left, right| left.name.cmp(&right.name));
    scripts
}

fn infer_script_runner(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|item| item.to_str()) {
        Some("py") => Some("python3"),
        Some("sh") => Some("bash"),
        _ => None,
    }
}

fn resolve_skill_script(
    skill: &SkillDescriptor,
    requested: Option<&str>,
) -> AppResult<SkillScriptDescriptor> {
    if skill.scripts.is_empty() {
        return Err(AppError::Runtime(format!(
            "skill {} has no runnable scripts",
            skill.id
        )));
    }

    if let Some(requested) = requested {
        return skill
            .scripts
            .iter()
            .find(|script| script.name == requested || script.path.ends_with(requested))
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound(format!("script {} for skill {}", requested, skill.id))
            });
    }

    if skill.scripts.len() == 1 {
        Ok(skill.scripts[0].clone())
    } else {
        Err(AppError::Runtime(format!(
            "skill {} has multiple scripts; specify one of: {}",
            skill.id,
            skill
                .scripts
                .iter()
                .map(|script| script.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

fn build_skill_command(
    script: &SkillScriptDescriptor,
    args: &[String],
) -> AppResult<crate::domain::task::TaskCommand> {
    let runner = script.runner.as_str();
    if !matches!(runner, "python3" | "bash") {
        return Err(AppError::Runtime(format!(
            "unsupported skill runner: {}",
            script.runner
        )));
    }

    let mut command_args = vec![script.path.clone()];
    command_args.extend(args.iter().cloned());
    Ok(crate::domain::task::TaskCommand {
        program: runner.to_string(),
        args: command_args,
    })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use chrono::Utc;
    use uuid::Uuid;

    use crate::config::config::{
        AppConfig, ModelConfig, ModelProviderConfig, RuntimeConfig, SandboxConfig,
        SandboxProfileConfig, ServerConfig, StorageConfig,
    };
    use crate::domain::agent::HermesAgentRequest;
    use crate::domain::learning::LearningCluster;
    use crate::domain::task::{
        AgentTask, CreateTaskRequest, ExecutionStatus, TaskCommand, TaskExecutionRecord,
        TaskPriority,
    };
    use crate::domain::tool::PromoteSkillCandidateRequest;

    use super::{
        AgentRuntime, build_chat_completions_endpoint, build_task_suggestions,
        cluster_suppression_policy, discover_workspace_contexts, infer_capability,
        infer_hermes_loop_capability, record_task_learning, unique_skill_dir,
    };

    fn test_config(data_dir: String) -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8787,
            },
            storage: StorageConfig {
                data_dir,
                state_file: "agentos-test.db".to_string(),
            },
            runtime: RuntimeConfig {
                max_concurrent_tasks: 2,
                session_window_size: 12,
                memory_search_limit: 5,
            },
            sandbox: SandboxConfig {
                allowed_programs: vec!["sh".to_string(), "bash".to_string(), "cat".to_string()],
                allowed_working_dirs: vec!["/root/space".to_string(), "/tmp".to_string()],
                allowed_env: vec!["PATH".to_string()],
                max_output_bytes: 8_192,
                profiles: vec![SandboxProfileConfig {
                    id: "read-only".to_string(),
                    writable: false,
                    allowed_working_dirs: vec!["/root/space".to_string()],
                    allowed_programs: vec!["sh".to_string(), "bash".to_string(), "cat".to_string()],
                }],
            },
            models: ModelConfig {
                default_model: "local-phi4".to_string(),
                providers: vec![
                    ModelProviderConfig {
                        id: "local-phi4".to_string(),
                        kind: "local".to_string(),
                        endpoint: "http://localhost:11434".to_string(),
                        model_name: Some("phi4".to_string()),
                        api_key_env: None,
                        capabilities: vec![
                            "chat".to_string(),
                            "code".to_string(),
                            "tools".to_string(),
                        ],
                    },
                    ModelProviderConfig {
                        id: "remote-gpt".to_string(),
                        kind: "remote-api".to_string(),
                        endpoint: "https://example.com".to_string(),
                        model_name: Some("gpt-5.2".to_string()),
                        api_key_env: Some("OPENAI_API_KEY".to_string()),
                        capabilities: vec!["chat".to_string(), "planning".to_string()],
                    },
                ],
            },
        }
    }

    #[test]
    fn capability_inference_prefers_code_for_implementation_requests() {
        assert_eq!(infer_capability("请帮我实现 hermes agent runtime"), "code");
        assert_eq!(infer_capability("给我做一个总结"), "summarize");
    }

    #[test]
    fn hermes_loop_prefers_tools_capability_when_available() {
        let config = test_config("/tmp".to_string());
        assert_eq!(
            infer_hermes_loop_capability(&config, "请帮我实现 hermes agent runtime"),
            "tools"
        );
    }

    #[test]
    fn endpoint_builder_handles_v1_and_raw_hosts() {
        assert_eq!(
            build_chat_completions_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            build_chat_completions_endpoint("http://localhost:11434"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn task_suggestions_cover_workspace_and_skill_intents() {
        let tasks =
            build_task_suggestions("扫描 workspace 并刷新 skill registry", "/root/space", &[]);
        assert!(!tasks.is_empty());
        assert!(tasks.iter().any(|task| task.title.contains("workspace")));
        assert!(tasks.iter().any(|task| task.title.contains("skill")));
    }

    #[test]
    fn strategic_clusters_can_guide_task_descriptions() {
        let clusters = vec![LearningCluster {
            key: "tools::scan-index".to_string(),
            title: "Workspace Scan Strategy".to_string(),
            capability: "tools".to_string(),
            report_count: 3,
            source_usage_count: 2,
            source_success_rate: 1.0,
            success_rate: 0.9,
            recency_score: 0.8,
            strategic_weight: 19.0,
            suppression_level: "healthy".to_string(),
            pruned_from_planning: false,
            common_lessons: vec!["先缩小扫描范围再扩展".to_string()],
            example_tasks: vec!["workspace scan".to_string()],
            recommended_commands: vec!["find . -maxdepth 2".to_string()],
            strategic_skill_candidates: Vec::new(),
        }];

        let tasks = build_task_suggestions("scan workspace", "/root/space", &clusters);
        assert!(
            tasks
                .iter()
                .any(|task| task.description.contains("参考长期经验簇"))
        );
        assert!(tasks.iter().any(|task| {
            task.strategy_sources
                .iter()
                .any(|key| key == "tools::scan-index")
        }));
    }

    #[test]
    fn workspace_context_discovery_detects_agents_file() {
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

    #[tokio::test]
    async fn hermes_chat_persists_session_and_memory() {
        let temp_dir = env::temp_dir().join(format!("agentos-runtime-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        let result = runtime
            .hermes_chat(HermesAgentRequest {
                session_id: None,
                title: Some("Hermes Test".to_string()),
                working_dir: Some("/root/space".to_string()),
                message: "请记住：默认用中文，并扫描 workspace 里的 skill".to_string(),
                auto_persist_memory: true,
            })
            .await
            .expect("run hermes chat");

        assert_eq!(result.session.messages.len(), 2);
        assert!(result.memory_written.is_some());
        assert!(!result.tool_trace.is_empty());
        assert!(!result.suggested_tasks.is_empty());
        assert!(!result.strategy_trace.task_sources.is_empty());

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn task_learning_persists_memory_and_report() {
        let temp_dir = env::temp_dir().join(format!("agentos-learning-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        let task = AgentTask::new(CreateTaskRequest {
            title: "workspace scan".to_string(),
            description: "扫描项目文件".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "pwd && ls".to_string()],
            },
            working_dir: "/root/space".to_string(),
            strategy_sources: Vec::new(),
        });
        let execution = TaskExecutionRecord {
            id: Uuid::new_v4(),
            task_id: task.id,
            sandbox_profile: "read-only".to_string(),
            command_line: "sh -lc pwd && ls".to_string(),
            status: ExecutionStatus::Succeeded,
            exit_code: Some(0),
            stdout: "/root/space\nAgentOS\n".to_string(),
            stderr: String::new(),
            duration_ms: 42,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            working_dir: "/root/space".to_string(),
            audit_log: vec!["allowed program sh".to_string()],
        };

        let report = record_task_learning(
            &runtime.store,
            &task,
            &execution,
            runtime.config.runtime.session_window_size,
        )
        .await
        .expect("record learning");

        let memories = runtime.list_memories().await.expect("list memories");
        assert!(memories.iter().any(|item| item.id == report.memory_ids[0]));

        let learning = runtime
            .task_execution_insights(task.id)
            .await
            .expect("get learning insights");
        assert_eq!(learning.learning_reports.len(), 1);
        assert!(!learning.learning_reports[0].skill_candidates.is_empty());
        assert!(learning.learning_reports[0].source_strategy_keys.is_empty());

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn promote_skill_candidate_generates_skill_files() {
        let temp_dir = env::temp_dir().join(format!("agentos-promote-test-{}", Uuid::new_v4()));
        let work_dir = temp_dir.join("workspace");
        fs::create_dir_all(&work_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        let task = AgentTask::new(CreateTaskRequest {
            title: "workspace scan".to_string(),
            description: "扫描项目文件".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "pwd && ls".to_string()],
            },
            working_dir: work_dir.to_string_lossy().to_string(),
            strategy_sources: Vec::new(),
        });
        runtime
            .store
            .upsert_task(task.clone())
            .await
            .expect("persist task");
        let execution = TaskExecutionRecord {
            id: Uuid::new_v4(),
            task_id: task.id,
            sandbox_profile: "read-only".to_string(),
            command_line: "sh -lc pwd && ls".to_string(),
            status: ExecutionStatus::Succeeded,
            exit_code: Some(0),
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_ms: 42,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            working_dir: work_dir.to_string_lossy().to_string(),
            audit_log: vec!["allowed program sh".to_string()],
        };
        let report = record_task_learning(
            &runtime.store,
            &task,
            &execution,
            runtime.config.runtime.session_window_size,
        )
        .await
        .expect("record learning");

        let promoted = runtime
            .promote_skill_candidate(PromoteSkillCandidateRequest {
                task_id: Some(task.id),
                cluster_key: None,
                candidate_id: report.skill_candidates[0].id.clone(),
                working_dir: Some(work_dir.to_string_lossy().to_string()),
            })
            .await
            .expect("promote skill");

        assert!(promoted.files.iter().any(|path| path.ends_with("SKILL.md")));
        assert!(promoted.skill.path.contains(".agentos/skills"));
        assert!(!promoted.skill.scripts.is_empty());

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn unique_skill_dir_adds_suffix_when_existing() {
        let temp_dir = env::temp_dir().join(format!("agentos-unique-skill-{}", Uuid::new_v4()));
        fs::create_dir_all(temp_dir.join("sample-skill")).expect("seed existing dir");
        let allocated = unique_skill_dir(&temp_dir, "sample-skill").expect("allocate dir");
        assert!(allocated.ends_with("sample-skill-2"));
        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn learning_summary_clusters_similar_reports() {
        let temp_dir = env::temp_dir().join(format!("agentos-summary-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        for idx in 0..2 {
            let task = AgentTask::new(CreateTaskRequest {
                title: format!("workspace scan {}", idx),
                description: "扫描项目文件".to_string(),
                priority: TaskPriority::Normal,
                sandbox_profile: "read-only".to_string(),
                command: TaskCommand {
                    program: "sh".to_string(),
                    args: vec!["-lc".to_string(), "pwd && ls".to_string()],
                },
                working_dir: "/root/space".to_string(),
                strategy_sources: Vec::new(),
            });
            runtime
                .store
                .upsert_task(task.clone())
                .await
                .expect("persist task");
            let execution = TaskExecutionRecord {
                id: Uuid::new_v4(),
                task_id: task.id,
                sandbox_profile: "read-only".to_string(),
                command_line: "sh -lc pwd && ls".to_string(),
                status: ExecutionStatus::Succeeded,
                exit_code: Some(0),
                stdout: "ok".to_string(),
                stderr: String::new(),
                duration_ms: 42,
                started_at: Utc::now(),
                finished_at: Utc::now(),
                working_dir: "/root/space".to_string(),
                audit_log: vec!["allowed program sh".to_string()],
            };
            record_task_learning(
                &runtime.store,
                &task,
                &execution,
                runtime.config.runtime.session_window_size,
            )
            .await
            .expect("record learning");
        }

        let summary = runtime
            .learning_summary(Some("/root/space".to_string()))
            .await
            .expect("build learning summary");
        assert!(summary.total_reports >= 2);
        assert!(summary.clusters.iter().any(|cluster| {
            cluster.report_count >= 2 && !cluster.strategic_skill_candidates.is_empty()
        }));

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn learning_summary_tracks_strategy_source_feedback() {
        let temp_dir = env::temp_dir().join(format!("agentos-source-feedback-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        let task = AgentTask::new(CreateTaskRequest {
            title: "workspace scan guided".to_string(),
            description: "扫描项目文件".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "pwd && ls".to_string()],
            },
            working_dir: "/root/space".to_string(),
            strategy_sources: vec!["tools::scan-index".to_string()],
        });
        runtime
            .store
            .upsert_task(task.clone())
            .await
            .expect("persist task");
        let execution = TaskExecutionRecord {
            id: Uuid::new_v4(),
            task_id: task.id,
            sandbox_profile: "read-only".to_string(),
            command_line: "sh -lc pwd && ls".to_string(),
            status: ExecutionStatus::Succeeded,
            exit_code: Some(0),
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_ms: 42,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            working_dir: "/root/space".to_string(),
            audit_log: vec!["allowed program sh".to_string()],
        };
        record_task_learning(
            &runtime.store,
            &task,
            &execution,
            runtime.config.runtime.session_window_size,
        )
        .await
        .expect("record learning");

        let summary = runtime
            .learning_summary(Some("/root/space".to_string()))
            .await
            .expect("build learning summary");
        assert!(summary.clusters.iter().any(|cluster| {
            cluster.key.ends_with("scan-index")
                && cluster.source_usage_count >= 1
                && cluster.source_success_rate > 0.0
        }));

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn strategy_timeline_records_execution_feedback() {
        let temp_dir =
            env::temp_dir().join(format!("agentos-strategy-timeline-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let runtime = AgentRuntime::new(test_config(temp_dir.to_string_lossy().to_string()))
            .await
            .expect("create runtime");

        let task = AgentTask::new(CreateTaskRequest {
            title: "workspace scan guided".to_string(),
            description: "扫描项目文件".to_string(),
            priority: TaskPriority::Normal,
            sandbox_profile: "read-only".to_string(),
            command: TaskCommand {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "pwd && ls".to_string()],
            },
            working_dir: "/root/space".to_string(),
            strategy_sources: vec!["tools::scan-index".to_string()],
        });
        runtime
            .store
            .upsert_task(task.clone())
            .await
            .expect("persist task");
        let execution = TaskExecutionRecord {
            id: Uuid::new_v4(),
            task_id: task.id,
            sandbox_profile: "read-only".to_string(),
            command_line: "sh -lc pwd && ls".to_string(),
            status: ExecutionStatus::Succeeded,
            exit_code: Some(0),
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_ms: 42,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            working_dir: "/root/space".to_string(),
            audit_log: vec!["allowed program sh".to_string()],
        };
        record_task_learning(
            &runtime.store,
            &task,
            &execution,
            runtime.config.runtime.session_window_size,
        )
        .await
        .expect("record learning");

        let timeline = runtime
            .strategy_timeline(Some("/root/space".to_string()), Some(10))
            .await
            .expect("load strategy timeline");
        assert!(
            timeline
                .events
                .iter()
                .any(|event| event.strategy_source_key == "tools::scan-index")
        );

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn suppression_policy_prunes_low_quality_clusters() {
        let (state, pruned, penalty) = cluster_suppression_policy(4, 0.2, 0.4);
        assert_eq!(state, "pruned");
        assert!(pruned);
        assert!(penalty > 0.0);

        let (state, pruned, _) = cluster_suppression_policy(2, 0.4, 0.9);
        assert_eq!(state, "degraded");
        assert!(!pruned);
    }
}
