# AgentOS 技术债清单

本文用于记录当前 AgentOS 原型里的主要技术债、风险等级以及推荐治理顺序。

## 1. 总体判断

当前 AgentOS 已经具备不错的原型完整度：

- 有 backend / frontend / docs；
- 有任务、会话、记忆、技能、模型与学习链路；
- 有 SQLite、FTS、策略反馈时间线；
- 有可运行的本地执行器。

但从工程化角度看，仍有明显技术债，主要集中在：

- runtime 过于集中；
- API 与前端契约还不够稳定；
- 测试覆盖明显不足；
- 执行边界仍然偏“策略级”而不是“隔离级”；
- learning/skill 质量控制还比较初级。

## 2. 高优先级技术债

### 2.1 `agent_runtime.rs` 过大、职责过重

现状：

- `backend/src/runtime/agent_runtime.rs` 同时承担了任务、会话、记忆、技能、模型、上下文、学习、Hermes loop 等职责。

风险：

- 高耦合；
- 难测试；
- 难 review；
- 后续功能继续叠加时容易失控。

建议：

- 拆成 `task_runtime.rs`、`session_runtime.rs`、`memory_runtime.rs`、`learning_runtime.rs`、`model_router.rs`、`workspace_context.rs` 等子模块；
- `AgentRuntime` 只保留聚合与编排职责。

### 2.2 缺少系统化测试矩阵

现状：

- 当前已有部分测试，但整体覆盖不足，尤其是跨模块链路测试。

风险：

- 修改 learning / routing / storage 时容易引入回归；
- UI 与 API 之间字段漂移不易及时发现。

建议：

- 增加 task -> execution -> learning 的端到端测试；
- 增加 session search / memory search / strategy timeline 回归测试；
- 增加前端 smoke test。

### 2.3 API 错误语义还不统一

现状：

- handler 层当前主要把错误映射为 `404` 或 `500`，结构较粗。

风险：

- 前端难以稳定处理错误；
- 外部接入难形成一致客户端逻辑。

建议：

- 引入统一错误响应结构；
- 区分 validation / not_found / conflict / runtime / storage / executor 等类型。

### 2.4 Sandbox 还不是强隔离

现状：

- 当前 sandbox 主要依赖 allow-list、cwd policy、timeout、cancel 与环境变量白名单。

风险：

- 容易被误认为是“安全沙箱”；
- 不适合直接承载更高风险执行任务。

建议：

- 文档里持续强调边界；
- 后续尽快引入 Docker executor 或更强执行后端。

## 3. 中优先级技术债

### 3.1 存储层过度依赖 payload JSON

现状：

- 很多表以 payload JSON 为主，只抽取少量索引列。

风险：

- schema 演进简单，但复杂查询会逐渐变难；
- 数据治理与迁移成本会在中后期上升。

建议：

- 保留 payload 灵活性；
- 同时逐步把高频查询字段结构化；
- 对 learning / strategy / skill usage 等高价值对象尽早补专门列。

### 3.2 前端视图与数据模型耦合偏紧

现状：

- `frontend/src/App.tsx` 承担大量类型定义、请求结果消费与面板逻辑。

风险：

- 视图层难扩展；
- 难引入更复杂交互；
- 状态管理会快速变脆弱。

建议：

- 抽出 API client、types、hooks、panel components；
- 把 overview/task/learning/model 分区域模块化。

### 3.3 Skill schema 仍偏初级

现状：

- 晋升后的技能主要生成 `SKILL.md` 和 `scripts/run.sh`。

风险：

- 技能元数据不足；
- runner 过于单一；
- 版本化和兼容性难管理。

建议：

- 增加 skill manifest；
- 支持更多 runner；
- 增加 skill usage metrics。

### 3.4 学习质量控制仍较粗糙

现状：

- 已有 strategy cluster、success_rate、suppression、prune，但解释性和精度还有空间。

风险：

- 容易形成噪声策略；
- 容易让低质量经验在早期获得过高权重。

建议：

- 加入更多 confidence 维度；
- 提升 cluster merge/split 能力；
- 增加“为何采用/为何降权”的解释字段。

## 4. 低优先级但需要跟踪的技术债

### 4.1 配置能力仍偏单机原型

现状：

- 当前配置模型足够支撑单机，但还不适合复杂执行器矩阵与多环境配置。

建议：

- 后续为 executors、workers、skills、contexts 引入更清晰的配置层次。

### 4.2 缺少 context watch 机制

现状：

- 当前上下文文件主要靠每次请求时重新发现。

建议：

- 后续增加文件变更监听与缓存失效机制。

### 4.3 缺少面向外部集成的稳定 SDK 层

现状：

- 外部系统接入主要依赖 HTTP API，尚无单独 client SDK。

建议：

- 在 API 稳定后再考虑 Rust/TS client。

## 5. 推荐治理顺序

建议按照以下顺序治理：

1. 拆 runtime；
2. 补测试；
3. 统一 API 错误结构；
4. 拆前端 App；
5. 提升 learning / skill 质量；
6. 引入更强执行器；
7. 再做多 Agent / ACP / 远程 worker。

## 6. 技术债治理原则

治理技术债时，建议遵循以下原则：

- 不要为了“看起来整洁”做大重构，优先做可验证的渐进式拆分；
- 每修一类债，都补对应文档和测试；
- 先处理会放大后续开发成本的债；
- 对运行时、学习、执行边界相关债务要优先于表层 UI 优化。
