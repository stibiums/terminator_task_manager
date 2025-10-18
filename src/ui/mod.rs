use anyhow::Result;
use chrono::{Datelike, TimeZone, Timelike, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::io;

use crate::db::Database;
use crate::models::{Note, PomodoroSession, Priority, Task, TaskStatus};
use crate::pomodoro::PomodoroTimer;

mod task_list;
mod note_list;
mod pomodoro_view;

pub use task_list::TaskListWidget;
pub use note_list::NoteListWidget;
pub use pomodoro_view::PomodoroWidget;

/// 应用状态
pub struct App {
    pub db_path: String,
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    pub pomodoro: PomodoroTimer,
    pub current_tab: usize,
    pub task_list_state: ListState,
    pub note_list_state: ListState,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub input_title: String,
    pub show_dialog: DialogType,
    pub status_message: Option<String>,
    // 日期时间选择器状态
    pub datetime_picker_field: usize, // 0=年, 1=月, 2=日, 3=时, 4=分
    pub datetime_year: i32,
    pub datetime_month: u32,
    pub datetime_day: u32,
    pub datetime_hour: u32,
    pub datetime_minute: u32,
    // 番茄钟统计
    pub pomodoro_completed_today: usize,
    pub pomodoro_total_minutes: usize,
    // Vim状态
    pub last_key: Option<KeyCode>,
    pub number_prefix: String,
}

/// 输入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,      // 插入模式 (类似vim的i)
    Command,     // 命令模式 (类似vim的:)
}

/// 对话框类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogType {
    None,
    CreateTask,
    EditTask,
    DeleteConfirm,
    CreateNote,
    EditNote,
    Help,
    SetDeadline,
}

impl Default for App {
    fn default() -> Self {
        let mut task_list_state = ListState::default();
        task_list_state.select(Some(0));

        let mut note_list_state = ListState::default();
        note_list_state.select(Some(0));

        let now = chrono::Local::now();

        Self {
            db_path: String::new(),
            tasks: Vec::new(),
            notes: Vec::new(),
            pomodoro: PomodoroTimer::default(),
            current_tab: 0,
            task_list_state,
            note_list_state,
            should_quit: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_title: String::new(),
            show_dialog: DialogType::None,
            status_message: None,
            datetime_picker_field: 0,
            datetime_year: now.year(),
            datetime_month: now.month(),
            datetime_day: now.day(),
            datetime_hour: now.hour(),
            datetime_minute: now.minute(),
            pomodoro_completed_today: 0,
            pomodoro_total_minutes: 0,
            last_key: None,
            number_prefix: String::new(),
        }
    }
}

impl App {
    pub fn new(db_path: String) -> Result<Self> {
        let mut app = Self {
            db_path: db_path.clone(),
            ..Default::default()
        };
        app.reload_data()?;
        Ok(app)
    }

    /// 从数据库重新加载数据
    pub fn reload_data(&mut self) -> Result<()> {
        let db = Database::open(&self.db_path)?;
        self.tasks = db.get_all_tasks()?;
        self.notes = db.get_all_notes()?;

        // 加载番茄钟统计
        let (completed, minutes) = db.get_today_pomodoro_stats()?;
        self.pomodoro_completed_today = completed;
        self.pomodoro_total_minutes = minutes;

        // 加载番茄钟配置
        let (work, break_time) = db.get_pomodoro_config()?;
        self.pomodoro.work_duration = work;
        self.pomodoro.break_duration = break_time;

        // 自动排序任务
        self.sort_tasks();

        // 更新选择状态
        if !self.tasks.is_empty() && self.task_list_state.selected().is_none() {
            self.task_list_state.select(Some(0));
        }
        if !self.notes.is_empty() && self.note_list_state.selected().is_none() {
            self.note_list_state.select(Some(0));
        }

        Ok(())
    }

    /// 任务自动排序
    /// 排序规则：
    /// 1. 未完成的任务优先（按状态：InProgress > Todo > Completed）
    /// 2. 在同状态下，按优先级排序（High > Medium > Low）
    /// 3. 在同优先级下，按DDL时间排序（有DDL的优先，且时间早的优先）
    fn sort_tasks(&mut self) {
        self.tasks.sort_by(|a, b| {
            use std::cmp::Ordering;

            // 1. 首先按状态排序
            let status_order = |status: &TaskStatus| match status {
                TaskStatus::InProgress => 0,
                TaskStatus::Todo => 1,
                TaskStatus::Completed => 2,
            };

            let status_cmp = status_order(&a.status).cmp(&status_order(&b.status));
            if status_cmp != Ordering::Equal {
                return status_cmp;
            }

            // 2. 同状态下，按优先级排序（逆序，因为High=3, Medium=2, Low=1）
            let priority_cmp = (b.priority as i32).cmp(&(a.priority as i32));
            if priority_cmp != Ordering::Equal {
                return priority_cmp;
            }

            // 3. 同优先级下，按DDL排序
            match (&a.due_date, &b.due_date) {
                (Some(a_due), Some(b_due)) => a_due.cmp(b_due), // 都有DDL，早的优先
                (Some(_), None) => Ordering::Less,               // a有DDL，a优先
                (None, Some(_)) => Ordering::Greater,            // b有DDL，b优先
                (None, None) => Ordering::Equal,                 // 都没有DDL，相等
            }
        });
    }

    /// 切换标签页
    pub fn next_tab(&mut self) {
        self.current_tab = (self.current_tab + 1) % 3;
    }

    pub fn previous_tab(&mut self) {
        if self.current_tab > 0 {
            self.current_tab -= 1;
        } else {
            self.current_tab = 2;
        }
    }

    pub fn goto_tab(&mut self, tab: usize) {
        if tab < 3 {
            self.current_tab = tab;
        }
    }

