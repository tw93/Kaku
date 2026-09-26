# 功能与 AI

Kaku 比普通终端多做了哪些事，从右键菜单、链接识别到 AI 助手和 Shell 工具。

## 鼠标操作

在分屏里右键，可以粘贴、搜索、打开 AI 对话或命令面板，往任意方向拆分，或者关掉点中的分屏，关闭前要不要确认跟着你的设置走，正在接管鼠标的 TUI 还是会自己收到这次右键。

`Cmd + 点击`打开网址和文件路径，网址在终端边缘自动折行也能拿到完整地址，硬换行之后的无关输出不会被拼进链接。

## Kaku Assistant

Kaku Assistant 能给失败的命令出修复建议，把一句描述变成命令，也能围绕终端输出和项目文件聊天。

**配置**

运行 `kaku ai` 打开 AI 设置，开启 Kaku Assistant 后填好下面几项：

| 字段 | 说明 |
| --- | --- |
| Auth Type | API key 或 Codex CLI 登录 |
| Simple Model | `#` 生成命令、修复命令和轻量对话 |
| Deep Model | `Cmd + L` / `k` 的主力对话和工具调用 |
| Base URL | OpenAI 兼容的 API 根地址，比如 `https://api.openai.com/v1` |
| API Key | Auth Type 选 API key 时，填服务商给的 key |

接自定义的 OpenAI 兼容服务时，Auth Type 保持 API key，填上它的 Base URL，模型名自己手动写。

用 API key 连接时还要选 **API Mode**，`chat_completions` 或 `responses`，Responses 模式下服务商支持的话还能开 **Native Web Search**。

用 Codex 鉴权时，Kaku 直接读现有的 Codex 连接设置，**Follow Codex** 跟着这个连接用同一个模型，也可以在 Kaku 里单独指定 Simple Model 和 Deep Model。

## AI Chat 面板

按 `Cmd + L` 打开 AI Chat 面板，回答以 Markdown 流式输出，代码块带高亮，可以带上终端上下文，经你同意后还能调用项目文件、shell 命令、网页搜索和记忆这几类工具，它也会根据当前目录里的 `Cargo.toml`、`package.json` 这类文件把项目类型告诉模型。

Simple Model 和 Deep Model 不同时，按 `Shift + Tab` 在两者之间切换，两者相同时，`/model` 会列出当前服务商返回的全部模型，你配置的模型始终留在列表里。输入 `/suggest` 可以让 Kaku 猜你下一句想发什么。

在 shell 里用 `k` 或 `kaku chat`，接着用的是同一份对话记录：

```bash
k "summarize the current project"
kaku chat
```

shell 里的版本比面板简单，只输出纯文本，支持 `/new`、`/resume`、`/clear`、`/status`、`/memory` 和 `/exit`。

**命令失败修复**

命令以非零状态退出时，Kaku Assistant 会自动把这条命令、退出码、工作目录和 git 分支发给模型，在行内给出修复建议，按 `Cmd + Shift + E` 粘回终端，`rm -rf`、`git reset --hard` 这类危险命令只粘贴，不会自动执行。

`Ctrl+C` 退出、help 参数、单独调用包管理器、git pull 冲突，以及前台不是 shell 的进程，都不会触发。

**把一句人话变成命令**

在提示符里输入 `# <描述>` 回车，Kaku 会在 shell 执行之前把这行拦下来，连同当前目录和 git 分支发给模型，再把生成的命令放回提示符，你看一眼没问题再运行。

```
# list all files modified in the last 7 days
# find and kill the process on port 3000
# compress the src folder excluding node_modules
```

`#` 生成和自动修复跟 `Cmd + L` 走同一个服务商和同一份凭据，所以 Codex 或 Copilot 登录在这里也能用，不只是 API key。

`#` 前缀在 zsh 和 fish 里都能用，请求回来之前你的原始描述一直留在屏幕上，模型给不出安全的命令时会改成解释原因，危险命令会放进提示符但标出来等你确认，不会自动执行。

