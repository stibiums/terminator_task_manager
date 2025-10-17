use anyhow::Result;
use clap::Parser;
use directories::ProjectDirs;
use std::path::PathBuf;
use chrono::Utc;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};

// 引用主crate的模块（通过路径）
#[path = "../db/mod.rs"]
mod db;
#[path = "../models/mod.rs"]
mod models;
#[path = "../notify/mod.rs"]
mod notify;
#[path = "../notes/mod.rs"]
mod notes;
#[path = "../pomodoro/mod.rs"]
mod pomodoro;

use db::Database;
use models::TaskStatus;
use notify::NotificationManager;

// 守护进程结构
pub struct Daemon {
    db: Arc<Mutex<Database>>,
    notifier: NotificationManager,
}

impl Daemon {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let db = Database::open(db_path)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            notifier: NotificationManager::new(),
        })
    }

    /// 运行守护进程
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Task daemon started");

        loop {
            // 检查提醒
            if let Err(e) = self.check_reminders().await {
                tracing::error!("Error checking reminders: {}", e);
            }

            // 每分钟检查一次
            sleep(Duration::from_secs(60)).await;
        }
    }

    /// 检查并发送提醒
    async fn check_reminders(&self) -> Result<()> {
        let db = self.db.lock().unwrap();
        let tasks = db.get_all_tasks()?;
        let now = Utc::now();

        for task in tasks {
            if task.status != TaskStatus::Completed {
                if let Some(reminder_time) = task.reminder_time {
                    // 如果提醒时间在过去1分钟内，发送提醒
                    if reminder_time <= now && (now - reminder_time).num_minutes() < 1 {
                        let body = format!(
                            "截止时间: {}",
                            task.due_date
                                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "无".to_string())
                        );

                        if let Err(e) = self.notifier.send_task_reminder(&task.title, &body) {
                            tracing::error!("Failed to send reminder: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Parser)]
#[command(name = "taskd")]
#[command(about = "Task manager daemon", long_about = None)]
struct Cli {
    /// Database path (defaults to user data directory)
    #[arg(short, long)]
    db_path: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 设置日志
    let log_level = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    // 确定数据库路径
    let db_path = cli.db_path.unwrap_or_else(|| {
        let proj_dirs = ProjectDirs::from("com", "terminator-task", "tasks")
            .expect("Failed to get project directories");
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).expect("Failed to create data directory");
        data_dir.join("tasks.db")
    });

    tracing::info!("Using database: {:?}", db_path);

    // 创建并运行守护进程
    let daemon = Daemon::new(db_path)?;
    daemon.run().await?;

    Ok(())
}
