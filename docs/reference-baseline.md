# AgentOS 参考基线

本文说明 AgentOS 如何参考 Codex 与 Hermes Agent，并解释这些能力在单机 AgentOS 中被如何重新组织。

## 1. 为什么参考 Codex 和 Hermes Agent

AgentOS 选择这两套系统作为基线，是因为它们分别代表了 Agent 系统里两类最关键的能力：

- Codex 更强调执行纪律：本地运行、工作区约束、明确的工具调用边界、任务执行可观测性、结果可追溯。
- Hermes Agent 更强调持续进化：长期记忆、跨会话连续性、技能沉淀、学习闭环、用户模型与人格上下文。

AgentOS 的目标不是简单复刻任意一方，而是把二者组合成一个“可执行、可记忆、可学习、可运维”的单机 Agent 内核。

## 2. 从 Codex 借鉴的设计模式

### 2.1 工作区上下文优先

Codex 的重要思想之一是：工作区不仅是文件集合，也是执行规则与项目语义的来源。

AgentOS 沿用这一思想，自动发现并注入以下上下文文件：

- `AGENTS.md`
- `README.md`
- `SOUL.md`
- `MEMORY.md`
- `USER.md`

这意味着 AgentOS 在规划和生成响应时，不是只依赖当前消息，而是把仓库中的“约束、背景、人格、偏好”一起作为输入。

对应实现：

- `backend/src/runtime/agent_runtime.rs`
- `POST /api/v1/contexts`

### 2.2 本地执行与显式沙箱

Codex 强调：Agent 的执行权必须可见、可控、可限制。

AgentOS 采用了相同的基本方向：

- 程序 allow-list；
- 工作目录 allow-list；
- 显式的 sandbox profile；
- 执行超时与取消；
- stdout/stderr 与 audit log 持久化。

这让 AgentOS 的任务执行具备明确边界，而不是在模糊权限下“隐式做事”。

### 2.3 执行可追踪与结构化溯源

Codex 类型系统的优势在于，用户可以回看：执行了什么、在哪执行、结果怎样、为什么这样做。

AgentOS 将这一点进一步扩展为三层溯源：

- `TaskExecutionRecord`: 命令、工作目录、输出、退出码、审计日志；
- `strategy_sources`: 某个任务受哪些策略簇影响；
- `strategy_trace`: 某次回复级别的策略来源。

这为后续学习闭环提供了证据基础。

### 2.4 模型路由属于控制面，而不是 prompt 小技巧

Codex 把“选哪个模型”“何时调用工具”视为运行时控制问题，而不是纯提示词问题。

AgentOS 也采用类似做法，把模型能力匹配、默认模型、local-first 倾向统一纳入 runtime：

- `GET /api/v1/models`
- `POST /api/v1/models/route`
- `POST /api/v1/models/default`

## 3. 从 Hermes Agent 借鉴的设计模式

### 3.1 学习闭环

Hermes Agent 最有代表性的能力是：执行过的事情不会白做，系统会把经验变成之后的优势。

AgentOS 直接吸收这一点，在任务完成后自动生成：

- `TaskLearningReport`；
- recap 与 lessons learned；
- episodic memory；
- skill candidate；
- 长期策略簇；
- strategy evaluation timeline。

### 3.2 会话连续性与搜索

Hermes 把 Agent 看成“持续合作对象”，而不是一次性的问答机。

AgentOS 对应采用：

- 会话持久化；
- 会话窗口压缩；
- 历史消息检索；
- 跨会话召回。

实现上使用 SQLite + FTS5，避免系统一开始就依赖外部检索基础设施。

### 3.3 多层记忆模型

Hermes 把短期交互、长期用户偏好、经验记忆、人格输入分开对待。AgentOS 保留了这个思路，定义了：

- `ShortTerm`
- `LongTerm`
- `Episodic`
- `Semantic`

同时结合 embedding 与关键词命中做混合检索。

### 3.4 从经验中提炼技能

Hermes 会把成功路径沉淀为 Skill。AgentOS 也支持从 learning report 中提取 `skill_candidates`，并晋升为工作区下的真实技能目录：

- `.agentos/skills/<skill-id>/`

这使技能成为可见、可编辑、可复用的资产，而不是数据库黑盒。

### 3.5 Persona / User Model 文件体系

Hermes 推广了 `SOUL.md`、`MEMORY.md`、`USER.md` 这样的上下文文件习惯。

AgentOS 明确支持这些文件，并把它们与 Codex 风格的 `AGENTS.md` 放在同一套工作区上下文层中统一消费。

## 4. AgentOS 的融合方式

### 4.1 AgentOS 把 Agent 当作“操作系统内核”而不是单一聊天入口

Codex 和 Hermes 都有强 Agent 能力，但 AgentOS 更进一步：它把任务、会话、记忆、技能、模型、反馈时间线视为同一个操作系统里的基础对象。

### 4.2 执行与学习共用同一套对象模型

AgentOS 不是一个“聊天子系统”加一个“自动化子系统”的拼接，而是使用统一链路：

- task -> execution -> learning report -> memory -> skill candidate -> promoted skill

这样 UI、存储、分析与后续自动调优都更容易闭环。

### 4.3 单机优先，先把本地内核做扎实

Hermes 已经支持云端、网关、多平台与多环境。AgentOS 当前刻意从单机边界起步，原因是：

- 持久化更简单；
- 权限边界更清晰；
- 故障面更容易解释；
- 便于之后演化到分布式版本。

### 4.4 学习必须带证据

Codex 的可观测性与 Hermes 的自学习在 AgentOS 里结合为一条原则：

- 没有执行证据的“经验”不应长期主导后续规划。

因此策略簇不仅记录 lesson，也记录 adoption 后是否成功、何时被证伪。

### 4.5 上下文文件、记忆、策略簇是三层不同抽象

AgentOS 刻意区分：

- 静态上下文：`AGENTS.md` / `README.md` / `SOUL.md` / `MEMORY.md` / `USER.md`
- 可检索记忆：`MemoryEntry`
- 可评估策略：`LearningCluster`

这三者职责不同，不能混成一个“大 prompt blob”。

## 5. AgentOS 与两者的有意差异

### 5.1 暂不把远程多平台作为第一目标

Hermes 很强的一点是 Telegram、Discord、Slack、远程环境、多终端统一接入。AgentOS 当前阶段不把它作为主目标，而优先把单机控制面与学习面做好。

### 5.2 暂不以编辑器协议为中心

Codex 很适合编辑器/CLI 协同场景。AgentOS 当前更偏 HTTP API + Dashboard 的控制面架构，后续可再接 ACP 或 IDE 插件。

### 5.3 暂不引入完整多 Agent 树

Hermes 支持 subagent 与并行工作流。AgentOS 当前仍以单机上的统一 runtime 为核心，尚未形成 supervisor/worker 的分层树状拓扑。

### 5.4 当前沙箱仍是应用层策略，不是 OS 级隔离

这一点必须明确：AgentOS 当前的 sandbox 是 allow-list + timeout + cancellation + cwd policy，不是容器、虚拟机、seccomp 或 namespace 级别隔离。

## 6. 对 AgentOS 路线的启发

AgentOS 建议按以下顺序演进：

1. 先把本地任务执行与状态溯源做扎实；
2. 再把记忆、策略评估、技能晋升做成可量化闭环；
3. 再扩展到编辑器协议、远程 worker 与多 Agent；
4. 最后才是分布式 AgentOS。