**assistant.toml 字段**

配置文件在 `~/.config/kaku/assistant.toml`：

| 字段 | 说明 |
| --- | --- |
| `enabled` | `true` 开启，`false` 关闭 |
| `api_key` | AI 服务的 API key |
| `model` | Simple Model，用于 `#` 生成命令、修复命令和轻量对话 |
| `chat_model` | Deep Model，用于 `Cmd + L` / `k` 的主力对话和工具调用 |
| `chat_model_choices` | 可选，面板模型选择器里的对话模型清单，设置后代替服务商返回的列表 |
| `auto_fix_ignored_exit_codes` | 可选，这些退出码不触发自动修复建议，比如 `[2]` |
| `base_url` | OpenAI 兼容的 API 根地址 |
| `custom_headers` | 给企业代理加的额外 HTTP 头，比如 `["X-Customer-ID: your-id"]` |
| `web_search_provider` | 可选的搜索后端：`brave`、`pipellm` 或 `tavily` |
| `web_search_api_key` | 所选搜索后端的 API key |
| `web_fetch_script` | 可选，自定义把网址抓成 Markdown 的脚本 |
| `chat_tools_enabled` | 对话服务不支持工具调用时设为 `false` |
| `auth_type` | 鉴权模式：`api_key`、`codex` 或 `copilot`，设置面板里只有前两个，`copilot` 需要手动填 |
| `memory_curator_model` | 可选，后台整理记忆时用的便宜模型 |

旧配置里可能还留着 `fast_model`，Kaku 会把它当成 Simple Model，下次保存 Assistant 设置时并回 `model`。

## 窗口快照

关闭或隐藏窗口时，Kaku 会自动保存多标签、多分屏的窗口布局，用 **Shell > Restore Previous Window** 或 `Cmd + Option + Shift + T` 就能打开上次保存的布局，快照文件丢了或坏了也不会出错，只会提示没有可用的快照。

## AppleScript

Kaku 带了一份极简的 AppleScript 字典，所以能在 Script Editor 和其他自动化工具里看到它，接口刻意做得很小，除了 `quit` 都是只读的。

```applescript
tell application "Kaku"
  get name        -- "Kaku"
  get version     -- e.g. "0.20.0"
  get frontmost   -- true / false
  quit            -- optional `saving ask|yes|no`
end tell
```

想看完整字典，在 Script Editor 里选 File > Open Dictionary，再选 `/Applications/Kaku.app`。Kaku 没有 `do script` 动词，AppleScript 没法通过 Kaku 执行 shell 命令。

## Lazygit 集成

按 `Cmd + Shift + G` 在当前分屏打开 lazygit，Kaku 会从 PATH 或 Homebrew 的常见安装位置找到它。

git 仓库里有未提交的改动、而你还没在这个目录用过 lazygit 时，Kaku 会提示一次。

用 `brew install lazygit` 或 `kaku init` 安装 lazygit。

## Yazi 文件管理器

按 `Cmd + Shift + Y` 在当前分屏打开 yazi，shell 里的 `y` 包装同样能打开，退出时还会同步 shell 的工作目录。

**主题同步**：Kaku 会自动更新 `~/.config/yazi/theme.toml`，跟着当前配色用 Kaku Dark 或 Kaku Light，yazi 这边不用自己配主题。

用 `brew install yazi` 或 `kaku init` 安装 yazi。

## 远程会话

分屏连到别的机器时，标签上显示 ssh 图标和主机名，不再是本地路径，远程标签一眼就能认出来，分屏标签里只有远程那一侧会显示主机名，哪边是本地也很清楚。`ssh`、`mosh`、`autossh`、`et` 会话都能识别。

在远程分屏里，AI 聊天会关掉本地的文件和 shell 工具，因为当前目录在另一台主机上，在本机跑只会碰到同名的本地路径，也不会去识别本机的项目类型。面板这时根据屏幕上的内容回答，给出让你在远程主机上执行的命令，`@cwd` 在这里不可用，会直接说明，不会附上错的目录。

