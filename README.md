# AgentOS — 单机 Agent Operating System 原型

AgentOS 是一个面向单机环境的本地优先 Agent 操作系统原型。

它的设计目标不是只做一个“会聊天的助手”，而是把 Agent 组织成一个持续运行的软件系统，让它同时具备：

- Codex 风格的执行纪律、工作区约束与可观测性；
- Hermes Agent 风格的长期记忆、学习闭环与技能演化；
- 面向任务、会话、记忆、技能、模型、策略反馈的统一对象模型。

当前仓库已经包含一个可运行的 AgentOS 最小实现：

- `backend/`: Rust + Axum 本地控制面
- `frontend/`: React + Vite 运行时仪表盘
- `docs/`: 设计文档、架构文档、运行时/学习文档、API/数据模型文档

## AgentOS 想解决什么问题

很多 Agent 系统要么偏聊天，要么偏自动化：

- 偏聊天的系统常常缺少执行边界、工作区规则与长期状态；
- 偏自动化的系统常常缺少连续会话、用户建模与经验沉淀。

AgentOS 的目标是把以下能力收敛到同一个本地运行时里：

- 理解工作区；
- 生成任务；
- 在受控边界中执行任务；
- 记录执行证据；
- 形成记忆与学习报告；
- 把可复用经验晋升为技能；
- 用策略反馈继续改善下一次规划。

## 核心能力

### 1. 任务运行时

AgentOS 使用 `Task` 作为执行货币，支持：

- 任务创建与状态流转；
- 后台执行；
- 取消与超时；
- 执行审计日志；
- 执行结果回写与后续学习。

### 2. 会话与上下文

AgentOS 通过 `Session` 维持持续协作状态，支持：

- 会话持久化；
- 消息窗口压缩；
- SQLite FTS5 会话搜索；
- 工作区上下文文件发现。

自动发现的上下文文件包括：

- `AGENTS.md`
- `README.md`
- `SOUL.md`
- `MEMORY.md`
- `USER.md`

### 3. 记忆系统

AgentOS 使用本地 SQLite 存储多类记忆：

- `ShortTerm`
- `LongTerm`
- `Episodic`
- `Semantic`

记忆检索采用混合方案：

- local embedding
- cosine similarity
- keyword hit boost

### 4. 学习闭环

任务执行完成后，系统会自动生成：

- `TaskLearningReport`
- recap / lessons learned
- episodic memory
- skill candidate
- strategy evaluation event

相似学习报告会进一步聚类为长期策略簇，用于后续任务规划。

### 5. 技能系统

AgentOS 同时支持：

- builtin skills
- 从本地目录发现技能
- 将 learning candidate 晋升为真实技能

晋升后的技能会写入：

- `.agentos/skills/<skill-id>/`

这让技能成为工作区资产，而不是隐藏在数据库里的黑盒。

### 6. 模型路由

AgentOS 把模型选择作为运行时控制问题处理，而不是藏在 prompt 里。

当前支持：

- 根据 capability 路由模型；
- default model 切换；
- local-first 偏好；
- fallback 顺序返回。

## 架构概览

AgentOS 当前采用五层架构：

```text
Interface Layer
  -> React Dashboard / API Client
Control Plane Layer
  -> Axum routes / handlers
Runtime Orchestration Layer
  -> AgentRuntime / Hermes-style Loop / Model Router / Learning
Execution Layer
  -> Sandbox Executor / Local Process Runner
Persistence & Feedback Layer
  -> SQLite / Session FTS / Memories / Learning Reports / Strategy Events
```

关键模块映射：

- `backend/src/runtime/agent_runtime.rs`: 编排核心
- `backend/src/executor/sandbox.rs`: 执行边界与审计
- `backend/src/storage/sqlite_store.rs`: 本地持久层
- `frontend/src/App.tsx`: Dashboard 主视图

## 仓库结构

```text
AgentOS/
├── backend/
│   ├── config.yaml
│   ├── Cargo.toml
│   └── src/
│       ├── api/
│       ├── config/
│       ├── domain/
│       ├── executor/
│       ├── runtime/
│       ├── storage/
│       ├── error.rs
│       ├── main.rs
│       └── state.rs
├── frontend/
│   ├── package.json
│   └── src/
├── docs/
│   ├── README.md
│   ├── overview.md
│   ├── reference-baseline.md
│   ├── design.md
│   ├── architecture.md
│   ├── runtime-learning.md
│   ├── api-data-model.md
│   ├── roadmap.md
│   └── technical-debt.md
└── plan.md
```

