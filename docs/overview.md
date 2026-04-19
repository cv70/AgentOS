# AgentOS Overview

## 1. 一句话定义

AgentOS 是一个面向单机环境的本地优先 Agent Operating System：

- 用 Codex 风格的方法解决“如何安全、可见、可约束地执行”；
- 用 Hermes Agent 风格的方法解决“如何持续记忆、学习、沉淀技能”。

它不是单纯的聊天助手，也不是单纯的自动化脚本壳，而是一个把任务、会话、记忆、技能、模型和学习闭环统一起来的 Agent 运行时内核。

## 2. 我们为什么做 AgentOS

当前 Agent 系统常见两类问题：

- 只会聊天：上下文短、记不住、不会稳定执行；
- 只会自动化：能执行，但缺乏连续会话、用户偏好、长期经验积累。

AgentOS 的目标是把以下能力收敛到一个系统里：

- 理解工作区和项目规则；
- 生成和执行受控任务；
- 跨会话保留长期记忆；
- 从执行结果中学习；
- 把高质量经验沉淀成技能；
- 用策略反馈继续优化后续规划。

## 3. 设计来源

### 来自 Codex 的设计思想

- 工作区上下文优先：`AGENTS.md`、`README.md` 等文件是执行输入
- 本地优先执行：强调用户机器上的可见执行
- 显式沙箱与权限边界：allow-list、工作目录限制、取消与超时
- 可观测性与可追溯：任务执行、审计日志、策略来源都可回看
- 模型/工具选择属于 runtime 控制逻辑，而不是 prompt 小技巧

### 来自 Hermes Agent 的设计思想

- 长期连续性：会话与用户偏好可持续保留
- 学习闭环：执行后自动生成 learning report、memory、skill candidate
- 多层记忆：短期、长期、情节性、语义性知识分层
- 技能演化：把成功路径晋升为可复用 Skill
- Persona / User Model 文件：`SOUL.md`、`USER.md`、`MEMORY.md`

## 4. AgentOS 的核心主张

AgentOS 有四个核心主张：

### 4.1 Agent 是“操作系统内核”，不是一次性会话

系统的核心对象不是一段聊天记录，而是：

- task
- session
- memory
- skill
- model
- learning report
- strategy timeline

### 4.2 执行必须有边界

AgentOS 当前通过以下方式保证最小可控执行：

- 程序 allow-list
- 工作目录 allow-list
- sandbox profiles
- timeout / cancel
- stdout / stderr / audit log 持久化

### 4.3 学习必须绑定执行证据

策略不是凭空产生的。每条经验都应该能回到：

- 它影响了哪个任务；
- 该任务执行结果如何；
- 这条经验是被验证还是被证伪。

### 4.4 Skill 是工作区资产

被晋升的技能会写入工作区 `.agentos/skills/` 下，而不是藏在数据库里。这意味着技能：

- 可读；
- 可编辑；
- 可复用；
- 可版本化。

## 5. 系统架构概览

AgentOS 当前采用五层架构：

```text
Interface Layer
  - React Dashboard / API Client
Control Plane Layer
  - Axum Routes / Handlers
Runtime Orchestration Layer
  - AgentRuntime / Hermes-style Loop / Model Router / Learning
Execution Layer
  - Sandbox Executor / Local Process Runner
Persistence & Feedback Layer
  - SQLite / Session FTS / Memories / Learning Reports / Strategy Events
```

## 6. 当前实现已经具备的能力

### 6.1 任务运行时

- 创建任务、更新状态、后台运行
- 取消任务
- 记录执行结果
- 记录执行审计日志

### 6.2 会话与上下文

- 持久化 session
- 消息窗口压缩
- SQLite FTS5 会话搜索
- 自动发现工作区上下文文件

### 6.3 记忆系统

- 本地 SQLite 存储 memory
- embedding + keyword 混合检索
- 支持多 scope memory

### 6.4 学习闭环

- 任务完成后生成 learning report
- 写 episodic memory
- 产出 skill candidate
- 聚合成长期策略簇
- 输出 strategy evaluation timeline

### 6.5 技能系统

- 自动发现技能目录
- 执行技能脚本
- 从 learning candidate 晋升技能

### 6.6 模型路由

- 根据 capability 选模型
- 支持 local-first 偏好
- 支持 fallback
- 支持默认模型切换

## 7. 关键数据流

### 聊天/规划链路

```text
user message
  -> session
  -> workspace contexts
  -> memory/session retrieval
  -> model route
  -> LLM loop or heuristic fallback
  -> assistant response
```

### 执行/学习链路

```text
task
  -> sandbox execution
  -> execution record
  -> learning report
  -> memory
  -> skill candidate
  -> strategy evaluation event
```

## 8. 当前边界

AgentOS 当前是一个“单机本地控制面 + 学习内核”，还不是完整分布式 Agent 平台。

当前尚未完善或尚未引入：

- 多 Agent 委派树
- Docker / SSH / Remote Worker 执行器
- ACP / IDE 集成
- 强隔离容器 / VM 级沙箱
- 多节点调度与共享记忆

## 9. 接下来最重要的事

如果把 AgentOS 看成一个产品与平台，下一阶段最重要的是三件事：

- 把 runtime 模块拆清楚，降低耦合
- 把学习与策略反馈做得更可信、更可解释
- 把本地执行器升级为可替换的多后端执行器

## 10. 文档导航

如果读者想继续深入：

- `docs/reference-baseline.md`：参考了 Codex / Hermes 的哪些思想
- `docs/design.md`：完整设计目标与原则
- `docs/architecture.md`：完整架构分层与模块职责
- `docs/runtime-learning.md`：运行时与学习闭环细节
- `docs/api-data-model.md`：API 与数据模型
- `docs/roadmap.md`：后续路线图
- `docs/technical-debt.md`：当前技术债