    /// 任务列表导航
    pub fn next_task(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let i = match self.task_list_state.selected() {
            Some(i) => {
                if i >= self.tasks.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.task_list_state.select(Some(i));
    }

    pub fn previous_task(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let i = match self.task_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tasks.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.task_list_state.select(Some(i));
    }

    /// 便签列表导航
    pub fn next_note(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let i = match self.note_list_state.selected() {
            Some(i) => {
                if i >= self.notes.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.note_list_state.select(Some(i));
    }

    pub fn previous_note(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let i = match self.note_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.notes.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.note_list_state.select(Some(i));
    }

    /// vim风格：跳到第一个
    pub fn goto_first_task(&mut self) {
        if !self.tasks.is_empty() {
            self.task_list_state.select(Some(0));
        }
    }

    pub fn goto_last_task(&mut self) {
        if !self.tasks.is_empty() {
            self.task_list_state.select(Some(self.tasks.len() - 1));
        }
    }

    pub fn goto_first_note(&mut self) {
        if !self.notes.is_empty() {
            self.note_list_state.select(Some(0));
        }
    }

    pub fn goto_last_note(&mut self) {
        if !self.notes.is_empty() {
            self.note_list_state.select(Some(self.notes.len() - 1));
        }
    }

    /// 获取当前选中的任务
    pub fn selected_task(&self) -> Option<&Task> {
        self.task_list_state
            .selected()
            .and_then(|i| self.tasks.get(i))
    }

    pub fn selected_task_mut(&mut self) -> Option<&mut Task> {
        self.task_list_state
            .selected()
            .and_then(|i| self.tasks.get_mut(i))
    }

    /// 获取当前选中的便签
    pub fn selected_note(&self) -> Option<&Note> {
        self.note_list_state
            .selected()
            .and_then(|i| self.notes.get(i))
    }

    /// 切换任务完成状态
    pub fn toggle_task_status(&mut self) -> Result<()> {
        let db_path = self.db_path.clone();

        if let Some(task) = self.selected_task_mut() {
            task.status = match task.status {
                TaskStatus::Todo => TaskStatus::Completed,
                TaskStatus::Completed => TaskStatus::Todo,
                TaskStatus::InProgress => TaskStatus::Completed,
            };
            task.updated_at = Utc::now();
            if task.status == TaskStatus::Completed {
                task.completed_at = Some(Utc::now());
            } else {
                task.completed_at = None;
            }

            let db = Database::open(&db_path)?;
            db.update_task(task)?;
            self.status_message = Some(format!("任务状态已更新"));
        }

        // 立即重新排序
        self.sort_tasks();
        Ok(())
    }

    /// 创建新任务
    pub fn create_task(&mut self) -> Result<()> {
        if self.input_buffer.is_empty() {
            return Ok(());
        }

        let db = Database::open(&self.db_path)?;
        let task = Task::new(self.input_buffer.clone());
        let id = db.create_task(&task)?;

        self.input_buffer.clear();
        self.show_dialog = DialogType::None;
        self.input_mode = InputMode::Normal;
        self.reload_data()?;
        self.status_message = Some(format!("任务 #{} 已创建", id));

        Ok(())
    }

    /// 删除任务
    pub fn delete_task(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            if let Some(id) = task.id {
                let db = Database::open(&self.db_path)?;
                db.delete_task(id)?;
                self.reload_data()?;
                self.status_message = Some(format!("任务 #{} 已删除", id));
            }
        }
        self.show_dialog = DialogType::None;
        Ok(())
    }

    /// 创建便签
    pub fn create_note(&mut self) -> Result<()> {
        if self.input_buffer.is_empty() {
            return Ok(());
        }

        let db = Database::open(&self.db_path)?;
        let note = Note::new(self.input_title.clone(), self.input_buffer.clone());
        let id = db.create_note(&note)?;

        self.input_buffer.clear();
        self.input_title.clear();
        self.show_dialog = DialogType::None;
        self.input_mode = InputMode::Normal;
        self.reload_data()?;
        self.status_message = Some(format!("便签 #{} 已创建", id));

        Ok(())
    }

    /// 删除便签
    pub fn delete_note(&mut self) -> Result<()> {
        if let Some(note) = self.selected_note() {
            if let Some(id) = note.id {
                let db = Database::open(&self.db_path)?;
                db.delete_note(id)?;
                self.reload_data()?;
                self.status_message = Some(format!("便签 #{} 已删除", id));
            }
        }
        Ok(())
    }

    /// 循环切换任务优先级
    pub fn cycle_priority(&mut self) -> Result<()> {
        let db_path = self.db_path.clone();

        if let Some(task) = self.selected_task_mut() {
            task.priority = match task.priority {
                Priority::Low => Priority::Medium,
                Priority::Medium => Priority::High,
                Priority::High => Priority::Low,
            };
            task.updated_at = Utc::now();

            let db = Database::open(&db_path)?;
            db.update_task(task)?;
            self.status_message = Some(format!("优先级已更新"));
        }

        // 立即重新排序
        self.sort_tasks();
        Ok(())
    }

    /// 初始化日期时间选择器 (设置为当前选中任务的deadline，或当前时间)
    pub fn init_datetime_picker(&mut self) {
        if let Some(task) = self.selected_task() {
            if let Some(due_date) = task.due_date {
                let local = due_date.with_timezone(&chrono::Local);
                self.datetime_year = local.year();
                self.datetime_month = local.month();
                self.datetime_day = local.day();
                self.datetime_hour = local.hour();
                self.datetime_minute = local.minute();
            } else {
                let now = chrono::Local::now();
                self.datetime_year = now.year();
                self.datetime_month = now.month();
                self.datetime_day = now.day();
                self.datetime_hour = now.hour();
                self.datetime_minute = now.minute();
            }
        }
        self.datetime_picker_field = 0;
    }

    /// 日期时间选择器：移动到下一个字段
    pub fn datetime_picker_next_field(&mut self) {
        self.datetime_picker_field = (self.datetime_picker_field + 1) % 5;
    }

    /// 日期时间选择器：移动到上一个字段
    pub fn datetime_picker_prev_field(&mut self) {
        if self.datetime_picker_field == 0 {
            self.datetime_picker_field = 4;
        } else {
            self.datetime_picker_field -= 1;
        }
    }

    /// 日期时间选择器：增加当前字段的值
    pub fn datetime_picker_increment(&mut self) {
        match self.datetime_picker_field {
            0 => self.datetime_year += 1,
            1 => {
                self.datetime_month += 1;
                if self.datetime_month > 12 {
                    self.datetime_month = 1;
                }
            }
            2 => {
                let max_day = Self::days_in_month(self.datetime_year, self.datetime_month);
                self.datetime_day += 1;
                if self.datetime_day > max_day {
                    self.datetime_day = 1;
                }
            }
            3 => {
                self.datetime_hour += 1;
                if self.datetime_hour > 23 {
                    self.datetime_hour = 0;
                }
            }
            4 => {
                self.datetime_minute += 1;
                if self.datetime_minute > 59 {
                    self.datetime_minute = 0;
                }
            }
            _ => {}
        }
    }

    /// 日期时间选择器：减少当前字段的值
    pub fn datetime_picker_decrement(&mut self) {
        match self.datetime_picker_field {
            0 => self.datetime_year -= 1,
            1 => {
                if self.datetime_month == 1 {
                    self.datetime_month = 12;
                } else {
                    self.datetime_month -= 1;
                }
            }
            2 => {
                if self.datetime_day == 1 {
                    let max_day = Self::days_in_month(self.datetime_year, self.datetime_month);
                    self.datetime_day = max_day;
                } else {
                    self.datetime_day -= 1;
                }
            }
            3 => {
                if self.datetime_hour == 0 {
                    self.datetime_hour = 23;
                } else {
                    self.datetime_hour -= 1;
                }
            }
            4 => {
                if self.datetime_minute == 0 {
                    self.datetime_minute = 59;
                } else {
                    self.datetime_minute -= 1;
                }
            }
            _ => {}
        }
    }

    /// 获取某月的天数
    fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    /// 应用选中的日期时间到当前任务
    pub fn apply_deadline(&mut self) -> Result<()> {
        let db_path = self.db_path.clone();

        // 先提取datetime值，避免借用冲突
        let year = self.datetime_year;
        let month = self.datetime_month;
        let day = self.datetime_day;
        let hour = self.datetime_hour;
        let minute = self.datetime_minute;

        if let Some(task) = self.selected_task_mut() {
            // 创建本地时间
            let local_dt = chrono::Local
                .with_ymd_and_hms(year, month, day, hour, minute, 0)
                .single();

            if let Some(local_dt) = local_dt {
                task.due_date = Some(local_dt.with_timezone(&Utc));
                task.updated_at = Utc::now();

                let db = Database::open(&db_path)?;
                db.update_task(task)?;
                self.status_message = Some(format!(
                    "DDL已设置: {}-{:02}-{:02} {:02}:{:02}",
                    year, month, day, hour, minute
                ));
            } else {
                self.status_message = Some("无效的日期时间".to_string());
            }
        }

        // 立即重新排序
        self.sort_tasks();
        self.show_dialog = DialogType::None;
        Ok(())
    }
}

/// 运行TUI应用
pub fn run_app(db_path: String) -> Result<()> {
    // 设置终端
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 创建应用状态
    let mut app = App::new(db_path)?;

    // 主循环
    let res = run_ui_loop(&mut terminal, &mut app);

    // 恢复终端
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

/// UI主循环
fn run_ui_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(1000))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key_event(app, key.code)?;
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(app, mouse)?;
                }
                _ => {}
            }
        }

        // 番茄钟计时
        if app.pomodoro.state == crate::pomodoro::PomodoroState::Working
            || app.pomodoro.state == crate::pomodoro::PomodoroState::Break
        {
            if !app.pomodoro.tick() {
                // 时间到，切换状态
                match app.pomodoro.state {
                    crate::pomodoro::PomodoroState::Working => {
                        // 工作时段完成，保存到数据库
                        if let (Some(start_time), Ok(db)) = (app.pomodoro.start_time, Database::open(&app.db_path)) {
                            let session = PomodoroSession {
                                id: None,
                                task_id: app.pomodoro.current_task_id,
                                start_time,
                                end_time: Some(Utc::now()),
                                duration_minutes: app.pomodoro.work_duration,
                                completed: true,
                            };
                            let _ = db.create_pomodoro(&session);
                        }

                        app.pomodoro_completed_today += 1;
                        app.pomodoro_total_minutes += app.pomodoro.work_duration as usize;
                        app.pomodoro.start_break();
                        app.status_message = Some("🎉 工作时段完成！开始休息！".to_string());
                    }
                    crate::pomodoro::PomodoroState::Break => {
                        app.pomodoro.stop();
                        app.status_message = Some("番茄钟完成！".to_string());
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// 执行vim命令
fn execute_command(app: &mut App) -> Result<()> {
    let cmd = app.input_buffer.trim();

    // 空命令
    if cmd.is_empty() {
        return Ok(());
    }

    // 数字跳转: :5 跳到第5行
    if let Ok(line_num) = cmd.parse::<usize>() {
        if line_num > 0 {
            match app.current_tab {
                0 => {
                    if line_num <= app.tasks.len() {
                        app.task_list_state.select(Some(line_num - 1));
                        app.status_message = Some(format!("跳转到第{}行", line_num));
                    }
                }
                1 => {
                    if line_num <= app.notes.len() {
                        app.note_list_state.select(Some(line_num - 1));
                        app.status_message = Some(format!("跳转到第{}行", line_num));
                    }
                }
                _ => {}
            }
        }
        return Ok(());
    }

    // 解析命令和参数
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let command = parts.first().unwrap_or(&"");

    match *command {
        // 退出命令
        "q" | "quit" => {
            app.should_quit = true;
        }
        "wq" | "x" => {
            // 保存并退出 (虽然我们是自动保存)
            app.should_quit = true;
        }

        // 删除命令
        "d" | "delete" => {
            app.show_dialog = DialogType::DeleteConfirm;
        }

        // 新建命令
        "new" | "n" => {
            let title = parts[1..].join(" ");
            if !title.is_empty() {
                match app.current_tab {
                    0 => {
                        let db = Database::open(&app.db_path)?;
                        let task = Task::new(title.clone());
                        let id = db.create_task(&task)?;
                        app.reload_data()?;
                        app.status_message = Some(format!("任务 #{} 已创建", id));
                    }
                    1 => {
                        let db = Database::open(&app.db_path)?;
                        let note = Note::new("新便签".to_string(), title.clone());
                        let id = db.create_note(&note)?;
                        app.reload_data()?;
                        app.status_message = Some(format!("便签 #{} 已创建", id));
                    }
                    _ => {}
                }
            } else {
                match app.current_tab {
                    0 => {
                        app.show_dialog = DialogType::CreateTask;
                        app.input_mode = InputMode::Insert;
                    }
                    1 => {
                        app.show_dialog = DialogType::CreateNote;
                        app.input_mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
        }

        // 编辑命令
        "e" | "edit" => {
            // TODO: 实现编辑功能
            app.status_message = Some("编辑功能即将推出".to_string());
        }

        // 番茄钟配置命令
        "pomo" | "pomodoro" => {
            if parts.len() > 1 {
                for arg in &parts[1..] {
                    if let Some((key, value)) = arg.split_once('=') {
                        match key {
                            "work" | "w" => {
                                if let Ok(minutes) = value.parse::<i32>() {
                                    if minutes >= 1 && minutes <= 120 {
                                        app.pomodoro.work_duration = minutes;
                                        if let Ok(db) = Database::open(&app.db_path) {
                                            let _ = db.save_pomodoro_config(
                                                app.pomodoro.work_duration,
                                                app.pomodoro.break_duration
                                            );
                                        }
                                        app.status_message = Some(format!("工作时长设置为 {} 分钟", minutes));
                                    }
                                }
                            }
                            "break" | "b" => {
                                if let Ok(minutes) = value.parse::<i32>() {
                                    if minutes >= 1 && minutes <= 60 {
                                        app.pomodoro.break_duration = minutes;
                                        if let Ok(db) = Database::open(&app.db_path) {
                                            let _ = db.save_pomodoro_config(
                                                app.pomodoro.work_duration,
                                                app.pomodoro.break_duration
                                            );
                                        }
                                        app.status_message = Some(format!("休息时长设置为 {} 分钟", minutes));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                app.status_message = Some(format!(
                    "番茄钟配置: 工作{}分钟 休息{}分钟 | 用法: :pomo work=25 break=5",
                    app.pomodoro.work_duration,
                    app.pomodoro.break_duration
                ));
            }
        }

        // 帮助命令
        "h" | "help" => {
            app.show_dialog = DialogType::Help;
        }

        // 未知命令
        _ => {
            app.status_message = Some(format!("未知命令: {}", cmd));
        }
    }

    Ok(())
}

/// 处理键盘事件
fn handle_key_event(app: &mut App, key: KeyCode) -> Result<()> {
    // 对话框模式
    if app.show_dialog != DialogType::None {
        // 特殊处理：SetDeadline dialog 使用方向键导航
        if app.show_dialog == DialogType::SetDeadline {
            match key {
                KeyCode::Left | KeyCode::Char('h') => {
                    app.datetime_picker_prev_field();
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    app.datetime_picker_next_field();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.datetime_picker_increment();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.datetime_picker_decrement();
                }
                KeyCode::Enter => {
                    app.apply_deadline()?;
                }
                KeyCode::Esc => {
                    app.show_dialog = DialogType::None;
                }
                _ => {}
            }
            return Ok(());
        }

        match app.input_mode {
            InputMode::Insert => {
                match key {
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.input_buffer.clear();
                        app.input_title.clear();
                        app.show_dialog = DialogType::None;
                    }
                    KeyCode::Enter => {
                        match app.show_dialog {
                            DialogType::CreateTask => app.create_task()?,
                            DialogType::CreateNote => {
                                // Tab键才切换到内容，Enter在有标题后创建
                                if !app.input_title.is_empty() {
                                    app.create_note()?;
                                } else {
                                    // 第一次Enter：将buffer内容作为标题
                                    app.input_title = app.input_buffer.clone();
                                    app.input_buffer.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Char(c) => {
                        app.input_buffer.push(c);
                    }
                    KeyCode::Backspace => {
                        app.input_buffer.pop();
                    }
                    _ => {}
                }
            }
            InputMode::Normal => {
                match key {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if app.show_dialog == DialogType::DeleteConfirm {
                            if app.current_tab == 0 {
                                app.delete_task()?;
                            } else {
                                app.delete_note()?;
                            }
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        app.show_dialog = DialogType::None;
                    }
                    KeyCode::Char('i') => {
                        if matches!(app.show_dialog, DialogType::CreateTask | DialogType::CreateNote | DialogType::EditTask | DialogType::EditNote) {
                            app.input_mode = InputMode::Insert;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // 命令模式处理
    if app.input_mode == InputMode::Command {
        match key {
            KeyCode::Enter => {
                // 执行命令
                execute_command(app)?;
                app.input_buffer.clear();
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                app.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                app.input_buffer.pop();
            }
            KeyCode::Esc => {
                app.input_buffer.clear();
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        return Ok(());
    }

    // 正常模式快捷键
    match app.input_mode {
        InputMode::Normal => {
            match key {
                // vim风格命令模式: 按:进入
                KeyCode::Char(':') => {
                    app.input_mode = InputMode::Command;
                    app.input_buffer.clear();
                    app.number_prefix.clear();
                    app.last_key = None;
                }

                // 数字前缀 (vim风格: 5j 向下移动5行)
                KeyCode::Char(c @ '0'..='9') => {
                    // 如果是在标签切换 (1/2/3) 且没有前缀，则切换标签
                    if app.number_prefix.is_empty() && matches!(c, '1' | '2' | '3') {
                        app.goto_tab((c as u8 - b'1') as usize);
                        app.last_key = Some(key);
                    } else {
                        // 否则累积数字前缀
                        app.number_prefix.push(c);
                        app.last_key = Some(key);
                    }
                }

                // 标签页切换: Tab, Shift+Tab
                KeyCode::Tab => {
                    app.next_tab();
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::BackTab => {
                    app.previous_tab();
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }

                // vim导航: j/k, h/l, gg/G (支持数字前缀)
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = if app.number_prefix.is_empty() {
                        1
                    } else {
                        app.number_prefix.parse::<usize>().unwrap_or(1)
                    };

                    match app.current_tab {
                        0 => {
                            for _ in 0..count {
                                app.next_task();
                            }
                        }
                        1 => {
                            for _ in 0..count {
                                app.next_note();
                            }
                        }
                        _ => {}
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let count = if app.number_prefix.is_empty() {
                        1
                    } else {
                        app.number_prefix.parse::<usize>().unwrap_or(1)
                    };

                    match app.current_tab {
                        0 => {
                            for _ in 0..count {
                                app.previous_task();
                            }
                        }
                        1 => {
                            for _ in 0..count {
                                app.previous_note();
                            }
                        }
                        _ => {}
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    let count = if app.number_prefix.is_empty() {
                        1
                    } else {
                        app.number_prefix.parse::<usize>().unwrap_or(1)
                    };

                    for _ in 0..count {
                        app.previous_tab();
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let count = if app.number_prefix.is_empty() {
                        1
                    } else {
                        app.number_prefix.parse::<usize>().unwrap_or(1)
                    };

                    for _ in 0..count {
                        app.next_tab();
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('g') => {
                    // gg: 双击g跳到顶部
                    if app.last_key == Some(KeyCode::Char('g')) {
                        match app.current_tab {
                            0 => app.goto_first_task(),
                            1 => app.goto_first_note(),
                            _ => {}
                        }
                        app.number_prefix.clear();
                        app.last_key = None; // 清除，避免连续gg
                    } else {
                        // 第一次按g，等待第二次
                        app.last_key = Some(key);
                    }
                }
                KeyCode::Char('G') => {
                    // G: 跳到末尾 (支持数字前缀如 5G 跳到第5行)
                    if app.number_prefix.is_empty() {
                        match app.current_tab {
                            0 => app.goto_last_task(),
                            1 => app.goto_last_note(),
                            _ => {}
                        }
                    } else {
                        // 数字G: 跳到指定行号
                        if let Ok(line_num) = app.number_prefix.parse::<usize>() {
                            if line_num > 0 {
                                match app.current_tab {
                                    0 => {
                                        if line_num <= app.tasks.len() {
                                            app.task_list_state.select(Some(line_num - 1));
                                        }
                                    }
                                    1 => {
                                        if line_num <= app.notes.len() {
                                            app.note_list_state.select(Some(line_num - 1));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }

                // 任务操作
                KeyCode::Char('n') | KeyCode::Char('a') => {
                    // 新建
                    match app.current_tab {
                        0 => {
                            app.show_dialog = DialogType::CreateTask;
                            app.input_mode = InputMode::Insert;
                            app.input_buffer.clear();
                        }
                        1 => {
                            app.show_dialog = DialogType::CreateNote;
                            app.input_mode = InputMode::Insert;
                            app.input_buffer.clear();
                            app.input_title.clear();
                        }
                        _ => {}
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char(' ') | KeyCode::Char('x') => {
                    // 切换完成状态
                    if app.current_tab == 0 {
                        app.toggle_task_status()?;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('d') => {
                    // 删除 (需要确认) - vim风格: dd删除
                    if app.last_key == Some(KeyCode::Char('d')) {
                        // dd: 快速删除，直接显示确认对话框
                        app.show_dialog = DialogType::DeleteConfirm;
                        app.number_prefix.clear();
                        app.last_key = None;
                    } else {
                        // 第一次按d，等待第二次
                        app.last_key = Some(key);
                    }
                }
                KeyCode::Char('p') => {
                    // 切换优先级
                    if app.current_tab == 0 {
                        app.cycle_priority()?;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('t') => {
                    // 设置DDL时间
                    if app.current_tab == 0 && !app.tasks.is_empty() {
                        app.init_datetime_picker();
                        app.show_dialog = DialogType::SetDeadline;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }

                // 番茄钟操作 (在番茄钟标签页)
                KeyCode::Char('s') => {
                    if app.current_tab == 2 {
                        match app.pomodoro.state {
                            crate::pomodoro::PomodoroState::Idle => {
                                app.pomodoro.start_work(None);
                                app.status_message = Some("番茄钟开始！".to_string());
                            }
                            crate::pomodoro::PomodoroState::Working
                            | crate::pomodoro::PomodoroState::Break => {
                                app.pomodoro.pause();
                                app.status_message = Some("已暂停".to_string());
                            }
                            crate::pomodoro::PomodoroState::Paused => {
                                app.pomodoro.resume();
                                app.status_message = Some("继续计时".to_string());
                            }
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('S') => {
                    // 停止番茄钟
                    app.pomodoro.stop();
                    app.status_message = Some("番茄钟已停止".to_string());
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                // 番茄钟自定义时长 (在番茄钟标签页)
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    if app.current_tab == 2 && app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                        app.pomodoro.work_duration += 5;
                        // 保存配置到数据库
                        if let Ok(db) = Database::open(&app.db_path) {
                            let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                        }
                        app.status_message = Some(format!("工作时长: {}分钟 (已保存)", app.pomodoro.work_duration));
                    }
                }
                KeyCode::Char('-') | KeyCode::Char('_') => {
                    if app.current_tab == 2 && app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                        if app.pomodoro.work_duration > 5 {
                            app.pomodoro.work_duration -= 5;
                            // 保存配置到数据库
                            if let Ok(db) = Database::open(&app.db_path) {
                                let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                            }
                            app.status_message = Some(format!("工作时长: {}分钟 (已保存)", app.pomodoro.work_duration));
                        }
                    }
                }
                KeyCode::Char('[') => {
                    if app.current_tab == 2 && app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                        app.pomodoro.break_duration += 1;
                        // 保存配置到数据库
                        if let Ok(db) = Database::open(&app.db_path) {
                            let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                        }
                        app.status_message = Some(format!("休息时长: {}分钟 (已保存)", app.pomodoro.break_duration));
                    }
                }
                KeyCode::Char(']') => {
                    if app.current_tab == 2 && app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                        if app.pomodoro.break_duration > 1 {
                            app.pomodoro.break_duration -= 1;
                            // 保存配置到数据库
                            if let Ok(db) = Database::open(&app.db_path) {
                                let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                            }
                            app.status_message = Some(format!("休息时长: {}分钟 (已保存)", app.pomodoro.break_duration));
                        }
                    }
                }

                // 帮助
                KeyCode::Char('?') => {
                    app.show_dialog = DialogType::Help;
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }

                // Escape键: 清除vim状态
                KeyCode::Esc => {
                    app.number_prefix.clear();
                    app.last_key = None;
                    app.status_message = None;
                }

                // q键: 退出
                KeyCode::Char('q') => {
                    app.should_quit = true;
                }

                _ => {
                    // 其他未处理的键: 清除vim状态
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// 处理鼠标事件
fn handle_mouse_event(app: &mut App, mouse: MouseEvent) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse.row;
            let col = mouse.column;

            // 点击标签页区域 (前3行)
            if row <= 2 {
                // 标签页宽度估算: 每个标签约20个字符
                // Tab titles: "📝 Tasks (1)", "📓 Notes (2)", "🍅 Pomodoro (3)"
                let tab_width = 20;
                if col < tab_width {
                    app.goto_tab(0);
                } else if col < tab_width * 2 {
                    app.goto_tab(1);
                } else if col < tab_width * 3 {
                    app.goto_tab(2);
                }
            }
            // 点击内容区域 - 选择列表项
            else if row > 3 {
                // row 0-2: tabs
                // row 3: 内容区开始
                // 内容区内部有1行边框 + 1行标题
                let content_start_row = 5; // 3 (tabs) + 1 (border) + 1 (title)

                if row >= content_start_row {
                    let item_index = (row - content_start_row) as usize;

                    match app.current_tab {
                        0 => {
                            // 点击任务列表
                            if item_index < app.tasks.len() {
                                app.task_list_state.select(Some(item_index));
                            }
                        }
                        1 => {
                            // 点击便签列表
                            if item_index < app.notes.len() {
                                app.note_list_state.select(Some(item_index));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        MouseEventKind::ScrollDown => {
            match app.current_tab {
                0 => app.next_task(),
                1 => app.next_note(),
                _ => {}
            }
        }
        MouseEventKind::ScrollUp => {
            match app.current_tab {
                0 => app.previous_task(),
                1 => app.previous_note(),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// 渲染UI
fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // 标签页
            Constraint::Min(0),      // 内容
            Constraint::Length(2),   // 状态栏
        ])
        .split(f.area());

    // 标签页
    let titles = vec!["📝 Tasks (1)", "📓 Notes (2)", "🍅 Pomodoro (3)"];
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Task Manager"))
        .select(app.current_tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    // 内容区域
    match app.current_tab {
        0 => render_tasks(f, app, chunks[1]),
        1 => render_notes(f, app, chunks[1]),
        2 => render_pomodoro(f, app, chunks[1]),
        _ => {}
    }

    // 状态栏
    render_status_bar(f, app, chunks[2]);

    // 对话框
    if app.show_dialog != DialogType::None {
        render_dialog(f, app);
    }
}

/// 渲染任务列表
fn render_tasks(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .map(|task| {
            let priority_icon = match task.priority {
                Priority::High => "🔴",
                Priority::Medium => "🟡",
                Priority::Low => "🟢",
            };
            let status_icon = match task.status {
                TaskStatus::Completed => "✅",
                TaskStatus::InProgress => "🔄",
                TaskStatus::Todo => "⭕",
            };

            // 添加DDL显示
            let ddl_info = if let Some(due_date) = task.due_date {
                let local = due_date.with_timezone(&chrono::Local);
                format!(" [DDL: {}]", local.format("%m-%d %H:%M"))
            } else {
                String::new()
            };

            let content = format!("{} {} {}{}", status_icon, priority_icon, task.title, ddl_info);
            ListItem::new(content)
        })
        .collect();

    let help_text = if app.tasks.is_empty() {
        "按 'n' 创建新任务 | '?' 显示帮助"
    } else {
        "j/k:导航 | Space:切换状态 | t:设置DDL | p:优先级 | n:新建 | d:删除 | ?:帮助"
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("任务列表 ({} 个)", app.tasks.len()))
                .title_bottom(help_text),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.task_list_state);
}

/// 渲染便签列表 (平铺卡片式)
fn render_notes(f: &mut Frame, app: &mut App, area: Rect) {
    if app.notes.is_empty() {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from("还没有便签"),
            Line::from(""),
            Line::from("按 'n' 创建新便签"),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("便签墙")
        );
        f.render_widget(help, area);
        return;
    }

    // 计算卡片布局：每行3个卡片
    let cards_per_row = 3;
    let card_height = 8; // 每个卡片的高度
    let num_rows = (app.notes.len() + cards_per_row - 1) / cards_per_row;

    // 创建垂直布局
    let mut row_constraints = vec![];
    for _ in 0..num_rows {
        row_constraints.push(Constraint::Length(card_height));
    }
    row_constraints.push(Constraint::Min(0)); // 剩余空间

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .margin(1)
        .split(area);

    // 渲染每一行的卡片
    let selected_idx = app.note_list_state.selected().unwrap_or(0);

    for row_idx in 0..num_rows {
        let start_idx = row_idx * cards_per_row;
        let end_idx = (start_idx + cards_per_row).min(app.notes.len());

        // 创建该行的列布局
        let mut col_constraints = vec![];
        for _ in 0..(end_idx - start_idx) {
            col_constraints.push(Constraint::Percentage(100 / cards_per_row as u16));
        }

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(rows[row_idx]);

        // 渲染该行的每个卡片
        for (col_idx, note_idx) in (start_idx..end_idx).enumerate() {
            let note = &app.notes[note_idx];
            let is_selected = note_idx == selected_idx;

            // 截取内容预览（前3行）
            let content_preview: Vec<&str> = note.content
                .lines()
                .take(3)
                .collect();

            let mut lines = vec![];
            lines.push(Line::from(Span::styled(
                &note.title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            for line in content_preview {
                let truncated = if line.len() > 30 {
                    format!("{}...", &line[0..27])
                } else {
                    line.to_string()
                };
                lines.push(Line::from(Span::styled(
                    truncated,
                    Style::default().fg(Color::Gray),
                )));
            }

            let card_style = if is_selected {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let symbol = if is_selected { "▶ " } else { "" };
            let title = format!("{}📝 Note #{}", symbol, note_idx + 1);

            let card = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .style(card_style),
                )
                .wrap(Wrap { trim: true });

            f.render_widget(card, cols[col_idx]);
        }
    }

    // 渲染底部帮助栏
    let help_area = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };

    let help_text = "j/k:导航 | n:新建 | d:删除 | ?:帮助";
    let help = Paragraph::new(help_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    f.render_widget(help, help_area);
}

/// 渲染番茄钟
fn render_pomodoro(f: &mut Frame, app: &mut App, area: Rect) {
    let state_text = match app.pomodoro.state {
        crate::pomodoro::PomodoroState::Idle => "⏸️  空闲",
        crate::pomodoro::PomodoroState::Working => "🔥 工作中",
        crate::pomodoro::PomodoroState::Break => "☕ 休息中",
        crate::pomodoro::PomodoroState::Paused => "⏸️  已暂停",
    };

    let progress_bar = "█".repeat((app.pomodoro.progress() / 5.0) as usize);

    let mut content = vec![
        Line::from(""),
        Line::from(Span::styled(
            "🍅 番茄钟计时器",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("状态: "),
            Span::styled(state_text, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("剩余时间: "),
            Span::styled(
                app.pomodoro.format_remaining(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(format!("进度: [{}{}] {:.0}%",
            progress_bar,
            " ".repeat(20 - progress_bar.len()),
            app.pomodoro.progress()
        )),
        Line::from(""),
    ];

    // 统计信息
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "📊 统计",
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(format!("今日完成: {} 个番茄钟", app.pomodoro_completed_today)));
    content.push(Line::from(format!("专注时长: {} 分钟", app.pomodoro_total_minutes)));
    content.push(Line::from(""));

    // 配置信息
    content.push(Line::from(Span::styled(
        "⚙️ 配置",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    content.push(Line::from(format!("工作时长: {} 分钟", app.pomodoro.work_duration)));
    content.push(Line::from(format!("休息时长: {} 分钟", app.pomodoro.break_duration)));
    content.push(Line::from(""));
    content.push(Line::from(""));

    // 快捷键
    content.push(Line::from("快捷键:"));
    content.push(Line::from("  s       - 开始/暂停"));
    content.push(Line::from("  S       - 停止"));
    if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
        content.push(Line::from("  +/-     - 调整工作时长(±5分钟)"));
        content.push(Line::from("  [ / ]   - 调整休息时长(±1分钟)"));
    }

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// 渲染状态栏
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status = match app.input_mode {
        InputMode::Command => {
            // Command模式：显示正在输入的命令
            format!(":{}", app.input_buffer)
        }
        InputMode::Insert => {
            // Insert模式：显示模式名称
            "-- INSERT --".to_string()
        }
        InputMode::Normal => {
            // Normal模式：显示vim状态、数字前缀或状态消息
            let mut parts = vec![];

            // 显示数字前缀（如果有）
            if !app.number_prefix.is_empty() {
                parts.push(format!("[{}]", app.number_prefix));
            }

            // 显示等待中的按键（如 'g' 或 'd'）
            if let Some(last_key) = app.last_key {
                match last_key {
                    KeyCode::Char('g') => parts.push("[g]".to_string()),
                    KeyCode::Char('d') => parts.push("[d]".to_string()),
                    _ => {}
                }
            }

            // 显示状态消息或默认帮助
            if let Some(ref msg) = app.status_message {
                parts.push(msg.clone());
            } else if parts.is_empty() {
                parts.push("Tab/h/l:切换标签 | gg/G:首尾 | 5j:向下5行 | dd:删除 | n:新建 | ?:帮助 | :q退出".to_string());
            }

            parts.join(" ")
        }
    };

    let status_bar = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .block(Block::default());

    f.render_widget(status_bar, area);
}

/// 渲染对话框
fn render_dialog(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 40, f.area());

    let (title, content) = match app.show_dialog {
        DialogType::CreateTask => {
            ("创建新任务", vec![
                Line::from(""),
                Line::from("请输入任务标题:"),
                Line::from(""),
                Line::from(Span::styled(
                    &app.input_buffer,
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from("按 Enter 确认, Esc 取消"),
            ])
        }
        DialogType::CreateNote => {
            let (current_field, instructions) = if app.input_title.is_empty() {
                ("标题", "输入标题后按 Enter 继续")
            } else {
                ("内容", "输入内容后按 Enter 创建")
            };

            ("创建新便签", vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("第1步: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "便签标题",
                        if app.input_title.is_empty() {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Green)
                        }
                    ),
                ]),
                Line::from(Span::styled(
                    if app.input_title.is_empty() {
                        &app.input_buffer
                    } else {
                        &app.input_title
                    },
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("第2步: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "便签内容",
                        if !app.input_title.is_empty() {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }
                    ),
                ]),
                Line::from(Span::styled(
                    if !app.input_title.is_empty() {
                        &app.input_buffer
                    } else {
                        ""
                    },
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::raw("当前: "),
                    Span::styled(current_field, Style::default().fg(Color::Green)),
                ]),
                Line::from(""),
                Line::from(instructions),
                Line::from("Esc: 取消"),
            ])
        }
        DialogType::DeleteConfirm => {
            let item_name = if app.current_tab == 0 {
                app.selected_task().map(|t| t.title.as_str()).unwrap_or("")
            } else {
                app.selected_note().map(|n| n.title.as_str()).unwrap_or("")
            };

            ("确认删除", vec![
                Line::from(""),
                Line::from("确定要删除以下项目吗？"),
                Line::from(""),
                Line::from(Span::styled(
                    item_name,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(""),
                Line::from("y - 确认删除"),
                Line::from("n - 取消"),
            ])
        }
        DialogType::Help => {
            ("快捷键帮助", vec![
                Line::from(""),
                Line::from(Span::styled("导航", Style::default().add_modifier(Modifier::BOLD))),
                Line::from("  j/k, ↓/↑  : 上下移动"),
                Line::from("  h/l, ←/→  : 切换标签页"),
                Line::from("  g/G       : 跳到首/尾"),
                Line::from("  1/2/3     : 快速切换标签"),
                Line::from(""),
                Line::from(Span::styled("任务操作", Style::default().add_modifier(Modifier::BOLD))),
                Line::from("  n/a       : 新建"),
                Line::from("  Space/x   : 切换完成状态"),
                Line::from("  t         : 设置DDL时间"),
                Line::from("  d         : 删除"),
                Line::from("  p         : 切换优先级"),
                Line::from(""),
                Line::from(Span::styled("番茄钟", Style::default().add_modifier(Modifier::BOLD))),
                Line::from("  s         : 开始/暂停"),
                Line::from("  S         : 停止"),
                Line::from(""),
                Line::from(Span::styled("其他", Style::default().add_modifier(Modifier::BOLD))),
                Line::from("  q         : 退出"),
                Line::from("  ?         : 显示此帮助"),
                Line::from(""),
                Line::from("按任意键关闭"),
            ])
        }
        DialogType::SetDeadline => {
            // 构建日期时间选择器显示
            let field_names = ["年", "月", "日", "时", "分"];
            let values = [
                format!("{:04}", app.datetime_year),
                format!("{:02}", app.datetime_month),
                format!("{:02}", app.datetime_day),
                format!("{:02}", app.datetime_hour),
                format!("{:02}", app.datetime_minute),
            ];

            // 构建显示行，高亮当前选中的字段
            let mut datetime_spans = vec![];
            for i in 0..5 {
                if i == app.datetime_picker_field {
                    // 当前选中的字段：高亮显示
                    datetime_spans.push(Span::styled(
                        values[i].clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                } else {
                    datetime_spans.push(Span::raw(values[i].clone()));
                }

                // 添加分隔符
                if i < 2 {
                    datetime_spans.push(Span::raw("-"));
                } else if i == 2 {
                    datetime_spans.push(Span::raw("  "));
                } else if i == 3 {
                    datetime_spans.push(Span::raw(":"));
                }
            }

            {
                // 计算时间差
                let now = chrono::Local::now();
                let selected_dt = chrono::Local
                    .with_ymd_and_hms(
                        app.datetime_year,
                        app.datetime_month,
                        app.datetime_day,
                        app.datetime_hour,
                        app.datetime_minute,
                        0,
                    )
                    .single();

                let time_diff = if let Some(selected) = selected_dt {
                    let diff = selected.signed_duration_since(now);
                    let hours = diff.num_hours();
                    let days = diff.num_days();

                    if days > 0 {
                        format!("{} 天后", days)
                    } else if days < 0 {
                        format!("{} 天前 (已过期)", -days)
                    } else if hours > 0 {
                        format!("{} 小时后", hours)
                    } else if hours < 0 {
                        format!("{} 小时前 (已过期)", -hours)
                    } else {
                        "当前时间".to_string()
                    }
                } else {
                    "无效日期".to_string()
                };

                {
                    let mut content = vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "════════════════════════════════════════",
                            Style::default().fg(Color::DarkGray),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::raw("  "),
                            Span::styled(
                                "待设定时间:",
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(""),
                    ];

                    // 添加日期时间显示
                    let mut dt_line = vec![Span::raw("     ")];
                    dt_line.extend(datetime_spans);
                    content.push(Line::from(dt_line));

                    content.extend(vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::raw("  当前调整: "),
                            Span::styled(
                                field_names[app.datetime_picker_field],
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                            ),
                            Span::raw("  ("),
                            Span::styled(time_diff, Style::default().fg(Color::Green)),
                            Span::raw(")"),
                        ]),
                        Line::from(""),
                        Line::from(Span::styled(
                            "════════════════════════════════════════",
                            Style::default().fg(Color::DarkGray),
                        )),
                        Line::from(""),
                        Line::from("操作:"),
                        Line::from("  ↑/k 增加  ↓/j 减少"),
                        Line::from("  ←/h 上一字段  →/l 下一字段"),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("Enter", Style::default().fg(Color::Green)),
                            Span::raw(" 确认  "),
                            Span::styled("Esc", Style::default().fg(Color::Red)),
                            Span::raw(" 取消"),
                        ]),
                    ]);

                    ("设置DDL时间", content)
                }
            }
        }
        _ => ("", vec![]),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

/// 居中矩形
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
