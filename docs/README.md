# AgentOS 文档总览

AgentOS 是一个面向单机环境的 Agent Operating System 原型。它吸收了 Codex 在本地执行、可观测性、工作区约束方面的设计思想，也吸收了 Hermes Agent 在长期记忆、技能演化、学习闭环方面的设计思想。

本目录用于沉淀 AgentOS 当前实现的文档基线，以及下一阶段演进所需的设计共识。

## 文档地图

- `overview.md`: 适合汇报和快速对齐的总览文档。
- `design.md`: 产品定位、设计目标、核心原则、边界与路线图。
- `architecture.md`: 系统分层、模块职责、关键链路、部署拓扑与扩展点。
- `runtime-learning.md`: 运行时循环、任务执行、会话与记忆、学习闭环、技能晋升与策略反馈。
- `api-data-model.md`: API 面、领域模型、SQLite 持久化结构与对象生命周期。
- `reference-baseline.md`: Codex 与 Hermes Agent 的参考基线，以及 AgentOS 的取舍与融合方式。
- `roadmap.md`: 下一阶段里程碑、优先 backlog 与演进顺序。
- `technical-debt.md`: 当前实现中的主要技术债、风险与治理顺序。

## 当前实现对应关系

AgentOS 当前主要由以下部分组成：

- `backend/`: Rust + Axum 本地控制面。
- `frontend/`: React + Vite 运行时可视化面板。
- `backend/src/runtime/agent_runtime.rs`: 任务、会话、记忆、技能、模型路由、Hermes 风格循环、学习汇总的核心编排器。
- `backend/src/executor/sandbox.rs`: 本地沙箱执行器，负责 allow-list、超时、取消、输出截断与审计日志。
- `backend/src/storage/sqlite_store.rs`: SQLite 持久层，负责任务、执行记录、会话、记忆、学习报告、策略时间线等数据存储。

## 推荐阅读顺序

1. 先看 `overview.md`，快速理解 AgentOS 是什么、做到哪一步。
2. 再看 `reference-baseline.md`，理解 AgentOS 参考了什么。
3. 再看 `design.md`，理解 AgentOS 想解决什么问题。
4. 再看 `architecture.md`，理解系统是如何分层和拆模块的。
5. 再看 `runtime-learning.md`，理解运行时与学习闭环。
6. 再看 `api-data-model.md`，理解接口与数据模型。
7. 再看 `roadmap.md`，理解下一阶段开发优先级。
8. 最后看 `technical-debt.md`，理解当前实现的主要工程风险。

## 读者对象

这些文档主要面向：

- 负责 AgentOS 方向与路线的产品/架构设计者；
- 需要扩展运行时、前后端能力的工程师；
- 关注本地 Agent 执行、记忆与自学习模式的研究者；
- 需要评估系统边界、可信性与运维可见性的使用者。
