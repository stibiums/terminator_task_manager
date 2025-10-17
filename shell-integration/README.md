# Shell 集成指南

本目录包含了将任务管理器集成到终端Shell的配置文件和脚本。

## 集成方式

### 1. Starship Prompt 集成（推荐）

[Starship](https://starship.rs/) 是一个快速、可定制的shell提示符。

**安装步骤：**

1. 安装 Starship（如果还没有安装）：
   ```bash
   curl -sS https://starship.rs/install.sh | sh
   ```

2. 将 `starship.toml` 中的配置添加到您的 Starship 配置文件：
   ```bash
   cat starship.toml >> ~/.config/starship.toml
   ```

3. 重启终端或执行：
   ```bash
   source ~/.bashrc  # 或 ~/.zshrc
   ```

**效果：**
- 在提示符中显示待办任务数量 📋 3
- 显示高优先级任务数量 🔴 1

### 2. Bash/Zsh Prompt 直接集成

如果您不使用 Starship，可以直接将脚本集成到 shell 配置中。

**安装步骤：**

1. 复制脚本到配置目录：
   ```bash
   cp tasks-prompt.sh ~/.config/
   ```

2. 在 `~/.bashrc` 或 `~/.zshrc` 中添加：
   ```bash
   source ~/.config/tasks-prompt.sh
   ```

3. 重新加载配置：
   ```bash
   source ~/.bashrc  # 或 ~/.zshrc
   ```

**功能：**
- ✅ 显示任务统计在提示符中
- ✅ 快捷键 `Ctrl+T` 快速打开任务管理器
- ✅ 便捷别名：`t`, `tl`, `ta`, `ts`

### 3. Terminator 终端配置

对于 Terminator 终端模拟器，您可以：

1. **快捷键配置**：
   打开 Terminator → Preferences → Keybindings
   添加自定义快捷键运行 `tasks show`

2. **右键菜单**：
   编辑 `~/.config/terminator/config`，添加自定义命令：
   ```ini
   [keybindings]
     # 添加快捷键
     # Ctrl+Alt+T 打开任务管理器
   ```

### 4. 点击展开功能（实验性）

某些现代终端（如 Kitty、WezTerm）支持 OSC 8 超链接，可以实现点击展开效果。

**示例配置**（需要终端支持）：
```bash
# 在 prompt 中添加可点击链接
echo -e "\e]8;;command://tasks show\e\\\📋 Tasks\e]8;;\e\\"
```

## 使用说明

安装后，您的终端提示符会显示任务统计信息，例如：

```
📋 5 🔴 2 user@host:~/project$
```

这表示：
- 📋 5：共有 5 个待办任务
- 🔴 2：其中 2 个是高优先级

### 快捷键

- `Ctrl+T`：快速打开任务管理器 TUI
- `t`：`tasks` 的别名
- `tl`：`tasks list` 的别名
- `ta <title>`：快速添加任务
- `ts`：打开 TUI 界面

## 自定义

您可以根据需要修改 `tasks-prompt.sh` 中的 `get_task_stats()` 函数来自定义显示内容。

例如，只显示今天的任务：
```bash
get_task_stats() {
    local today_tasks=$(tasks list 2>/dev/null | grep $(date +%Y-%m-%d) | wc -l)
    [ "$today_tasks" -gt 0 ] && echo -n "📅 $today_tasks "
}
```

## 故障排除

如果集成不工作，请检查：

1. ✅ `tasks` 命令是否在 PATH 中：
   ```bash
   command -v tasks
   ```

2. ✅ Shell 配置文件是否正确加载：
   ```bash
   echo $SHELL
   ```

3. ✅ 权限是否正确：
   ```bash
   chmod +x ~/.config/tasks-prompt.sh
   ```
