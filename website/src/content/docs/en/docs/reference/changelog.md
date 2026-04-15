---
title: Changelog
description: Kaku's release history at a glance
---

The full, linkable release notes live on [GitHub Releases](https://github.com/tw93/Kaku/releases). The list below walks through every version in reverse chronological order so you can see how Kaku evolved.

## V0.9.0 Spark ✨ — 2026-04-04

AI that meets you at the command line.

- **Natural language to command**: type `# <description>` at the prompt, press Enter, Kaku asks the LLM and injects the resulting command back; works in zsh and fish, saved to shell history
- **Option + Click cursor movement**: click anywhere on the current line to move the cursor there, with correct handling of wide and multi-byte characters
- **Always on top**: pin any window above others via the Window menu
- **Traffic lights position**: new `traffic_lights` setting to customize the macOS window control buttons
- **Provider presets**: MiniMax added
- **Stability**: fixed Option+Click crash, divide-by-zero in split pane sizing, unwrap panic in mouse handling

## V0.8.0 Fish 🐟 — 2026-03-23

A first-class welcome for fish shell users.

- **Full fish shell support**: `kaku init` provisions Starship, the Yazi launcher, theme sync, and a conf.d entry for fish
- **Bell tab indicator**: background tabs show a bell prefix when a task finishes, with optional Dock badge
- **Remember last directory**: new tabs and windows restore your last working directory (toggleable in `kaku config`)
- **Update / Doctor in their own tab**: `kaku update` and `kaku doctor` open in a dedicated tab instead of blocking the current session
- **Basename-only tab titles**: new `tab_title_basename_only` option
- **Scrollback fixes**: viewport no longer jumps to the top during rapid output, and no more unexpected jumps while using Claude Code

## V0.7.1 Flow 🌊 — 2026-03-13

Polish across theming, settings, and the AI workflow.

- **Auto theme switching**: follows macOS light/dark, with improved transparency rendering and Yazi theme sync
- **Safer close flows**: tab and pane close confirmations, refreshed overlay styling, fixed title-bar double-click interfering with window drag
- **`kaku config` leveled up**: clearer grouped sections, a pinned footer with contextual key hints, more reliable parsing and reload
- **AI configuration**: `kaku ai` picks up Antigravity support, quota tracking, background loading, and more reliable OAuth refresh
- **Pane input broadcast**: synchronized typing across panes with safeguards against broadcasting overlay input
- **File & editor workflow**: better file-link opening, a remote files shortcut for SSH sessions, respect for `$EDITOR`
- **Rounded scrollbars**: optional, enable in `kaku config`

## V0.6.0 Clarity ☀️ — 2026-03-08

Light theme, AI usage visibility, and an interactive settings TUI — all in one release.

- **Light theme**: dynamic font weight, improved ANSI colors, Claude Code-specific color overrides
- **AI usage visibility**: the AI panel now surfaces usage summaries and remaining quota, adds Kimi Code support, Kimi usage tracking, and more reliable Claude OAuth token persistence
- **Interactive settings TUI**: `kaku config` becomes a richer interactive editor with save-on-exit and theme-aware updates
- **Tab workflow**: drag to reorder, double-click to rename inline, `Cmd + Shift + T` to reopen a closed tab, programmatic `kaku set-tab-title`
- **Path hyperlinks**: click file paths in terminal output to open them
- **Shell integration polish**: better first-run without Homebrew, fixed Starship right-prompt leakage after `Ctrl + C`, removed the forced `TERM=kaku` that broke SSH Delete key
- **macOS input fixes**: non-Latin IMEs no longer block `Cmd + alnum` shortcuts; dead keys and Turkish tilde input fixed
- **Memory**: lazy scrollback allocation, capped background image and gradient caches for stable long-session memory use

## V0.5.1 Kindness 🌴 — 2026-02-28

Follow-up fixes for V0.5.0.

- `y` launcher no longer clashes with existing `alias y=yarn`
- SSH sessions force `TERM=xterm-256color` so remotes without `kaku` terminfo render correctly
- Fixed `Cmd + Shift + ,` not passing through to tmux
- Fixed `kaku cli split-pane` panic, spurious error toast during AI analysis, and an update notification appearing even when `check_for_updates` was disabled

## V0.5.0 Yohaku 🪽 — 2026-02-27

The AI era starts here.

- **AI shell error recovery**: when a command fails Kaku sends it to an LLM and shows a fix suggestion inline; press `Cmd + Shift + E` to apply
- **Yazi built in**: press `Cmd + Shift + Y` or type `y` to open it, layout and theme configured on first run
- **Command palette**: `Cmd + Shift + P` for fuzzy command search with native text editing
- **Kaku Doctor**: `kaku doctor` interactively checks and fixes common setup issues
- **Global hotkey**: `Ctrl + Opt + Cmd + K` shows or hides Kaku from anywhere
- **Shell text editing**: `Cmd + A` select all, `Shift + arrows` extend selection, type to replace
- **Unified AI config**: `kaku ai` covers Kaku Assistant, Factory Droid, and opencode.jsonc
- **Faster startup**: Lua bytecode caching, deferred module loading, Fat LTO

## V0.4.0 AIIIIIII 🥂 — 2026-02-19

A first step into AI, paired with a graphics pipeline overhaul.

- **`kaku ai` command**: a unified home for configuring all your AI coding tools
- **WebGpu by default**: typical memory drops from ~200 MB to ~80 MB; falls back to OpenGL on failure
- **Lazygit built in**: `Cmd + Shift + G` launches it, with a contextual hint for git repos
- **Split UX**: active-pane marker, `split_thickness` option, `Cmd + Opt + arrows` to jump between panes
- **Smarter `Cmd + W`**: does the right thing across panes, tabs, and multi-window setups
- **SSH + 1Password**: remote sessions force `TERM=xterm-256color`; shell integration auto-detects the 1Password SSH agent and adds `IdentitiesOnly=yes`
- **Per-pane encoding**: switch between UTF-8, GBK, GB18030, Big5, EUC-KR, Shift-JIS independently per pane
- **URL scheme**: `kaku://open-tab?tty=<device>` lets external scripts jump straight to a specific pane

## V0.3.1 New Year 🎋 — 2026-02-16

- `Cmd + K` clears the screen (`Cmd + R` still supported)
- `Cmd + Shift + S` toggles split pane direction
- SSH sessions standardize on `xterm-256color` to avoid missing terminfo on remotes
- Fixed macOS dictation / voice input
- Tab key restored to show the completion list; Right Arrow accepts autosuggestions
- `kaku init` auto-creates `~/.config/kaku/kaku.lua` if missing

## V0.3.0 Happy 🥙 — 2026-02-16

System integration goes wide.

- **Smoother fullscreen**: stable transitions, correct padding when splitting or creating tabs, refined tab-bar visibility
- **SSH host in tab**: the tab shows the remote hostname when connected
- **Finder integration**: right-click any folder → `Open in Kaku`
- **Set as default terminal**: menu bar Kaku → Set as Default Terminal
- **Shell history peek**: scroll up in vim or tmux to view shell history without leaving
- **Image paste**: paste images from other apps into terminal apps; saved to temp and path auto-inserted
- **Selection autoscroll**: dragging past the viewport auto-scrolls so you can keep selecting
- **Toast notifications**: visual feedback for copy and config reload
- **Multi-file Dock drop**: drop multiple files on the Dock icon, each opens in its own tab
- **Bar cursor**: modern default, supports Vim-mode switching; window shadow disabled by default to reduce GPU usage

## V0.2.0 Craft 🍺 — 2026-02-13

From "it runs" to "it installs and feels great."

- **Apple notarized**: no more security warnings, works out of the box
- **Universal binary**: one DMG for both Apple Silicon and Intel
- **Homebrew support**: `brew install tw93/tap/kakuku`
- **Unified `kaku` CLI**: `init`, `update`, `reset`, `config`, and more
- **User config loading fix**: `~/.config/kaku/kaku.lua` is no longer overridden by defaults
- **Fullscreen time display**: a subtle clock in the bottom-right while fullscreen, nudging you to take a break
- **Git Delta tuning**: aligned theme, side-by-side diff by default, cleaner headers
- **Chinese path support**: tab titles show CJK characters correctly instead of URL-encoded strings
- **Persistent sessions**: `Cmd + W` hides the window when only one tab is left instead of quitting
- **Font zoom and window size persisted across restarts**
- **Menu bar**: command palette, settings, check for updates, native notifications
- **Built-in updater**: `kaku update` in the terminal or the menu bar item

## V0.1.1 Easy to use 🍤 — 2026-02-09

- **Kaku Theme**: signature high-contrast dark theme tuned for long Claude / Codex sessions
- Optimized macOS font rasterization (light hinting) for crisper Retina text
- First-run wizard can apply Kaku Theme automatically
- New debug overlay to troubleshoot shell integration and config
- `setup_zsh.sh` script ships common aliases and git shortcuts
- Tab bar supports path-based titles and visual indicators

## V0.1.0 Freshmen 🧝‍♀️ — 2026-02-08

Kaku's first release. Built on top of [WezTerm](https://github.com/wez/wezterm), deeply tuned for AI coding on macOS.

- GPU-accelerated rendering, deeply tuned for macOS
- Built-in shell suite: Starship prompt, z smart jumper, zsh syntax highlighting and autosuggestions
- Smart first-run wizard that detects your environment and safely backs up existing configs
- Native animations, intuitive shortcuts, split pane management, focus mode
- Universal binary (Apple Silicon + Intel), single lightweight DMG