## 文档索引

建议按以下顺序阅读：

1. `docs/overview.md`
2. `docs/reference-baseline.md`
3. `docs/design.md`
4. `docs/architecture.md`
5. `docs/runtime-learning.md`
6. `docs/api-data-model.md`
7. `docs/roadmap.md`
8. `docs/technical-debt.md`

## 当前实现亮点

### Workspace Context Layer

系统会在工作区中自动发现上下文文件，并将其用于：

- 规划；
- 记忆召回；
- 模型路由；
- Hermes 风格 loop。

### Hardened Sandbox Executor

当前执行器具备以下最小可用控制能力：

- 程序 allow-list；
- 工作目录 allow-list；
- `read-only` / `workspace-write` / `tmp-only` profiles；
- 环境变量白名单；
- 超时控制；
- 取消控制；
- 审计日志。

### Strategy Feedback Timeline

当前系统已支持策略评估时间线，能记录：

- 哪个策略影响了任务；
- 该任务执行结果如何；
- 该策略是否被验证或证伪。

## 运行方式

### Backend

```bash
cd backend
cargo run
```

默认监听：

- `http://127.0.0.1:8787`

如需远程模型，请配置对应环境变量，例如：

```bash
export OPENAI_API_KEY=...
```

### Frontend

```bash
cd frontend
npm install
npm run dev
```

默认监听：

- `http://127.0.0.1:4173`

并通过 Vite 代理访问后端 API。

## API 示例

### 创建任务

```bash
curl -X POST http://127.0.0.1:8787/api/v1/tasks \
  -H 'content-type: application/json' \
  -d '{
    "title":"workspace scan",
    "description":"list project files",
    "priority":"NORMAL",
    "sandbox_profile":"workspace-write",
    "command":{"program":"sh","args":["-lc","pwd && ls"]},
    "working_dir":"/root/space"
  }'
```

### 后台执行任务

```bash
curl -X POST http://127.0.0.1:8787/api/v1/tasks/<task-id>/run
```

### 取消任务

```bash
curl -X POST http://127.0.0.1:8787/api/v1/tasks/<task-id>/cancel
```

### 检索记忆

```bash
curl -X POST http://127.0.0.1:8787/api/v1/memories/search \
  -H 'content-type: application/json' \
  -d '{"query":"本地执行和用户偏好"}'
```

### 查看工作区上下文

```bash
curl -X POST http://127.0.0.1:8787/api/v1/contexts \
  -H 'content-type: application/json' \
  -d '{"working_dir":"/root/space"}'
```

### 查看学习聚类

```bash
curl -X POST http://127.0.0.1:8787/api/v1/learning/summary \
  -H 'content-type: application/json' \
  -d '{"working_dir":"/root/space"}'
```

### 查看策略反馈时间线

```bash
curl -X POST http://127.0.0.1:8787/api/v1/learning/timeline \
  -H 'content-type: application/json' \
  -d '{"working_dir":"/root/space","limit":12}'
```

### 执行 Hermes 风格 Agent Loop

```bash
curl -X POST http://127.0.0.1:8787/api/v1/agent/hermes/chat \
  -H 'content-type: application/json' \
  -d '{
    "title":"Hermes Replica Session",
    "working_dir":"/root/space",
    "message":"请复刻 hermes-agent，并给我一个适合在 AgentOS 中落地的实现方案",
    "auto_persist_memory": true
  }'
```

## 当前边界说明

当前 AgentOS 是“单机本地控制面 + 运行时学习内核”，还不是完整的分布式 Agent 平台。

当前尚未覆盖：

- 多 Agent 树状委派；
- Docker / SSH / Remote Worker 执行器；
- 编辑器 ACP 协议接入；
- 强隔离容器/VM 级沙箱；
- 多节点共享调度与共享记忆。

## 下一步

如果你要继续往下推进，建议优先看：

- `docs/overview.md`：适合汇报和快速理解的总览
- `docs/roadmap.md`：下一阶段 backlog 与里程碑
- `docs/technical-debt.md`：当前实现中的主要技术债与治理优先级
