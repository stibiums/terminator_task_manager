# 快速入门指南

## 5分钟上手 Terminator Task Manager

### 1. 安装

```bash
# 编译项目
cargo build --release

# 安装到系统（可选）
sudo cp target/release/tasks /usr/local/bin/
sudo cp target/release/taskd /usr/local/bin/
```

### 2. 基础使用

#### 添加你的第一个任务
```bash
tasks add "学习 Rust"
tasks add "完成项目文档"
tasks add "代码审查"
```

#### 查看所有任务
```bash
tasks list
```

输出示例：
```
[3] ⭕ 🟡 代码审查
[2] ⭕ 🟡 完成项目文档
[1] ⭕ 🟡 学习 Rust
```

图标说明：
- ⭕ = 待办
- 🔄 = 进行中
- ✅ = 已完成
- 🟢 = 低优先级
- 🟡 = 中优先级
- 🔴 = 高优先级

#### 完成一个任务
```bash
tasks complete 1
```

### 3. 使用 TUI 界面

启动交互式界面：
```bash
tasks
# 或
tasks show
```

#### TUI 快捷键速查

**导航**
- `Tab` / `Shift+Tab`：切换标签页
- `j` 或 `↓`：向下移动
- `k` 或 `↑`：向上移动
- `q`：退出

**任务操作**
- `n`：创建新任务
- `Space`：切换完成状态
- `d`：删除任务
- `e`：编辑任务

**番茄钟**（切换到番茄钟标签）
- `p`：开始/暂停
- `s`：停止
- `r`：重置

### 4. 后台守护进程（可选）

启动守护进程以获得任务提醒：

```bash
# 前台运行（测试）
taskd

# 后台运行
nohup taskd > /tmp/taskd.log 2>&1 &

# 或使用 screen/tmux
screen -dmS taskd taskd
```

### 5. Shell 集成（可选）

在 shell 提示符中显示任务统计：

```bash
# 复制集成脚本
cp shell-integration/tasks-prompt.sh ~/.config/

# 添加到 .bashrc 或 .zshrc
echo "source ~/.config/tasks-prompt.sh" >> ~/.bashrc

# 重新加载配置
source ~/.bashrc
```

现在你的提示符会显示：
```
📋 3 🔴 1 user@host:~/project$
```

并且可以使用 `Ctrl+T` 快速打开任务管理器！

### 6. 数据位置

所有数据存储在：
```
~/.local/share/terminator-task/tasks.db
```

可以使用任何 SQLite 工具查看或备份。

### 7. 常用命令速查表

| 命令 | 说明 |
|------|------|
| `tasks add "<title>"` | 添加新任务 |
| `tasks list` | 列出所有任务 |
| `tasks show` | 打开 TUI 界面 |
| `tasks complete <id>` | 标记任务完成 |
| `taskd` | 启动守护进程 |

### 8. 下一步

- 阅读完整 [README.md](README.md) 了解所有功能
- 查看 [shell-integration/README.md](shell-integration/README.md) 深入定制 shell 集成
- 探索 TUI 界面的便签和番茄钟功能

---

🎉 开始高效管理你的任务吧！
