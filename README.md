# Terminator Task Manager

一个集成在终端中的现代化任务管理器，支持日程管理、番茄钟计时和便签功能。使用 Rust 和 ratatui 构建，提供高效的 TUI 交互界面。

## ✨ 特性

### 核心功能

- **📝 任务管理**
  - 创建、编辑、删除任务
  - 按优先级（高/中/低）、时间、完成状态多维度排序
  - 支持任务截止日期和提醒时间
  - 任务状态跟踪（待办/进行中/已完成）

- **🍅 番茄钟**
  - 可自定义工作和休息时长（默认25分钟工作，5分钟休息）
  - 倒计时显示和进度条
  - 与任务关联，自动记录工作时长
  - 完成后系统通知

- **📓 便签功能**
  - 快速笔记
  - 支持 Markdown 格式
  - 可关联到具体任务
  - 全文搜索

- **⏰ 智能提醒**
  - 系统桌面通知（libnotify）
  - 终端内闪烁提示
  - 后台守护进程持续监控

### Shell 集成

- **终端状态栏显示**：在提示符中显示任务统计
- **快捷键支持**：Ctrl+T 快速打开管理器
- **Starship 集成**：美化的提示符集成
- **点击展开**：支持 OSC 8 的终端可点击打开（实验性）

## 📦 安装

### 前置要求

- Rust 1.70+ （安装：`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`）
- Linux 系统（其他系统未测试）
- libnotify（用于系统通知）：`sudo apt install libnotify-bin` (Ubuntu/Debian)

### 编译安装

```bash
# 克隆仓库
git clone https://github.com/yourusername/terminator-task.git
cd terminator-task

# 编译
cargo build --release

# 安装到系统
sudo cp target/release/tasks /usr/local/bin/
sudo cp target/release/taskd /usr/local/bin/

# 验证安装
tasks --help
```

### 可选：Shell 集成

查看 [shell-integration/README.md](shell-integration/README.md) 了解如何集成到您的 shell。

## 🚀 使用方法

### 基础命令

```bash
# 启动 TUI 界面
tasks
# 或
tasks show

# 快速添加任务
tasks add "完成项目文档"

# 列出所有任务
tasks list

# 标记任务完成
tasks complete 1
```

### TUI 界面操作

启动 TUI 后的快捷键：

#### 通用操作
- `Tab` / `Shift+Tab`：切换标签页（任务/便签/番茄钟）
- `q`：退出程序
- `j` / `↓`：向下移动
- `k` / `↑`：向上移动

#### 任务页面
- `n`：创建新任务
- `Space`：切换任务完成状态
- `d`：删除选中任务
- `e`：编辑任务

#### 便签页面
- `n`：创建新便签
- `d`：删除选中便签
- `e`：编辑便签

#### 番茄钟页面
- `p`：开始/暂停番茄钟
- `s`：停止番茄钟
- `r`：重置

### 后台守护进程

启动守护进程以持续监控任务提醒：

```bash
# 前台运行（调试）
taskd

# 后台运行
nohup taskd &

# 使用 systemd（推荐）
# 创建 ~/.config/systemd/user/taskd.service
sudo systemctl --user enable taskd
sudo systemctl --user start taskd
```

## 📂 项目结构

```
terminator-task/
├── Cargo.toml              # Rust 项目配置
├── src/
│   ├── main.rs            # TUI 主程序入口
│   ├── daemon/            # 守护进程
│   │   └── main.rs
│   ├── models/            # 数据模型
│   ├── db/                # SQLite 数据库层
│   ├── ui/                # TUI 界面
│   ├── pomodoro/          # 番茄钟模块
│   ├── notes/             # 便签模块
│   └── notify/            # 通知系统
├── shell-integration/     # Shell 集成脚本
└── README.md
```

## 🛠️ 技术栈

- **语言**：Rust (Edition 2021)
- **TUI 框架**：[ratatui](https://github.com/ratatui/ratatui) + crossterm
- **数据库**：SQLite (rusqlite)
- **异步运行时**：Tokio
- **通知**：notify-rust
- **时间处理**：chrono

## 📝 配置

配置文件位置：`~/.config/terminator-task/config.toml`

示例配置：

```toml
[pomodoro]
work_duration = 25      # 工作时长（分钟）
break_duration = 5      # 休息时长（分钟）

[notifications]
enabled = true
sound = true

[database]
# 默认使用 ~/.local/share/terminator-task/tasks.db
# path = "/custom/path/tasks.db"
```

## 🔧 开发

### 运行测试

```bash
cargo test
```

### 开发模式运行

```bash
# TUI 界面
cargo run --bin tasks

# 守护进程
cargo run --bin taskd -- --debug
```

### 代码格式化

```bash
cargo fmt
cargo clippy
```

## 🤝 贡献

欢迎贡献！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支：`git checkout -b feature/amazing-feature`
3. 提交更改：`git commit -m 'Add amazing feature'`
4. 推送分支：`git push origin feature/amazing-feature`
5. 提交 Pull Request

## 📜 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件

## 🙏 致谢

- [ratatui](https://github.com/ratatui/ratatui) - 优秀的 TUI 框架
- [Starship](https://starship.rs/) - Shell 提示符集成灵感
- 所有贡献者

## 📮 联系方式

- Issue Tracker：[GitHub Issues](https://github.com/yourusername/terminator-task/issues)
- Email：your.email@example.com

## 🗺️ Roadmap

- [ ] 任务标签和分类
- [ ] 任务统计和报表
- [ ] 云同步支持
- [ ] 移动端查看
- [ ] AI 智能提醒
- [ ] 协作功能
- [ ] 导入/导出（JSON、CSV）
- [ ] 主题自定义
- [ ] 键盘快捷键自定义

---

⭐ 如果这个项目对您有帮助，请给个 Star！
