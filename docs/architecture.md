# AgentOS 架构文档

## 1. 总体架构

AgentOS 当前采用“五层架构”，把交互、控制、编排、执行、存储分离。

```text
+-----------------------------------------------------------+
|  Interface Layer                                          |
|  - React Dashboard                                        |
|  - REST API Client                                        |
+-----------------------------------------------------------+
|  Control Plane Layer                                      |
|  - Axum Router                                            |
|  - Handler / Request DTO / Response DTO                   |
+-----------------------------------------------------------+
|  Runtime Orchestration Layer                              |
|  - AgentRuntime                                           |
|  - Hermes-style Loop                                      |
|  - Model Router                                           |
|  - Workspace Context Loader                               |
|  - Skill Registry                                         |
|  - Learning Summary / Strategy Timeline                   |
+-----------------------------------------------------------+
|  Execution Layer                                          |
|  - Sandbox Executor                                       |
|  - Local Process Runner                                   |
|  - Timeout / Cancel / Audit                               |
+-----------------------------------------------------------+
|  Persistence & Feedback Layer                             |
|  - SQLite Store                                           |
|  - Session FTS                                            |
|  - Memory Embedding Store                                 |
|  - Learning Report / Strategy Event Store                 |
+-----------------------------------------------------------+
```

## 2. 运行时拓扑

### Backend

后端是一个 Rust + Axum 服务，承担本地控制面的职责。

关键入口：

- `backend/src/main.rs`

关键状态容器：

- `backend/src/state.rs`
- `AppState { runtime: AgentRuntime }`

核心编排器：

- `backend/src/runtime/agent_runtime.rs`

### Frontend

前端是 React + Vite Dashboard，用于展示 AgentOS 的系统状态，而不只是一个聊天界面。

它应该可视化：

- overview；
- tasks / executions；
- sessions / memories；
- tools / skills / contexts；
- models；
- learning summary / strategy timeline。

## 3. 模块职责拆分

### 3.1 API 层

文件：

- `backend/src/api/v1/routes.rs`
- `backend/src/api/v1/handlers.rs`

职责：

- 定义 REST 路由；
- 解析请求参数；
- 调用 runtime；
- 把结果序列化给前端或客户端。

原则：

- API 层尽量薄；
- 不在 handler 中放复杂业务逻辑；
- 复杂状态编排全部进入 runtime。

### 3.2 Domain 层

文件：

- `backend/src/domain/task.rs`
- `backend/src/domain/session.rs`
- `backend/src/domain/memory.rs`
- `backend/src/domain/tool.rs`
- `backend/src/domain/model.rs`
- `backend/src/domain/learning.rs`
- `backend/src/domain/context.rs`
- `backend/src/domain/agent.rs`

职责：

- 定义系统基础对象；
- 定义状态枚举、请求对象、响应对象；
- 保证前后端、runtime、storage 之间有稳定的数据语言。

### 3.3 Runtime 编排层

文件：

- `backend/src/runtime/agent_runtime.rs`

职责：

- 启动时 seed 初始数据；
- 管理任务、会话、记忆、技能、模型；
- 驱动 Hermes 风格 agent loop；
- 管理 learning summary 和 strategy timeline；
- 管理 workspace context 注入；
- 协调 storage 与 executor。

这是 AgentOS 当前最核心的“大脑”。

### 3.4 Execution 执行层

文件：

- `backend/src/executor/sandbox.rs`

职责：

- 根据 sandbox policy 校验任务；
- 解析 sandbox profile；
- 启动本地子进程；
- 收集 stdout/stderr；
- 管理 timeout、cancel、output truncate；
- 生成 audit log；
- 根据执行结果回写任务状态。

### 3.5 Storage 持久层

文件：

- `backend/src/storage/sqlite_store.rs`

职责：

- 初始化 SQLite schema；
- 兼容旧 JSON 状态迁移；
- 持久化 task/session/memory 等对象；
- 维护 session FTS 搜索索引；
- 存储 learning report 与 strategy event。

## 4. 核心子系统

### 4.1 任务子系统

关键对象：

- `AgentTask`
- `TaskExecutionRecord`
- `TaskLearningReport`
- `StrategyEvaluationEvent`

关键链路：

1. 创建任务；
2. 校验沙箱策略；
3. 后台执行；
4. 持久化 execution；
5. 生成 learning report；
6. 写入 strategy timeline。

### 4.2 会话子系统

