use anyhow::Result;
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use std::path::PathBuf;

mod db;
mod models;
mod notify;
mod notes;
mod pomodoro;
mod ui;

use db::Database;
use models::{Note, Task};

#[derive(Parser)]
#[command(name = "tasks")]
#[command(about = "Terminal task manager with pomodoro and notes", long_about = None)]
struct Cli {
    /// Database path (defaults to user data directory)
    #[arg(short, long)]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI interface
    Show,

    /// Add a new task
    Add {
        /// Task title
        title: String,
    },

    /// List all tasks
    List,

    /// Mark a task as completed
    Complete {
        /// Task ID
        id: i64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // 确定数据库路径
    let db_path = cli.db_path.unwrap_or_else(|| {
        let proj_dirs = ProjectDirs::from("com", "terminator-task", "tasks")
            .expect("Failed to get project directories");
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).expect("Failed to create data directory");
        data_dir.join("tasks.db")
    });

    match cli.command {
        Some(Commands::Show) | None => {
            // 启动TUI
            ui::run_app()?;
        }
        Some(Commands::Add { title }) => {
            let db = Database::open(&db_path)?;
            let task = Task::new(title);
            let id = db.create_task(&task)?;
            println!("✅ Task created with ID: {}", id);
        }
        Some(Commands::List) => {
            let db = Database::open(&db_path)?;
            let tasks = db.get_all_tasks()?;

            if tasks.is_empty() {
                println!("No tasks found.");
            } else {
                for task in tasks {
                    let status_icon = match task.status {
                        models::TaskStatus::Completed => "✅",
                        models::TaskStatus::InProgress => "🔄",
                        models::TaskStatus::Todo => "⭕",
                    };
                    let priority_icon = match task.priority {
                        models::Priority::High => "🔴",
                        models::Priority::Medium => "🟡",
                        models::Priority::Low => "🟢",
                    };
                    println!(
                        "[{}] {} {} {}",
                        task.id.unwrap(),
                        status_icon,
                        priority_icon,
                        task.title
                    );
                }
            }
        }
        Some(Commands::Complete { id }) => {
            let db = Database::open(&db_path)?;
            let mut tasks = db.get_all_tasks()?;

            if let Some(task) = tasks.iter_mut().find(|t| t.id == Some(id)) {
                task.status = models::TaskStatus::Completed;
                task.completed_at = Some(chrono::Utc::now());
                task.updated_at = chrono::Utc::now();
                db.update_task(task)?;
                println!("✅ Task {} marked as completed", id);
            } else {
                println!("❌ Task {} not found", id);
            }
        }
    }

    Ok(())
}
