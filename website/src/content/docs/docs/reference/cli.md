---
title: CLI 命令
description: Kaku 命令行接口完整参考
---

# CLI 参考

在终端运行 `kaku` 可查看所有可用命令。

## kaku ai

在 Kaku 内打开 AI 设置面板。可配置外部 AI 编码工具（Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code 等）以及 Kaku Assistant。

```bash
kaku ai
```

## kaku config

用默认编辑器打开 Kaku 配置文件（`~/.config/kaku/kaku.lua`）。也可以在设置面板中通过 `Cmd + ,` 访问。

```bash
kaku config
```

## kaku doctor

运行诊断，检查 Kaku 的 shell 集成、PATH 配置以及可选工具的安装状态。如果你感觉有什么不对，先跑一下这个。

```bash
kaku doctor
```

## kaku update

检查并安装最新的 Kaku 发行版。

```bash
kaku update
```

## kaku reset

把 Kaku 的配置和状态文件重置为默认值。请谨慎使用——此命令会覆盖 `~/.config/kaku/kaku.lua`。

```bash
kaku reset
```

## kaku init

为 zsh 和/或 fish 安装 Kaku 的 shell 集成。会生成 `~/.config/kaku/zsh/kaku.zsh`，以及可选的 `~/.config/kaku/fish/kaku.fish`。同时会通过 Homebrew 安装可选的 CLI 工具（Starship、Delta、Lazygit、Yazi）。

```bash
kaku init
```

如果 `kaku` 命令在 shell 中丢失了，可用下面的命令恢复：

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

从脚本或外部工具中与 Kaku 多路复用器交互。

```bash
kaku cli split-pane                          # 拆分当前 pane
kaku cli split-pane -- bash -c "echo hello"  # 拆分并运行一条命令
kaku cli --help                              # 列出所有子命令
kaku cli split-pane --help                   # 查看某个子命令的帮助
```

适用于把 Kaku 集成到 AI 工具或需要以编程方式打开 pane / 标签的 shell 脚本中。