关键对象：

- `AgentSession`
- `SessionMessage`
- `SessionSummary`
- `SessionSearchResult`

关键链路：

1. 创建或加载会话；
2. 追加消息；
3. 超出窗口后压缩历史；
4. 将消息持久化并进入 FTS 索引；
5. 供后续检索与回忆使用。

### 4.3 记忆子系统

关键对象：

- `MemoryEntry`
- `MemorySearchResult`

关键链路：

1. 显式创建或自动写入；
2. 生成 embedding；
3. 存入 SQLite；
4. 在需要时通过语义 + 关键词混合召回。

### 4.4 技能子系统

关键对象：

- `SkillDescriptor`
- `SkillScriptDescriptor`
- `SkillExecutionResult`
- `PromotedSkillResult`

关键链路：

1. 从 skill root 自动发现技能；
2. 根据请求进行匹配与推荐；
3. 运行 skill script；
4. 将 learning candidate 晋升为正式技能。

### 4.5 模型子系统

关键对象：

- `ModelProvider`
- `ModelRouteDecision`

关键链路：

1. 推断 capability；
2. 根据 capability/default/local-first 做排序；
3. 选择模型与 fallback；
4. 失败时退回启发式规划。

## 5. 核心请求流程

### 5.1 Hermes 风格聊天流程

```text
client request
  -> API handler
  -> load/create session
  -> append user message
  -> route model
  -> load workspace contexts
  -> build learning summary
  -> retrieve memories/sessions/skills/tasks
  -> run LLM loop or heuristic fallback
  -> append assistant message
  -> optional memory persist
  -> return response + traces
```

关键返回内容包括：

- assistant_message；
- tool_trace；
- model_route；
- memory_hits；
- session_hits；
- suggested_tasks；
- suggested_skills；
- strategy_trace。

### 5.2 任务执行流程

```text
create/run task
  -> validate sandbox policy
  -> set RUNNING
  -> spawn background runner
  -> collect output and audit
  -> finalize task status
  -> persist execution record
  -> generate learning report
  -> emit strategy events
```

### 5.3 技能晋升流程

```text
choose skill candidate
  -> resolve source task or cluster
  -> create .agentos/skills/<skill-id>/
  -> write SKILL.md
  -> write scripts/run.sh
  -> re-discover as installable skill
```

## 6. 信任边界与安全边界

### 当前已控制的边界

- allowed program；
- allowed working dir；
- writable / read-only profile；
- env_clear 后只透传白名单环境变量；
- output size limit；
- timeout；
- cancel。

### 当前尚未覆盖的边界

- 容器级文件系统隔离；
- namespace / cgroup / seccomp；
- 每任务网络权限隔离；
- 用户级权限切换；
- 强资源配额限制。

因此当前 sandbox 应被理解为“运行时策略层”，而不是“强隔离安全边界”。

## 7. 持久化架构

AgentOS 使用 SQLite 作为本地单机的 source of truth。

原因：

- 部署简单；
- 状态透明；
- 易于本地备份与迁移；
- FTS5 足以支撑会话搜索；
- schema 演进成本低。

数据大体分为三类：

- 运行状态：tasks / sessions / memories；
- 审计状态：task_executions；
- 学习状态：learning_reports / strategy_events。

## 8. 前端的架构角色

AgentOS 前端不应只是“展示最终答案”的页面，而应成为系统可观测性平面。

它应该回答这些问题：

- 系统此刻知道什么？
- 系统正在做什么？
- 系统最近学到了什么？
- 哪些策略正在被验证或证伪？

这正是 Codex 的执行可见性与 Hermes 的持续学习在 UI 层的结合点。

## 9. 扩展点

### 执行器扩展

未来可增加：

- Docker executor；
- SSH executor；
- remote worker executor；
- editor-bound executor。

### 检索扩展

未来可增加：

- 外部向量数据库；
- 远程记忆服务；
- workspace semantic index。

### Agent 拓扑扩展

未来可增加：

- supervisor / worker；
- specialized subagent；
- delegated planner；
- remote execution agent。

## 10. 架构不变量

AgentOS 后续演进时，建议保持以下不变量：

- 每次执行都能从持久化状态中重建；
- task、session、memory、skill 必须始终是独立可寻址对象；
- 学习必须绑定执行证据；
- workspace context 必须是一等输入；
- 即使远程模型失效，本地单机内核也应可继续工作。
