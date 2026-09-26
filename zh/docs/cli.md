# CLI 参考

kaku 命令能做的事，从 AI 设置到诊断和更新。

直接运行 `kaku`，可以从几个常用命令里挑一个。

## kaku ai

打开 AI 设置面板，配置 Kaku Assistant 和 Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code 这些外部编码工具。

```bash
kaku ai
```

## kaku chat

在任意 shell 里启动 Kaku 的 AI 对话，它就是内置 `k` 命令的别名，`k` 不在 PATH 上时也能用。

```bash
kaku chat                 # open interactive chat
kaku chat "explain this"  # one-shot prompt
```

对话读取 `~/.config/kaku/assistant.toml`，和 `Cmd + L` 聊天面板共用同一份对话和记忆，交互模式下支持 `/new`、`/resume`、`/clear`、`/status`、`/memory` 和 `/exit`。

## kaku config

打开配置 TUI，调整常用设置和 Lua 覆盖，`~/.config/kaku/kaku.lua` 不存在时会先创建，在 Kaku 里按 `Cmd + ,` 打开的也是这个界面。

```bash
kaku config
```

## kaku doctor

检查 app bundle、PATH 和 shell 集成，刚装完或者感觉哪里不对时先跑一遍。

```bash
kaku doctor
```

## kaku update

下载并安装最新的 Kaku 版本。

```bash
kaku update
```

运行 `kaku --version` 可以查看已安装的版本，想查图形程序的版本又不想开窗口，可以运行 `/Applications/Kaku.app/Contents/MacOS/kaku-gui --version`。

## kaku reset

移除 Kaku 管理的 shell 和 tmux 集成、git delta 默认配置和部分状态，以及 `~/.config/kaku/kaku.lua` 里托管的主题块，托管块之外你自己写的 Lua 会保留。这个命令要谨慎用，之后想恢复 shell 集成再跑一次 `kaku init`。

```bash
kaku reset
```

## kaku init

为 zsh、fish 或两者配置或刷新 shell 集成，会生成 `~/.config/kaku/zsh/kaku.zsh`，按需生成 `~/.config/kaku/fish/kaku.fish`，在交互式 shell 里还会问你要不要用 Homebrew 装上缺失的 Starship、Delta、Lazygit、Yazi 这些可选工具。

```bash
kaku init
```

shell 里突然找不到 `kaku` 命令时，用下面的命令刷新集成，不会弹出可选工具的安装提示：

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

在脚本和外部工具里操作 Kaku 的多路复用器，比如让 AI 工具或 shell 脚本自动开分屏和标签。

```bash
kaku cli split-pane                          # split current pane
kaku cli split-pane -- bash -c "echo hello"  # split and run a command
kaku cli --help                              # list all subcommands
kaku cli split-pane --help                   # help for a specific subcommand
```

---

Source: https://kaku.fun/zh/docs/cli
Site index for LLMs: https://kaku.fun/llms.txt
