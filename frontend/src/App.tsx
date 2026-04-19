import { FormEvent, useEffect, useMemo, useState } from 'react'
import './App.css'

type Task = {
  id: string
  title: string
  description: string
  priority: string
  status: string
  sandbox_profile: string
  working_dir: string
  strategy_sources: string[]
  last_exit_code: number | null
  command: {
    program: string
    args: string[]
  }
}

type Session = {
  id: string
  title: string
  working_dir: string
  messages: Array<{ id: string; role: string; content: string }>
  summary: { compressed_context: string; pinned_decisions: string[] }
}

type SessionSearchResult = {
  session_id: string
  title: string
  working_dir: string
  role: string
  excerpt: string
  score: number
  created_at: string
}

type Memory = {
  id: string
  title: string
  content: string
  scope: string
  tags: string[]
}

type WorkspaceContextFile = {
  kind: string
  path: string
  title: string
  excerpt: string
  guidance: string[]
}

type TaskReceipt = {
  task_id: string
  status: string
  message: string
}

type ExecutionRecord = {
  id: string
  task_id: string
  status: string
  exit_code: number | null
  stdout: string
  stderr: string
  duration_ms: number
  command_line: string
  working_dir: string
  audit_log: string[]
}

type SchedulerStatus = {
  max_concurrent_tasks: number
  running_task_ids: string[]
  queued_task_ids: string[]
  available_slots: number
}

type ApiErrorEnvelope = {
  error?: {
    kind: string
    message: string
  }
}

type SkillCandidate = {
  id: string
  title: string
  description: string
  rationale: string
  suggested_trigger: string
}

type TaskLearningReport = {
  id: string
  task_id: string
  execution_id: string
  status: string
  source_strategy_keys: string[]
  recap: string
  lessons: string[]
  memory_ids: string[]
  session_id: string | null
  skill_candidates: SkillCandidate[]
  created_at: string
}

type LearningCluster = {
  key: string
  title: string
  capability: string
  report_count: number
  source_usage_count: number
  source_success_rate: number
  success_rate: number
  recency_score: number
  strategic_weight: number
  suppression_level: string
  pruned_from_planning: boolean
  common_lessons: string[]
  example_tasks: string[]
  recommended_commands: string[]
  strategic_skill_candidates: SkillCandidate[]
}

type LearningSummary = {
  generated_at: string
  total_reports: number
  clusters: LearningCluster[]
}

type StrategyEvaluationEvent = {
  id: string
  task_id: string
  execution_id: string
  strategy_source_key: string
  event_kind: string
  outcome_status: string
  summary: string
  evidence: string
  created_at: string
}

type StrategyTimeline = {
  generated_at: string
  total_events: number
  events: StrategyEvaluationEvent[]
}

type PromotedSkillResult = {
  skill: Skill
  files: string[]
  source_task: Task
}

type TaskExecutionInsights = {
  task_id: string
  executions: ExecutionRecord[]
  learning_reports: TaskLearningReport[]
}

type Tool = {
  id: string
  display_name: string
  category: string
  permissions: string[]
  hot_reload: boolean
}

type Skill = {
  id: string
  description: string
  trigger: string
  installed: boolean
  source: string
  path: string
  scripts: Array<{
    name: string
    path: string
    runner: string
  }>
}

type Model = {
  id: string
  kind: string
  endpoint: string
  capabilities: string[]
  is_default: boolean
  routing_weight: number
}

type ModelRouteDecision = {
  selected: Model
  fallbacks: Model[]
  reason: string
}

type SkillExecutionResult = {
  skill: Skill
  selected_script: {
    name: string
    path: string
    runner: string
  }
  task: Task
  execution: ExecutionRecord
}

type HermesAction = {
  kind: string
  title: string
  detail: string
}

type HermesToolEvent = {
  tool: string
  detail: string
}

type HermesTaskStrategyTrace = {
  task_title: string
  strategy_sources: string[]
}

type HermesStrategyTrace = {
  response_sources: string[]
  task_sources: HermesTaskStrategyTrace[]
}

type HermesAgentResponse = {
  session: Session
  assistant_message: string
  routed_model: ModelRouteDecision
  workspace_contexts: WorkspaceContextFile[]
  strategic_clusters: LearningCluster[]
  strategy_trace: HermesStrategyTrace
  memory_hits: Memory[]
  session_hits: SessionSearchResult[]
  suggested_skills: Skill[]
  suggested_tasks: Task[]
  actions: HermesAction[]
  tool_trace: HermesToolEvent[]
  memory_written: Memory | null
}

type Overview = {
  node_name: string
  scheduler: { max_concurrent_tasks: number; queue_depth: number; running: number; paused: number }
  sessions: { total: number; window_size: number }
  memory: { total: number; search_limit: number }
  tools: { tools: number; skills: number; hot_reload_enabled: boolean }
  models: Model[]
  recent_tasks: Task[]
  recent_sessions: Session[]
  recent_memories: Memory[]
}

const statLabels = [
  { key: 'running', label: '运行任务' },
  { key: 'queue', label: '排队任务' },
  { key: 'sessions', label: '活跃会话' },
  { key: 'memory', label: '记忆条目' },
] as const

const initialTaskForm = {
  title: 'workspace scan',
  description: 'list project files',
  priority: 'NORMAL',
  sandbox_profile: 'workspace-write',
  program: 'sh',
  script: 'pwd && ls',
  working_dir: '/root/space',
}

const initialMemoryForm = {
  title: '运行偏好',
  content: '优先在本地执行工作区任务，并保留审计日志。',
  scope: 'LONG_TERM',
  tags: 'preference, local-first',
}

const initialHermesPrompt = '请复刻 hermes-agent，并给我一个适合在 AgentOS 中落地的实现方案。'

