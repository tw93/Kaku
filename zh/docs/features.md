# 功能与 AI

Kaku Assistant、AI Chat、错误恢复、自然语言命令和内置 Shell 工具。

## Kaku Assistant

Kaku Assistant 可以为失败的命令提供建议、根据描述生成命令，也可以围绕终端输出和项目文件聊天。

**配置**

运行 `kaku ai` 打开 AI 设置面板。开启 Kaku Assistant，然后直接编辑模型、鉴权、Base URL 和 API key 字段。

| 字段 | 说明 |
| --- | --- |
| Auth Type | API key 或 Codex CLI 登录 |
| Simple Model | 用于 `#` 命令生成、命令修复和轻量对话 |
| Deep Model | 用于主力 `Cmd + L` / `k` 对话和工具调用 |
| Base URL | OpenAI 兼容 API 根 URL，例如 `https://api.openai.com/v1` |
| API Key | Auth Type 为 API key 时填写服务方 API key |

使用自定义 OpenAI 兼容服务时，Auth Type 保持 API key，填写服务方的 Base URL，再手动设置模型名。

使用 API key 时，Base URL 填 API 根地址，例如 `https://api.openai.com/v1`，再按服务选择 **API Mode**：`chat_completions` 或 `responses`。Responses 模式下，服务商支持时还可以开启 **Native Web Search**。

使用 Codex 鉴权时，Kaku 读取已有的 Codex 连接设置，**Follow Codex** 跟随该连接的模型，也可以在 Kaku 中分别指定 Simple Model 和 Deep Model。

## AI Chat 面板

按 `Cmd + L` 打开内置的 AI Chat 面板。它会流式输出 Markdown 答案， 高亮代码块，可以带上终端上下文，也可以在你授权后调用工具， 访问项目文件、shell 命令、网页搜索和记忆。当 Simple Model 和 Deep Model 不同时，在面板里按 `Shift + Tab` 在两者之间切换。在面板里输入 `/suggest`， 让 Kaku 预测你接下来可能想发的内容。

在 shell 里用 `k` 或 `kaku chat` 可以共享同一份对话存储：

```bash
k "summarize the current project"
kaku chat
```

独立 CLI 比 overlay 更简单，只输出纯终端文本，支持 `/new`、`/resume`、`/clear`、`/status`、`/memory` 和 `/exit`。

**命令失败修复**

当一条命令以非零状态退出时，Kaku Assistant 会自动把失败的命令、退出码、工作目录和 git 分支发给 LLM，并在行内给出建议的修复方案。按 `Cmd + Shift + E` 把建议粘进终端。危险命令（比如 `rm -rf`、`git reset --hard`）只粘贴、永不自动执行。

以下情况不会触发 Assistant：`Ctrl+C` 退出、help 参数、单独的包管理器调用、git pull 冲突，以及非 shell 的前台进程。

**把一句人话变成命令**

在提示符里输入 `# <描述>` 再回车，就能把一句人话生成 shell 命令。Kaku 在 shell 看到这行之前先拦下来，把你的描述连同当前目录和 git 分支发给 LLM，再把生成的命令注回提示符，等你审一眼再运行。

```
# list all files modified in the last 7 days
# find and kill the process on port 3000
# compress the src folder excluding node_modules
```

`#` 生成和自动修复命令与 `Cmd + L` 使用同一服务商和同一份凭据，所以 Codex 或 Copilot 登录同样能用在这里，不是只有 API key 才行。

`#` 前缀在 zsh 和 fish 里都能用。请求进行中你的原始描述一直可见。如果模型给不出安全的命令，它会改成注入一段简短解释。危险命令会被载入但标记为需审查，永不自动执行。

**assistant.toml 字段**

配置文件在 `~/.config/kaku/assistant.toml`：

