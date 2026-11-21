# Claude Code hooks 实用指南

> hook 写在哪、哪些事件真正有用，以及三个能直接用的例子，编辑后格式化、拦下危险命令、做完时发通知。

发布于 2026-10-03 · Tw93 · https://kaku.fun/zh/blog/claude-code-hooks

Claude Code 的 hook 是一段 shell 命令，会话里某个事件触发时它会跑，配置写在 settings JSON 的 `hooks` 对象里。真正用得上的是 PreToolUse，它可以在工具调用之前拦住或放行，PostToolUse 在工具成功之后跑，Notification 在 Claude Code 发出通知时跑，Stop 在 Claude 回答结束时跑。退出码、matcher 和 JSON 字段都以 [hooks 指南](https://code.claude.com/docs/en/hooks-guide) 和 [hooks 参考](https://code.claude.com/docs/en/hooks) 为准，2026 年 10 月核对过。

## hook 放在哪

一个 hook 要写明事件、matcher 和 handler，`"type": "command"` 会跑一条 shell 命令，事件的 JSON 从 stdin 送进来。

| 文件 | 范围 | 共享吗 |
|---|---|---|
| `~/.claude/settings.json` | 这台机器上的每个项目 | 不共享 |
| `.claude/settings.json` | 这个项目 | 共享，提交它 |
| `.claude/settings.local.json` | 这个项目 | 不共享，Claude Code 往这个文件写入设置时会把它写进 `.gitignore` |

用户、项目和本地的 hook 会加在一起跑，托管设置里的 hook 也照样跑，只有 `disableAllHooks` 同样写在托管设置里才会停。只关这一次：

```bash
claude --settings '{"disableAllHooks": true}'
```

`/hooks` 会把 hook 列出来，只读，标成 user、project、local 或 plugin，保存了却找不到的话，去看末尾多出来的逗号，或者文件里的注释，settings JSON 这两样都不认。

## 真正用得上的事件

![先是 PreToolUse，然后是工具调用和 PostToolUse，Claude 回答完时是 Stop](https://kaku.fun/img/blog/claude-code-hooks-lifecycle.webp)

*一轮回答里每调用一次工具，PreToolUse、工具本身和 PostToolUse 就走一遍，Stop 只在回答结束时触发一次，Notification 则在 Claude Code 需要你的时候触发。*

hooks 参考里列的事件比这多，平时会碰到的就是下面六个，按触发先后从 SessionStart 排到 Stop。

| 事件 | 什么时候触发 |
|---|---|
| `SessionStart` | 会话开始，或者恢复一个会话 |
| `UserPromptSubmit` | 提示词提交之后，Claude 处理之前 |
| `PreToolUse` | 工具调用之前，可以拦住这次调用 |
| `PostToolUse` | 工具调用成功之后 |
| `Notification` | Claude Code 发出一条通知 |
| `Stop` | Claude 这一轮回答结束 |

你要是中途把这一轮打断，`Stop` 不会出声，API 出错时走的是 `StopFailure`，不是 `Stop`。

`PreToolUse` 和 `PostToolUse` 按工具名匹配，`Edit|Write` 和 `Edit, Write` 是同一份区分大小写的列表。matcher 里一旦出现字母、数字、`_`、`-`、空格、逗号和 `|` 以外的字符，整段 matcher 就当成正则，不会自动加上 `^` 和 `$`，所以 `Edit.*` 也会匹配到 `NotebookEdit`。`Stop` 忽略 matcher，留空、写成 `"*"`，或者根本不写，每次都会触发。Edit 和 Write 传来绝对路径 `tool_input.file_path`，Bash 传来 `tool_input.command`。

## 改完文件再格式化

`PostToolUse` 要等工具调用成功才跑，文件这时候已经写好了，下面这个 Prettier hook 放在项目的 `.claude/settings.json` 里，把刚改的文件格式化：

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path' | xargs npx prettier --write"
          }
        ]
      }
    ]
  }
}
```

`jq` 负责读出路径，找不到命令时会报 `jq: command not found`，用 `brew install jq` 装上，退出码 0 的输出不会进对话。

用 `Bash` 改写文件不会触发 `Edit|Write`，想让 hook 不管文件是谁改的都能看到，指南里用的是 `FileChanged`。

## 命令跑之前拦住

`PreToolUse` 在参数已经齐了、工具还没跑、权限模式也还没检查时跑一次，退出码 2 拦住这次调用，Claude 把 stderr 读成原因，退出码 0 时原来的权限流程照旧走，并不等于批准这次调用。

存成 `.claude/hooks/no-rm.sh`，再 `chmod +x`，它会拦住以 `rm` 开头的 Bash 命令。

```bash
#!/bin/bash
input=$(cat)
command=$(jq -r '.tool_input.command' <<<"$input")

