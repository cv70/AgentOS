# AgentOS 运行时与学习架构

## 1. 运行时哲学

AgentOS 把 Agent 的工作过程抽象为四步：

1. 理解上下文；
2. 选择动作；
3. 在约束内执行；
4. 把结果回流为长期知识。

这里是 Codex 与 Hermes 思想融合最深的位置：

- Codex 贡献“受控执行 + 可观测”；
- Hermes 贡献“长期连续性 + 学习闭环”。

## 2. Hermes 风格交互循环

当前 `backend/src/runtime/agent_runtime.rs` 中的 `hermes_chat(...)` 是 AgentOS 的主循环内核。

### Stage A：会话落地

系统首先：

- 加载已有 session 或创建新 session；
- 追加用户消息；
- 把更新后的 session 持久化。

这让每一次对话都挂接到明确的会话对象上，而不是只在内存里短暂存在。

### Stage B：能力路由

系统会根据用户消息推断当前需要的 capability，然后选择合适模型。

当前路由逻辑综合考虑：

- capability match；
- default model；
- 是否 prefer local；
- routing weight。

这使“模型选择”成为运行时控制逻辑，而不是 prompt 内部隐式策略。

### Stage C：上下文装配

AgentOS 在生成回复前，会聚合三类上下文。

#### 1）工作区上下文

来源于：

- `AGENTS.md`
- `README.md`
- `SOUL.md`
- `MEMORY.md`
- `USER.md`

#### 2）学习上下文

来源于 learning summary 中的高价值策略簇，通常取最相关的若干个 cluster 注入规划与回复。

#### 3）检索上下文

来源于：

- memory search；
- session search；
- skill match；
- task suggestion。

### Stage D：回复生成

系统优先走 LLM 驱动的 Hermes-style loop。

如果模型不可用、调用失败或外部条件不满足，则退回启发式路径。即便在退化模式下，系统仍然会：

- 检索记忆；
- 检索历史会话；
- 匹配技能；
- 生成任务建议；
- 可选写入新记忆。

这意味着 AgentOS 的降级是“质量下降”，不是“功能消失”。

### Stage E：回写状态

在回复后，系统可根据输入参数和上下文决定是否：

- 自动写入 memory；
- 更新 assistant message 到 session；
- 为 UI 记录 tool trace / strategy trace。

## 3. 任务运行时模型

任务是 AgentOS 的执行原子。

### 任务状态机

```text
PENDING -> RUNNING -> DONE
                 -> FAILED
                 -> CANCELLED
```

### 任务来源

任务可以来源于三种路径：

- 用户直接通过 API 创建；
- Agent 在交互过程中提出任务草案；
- Skill 执行时临时包装为 task 再运行。

### 资源配置模型

当前资源配置按 priority 自动映射：

- `HIGH`: 更高 CPU / 内存 / timeout；
- `NORMAL`: 默认配置；
- `LOW`: 更轻量资源配置。

这样既保留资源差异，又避免用户每次都手写复杂资源参数。

### 任务溯源

任务可携带 `strategy_sources`，表示这个任务受哪些长期策略簇影响。

这是学习闭环的关键，因为它允许系统在事后问：

- 这次行动是受哪些经验驱动的？
- 这些经验最终被证实还是被证伪？

## 4. 沙箱执行模型

`SandboxExecutor` 是 AgentOS 当前的本地执行器。

### 4.1 校验阶段

执行前必须校验：

- sandbox profile 是否存在；
- 程序是否在全局和 profile allow-list 中；
- 工作目录是否在允许范围内。

### 4.2 执行阶段

通过校验后，执行器会：

- `env_clear()`；
- 只透传白名单环境变量；
- 设置工作目录；
- 启动子进程；
- 收集 stdout/stderr；
- 支持 timeout；
- 支持 cancel；
- 对输出做大小截断。

### 4.3 审计阶段

每次执行都会形成 audit log，至少包括：

- sandbox profile；
- working dir；
- allowed program；
- writable 标志；
- resources；
- command line；
- timeout/cancel/success/failure 结论。

这些信息后续会被 learning report 和 strategy event 当作“证据层”。

## 5. 会话运行时模型