| 字段 | 说明 |
| --- | --- |
| `enabled` | `true` 开启，`false` 关闭 |
| `api_key` | 你的 AI 服务 API key |
| `model` | Simple Model，用于 `#` 命令生成、命令修复和轻量对话 |
| `chat_model` | Deep Model，用于主力 `Cmd + L` / `k` 对话和工具调用 |
| `chat_model_choices` | 可选，给 overlay 选择器用的对话模型清单 |
| `auto_fix_ignored_exit_codes` | 可选，不触发自动命令修复建议的退出码，比如 `[2]` |
| `base_url` | OpenAI 兼容的 API 根 URL |
| `custom_headers` | 给企业代理用的额外 HTTP 头，比如 `["X-Customer-ID: your-id"]` |
| `web_search_provider` | 可选搜索后端：`brave`、`pipellm` 或 `tavily` |
| `web_search_api_key` | 所选搜索后端的 API key |
| `web_fetch_script` | 可选，自定义的 URL 转 Markdown 抓取脚本 |
| `chat_tools_enabled` | 设为 `false` 可对不支持工具的对话服务关闭工具调用 |
| `auth_type` | 高级鉴权模式：`api_key`、`codex` 或 `copilot`。设置面板提供前两个，`copilot` 需手动填写 |
| `memory_curator_model` | 可选，给后台记忆整理用的更便宜的模型 |

旧配置里可能还有 `fast_model`，Kaku 会把它当作 Simple Model， 下次保存 Assistant 设置时再折叠回 `model`。

## 窗口快照

关闭或隐藏窗口时，Kaku 会自动保存多标签、多分屏的窗口布局。用 **Shell > Restore Previous Window** 或 `Cmd + Option + Shift + T` 重新打开上次保存的布局。快照文件缺失或损坏时 Kaku 也能容错，只会提示没有可用快照。

## AppleScript

Kaku 内置一份极简的 AppleScript 字典，因此它会出现在 Script Editor 和其他自动化工具里。暴露的接口故意做得很小，除了 `quit` 之外都是只读的。

```applescript
tell application "Kaku"
  get name        -- "Kaku"
  get version     -- e.g. "0.19.0"
  get frontmost   -- true / false
  quit            -- optional `saving ask|yes|no`
end tell
```

在 Script Editor 里打开 `/Applications/Kaku.app`，File → Open Dictionary 即可浏览完整字典。Kaku 没有 `do script` 动词，不会把 shell 执行暴露给 AppleScript。

## Lazygit 集成

按 `Cmd + Shift + G` 在当前分屏里启动 lazygit。Kaku 会自动从 PATH 或常见的 Homebrew 位置找到 lazygit 二进制。

当一个 git 仓库有未提交改动、且还没在这个目录用过 lazygit 时，Kaku 会显示一次性提示，提醒你可以用它。

用 `brew install lazygit` 或 `kaku init` 安装 lazygit。

## Yazi 文件管理器

按 `Cmd + Shift + Y` 在当前分屏里启动 yazi。shell 包装 `y` 同样会启动 yazi，并在退出时同步 shell 的当前工作目录。

**主题同步**：Kaku 会自动更新 `~/.config/yazi/theme.toml`，匹配当前色彩方案，无需手动配置。

用 `brew install yazi` 或 `kaku init` 安装 yazi。

## 远程会话

某个分屏连到了别的机器时，Kaku 会在标签上显示一个 ssh 图标和主机名，而不是本地路径，远程标签一眼就能认出来。分屏的标签里只有远程那一侧会贡献主机名，所以你能看出哪边是本地。`ssh`、`mosh`、`autossh`、`et` 会话都能识别。

在远程分屏里，AI 聊天的行为会变。本地的文件和 shell 工具会关掉，因为当前目录属于另一台主机，在本地跑这些工具只会打到同名的本地路径上。此时聊天面板基于屏幕上的内容回答，并给出你可以在远程主机上执行的命令。`@cwd` 在这里不可用，并会直接说明原因，而不是附上错的目录。

