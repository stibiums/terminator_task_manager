use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs},
    Frame, Terminal,
};
use std::io;

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
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    pub pomodoro: PomodoroTimer,
    pub current_tab: usize,
    pub task_list_state: ListState,
    pub note_list_state: ListState,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub input_buffer: String,
}

/// 输入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

impl Default for App {
    fn default() -> Self {
        let mut task_list_state = ListState::default();
        task_list_state.select(Some(0));

        let mut note_list_state = ListState::default();
        note_list_state.select(Some(0));

        Self {
            tasks: Vec::new(),
            notes: Vec::new(),
            pomodoro: PomodoroTimer::default(),
            current_tab: 0,
            task_list_state,
            note_list_state,
            should_quit: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
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

    /// 获取当前选中的任务
    pub fn selected_task(&self) -> Option<&Task> {
        self.task_list_state
            .selected()
            .and_then(|i| self.tasks.get(i))
    }

    /// 获取当前选中的便签
    pub fn selected_note(&self) -> Option<&Note> {
        self.note_list_state
            .selected()
            .and_then(|i| self.notes.get(i))
    }
}

/// 运行TUI应用
pub fn run_app() -> Result<()> {
    // 设置终端
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 创建应用状态
    let mut app = App::new();

    // 主循环
    let res = run_ui_loop(&mut terminal, &mut app);

    // 恢复终端
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => app.should_quit = true,
                            KeyCode::Tab => app.next_tab(),
                            KeyCode::BackTab => app.previous_tab(),
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
                            KeyCode::Char('n') => {
                                // TODO: 新建任务/便签
                            }
                            KeyCode::Char(' ') => {
                                // TODO: 切换任务完成状态
                            }
                            KeyCode::Char('p') => {
                                // 番茄钟操作
                                if app.current_tab == 2 {
                                    match app.pomodoro.state {
                                        crate::pomodoro::PomodoroState::Idle => {
                                            app.pomodoro.start_work(None);
                                        }
                                        crate::pomodoro::PomodoroState::Working
                                        | crate::pomodoro::PomodoroState::Break => {
                                            app.pomodoro.pause();
                                        }
                                        crate::pomodoro::PomodoroState::Paused => {
                                            app.pomodoro.resume();
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('s') => {
                                // 停止番茄钟
                                app.pomodoro.stop();
                            }
                            _ => {}
                        },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Char(c) => {
                                app.input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input_buffer.pop();
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.input_buffer.clear();
                            }
                            _ => {}
                        },
                    }
                }
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
                    }
                    crate::pomodoro::PomodoroState::Break => {
                        app.pomodoro.stop();
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

/// 渲染UI
fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    // 标签页
    let titles = vec!["Tasks", "Notes", "Pomodoro"];
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("📋 Task Manager"))
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

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("📝 Tasks (j/k: navigate, space: toggle, n: new, q: quit)"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut app.task_list_state);
}

/// 渲染便签列表
fn render_notes(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .notes
        .iter()
        .map(|note| ListItem::new(format!("📝 {}", note.title)))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("📓 Notes (j/k: navigate, n: new, q: quit)"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut app.note_list_state);
}

/// 渲染番茄钟
fn render_pomodoro(f: &mut Frame, app: &mut App, area: Rect) {
    let state_text = match app.pomodoro.state {
        crate::pomodoro::PomodoroState::Idle => "空闲",
        crate::pomodoro::PomodoroState::Working => "工作中",
        crate::pomodoro::PomodoroState::Break => "休息中",
        crate::pomodoro::PomodoroState::Paused => "已暂停",
    };

    let content = format!(
        "🍅 番茄钟\n\n状态: {}\n剩余时间: {}\n进度: {:.1}%\n\n按 'p' 开始/暂停, 按 's' 停止",
        state_text,
        app.pomodoro.format_remaining(),
        app.pomodoro.progress()
    );

    let paragraph = Paragraph::new(content).block(Block::default().borders(Borders::ALL));

    f.render_widget(paragraph, area);
}