if [[ "$command" == rm* ]]; then
  echo "Blocked: rm commands are not allowed" >&2
  exit 2
fi

exit 0
```

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/no-rm.sh"
          }
        ]
      }
    ]
  }
}
```

`$CLAUDE_PROJECT_DIR` 是会话开始时的项目根目录，命令得是 `rm` 开头才会被拦住，所以 `npm test && rm -rf dist` 会漏过去。

要在 JSON 里给出决定，就以 0 退出，stdout 里只能有下面这一个对象。`permissionDecision` 得放在 `hookSpecificOutput` 里，放在最外层的话，Claude Code 会忽略它，也不报错。

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked by hook"
  }
}
```

`"deny"` 取消这次调用，`"allow"` 跳过询问，deny 和 ask 的规则仍然生效，托管设置里的拒绝名单也在内，`"ask"` 会把询问弹出来。JSON 里就算写着 `"allow"`，退出码 2 仍然拦住，`"deny"` 在 bypass permissions 模式下也照样拦住。匹配到的 hook 会并行跑，最严的结果胜出，从严到松是 deny、defer、ask、allow。

## Claude 做完时提醒

`Stop` 在每次回答结束时触发，不只是任务做完那一次，你中途打断就不出声，退出码 2 会让 Claude 继续，用来提醒的 hook 以 0 退出。

序列要写在 `terminalSequence` 里，因为 hook 没有 `/dev/tty`，由 Claude Code 替它写出去，tmux 和 GNU screen 里也一样，而且只在交互会话还留在屏幕上时才写。允许的值是 OSC 0、1、2、9、99、777，以及铃声。OSC 9 是 iTerm2、ConEmu、Windows Terminal 和 WezTerm 的通知，OSC 777 是 urxvt、Ghostty 和 Warp 的通知，别的都会被丢掉。

输入里的 `last_assistant_message` 是整段回复，没法直接当通知文字，所以脚本只发固定的一行，用 `printf` 拼出 OSC 777，再用 `jq -nc` 包成 JSON：

```bash
#!/bin/bash
seq=$(printf '\033]777;notify;%s;%s\007' "Claude Code" "Finished")
jq -nc --arg seq "$seq" '{terminalSequence: $seq}'
```

`Stop` 没有 matcher，注册脚本时不要加：

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/notify-stop.sh"
          }
        ]
      }
    ]
  }
}
```

`Notification` hook 也可以返回 `terminalSequence`，`preferredNotifChannel` 设成 `notifications_disabled` 也停不了它。输入里有 `message` 和 `notification_type`，`permission_prompt` 在大约六秒没有打字时触发，`idle_prompt` 在回答结束之后大约 60 秒、你还没打字时触发。

## 看 hook 到底跑了什么

`/hooks` 里看得到这条 hook，它却一次都不跑，多半是 matcher 没对上工具名，`Ctrl + O` 打开 transcript 之后，成功是空白，拦住了会把原因写出来。退出码不是 0 也不是 2 时，通常是不拦截的错误，动作照样继续，输出里带着 `Failed with non-blocking status code:` 和 stderr 的第一行。

