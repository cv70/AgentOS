use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::task;
use uuid::Uuid;

use crate::config::config::StorageConfig;
use crate::domain::learning::{StrategyEvaluationEvent, TaskLearningReport};
use crate::domain::memory::{MemoryEntry, MemorySearchResult};
use crate::domain::session::{AgentSession, MessageRole, SessionSearchResult};
use crate::domain::task::{AgentTask, TaskExecutionRecord};
use crate::error::{AppError, AppResult};

const EMBEDDING_DIM: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub tasks: Vec<AgentTask>,
    pub sessions: Vec<AgentSession>,
    pub memories: Vec<MemoryEntry>,
}

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
    legacy_state_path: PathBuf,
}

impl SqliteStore {
    pub async fn new(config: &StorageConfig) -> AppResult<Self> {
        let data_dir = Path::new(&config.data_dir);
        fs::create_dir_all(data_dir).await?;
        let db_path = data_dir.join(&config.state_file);
        let legacy_state_path = data_dir.join("agentos-state.json");

        let conn = Connection::open(db_path)
            .map_err(|error| AppError::Storage(format!("open sqlite database: {error}")))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            legacy_state_path,
        };
        store.init().await?;
        store.migrate_legacy_json_if_needed().await?;
        Ok(store)
    }

    async fn init(&self) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS tasks (
                        id TEXT PRIMARY KEY,
                        payload TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS task_executions (
                        id TEXT PRIMARY KEY,
                        task_id TEXT NOT NULL,
                        payload TEXT NOT NULL,
                        started_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS task_learning_reports (
                        id TEXT PRIMARY KEY,
                        task_id TEXT NOT NULL,
                        payload TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS strategy_evaluation_events (
                        id TEXT PRIMARY KEY,
                        task_id TEXT NOT NULL,
                        strategy_source_key TEXT NOT NULL,
                        payload TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS sessions (
                        id TEXT PRIMARY KEY,
                        payload TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS session_messages (
                        row_id INTEGER PRIMARY KEY AUTOINCREMENT,
                        message_id TEXT NOT NULL,
                        session_id TEXT NOT NULL,
                        role TEXT NOT NULL,
                        content TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS memories (
                        id TEXT PRIMARY KEY,
                        title TEXT NOT NULL,
                        content TEXT NOT NULL,
                        scope TEXT NOT NULL,
                        tags_json TEXT NOT NULL,
                        embedding_json TEXT NOT NULL,
                        payload TEXT NOT NULL,
                        created_at TEXT NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_task_executions_task_id ON task_executions(task_id, started_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_task_learning_reports_task_id ON task_learning_reports(task_id, created_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_strategy_events_task_id ON strategy_evaluation_events(task_id, created_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_strategy_events_source_key ON strategy_evaluation_events(strategy_source_key, created_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_session_messages_session_id ON session_messages(session_id, created_at DESC);
                    CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at DESC);
                    "#,
                )
                .map_err(|error| AppError::Storage(format!("initialize sqlite schema: {error}")))?;
            guard
                .execute(
                    "CREATE VIRTUAL TABLE IF NOT EXISTS session_messages_fts USING fts5(content, role, session_id UNINDEXED, created_at UNINDEXED)",
                    [],
                )
                .map_err(|error| AppError::Storage(format!("initialize session message fts: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join sqlite init task: {error}")))?
    }

    async fn migrate_legacy_json_if_needed(&self) -> AppResult<()> {
        if !fs::try_exists(&self.legacy_state_path).await? {
            return Ok(());
        }

        let task_count = self.list_tasks().await?.len();
        let session_count = self.list_sessions().await?.len();
        let memory_count = self.list_memories().await?.len();
        if task_count + session_count + memory_count > 0 {
            return Ok(());
        }

        let raw = fs::read_to_string(&self.legacy_state_path).await?;
        let legacy: PersistedState = serde_json::from_str(&raw)?;
        for task in legacy.tasks {
            self.upsert_task(task).await?;
        }
        for session in legacy.sessions {
            self.upsert_session(session).await?;
        }
        for memory in legacy.memories {
            self.upsert_memory(memory).await?;
        }
        Ok(())
    }

    pub async fn list_tasks(&self) -> AppResult<Vec<AgentTask>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || {
            query_payloads::<AgentTask>(
                &conn,
                "SELECT payload FROM tasks ORDER BY created_at DESC",
                "list tasks",
            )
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list tasks task: {error}")))?
    }

    pub async fn upsert_task(&self, task_entry: AgentTask) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&task_entry)?;
            let id = task_entry.id.to_string();
            let created_at = task_entry.created_at.to_rfc3339();
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute(
                    r#"
                    INSERT INTO tasks (id, payload, created_at)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(id) DO UPDATE SET
                        payload = excluded.payload,
                        created_at = excluded.created_at
                    "#,
                    params![id, payload, created_at],
                )
                .map_err(|error| AppError::Storage(format!("upsert task: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join upsert task task: {error}")))?
    }

    pub async fn get_task(&self, task_id: Uuid) -> AppResult<Option<AgentTask>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || {
            query_payload_by_id::<AgentTask>(&conn, "tasks", task_id, "get task")
        })
        .await
        .map_err(|error| AppError::Storage(format!("join get task task: {error}")))?
    }

    pub async fn list_task_executions(&self, task_id: Uuid) -> AppResult<Vec<TaskExecutionRecord>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<Vec<TaskExecutionRecord>> {
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let mut stmt = guard
                .prepare("SELECT payload FROM task_executions WHERE task_id = ?1 ORDER BY started_at DESC")
                .map_err(|error| AppError::Storage(format!("prepare list executions: {error}")))?;
            let rows = stmt
                .query_map([task_id.to_string()], |row| row.get::<_, String>(0))
                .map_err(|error| AppError::Storage(format!("query list executions: {error}")))?;
            let mut items = Vec::new();
            for row in rows {
                let payload = row.map_err(|error| AppError::Storage(format!("read execution row: {error}")))?;
                items.push(serde_json::from_str(&payload)?);
            }
            Ok(items)
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list executions task: {error}")))?
    }

    pub async fn add_task_execution(&self, record: TaskExecutionRecord) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&record)?;
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute(
                    "INSERT INTO task_executions (id, task_id, payload, started_at) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        record.id.to_string(),
                        record.task_id.to_string(),
                        payload,
                        record.started_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| AppError::Storage(format!("insert task execution: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join insert execution task: {error}")))?
    }

    pub async fn list_task_learning_reports(
        &self,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskLearningReport>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<Vec<TaskLearningReport>> {
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let mut stmt = guard
                .prepare(
                    "SELECT payload FROM task_learning_reports WHERE task_id = ?1 ORDER BY created_at DESC",
                )
                .map_err(|error| {
                    AppError::Storage(format!("prepare list learning reports: {error}"))
                })?;
            let rows = stmt
                .query_map([task_id.to_string()], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    AppError::Storage(format!("query list learning reports: {error}"))
                })?;
            let mut items = Vec::new();
            for row in rows {
                let payload =
                    row.map_err(|error| AppError::Storage(format!("read learning row: {error}")))?;
                items.push(serde_json::from_str(&payload)?);
            }
            Ok(items)
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list learning reports task: {error}")))?
    }

    pub async fn add_task_learning_report(&self, report: TaskLearningReport) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&report)?;
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute(
                    "INSERT INTO task_learning_reports (id, task_id, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        report.id.to_string(),
                        report.task_id.to_string(),
                        payload,
                        report.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| AppError::Storage(format!("insert task learning report: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join insert learning report task: {error}")))?
    }

    pub async fn list_all_task_learning_reports(&self) -> AppResult<Vec<TaskLearningReport>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<Vec<TaskLearningReport>> {
            query_payloads::<TaskLearningReport>(
                &conn,
                "SELECT payload FROM task_learning_reports ORDER BY created_at DESC",
                "list all learning reports",
            )
        })
        .await
        .map_err(|error| {
            AppError::Storage(format!("join list all learning reports task: {error}"))
        })?
    }

    pub async fn add_strategy_evaluation_event(
        &self,
        event: StrategyEvaluationEvent,
    ) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&event)?;
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute(
                    "INSERT INTO strategy_evaluation_events (id, task_id, strategy_source_key, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        event.id.to_string(),
                        event.task_id.to_string(),
                        event.strategy_source_key,
                        payload,
                        event.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| AppError::Storage(format!("insert strategy event: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join insert strategy event task: {error}")))?
    }

    pub async fn list_strategy_evaluation_events(
        &self,
        limit: usize,
    ) -> AppResult<Vec<StrategyEvaluationEvent>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<Vec<StrategyEvaluationEvent>> {
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let mut stmt = guard
                .prepare(
                    "SELECT payload FROM strategy_evaluation_events ORDER BY created_at DESC LIMIT ?1",
                )
                .map_err(|error| {
                    AppError::Storage(format!("prepare list strategy events: {error}"))
                })?;
            let rows = stmt
                .query_map([limit as i64], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    AppError::Storage(format!("query list strategy events: {error}"))
                })?;
            let mut items = Vec::new();
            for row in rows {
                let payload =
                    row.map_err(|error| AppError::Storage(format!("read strategy row: {error}")))?;
                items.push(serde_json::from_str(&payload)?);
            }
            Ok(items)
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list strategy events task: {error}")))?
    }

    pub async fn list_sessions(&self) -> AppResult<Vec<AgentSession>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || {
            query_payloads::<AgentSession>(
                &conn,
                "SELECT payload FROM sessions ORDER BY updated_at DESC",
                "list sessions",
            )
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list sessions task: {error}")))?
    }

    pub async fn upsert_session(&self, session_entry: AgentSession) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&session_entry)?;
            let id = session_entry.id.to_string();
            let updated_at = session_entry.updated_at.to_rfc3339();
            let mut guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let tx = guard.transaction().map_err(|error| {
                AppError::Storage(format!("begin session transaction: {error}"))
            })?;
            tx.execute(
                r#"
                    INSERT INTO sessions (id, payload, updated_at)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(id) DO UPDATE SET
                        payload = excluded.payload,
                        updated_at = excluded.updated_at
                    "#,
                params![id, payload, updated_at],
            )
            .map_err(|error| AppError::Storage(format!("upsert session: {error}")))?;
            tx.execute(
                "DELETE FROM session_messages WHERE session_id = ?1",
                [session_entry.id.to_string()],
            )
            .map_err(|error| AppError::Storage(format!("clear session messages: {error}")))?;
            tx.execute(
                "DELETE FROM session_messages_fts WHERE session_id = ?1",
                [session_entry.id.to_string()],
            )
            .map_err(|error| AppError::Storage(format!("clear session message fts: {error}")))?;

            for message in &session_entry.messages {
                let role = serde_json::to_string(&message.role)?;
                tx.execute(
                    r#"
                    INSERT INTO session_messages (message_id, session_id, role, content, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    "#,
                    params![
                        message.id.to_string(),
                        session_entry.id.to_string(),
                        &role,
                        &message.content,
                        message.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| AppError::Storage(format!("insert session message: {error}")))?;
                tx.execute(
                    r#"
                    INSERT INTO session_messages_fts (content, role, session_id, created_at)
                    VALUES (?1, ?2, ?3, ?4)
                    "#,
                    params![
                        &message.content,
                        &role,
                        session_entry.id.to_string(),
                        message.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| {
                    AppError::Storage(format!("insert session message fts row: {error}"))
                })?;
            }

            tx.commit().map_err(|error| {
                AppError::Storage(format!("commit session transaction: {error}"))
            })?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join upsert session task: {error}")))?
    }

    pub async fn get_session(&self, session_id: Uuid) -> AppResult<Option<AgentSession>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || {
            query_payload_by_id::<AgentSession>(&conn, "sessions", session_id, "get session")
        })
        .await
        .map_err(|error| AppError::Storage(format!("join get session task: {error}")))?
    }

    pub async fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> AppResult<Vec<SessionSearchResult>> {
        let conn = self.conn.clone();
        let query = query.trim().to_string();
        task::spawn_blocking(move || -> AppResult<Vec<SessionSearchResult>> {
            if query.is_empty() {
                return Ok(Vec::new());
            }

            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let mut results = Vec::new();

            let fts_query = format!("\"{}\"", query.replace('\"', " "));
            let mut stmt = guard
                .prepare(
                    r#"
                    SELECT s.payload, sm.role, sm.content, sm.created_at, bm25(session_messages_fts) AS rank
                    FROM session_messages_fts
                    JOIN session_messages sm ON sm.row_id = session_messages_fts.rowid
                    JOIN sessions s ON s.id = sm.session_id
                    WHERE session_messages_fts MATCH ?1
                    ORDER BY rank
                    LIMIT ?2
                    "#,
                )
                .map_err(|error| AppError::Storage(format!("prepare search sessions: {error}")))?;
            let rows = stmt
                .query_map(params![fts_query, limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, f32>(4)?,
                    ))
                })
                .map_err(|error| AppError::Storage(format!("query search sessions: {error}")))?;

            for row in rows {
                let (payload, role_json, content, created_at, rank) =
                    row.map_err(|error| AppError::Storage(format!("read session search row: {error}")))?;
                let session: AgentSession = serde_json::from_str(&payload)?;
                let role: MessageRole = serde_json::from_str(&role_json)?;
                results.push(SessionSearchResult {
                    session_id: session.id,
                    title: session.title,
                    working_dir: session.working_dir,
                    role,
                    excerpt: build_excerpt(&content, &query),
                    score: 1.0 / (1.0 + rank.abs()),
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                        .map_err(|error| AppError::Storage(format!("parse session message timestamp: {error}")))?
                        .with_timezone(&chrono::Utc),
                });
            }

            if !results.is_empty() {
                return Ok(results);
            }

            let mut fallback_stmt = guard
                .prepare(
                    r#"
                    SELECT s.payload, sm.role, sm.content, sm.created_at
                    FROM session_messages sm
                    JOIN sessions s ON s.id = sm.session_id
                    WHERE sm.content LIKE ?1
                    ORDER BY sm.created_at DESC
                    LIMIT ?2
                    "#,
                )
                .map_err(|error| AppError::Storage(format!("prepare fallback session search: {error}")))?;
            let like_query = format!("%{}%", query);
            let rows = fallback_stmt
                .query_map(params![like_query, limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| AppError::Storage(format!("query fallback session search: {error}")))?;

            for row in rows {
                let (payload, role_json, content, created_at) =
                    row.map_err(|error| AppError::Storage(format!("read fallback session row: {error}")))?;
                let session: AgentSession = serde_json::from_str(&payload)?;
                let role: MessageRole = serde_json::from_str(&role_json)?;
                results.push(SessionSearchResult {
                    session_id: session.id,
                    title: session.title,
                    working_dir: session.working_dir,
                    role,
                    excerpt: build_excerpt(&content, &query),
                    score: 0.5,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                        .map_err(|error| AppError::Storage(format!("parse fallback session timestamp: {error}")))?
                        .with_timezone(&chrono::Utc),
                });
            }

            Ok(results)
        })
        .await
        .map_err(|error| AppError::Storage(format!("join search sessions task: {error}")))?
    }

    pub async fn list_memories(&self) -> AppResult<Vec<MemoryEntry>> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || {
            query_payloads::<MemoryEntry>(
                &conn,
                "SELECT payload FROM memories ORDER BY created_at DESC",
                "list memories",
            )
        })
        .await
        .map_err(|error| AppError::Storage(format!("join list memories task: {error}")))?
    }

    pub async fn upsert_memory(&self, memory_entry: MemoryEntry) -> AppResult<()> {
        let conn = self.conn.clone();
        task::spawn_blocking(move || -> AppResult<()> {
            let payload = serde_json::to_string(&memory_entry)?;
            let embedding = embed_text(&memory_entry.title, &memory_entry.content, &memory_entry.tags);
            let embedding_json = serde_json::to_string(&embedding)?;
            let tags_json = serde_json::to_string(&memory_entry.tags)?;
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            guard
                .execute(
                    r#"
                    INSERT INTO memories (id, title, content, scope, tags_json, embedding_json, payload, created_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        content = excluded.content,
                        scope = excluded.scope,
                        tags_json = excluded.tags_json,
                        embedding_json = excluded.embedding_json,
                        payload = excluded.payload,
                        created_at = excluded.created_at
                    "#,
                    params![
                        memory_entry.id.to_string(),
                        memory_entry.title,
                        memory_entry.content,
                        serde_json::to_string(&memory_entry.scope)?,
                        tags_json,
                        embedding_json,
                        payload,
                        memory_entry.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|error| AppError::Storage(format!("upsert memory: {error}")))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Storage(format!("join upsert memory task: {error}")))?
    }

    pub async fn search_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> AppResult<Vec<MemorySearchResult>> {
        let conn = self.conn.clone();
        let query = query.to_string();
        task::spawn_blocking(move || -> AppResult<Vec<MemorySearchResult>> {
            let query_embedding = embed_query(&query);
            let query_tokens = tokenize(&query);
            let guard = conn
                .lock()
                .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
            let mut stmt = guard
                .prepare("SELECT payload, embedding_json, title, content, tags_json FROM memories")
                .map_err(|error| AppError::Storage(format!("prepare search memories: {error}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(|error| AppError::Storage(format!("query search memories: {error}")))?;

            let mut results = Vec::new();
            for row in rows {
                let (payload, embedding_json, title, content, tags_json) =
                    row.map_err(|error| AppError::Storage(format!("read search row: {error}")))?;
                let memory: MemoryEntry = serde_json::from_str(&payload)?;
                let embedding: Vec<f32> = serde_json::from_str(&embedding_json)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json)?;
                let keyword_hits = query_tokens
                    .iter()
                    .filter(|token| contains_token(&title, &content, &tags, token))
                    .count();
                let score =
                    cosine_similarity(&query_embedding, &embedding) + keyword_hits as f32 * 0.08;
                if score > 0.0 {
                    results.push(MemorySearchResult {
                        memory,
                        score,
                        keyword_hits,
                    });
                }
            }

            results.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(Ordering::Equal)
                    .then(right.keyword_hits.cmp(&left.keyword_hits))
            });
            results.truncate(limit);
            Ok(results)
        })
        .await
        .map_err(|error| AppError::Storage(format!("join search memories task: {error}")))?
    }
}

fn query_payloads<T>(conn: &Arc<Mutex<Connection>>, sql: &str, label: &str) -> AppResult<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let guard = conn
        .lock()
        .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
    let mut stmt = guard
        .prepare(sql)
        .map_err(|error| AppError::Storage(format!("prepare {label}: {error}")))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| AppError::Storage(format!("query {label}: {error}")))?;

    let mut items = Vec::new();
    for row in rows {
        let payload =
            row.map_err(|error| AppError::Storage(format!("read {label} row: {error}")))?;
        items.push(serde_json::from_str(&payload)?);
    }
    Ok(items)
}

