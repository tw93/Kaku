# Kaku Blog

Notes on Mac terminals, AI coding tools and how Kaku is built, by Tw93.

## Claude Code and Codex

Install, configure and run AI coding agents in the terminal.

- [Gemini CLI on Mac: Install, Sign In and What Changed in 2026](https://kaku.fun/blog/gemini-cli-setup.md) (2026-10-26): Install Gemini CLI with npm or Homebrew, which sign-in paths still work after June 18, 2026, GEMINI.md, settings.json, MCP, sandboxing and Kaku.
- [Claude Code Cost: Pricing, /usage, ccusage and Ways to Cut It](https://kaku.fun/blog/claude-code-cost.md) (2026-10-23): How Claude Code is billed on plans and API keys, where /usage and ccusage show spend, what drives cost, and the habits that cut it.
- [Claude Code Slash Commands: The Ones That Matter and How to Write Your Own](https://kaku.fun/blog/claude-code-slash-commands.md) (2026-10-22): The Claude Code slash commands worth knowing, their keyboard shortcuts, and how to write custom commands with arguments, plus three to copy.
- [Claude Code Permissions: Modes, Rules and --dangerously-skip-permissions](https://kaku.fun/blog/claude-code-permissions.md) (2026-10-21): Claude Code permission modes, allow/ask/deny rules, what bypassPermissions risks, a safer middle-ground config and a hook that blocks commands.
- [CLAUDE.md Explained: Memory Files, Load Order and What to Write](https://kaku.fun/blog/claude-md-memory.md) (2026-10-20): Where Claude Code reads CLAUDE.md from, how files stack, imports and rules, what to put in one, and how it relates to AGENTS.md and auto memory.
- [Claude Code MCP Servers: Add, Scope and Approve Them](https://kaku.fun/blog/claude-code-mcp-servers.md) (2026-10-19): Add Claude Code MCP servers over HTTP or stdio, choose local, project, or user scope, and keep tokens out of a committed .mcp.json.
- [Claude Code Subagents: What They Are and When They Help](https://kaku.fun/blog/claude-code-subagents.md) (2026-10-11): How subagents differ from skills and agent teams, how to define one, which jobs they suit, and what they cost in context and tokens.
- [Codex CLI vs Claude Code: A Practical Comparison](https://kaku.fun/blog/codex-vs-claude-code.md) (2026-10-09): Codex CLI and Claude Code compared on install, sign-in, config files, approvals, extension points and notifications, and how to run both on one repo.
- [Run Several Claude Code Sessions at Once with Git Worktrees](https://kaku.fun/blog/claude-code-git-worktrees.md) (2026-10-07): Give each Claude Code session its own checkout with git worktree or claude --worktree, set each one up, merge the work back and clean up.
- [Claude Code Skills: How to Write and Use Them](https://kaku.fun/blog/claude-code-skills.md) (2026-10-06): What a skill is next to CLAUDE.md, commands and subagents, how to write SKILL.md and its description, and how to share skills.
- [Claude Code Status Line: Show Model, Context and Git Branch](https://kaku.fun/blog/claude-code-statusline.md) (2026-10-05): Set up a Claude Code status line, read the session JSON it receives, and show the model, context use and git branch without slowing it down.
- [Claude Code Hooks: A Practical Guide with Examples](https://kaku.fun/blog/claude-code-hooks.md) (2026-10-03): Where hooks live, the events that matter, and three working examples: format after an edit, block a command, and notify when Claude finishes.
- [Install Claude Code on Mac: Installer, Homebrew and Updates](https://kaku.fun/blog/install-claude-code-mac.md) (2026-10-02): Install Claude Code with the native installer, Homebrew or npm, sign in, check it with claude doctor, and control updates and release channels.
- [Claude Code Remote: Three Ways to Reach a Running Session](https://kaku.fun/blog/claude-code-remote.md) (2026-09-30): Anthropic's Remote Control, cloud sessions, and opening your Mac's terminal from another computer with kaku remote: what each needs and where it runs.
- [Setting Up Codex CLI on Mac: Install, Login and Config](https://kaku.fun/blog/codex-cli-setup.md) (2026-09-29): Install OpenAI's Codex CLI on macOS, sign in with ChatGPT or an API key, and set up config.toml, approvals, notifications and resuming sessions.
- [Claude Code Agent Teams: Turn Them On and Where They Show Up](https://kaku.fun/blog/claude-code-agent-teams.md) (2026-09-26): Agent teams stay off until CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1. In Kaku, teammates stay in one panel unless you run Claude Code in tmux with split panes on.
- [Cursor vs Claude Code: Editor Agent or Terminal Agent](https://kaku.fun/blog/cursor-vs-claude-code.md) (2026-09-25): How Cursor and Claude Code differ in where you review changes, what you pay for, context, parallel sessions, and how to use both together.
- [Claude Code Plan Mode: When and How to Use It](https://kaku.fun/blog/claude-code-plan-mode.md) (2026-09-23): What plan mode allows in Claude Code, how to turn it on with Shift+Tab, /plan or a flag, how to review and edit the plan, and when to skip it.
- [Claude Code Notifications Not Showing? How to Fix Them on Mac](https://kaku.fun/blog/claude-code-notifications.md) (2026-09-20): Why Claude Code and Codex stay silent in most terminals, which setting fixes each one, and how terminal notification sequences reach macOS.
- [The Best Terminal for Claude Code: What Actually Matters](https://kaku.fun/blog/best-terminal-for-claude-code.md) (2026-09-19): Any terminal runs Claude Code. Five things decide how long sessions feel: multi-line prompts, notifications, clickable paths, restarts and splits.

## Terminal

Choose and configure a Mac terminal: fonts, splits, shortcuts.

- [Shell Integration: What OSC 7, 133 and 9 Tell the Terminal](https://kaku.fun/blog/shell-integration-osc.md) (2026-10-17): How OSC 7, 133, 9 and 8 reach a terminal, which ones Kaku 0.25.0 emits for zsh and fish, and printf checks for directory, prompts and notifications.
- [How Terminal Emulators Work](https://kaku.fun/blog/how-terminal-emulators-work.md) (2026-10-16): A terminal owns the pty and the cell grid. The shell is a child that writes bytes. Follow that path from those bytes to pixels.
- [Mac Terminal Shortcuts That Save the Most Time](https://kaku.fun/blog/mac-terminal-shortcuts.md) (2026-10-13): The shell line-editing keys that work in every terminal, how to make Option act as Meta, and the app shortcuts for tabs, splits and search.
- [Ghostty Config Guide: The Settings Worth Changing on Mac](https://kaku.fun/blog/ghostty-config.md) (2026-10-10): Where Ghostty's config lives, its syntax, and the font, theme, window, keybind and Option key settings worth changing on a Mac.
- [Nerd Fonts on Mac: Install Them or Use a Symbols Fallback](https://kaku.fun/blog/nerd-fonts-mac.md) (2026-10-08): Why prompts and TUIs show boxes instead of icons, how to install a patched Nerd Font with Homebrew, and when a symbols-only fallback is enough.
- [tmux Alternative on Mac: Do You Still Need tmux?](https://kaku.fun/blog/tmux-alternative-mac.md) (2026-10-01): What tmux gives you that a terminal does not, what native splits and session restore already cover, and when to keep tmux for remote work.
- [A Practical WezTerm Config for Mac, and What Kaku Changes](https://kaku.fun/blog/wezterm-config-for-mac.md) (2026-09-28): The WezTerm settings that matter most on macOS, from fonts and line height to keys and rendering, and which defaults Kaku picked and why.
- [Ghostty vs iTerm2 vs WezTerm: Which One Fits You](https://kaku.fun/blog/ghostty-vs-iterm2-vs-wezterm.md) (2026-09-22): Three popular Mac terminals compared on configuration, splits and tmux, images, notifications, release cadence and running AI coding tools.
- [The Best Terminal for Mac in 2026: Eight Options Compared](https://kaku.fun/blog/best-terminal-for-mac.md) (2026-09-21): Terminal.app, iTerm2, Ghostty, WezTerm, Warp, Kitty, Alacritty and Kaku compared on setup, configuration, AI tools and price, checked October 2026.

## Shell and tools

Set up zsh, the prompt and the command-line tools around it.

- [SSH Config on Mac: Hosts, Keys and the Keychain](https://kaku.fun/blog/ssh-config-mac.md) (2026-10-18): Put specific Host blocks in ~/.ssh/config, check them with ssh -G, and keep Apple keychain options on /usr/bin/ssh.
- [Yazi on Mac: A Fast Terminal File Manager Setup](https://kaku.fun/blog/yazi-file-manager.md) (2026-10-15): Install Yazi and its preview tools on macOS, learn the keys that matter, add the y wrapper that changes directory on quit, and configure it.
- [Set Up fzf with Zsh on Mac: History, Files and Directories](https://kaku.fun/blog/fzf-zsh-mac.md) (2026-10-14): Install fzf, turn on its zsh key bindings for history, files and directories, fix Alt+C on a Mac, and add previews and fzf-tab.
- [Starship Prompt on Mac: Install, Configure and Keep It Fast](https://kaku.fun/blog/starship-prompt.md) (2026-10-12): Install Starship for zsh or fish, configure starship.toml, format strings and presets, and find the modules and timeouts that make a prompt slow.
- [Zsh vs Bash on Mac: Which Shell Should You Use](https://kaku.fun/blog/zsh-vs-bash.md) (2026-10-04): Why macOS moved to zsh, the differences you notice day to day, which shell to write scripts in, and how to switch with chsh or Terminal settings.
- [A Fast Zsh Setup Without Oh My Zsh](https://kaku.fun/blog/zsh-without-oh-my-zsh.md) (2026-09-27): Autosuggestions, syntax highlighting, completions and a Starship prompt with plain zsh, the load order that keeps it fast, and how to measure it.
- [Lazygit and Delta: A Terminal Git Workflow on Mac](https://kaku.fun/blog/lazygit-and-delta.md) (2026-09-24): Install lazygit and delta, wire delta into git and into lazygit's diff view, and the shortcuts that make reviewing an agent's changes quick.
