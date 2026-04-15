---
title: AI 功能总览
description: Kaku 内建 AI 助手的完整介绍
---

# 功能

## Kaku Assistant

Kaku Assistant 提供两种模式：自动错误修复和按需的自然语言转命令。

**配置**

运行 `kaku ai` 打开 AI 设置面板。启用 Kaku Assistant，选择一个 Provider，并填入 API Key。

| Provider | Base URL | 模型 |
| :--- | :--- | :--- |
| OpenAI | `https://api.openai.com/v1` | （自由填写） |
| Custom | （手动填写） | （自由填写） |

选中预设 Provider 后会自动填好 Base URL 并刷新模型下拉列表。

**错误自动修复**

当命令以非零退出码结束时，Kaku Assistant 会自动把失败的命令、退出码、工作目录和 git 分支发送给 LLM，并在命令行内嵌显示建议的修复方案。按 `Cmd + Shift + E` 把建议粘贴到终端。对于危险命令（例如 `rm -rf`、`git reset --hard`），只会粘贴到命令行，绝不会自动执行。

以下情况不会触发助手：`Ctrl+C` 退出、help 参数、单独运行包管理器、git pull 冲突，以及非 shell 的前台进程。

**自然语言转命令**

在提示符前输入 `# <描述>` 并回车，即可从自然语言生成 shell 命令。Kaku 会在 shell 接收到这行之前拦截它，把你的查询连同当前目录和 git 分支发送给 LLM，再把返回的命令注回提示符，供你审查后执行。

```
# 列出最近 7 天修改过的所有文件
# 找到并终止占用 3000 端口的进程
# 压缩 src 目录，排除 node_modules
```

`#` 前缀在 zsh 和 fish 下都可用。请求发出期间原始查询保持可见。如果模型无法生成一个安全的命令，会改为注入一段简短的说明。危险命令也会被注入但会标注需要人工确认，绝不会自动执行。

**assistant.toml 字段**

配置文件位于 `~/.config/kaku/assistant.toml`：

| 字段 | 说明 |
| :--- | :--- |
| `enabled` | `true` 启用，`false` 禁用 |
| `api_key` | 你的 Provider API Key |
| `model` | 模型标识，例如 `DeepSeek-V3.2` |
| `base_url` | 兼容 OpenAI 协议的 API 根地址 |
| `custom_headers` | 用于企业代理的额外 HTTP 头，例如 `["X-Customer-ID: your-id"]` |

---

## Lazygit 集成

按 `Cmd + Shift + G` 在当前 pane 中启动 lazygit。Kaku 会自动从 PATH 或常见的 Homebrew 路径中探测 lazygit。

如果 git 仓库中有未提交的变更，且你还没在该目录使用过 lazygit，Kaku 会一次性提示你可以使用这个功能。

通过 `brew install lazygit` 或 `kaku init` 安装 lazygit。

---

## Yazi 文件管理器

按 `Cmd + Shift + Y` 在当前 pane 中启动 yazi。shell 包装命令 `y` 同样会启动 yazi，并在退出时同步 shell 的工作目录。

**主题同步**：Kaku 会自动更新 `~/.config/yazi/theme.toml`，使其与当前配色方案（Kaku Dark 或 Kaku Light）保持一致，无需手动配置 yazi 主题。

通过 `brew install yazi` 或 `kaku init` 安装 yazi。

---

## 远程文件

按 `Cmd + Shift + R` 即可通过 `sshfs` 把当前 SSH 会话的远程文件系统挂载到本地，并用 yazi 打开。

Kaku 会自动从活动 pane 中识别 SSH 目标。挂载点位于 `~/Library/Caches/dev.kaku/sshfs/<host>`。

前置条件：已安装 `sshfs`（`brew install macfuse sshfs`），并已为目标主机配置免密 SSH 登录（基于密钥）。

---

## Shell 套件

Kaku 自带一组精选的 shell 插件，在 Kaku 会话中自动加载。

**zsh 内建插件**

- **z**：更聪明的 `cd`，会学习你最常去的目录。用 `z <dir>` 跳转，`z -l <dir>` 列出候选，`z -t` 查看最近使用的目录。
- **zsh-completions**：为常见 CLI 工具提供的扩展补全。
- **zsh-syntax-highlighting**：实时命令着色和错误高亮。
- **zsh-autosuggestions**：类 fish 的基于历史的即时补全。

**Fish 支持**

运行 `kaku init` 会生成 `~/.config/kaku/fish/kaku.fish` 以供 fish 用户使用。`kaku doctor` 会同时检查 zsh 和 fish 的集成路径。

**可选工具（通过 `kaku init` 安装）**

- **Starship**：快速、可定制的命令行提示符，内置 git 和环境信息。
- **Delta**：带语法高亮的 git diff / grep 分页器。
- **Lazygit**：终端中的 git UI。
- **Yazi**：终端文件管理器。

**关闭 Smart Tab**

如果你已经有自己的补全工作流（例如 `fzf-tab`），可在加载 Kaku shell 集成之前设置：

```zsh
export KAKU_SMART_TAB_DISABLE=1
```

```fish
set -gx KAKU_SMART_TAB_DISABLE 1
```
