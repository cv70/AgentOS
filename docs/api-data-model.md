# AgentOS API 与数据模型

## 1. API 设计原则

AgentOS 的 API 不是只暴露一个 `/chat` 入口，而是显式暴露系统内部的运行对象。

这样做的目的有三个：

- 前端能做系统级可观测性；
- 外部集成能精准访问特定能力；
- 数据生命周期能和运行逻辑一一对应。

当前 API 以这些对象为中心：

- tasks；
- sessions；
- memories；
- tools / skills；
- models；
- workspace contexts；
- learning summary / strategy timeline。

## 2. API 总览

统一前缀：

- `/api/v1`

### 2.1 Overview

- `GET /overview`

返回系统总览快照，包括：

- scheduler 状态；
- recent tasks；
- recent sessions；
- recent memories；
- tools；
- models。

### 2.2 Tasks

- `GET /tasks`
- `POST /tasks`
- `PATCH /tasks/:task_id/status`
- `POST /tasks/:task_id/run`
- `POST /tasks/:task_id/cancel`
- `GET /tasks/:task_id/executions`
- `GET /executions/:execution_id`
- `GET /scheduler`

这组接口覆盖任务创建、调度、取消、状态修改与执行后洞察。

其中：

- `/scheduler` 返回当前并发上限、运行中任务 ID、排队任务 ID 和剩余容量；
- `/executions/:execution_id` 允许前端按 execution-first 方式直接查看一次执行的 stdout / stderr / audit log。

### 2.3 Sessions

- `GET /sessions`
- `POST /sessions`
- `POST /sessions/search`
- `POST /sessions/:session_id/messages`

### 2.4 Memories

- `GET /memories`
- `POST /memories`
- `POST /memories/search`

### 2.5 Tools / Skills / Contexts

- `GET /tools`
- `POST /contexts`
- `POST /skills/promote`
- `POST /skills/:skill_id/run`

### 2.6 Learning

- `POST /learning/summary`
- `POST /learning/timeline`

### 2.7 Agent Loop

- `POST /agent/hermes/chat`

### 2.8 Models

- `GET /models`
- `POST /models/route`
- `POST /models/default`

### 2.9 统一错误返回

所有失败响应统一返回：

```json
{
  "error": {
    "kind": "conflict",
    "message": "conflict: task <id> is already scheduled"
  }
}
```

当前 `kind` 主要包括：

- `validation`
- `conflict`
- `not_found`
- `runtime`
- `storage`
- `configuration`

这让 Dashboard 或外部接入方可以稳定区分“参数错误”“调度冲突”“对象不存在”和“运行时失败”。

## 3. 核心领域对象

### 3.1 AgentTask

表示一个可执行任务。

关键字段：

- `id`
- `title`
- `description`
- `priority`
- `status`
- `sandbox_profile`
- `resources`
- `command`
- `working_dir`
- `strategy_sources`
- `last_exit_code`
- `created_at`
- `updated_at`

### 3.2 TaskExecutionRecord

表示任务的一次具体执行。

关键字段：

- `task_id`
- `sandbox_profile`
- `command_line`
- `status`
- `exit_code`
- `stdout`
- `stderr`
- `duration_ms`
- `working_dir`
- `audit_log`

### 3.3 AgentSession

表示持久化会话状态。

关键字段：

- `title`
- `working_dir`
- `messages`
- `summary`
- `created_at`
- `updated_at`

### 3.4 MemoryEntry

表示可检索的长期知识条目。

关键字段：

- `scope`
- `title`
- `content`
- `tags`
- `created_at`

### 3.5 SkillDescriptor

表示一个技能包。

关键字段：

- `id`
- `description`
- `trigger`
- `installed`
- `source`
- `path`
- `scripts`

### 3.6 ModelProvider

表示一个可路由的模型提供者。

关键字段：

- `id`
- `kind`
- `endpoint`
- `capabilities`
- `is_default`
- `routing_weight`

### 3.7 TaskLearningReport

表示一次执行后生成的学习报告。

关键字段：

- `task_id`
- `execution_id`
- `status`
- `source_strategy_keys`
- `recap`
- `lessons`
- `memory_ids`
- `session_id`
- `skill_candidates`
- `created_at`

### 3.8 StrategyEvaluationEvent

表示策略被采用后的执行反馈事件。

关键字段：

- `task_id`
- `execution_id`
- `strategy_source_key`
- `event_kind`
- `outcome_status`
- `summary`
- `evidence`
- `created_at`

## 4. SQLite 持久化结构

AgentOS 当前使用 SQLite 作为本地 source of truth。

### 表结构

- `tasks`
- `task_executions`
- `task_learning_reports`
- `strategy_evaluation_events`
- `sessions`
- `session_messages`
- `memories`
- `session_messages_fts`（虚表）

### 设计特点

当前采用“主 payload JSON + 关键索引列”的混合模式。

优点：

- 迭代早期 schema 演进成本低；
- 领域对象可以直接序列化/反序列化；
- 仍可对 created_at、task_id、strategy_source_key 等关键维度建立索引。

## 5. 搜索模型

### 5.1 Session Search

会话搜索分两步：

1. 先用 FTS5 做全文检索与排序；
2. 若无结果，再用 `LIKE` 做回退检索。

### 5.2 Memory Search

记忆搜索使用：

- 本地 embedding；
- cosine similarity；
- keyword hit boost。

因此它同时具备语义检索与关键词召回能力。

## 6. 配置模型

主要配置位于 `backend/config.yaml`，分为五组：

- `server`
- `storage`
- `runtime`
- `sandbox`
- `models`

### 配置语义

- `server`: 服务监听地址；
- `storage`: 数据目录与数据库文件；
- `runtime`: session window、memory search limit、最大并发等；
- `sandbox`: 程序、目录、环境变量、profile 等执行边界；
- `models`: 默认模型和 provider 列表。

## 7. 数据生命周期

### 7.1 Task 生命周期

```text
CreateTaskRequest
  -> AgentTask
  -> TaskExecutionRecord
  -> TaskLearningReport
  -> StrategyEvaluationEvent
```

### 7.2 Session 生命周期

```text
CreateSessionRequest
  -> AgentSession
  -> append messages
  -> compress summary
  -> session search recall
```

### 7.3 Memory 生命周期

```text
CreateMemoryRequest or autopersist
  -> MemoryEntry
  -> embedding generation
  -> retrieval
```

### 7.4 Skill 生命周期

```text
TaskLearningReport.skill_candidates
  -> PromoteSkillCandidateRequest
  -> generated skill directory
  -> SkillDescriptor discovery
  -> RunSkillRequest execution
```

## 8. 后续接口演进建议

为了保持 API 清晰，建议后续遵守以下原则：

- 优先新增字段，不轻易破坏既有对象；
- 继续保留 `strategy_sources` / `source_strategy_keys` 这类溯源字段；
- 让 execution evidence 可单独查询，不依赖重放整段会话；
- request DTO 与持久化 entity 保持分离；
- 新能力尽量以新的对象和 endpoint 形式加入，而不是把旧接口做成万能入口。

## 9. 后续仍建议新增的 API

后续自然可增加：

- `/skills/reload`：刷新技能注册表；
- `/contexts/watch`：订阅上下文文件变化；
- `/agents/subtasks`：未来多 Agent 体系引入后使用。
