use chrono::{DateTime, Duration, Utc};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration as TokioDuration};

use crate::models::PomodoroSession;

/// 番茄钟状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroState {
    Idle,
    Working,
    Break,
    Paused,
}

/// 番茄钟计时器
#[derive(Debug, Clone)]
pub struct PomodoroTimer {
    pub state: PomodoroState,
    pub work_duration: i32,      // 工作时长（分钟）
    pub break_duration: i32,     // 休息时长（分钟）
    pub remaining_seconds: i32,   // 剩余秒数
    pub current_task_id: Option<i64>,
    pub session_id: Option<i64>,
    pub start_time: Option<DateTime<Utc>>,
}

impl Default for PomodoroTimer {
    fn default() -> Self {
        Self {
            state: PomodoroState::Idle,
            work_duration: 25,
            break_duration: 5,
            remaining_seconds: 0,
            current_task_id: None,
            session_id: None,
            start_time: None,
        }
    }
}

impl PomodoroTimer {
    pub fn new(work_duration: i32, break_duration: i32) -> Self {
        Self {
            work_duration,
            break_duration,
            ..Default::default()
        }
    }

    /// 开始工作计时
    pub fn start_work(&mut self, task_id: Option<i64>) {
        self.state = PomodoroState::Working;
        self.remaining_seconds = self.work_duration * 60;
        self.current_task_id = task_id;
        self.start_time = Some(Utc::now());
    }

    /// 开始休息
    pub fn start_break(&mut self) {
        self.state = PomodoroState::Break;
        self.remaining_seconds = self.break_duration * 60;
        self.start_time = Some(Utc::now());
    }

    /// 暂停
    pub fn pause(&mut self) {
        if self.state == PomodoroState::Working || self.state == PomodoroState::Break {
            self.state = PomodoroState::Paused;
        }
    }

    /// 恢复
    pub fn resume(&mut self) {
        if self.state == PomodoroState::Paused {
            self.state = PomodoroState::Working;
        }
    }

    /// 停止
    pub fn stop(&mut self) {
        self.state = PomodoroState::Idle;
        self.remaining_seconds = 0;
        self.current_task_id = None;
        self.session_id = None;
        self.start_time = None;
    }

    /// 减少一秒
    pub fn tick(&mut self) -> bool {
        if self.state == PomodoroState::Working || self.state == PomodoroState::Break {
            if self.remaining_seconds > 0 {
                self.remaining_seconds -= 1;
                true
            } else {
                false // 时间到
            }
        } else {
            false
        }
    }

    /// 获取进度百分比
    pub fn progress(&self) -> f32 {
        let total = match self.state {
            PomodoroState::Working => self.work_duration * 60,
            PomodoroState::Break => self.break_duration * 60,
            _ => return 0.0,
        };

        if total == 0 {
            return 0.0;
        }

        ((total - self.remaining_seconds) as f32 / total as f32) * 100.0
    }

    /// 格式化剩余时间
    pub fn format_remaining(&self) -> String {
        let minutes = self.remaining_seconds / 60;
        let seconds = self.remaining_seconds % 60;
        format!("{:02}:{:02}", minutes, seconds)
    }
}