内置的 zsh、fish、bash 集成对 `ssh` 的包装行为一致：远程主机没有 `kaku` terminfo 时回退到 `xterm-256color`，`mosh` 也有同样的回退。如果你自己定义了 `ssh` 函数，Kaku 不会覆盖它。

## 远端文件

按 `Cmd + Shift + R` 通过 `sshfs` 把当前 SSH 会话的远端文件系统挂到本地，并用 yazi 打开。

Kaku 会从当前活动分屏自动识别 SSH 目标。挂载点在 `~/Library/Caches/dev.kaku/sshfs/<host>`。

前置条件：装好 `sshfs`（`brew install macfuse sshfs`），并对远端主机配好免密 SSH 鉴权（基于密钥）。

## Shell 套件

Kaku 内置一套精选的 shell 插件，会在 Kaku 会话里自动加载。

**Zsh 插件（内置）**

- **zsh-z**：更聪明的 `cd`，会学习你最常用的目录，并复用已有的 `~/.z` 历史。用 `z <dir>`，用 `z -l <dir>` 列出匹配项，用 `z -t` 看最近目录。当文件系统补全没有匹配时，`cd` + Tab 也会回退到这份历史。
- **zsh-completions**：常见 CLI 工具的扩展补全。
- **fast-syntax-highlighting**：实时命令着色和错误高亮，颜色比 zsh-syntax-highlighting 更丰富、启动也更快。
- **zsh-autosuggestions**：fish 风格的历史补全，边打字边提示。

**Fish 支持**

fish 用户运行 `kaku init` 即可生成 `~/.config/kaku/fish/kaku.fish`。`kaku doctor` 会检查 zsh 和 fish 集成。

**可选工具（通过 `kaku init` 安装）**

- **Starship**：快速、可定制的提示符，带 git 和环境信息。Kaku 只在 Kaku 内启用它，你的其他终端保留各自原有的提示符。
- **Delta**：给 git diff 和 grep 用的语法高亮分页器。
- **Lazygit**：终端里的 git 界面。
- **Yazi**：终端文件管理器。

**Smart Tab**

Kaku 的 Smart Tab 接管了 zsh 里的 Tab 键，提供更聪明的补全行为。它支持三种模式：

| 模式 | 行为 | 环境变量 |
| --- | --- | --- |
| Suggestion First（默认） | 有灰色自动建议时 Tab 先接受建议，否则回退到补全列表 | -（这就是默认） |
| Completion First | Tab 先显示补全列表；用 `->` 接受自动建议 | `KAKU_TAB_ACCEPT_SUGGEST_FIRST=0` |
| Off | 完全关闭 Smart Tab，恢复 zsh 原生的 Tab 行为 | `KAKU_SMART_TAB_DISABLE=1` |

也可以通过 `kaku config`（Behavior 下的 **Smart Tab** 选项）或在 `kaku.lua` 里设置：

```lua
config.smart_tab_mode = "suggestion_first"   -- 默认：Tab 先接受自动建议
config.smart_tab_mode = "completion_first"   -- Tab 先显示补全列表
config.smart_tab_mode = "off"                -- 关闭 Smart Tab
```

如果你更想用环境变量（比如因为你在多个终端之间共享同一份 zshrc），在 source Kaku 的 shell 集成之前加上其中一行：

```zsh
export KAKU_TAB_ACCEPT_SUGGEST_FIRST=0  # 改回 completion-first（suggestion-first 是默认）
# or
export KAKU_SMART_TAB_DISABLE=1         # disable Smart Tab
```

```fish
set -gx KAKU_SMART_TAB_DISABLE 1
```

在 shell rc 里设置的环境变量优先级高于 `kaku.lua` 的设置。Smart Tab 只在 Kaku 会话里生效（`TERM_PROGRAM=Kaku`）。

---

Source: https://kaku.fun/zh/docs/features
Site index for LLMs: https://kaku.fun/llms.txt
