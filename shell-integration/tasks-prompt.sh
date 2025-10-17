#!/bin/bash
# 任务管理器 Shell 集成脚本
# 用法: source ~/.config/tasks-prompt.sh

# 获取任务统计
get_task_stats() {
    if ! command -v tasks &> /dev/null; then
        return
    fi

    local task_count=$(tasks list 2>/dev/null | wc -l)
    local urgent_count=$(tasks list 2>/dev/null | grep -c '🔴' || echo 0)

    if [ "$task_count" -gt 0 ]; then
        echo -n "📋 $task_count"
        if [ "$urgent_count" -gt 0 ]; then
            echo -n " 🔴 $urgent_count"
        fi
        echo -n " "
    fi
}

# Bash prompt集成
if [ -n "$BASH_VERSION" ]; then
    # 添加任务状态到PS1
    # export PS1="$(get_task_stats)$PS1"

    # 或者使用PROMPT_COMMAND动态更新
    if [[ ! "$PROMPT_COMMAND" =~ "get_task_stats" ]]; then
        export PROMPT_COMMAND="echo -n \"\$(get_task_stats)\"; $PROMPT_COMMAND"
    fi
fi

# Zsh prompt集成
if [ -n "$ZSH_VERSION" ]; then
    # Zsh使用precmd
    precmd_tasks() {
        echo -n "$(get_task_stats)"
    }

    # 添加到precmd_functions数组
    if [[ ! " ${precmd_functions[@]} " =~ " precmd_tasks " ]]; then
        precmd_functions+=(precmd_tasks)
    fi
fi

# 便捷别名
alias t='tasks'
alias tl='tasks list'
alias ta='tasks add'
alias ts='tasks show'

# 快捷键绑定（Ctrl+T 打开任务管理器）
if [ -n "$BASH_VERSION" ]; then
    bind -x '"\C-t":"tasks show"'
elif [ -n "$ZSH_VERSION" ]; then
    bindkey -s '^t' 'tasks show\n'
fi

echo "✅ 任务管理器Shell集成已加载"
echo "   使用快捷键 Ctrl+T 快速打开任务管理器"
echo "   别名: t, tl, ta, ts"
