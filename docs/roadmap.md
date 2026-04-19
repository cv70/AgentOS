# AgentOS 路线图

本文把 AgentOS 下一阶段的演进拆成可执行 backlog，目标是让它从“单机原型”稳步演进为“可扩展的 Agent Runtime 内核”。

## 1. 路线图原则

规划优先级遵循四条原则：

- 先打磨单机内核，再扩展接入面；
- 先补可观测性与可验证性，再做复杂自治；
- 先让技能与学习真正可用，再谈规模化；
- 先把边界讲清楚，再提升自动化权限。

## 2. Phase 1：单机内核稳定化

目标：把当前 backend + frontend + docs 变成一个“结构完整、行为一致、接口可维护”的本地系统。

### 2.1 Runtime 稳定化

- 梳理 `agent_runtime.rs`，拆分任务、学习、模型、上下文、技能子模块
- 统一 runtime 内部错误语义与日志输出
- 把启发式 fallback 从大函数中进一步模块化
- 明确 session / memory / strategy 的边界

### 2.2 API 稳定化

- 为每个 endpoint 补齐稳定的 request/response 示例
- 统一错误返回结构
- 增加 execution-first 查询接口
- 增加 scheduler 状态接口

### 2.3 Dashboard 稳定化

- 补齐 loading / empty / error 状态
- 统一 overview、task、learning panel 的字段展示方式
- 增加 strategy timeline 的可读性与筛选能力
- 增加 skill promotion 结果可视化

### 2.4 验证与测试

- 为 runtime 关键路径补测试
- 补 session search / memory search / learning summary 的回归测试
- 建立前端最小 smoke test
- 增加 docs 与 README 的一致性检查流程

## 3. Phase 2：学习质量增强

目标：让 AgentOS 的 learning loop 从“有”变成“好用”。

### 3.1 记忆质量

- 改进 memory ranking
- 增加 tag / scope / recency 参与排序
- 支持显式 memory pin / demote
- 增加“为什么召回这条记忆”的解释字段

### 3.2 策略质量

- 优化 strategy clustering 算法
- 增加 cluster merge / split 机制
- 增加低质量策略自动降权与清理规则
- 增加 strategy confidence 指标

### 3.3 技能质量

- 让 skill candidate 模板更稳定
- 支持除 shell 外的更多 runner
- 增加 skill metadata versioning
- 增加 skill success metrics 与 usage history

### 3.4 评估面板

- 增加 strategy adoption -> outcome 的图形化链路
- 增加 cluster 对任务草案影响的展示
- 增加记忆命中、技能命中、模型路由命中的统计看板

## 4. Phase 3：执行器扩展

目标：从本地 shell executor 扩展到多种执行后端。

### 4.1 Docker Executor

- 增加容器化执行 profile
- 把当前 sandbox policy 映射到容器配置
- 增加镜像、挂载、网络策略定义

### 4.2 SSH / Remote Worker Executor

- 增加远程机器配置模型
- 支持远程命令执行与结果回传
- 把 execution record 与 audit log 统一回收

### 4.3 Worker Capability Registry

- 声明不同 worker 的能力与限制
- 让 task 可根据 capability 选择执行器
- 为后续多节点调度做准备

## 5. Phase 4：多 Agent 与委派

目标：让 AgentOS 从单一 runtime 演化为 supervisor + worker 体系。

### 5.1 Subtask 模型

- 引入 subtask / parent task 关系
- 支持委派链路与状态聚合
- 支持每个 subtask 的独立 execution / learning

### 5.2 Specialist Agent

- 支持 skill-specific worker
- 支持 planner / executor / reviewer 角色分工
- 支持按能力与上下文委派

### 5.3 多 Agent 反馈整合

- 合并多 agent 的 strategy trace
- 统一多 agent 的 learning report
- 对冲突经验进行 arbitration

## 6. Phase 5：接入面扩展

目标：把 AgentOS 内核接入更多使用场景。

### 6.1 ACP / IDE 集成

- 增加 ACP-compatible 入口层
- 将 tasks / diffs / command approval 暴露给编辑器
- 支持工作区级上下文同步

### 6.2 CLI / TUI

- 增加命令行交互模式
- 支持 session、task、memory、skill 的终端级查看与操作
- 为无人值守模式预留入口

### 6.3 Messaging Gateway

- 在单机内核稳定后，再考虑 Telegram / Slack / Discord 等桥接
- 保持消息渠道与核心 runtime 解耦

## 7. 优先 backlog

建议优先级从高到低如下：

### P0

- 拆分 `agent_runtime.rs`
- 补 runtime 核心测试
- 统一 API 错误结构
- 补 Dashboard 对 learning timeline 的交互能力

### P1

- 引入 `/scheduler` 和 `/executions/:id`
- 改进 memory ranking
- 改进 skill promotion 模板
- 增加 docs 驱动的开发约束

### P2

- Docker executor
- worker capability registry
- ACP 接入层
- subtask / delegation 模型

## 8. 交付物建议

每一阶段都建议至少产出四类交付物：

- 代码实现；
- API 示例；
- UI 可见性；
- 文档更新。

这样 AgentOS 才不会出现“功能已经存在，但别人不知道如何理解和使用”的情况。