```bash
echo '{"tool_name":"Bash","tool_input":{"command":"ls"}}' | ./.claude/hooks/no-rm.sh
echo $?
```

`claude --debug` 把日志写到 `~/.claude/debug/<session-id>.txt`，终端里什么都不打印。要自己定路径就用 `claude --debug-file /tmp/claude.log`，会话开到一半想记日志，用 `/debug`。`Hook output does not start with {` 表示 stdout 不是单独一个 JSON 对象，常常是 shell profile 先打了一行，后面的 JSON 就不会生效。`Hook JSON output had unrecognized keys` 表示有字段放错了层级。

## hook 用的是你的权限

`"type": "command"` 的 hook 以你的用户身份跑，账号能读、能改、能删的文件它都能动，要让它在每次工具调用前都跑，先把脚本从头读一遍，要展开的变量都加上引号。

交互会话会先等你确认信任这个工作区，然后才跑任何设置文件里的 hook，`~/.claude/settings.json` 里的也在内。`claude -p` 跳过这个对话框，把当前目录当成已信任，提交在仓库里的 `.claude/settings.json` hook 就会直接跑。仓库要不是你写的，先把 `.claude/` 读一遍，或者带上 `--settings '{"disableAllHooks": true}'`。

## hook 和 Kaku

Kaku 不会往你的 agent 里加 hook，也不会加指令，`kaku ai` 只有你在那里改某个工具的设置时才会动它，你自己写的字段会留着。

Claude Code 自带的提醒是另一路开关，Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用任何设置，更早的版本要在 `~/.claude/settings.json` 里设上 `"preferredNotifChannel": "ghostty"`，见[Claude Code 通知](/zh/blog/claude-code-notifications)。

Stop 脚本和这条通道是分开的，它让 Claude Code 写出 OSC 777，也就是允许的那几种里 Ghostty 用的通知。Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上时，Claude 做完的时候 Kaku 本来就会提醒，再挂这个 Stop hook 会多出一条通知，所以它适合更早的版本或者别的终端。OSC 9 也在允许的那几种里，Codex 设了 `notification_method = "osc9"` 时，Kaku 会把它显示成工具自己写的那条消息。

要在会话之外的脚本里接住 Claude 做完这件事，不用 hook 也行，Kaku 0.29.0 及以上的 `kaku cli events` 读的是 Claude Code 2.1.295 及以上上报的状态，哪个窗格状态一变就打印一行：

```bash
kaku cli events --for done,blocked --json
```

每行是一个 JSON 对象，带 `pane_id`、`title`、`from` 和 `to`，加 `--once` 碰到第一条匹配就退出，只盯一个窗格就用 `kaku cli wait --pane-id <id>`。

## 常见问题

### 退出码 0 算不算放行？

不算，`PreToolUse` 返回 0 时原来的权限检查还在，要跳过询问，得在 `hookSpecificOutput` 里写上 `"permissionDecision": "allow"`，再以 0 退出，拒绝规则仍然生效。

### 格式化为什么漏了文件？

`Edit|Write` 只在 Edit 或 Write 工具之后跑，用 `Bash` 改写文件不会触发它，文件已经写完，`PostToolUse` 也撤不回这一次写入。

### 为什么 Stop hook 之后 Claude 还在继续？

退出码 2，或者 `{"decision": "block", "reason": "..."}`，都是让 Claude 继续，连续拦住八次，中间没有工具调用，Claude Code 会结束这一轮。只想通知的时候以 0 退出，真要拦住的话，脚本先看 `stop_hook_active`，已经是 `true` 就以 0 退出，免得一直拦下去。

### 我正在用的这个窗口能看到通知吗？

Kaku 在后台时才显示，你正看着的窗格不会弹，`terminalSequence` 只有交互会话还留在屏幕上时才会写出去。

### 这些 hook 是 Kaku 装的吗？

不是，这些 hook 得你自己写进 Claude Code 的设置，Kaku 不会自己加 hook 或指令，`kaku ai` 会留着你写过的字段。
