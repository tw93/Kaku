# CLI 参考

在 shell 里打开 AI 设置、聊天、配置、诊断、更新和多路复用命令。

在终端里运行 `kaku` 查看所有可用命令。

## kaku ai

在 Kaku 里打开 AI 设置面板。配置外部编码工具（Claude Code、Codex、Gemini CLI 等）和 Kaku Assistant。

```bash
kaku ai
```

## kaku chat

从任意 shell 启动 Kaku 的独立 AI 对话。它是内置 `k` 助手的一个好记别名， 所以即使 `k` 不在 PATH 上也能用。

```bash
kaku chat                 # open interactive chat
kaku chat "explain this"  # one-shot prompt
```

对话使用 `~/.config/kaku/assistant.toml`，和 `Cmd + L` overlay 共享同一份对话与 记忆文件，在交互模式下支持 `/new`、`/resume`、`/clear`、 `/status`、`/memory` 和 `/exit`。

## kaku config

打开 Kaku 配置 TUI，用来调整常用设置和 Lua 覆盖。它会确保 `~/.config/kaku/kaku.lua` 存在，也可以在设置面板里用 `Cmd + ,` 打开。

```bash
kaku config
```

## kaku doctor

跑一遍诊断，确认 Kaku 的 app bundle、shell 集成、PATH 条目和可选工具都正常。安装后或感觉哪里坏了，先跑它。

```bash
kaku doctor
```

## kaku update

检查并安装最新的 Kaku 版本。

```bash
kaku update
```

运行 `kaku --version`查看已安装版本，也可以用 `/Applications/Kaku.app/Contents/MacOS/kaku-gui --version`直接查询图形程序版本，不会打开窗口。

## kaku reset

移除 Kaku 管理的 shell 和 tmux 集成、Kaku 管理的 git delta 默认值、部分 Kaku 状态，以及 `~/.config/kaku/kaku.lua` 里的托管主题块。托管块之外的用户 Lua 会保留。谨慎使用；如果还想恢复 shell 集成，再运行 `kaku init`。

```bash
kaku reset
```

## kaku init

为 zsh 和/或 fish 配置或刷新 Kaku 的 shell 集成。会创建 `~/.config/kaku/zsh/kaku.zsh`，并可选创建 `~/.config/kaku/fish/kaku.fish`。在交互式 shell 里，它会询问是否通过 Homebrew 安装缺失的 Starship、Delta、Lazygit、Yazi 等可选 CLI 工具。

```bash
kaku init
```

如果 `kaku` 命令从 shell 里消失了，用这个刷新集成，不触发可选工具安装提示：

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

从脚本和外部工具与 Kaku 的多路复用器交互。

```bash
kaku cli split-pane                          # split current pane
kaku cli split-pane -- bash -c "echo hello"  # split and run a command
kaku cli --help                              # list all subcommands
kaku cli split-pane --help                   # help for a specific subcommand
```

适合把 Kaku 接进需要以编程方式打开分屏或标签的 AI 工具和 shell 脚本。

---

Source: https://kaku.fun/zh/docs/cli
Site index for LLMs: https://kaku.fun/llms.txt
