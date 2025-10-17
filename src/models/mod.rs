use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 任务优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Low = 1,
    Medium = 2,
    High = 3,
}

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Completed,
}

/// 任务数据模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub priority: Priority,
    pub status: TaskStatus,
    pub due_date: Option<DateTime<Utc>>,
    pub reminder_time: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub pomodoro_count: i32,
}

/// 便签数据模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Option<i64>,
    pub title: String,
    pub content: String,
    pub task_id: Option<i64>, // 关联到任务
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 番茄钟记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomodoroSession {
    pub id: Option<i64>,
    pub task_id: Option<i64>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_minutes: i32, // 计划时长
    pub completed: bool,
}

impl Task {
    pub fn new(title: String) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            title,
            description: None,
            priority: Priority::Medium,
            status: TaskStatus::Todo,
            due_date: None,
            reminder_time: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            pomodoro_count: 0,
        }
    }

    pub fn is_overdue(&self) -> bool {
        if let Some(due) = self.due_date {
            due < Utc::now() && self.status != TaskStatus::Completed
        } else {
            false
        }
    }
}

impl Note {
    pub fn new(title: String, content: String) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            title,
            content,
            task_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}
