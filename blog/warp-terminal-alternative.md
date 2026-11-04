# Warp Terminal Alternatives for Mac: What to Pick and Why

> Why people leave Warp, what it still does best, and how iTerm2, Ghostty, WezTerm, Kitty, Alacritty and Kaku compare on GPU, config, AI and license.

Published 2026-11-04 · Tw93 · https://kaku.fun/blog/warp-terminal-alternative

If you want to leave Warp, the best alternative depends on what made you look. For a plain terminal with no account and no built-in AI, pick Ghostty or iTerm2. For a Lua-scripted one, pick WezTerm. For a terminal that works on the first launch and lets your own Claude Code or Codex do the AI work, Kaku is the one I make. Warp is still the better fit if you want an agent and blocks inside the terminal itself. This post lists what Warp does well, the reasons people leave, and how six alternatives compare on the things that usually decide it. Facts come from each project's own pages, read in October 2026.

## What Warp is good at

Warp calls its terminal [an open-source terminal built for AI-assisted software development](https://www.warp.dev/), and its client repository describes it as an agentic development environment. Two things set it apart from a classic terminal.

The first is [blocks](https://docs.warp.dev/terminal/blocks): each command and its output are grouped into one unit that you can copy, search, filter, bookmark, share and jump between. In an ordinary terminal that output is just a long scroll of text. The second is the built-in agent, and the repository says you can also bring a CLI agent such as Claude Code or Codex. If those two are what you came for, no alternative below copies them, and staying on Warp is the right call.

## Why people look elsewhere

Each of these is checked against what Warp's pages say today.

- **An account.** The quickstart puts sign-up in the top right corner and says you can skip it and start using Warp immediately, so a login is not needed to open the terminal.
- **Telemetry.** Warp's privacy docs say analytics are on a toggle, "Help improve Warp" under Settings, Privacy, and that with telemetry off no console interactions are persisted on its servers. The same page says telemetry must be on to use AI features on the free plan, so turning it off costs you those features there. Business and Enterprise plans are described as covered by a zero data retention agreement. Paid plans can turn telemetry off and keep using AI. Crash reports are a separate toggle, "Send crash reports", sent through Sentry. To see what Warp sends, search the Command Palette for "Show Warp Network Log", which inserts a `tail -f` command for its network log; the docs note that crash report traffic does not show up there.
- **AI that is part of the product.** Warp's agent is built in, and its pricing page sells agent usage on top of the free plan. If you already work with Claude Code or Codex, the built-in agent may be something you never use.
- **Not a plain terminal.** Warp groups output into blocks and puts its own input editor where the shell prompt usually is. If you want the terminal to stay out of the way of programs such as [Claude Code](/blog/best-terminal-for-claude-code), a classic terminal is simpler.

Closed source is no longer on the list. The [warpdotdev/warp repository](https://github.com/warpdotdev/warp) says the client codebase is open source, with the UI framework crates under MIT and the rest under AGPL v3. The README covers the client; it does not say whether the server side is open.

## The alternatives

| Terminal | GPU rendering | Config | AI features | License |
|---|---|---|---|---|
| Warp | Metal on macOS | Settings file and settings pages | Built-in agent, or bring your own CLI agent | AGPL v3, with MIT UI crates |
| iTerm2 | Optional GPU renderer | Settings window | Optional AI plugin with your own API key | GPL v2 or later, effectively v3 |
| Ghostty | Metal on macOS | Text file, `key = value` | None built in | MIT |
| WezTerm | GPU-accelerated | Lua | None built in | MIT |
| Kitty | GPU based | `kitty.conf` | None built in | GPL-3.0 |
| Alacritty | OpenGL | `alacritty.toml` | None built in | Apache-2.0 |
| Kaku | WezTerm's renderer | Lua, `~/.config/kaku/kaku.lua` | Optional assistant on your own provider | MIT |

Short notes on each:

**iTerm2** is the established Mac terminal. Its COPYING file says GPL v2 or later, effectively GPL v3 because of Apache-2.0 dependencies. Its docs describe a GPU renderer that improves drawing performance and may use more energy, and an AI plugin where you supply your own API key. Stay if you already have profiles you rely on.

**Ghostty** is MIT licensed, uses Metal on macOS, and reads a `key = value` file from `~/.config/ghostty/config.ghostty` or `~/Library/Application Support/com.mitchellh.ghostty/config.ghostty`, reloaded with `Cmd + Shift + ,`. It has no AI of its own, which suits people who bring their own agent.

**WezTerm** is a cross-platform terminal and multiplexer written in Rust, configured in Lua, with panes, tabs and windows on local and remote hosts. It suits people who like to script their terminal.

**Kitty** is GPU-based with its own `kitty.conf`, remote control from scripts and Python extensions called kittens.

**Alacritty** is deliberately small. Its README says you will not find tabs or splits, and leaves them to a window manager or tmux, so pair it with [tmux](/blog/tmux-alternative-mac) if you want them.

**Kaku** is a macOS terminal based on WezTerm with fonts, themes, shell integration and Mac shortcuts already set, and it keeps Lua configuration. It has no account, and its AI is optional: `kaku ai` points the assistant at your own provider. After a command fails, `Cmd + Shift + E` runs the suggested fix, and only pastes it without running when the command looks dangerous, such as `rm -rf`. Splits are `Cmd + D` and `Cmd + Shift + D`, and tabs and splits come back after a restart. It runs on macOS only, and it has no blocks and no hosted agent.

For a head-to-head on setup effort and release cadence, [the best terminal for Mac](/blog/best-terminal-for-mac) and [Ghostty vs iTerm2 vs WezTerm](/blog/ghostty-vs-iterm2-vs-wezterm) cover the same terminals in more depth.

## How to choose

- You want blocks and a built-in agent: stay on Warp, and decide about telemetry in its Privacy settings. If you use its agent, also look at Secret Redaction, which is off by default and masks text matching your patterns before AI requests are sent.
- You want AI from your own subscription: any terminal on the list runs Claude Code and Codex, so choose on the terminal itself. Kaku adds [notifications](/blog/claude-code-notifications) and `kaku ai` setup for them.
- You want the fewest moving parts: Ghostty.
- You want deep features behind a settings window: iTerm2.
- You want to program it: WezTerm, or Kaku if you want WezTerm's Lua with Mac defaults already chosen.
- You want minimal and use tmux: Alacritty.

## What goes wrong when you switch

- **Blocks are gone.** No alternative groups command output the same way. Shell history and search in your terminal replace part of it, but not the per-command copy.
- **Shell setup moves to your shell.** In a classic terminal, completions, prompt and syntax highlighting come from zsh itself. [A fast zsh setup without Oh My Zsh](/blog/zsh-without-oh-my-zsh) covers that, and Kaku bundles zsh plugins and optional Starship.
- **Keybindings.** Each terminal has its own shortcut set, so check its keybinding list before you decide.
- **Your Warp data.** What you keep in Warp Drive or share from Warp is tied to your Warp account, so copy out what you need before you uninstall. The privacy page explains how to delete the account.

## FAQ

### Is Warp free?

Warp has a free plan, and its docs say telemetry must be on to use AI features there. Check its pricing page for current plan limits, since they change.

### Do I need an account to use Warp?

No. The quickstart says you can skip sign-up and start using Warp immediately.

### Is there an open source alternative to Warp?

Warp's own client is open source under AGPL v3 and MIT. Among the alternatives, Ghostty and Kaku are MIT, Kitty is GPL-3.0, and Alacritty is Apache-2.0.

### Which terminal is best for Claude Code if I leave Warp?

Any of them runs it. What differs is multi-line input, notifications and restoring sessions, which [the best terminal for Claude Code](/blog/best-terminal-for-claude-code) compares.
