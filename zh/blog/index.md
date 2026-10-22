# Kaku 博客

关于 Mac 终端、AI 编程工具和 Kaku 怎么做出来的一些记录，作者 Tw93。

## Claude Code 与 Codex

在终端里安装、配置和使用 AI 编程助手。

- [Claude Code 斜杠命令：常用命令和自定义命令写法](https://kaku.fun/zh/blog/claude-code-slash-commands.md) (2026-10-22): Claude Code 常用斜杠命令、配套快捷键，以及带参数的自定义命令写法和三个可抄的例子。
- [Claude Code 权限：模式、规则和跳过确认](https://kaku.fun/zh/blog/claude-code-permissions.md) (2026-10-21): Claude Code 的权限模式、allow/ask/deny 规则，bypass 的风险，一份更稳的配置，以及拦命令的 hook。
- [CLAUDE.md 怎么写：记忆文件、加载顺序和该写什么](https://kaku.fun/zh/blog/claude-md-memory.md) (2026-10-20): Claude Code 从哪读 CLAUDE.md，文件怎么叠加，引用和 rules，该写什么，以及和 AGENTS.md 的关系。
- [配置 Claude Code 的 MCP 服务器](https://kaku.fun/zh/blog/claude-code-mcp-servers.md) (2026-10-19): 用 claude mcp add 接 HTTP 或 stdio，分清 local、project、user，密钥放环境变量。
- [Claude Code subagents 是什么，什么时候用](https://kaku.fun/zh/blog/claude-code-subagents.md) (2026-10-11): subagent 和 skill、agent teams 有什么区别，怎么定义一个，适合什么活，以及在上下文和 token 上的代价。
- [Codex CLI 和 Claude Code 对比](https://kaku.fun/zh/blog/codex-vs-claude-code.md) (2026-10-09): 从安装、登录、配置文件、审批、扩展方式和通知几方面对比 Codex CLI 和 Claude Code，以及在同一个仓库里一起用。
- [用 git worktree 同时跑多个 Claude Code](https://kaku.fun/zh/blog/claude-code-git-worktrees.md) (2026-10-07): 用 git worktree 或 claude --worktree 给每个 Claude Code 会话一份独立的检出，配好环境，再把改动合回去并清理干净。
- [Claude Code skills 怎么写、怎么用](https://kaku.fun/zh/blog/claude-code-skills.md) (2026-10-06): skill 和 CLAUDE.md、命令、subagent 有什么区别，SKILL.md 和它的描述怎么写，以及怎么分享 skill。
- [配置 Claude Code 状态栏，显示模型、上下文和 git 分支](https://kaku.fun/zh/blog/claude-code-statusline.md) (2026-10-05): 给 Claude Code 配一个状态栏，读懂它收到的会话 JSON，在不拖慢的前提下显示模型、上下文用量和 git 分支。
- [Claude Code hooks 实用指南](https://kaku.fun/zh/blog/claude-code-hooks.md) (2026-10-03): hook 写在哪、哪些事件真正有用，以及三个能直接用的例子，编辑后格式化、拦下危险命令、做完时发通知。
- [在 Mac 上安装和更新 Claude Code](https://kaku.fun/zh/blog/install-claude-code-mac.md) (2026-10-02): 用原生安装脚本、Homebrew 或 npm 装好 Claude Code，登录后用 claude doctor 检查，再管好自动更新和发布渠道。
- [远程使用 Claude Code，接上正在跑的会话的三种办法](https://kaku.fun/zh/blog/claude-code-remote.md) (2026-09-30): Anthropic 的 Remote Control、云端会话，以及用 kaku remote 在另一台电脑上打开 Mac 的终端，各自需要什么、跑在哪里。
- [在 Mac 上配好 Codex CLI：安装、登录和配置](https://kaku.fun/zh/blog/codex-cli-setup.md) (2026-09-29): 在 macOS 上安装 OpenAI 的 Codex CLI，用 ChatGPT 账号或 API key 登录，再配好 config.toml、审批、通知和会话恢复。
- [Claude Code agent teams 怎么打开，在 Kaku 里显示在哪](https://kaku.fun/zh/blog/claude-code-agent-teams.md) (2026-09-26): agent teams 默认关着，要设 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1，在 Kaku 里分屏要走 tmux。
- [Cursor 和 Claude Code 怎么选](https://kaku.fun/zh/blog/cursor-vs-claude-code.md) (2026-09-25): Cursor 和 Claude Code 在哪里看改动、怎么收费、上下文、并行会话上有什么不同，以及两者怎么一起用。
- [Claude Code 的 plan mode 什么时候用、怎么用](https://kaku.fun/zh/blog/claude-code-plan-mode.md) (2026-09-23): plan mode 允许做什么，怎么用 Shift+Tab、/plan 或命令行参数打开，怎么审和改计划，以及什么时候不用它。
- [Claude Code 通知不弹出，在 Mac 上怎么修](https://kaku.fun/zh/blog/claude-code-notifications.md) (2026-09-20): 为什么 Claude Code 和 Codex 在大多数终端里不发通知，每种情况该改哪个设置，以及终端通知序列是怎么走到 macOS 的。
- [Claude Code 用什么终端最好，真正影响体验的五件事](https://kaku.fun/zh/blog/best-terminal-for-claude-code.md) (2026-09-19): Claude Code 在哪个终端都能跑，长时间会话舒不舒服取决于五件事：多行输入、通知、可点击的路径、重启后恢复和分屏。

## 终端

挑选和配置 Mac 终端，包括字体、分屏和快捷键。

- [终端 shell 集成靠 OSC 把目录和提示符交给终端](https://kaku.fun/zh/blog/shell-integration-osc.md) (2026-10-17): OSC 7、133、9、8 各自告诉终端什么，Kaku 0.25.0 的 fish 和 zsh 实际发哪些，以及怎么用 printf 核对。
- [终端模拟器是怎么工作的](https://kaku.fun/zh/blog/how-terminal-emulators-work.md) (2026-10-16): 终端管着 pty 和字符网格，shell 是写字节的子进程。顺着这条路径，字节变成格子，再变成像素。
- [Mac 终端里最省时间的快捷键](https://kaku.fun/zh/blog/mac-terminal-shortcuts.md) (2026-10-13): 在任何终端里都能用的命令行编辑快捷键，怎么让 Option 当 Meta 用，以及标签、分屏和搜索的应用快捷键。
- [Ghostty 配置指南，Mac 上值得改的几项设置](https://kaku.fun/zh/blog/ghostty-config.md) (2026-10-10): Ghostty 的配置文件在哪、怎么写，以及在 Mac 上值得改的字体、主题、窗口、快捷键和 Option 键设置。
- [Mac 上的 Nerd Font，装补丁字体还是用符号回退](https://kaku.fun/zh/blog/nerd-fonts-mac.md) (2026-10-08): 提示符和终端工具里的图标为什么变成方块，怎么用 Homebrew 装补丁版 Nerd Font，什么时候只靠符号回退就够了。
- [在 Mac 上还需要 tmux 吗](https://kaku.fun/zh/blog/tmux-alternative-mac.md) (2026-10-01): tmux 能做到哪些终端做不到的事，原生分屏和会话恢复已经覆盖了哪些，以及远程工作时为什么还值得留着 tmux。
- [一份实用的 Mac 版 WezTerm 配置，以及 Kaku 改了什么](https://kaku.fun/zh/blog/wezterm-config-for-mac.md) (2026-09-28): 在 macOS 上最值得改的 WezTerm 设置，从字体、行高到快捷键和渲染，以及 Kaku 选了哪些默认值、为什么。
- [Ghostty、iTerm2 和 WezTerm 怎么选](https://kaku.fun/zh/blog/ghostty-vs-iterm2-vs-wezterm.md) (2026-09-22): 三个常用的 Mac 终端，按配置方式、分屏和 tmux、图片、通知、发版节奏以及跑 AI 编程工具来对比。
- [2026 年 Mac 上的终端怎么选](https://kaku.fun/zh/blog/best-terminal-for-mac.md) (2026-09-21): Terminal.app、iTerm2、Ghostty、WezTerm、Warp、Kitty、Alacritty 和 Kaku，按上手成本、配置方式、AI 工具和价格对比，2026 年 10 月核对。

## Shell 与命令行工具

配置 zsh、提示符和围绕它的命令行工具。

- [在 Mac 上配置 SSH config](https://kaku.fun/zh/blog/ssh-config-mac.md) (2026-10-18): 具体 Host 写在 Host * 前面，用 ssh -G 核对，钥匙串选项只留给系统自带的 ssh。
- [在 Mac 上用 Yazi，快速的终端文件管理器](https://kaku.fun/zh/blog/yazi-file-manager.md) (2026-10-15): 在 macOS 上装好 Yazi 和预览依赖，记住常用按键，加上退出时切换目录的 y 包装函数，再做些配置。
- [在 Mac 上给 zsh 配好 fzf，搜历史、找文件、跳目录](https://kaku.fun/zh/blog/fzf-zsh-mac.md) (2026-10-14): 安装 fzf，打开它在 zsh 里搜历史、找文件、跳目录的快捷键，解决 Mac 上 Alt+C 不生效的问题，再加上预览和 fzf-tab。
- [在 Mac 上用 Starship 提示符，安装、配置和保持够快](https://kaku.fun/zh/blog/starship-prompt.md) (2026-10-12): 给 zsh 或 fish 装好 Starship，配置 starship.toml、格式字符串和预设，找出拖慢提示符的模块和超时设置。
- [Mac 上用 zsh 还是 bash](https://kaku.fun/zh/blog/zsh-vs-bash.md) (2026-10-04): macOS 为什么换成 zsh，日常用起来有哪些差别，脚本该用哪个 shell 写，以及怎么用 chsh 或终端设置切换。
- [不用 Oh My Zsh，把 zsh 配得又快又好用](https://kaku.fun/zh/blog/zsh-without-oh-my-zsh.md) (2026-09-27): 用原生 zsh 配好自动建议、语法高亮、补全和 Starship 提示符，讲清楚保持启动快的加载顺序，以及怎么测。
- [Lazygit 加 delta，在终端里用好 git](https://kaku.fun/zh/blog/lazygit-and-delta.md) (2026-09-24): 安装 lazygit 和 delta，把 delta 接进 git 和 lazygit 的 diff 视图，以及审 agent 改动时最顺手的几个快捷键。