async function readApiError(response: Response) {
  const text = await response.text()

  try {
    const parsed: ApiErrorEnvelope = JSON.parse(text)
    if (parsed.error?.message) {
      return `[${parsed.error.kind}] ${parsed.error.message}`
    }
  } catch {
    // Ignore JSON parse failures and use the raw response body instead.
  }

  return text || `${response.status} ${response.statusText}`
}

async function requestJson<T>(input: RequestInfo | URL, init?: RequestInit): Promise<T> {
  const response = await fetch(input, init)
  if (!response.ok) {
    throw new Error(await readApiError(response))
  }
  return response.json() as Promise<T>
}

function App() {
  const [overview, setOverview] = useState<Overview | null>(null)
  const [schedulerStatus, setSchedulerStatus] = useState<SchedulerStatus | null>(null)
  const [tools, setTools] = useState<{ tools: Tool[]; skills: Skill[] }>({ tools: [], skills: [] })
  const [tasks, setTasks] = useState<Task[]>([])
  const [sessions, setSessions] = useState<Session[]>([])
  const [memories, setMemories] = useState<Memory[]>([])
  const [workspaceContexts, setWorkspaceContexts] = useState<WorkspaceContextFile[]>([])
  const [selectedTaskId, setSelectedTaskId] = useState<string>('')
  const [executions, setExecutions] = useState<ExecutionRecord[]>([])
  const [selectedExecutionId, setSelectedExecutionId] = useState<string>('')
  const [selectedExecution, setSelectedExecution] = useState<ExecutionRecord | null>(null)
  const [learningReports, setLearningReports] = useState<TaskLearningReport[]>([])
  const [learningSummary, setLearningSummary] = useState<LearningSummary | null>(null)
  const [strategyTimeline, setStrategyTimeline] = useState<StrategyTimeline | null>(null)
  const [learningMessage, setLearningMessage] = useState('')
  const [taskForm, setTaskForm] = useState(initialTaskForm)
  const [memoryForm, setMemoryForm] = useState(initialMemoryForm)
  const [taskMessage, setTaskMessage] = useState('')
  const [memoryMessage, setMemoryMessage] = useState('')
  const [runMessage, setRunMessage] = useState('')
  const [sessionQuery, setSessionQuery] = useState('')
  const [sessionResults, setSessionResults] = useState<SessionSearchResult[]>([])
  const [sessionMessage, setSessionMessage] = useState('')
  const [routeCapability, setRouteCapability] = useState('code')
  const [preferLocal, setPreferLocal] = useState(true)
  const [routeDecision, setRouteDecision] = useState<ModelRouteDecision | null>(null)
  const [routeMessage, setRouteMessage] = useState('')
  const [selectedSkillId, setSelectedSkillId] = useState('')
  const [selectedSkillScript, setSelectedSkillScript] = useState('')
  const [skillArgs, setSkillArgs] = useState('')
  const [skillMessage, setSkillMessage] = useState('')
  const [skillResult, setSkillResult] = useState<SkillExecutionResult | null>(null)
  const [hermesPrompt, setHermesPrompt] = useState(initialHermesPrompt)
  const [hermesSessionId, setHermesSessionId] = useState<string>('')
  const [hermesMessage, setHermesMessage] = useState('')
  const [hermesResult, setHermesResult] = useState<HermesAgentResponse | null>(null)
  const [dashboardError, setDashboardError] = useState('')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  const refreshDashboard = async (preserveTaskId?: string) => {
    setDashboardError('')
    const [overviewJson, schedulerJson, toolsJson, tasksJson, sessionsJson, memoriesJson, contextsJson, learningSummaryJson, strategyTimelineJson] = await Promise.all([
      requestJson<Overview>('/api/v1/overview'),
      requestJson<SchedulerStatus>('/api/v1/scheduler'),
      requestJson<{ tools: Tool[]; skills: Skill[] }>('/api/v1/tools'),
      requestJson<Task[]>('/api/v1/tasks'),
      requestJson<Session[]>('/api/v1/sessions'),
      requestJson<Memory[]>('/api/v1/memories'),
      requestJson<WorkspaceContextFile[]>('/api/v1/contexts', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ working_dir: '/root/space' }),
      }),
      requestJson<LearningSummary>('/api/v1/learning/summary', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ working_dir: '/root/space' }),
      }),
      requestJson<StrategyTimeline>('/api/v1/learning/timeline', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ working_dir: '/root/space', limit: 12 }),
      }),
    ])

    setOverview(overviewJson)
    setSchedulerStatus(schedulerJson)
    setTools(toolsJson)
    setTasks(tasksJson)
    setSessions(sessionsJson)
    setMemories(memoriesJson)
    setWorkspaceContexts(contextsJson)
    setLearningSummary(learningSummaryJson)
    setStrategyTimeline(strategyTimelineJson)

    const nextTaskId = preserveTaskId || selectedTaskId || tasksJson[0]?.id || ''
    if (nextTaskId) {
      setSelectedTaskId(nextTaskId)
      const executionJson = await requestJson<TaskExecutionInsights>(`/api/v1/tasks/${nextTaskId}/executions`)
      setExecutions(executionJson.executions)
      setLearningReports(executionJson.learning_reports)
      const nextExecutionId = selectedExecutionId || executionJson.executions[0]?.id || ''
      setSelectedExecutionId(nextExecutionId)
      if (nextExecutionId) {
        setSelectedExecution(await requestJson<ExecutionRecord>(`/api/v1/executions/${nextExecutionId}`))
      } else {
        setSelectedExecution(null)
      }
    } else {
      setExecutions([])
      setLearningReports([])
      setSelectedExecutionId('')
      setSelectedExecution(null)
    }
  }

  useEffect(() => {
    refreshDashboard()
      .catch((error) => {
        console.error('failed to load dashboard', error)
        setDashboardError(`初始化失败: ${String(error)}`)
      })
      .finally(() => setLoading(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (!selectedTaskId) {
      return
    }

    requestJson<TaskExecutionInsights>(`/api/v1/tasks/${selectedTaskId}/executions`)
      .then((data: TaskExecutionInsights) => {
        setExecutions(data.executions)
        setLearningReports(data.learning_reports)
        const nextExecutionId = data.executions[0]?.id || ''
        setSelectedExecutionId(nextExecutionId)
        if (!nextExecutionId) {
          setSelectedExecution(null)
        }
      })
      .catch((error) => {
        console.error('failed to load executions', error)
        setRunMessage(`执行记录加载失败: ${String(error)}`)
      })
  }, [selectedTaskId])

  useEffect(() => {
    if (!selectedExecutionId) {
      setSelectedExecution(null)
      return
    }

    requestJson<ExecutionRecord>(`/api/v1/executions/${selectedExecutionId}`)
      .then((data) => setSelectedExecution(data))
      .catch((error) => {
        console.error('failed to load execution detail', error)
        setRunMessage(`执行详情加载失败: ${String(error)}`)
      })
  }, [selectedExecutionId])

  useEffect(() => {
    if (selectedSkillId || tools.skills.length === 0) {
      return
    }
    setSelectedSkillId(tools.skills[0].id)
    setSelectedSkillScript(tools.skills[0].scripts[0]?.name ?? '')
  }, [selectedSkillId, tools.skills])

  const statValues = useMemo(() => {
    if (!overview) {
      return {
        running: '0',
        queue: '0',
        sessions: '0',
        memory: '0',
      }
    }

    return {
      running: String(overview.scheduler.running),
      queue: String(overview.scheduler.queue_depth),
      sessions: String(overview.sessions.total),
      memory: String(overview.memory.total),
    }
  }, [overview])

  const selectedTask = tasks.find((task) => task.id === selectedTaskId) ?? null
  const selectedSkill = tools.skills.find((skill) => skill.id === selectedSkillId) ?? null
  const queuedTasks = useMemo(
    () =>
      (schedulerStatus?.queued_task_ids ?? [])
        .map((id) => tasks.find((task) => task.id === id))
        .filter(Boolean) as Task[],
    [schedulerStatus, tasks],
  )
  const runningTasks = useMemo(
    () =>
      (schedulerStatus?.running_task_ids ?? [])
        .map((id) => tasks.find((task) => task.id === id))
        .filter(Boolean) as Task[],
    [schedulerStatus, tasks],
  )

  const handleCreateTask = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setBusy(true)
    setTaskMessage('')

    try {
      const task = await requestJson<Task>('/api/v1/tasks', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          title: taskForm.title,
          description: taskForm.description,
          priority: taskForm.priority,
          sandbox_profile: taskForm.sandbox_profile,
          command: {
            program: taskForm.program,
            args: ['-lc', taskForm.script],
          },
          working_dir: taskForm.working_dir,
        }),
      })
      setTaskMessage(`任务 ${task.title} 已创建`)
      setSelectedTaskId(task.id)
      await refreshDashboard(task.id)
    } catch (error) {
      setTaskMessage(`创建失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleRunTask = async (taskId: string) => {
    setBusy(true)
    setRunMessage('')

    try {
      const receipt = await requestJson<TaskReceipt>(`/api/v1/tasks/${taskId}/run`, { method: 'POST' })
      setRunMessage(`执行已提交: ${receipt.message}`)
      await refreshDashboard(taskId)
    } catch (error) {
      setRunMessage(`执行失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleCancelTask = async (taskId: string) => {
    setBusy(true)
    setRunMessage('')

    try {
      const receipt = await requestJson<TaskReceipt>(`/api/v1/tasks/${taskId}/cancel`, { method: 'POST' })
      setRunMessage(`取消请求已发送: ${receipt.message}`)
      await refreshDashboard(taskId)
    } catch (error) {
      setRunMessage(`取消失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleCreateMemory = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setBusy(true)
    setMemoryMessage('')

    try {
      const memory = await requestJson<Memory>('/api/v1/memories', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          title: memoryForm.title,
          content: memoryForm.content,
          scope: memoryForm.scope,
          tags: memoryForm.tags
            .split(',')
            .map((item) => item.trim())
            .filter(Boolean),
        }),
      })
      setMemoryMessage(`已写入记忆: ${memory.title}`)
      await refreshDashboard(selectedTaskId)
    } catch (error) {
      setMemoryMessage(`写入失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleSearchSessions = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setBusy(true)
    setSessionMessage('')

    try {
      const results = await requestJson<SessionSearchResult[]>('/api/v1/sessions/search', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ query: sessionQuery, limit: 6 }),
      })
      setSessionResults(results)
      setSessionMessage(results.length === 0 ? '没有命中会话消息。' : `命中 ${results.length} 条会话消息`)
    } catch (error) {
      setSessionMessage(`检索失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleRouteModel = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setBusy(true)
    setRouteMessage('')

    try {
      const decision = await requestJson<ModelRouteDecision>('/api/v1/models/route', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ capability: routeCapability, prefer_local: preferLocal }),
      })
      setRouteDecision(decision)
      setRouteMessage(`已为 ${routeCapability} 能力完成路由`) 
    } catch (error) {
      setRouteMessage(`路由失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleSetDefaultModel = async (modelId: string) => {
    setBusy(true)
    setRouteMessage('')

    try {
      await requestJson<Model[]>('/api/v1/models/default', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model_id: modelId }),
      })
      setRouteMessage(`默认模型已切换为 ${modelId}`)
      await refreshDashboard(selectedTaskId)
    } catch (error) {
      setRouteMessage(`切换失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleRunSkill = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!selectedSkillId) {
      setSkillMessage('先选择一个 Skill。')
      return
    }

    setBusy(true)
    setSkillMessage('')

    try {
      const result = await requestJson<SkillExecutionResult>(`/api/v1/skills/${encodeURIComponent(selectedSkillId)}/run`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          script_name: selectedSkillScript || undefined,
          args: skillArgs.split(' ').map((item) => item.trim()).filter(Boolean),
          sandbox_profile: 'workspace-write',
          working_dir: '/root/space',
        }),
      })
      setSkillResult(result)
      setSkillMessage(`Skill ${result.skill.id} 已执行`)
      setSelectedTaskId(result.task.id)
      await refreshDashboard(result.task.id)
    } catch (error) {
      setSkillMessage(`Skill 执行失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleHermesChat = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!hermesPrompt.trim()) {
      setHermesMessage('先输入一条 Hermes 指令。')
      return
    }

    setBusy(true)
    setHermesMessage('')

    try {
      const result = await requestJson<HermesAgentResponse>('/api/v1/agent/hermes/chat', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          session_id: hermesSessionId || undefined,
          title: 'Hermes Replica Session',
          working_dir: '/root/space',
          message: hermesPrompt,
          auto_persist_memory: true,
        }),
      })
      setHermesResult(result)
      setHermesSessionId(result.session.id)
      setHermesMessage(`Hermes loop 已完成，使用模型 ${result.routed_model.selected.id}`)
      await refreshDashboard(selectedTaskId)
    } catch (error) {
      setHermesMessage(`Hermes 调用失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  const handlePromoteSkillCandidate = async (candidateId: string, taskId?: string, clusterKey?: string) => {
    setBusy(true)
    setLearningMessage('')

    try {
      const result = await requestJson<PromotedSkillResult>('/api/v1/skills/promote', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          task_id: taskId || undefined,
          cluster_key: clusterKey || undefined,
          candidate_id: candidateId,
          working_dir: '/root/space',
        }),
      })
      setLearningMessage(`已晋升 Skill ${result.skill.id}`)
      await refreshDashboard(selectedTaskId)
    } catch (error) {
      setLearningMessage(`Skill 晋升失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  if (loading) {
    return <main className="loading-shell">Booting AgentOS...</main>
  }

  return (
    <main className="app-shell">
      {dashboardError ? (
        <section className="alert-banner" role="alert">
          <strong>Dashboard Error</strong>
          <span>{dashboardError}</span>
          <button
            className="secondary-button"
            onClick={() =>
              refreshDashboard(selectedTaskId).catch((error) =>
                setDashboardError(`刷新失败: ${String(error)}`),
              )
            }
            type="button"
          >
            重试
          </button>
        </section>
      ) : null}

      <section className="hero-panel">
        <div className="hero-copy">
          <span className="eyebrow">Single-node agent kernel</span>
          <h1>AgentOS</h1>
          <p>参考 Hermes 的会话搜索、技能发现和模型路由思路，把任务调度、长期记忆与可审计执行收敛到一个本地节点。</p>
        </div>
        <div className="hero-orbit">
          <div className="ring ring-a" />
          <div className="ring ring-b" />
          <div className="core-card">
            <span>Node</span>
            <strong>{overview?.node_name ?? 'agentos-local-node'}</strong>
            <small>local-first / searchable / skill-aware</small>
          </div>
        </div>
      </section>

      <section className="stats-grid">
        {statLabels.map((item) => (
          <article key={item.key} className="stat-card">
            <span>{item.label}</span>
            <strong>{statValues[item.key]}</strong>
          </article>
        ))}
      </section>

      <section className="content-grid">
        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Runtime Lanes</h2>
            <span>任务 / 会话 / 记忆</span>
          </div>
          <div className="lane-grid">
            <div className="lane-card accent-red">
              <h3>Scheduler</h3>
              <p>
                并发上限 {schedulerStatus?.max_concurrent_tasks ?? overview?.scheduler.max_concurrent_tasks ?? 0}
                {' / '}
                剩余槽位 {schedulerStatus?.available_slots ?? 0}
              </p>
              {runningTasks.length === 0 ? (
                <div className="line-item">
                  <strong>无运行中任务</strong>
                  <span>当前可以直接起跑新的任务</span>
                </div>
              ) : (
                runningTasks.map((task) => (
                  <button
                    key={task.id}
                    className={`line-item action-line${selectedTaskId === task.id ? ' selected' : ''}`}
                    onClick={() => setSelectedTaskId(task.id)}
                    type="button"
                  >
                    <strong>{task.title}</strong>
                    <span>RUNNING / {task.priority}</span>
                  </button>
                ))
              )}
              {queuedTasks.slice(0, 3).map((task) => (
                <button
                  key={task.id}
                  className={`line-item action-line queue-item${selectedTaskId === task.id ? ' selected' : ''}`}
                  onClick={() => setSelectedTaskId(task.id)}
                  type="button"
                >
                  <strong>{task.title}</strong>
                  <span>QUEUED / {task.priority}</span>
                </button>
              ))}
            </div>
            <div className="lane-card accent-gold">
              <h3>Session Window</h3>
              <p>窗口保留 {overview?.sessions.window_size} 条消息</p>
              {sessions.slice(0, 2).map((session) => (
                <div key={session.id} className="line-item">
                  <strong>{session.title}</strong>
                  <span>{session.summary.compressed_context}</span>
                </div>
              ))}
            </div>
            <div className="lane-card accent-blue">
              <h3>Memory Mesh</h3>
              <p>检索限制 {overview?.memory.search_limit} 条</p>
              {memories.slice(0, 3).map((memory) => (
                <div key={memory.id} className="line-item">
                  <strong>{memory.title}</strong>
                  <span>{memory.scope}</span>
                </div>
              ))}
            </div>
            <div className="lane-card accent-gold">
              <h3>Context Files</h3>
              <p>Codex / Hermes 工作区上下文</p>
              {workspaceContexts.length === 0 ? (
                <div className="line-item">
                  <strong>无上下文文件</strong>
                  <span>可添加 `AGENTS.md` / `SOUL.md` / `MEMORY.md`</span>
                </div>
              ) : (
                workspaceContexts.slice(0, 3).map((context) => (
                  <div key={context.path} className="line-item">
                    <strong>{context.title}</strong>
                    <span>{context.kind} / {context.path}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Interactive Console</h2>
            <span>{busy ? '处理中...' : '可直接创建和执行'}</span>
          </div>
          <div className="console-grid">
            <form className="action-form" onSubmit={handleCreateTask}>
              <h3>Create Task</h3>
              <label>
                标题
                <input value={taskForm.title} onChange={(event) => setTaskForm({ ...taskForm, title: event.target.value })} />
              </label>
              <label>
                描述
                <textarea value={taskForm.description} onChange={(event) => setTaskForm({ ...taskForm, description: event.target.value })} rows={2} />
              </label>
              <div className="form-row">
                <label>
                  优先级
                  <select value={taskForm.priority} onChange={(event) => setTaskForm({ ...taskForm, priority: event.target.value })}>
                    <option value="HIGH">HIGH</option>
                    <option value="NORMAL">NORMAL</option>
                    <option value="LOW">LOW</option>
                  </select>
                </label>
                <label>
                  沙箱
                  <select value={taskForm.sandbox_profile} onChange={(event) => setTaskForm({ ...taskForm, sandbox_profile: event.target.value })}>
                    <option value="read-only">read-only</option>
                    <option value="workspace-write">workspace-write</option>
                    <option value="tmp-only">tmp-only</option>
                  </select>
                </label>
              </div>
              <p className="muted-copy profile-copy">
                `read-only` 仅允许 `/root/space`；`workspace-write` 面向工作区；`tmp-only` 只允许 `/tmp`
              </p>
              <label>
                程序
                <input value={taskForm.program} onChange={(event) => setTaskForm({ ...taskForm, program: event.target.value })} />
              </label>
              <label>
                脚本
                <textarea value={taskForm.script} onChange={(event) => setTaskForm({ ...taskForm, script: event.target.value })} rows={3} />
              </label>
              <label>
                工作目录
                <input value={taskForm.working_dir} onChange={(event) => setTaskForm({ ...taskForm, working_dir: event.target.value })} />
              </label>
              <button className="primary-button" disabled={busy} type="submit">创建任务</button>
              {taskMessage ? <p className="feedback-text">{taskMessage}</p> : null}
            </form>

            <div className="action-form execution-panel">
              <h3>Run Selected Task</h3>
              {selectedTask ? (
                <>
                  <div className="selected-task-card">
                    <strong>{selectedTask.title}</strong>
                    <span>{selectedTask.status} · {selectedTask.command.program} {selectedTask.command.args.join(' ')}</span>
                    <small>{selectedTask.working_dir}</small>
                    {selectedTask.strategy_sources.length > 0 ? (
                      <small>strategy: {selectedTask.strategy_sources.join(' / ')}</small>
                    ) : null}
                  </div>
                  <div className="button-row">
                    <button className="primary-button" disabled={busy} type="button" onClick={() => handleRunTask(selectedTask.id)}>
                      执行当前任务
                    </button>
                    <button
                      className="primary-button subtle"
                      disabled={busy || selectedTask.status !== 'RUNNING'}
                      type="button"
                      onClick={() => handleCancelTask(selectedTask.id)}
                    >
                      取消运行
                    </button>
                  </div>
                </>
              ) : (
                <p className="muted-copy">先从左侧任务列表里选一个任务。</p>
              )}
              {runMessage ? <p className="feedback-text">{runMessage}</p> : null}

              <div className="execution-list">
                {executions.length === 0 ? (
                  <p className="muted-copy">暂无执行记录</p>
                ) : (
                  executions.slice(0, 3).map((item) => (
                    <button
                      key={item.id}
                      className={`execution-card action-line${selectedExecutionId === item.id ? ' selected' : ''}`}
                      onClick={() => setSelectedExecutionId(item.id)}
                      type="button"
                    >
                      <div className="execution-meta">
                        <strong>{item.status}</strong>
                        <span>{item.duration_ms} ms / exit {String(item.exit_code)}</span>
                      </div>
                      <code>{item.command_line}</code>
                      <pre>{item.stdout || item.stderr || 'no output'}</pre>
                    </button>
                  ))
                )}
              </div>
              <div className="execution-list">
                {selectedExecution ? (
                  <article className="execution-card detail-card">
                    <div className="execution-meta">
                      <strong>execution {selectedExecution.id.slice(0, 8)}</strong>
                      <span>{selectedExecution.working_dir}</span>
                    </div>
                    <code>{selectedExecution.command_line}</code>
                    <pre>{selectedExecution.stdout || selectedExecution.stderr || 'no output'}</pre>
                    <div className="audit-trail">
                      {selectedExecution.audit_log.map((line) => (
                        <small key={line}>{line}</small>
                      ))}
                    </div>
                  </article>
                ) : (
                  <p className="muted-copy">选择一条 execution，可查看完整 audit log。</p>
                )}
              </div>
              <div className="execution-list">
                {learningReports.length === 0 ? (
                  <p className="muted-copy">该任务还没有学习沉淀。</p>
                ) : (
                  learningReports.slice(0, 2).map((report) => (
                    <article key={report.id} className="execution-card">
                      <div className="execution-meta">
                        <strong>learning / {report.status}</strong>
                        <span>{new Date(report.created_at).toLocaleString()}</span>
                      </div>
                      <p>{report.recap}</p>
                      <small>{report.lessons.join(' · ')}</small>
                    </article>
                  ))
                )}
              </div>
            </div>

            <form className="action-form" onSubmit={handleCreateMemory}>
              <h3>Write Memory</h3>
              <label>
                标题
                <input value={memoryForm.title} onChange={(event) => setMemoryForm({ ...memoryForm, title: event.target.value })} />
              </label>
              <label>
                内容
                <textarea value={memoryForm.content} onChange={(event) => setMemoryForm({ ...memoryForm, content: event.target.value })} rows={4} />
              </label>
              <div className="form-row">
                <label>
                  Scope
                  <select value={memoryForm.scope} onChange={(event) => setMemoryForm({ ...memoryForm, scope: event.target.value })}>
                    <option value="SHORT_TERM">SHORT_TERM</option>
                    <option value="LONG_TERM">LONG_TERM</option>
                    <option value="EPISODIC">EPISODIC</option>
                    <option value="SEMANTIC">SEMANTIC</option>
                  </select>
                </label>
                <label>
                  Tags
                  <input value={memoryForm.tags} onChange={(event) => setMemoryForm({ ...memoryForm, tags: event.target.value })} />
                </label>
              </div>
              <button className="primary-button alt" disabled={busy} type="submit">写入记忆</button>
              {memoryMessage ? <p className="feedback-text">{memoryMessage}</p> : null}
            </form>
          </div>
        </article>

        <article className="panel">
          <div className="panel-head">
            <h2>Tool Layer</h2>
            <span>{overview?.tools.hot_reload_enabled ? '热加载已启用' : '热加载关闭'}</span>
          </div>
          <div className="stack-list">
            {tools.tools.map((tool) => (
              <div key={tool.id} className="stack-card">
                <div>
                  <strong>{tool.display_name}</strong>
                  <span>{tool.category}</span>
                </div>
                <p>{tool.permissions.join(' · ')}</p>
              </div>
            ))}
          </div>
        </article>

        <article className="panel">
          <div className="panel-head">
            <h2>Skill Registry</h2>
            <span>{tools.skills.length} installed</span>
          </div>
          <div className="stack-list compact">
            {tools.skills.map((skill) => (
              <button
                key={skill.id}
                className={`stack-card action-line${selectedSkillId === skill.id ? ' selected' : ''}`}
                onClick={() => {
                  setSelectedSkillId(skill.id)
                  setSelectedSkillScript(skill.scripts[0]?.name ?? '')
                }}
                type="button"
              >
                <div>
                  <strong>{skill.id}</strong>
                  <span>{skill.source}</span>
                </div>
                <p>{skill.description}</p>
                <small>{skill.scripts.length > 0 ? `${skill.scripts.length} scripts · ${skill.path}` : `no scripts · ${skill.path}`}</small>
              </button>
            ))}
          </div>
        </article>

        <article className="panel">
          <div className="panel-head">
            <h2>Skill Runner</h2>
            <span>{selectedSkill ? selectedSkill.id : '选择左侧 Skill'}</span>
          </div>
          <form className="action-form" onSubmit={handleRunSkill}>
            {selectedSkill ? (
              <>
                <div className="selected-task-card">
                  <strong>{selectedSkill.id}</strong>
                  <span>{selectedSkill.description}</span>
                  <small>{selectedSkill.path}</small>
                </div>
                <label>
                  脚本
                  <select value={selectedSkillScript} onChange={(event) => setSelectedSkillScript(event.target.value)}>
                    {selectedSkill.scripts.length === 0 ? (
                      <option value="">no runnable scripts</option>
                    ) : (
                      selectedSkill.scripts.map((script) => (
                        <option key={script.name} value={script.name}>
                          {script.name} / {script.runner}
                        </option>
                      ))
                    )}
                  </select>
                </label>
                <label>
                  参数
                  <input placeholder="例如: my-plugin --with-skills" value={skillArgs} onChange={(event) => setSkillArgs(event.target.value)} />
                </label>
                <button className="primary-button alt" disabled={busy || selectedSkill.scripts.length === 0} type="submit">
                  执行 Skill
                </button>
              </>
            ) : (
              <p className="muted-copy">从 Skill Registry 里选择一个可执行 Skill。</p>
            )}
            {skillMessage ? <p className="feedback-text">{skillMessage}</p> : null}
            {skillResult ? (
              <article className="execution-card">
                <div className="execution-meta">
                  <strong>{skillResult.execution.status}</strong>
                  <span>{skillResult.selected_script.runner} / {skillResult.execution.duration_ms} ms</span>
                </div>
                <code>{skillResult.execution.command_line}</code>
                <pre>{skillResult.execution.stdout || skillResult.execution.stderr || 'no output'}</pre>
              </article>
            ) : null}
          </form>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Hermes Replica</h2>
            <span>{hermesSessionId ? `session ${hermesSessionId.slice(0, 8)}` : '本地 agent loop'}</span>
          </div>
          <form className="action-form" onSubmit={handleHermesChat}>
            <label>
              指令
              <textarea value={hermesPrompt} onChange={(event) => setHermesPrompt(event.target.value)} rows={4} />
            </label>
            <button className="primary-button" disabled={busy} type="submit">运行 Hermes Loop</button>
            {hermesMessage ? <p className="feedback-text">{hermesMessage}</p> : null}
          </form>
          {hermesResult ? (
            <div className="console-grid">
              <article className="execution-card">
                <div className="execution-meta">
                  <strong>{hermesResult.routed_model.selected.id}</strong>
                  <span>{hermesResult.routed_model.reason}</span>
                </div>
                <pre>{hermesResult.assistant_message}</pre>
              </article>
              <div className="stack-list compact">
                {hermesResult.workspace_contexts.map((context) => (
                  <div key={context.path} className="stack-card">
                    <div>
                      <strong>{context.title}</strong>
                      <span>{context.kind}</span>
                    </div>
                    <p>{context.guidance.join(' · ') || context.excerpt}</p>
                    <small>{context.path}</small>
                  </div>
                ))}
              </div>
              <div className="stack-list compact">
                {hermesResult.strategic_clusters.map((cluster) => (
                  <div key={cluster.key} className="stack-card">
                    <div>
                      <strong>{cluster.title}</strong>
                      <span>{cluster.capability} / {Math.round(cluster.success_rate * 100)}%</span>
                    </div>
                    <p>{cluster.common_lessons.join(' · ')}</p>
                    <small>{cluster.report_count} reports · weight {cluster.strategic_weight.toFixed(1)}</small>
                    <small>source feedback: {cluster.source_usage_count} uses / {Math.round(cluster.source_success_rate * 100)}% success</small>
                    <small>state: {cluster.suppression_level}{cluster.pruned_from_planning ? ' / pruned' : ''}</small>
                  </div>
                ))}
              </div>
              <div className="stack-list compact">
                {hermesResult.actions.map((action) => (
                  <div key={`${action.kind}-${action.title}`} className="stack-card">
                    <div>
                      <strong>{action.title}</strong>
                      <span>{action.kind}</span>
                    </div>
                    <p>{action.detail}</p>
                  </div>
                ))}
              </div>
              <div className="stack-list compact">
                <div className="stack-card">
                  <div>
                    <strong>Strategy Trace</strong>
                    <span>provenance</span>
                  </div>
                  <p>
                    {hermesResult.strategy_trace.response_sources.length > 0
                      ? hermesResult.strategy_trace.response_sources.join(' · ')
                      : 'no response-level strategy sources'}
                  </p>
                  <small>
                    {hermesResult.strategy_trace.task_sources.length > 0
                      ? hermesResult.strategy_trace.task_sources
                          .map((item) => `${item.task_title}: ${item.strategy_sources.join(', ') || 'none'}`)
                          .join(' || ')
                      : 'no task-level strategy sources'}
                  </small>
                </div>
              </div>
              <div className="stack-list compact">
                {hermesResult.tool_trace.map((event) => (
                  <div key={`${event.tool}-${event.detail}`} className="stack-card">
                    <div>
                      <strong>{event.tool}</strong>
                      <span>tool trace</span>
                    </div>
                    <p>{event.detail}</p>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Learning Loop</h2>
            <span>task recap / memory / skill candidates</span>
          </div>
          {learningMessage ? <p className="feedback-text">{learningMessage}</p> : null}
          <div className="console-grid">
            {learningReports.length === 0 ? (
              <div className="stack-card">
                <div>
                  <strong>暂无自学习结果</strong>
                  <span>autopilot</span>
                </div>
                <p>任务执行完成后，AgentOS 会自动写入 episodic memory、更新 recap session，并生成 skill 候选。</p>
              </div>
            ) : (
              learningReports.slice(0, 3).map((report) => (
                <div key={report.id} className="stack-card">
                  <div>
                    <strong>{report.status}</strong>
                    <span>{new Date(report.created_at).toLocaleString()}</span>
                  </div>
                  <p>{report.recap}</p>
                  <small>{report.lessons.join(' · ')}</small>
                  {report.source_strategy_keys.length > 0 ? (
                    <small>sources: {report.source_strategy_keys.join(' / ')}</small>
                  ) : null}
                  {report.skill_candidates.length > 0 ? (
                    <div className="stack-list compact">
                      {report.skill_candidates.map((item) => (
                        <div key={item.id} className="selected-task-card">
                          <strong>{item.title}</strong>
                          <span>{item.suggested_trigger}</span>
                          <small>{item.rationale}</small>
                          <button
                            className="secondary-button"
                            disabled={busy}
                            onClick={() => handlePromoteSkillCandidate(item.id, report.task_id)}
                            type="button"
                          >
                            晋升为 Skill
                          </button>
                        </div>
                      ))}
                    </div>
                  ) : null}
                </div>
              ))
            )}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Strategic Learning</h2>
            <span>{learningSummary ? `${learningSummary.total_reports} reports clustered` : 'clustered experience'}</span>
          </div>
          <div className="stack-list compact">
            {!learningSummary || learningSummary.clusters.length === 0 ? (
              <div className="stack-card">
                <div>
                  <strong>暂无长期经验簇</strong>
                  <span>strategic memory</span>
                </div>
                <p>当相似任务累计两次以上后，AgentOS 会把 learning reports 聚类，生成更高层的长期经验与战略 Skill 候选。</p>
              </div>
            ) : (
              learningSummary.clusters.slice(0, 6).map((cluster) => (
                <div key={cluster.key} className="stack-card">
                  <div>
                    <strong>{cluster.title}</strong>
                    <span>{cluster.capability} / {Math.round(cluster.success_rate * 100)}%</span>
                  </div>
                  <p>{cluster.common_lessons.join(' · ')}</p>
                  <small>
                    {cluster.report_count} reports · weight {cluster.strategic_weight.toFixed(1)} · recency {cluster.recency_score.toFixed(2)}
                  </small>
                  <small>
                    source feedback: {cluster.source_usage_count} uses / {Math.round(cluster.source_success_rate * 100)}% success
                  </small>
                  <small>state: {cluster.suppression_level}{cluster.pruned_from_planning ? ' / pruned' : ''}</small>
                  <small>tasks: {cluster.example_tasks.join(' / ')}</small>
                  {cluster.recommended_commands.length > 0 ? (
                    <small>Commands: {cluster.recommended_commands.join(' || ')}</small>
                  ) : null}
                  {cluster.strategic_skill_candidates.map((candidate) => (
                    <div key={candidate.id} className="selected-task-card">
                      <strong>{candidate.title}</strong>
                      <span>{candidate.suggested_trigger}</span>
                      <small>{candidate.rationale}</small>
                      <button
                        className="secondary-button"
                        disabled={busy}
                        onClick={() => handlePromoteSkillCandidate(candidate.id, undefined, cluster.key)}
                        type="button"
                      >
                        晋升战略 Skill
                      </button>
                    </div>
                  ))}
                </div>
              ))
            )}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Strategy Timeline</h2>
            <span>{strategyTimeline ? `${strategyTimeline.total_events} evaluation events` : 'evaluation events'}</span>
          </div>
          <div className="stack-list compact">
            {!strategyTimeline || strategyTimeline.events.length === 0 ? (
              <div className="stack-card">
                <div>
                  <strong>暂无策略评估事件</strong>
                  <span>timeline</span>
                </div>
                <p>任务执行采用 `strategy_sources` 后，这里会记录哪些经验簇被验证、证伪或持续命中。</p>
              </div>
            ) : (
              strategyTimeline.events.map((event) => (
                <div key={event.id} className="stack-card">
                  <div>
                    <strong>{event.strategy_source_key}</strong>
                    <span>{event.outcome_status}</span>
                  </div>
                  <p>{event.summary}</p>
                  <small>{event.evidence}</small>
                  <small>{new Date(event.created_at).toLocaleString()}</small>
                </div>
              ))
            )}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Workspace Context</h2>
            <span>Codex AGENTS.md + Hermes SOUL/MEMORY</span>
          </div>
          <div className="stack-list compact">
            {workspaceContexts.length === 0 ? (
              <div className="stack-card">
                <div>
                  <strong>暂无工作区上下文</strong>
                  <span>context layer</span>
                </div>
                <p>在工作区根目录放置 `AGENTS.md`、`SOUL.md`、`MEMORY.md` 或 `USER.md` 后，这里会自动被 AgentOS 发现并注入到 Hermes Loop。</p>
              </div>
            ) : (
              workspaceContexts.map((context) => (
                <div key={context.path} className="stack-card">
                  <div>
                    <strong>{context.title}</strong>
                    <span>{context.kind}</span>
                  </div>
                  <p>{context.excerpt || context.guidance.join(' · ')}</p>
                  <small>{context.path}</small>
                </div>
              ))
            )}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Session Search</h2>
            <span>FTS + LIKE fallback</span>
          </div>
          <form className="search-bar" onSubmit={handleSearchSessions}>
            <input placeholder="检索历史消息，如：技能发现 / 本地执行 / 模型路由" value={sessionQuery} onChange={(event) => setSessionQuery(event.target.value)} />
            <button className="primary-button alt" disabled={busy || !sessionQuery.trim()} type="submit">搜索</button>
          </form>
          {sessionMessage ? <p className="feedback-text">{sessionMessage}</p> : null}
          <div className="stack-list compact">
            {sessionResults.map((result) => (
              <div key={`${result.session_id}-${result.created_at}`} className="stack-card">
                <div>
                  <strong>{result.title}</strong>
                  <span>{result.role} / {result.score.toFixed(2)}</span>
                </div>
                <p>{result.excerpt}</p>
                <small>{result.working_dir}</small>
              </div>
            ))}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Model Router</h2>
            <span>本地优先，多模型回退</span>
          </div>
          <form className="router-toolbar" onSubmit={handleRouteModel}>
            <select value={routeCapability} onChange={(event) => setRouteCapability(event.target.value)}>
              <option value="chat">chat</option>
              <option value="code">code</option>
              <option value="summarize">summarize</option>
              <option value="planning">planning</option>
              <option value="tools">tools</option>
            </select>
            <label className="toggle-line">
              <input checked={preferLocal} onChange={(event) => setPreferLocal(event.target.checked)} type="checkbox" />
              优先本地模型
            </label>
            <button className="primary-button" disabled={busy} type="submit">执行路由</button>
          </form>
          {routeMessage ? <p className="feedback-text">{routeMessage}</p> : null}
          {routeDecision ? (
            <div className="route-decision">
              <div className="selected-task-card">
                <strong>{routeDecision.selected.id}</strong>
                <span>{routeDecision.selected.kind} / weight {routeDecision.selected.routing_weight}</span>
                <small>{routeDecision.reason}</small>
              </div>
              {routeDecision.fallbacks.length > 0 ? (
                <p className="muted-copy">Fallbacks: {routeDecision.fallbacks.map((model) => model.id).join(' -> ')}</p>
              ) : null}
            </div>
          ) : null}
          <div className="model-grid">
            {overview?.models.map((model) => (
              <div key={model.id} className={`model-card${model.is_default ? ' active' : ''}`}>
                <span>{model.kind}</span>
                <strong>{model.id}</strong>
                <p>{model.endpoint}</p>
                <small>{model.capabilities.join(' / ')} / weight {model.routing_weight}</small>
                <button className="secondary-button" disabled={busy || model.is_default} onClick={() => handleSetDefaultModel(model.id)} type="button">
                  {model.is_default ? '当前默认' : '设为默认'}
                </button>
              </div>
            ))}
          </div>
        </article>
      </section>
    </main>
  )
}

export default App
