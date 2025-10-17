use anyhow::Result;
use chrono::Utc;
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
use crate::models::{Note, Priority, Task, TaskStatus};
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
}

impl Default for App {
    fn default() -> Self {
        let mut task_list_state = ListState::default();
        task_list_state.select(Some(0));

        let mut note_list_state = ListState::default();
        note_list_state.select(Some(0));

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

        // 更新选择状态
        if !self.tasks.is_empty() && self.task_list_state.selected().is_none() {
            self.task_list_state.select(Some(0));
        }
        if !self.notes.is_empty() && self.note_list_state.selected().is_none() {
            self.note_list_state.select(Some(0));
        }

        Ok(())
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

        if event::poll(std::time::Duration::from_millis(100))? {
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
                        app.pomodoro.start_break();
                        app.status_message = Some("休息时间！".to_string());
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

/// 处理键盘事件
fn handle_key_event(app: &mut App, key: KeyCode) -> Result<()> {
    // 对话框模式
    if app.show_dialog != DialogType::None {
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
                            DialogType::CreateNote => app.create_note()?,
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

    // 正常模式快捷键
    match app.input_mode {
        InputMode::Normal => {
            match key {
                // 退出: q, Ctrl+C
                KeyCode::Char('q') | KeyCode::Char('Q') => app.should_quit = true,

                // 标签页切换: Tab, Shift+Tab, 1-3
                KeyCode::Tab => app.next_tab(),
                KeyCode::BackTab => app.previous_tab(),
                KeyCode::Char('1') => app.goto_tab(0),
                KeyCode::Char('2') => app.goto_tab(1),
                KeyCode::Char('3') => app.goto_tab(2),

                // vim导航: j/k, h/l, gg/G
                KeyCode::Down | KeyCode::Char('j') => match app.current_tab {
                    0 => app.next_task(),
                    1 => app.next_note(),
                    _ => {}
                },
                KeyCode::Up | KeyCode::Char('k') => match app.current_tab {
                    0 => app.previous_task(),
                    1 => app.previous_note(),
                    _ => {}
                },
                KeyCode::Left | KeyCode::Char('h') => app.previous_tab(),
                KeyCode::Right | KeyCode::Char('l') => app.next_tab(),
                KeyCode::Char('g') => {
                    // 等待下一个g (gg跳到顶部)
                    match app.current_tab {
                        0 => app.goto_first_task(),
                        1 => app.goto_first_note(),
                        _ => {}
                    }
                }
                KeyCode::Char('G') => {
                    match app.current_tab {
                        0 => app.goto_last_task(),
                        1 => app.goto_last_note(),
                        _ => {}
                    }
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
                }
                KeyCode::Char(' ') | KeyCode::Char('x') => {
                    // 切换完成状态
                    if app.current_tab == 0 {
                        app.toggle_task_status()?;
                    }
                }
                KeyCode::Char('d') => {
                    // 删除 (需要确认)
                    app.show_dialog = DialogType::DeleteConfirm;
                }
                KeyCode::Char('p') => {
                    // 切换优先级
                    if app.current_tab == 0 {
                        app.cycle_priority()?;
                    }
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
                }
                KeyCode::Char('S') => {
                    // 停止番茄钟
                    app.pomodoro.stop();
                    app.status_message = Some("番茄钟已停止".to_string());
                }

                // 帮助
                KeyCode::Char('?') => {
                    app.show_dialog = DialogType::Help;
                }

                _ => {}
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
            // 可以根据鼠标位置实现点击选择等功能
            // 这里简单实现：点击切换标签页
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
            let content = format!("{} {} {}", status_icon, priority_icon, task.title);
            ListItem::new(content)
        })
        .collect();

    let help_text = if app.tasks.is_empty() {
        "按 'n' 创建新任务 | '?' 显示帮助"
    } else {
        "j/k:导航 | Space:切换状态 | n:新建 | d:删除 | p:优先级 | ?:帮助"
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

/// 渲染便签列表
fn render_notes(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .notes
        .iter()
        .map(|note| ListItem::new(format!("📝 {}", note.title)))
        .collect();

    let help_text = if app.notes.is_empty() {
        "按 'n' 创建新便签 | '?' 显示帮助"
    } else {
        "j/k:导航 | n:新建 | d:删除 | ?:帮助"
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("便签列表 ({} 个)", app.notes.len()))
                .title_bottom(help_text),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.note_list_state);
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

    let content = vec![
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
        Line::from(""),
        Line::from("快捷键:"),
        Line::from("  s - 开始/暂停"),
        Line::from("  S - 停止"),
    ];

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// 渲染状态栏
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let mode_text = match app.input_mode {
        InputMode::Normal => "NORMAL",
        InputMode::Insert => "INSERT",
        InputMode::Command => "COMMAND",
    };

    let status = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        format!("模式: {} | Tab/h/l:切换标签 | q:退出 | ?:帮助", mode_text)
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
            ("创建新便签", vec![
                Line::from(""),
                Line::from("便签标题:"),
                Line::from(Span::styled(
                    if app.input_title.is_empty() { "(输入标题...)" } else { &app.input_title },
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from("便签内容:"),
                Line::from(Span::styled(
                    &app.input_buffer,
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from("Enter:确认 | Esc:取消"),
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
