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

type Memory = {
  id: string
  title: string
  content: string
  scope: string
  tags: string[]
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
}

type Model = {
  id: string
  kind: string
  endpoint: string
  capabilities: string[]
  is_default: boolean
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

function App() {
  const [overview, setOverview] = useState<Overview | null>(null)
  const [tools, setTools] = useState<{ tools: Tool[]; skills: Skill[] }>({ tools: [], skills: [] })
  const [tasks, setTasks] = useState<Task[]>([])
  const [sessions, setSessions] = useState<Session[]>([])
  const [memories, setMemories] = useState<Memory[]>([])
  const [selectedTaskId, setSelectedTaskId] = useState<string>('')
  const [executions, setExecutions] = useState<ExecutionRecord[]>([])
  const [taskForm, setTaskForm] = useState(initialTaskForm)
  const [memoryForm, setMemoryForm] = useState(initialMemoryForm)
  const [taskMessage, setTaskMessage] = useState('')
  const [memoryMessage, setMemoryMessage] = useState('')
  const [runMessage, setRunMessage] = useState('')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  const refreshDashboard = async (preserveTaskId?: string) => {
    const [overviewRes, toolsRes, tasksRes, sessionsRes, memoriesRes] = await Promise.all([
      fetch('/api/v1/overview'),
      fetch('/api/v1/tools'),
      fetch('/api/v1/tasks'),
      fetch('/api/v1/sessions'),
      fetch('/api/v1/memories'),
    ])

    const [overviewJson, toolsJson, tasksJson, sessionsJson, memoriesJson] = await Promise.all([
      overviewRes.json(),
      toolsRes.json(),
      tasksRes.json(),
      sessionsRes.json(),
      memoriesRes.json(),
    ])

    setOverview(overviewJson)
    setTools(toolsJson)
    setTasks(tasksJson)
    setSessions(sessionsJson)
    setMemories(memoriesJson)

    const nextTaskId = preserveTaskId || selectedTaskId || tasksJson[0]?.id || ''
    if (nextTaskId) {
      setSelectedTaskId(nextTaskId)
      const executionRes = await fetch(`/api/v1/tasks/${nextTaskId}/executions`)
      if (executionRes.ok) {
        setExecutions(await executionRes.json())
      }
    } else {
      setExecutions([])
    }
  }

  useEffect(() => {
    refreshDashboard()
      .catch((error) => {
        console.error('failed to load dashboard', error)
      })
      .finally(() => setLoading(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (!selectedTaskId) {
      return
    }

    fetch(`/api/v1/tasks/${selectedTaskId}/executions`)
      .then((response) => response.json())
      .then((data) => setExecutions(data))
      .catch((error) => console.error('failed to load executions', error))
  }, [selectedTaskId])

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

  const handleCreateTask = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setBusy(true)
    setTaskMessage('')

    try {
      const response = await fetch('/api/v1/tasks', {
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

      if (!response.ok) {
        throw new Error(await response.text())
      }

      const task: Task = await response.json()
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
      const response = await fetch(`/api/v1/tasks/${taskId}/run`, { method: 'POST' })
      if (!response.ok) {
        throw new Error(await response.text())
      }

      const receipt: TaskReceipt = await response.json()
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
      const response = await fetch(`/api/v1/tasks/${taskId}/cancel`, { method: 'POST' })
      if (!response.ok) {
        throw new Error(await response.text())
      }

      const receipt: TaskReceipt = await response.json()
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
      const response = await fetch('/api/v1/memories', {
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

      if (!response.ok) {
        throw new Error(await response.text())
      }

      const memory: Memory = await response.json()
      setMemoryMessage(`已写入记忆: ${memory.title}`)
      await refreshDashboard(selectedTaskId)
    } catch (error) {
      setMemoryMessage(`写入失败: ${String(error)}`)
    } finally {
      setBusy(false)
    }
  }

  if (loading) {
    return <main className="loading-shell">Booting AgentOS...</main>
  }

  return (
    <main className="app-shell">
      <section className="hero-panel">
        <div className="hero-copy">
          <span className="eyebrow">Single-node agent kernel</span>
          <h1>AgentOS</h1>
          <p>把任务调度、会话压缩、长期记忆、工具热加载和模型路由收敛到一个本地节点里。</p>
        </div>
        <div className="hero-orbit">
          <div className="ring ring-a" />
          <div className="ring ring-b" />
          <div className="core-card">
            <span>Node</span>
            <strong>{overview?.node_name ?? 'agentos-local-node'}</strong>
            <small>local-first / auditable / sandbox-aware</small>
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
              <p>并发上限 {overview?.scheduler.max_concurrent_tasks}</p>
              {tasks.slice(0, 4).map((task) => (
                <button
                  key={task.id}
                  className={`line-item action-line${selectedTaskId === task.id ? ' selected' : ''}`}
                  onClick={() => setSelectedTaskId(task.id)}
                  type="button"
                >
                  <strong>{task.title}</strong>
                  <span>{task.status} / {task.priority}</span>
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
                    <span>{selectedTask.command.program} {selectedTask.command.args.join(' ')}</span>
                    <small>{selectedTask.working_dir}</small>
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
                    <article key={item.id} className="execution-card">
                      <div className="execution-meta">
                        <strong>{item.status}</strong>
                        <span>{item.duration_ms} ms / exit {String(item.exit_code)}</span>
                      </div>
                      <code>{item.command_line}</code>
                      <pre>{item.stdout || item.stderr || 'no output'}</pre>
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
              <div key={skill.id} className="stack-card">
                <div>
                  <strong>{skill.id}</strong>
                  <span>{skill.trigger}</span>
                </div>
                <p>{skill.description}</p>
              </div>
            ))}
          </div>
        </article>

        <article className="panel panel-wide">
          <div className="panel-head">
            <h2>Model Router</h2>
            <span>本地优先，多模型回退</span>
          </div>
          <div className="model-grid">
            {overview?.models.map((model) => (
              <div key={model.id} className={`model-card${model.is_default ? ' active' : ''}`}>
                <span>{model.kind}</span>
                <strong>{model.id}</strong>
                <p>{model.endpoint}</p>
                <small>{model.capabilities.join(' / ')}</small>
              </div>
            ))}
          </div>
        </article>
      </section>
    </main>
  )
}

export default App
