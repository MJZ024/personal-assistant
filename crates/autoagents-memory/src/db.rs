//! SQLite database management.

use rusqlite::{params, Connection, Result as SqlResult, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::MemoryConfig;
use super::task::{TaskRecord, TaskStatus};
use super::preferences::Preference;

/// Thread-safe wrapper around SQLite connection.
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    config: MemoryConfig,
}

impl Database {
    /// Open or create the database with all required tables.
    pub fn open(config: &MemoryConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let path = Path::new(&config.db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode for crash safety and concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            config: config.clone(),
        };

        db.initialize_tables()?;

        Ok(db)
    }

    fn initialize_tables(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER NOT NULL DEFAULT 0,
                agent_type TEXT,
                parent_task_id TEXT,
                progress_pct INTEGER DEFAULT 0,
                blocking_reason TEXT,
                next_action TEXT,
                result_summary TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS task_dependencies (
                task_id TEXT NOT NULL,
                depends_on_task_id TEXT NOT NULL,
                PRIMARY KEY (task_id, depends_on_task_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id),
                FOREIGN KEY (depends_on_task_id) REFERENCES tasks(id)
            );

            CREATE TABLE IF NOT EXISTS preferences (
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT,
                confidence REAL DEFAULT 1.0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (category, key)
            );

            CREATE TABLE IF NOT EXISTS heartbeat_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                event_type TEXT NOT NULL,
                detail TEXT
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                agent_name TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                permission_level TEXT NOT NULL,
                command_or_args TEXT,
                result TEXT,
                user_confirmed INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);
            CREATE INDEX IF NOT EXISTS idx_preferences_cat ON preferences(category);
            ",
        )?;
        Ok(())
    }

    // ── Task Operations ──

    pub fn upsert_task(&self, task: &TaskRecord) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tasks (id, description, status, priority, agent_type, parent_task_id,
             progress_pct, blocking_reason, next_action, result_summary, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'), ?11)
             ON CONFLICT(id) DO UPDATE SET
             description=excluded.description, status=excluded.status,
             priority=excluded.priority, agent_type=excluded.agent_type,
             progress_pct=excluded.progress_pct, blocking_reason=excluded.blocking_reason,
             next_action=excluded.next_action, result_summary=excluded.result_summary,
             updated_at=datetime('now'), completed_at=excluded.completed_at",
            params![
                task.id, task.description, task.status.to_str(), task.priority,
                task.agent_type, task.parent_task_id, task.progress_pct,
                task.blocking_reason, task.next_action, task.result_summary,
                task.completed_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> SqlResult<Option<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, description, status, priority, agent_type, parent_task_id,
             progress_pct, blocking_reason, next_action, result_summary,
             created_at, updated_at, completed_at FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok(TaskRecord {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    status: TaskStatus::from_str(row.get::<_, String>(2)?.as_str()),
                    priority: row.get(3)?,
                    agent_type: row.get(4)?,
                    parent_task_id: row.get(5)?,
                    progress_pct: row.get(6)?,
                    blocking_reason: row.get(7)?,
                    next_action: row.get(8)?,
                    result_summary: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    completed_at: row.get(12)?,
                })
            },
        )
        .optional()
    }

    pub fn get_active_tasks(&self) -> SqlResult<Vec<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, agent_type, parent_task_id,
             progress_pct, blocking_reason, next_action, result_summary,
             created_at, updated_at, completed_at FROM tasks
             WHERE status IN ('pending', 'in_progress', 'waiting_confirmation')
             ORDER BY priority DESC, created_at ASC",
        )?;
        let tasks = stmt.query_map([], |row| {
            Ok(TaskRecord {
                id: row.get(0)?,
                description: row.get(1)?,
                status: TaskStatus::from_str(row.get::<_, String>(2)?.as_str()),
                priority: row.get(3)?,
                agent_type: row.get(4)?,
                parent_task_id: row.get(5)?,
                progress_pct: row.get(6)?,
                blocking_reason: row.get(7)?,
                next_action: row.get(8)?,
                result_summary: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                completed_at: row.get(12)?,
            })
        })?;

        let mut result = Vec::new();
        for task in tasks {
            result.push(task?);
        }
        Ok(result)
    }

    pub fn get_pending_tasks(&self) -> SqlResult<Vec<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, agent_type, parent_task_id,
             progress_pct, blocking_reason, next_action, result_summary,
             created_at, updated_at, completed_at FROM tasks
             WHERE status = 'pending'
             ORDER BY priority DESC, created_at ASC",
        )?;
        let tasks = stmt.query_map([], |row| {
            Ok(TaskRecord {
                id: row.get(0)?,
                description: row.get(1)?,
                status: TaskStatus::from_str(row.get::<_, String>(2)?.as_str()),
                priority: row.get(3)?,
                agent_type: row.get(4)?,
                parent_task_id: row.get(5)?,
                progress_pct: row.get(6)?,
                blocking_reason: row.get(7)?,
                next_action: row.get(8)?,
                result_summary: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                completed_at: row.get(12)?,
            })
        })?;

        let mut result = Vec::new();
        for task in tasks {
            result.push(task?);
        }
        Ok(result)
    }

    pub fn get_in_progress_tasks(&self) -> SqlResult<Vec<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, agent_type, parent_task_id,
             progress_pct, blocking_reason, next_action, result_summary,
             created_at, updated_at, completed_at FROM tasks
             WHERE status = 'in_progress'
             ORDER BY priority DESC, created_at ASC",
        )?;
        let tasks = stmt.query_map([], |row| {
            Ok(TaskRecord {
                id: row.get(0)?,
                description: row.get(1)?,
                status: TaskStatus::from_str(row.get::<_, String>(2)?.as_str()),
                priority: row.get(3)?,
                agent_type: row.get(4)?,
                parent_task_id: row.get(5)?,
                progress_pct: row.get(6)?,
                blocking_reason: row.get(7)?,
                next_action: row.get(8)?,
                result_summary: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                completed_at: row.get(12)?,
            })
        })?;

        let mut result = Vec::new();
        for task in tasks {
            result.push(task?);
        }
        Ok(result)
    }

    // ── Preference Operations ──

    pub fn set_preference(&self, pref: &Preference) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO preferences (category, key, value, source, confidence, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(category, key) DO UPDATE SET
             value=excluded.value, source=excluded.source,
             confidence=excluded.confidence, updated_at=datetime('now')",
            params![pref.category.to_str(), pref.key, pref.value, pref.source, pref.confidence],
        )?;
        Ok(())
    }

    pub fn get_preferences_by_category(
        &self,
        category: &str,
    ) -> SqlResult<Vec<Preference>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT category, key, value, source, confidence, created_at, updated_at
             FROM preferences WHERE category = ?1 ORDER BY key",
        )?;
        let prefs = stmt.query_map(params![category], |row| {
            Ok(Preference {
                category: super::preferences::PreferenceCategory::from_str(row.get::<_, String>(0)?.as_str()),
                key: row.get(1)?,
                value: row.get(2)?,
                source: row.get(3)?,
                confidence: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;

        let mut result = Vec::new();
        for pref in prefs {
            result.push(pref?);
        }
        Ok(result)
    }

    // ── Heartbeat Operations ──

    pub fn log_heartbeat(&self, event_type: &str, detail: Option<&str>) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO heartbeat_log (event_type, detail) VALUES (?1, ?2)",
            params![event_type, detail],
        )?;
        Ok(())
    }

    // ── WAL Maintenance ──

    pub fn wal_checkpoint(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    // ── Backup ──

    pub fn backup(&self) -> Result<String, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        let backup_path = format!("{}/backup-{}.sqlite", self.config.backup_dir, timestamp);

        std::fs::create_dir_all(&self.config.backup_dir)?;
        conn.execute("VACUUM INTO ?1", params![backup_path])?;

        Ok(backup_path)
    }

    fn run_heartbeat_loop(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []).ok();
        conn.execute(
            "INSERT INTO heartbeat_log (event_type, detail) VALUES (?1, ?2)",
            params!["wal_checkpoint", "automatic"],
        )
        .ok();
    }

    pub fn run_checkpoint(&self) {
        self.run_heartbeat_loop();
    }
}