Session 在 AgentOS 中是“连续协作状态”，而不是仅供展示的聊天记录。

### 会话结构

- title；
- working_dir；
- messages；
- summary；
- pinned_decisions；
- created_at / updated_at。

### 会话窗口压缩

当消息数超过 `session_window_size` 时，旧消息会从当前窗口移除，并更新 `compressed_context`。

这是一种轻量级上下文压缩策略，保证：

- 当前 prompt 不无限膨胀；
- 历史又不会完全消失。

### 会话搜索

会话搜索分两层：

1. 优先使用 SQLite FTS5；
2. 若 FTS 无结果，再 fallback 到 `LIKE` 搜索。

这使 AgentOS 能快速回忆：

- 之前做过的操作；
- 已达成的决策；
- 相似任务的历史表达。

## 6. 记忆运行时模型

Memory 用于表达“跨会话仍然有效的知识”。

### 记忆类型

- `ShortTerm`: 短期事实；
- `LongTerm`: 稳定偏好或长期约束；
- `Episodic`: 与具体事件相关的经验；
- `Semantic`: 抽象出来的知识或原则。

### 检索方式

当前采用混合检索：

- embedding cosine similarity；
- keyword hit boost；
- tags/title 辅助。

因此 AgentOS 同时具备：

- 语义近似回忆；
- 关键词硬匹配召回。

## 7. 学习闭环

学习闭环是 AgentOS 从“可执行”升级为“可进化”的关键能力。

### 触发时机

当 task execution 完成后，系统自动触发学习逻辑。

### 产出物

当前可能生成：

- `TaskLearningReport`；
- recap；
- lessons；
- episodic memory；
- `skill_candidates`；
- `source_strategy_keys`；
- `StrategyEvaluationEvent`。

### 设计价值

执行结果不再只是 UI 上一次性的 stdout/stderr，而会成为可持续消费的系统知识。

## 8. 策略聚类模型

多个 learning report 会被聚合成 `LearningCluster`。

每个 cluster 当前会关注：

- title / capability；
- report_count；
- source_usage_count；
- source_success_rate；
- success_rate；
- recency_score；
- strategic_weight；
- suppression_level；
- pruned_from_planning；
- common_lessons；
- example_tasks；
- recommended_commands；
- strategic_skill_candidates。

### 如何理解策略簇

可以把策略簇理解为：

- “系统对某类工作成功方法的当前假设”。

它比单条 memory 更强，因为它不仅可检索，而且可被评估、排序、降权与剪枝。

## 9. 策略反馈时间线

AgentOS 引入 `StrategyEvaluationEvent` 的目的是回答一个核心问题：

- “某个被系统采用的策略，是否真的帮助了后续任务？”

每条 event 至少记录：

- task_id；
- execution_id；
- strategy_source_key；
- event_kind；
- outcome_status；
- summary；
- evidence；
- created_at。

### 用途

策略时间线可用于：

- Dashboard 可视化；
- 后续自动调参；
- 降权低质量策略；
- 识别真正稳定有效的经验簇。

## 10. 技能演化模型

技能是 AgentOS 中“被操作化后的经验”。

### 技能来源

- builtin skills；
- 自动发现的 skill roots；
- learning report 晋升；
- strategy cluster 晋升。

### 晋升流程

当 skill candidate 被选中晋升后，系统会生成：

- `SKILL.md`
- `scripts/run.sh`

路径为：

- `.agentos/skills/<skill-id>/`

### 设计意义

这让技能成为真实工作区资产：

- 可读；
- 可修改；
- 可版本管理；
- 可再次执行。

## 11. 失败与降级策略

### 模型不可用

退回启发式规划 + memory/session/skill 检索。

### 没有 workspace context

仍可基于 session、memory、task 运行。

### 没有学习历史

仍可作为本地任务控制面运行，并在后续逐步积累 learning artifacts。

## 12. 后续增强建议

当前运行时与学习架构可以自然演进到：

- turn-end async prefetch；
- 更细粒度的 user/agent memory mode；
- supervisor/subagent 策略继承；
- remote worker 统一写入 learning store；
- 更丰富的 skill schema，而不是只生成 shell script。
