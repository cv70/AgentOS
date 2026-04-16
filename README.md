# AgentOS — 单机 Agent 操作系统

参考 `AgentCluster` 的前后端目录结构，实现了一个可运行的单机版 AgentOS 原型：

- `backend/`: Rust + Axum 后端，提供任务、会话、记忆、工具、模型路由 API
- `frontend/`: React + Vite 运行时仪表盘，展示 AgentOS 五层架构的核心状态

## 目录结构

```text
AgentOS/
├── backend/
│   ├── config.yaml
│   ├── Cargo.toml
│   ├── src/
│   │   ├── api/
│   │   ├── config/
│   │   ├── domain/
│   │   ├── executor/
│   │   ├── runtime/
│   │   ├── storage/
│   │   ├── error.rs
│   │   ├── main.rs
│   │   └── state.rs
│   └── tests/
└── frontend/
    ├── package.json
    ├── src/
    │   ├── App.tsx
    │   ├── App.css
    │   ├── index.css
    │   └── main.tsx
    └── vite.config.ts
```

## 后端能力映射

- `runtime::agent_runtime`: 聚合任务调度、会话窗口、记忆管理、工具/Skill 注册、模型路由
- `storage::sqlite_store`: 使用本地 SQLite 持久化任务、会话、记忆和任务执行审计
- `executor::sandbox`: 提供本地 sandbox executor，支持 allow-list、环境变量白名单、超时控制、后台运行与取消
- `domain::*`: 映射 AgentOS README 中的 Task / Session / Memory / Tool / Model 核心对象
- `api::v1`: 提供 `/overview`、`/tasks`、`/tasks/:id/run`、`/tasks/:id/cancel`、`/tasks/:id/executions`、`/sessions`、`/memories`、`/memories/search`、`/tools`、`/models` 等接口

## SQLite + 向量检索

当前记忆层采用本地 SQLite 数据库 `backend/data/agentos.db`：

- `tasks` / `sessions`: 以 JSON payload 形式持久化，方便当前原型快速演进
- `memories`: 除原始 payload 外，额外存储 `title`、`content`、`tags` 和本地 embedding
- 检索策略: `cosine similarity + keyword hit boost`
- 兼容迁移: 若存在旧版 `backend/data/agentos-state.json`，首次启动会自动迁移到 SQLite

## Hardened Sandbox Executor

任务执行器现在具备更强的最小可用隔离能力：

- 命令 allow-list: 仅允许 `config.yaml` 中声明的程序启动
- 工作目录 allow-list: 任务只能在允许的根目录下执行
- profile 策略:
  - `read-only`: 仅允许 `/root/space`
  - `workspace-write`: 面向 `/root/space`
  - `tmp-only`: 仅允许 `/tmp`
- 环境变量白名单: 子进程默认 `env_clear()`，仅透传白名单变量
- 后台运行: `POST /api/v1/tasks/:task_id/run` 立即返回 receipt，任务在后台执行
- 取消能力: `POST /api/v1/tasks/:task_id/cancel` 向运行中的任务发送取消信号
- 审计记录: `GET /api/v1/tasks/:task_id/executions` 返回 stdout/stderr、退出码、耗时和 audit_log

## 运行方式

### Backend

```bash
cd backend
cargo run
```

默认监听 `http://127.0.0.1:8787`。

### Frontend

```bash
cd frontend
npm install
npm run dev
```

默认监听 `http://127.0.0.1:4173`，并通过 Vite 代理访问后端 API。

## API 示例

创建任务：

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

后台运行任务：

```bash
curl -X POST http://127.0.0.1:8787/api/v1/tasks/<task-id>/run
```

取消运行中的任务：

```bash
curl -X POST http://127.0.0.1:8787/api/v1/tasks/<task-id>/cancel
```

检索记忆：

```bash
curl -X POST http://127.0.0.1:8787/api/v1/memories/search \
  -H 'content-type: application/json' \
  -d '{"query":"本地执行和用户偏好"}'
```