内置的 zsh、fish、bash 集成都会包装 `ssh`，远程主机没有 `kaku` terminfo 时回退到 `xterm-256color`，`mosh` 也一样，你自己定义了 `ssh` 函数的话，Kaku 不会动它。

## 远端文件

按 `Cmd + Shift + R` 用 `sshfs` 把当前 SSH 会话的远端文件系统挂到本地，并在 yazi 里打开。

Kaku 从当前分屏自动识别 SSH 目标，挂载在 `~/Library/Caches/dev.kaku/sshfs/<host>`。

前置条件：装好 `sshfs`（`brew install macfuse sshfs`），远端主机配好基于密钥的 SSH 登录。

## Shell 套件

Kaku 自带一组 shell 插件，在 Kaku 会话里自动加载。

**Zsh 插件（内置）**

- **zsh-z**：更聪明的 `cd`，会记住你最常去的目录，也沿用已有的 `~/.z` 历史，`z <dir>` 跳转，`z -l <dir>` 列出匹配，`z -t` 看最近去过的目录，文件系统补全没有结果时，`cd` + Tab 也会从这份历史里补。
- **zsh-completions**：常用 CLI 工具的扩展补全。
- **fast-syntax-highlighting**：输入时实时着色、标出错误，颜色比 zsh-syntax-highlighting 更丰富，启动也更快。
- **zsh-autosuggestions**：像 fish 一样，边打字边根据历史给出建议。

**Fish 支持**

fish 用户运行 `kaku init` 会生成 `~/.config/kaku/fish/kaku.fish`，`kaku doctor` 会同时检查 zsh 和 fish 的集成。

**可选工具（通过 `kaku init` 安装）**

- **Starship**：快、可定制的提示符，带 git 和环境信息，只在 Kaku 里生效，其他终端还是原来的提示符，想在所有终端都用，就在 shell rc 里 Kaku 那一行之前设 `KAKU_PROMPT_EVERYWHERE=1`。
- **Delta**：给 git diff 和 grep 用的语法高亮分页器。
- **Lazygit**：终端里的 git 界面。
- **Yazi**：终端文件管理器。

**Smart Tab**

Smart Tab 接管 zsh 里的 Tab 键，让补全更顺手，有三种模式：

| 模式 | 行为 | 环境变量 |
| --- | --- | --- |
| Suggestion First（默认） | 有灰色自动建议时 Tab 先接受建议，没有就弹出补全列表 | 无 |
| Completion First | Tab 先弹出补全列表，用 `->` 接受自动建议 | `KAKU_TAB_ACCEPT_SUGGEST_FIRST=0` |
| Off | 完全关掉 Smart Tab，恢复 zsh 原生的 Tab 行为 | `KAKU_SMART_TAB_DISABLE=1` |

也可以在 `kaku config` 里改（Behavior 下的 **Smart Tab**），或者写进 `kaku.lua`：

```lua
config.smart_tab_mode = "suggestion_first"   -- 默认：Tab 先接受自动建议
config.smart_tab_mode = "completion_first"   -- Tab 先显示补全列表
config.smart_tab_mode = "off"                -- 关闭 Smart Tab
```

如果更习惯用环境变量，比如几个终端共用一份 zshrc，就在 source Kaku shell 集成之前加上其中一行：

```zsh
export KAKU_TAB_ACCEPT_SUGGEST_FIRST=0  # 改回 completion-first（suggestion-first 是默认）
# or
export KAKU_SMART_TAB_DISABLE=1         # disable Smart Tab
```

shell rc 里设的环境变量优先于 `kaku.lua`。Smart Tab 只在 Kaku 会话里生效（`TERM_PROGRAM=Kaku`，或者从 Kaku 里启动的 tmux）。

---

Source: https://kaku.fun/zh/docs/features
Site index for LLMs: https://kaku.fun/llms.txt
