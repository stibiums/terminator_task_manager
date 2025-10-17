use anyhow::Result;
use notify_rust::{Notification, Timeout};

/// 通知管理器
pub struct NotificationManager;

impl NotificationManager {
    pub fn new() -> Self {
        Self
    }

    /// 发送任务提醒
    pub fn send_task_reminder(&self, title: &str, body: &str) -> Result<()> {
        Notification::new()
            .summary(&format!("📅 {}", title))
            .body(body)
            .icon("calendar")
            .timeout(Timeout::Milliseconds(5000))
            .show()?;
        Ok(())
    }

    /// 发送番茄钟完成通知
    pub fn send_pomodoro_complete(&self, is_break: bool) -> Result<()> {
        let (summary, body) = if is_break {
            ("休息时间结束", "准备开始新的番茄钟吧！")
        } else {
            ("番茄钟完成", "干得好！休息一下吧。")
        };

        Notification::new()
            .summary(&format!("🍅 {}", summary))
            .body(body)
            .icon("emblem-default")
            .timeout(Timeout::Milliseconds(5000))
            .show()?;
        Ok(())
    }

    /// 发送普通通知
    pub fn send_notification(&self, title: &str, body: &str) -> Result<()> {
        Notification::new()
            .summary(title)
            .body(body)
            .timeout(Timeout::Milliseconds(3000))
            .show()?;
        Ok(())
    }
}
