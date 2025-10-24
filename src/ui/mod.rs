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
    pub cursor_position: usize, // 光标位置（字符索引）
    pub input_title: String,
    pub input_content: String, // 用于便签编辑时保存内容字段
    pub show_dialog: DialogType,
    pub status_message: Option<String>,
    pub note_edit_field: usize, // 0=标题, 1=内容
    pub pending_task_title: Option<String>, // 待创建任务的标题（用于强制设置DDL）
    // 日期时间选择器状态
    pub datetime_picker_field: usize, // 0=年, 1=月, 2=日, 3=时, 4=分
    pub datetime_input_buffer: String, // 当前字段的输入缓冲区（用于键盘直接输入）
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
    // 番茄钟计时控制
    pub last_tick_time: std::time::Instant,
    // 提示消息时间戳（用于自动消失）
    pub status_message_time: Option<std::time::Instant>,
    // 滚动偏移量
    pub help_scroll_offset: usize,
    pub pomodoro_scroll_offset: usize,
    pub note_scroll_offset: usize,
    pub view_note_scroll_offset: usize, // ViewNote对话框滚动
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
    ViewNote,
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
            cursor_position: 0,
            input_title: String::new(),
            input_content: String::new(),
            show_dialog: DialogType::None,
            status_message: None,
            note_edit_field: 0,
            pending_task_title: None,
            datetime_picker_field: 0,
            datetime_input_buffer: String::new(),
            datetime_year: now.year(),
            datetime_month: now.month(),
            datetime_day: now.day(),
            datetime_hour: now.hour(),
            datetime_minute: now.minute(),
            pomodoro_completed_today: 0,
            pomodoro_total_minutes: 0,
            last_key: None,
            number_prefix: String::new(),
            last_tick_time: std::time::Instant::now(),
            status_message_time: None,
            help_scroll_offset: 0,
            pomodoro_scroll_offset: 0,
            note_scroll_offset: 0,
            view_note_scroll_offset: 0,
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

    /// 设置状态消息（会自动记录时间戳，3秒后自动消失）
    pub fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
        self.status_message_time = Some(std::time::Instant::now());
    }

    /// 清除状态消息
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
        self.status_message_time = None;
    }

    /// 从数据库重新加载数据
    pub fn reload_data(&mut self) -> Result<()> {
        // 在重新加载数据之前，先保存当前选中任务的ID
        // 这很重要，因为重新加载后tasks数组会变化，但task_list_state的索引还是旧的
        let selected_task_id = self.selected_task().and_then(|t| t.id);

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

        // 在排序前，先根据保存的task id恢复选中状态
        // 这样sort_tasks就能正确保存和恢复选中位置
        if let Some(task_id) = selected_task_id {
            if let Some(new_index) = self.tasks.iter().position(|t| t.id == Some(task_id)) {
                self.task_list_state.select(Some(new_index));
            }
        }

        // 自动排序任务（会进一步保持选中状态）
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

    /// 任务自动排序（保持选中状态）
    /// 排序规则：
    /// 1. 未完成的任务优先（按状态：InProgress > Todo > Completed）
    /// 2. 在同状态下，按优先级排序（High > Medium > Low）
    /// 3. 在同优先级下，按DDL时间排序（有DDL的优先，且时间早的优先）
    fn sort_tasks(&mut self) {
        // 保存当前选中任务的ID
        let selected_task_id = self.selected_task().and_then(|t| t.id);

        // 执行排序
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

        // 恢复选中状态：找到之前选中任务的新位置
        if let Some(task_id) = selected_task_id {
            if let Some(new_index) = self.tasks.iter().position(|t| t.id == Some(task_id)) {
                self.task_list_state.select(Some(new_index));
            }
        }
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
            self.set_status_message("任务状态已更新".to_string());
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
        self.cursor_position = 0;
        self.show_dialog = DialogType::None;
        self.input_mode = InputMode::Normal;
        self.reload_data()?;
        self.set_status_message(format!("任务 #{} 已创建", id));

        Ok(())
    }

    /// 初始化编辑任务（加载当前任务内容到输入框）
    pub fn init_edit_task(&mut self) {
        if let Some(task) = self.selected_task().cloned() {
            self.input_buffer = task.title.clone();
            self.cursor_position = self.input_buffer.chars().count();
            self.show_dialog = DialogType::EditTask;
            self.input_mode = InputMode::Insert;
        }
    }

    /// 保存编辑后的任务
    pub fn save_edit_task(&mut self) -> Result<()> {
        if self.input_buffer.is_empty() {
            return Ok(());
        }

        if let Some(mut task) = self.selected_task().cloned() {
            task.title = self.input_buffer.clone();
            task.updated_at = chrono::Utc::now();

            let db = Database::open(&self.db_path)?;
            db.update_task(&task)?;

            self.input_buffer.clear();
            self.cursor_position = 0;
            self.show_dialog = DialogType::None;
            self.input_mode = InputMode::Normal;
            self.reload_data()?;
            self.set_status_message(format!("任务 #{} 已更新", task.id.unwrap_or(0)));
        }

        Ok(())
    }

    /// 删除任务
    pub fn delete_task(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            if let Some(id) = task.id {
                let db = Database::open(&self.db_path)?;
                db.delete_task(id)?;
                self.reload_data()?;
                self.set_status_message(format!("任务 #{} 已删除", id));
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
        self.cursor_position = 0;
        self.input_title.clear();
        self.show_dialog = DialogType::None;
        self.input_mode = InputMode::Normal;
        self.reload_data()?;
        self.set_status_message(format!("便签 #{} 已创建", id));

        Ok(())
    }

    /// 初始化编辑便签（加载当前便签内容到输入框）
    pub fn init_edit_note(&mut self) {
        if let Some(note) = self.selected_note().cloned() {
            self.input_title = note.title.clone();
            self.input_content = note.content.clone();
            self.input_buffer.clear(); // 清空buffer，等待用户选择字段
            self.note_edit_field = 0; // 从标题开始
            self.show_dialog = DialogType::EditNote;
            self.input_mode = InputMode::Normal; // 先进Normal模式，让用户选择编辑哪个字段
        }
    }

    /// 保存编辑后的便签
    pub fn save_edit_note(&mut self) -> Result<()> {
        if let Some(mut note) = self.selected_note().cloned() {
            note.title = self.input_title.clone();
            note.content = self.input_content.clone();
            note.updated_at = chrono::Utc::now();

            let db = Database::open(&self.db_path)?;
            db.update_note(&note)?;

            self.input_buffer.clear();
            self.cursor_position = 0;
            self.input_title.clear();
            self.input_content.clear();
            self.show_dialog = DialogType::None;
            self.input_mode = InputMode::Normal;
            self.note_edit_field = 0;
            self.reload_data()?;
            self.set_status_message(format!("便签 #{} 已更新", note.id.unwrap_or(0)));
        }

        Ok(())
    }

    /// 删除便签
    pub fn delete_note(&mut self) -> Result<()> {
        if let Some(note) = self.selected_note() {
            if let Some(id) = note.id {
                let db = Database::open(&self.db_path)?;
                db.delete_note(id)?;
                self.reload_data()?;
                self.set_status_message(format!("便签 #{} 已删除", id));
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
            self.set_status_message("优先级已更新".to_string());
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
        self.datetime_picker_apply_input(); // 切换字段前先应用当前输入
        self.datetime_picker_field = (self.datetime_picker_field + 1) % 5;
        self.datetime_input_buffer.clear();
    }

    /// 日期时间选择器：移动到上一个字段
    pub fn datetime_picker_prev_field(&mut self) {
        self.datetime_picker_apply_input(); // 切换字段前先应用当前输入
        if self.datetime_picker_field == 0 {
            self.datetime_picker_field = 4;
        } else {
            self.datetime_picker_field -= 1;
        }
        self.datetime_input_buffer.clear();
    }

    /// 日期时间选择器：应用输入缓冲区的值到当前字段
    fn datetime_picker_apply_input(&mut self) {
        if self.datetime_input_buffer.is_empty() {
            return;
        }

        if let Ok(value) = self.datetime_input_buffer.parse::<u32>() {
            match self.datetime_picker_field {
                0 => {
                    // 年份：2000-2099
                    if value >= 2000 && value <= 2099 {
                        self.datetime_year = value as i32;
                    }
                }
                1 => {
                    // 月份：1-12
                    if value >= 1 && value <= 12 {
                        self.datetime_month = value;
                    }
                }
                2 => {
                    // 日期：1-31（根据月份验证）
                    let max_day = Self::days_in_month(self.datetime_year, self.datetime_month);
                    if value >= 1 && value <= max_day {
                        self.datetime_day = value;
                    }
                }
                3 => {
                    // 小时：0-23
                    if value <= 23 {
                        self.datetime_hour = value;
                    }
                }
                4 => {
                    // 分钟：0-59
                    if value <= 59 {
                        self.datetime_minute = value;
                    }
                }
                _ => {}
            }
        }
    }

    /// 日期时间选择器：添加数字到输入缓冲区
    pub fn datetime_picker_input_digit(&mut self, digit: char) {
        // 根据当前字段限制输入长度
        let max_len = match self.datetime_picker_field {
            0 => 4, // 年份：4位
            1 | 2 => 2, // 月日：2位
            3 | 4 => 2, // 时分：2位
            _ => 2,
        };

        if self.datetime_input_buffer.len() < max_len {
            self.datetime_input_buffer.push(digit);
        }
    }

    /// 日期时间选择器：删除输入缓冲区的最后一个字符
    pub fn datetime_picker_backspace(&mut self) {
        self.datetime_input_buffer.pop();
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

    /// 应用选中的日期时间到当前任务或创建新任务
    pub fn apply_deadline(&mut self) -> Result<()> {
        let db_path = self.db_path.clone();

        // 先提取datetime值，避免借用冲突
        let year = self.datetime_year;
        let month = self.datetime_month;
        let day = self.datetime_day;
        let hour = self.datetime_hour;
        let minute = self.datetime_minute;

        // 创建本地时间
        let local_dt = chrono::Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single();

        if let Some(local_dt) = local_dt {
            let due_date = Some(local_dt.with_timezone(&Utc));

            // 检查是否是为新任务设置DDL
            if let Some(title) = self.pending_task_title.take() {
                // 创建新任务并设置DDL
                let db = Database::open(&db_path)?;
                let mut task = Task::new(title);
                task.due_date = due_date;
                let id = db.create_task(&task)?;
                self.set_status_message(format!(
                    "任务 #{} 已创建，DDL: {}-{:02}-{:02} {:02}:{:02}",
                    id, year, month, day, hour, minute
                ));
            } else if let Some(task) = self.selected_task_mut() {
                // 为现有任务设置DDL
                task.due_date = due_date;
                task.updated_at = Utc::now();

                let db = Database::open(&db_path)?;
                db.update_task(task)?;
                self.set_status_message(format!(
                    "DDL已设置: {}-{:02}-{:02} {:02}:{:02}",
                    year, month, day, hour, minute
                ));
            }
        } else {
            self.set_status_message("无效的日期时间".to_string());
            // 如果日期无效，清除pending_task_title避免状态混乱
            self.pending_task_title = None;
        }

        // 立即重新排序
        self.sort_tasks();
        self.show_dialog = DialogType::None;
        Ok(())
    }

    /// 计算 ViewNote 对话框的最大滚动偏移量
    pub fn get_view_note_max_scroll(&self) -> usize {
        if let Some(note) = self.selected_note() {
            // 计算便签内容的总行数
            let mut total_lines = note.content.lines().count();
            total_lines += 10; // 加上其他信息行（标题、分隔线、时间戳、快捷键等）

            // 假设对话框窗口高度为 30 行（居中矩形 40% 的高度）
            let window_height = 30;

            // 最大滚动偏移量 = 总行数 - 窗口高度
            total_lines.saturating_sub(window_height)
        } else {
            0
        }
    }

    /// 计算帮助对话框的最大滚动偏移量
    pub fn get_help_max_scroll(&self) -> usize {
        // 每个标签页的帮助内容行数（实际统计）
        let help_lines: usize = match self.current_tab {
            0 => 36,  // 任务管理帮助（导航4行+任务操作6行+命令模式7行+分隔线+提示）
            1 => 30,  // 便签墙帮助
            2 => 25,  // 番茄钟帮助
            _ => 20,
        };
        let window_height: usize = 20; // 对话框可显示的行数
        help_lines.saturating_sub(window_height)
    }

    /// 计算番茄钟界面的最大滚动偏移量
    pub fn get_pomodoro_max_scroll(&self) -> usize {
        // 番茄钟内容的行数（通常很少，所以通常不滚动）
        let content_lines: usize = 20; // 估算行数
        let window_height: usize = 40; // 番茄钟占据大部分空间
        content_lines.saturating_sub(window_height)
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

        // 使用较短的 poll 间隔以提高响应性，但用时间戳控制 tick 频率
        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key_event(app, key.code)?;
                }
                // 暂时禁用鼠标响应，后续再完善
                // Event::Mouse(mouse) => {
                //     if mouse.kind != MouseEventKind::Moved {
                //         handle_mouse_event(app, mouse)?;
                //     }
                // }
                _ => {}
            }
        }

        // 检查并清除过期的状态消息（3秒后自动消失）
        if let Some(msg_time) = app.status_message_time {
            let now = std::time::Instant::now();
            if now.duration_since(msg_time) >= std::time::Duration::from_secs(3) {
                app.clear_status_message();
            }
        }

        // 番茄钟计时：基于时间戳，确保严格按1秒间隔执行
        if app.pomodoro.state == crate::pomodoro::PomodoroState::Working
            || app.pomodoro.state == crate::pomodoro::PomodoroState::Break
        {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(app.last_tick_time);

            // 只有距离上次 tick 超过 1 秒才执行
            if elapsed >= std::time::Duration::from_secs(1) {
                app.last_tick_time = now;

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
                        app.set_status_message("🎉 工作时段完成！开始休息！".to_string());
                    }
                    crate::pomodoro::PomodoroState::Break => {
                        app.pomodoro.stop();
                        app.set_status_message("番茄钟完成！".to_string());
                    }
                    _ => {}
                }
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
    let cmd = app.input_buffer.trim().to_string();

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
                        app.set_status_message(format!("跳转到第{}行", line_num));
                    }
                }
                1 => {
                    if line_num <= app.notes.len() {
                        app.note_list_state.select(Some(line_num - 1));
                        app.set_status_message(format!("跳转到第{}行", line_num));
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
                        // 新建任务时强制设定DDL
                        app.pending_task_title = Some(title.clone());
                        // 初始化datetime picker为当前时间
                        let now = chrono::Local::now();
                        app.datetime_year = now.year();
                        app.datetime_month = now.month();
                        app.datetime_day = now.day();
                        app.datetime_hour = now.hour();
                        app.datetime_minute = now.minute();
                        app.datetime_picker_field = 0;
                        app.show_dialog = DialogType::SetDeadline;
                    }
                    1 => {
                        let db = Database::open(&app.db_path)?;
                        let note = Note::new("新便签".to_string(), title.clone());
                        let id = db.create_note(&note)?;
                        app.reload_data()?;
                        app.set_status_message(format!("便签 #{} 已创建", id));
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
            match app.current_tab {
                0 => {
                    if !app.tasks.is_empty() {
                        app.init_edit_task();
                    } else {
                        app.set_status_message("没有可编辑的任务".to_string());
                    }
                }
                1 => {
                    if !app.notes.is_empty() {
                        app.init_edit_note();
                    } else {
                        app.set_status_message("没有可编辑的便签".to_string());
                    }
                }
                _ => {}
            }
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
                                        app.set_status_message(format!("工作时长设置为 {} 分钟", minutes));
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
                                        app.set_status_message(format!("休息时长设置为 {} 分钟", minutes));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                app.set_status_message(format!(
                    "番茄钟配置: 工作{}分钟 休息{}分钟 | 用法: :pomo work=25 break=5",
                    app.pomodoro.work_duration,
                    app.pomodoro.break_duration
                ));
            }
        }

        // 切换优先级命令（支持参数：1=Low, 2=Medium, 3=High）
        "p" | "priority" => {
            if app.current_tab == 0 {
                if app.tasks.is_empty() {
                    app.set_status_message("没有任务可设置优先级".to_string());
                } else if parts.len() > 1 {
                    // 带参数：设置指定优先级
                    if let Some(mut task) = app.selected_task().cloned() {
                        let old_priority = task.priority;
                        match parts[1] {
                            "1" | "low" | "l" => {
                                task.priority = crate::models::Priority::Low;
                            }
                            "2" | "medium" | "m" => {
                                task.priority = crate::models::Priority::Medium;
                            }
                            "3" | "high" | "h" => {
                                task.priority = crate::models::Priority::High;
                            }
                            _ => {
                                app.set_status_message("用法: :p [1/low | 2/medium | 3/high]".to_string());
                                return Ok(());
                            }
                        }
                        let db = Database::open(&app.db_path)?;
                        db.update_task(&task)?;
                        app.reload_data()?;
                        app.set_status_message(format!("优先级: {:?} → {:?}", old_priority, task.priority));
                    } else {
                        app.set_status_message("没有选中的任务".to_string());
                    }
                } else {
                    // 无参数：循环切换
                    app.cycle_priority()?;
                }
            } else {
                app.set_status_message("只有任务才有优先级".to_string());
            }
        }

        // 切换完成状态命令（建议用Space键）
        "toggle" | "x" => {
            if app.current_tab == 0 {
                app.toggle_task_status()?;
            } else {
                app.set_status_message("只有任务才能切换完成状态 | 提示：用Space键更快".to_string());
            }
        }

        // 设置DDL命令（t=time/deadline）
        "t" | "ddl" | "deadline" | "due" => {
            if app.current_tab == 0 && !app.tasks.is_empty() {
                app.init_datetime_picker();
                app.show_dialog = DialogType::SetDeadline;
            } else {
                app.set_status_message("没有可设置DDL的任务 | 提示：按t键设置DDL".to_string());
            }
        }

        // 番茄钟开始/暂停命令
        "s" | "start" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页 (Tab 3)".to_string());
            } else {
                match app.pomodoro.state {
                    crate::pomodoro::PomodoroState::Idle => {
                        app.pomodoro.start_work(None);
                        app.set_status_message("番茄钟开始！".to_string());
                    }
                    crate::pomodoro::PomodoroState::Working
                    | crate::pomodoro::PomodoroState::Break => {
                        app.pomodoro.pause();
                        app.set_status_message("已暂停".to_string());
                    }
                    crate::pomodoro::PomodoroState::Paused => {
                        app.pomodoro.resume();
                        app.set_status_message("继续计时".to_string());
                    }
                }
            }
        }

        // 番茄钟取消命令
        "c" | "cancel" | "stop" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页 (Tab 3)".to_string());
            } else {
                if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                    app.pomodoro.stop();
                    app.set_status_message("番茄钟已取消".to_string());
                } else {
                    app.set_status_message("番茄钟未运行".to_string());
                }
            }
        }

        // 调整工作时长命令
        "work+" | "w+" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页".to_string());
            } else if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                app.set_status_message("番茄钟运行中，无法调整！先用:c取消".to_string());
            } else {
                app.pomodoro.work_duration += 5;
                if app.pomodoro.work_duration > 120 {
                    app.pomodoro.work_duration = 120;
                }
                if let Ok(db) = Database::open(&app.db_path) {
                    let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                }
                app.set_status_message(format!("工作时长: {}分钟", app.pomodoro.work_duration));
            }
        }
        "work-" | "w-" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页".to_string());
            } else if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                app.set_status_message("番茄钟运行中，无法调整！先用:c取消".to_string());
            } else {
                if app.pomodoro.work_duration > 5 {
                    app.pomodoro.work_duration -= 5;
                    if let Ok(db) = Database::open(&app.db_path) {
                        let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                    }
                    app.set_status_message(format!("工作时长: {}分钟", app.pomodoro.work_duration));
                } else {
                    app.set_status_message("工作时长最小为5分钟".to_string());
                }
            }
        }

        // 调整休息时长命令
        "break+" | "b+" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页".to_string());
            } else if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                app.set_status_message("番茄钟运行中，无法调整！先用:c取消".to_string());
            } else {
                app.pomodoro.break_duration += 1;
                if app.pomodoro.break_duration > 60 {
                    app.pomodoro.break_duration = 60;
                }
                if let Ok(db) = Database::open(&app.db_path) {
                    let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                }
                app.set_status_message(format!("休息时长: {}分钟", app.pomodoro.break_duration));
            }
        }
        "break-" | "b-" => {
            if app.current_tab != 2 {
                app.set_status_message("请先切换到番茄钟标签页".to_string());
            } else if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                app.set_status_message("番茄钟运行中，无法调整！先用:c取消".to_string());
            } else {
                if app.pomodoro.break_duration > 1 {
                    app.pomodoro.break_duration -= 1;
                    if let Ok(db) = Database::open(&app.db_path) {
                        let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                    }
                    app.set_status_message(format!("休息时长: {}分钟", app.pomodoro.break_duration));
                } else {
                    app.set_status_message("休息时长最小为1分钟".to_string());
                }
            }
        }

        // 帮助命令
        "h" | "help" | "?" => {
            app.show_dialog = DialogType::Help;
        }

        // 排序命令
        "sort" => {
            if app.current_tab == 0 {
                app.sort_tasks();
                app.set_status_message("已排序任务".to_string());
            } else {
                app.set_status_message("只有任务可以排序".to_string());
            }
        }

        // 未知命令
        _ => {
            app.set_status_message(format!("未知命令: {} | 输入:h查看帮助", cmd));
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
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    app.datetime_picker_next_field();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.datetime_picker_increment();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.datetime_picker_decrement();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    // 数字键：直接输入
                    app.datetime_picker_input_digit(c);
                }
                KeyCode::Backspace => {
                    // 退格键：删除输入缓冲区的最后一个字符
                    app.datetime_picker_backspace();
                }
                KeyCode::Enter => {
                    // 先应用当前输入，再保存DDL
                    app.datetime_picker_apply_input();
                    app.datetime_input_buffer.clear();
                    app.apply_deadline()?;
                }
                KeyCode::Esc => {
                    // 取消设置DDL，如果是新建任务的流程，也要清除pending_task_title
                    app.pending_task_title = None;
                    app.datetime_input_buffer.clear();
                    app.show_dialog = DialogType::None;
                }
                _ => {}
            }
            return Ok(());
        }

        // 特殊处理：Help dialog 支持滚动
        if app.show_dialog == DialogType::Help {
            let max_scroll = app.get_help_max_scroll();
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.help_scroll_offset > 0 {
                        app.help_scroll_offset -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.help_scroll_offset = (app.help_scroll_offset + 1).min(max_scroll);
                }
                KeyCode::PageUp => {
                    app.help_scroll_offset = app.help_scroll_offset.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    app.help_scroll_offset = (app.help_scroll_offset + 10).min(max_scroll);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    app.help_scroll_offset = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    app.help_scroll_offset = max_scroll;
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                    app.help_scroll_offset = 0;
                    app.show_dialog = DialogType::None;
                }
                _ => {}
            }
            return Ok(());
        }

        // 特殊处理：ViewNote dialog 支持滚动和编辑
        if app.show_dialog == DialogType::ViewNote {
            let max_scroll = app.get_view_note_max_scroll();
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.view_note_scroll_offset > 0 {
                        app.view_note_scroll_offset -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.view_note_scroll_offset = (app.view_note_scroll_offset + 1).min(max_scroll);
                }
                KeyCode::PageUp => {
                    app.view_note_scroll_offset = app.view_note_scroll_offset.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    app.view_note_scroll_offset = (app.view_note_scroll_offset + 10).min(max_scroll);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    app.view_note_scroll_offset = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    // 滚到底部
                    app.view_note_scroll_offset = max_scroll;
                }
                KeyCode::Char('e') => {
                    // 编辑当前便签
                    if let Some(note) = app.selected_note().cloned() {
                        app.input_title = note.title;
                        app.input_content = note.content;
                        app.note_edit_field = 0;
                        app.input_mode = InputMode::Normal;
                        app.show_dialog = DialogType::EditNote;
                        app.view_note_scroll_offset = 0;
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.view_note_scroll_offset = 0;
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
                        app.cursor_position = 0;
                        app.input_title.clear();
                        app.show_dialog = DialogType::None;
                    }
                    KeyCode::Enter => {
                        match app.show_dialog {
                            DialogType::CreateTask => {
                                // 新建任务时强制设定DDL
                                if !app.input_buffer.is_empty() {
                                    app.pending_task_title = Some(app.input_buffer.clone());
                                    app.input_buffer.clear();
                                    app.cursor_position = 0;
                                    app.input_mode = InputMode::Normal;
                                    // 初始化datetime picker为当前时间
                                    let now = chrono::Local::now();
                                    app.datetime_year = now.year();
                                    app.datetime_month = now.month();
                                    app.datetime_day = now.day();
                                    app.datetime_hour = now.hour();
                                    app.datetime_minute = now.minute();
                                    app.datetime_picker_field = 0;
                                    app.show_dialog = DialogType::SetDeadline;
                                }
                            }
                            DialogType::EditTask => app.save_edit_task()?,
                            DialogType::CreateNote => {
                                // Tab键才切换到内容，Enter在有标题后创建
                                if !app.input_title.is_empty() {
                                    app.create_note()?;
                                } else {
                                    // 第一次Enter：将buffer内容作为标题
                                    app.input_title = app.input_buffer.clone();
                                    app.input_buffer.clear();
                                    app.cursor_position = 0;
                                }
                            }
                            DialogType::EditNote => {
                                // 根据当前编辑的字段保存
                                if app.note_edit_field == 0 {
                                    // 保存标题到input_title，返回Normal模式让用户选择下一步
                                    app.input_title = app.input_buffer.clone();
                                    app.input_buffer.clear();
                                    app.cursor_position = 0;
                                    app.input_mode = InputMode::Normal;
                                } else {
                                    // 保存内容到input_content，然后完成整个编辑
                                    app.input_content = app.input_buffer.clone();
                                    app.save_edit_note()?;
                                }
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Char(c) => {
                        // 在光标位置插入字符
                        let byte_pos = app.input_buffer.char_indices()
                            .nth(app.cursor_position)
                            .map(|(pos, _)| pos)
                            .unwrap_or(app.input_buffer.len());
                        app.input_buffer.insert(byte_pos, c);
                        app.cursor_position += 1;
                    }
                    KeyCode::Backspace => {
                        // 删除光标前的字符
                        if app.cursor_position > 0 {
                            let byte_pos = app.input_buffer.char_indices()
                                .nth(app.cursor_position - 1)
                                .map(|(pos, _)| pos)
                                .unwrap_or(0);
                            app.input_buffer.remove(byte_pos);
                            app.cursor_position -= 1;
                        }
                    }
                    KeyCode::Delete => {
                        // 删除光标位置的字符
                        if app.cursor_position < app.input_buffer.chars().count() {
                            let byte_pos = app.input_buffer.char_indices()
                                .nth(app.cursor_position)
                                .map(|(pos, _)| pos)
                                .unwrap_or(app.input_buffer.len());
                            app.input_buffer.remove(byte_pos);
                        }
                    }
                    KeyCode::Left => {
                        // 向左移动光标
                        if app.cursor_position > 0 {
                            app.cursor_position -= 1;
                        }
                    }
                    KeyCode::Right => {
                        // 向右移动光标
                        let len = app.input_buffer.chars().count();
                        if app.cursor_position < len {
                            app.cursor_position += 1;
                        }
                    }
                    KeyCode::Home => {
                        // 移动到行首
                        app.cursor_position = 0;
                    }
                    KeyCode::End => {
                        // 移动到行尾
                        app.cursor_position = app.input_buffer.chars().count();
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
                            // 对于EditNote，先加载对应字段到input_buffer
                            if app.show_dialog == DialogType::EditNote {
                                if app.note_edit_field == 0 {
                                    // 编辑标题：从input_title加载
                                    app.input_buffer = app.input_title.clone();
                                } else {
                                    // 编辑内容：从input_content加载
                                    app.input_buffer = app.input_content.clone();
                                }
                            }
                            // 进入Insert模式，光标移到末尾
                            app.cursor_position = app.input_buffer.chars().count();
                            app.input_mode = InputMode::Insert;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        // 在EditNote对话框中用方向键切换字段
                        if app.show_dialog == DialogType::EditNote {
                            app.note_edit_field = 0; // 切换到标题
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        // 在EditNote对话框中用方向键切换字段
                        if app.show_dialog == DialogType::EditNote {
                            app.note_edit_field = 1; // 切换到内容
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
                app.cursor_position = 0;
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
                app.cursor_position = 0;
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
                    app.cursor_position = 0;
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
                        2 => {
                            // 番茄钟界面滚动
                            let max_scroll = app.get_pomodoro_max_scroll();
                            app.pomodoro_scroll_offset = (app.pomodoro_scroll_offset + count).min(max_scroll);
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
                        2 => {
                            // 番茄钟界面向上滚动
                            app.pomodoro_scroll_offset = app.pomodoro_scroll_offset.saturating_sub(count);
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
                            2 => app.pomodoro_scroll_offset = 0, // 番茄钟滚动到顶部
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

                // 任务操作（高频：保留单键）
                KeyCode::Char('n') | KeyCode::Char('a') | KeyCode::Char('o') | KeyCode::Char('O') => {
                    // 新建 (vim风格: n/a/o/O都可以) - 也可以用 :new 带参数
                    match app.current_tab {
                        0 => {
                            app.show_dialog = DialogType::CreateTask;
                            app.input_mode = InputMode::Insert;
                            app.input_buffer.clear();
                            app.cursor_position = 0;
                        }
                        1 => {
                            app.show_dialog = DialogType::CreateNote;
                            app.input_mode = InputMode::Insert;
                            app.input_buffer.clear();
                            app.cursor_position = 0;
                            app.input_title.clear();
                            app.input_content.clear();
                        }
                        _ => {}
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Enter => {
                    // Enter: 便签界面查看详情
                    if app.current_tab == 1 && !app.notes.is_empty() {
                        app.show_dialog = DialogType::ViewNote;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('e') => {
                    // 编辑当前项（高频）- 也可以用 :e 或 :edit
                    match app.current_tab {
                        0 => {
                            if !app.tasks.is_empty() {
                                app.init_edit_task();
                            }
                        }
                        1 => {
                            if !app.notes.is_empty() {
                                app.init_edit_note();
                            }
                        }
                        _ => {}
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char(' ') | KeyCode::Char('x') => {
                    // 切换完成状态（高频）- Space键是Vim风格的任务切换
                    if app.current_tab == 0 {
                        app.toggle_task_status()?;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('d') => {
                    // 删除（高频）- dd删除，也可以用 :d 或 :delete
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
                    // 切换优先级（中频）- 也可以用 :p 或 :priority
                    if app.current_tab == 0 {
                        app.cycle_priority()?;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('t') => {
                    // 设置DDL时间（中频）- t=time/deadline，也可以用 :ddl
                    if app.current_tab == 0 && !app.tasks.is_empty() {
                        app.init_datetime_picker();
                        app.show_dialog = DialogType::SetDeadline;
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }

                // 番茄钟操作（仅在番茄钟标签页有效）
                KeyCode::Char('s') => {
                    // 开始/暂停番茄钟（高频）- 也可以用 :s 或 :start
                    if app.current_tab == 2 {
                        match app.pomodoro.state {
                            crate::pomodoro::PomodoroState::Idle => {
                                app.pomodoro.start_work(None);
                                app.set_status_message("番茄钟开始！".to_string());
                            }
                            crate::pomodoro::PomodoroState::Working
                            | crate::pomodoro::PomodoroState::Break => {
                                app.pomodoro.pause();
                                app.set_status_message("已暂停".to_string());
                            }
                            crate::pomodoro::PomodoroState::Paused => {
                                app.pomodoro.resume();
                                app.set_status_message("继续计时".to_string());
                            }
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('S') | KeyCode::Char('c') => {
                    // 停止/取消番茄钟 - 也可以用 :c 或 :cancel
                    if app.current_tab == 2 {
                        // 只有在计时器运行或暂停时才需要停止
                        if app.pomodoro.state != crate::pomodoro::PomodoroState::Idle {
                            app.pomodoro.stop();
                            app.set_status_message("番茄钟已取消".to_string());
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                // 番茄钟自定义时长（仅在空闲状态下可调整）
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    // 增加工作时长 - 也可以用 :work+ 或 :w+
                    if app.current_tab == 2 {
                        if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                            app.pomodoro.work_duration += 5;
                            if app.pomodoro.work_duration > 120 {
                                app.pomodoro.work_duration = 120; // 最大120分钟
                            }
                            // 保存配置到数据库
                            if let Ok(db) = Database::open(&app.db_path) {
                                let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                            }
                            app.set_status_message(format!("工作时长: {}分钟 (已保存)", app.pomodoro.work_duration));
                        } else {
                            app.set_status_message("番茄钟运行中，无法调整时长！按S或c取消后再调整".to_string());
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('-') | KeyCode::Char('_') => {
                    // 减少工作时长 - 也可以用 :work- 或 :w-
                    if app.current_tab == 2 {
                        if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                            if app.pomodoro.work_duration > 5 {
                                app.pomodoro.work_duration -= 5;
                                // 保存配置到数据库
                                if let Ok(db) = Database::open(&app.db_path) {
                                    let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                                }
                                app.set_status_message(format!("工作时长: {}分钟 (已保存)", app.pomodoro.work_duration));
                            } else {
                                app.set_status_message("工作时长最小为5分钟".to_string());
                            }
                        } else {
                            app.set_status_message("番茄钟运行中，无法调整时长！按S或c取消后再调整".to_string());
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char('[') => {
                    // 增加休息时长 - 也可以用 :break+ 或 :b+
                    if app.current_tab == 2 {
                        if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                            app.pomodoro.break_duration += 1;
                            if app.pomodoro.break_duration > 60 {
                                app.pomodoro.break_duration = 60; // 最大60分钟
                            }
                            // 保存配置到数据库
                            if let Ok(db) = Database::open(&app.db_path) {
                                let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                            }
                            app.set_status_message(format!("休息时长: {}分钟 (已保存)", app.pomodoro.break_duration));
                        } else {
                            app.set_status_message("番茄钟运行中，无法调整时长！按S或c取消后再调整".to_string());
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
                }
                KeyCode::Char(']') => {
                    // 减少休息时长 - 也可以用 :break- 或 :b-
                    if app.current_tab == 2 {
                        if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
                            if app.pomodoro.break_duration > 1 {
                                app.pomodoro.break_duration -= 1;
                                // 保存配置到数据库
                                if let Ok(db) = Database::open(&app.db_path) {
                                    let _ = db.save_pomodoro_config(app.pomodoro.work_duration, app.pomodoro.break_duration);
                                }
                                app.set_status_message(format!("休息时长: {}分钟 (已保存)", app.pomodoro.break_duration));
                            } else {
                                app.set_status_message("休息时长最小为1分钟".to_string());
                            }
                        } else {
                            app.set_status_message("番茄钟运行中，无法调整时长！按S或c取消后再调整".to_string());
                        }
                    }
                    app.number_prefix.clear();
                    app.last_key = Some(key);
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

                // // q键: 退出 - 现在使用 :q
                // KeyCode::Char('q') => {
                //     app.should_quit = true;
                // }

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

/// 处理鼠标事件 (支持响应式布局)
fn handle_mouse_event(app: &mut App, mouse: MouseEvent) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse.row;
            let col = mouse.column;

            // 获取终端尺寸以计算响应式布局
            if let Ok((width, height)) = crossterm::terminal::size() {
                // 重新计算布局区域，与ui函数保持一致
                let full_rect = Rect::new(0, 0, width, height);
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),   // 标签页
                        Constraint::Min(0),      // 内容
                        Constraint::Length(2),   // 状态栏
                    ])
                    .split(full_rect);

                let tabs_area = chunks[0];      // 标签页区域
                let content_area = chunks[1];    // 内容区域

                // 点击标签页区域
                if row >= tabs_area.y && row < tabs_area.y + tabs_area.height {
                    // 动态计算每个标签的宽度（考虑边框）
                    let inner_width = tabs_area.width.saturating_sub(2); // 减去左右边框
                    let tab_width = inner_width / 3; // 3个标签平分宽度

                    // 计算点击位置在标签内的相对列位置（排除左边框）
                    let relative_col = col.saturating_sub(tabs_area.x + 1);

                    if relative_col < tab_width {
                        app.goto_tab(0);
                    } else if relative_col < tab_width * 2 {
                        app.goto_tab(1);
                    } else if relative_col < tab_width * 3 {
                        app.goto_tab(2);
                    }
                }
                // 点击内容区域 - 选择列表项
                else if row >= content_area.y && row < content_area.y + content_area.height {
                    match app.current_tab {
                        0 => {
                            // 任务列表: Block有上边框(1行) + 标题行(1行) = 2行偏移
                            // 底部还有边框(1行) + 帮助文本(1行，在边框内)
                            let top_offset = 2; // 上边框 + 标题
                            let bottom_offset = 2; // 底边框 + 底部帮助

                            // 可点击的内容起始行
                            let content_start_row = content_area.y + top_offset;
                            let content_end_row = content_area.y + content_area.height - bottom_offset;

                            if row >= content_start_row && row < content_end_row {
                                let item_index = (row - content_start_row) as usize;
                                if item_index < app.tasks.len() {
                                    app.task_list_state.select(Some(item_index));
                                }
                            }
                        }
                        1 => {
                            // 便签列表 - 支持卡片点击
                            // 卡片布局参数（需要与render_notes保持一致）
                            let cards_per_row = 3;
                            let card_height = 8;

                            // 内容区有1行margin
                            let margin = 1;
                            let content_start_row = content_area.y + margin;

                            if row >= content_start_row && !app.notes.is_empty() {
                                let relative_row = row - content_start_row;
                                let card_row = (relative_row / card_height) as usize;

                                // 计算卡片所在的列（每行3个卡片）
                                // 每个卡片占据 width/3 的宽度
                                let content_width = content_area.width.saturating_sub(margin * 2);
                                let card_width = content_width / cards_per_row as u16;
                                let relative_col = col.saturating_sub(content_area.x + margin);
                                let card_col = (relative_col / card_width).min(cards_per_row as u16 - 1) as usize;

                                // 计算点击的便签索引
                                let note_index = card_row * cards_per_row + card_col;

                                if note_index < app.notes.len() {
                                    app.note_list_state.select(Some(note_index));
                                }
                            }
                        }
                        2 => {
                            // 番茄钟界面 - 可以考虑添加按钮点击支持
                            // 当前暂不支持，保留滚轮功能即可
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Task Manager", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        )
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
    // 如果没有任务，显示欢迎提示
    if app.tasks.is_empty() {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "📝 欢迎使用任务管理器",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("快捷键:"),
            Line::from("  n/a/o - 创建新任务"),
            Line::from("  :new <标题> - 命令创建任务"),
            Line::from(""),
            Line::from("开始创建你的第一个任务吧！"),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 任务列表 ")
                .border_style(Style::default().fg(Color::Cyan))
        );
        f.render_widget(help, area);
        return;
    }

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
                format!(" [DDL: {}]", local.format("%Y-%m-%d %H:%M"))
            } else {
                String::new()
            };

            let content = format!("{} {} {}{}", status_icon, priority_icon, task.title, ddl_info);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    format!(" 任务列表 ({} 个) ", app.tasks.len()),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
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
            Line::from(Span::styled(
                "📓 便签墙",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("这里还没有便签"),
            Line::from(""),
            Line::from("快捷键:"),
            Line::from("  n/a/o - 创建新便签"),
            Line::from("  :new <内容> - 命令创建便签"),
            Line::from(""),
            Line::from("记录你的灵感和想法吧！"),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
                .title(" 便签墙 ")
        );
        f.render_widget(help, area);
        return;
    }

    // 计算卡片布局：每行3个卡片
    let cards_per_row = 3;
    let card_height = 8; // 每个卡片的高度
    let num_rows = (app.notes.len() + cards_per_row - 1) / cards_per_row;

    // 计算可见区域可以显示多少行
    let visible_rows = ((area.height.saturating_sub(2)) / card_height) as usize; // 减去边框
    let visible_rows = visible_rows.max(1); // 至少显示1行

    // 根据选中的便签自动调整滚动偏移量
    let selected_idx = app.note_list_state.selected().unwrap_or(0);
    let selected_row = selected_idx / cards_per_row;

    // 确保选中的行在可见范围内
    let mut scroll_offset = app.note_scroll_offset;
    if selected_row < scroll_offset {
        scroll_offset = selected_row;
    } else if selected_row >= scroll_offset + visible_rows {
        scroll_offset = selected_row.saturating_sub(visible_rows - 1);
    }

    // 限制滚动偏移量
    let max_scroll = num_rows.saturating_sub(visible_rows);
    scroll_offset = scroll_offset.min(max_scroll);

    // 更新app的滚动偏移量
    app.note_scroll_offset = scroll_offset;

    // 计算显示的行范围
    let start_row = scroll_offset;
    let end_row = (scroll_offset + visible_rows).min(num_rows);
    let visible_row_count = end_row - start_row;

    // 创建垂直布局（只为可见的行）
    let mut row_constraints = vec![];
    for _ in 0..visible_row_count {
        row_constraints.push(Constraint::Length(card_height));
    }
    row_constraints.push(Constraint::Min(0)); // 剩余空间

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .margin(1)
        .split(area);

    // 渲染可见的卡片行
    let selected_idx = app.note_list_state.selected().unwrap_or(0);

    for (display_row_idx, row_idx) in (start_row..end_row).enumerate() {
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
            .split(rows[display_row_idx]);

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

            let (card_style, border_style) = if is_selected {
                (
                    Style::default().fg(Color::White),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    Style::default().fg(Color::Gray),
                    Style::default().fg(Color::Magenta),
                )
            };

            let symbol = if is_selected { "▶ " } else { "  " };
            let title = format!("{}📝 便签 #{}", symbol, note_idx + 1);

            let card = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style)
                        .title(Span::styled(title, card_style)),
                )
                .wrap(Wrap { trim: true });

            f.render_widget(card, cols[col_idx]);
        }
    }
}

/// 渲染番茄钟
fn render_pomodoro(f: &mut Frame, app: &mut App, area: Rect) {
    // 分割界面：计时显示 + 下方信息
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(15),  // 计时显示区域
            Constraint::Min(0),      // 下方信息区域
        ])
        .split(area);

    // ========== 上部：大型计时显示 ==========
    let state_text = match app.pomodoro.state {
        crate::pomodoro::PomodoroState::Idle => "⏸️  空闲",
        crate::pomodoro::PomodoroState::Working => "🔥 工作中",
        crate::pomodoro::PomodoroState::Break => "☕ 休息中",
        crate::pomodoro::PomodoroState::Paused => "⏸️  已暂停",
    };

    let state_color = match app.pomodoro.state {
        crate::pomodoro::PomodoroState::Working => Color::Red,
        crate::pomodoro::PomodoroState::Break => Color::Green,
        _ => Color::Gray,
    };

    let time_remaining = app.pomodoro.format_remaining();
    let progress = app.pomodoro.progress();
    let progress_bar = "█".repeat((progress / 2.0) as usize); // 每 2% 一个块

    let mut timer_display = vec![
        Line::from(""),
        Line::from(Span::styled(
            state_text,
            Style::default().fg(state_color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        // 大型时间显示
        Line::from(Span::styled(
            &time_remaining,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        // 进度条
        Line::from(Span::styled(
            format!("[{}{}] {:.0}%",
                progress_bar,
                " ".repeat(50 - progress_bar.len()),
                progress
            ),
            if progress < 30.0 {
                Style::default().fg(Color::Green)
            } else if progress < 70.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            },
        )),
    ];

    let timer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            " ⏱️ 计时器 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let timer_para = Paragraph::new(timer_display)
        .block(timer_block)
        .alignment(Alignment::Center);

    f.render_widget(timer_para, chunks[0]);

    // ========== 下部：状态、统计、配置、快捷键 ==========
    let mut info_content = vec![
        Line::from(""),
        Line::from(Span::styled(
            "📊 统计",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "  今日完成: {} 个番茄钟 | 专注时长: {} 分钟",
            app.pomodoro_completed_today,
            app.pomodoro_total_minutes
        )),
        Line::from(""),
        Line::from(Span::styled(
            "⚙️ 配置",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "  工作: {} 分钟 | 休息: {} 分钟",
            app.pomodoro.work_duration,
            app.pomodoro.break_duration
        )),
        Line::from(""),
        Line::from("快捷键:  s 开始/暂停  |  S 停止"),
    ];

    if app.pomodoro.state == crate::pomodoro::PomodoroState::Idle {
        info_content.push(Line::from("             +/- 调整工作时长  |  [/] 调整休息时长"));
    }

    let info_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Span::styled(
            " 🍅 番茄钟 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let info_para = Paragraph::new(info_content)
        .block(info_block)
        .scroll((app.pomodoro_scroll_offset as u16, 0));

    f.render_widget(info_para, chunks[1]);
}

/// 渲染状态栏
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let (mode_indicator, status_text, bar_style) = match app.input_mode {
        InputMode::Command => {
            // Command模式：显示正在输入的命令
            ("COMMAND", format!(":{}", app.input_buffer), Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        }
        InputMode::Insert => {
            // Insert模式：显示模式名称
            ("INSERT", "正在编辑...".to_string(), Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD))
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
            let message = if let Some(ref msg) = app.status_message {
                msg.clone()
            } else if parts.is_empty() {
                "按 ? 显示帮助 | 按 : 进入命令模式".to_string()
            } else {
                parts.join(" ")
            };

            ("NORMAL", message, Style::default().bg(Color::DarkGray).fg(Color::White))
        }
    };

    let status_content = vec![
        Span::styled(format!(" {} ", mode_indicator), bar_style),
        Span::raw(" "),
        Span::raw(status_text),
    ];

    let status_bar = Paragraph::new(Line::from(status_content))
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
        DialogType::EditTask => {
            ("编辑任务", vec![
                Line::from(""),
                Line::from("修改任务标题:"),
                Line::from(""),
                Line::from(Span::styled(
                    &app.input_buffer,
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from("按 Enter 保存, Esc 取消"),
            ])
        }
        DialogType::EditNote => {
            let mode_hint = match app.input_mode {
                InputMode::Normal => "↑/↓/k/j:选择字段 | i:编辑 | Esc:取消",
                InputMode::Insert => "输入内容 | Enter:保存字段 | Esc:返回选择",
                _ => "",
            };

            // 显示标题和内容，根据当前模式选择显示哪个
            let title_display = if app.note_edit_field == 0 && app.input_mode == InputMode::Insert {
                &app.input_buffer  // 正在编辑标题时，显示buffer
            } else {
                &app.input_title   // 否则显示保存的标题
            };

            let content_display = if app.note_edit_field == 1 && app.input_mode == InputMode::Insert {
                &app.input_buffer  // 正在编辑内容时，显示buffer
            } else {
                &app.input_content // 否则显示保存的内容
            };

            ("编辑便签", vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("标题: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        if app.note_edit_field == 0 { "[选中]" } else { "" },
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    ),
                ]),
                Line::from(Span::styled(
                    title_display,
                    if app.note_edit_field == 0 && app.input_mode == InputMode::Insert {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else if app.note_edit_field == 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Gray)
                    }
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("内容: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        if app.note_edit_field == 1 { "[选中]" } else { "" },
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    ),
                ]),
                Line::from(Span::styled(
                    content_display,
                    if app.note_edit_field == 1 && app.input_mode == InputMode::Insert {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else if app.note_edit_field == 1 {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Gray)
                    }
                )),
                Line::from(""),
                Line::from(Span::styled(mode_hint, Style::default().fg(Color::Green))),
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
            // 根据当前标签页显示不同的帮助内容
            match app.current_tab {
                0 => {
                    // 任务界面帮助
                    ("任务管理 - 快捷键帮助", vec![
                        Line::from(""),
                        Line::from(Span::styled("━━━ 导航 ━━━", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                        Line::from("  j/k / ↓/↑     上下移动"),
                        Line::from("  h/l / Tab     切换标签"),
                        Line::from("  gg / G        首行/末行"),
                        Line::from("  5j / 10G      数字前缀跳转"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 任务操作 ━━━", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
                        Line::from("  n / a / o     新建任务"),
                        Line::from("  e             编辑任务"),
                        Line::from("  dd            删除任务(双击d)"),
                        Line::from("  Space / x     切换完成状态"),
                        Line::from("  p             切换优先级"),
                        Line::from("  t             设置DDL时间"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 命令模式 ━━━", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                        Line::from("  :new 标题     直接创建任务"),
                        Line::from("  :p [1/2/3]    设置优先级 (1=Low, 2=Med, 3=High)"),
                        Line::from("  :t / :ddl     设置DDL"),
                        Line::from("  :sort         排序任务"),
                        Line::from("  :q / :wq      退出"),
                        Line::from("  :5            跳转第5行"),
                        Line::from(""),
                        Line::from(Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray))),
                        Line::from(vec![
                            Span::styled("j/k ↓/↑", Style::default().fg(Color::Yellow)),
                            Span::styled(" 滚动 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
                            Span::styled(" 翻页 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("g/Home", Style::default().fg(Color::Yellow)),
                            Span::styled(" 顶部 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("Esc/q/?", Style::default().fg(Color::Yellow)),
                            Span::styled(" 关闭", Style::default().fg(Color::DarkGray)),
                        ]),
                    ])
                }
                1 => {
                    // 便签界面帮助
                    ("便签墙 - 快捷键帮助", vec![
                        Line::from(""),
                        Line::from(Span::styled("━━━ 导航 ━━━", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                        Line::from("  j/k / ↓/↑     上下移动"),
                        Line::from("  h/l / Tab     切换标签"),
                        Line::from("  gg / G        首行/末行"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 便签操作 ━━━", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))),
                        Line::from("  n / a / o     新建便签"),
                        Line::from("  e             编辑便签"),
                        Line::from("  dd            删除便签(双击d)"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 编辑便签 ━━━", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                        Line::from("  ↑/↓ 或 k/j    选择编辑字段(标题/内容)"),
                        Line::from("  i             进入编辑模式"),
                        Line::from("  Enter         保存当前字段"),
                        Line::from("  Esc           取消编辑"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 命令模式 ━━━", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                        Line::from("  :new 内容     直接创建便签"),
                        Line::from("  :q / :wq      退出"),
                        Line::from(""),
                        Line::from(Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray))),
                        Line::from(vec![
                            Span::styled("j/k ↓/↑", Style::default().fg(Color::Yellow)),
                            Span::styled(" 滚动 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
                            Span::styled(" 翻页 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("g/Home", Style::default().fg(Color::Yellow)),
                            Span::styled(" 顶部 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("Esc/q/?", Style::default().fg(Color::Yellow)),
                            Span::styled(" 关闭", Style::default().fg(Color::DarkGray)),
                        ]),
                    ])
                }
                2 => {
                    // 番茄钟界面帮助
                    ("番茄钟 - 快捷键帮助", vec![
                        Line::from(""),
                        Line::from(Span::styled("━━━ 导航 ━━━", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                        Line::from("  h/l / Tab     切换标签"),
                        Line::from("  1/2/3         快速跳转"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 番茄钟控制 ━━━", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
                        Line::from("  s             开始/暂停"),
                        Line::from("  S / c         停止/取消"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 时长调整（仅空闲时）━━━", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                        Line::from("  + / -         调整工作时长 (±5分钟)"),
                        Line::from("  [ / ]         调整休息时长 (±1分钟)"),
                        Line::from(""),
                        Line::from(Span::styled("━━━ 命令模式 ━━━", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                        Line::from("  :s / :start   开始/暂停"),
                        Line::from("  :c / :cancel  停止/取消"),
                        Line::from("  :pomo w=25 b=5 设置时长并保存"),
                        Line::from("  :q / :wq      退出"),
                        Line::from(""),
                        Line::from(Span::styled("提示: 工作25分钟 → 休息5分钟为标准番茄钟", Style::default().fg(Color::Gray))),
                        Line::from(""),
                        Line::from(Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray))),
                        Line::from(vec![
                            Span::styled("j/k ↓/↑", Style::default().fg(Color::Yellow)),
                            Span::styled(" 滚动 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
                            Span::styled(" 翻页 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("g/Home", Style::default().fg(Color::Yellow)),
                            Span::styled(" 顶部 | ", Style::default().fg(Color::DarkGray)),
                            Span::styled("Esc/q/?", Style::default().fg(Color::Yellow)),
                            Span::styled(" 关闭", Style::default().fg(Color::DarkGray)),
                        ]),
                    ])
                }
                _ => {
                    // 默认帮助（不应该到这里）
                    ("快捷键帮助", vec![
                        Line::from(""),
                        Line::from("按 ? 查看帮助"),
                        Line::from(""),
                        Line::from(Span::styled("按任意键关闭", Style::default().fg(Color::DarkGray))),
                    ])
                }
            }
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
                let display_value = if i == app.datetime_picker_field && !app.datetime_input_buffer.is_empty() {
                    // 如果当前字段有输入，显示输入缓冲区的内容
                    app.datetime_input_buffer.clone() + "_" // 添加下划线表示正在输入
                } else {
                    values[i].clone()
                };

                if i == app.datetime_picker_field {
                    // 当前选中的字段：高亮显示
                    datetime_spans.push(Span::styled(
                        display_value,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                } else {
                    datetime_spans.push(Span::raw(display_value));
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
                        Line::from("  0-9 直接输入数字  Backspace 删除"),
                        Line::from("  ↑/k 增加  ↓/j 减少"),
                        Line::from("  ←/h/→/l/Tab 切换字段"),
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
        DialogType::ViewNote => {
            if let Some(note) = app.selected_note() {
                let mut content = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        &note.title,
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                ];

                // 添加便签内容，支持长内容换行
                let note_lines: Vec<&str> = note.content.lines().collect();
                for line in note_lines {
                    content.push(Line::from(line));
                }

                content.extend(vec![
                    Line::from(""),
                    Line::from(Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                    Line::from(format!(
                        "创建: {}  更新: {}",
                        note.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
                        note.updated_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("e", Style::default().fg(Color::Green)),
                        Span::raw(" 编辑  "),
                        Span::styled("Esc/q", Style::default().fg(Color::Yellow)),
                        Span::raw(" 关闭"),
                    ]),
                    Line::from(vec![
                        Span::styled("j/k ↓/↑", Style::default().fg(Color::Yellow)),
                        Span::styled(" 滚动 | ", Style::default().fg(Color::DarkGray)),
                        Span::styled("g", Style::default().fg(Color::Yellow)),
                        Span::styled(" 顶部 | ", Style::default().fg(Color::DarkGray)),
                        Span::styled("G", Style::default().fg(Color::Yellow)),
                        Span::styled(" 底部", Style::default().fg(Color::DarkGray)),
                    ]),
                ]);

                ("查看便签", content)
            } else {
                ("查看便签", vec![Line::from("没有选中的便签")])
            }
        }
        _ => ("", vec![]),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));

    let mut paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });

    // 为Help对话框添加滚动支持
    if app.show_dialog == DialogType::Help {
        paragraph = paragraph.scroll((app.help_scroll_offset as u16, 0));
    }

    // 为ViewNote对话框添加滚动支持
    if app.show_dialog == DialogType::ViewNote {
        paragraph = paragraph.scroll((app.view_note_scroll_offset as u16, 0));
    }

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
