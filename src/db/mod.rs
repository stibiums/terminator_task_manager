use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::models::{Note, PomodoroSession, Priority, Task, TaskStatus};

pub struct Database {
    conn: Connection,
}

impl Database {
    /// 打开或创建数据库
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).context("Failed to open database")?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// 初始化数据库schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT,
                priority INTEGER NOT NULL,
                status INTEGER NOT NULL,
                due_date TEXT,
                reminder_time TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT,
                pomodoro_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                task_id INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS pomodoro_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER,
                start_time TEXT NOT NULL,
                end_time TEXT,
                duration_minutes INTEGER NOT NULL,
                completed INTEGER NOT NULL,
                FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE SET NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_due_date ON tasks(due_date);
            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_priority ON tasks(priority);
            CREATE INDEX IF NOT EXISTS idx_notes_task_id ON notes(task_id);
            "#,
        )?;
        Ok(())
    }

    // ==================== Task CRUD ====================

    /// 创建任务
    pub fn create_task(&self, task: &Task) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tasks (title, description, priority, status, due_date, reminder_time,
                               created_at, updated_at, pomodoro_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                task.title,
                task.description,
                task.priority as i32,
                task.status as i32,
                task.due_date.map(|d| d.to_rfc3339()),
                task.reminder_time.map(|d| d.to_rfc3339()),
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
                task.pomodoro_count,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 获取所有任务
    pub fn get_all_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, priority, status, due_date, reminder_time,
                    created_at, updated_at, completed_at, pomodoro_count
             FROM tasks
             ORDER BY priority DESC, due_date ASC",
        )?;

        let tasks = stmt
            .query_map([], |row| {
                Ok(Task {
                    id: Some(row.get(0)?),
                    title: row.get(1)?,
                    description: row.get(2)?,
                    priority: match row.get::<_, i32>(3)? {
                        1 => Priority::Low,
                        2 => Priority::Medium,
                        _ => Priority::High,
                    },
                    status: match row.get::<_, i32>(4)? {
                        0 => TaskStatus::Todo,
                        1 => TaskStatus::InProgress,
                        _ => TaskStatus::Completed,
                    },
                    due_date: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    reminder_time: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    completed_at: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    pomodoro_count: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tasks)
    }

    /// 更新任务
    pub fn update_task(&self, task: &Task) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET title = ?1, description = ?2, priority = ?3, status = ?4,
                            due_date = ?5, reminder_time = ?6, updated_at = ?7,
                            completed_at = ?8, pomodoro_count = ?9
             WHERE id = ?10",
            params![
                task.title,
                task.description,
                task.priority as i32,
                task.status as i32,
                task.due_date.map(|d| d.to_rfc3339()),
                task.reminder_time.map(|d| d.to_rfc3339()),
                task.updated_at.to_rfc3339(),
                task.completed_at.map(|d| d.to_rfc3339()),
                task.pomodoro_count,
                task.id,
            ],
        )?;
        Ok(())
    }

    /// 删除任务
    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ==================== Note CRUD ====================

    /// 创建便签
    pub fn create_note(&self, note: &Note) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notes (title, content, task_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                note.title,
                note.content,
                note.task_id,
                note.created_at.to_rfc3339(),
                note.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 获取所有便签
    pub fn get_all_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, content, task_id, created_at, updated_at
             FROM notes
             ORDER BY updated_at DESC",
        )?;

        let notes = stmt
            .query_map([], |row| {
                Ok(Note {
                    id: Some(row.get(0)?),
                    title: row.get(1)?,
                    content: row.get(2)?,
                    task_id: row.get(3)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(notes)
    }

    /// 更新便签
    pub fn update_note(&self, note: &Note) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET title = ?1, content = ?2, task_id = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                note.title,
                note.content,
                note.task_id,
                note.updated_at.to_rfc3339(),
                note.id,
            ],
        )?;
        Ok(())
    }

    /// 删除便签
    pub fn delete_note(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ==================== Pomodoro Sessions ====================

    /// 创建番茄钟会话
    pub fn create_pomodoro(&self, session: &PomodoroSession) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO pomodoro_sessions (task_id, start_time, end_time, duration_minutes, completed)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session.task_id,
                session.start_time.to_rfc3339(),
                session.end_time.map(|d| d.to_rfc3339()),
                session.duration_minutes,
                session.completed as i32,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 完成番茄钟会话
    pub fn complete_pomodoro(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE pomodoro_sessions SET end_time = ?1, completed = 1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// 获取任务的番茄钟记录
    pub fn get_task_pomodoros(&self, task_id: i64) -> Result<Vec<PomodoroSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, start_time, end_time, duration_minutes, completed
             FROM pomodoro_sessions
             WHERE task_id = ?1
             ORDER BY start_time DESC",
        )?;

        let sessions = stmt
            .query_map(params![task_id], |row| {
                Ok(PomodoroSession {
                    id: Some(row.get(0)?),
                    task_id: row.get(1)?,
                    start_time: DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    end_time: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    duration_minutes: row.get(4)?,
                    completed: row.get::<_, i32>(5)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// 获取今日完成的番茄钟统计
    pub fn get_today_pomodoro_stats(&self) -> Result<(usize, usize)> {
        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .with_timezone(&Utc);

        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*), SUM(duration_minutes)
             FROM pomodoro_sessions
             WHERE completed = 1 AND start_time >= ?1",
        )?;

        let (count, total_minutes): (i64, Option<i64>) = stmt.query_row(
            params![today_start.to_rfc3339()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        Ok((count as usize, total_minutes.unwrap_or(0) as usize))
    }
}