fn query_payload_by_id<T>(
    conn: &Arc<Mutex<Connection>>,
    table: &str,
    id: Uuid,
    label: &str,
) -> AppResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let guard = conn
        .lock()
        .map_err(|_| AppError::Storage("sqlite connection mutex poisoned".to_string()))?;
    let sql = format!("SELECT payload FROM {table} WHERE id = ?1");
    let payload: Option<String> = guard
        .query_row(&sql, [id.to_string()], |row| row.get(0))
        .optional()
        .map_err(|error| AppError::Storage(format!("{label}: {error}")))?;
    payload
        .map(|raw| serde_json::from_str(&raw).map_err(AppError::from))
        .transpose()
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | '-'))
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn embed_query(query: &str) -> Vec<f32> {
    embed_text(query, "", &[])
}

fn embed_text(title: &str, content: &str, tags: &[String]) -> Vec<f32> {
    let mut vector = vec![0.0; EMBEDDING_DIM];
    let combined = format!("{title} {content} {}", tags.join(" "));
    for token in tokenize(&combined) {
        let hash = token.bytes().fold(0_u64, |acc, byte| {
            acc.wrapping_mul(131).wrapping_add(byte as u64)
        });
        let index = (hash as usize) % EMBEDDING_DIM;
        vector[index] += 1.0;
    }
    normalize(vector)
}

fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let magnitude = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for value in &mut vector {
            *value /= magnitude;
        }
    }
    vector
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right.iter()).map(|(a, b)| a * b).sum()
}

fn contains_token(title: &str, content: &str, tags: &[String], token: &str) -> bool {
    let token = token.to_lowercase();
    title.to_lowercase().contains(&token)
        || content.to_lowercase().contains(&token)
        || tags.iter().any(|tag| tag.to_lowercase().contains(&token))
}

fn build_excerpt(content: &str, query: &str) -> String {
    let normalized_content = content.trim();
    if normalized_content.is_empty() {
        return "empty message".to_string();
    }

    let content_chars: Vec<char> = normalized_content.chars().collect();
    let lower_content = normalized_content.to_lowercase();
    let lower_query = query.to_lowercase();

    if let Some(index) = lower_content.find(&lower_query) {
        let prefix_chars = lower_content[..index].chars().count();
        let query_chars = lower_query.chars().count();
        let start = prefix_chars.saturating_sub(32);
        let end = (prefix_chars + query_chars + 48).min(content_chars.len());
        let excerpt: String = content_chars[start..end].iter().collect();
        if start > 0 || end < content_chars.len() {
            format!("...{}...", excerpt)
        } else {
            excerpt
        }
    } else {
        content_chars.into_iter().take(88).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use uuid::Uuid;

    use crate::config::config::StorageConfig;
    use crate::domain::memory::{CreateMemoryRequest, MemoryEntry, MemoryScope};
    use crate::domain::session::{
        AgentSession, AppendMessageRequest, CreateSessionRequest, MessageRole,
    };

    use super::{SqliteStore, cosine_similarity, embed_text};

    #[test]
    fn embedding_is_more_similar_for_related_text() {
        let related_a = embed_text("rust runtime", "local scheduler", &["agent".into()]);
        let related_b = embed_text("rust runtime", "task scheduler", &["agent".into()]);
        let unrelated = embed_text("garden", "flower watering", &["plants".into()]);

        assert!(
            cosine_similarity(&related_a, &related_b) > cosine_similarity(&related_a, &unrelated)
        );
    }

    #[tokio::test]
    async fn memory_search_prefers_related_entries() {
        let temp_dir = env::temp_dir().join(format!("agentos-sqlite-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let store = SqliteStore::new(&StorageConfig {
            data_dir: temp_dir.to_string_lossy().to_string(),
            state_file: "memory-test.db".to_string(),
        })
        .await
        .expect("create store");

        store
            .upsert_memory(MemoryEntry::new(CreateMemoryRequest {
                scope: MemoryScope::LongTerm,
                title: "用户偏好".to_string(),
                content: "默认使用中文回复，并优先在本地执行任务。".to_string(),
                tags: vec!["preference".to_string(), "locale".to_string()],
            }))
            .await
            .expect("insert memory");
        store
            .upsert_memory(MemoryEntry::new(CreateMemoryRequest {
                scope: MemoryScope::Semantic,
                title: "园艺笔记".to_string(),
                content: "记录花园浇水和土壤湿度。".to_string(),
                tags: vec!["garden".to_string()],
            }))
            .await
            .expect("insert unrelated memory");

        let results = store
            .search_memories("中文 本地 执行", 5)
            .await
            .expect("search memories");

        assert!(!results.is_empty());
        assert_eq!(results[0].memory.title, "用户偏好");

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn session_search_prefers_matching_messages() {
        let temp_dir = env::temp_dir().join(format!("agentos-session-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let store = SqliteStore::new(&StorageConfig {
            data_dir: temp_dir.to_string_lossy().to_string(),
            state_file: "session-test.db".to_string(),
        })
        .await
        .expect("create store");

        let mut session = AgentSession::new(CreateSessionRequest {
            title: "代码会话".to_string(),
            working_dir: "/root/space".to_string(),
        });
        session.append_message(
            AppendMessageRequest {
                role: MessageRole::User,
                content: "请帮我实现 session 搜索和技能发现".to_string(),
            },
            12,
        );
        store.upsert_session(session).await.expect("insert session");

        let results = store
            .search_sessions("技能发现", 5)
            .await
            .expect("search sessions");
        assert!(!results.is_empty());
        assert_eq!(results[0].title, "代码会话");

        fs::remove_dir_all(temp_dir).expect("cleanup temp dir");
    }
}
