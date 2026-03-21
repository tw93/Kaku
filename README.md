<div align="center">
  <h1>Kaku</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

<p align="center">
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</p>

<p align="center">
  <img src="assets/kaku.jpeg" alt="Kaku Screenshot" width="1000" />
  <br/>
  Kaku is a deeply customized fork of <a href="https://github.com/wez/wezterm">WezTerm</a>, designed for an out-of-the-box experience.
</p>

## Features

- **Zero Config**: Defaults with JetBrains Mono, macOS font rendering, and low-res font sizing.
- **Theme-Aware Experience**: Auto-switches between dark and light modes with macOS, with tuned selection colors, font weight, and practical color overrides support.
- **Curated Shell Suite**: Built-in zsh plugins with optional CLI tools for prompt, diff, and navigation workflows.
- **Fast & Lightweight**: 40% smaller binary, instant startup, lazy loading, stripped-down GPU-accelerated core.
- **WezTerm-Compatible Config**: Use WezTerm's Lua config directly with full API compatibility and no migration.

## Quick Start

1. [Download Kaku DMG](https://github.com/tw93/Kaku/releases/latest) & Drag to Applications
2. Or install with Homebrew: `brew install tw93/tap/kakuku`
3. Open Kaku. The app is notarized by Apple, so it opens without security warnings
4. On first launch, Kaku will automatically set up your shell environment

## Usage Guide

Kaku comes with intuitive macOS-native shortcuts:

| Action | Shortcut |
| :--- | :--- |
| Toggle Global Window | `Cmd + Opt + Ctrl + K` |
| New Tab | `Cmd + T` |
| New Window | `Cmd + N` |
| Close Tab/Pane | `Cmd + W` |
| Navigate Tabs | `Cmd + Shift + [`, `Cmd + Shift + ]` or `Cmd + 1-9` |
| Navigate Panes | `Cmd + Opt + Arrows` |
| Split Pane Vertical | `Cmd + D` |
| Split Pane Horizontal | `Cmd + Shift + D` |
| Toggle Split Direction | `Cmd + Shift + S` |
| Zoom/Unzoom Pane | `Cmd + Shift + Enter` |
| Resize Pane | `Cmd + Ctrl + Arrows` |
| Open Settings Panel | `Cmd + ,` |
| Reopen Closed Tab | `Cmd + Shift + T` |
| Rename Tab | Double-click on tab title |
| Clear Screen | `Cmd + K` |
| Doctor Panel | `Ctrl + Shift + L` |
| AI Panel | `Cmd + Shift + A` |
| Kaku Assistant Apply Suggestion | `Cmd + Shift + E` |
| Open Lazygit | `Cmd + Shift + G` |
| Yazi File Manager | `Cmd + Shift + Y` or `y` |
| Font Size | `Cmd + +`, `Cmd + -`, `Cmd + 0` |
| Smart Jump | `z <dir>` |
| Smart Select | `z -l <dir>` |
| Recent Dirs | `z -t` |

### Intuitive Interactions

- **Visual Bell**: A bell prefix appears on inactive tabs when background tasks finish. Optional Dock badge is controlled by `bell_dock_badge` (off by default), and the tab prefix can be toggled with `bell_tab_indicator`.
- **File Path Hyperlinks**: Relative and home-based file paths in terminal output become clickable links.
- **Active Pane**: A subtle dot highlights the currently focused pane during split-screen workflows.
- **Global Hotkey**: Press `Cmd + Opt + Ctrl + K` anytime to float Kaku over your current workspace.
- **Copy on Select**: Highlighting any text automatically copies it to your clipboard with a confirmation toast.
- **Zoom Window**: Double-click the title bar or tab bar empty space to safely zoom or unzoom the window.
- **Finder Integration**: Right-click folders in macOS Finder and deploy Kaku via Services, or drop multiple files directly onto the Kaku Dock icon.
- **History Peek**: Scroll up while inside full-screen apps like `less` or `vim` to lift the screen and peek at your primary shell history without exiting.
- **Tab Reordering**: Drag and drop tabs to reorder them in the tab bar.
- **Close Confirmation**: Closing a tab or pane with active processes prompts for confirmation to prevent accidental data loss.
- **Pane Input Broadcast**: Synchronize typing across multiple panes simultaneously. Enable broadcast mode from the pane context menu or via the `kaku` command. Overlay input is never broadcast by mistake.

## Configuration

Kaku comes with a carefully curated shell stack for immediate productivity, so you can focus on AI coding without opening vscode:

Built-in shell stack defaults are:

- **z**: A smarter cd command that learns your most used directories for instant navigation.
- **zsh-completions**: Extended command and subcommand completion definitions.
- **Syntax Highlighting**: Real-time command validation and coloring.
- **Autosuggestions**: Intelligent, history-based completions similar to Fish shell.
- **Fish-friendly integration**: `kaku init` now can provision `~/.config/kaku/fish/kaku.fish` for fish users, and `kaku doctor` verifies both zsh/fish integration paths.

Optional CLI tools installed via Homebrew during `kaku init`:

- **Starship**: A fast, customizable prompt showing the current directory, git branch, and package versions.
- **Delta**: A syntax-highlighting pager for git, diff, and grep output.
- **Lazygit**: A terminal UI for fast, visual Git workflows without leaving the shell.
- **Yazi**: A terminal file manager. Use `y` to launch it and sync the shell directory on exit.

Kaku uses `~/.config/kaku/kaku.lua` for configuration, fully compatible with WezTerm's Lua API, with built-in defaults at `Kaku.app/Contents/Resources/kaku.lua` as fallback.

Run `kaku config` or press `Cmd + ,` to open the Settings TUI and edit common options (font, theme, opacity, bells, scrollbars, Kaku Assistant) without manually editing config files. The settings panel uses grouped sections with a pinned footer showing contextual key hints.

To enable rounded scrollbars, open `kaku config` and toggle the scrollbar style option.

If you already use your own Zsh completion workflow such as `fzf-tab`, Kaku's Smart Tab only applies inside Kaku sessions by default. You can also disable it explicitly before loading Kaku shell integration:

```zsh
export KAKU_SMART_TAB_DISABLE=1
[[ ":$PATH:" != *":$HOME/.config/kaku/zsh/bin:"* ]] && export PATH="$HOME/.config/kaku/zsh/bin:$PATH"
[[ -f "$HOME/.config/kaku/zsh/kaku.zsh" ]] && source "$HOME/.config/kaku/zsh/kaku.zsh"
```

```fish
set -gx KAKU_SMART_TAB_DISABLE 1
if not contains -- "$HOME/.config/kaku/fish/bin" $PATH
  set -gx PATH "$HOME/.config/kaku/fish/bin" $PATH
end
if test -f "$HOME/.config/kaku/fish/kaku.fish"
  source "$HOME/.config/kaku/fish/kaku.fish"
fi
```

You can also remap true-color output from specific apps to keep theme consistency:

```lua
config.color_overrides = {
  ['#6E6E6E'] = '#3A3942',
}
```

Run `kaku` in your terminal to see all available commands such as `kaku ai`, `kaku config`, `kaku doctor`, `kaku update`, and `kaku reset`.

## Kaku AI

Kaku includes a built-in assistant for command-line error recovery and a single AI settings page for coding tools.

- **Kaku Assistant**: Automatically analyzes failed commands and prepares a safe command suggestion. Enable or disable it from `kaku config`.
- **AI Tools Config**: Manage settings for tools like Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, Factory Droid, and OpenClaw.

Open AI settings with `kaku ai` to configure your external AI tools and edit **Kaku Assistant** details after it is enabled.
For enterprise gateway/proxy headers, edit `~/.config/kaku/assistant.toml` and set `custom_headers` there.

Tip: DeepSeek-V3.2 is a great low-cost option to start with for everyday AI coding tasks.

When Kaku Assistant has a suggestion ready after a command error, press `Cmd + Shift + E` to apply it.

## Why Kaku?

I heavily rely on the CLI for both work and personal projects. Tools I've built, like [Mole](https://github.com/tw93/mole) and [Pake](https://github.com/tw93/pake), reflect this.

I used Alacritty for years and learned to value speed and simplicity. As my workflow shifted toward AI-assisted coding, I wanted stronger tab and pane ergonomics. I also explored Kitty, Ghostty, Warp, and iTerm2. Each is strong in different areas, but I still wanted a setup that matched my own balance of performance, defaults, and control.

WezTerm is robust and highly hackable, and I am grateful for its engine and ecosystem. Kaku builds on that foundation with practical defaults for day one use, while keeping full Lua-based customization and a fast, lightweight feel.

So I built Kaku to be that environment: fast, polished, and ready to work.

### Performance

| Metric | Upstream | Kaku | Methodology |
| :--- | :--- | :--- | :--- |
| **Executable Size** | ~67 MB | ~40 MB | Aggressive symbol stripping & feature pruning |
| **Resources Volume** | ~100 MB | ~80 MB | Asset optimization & lazy-loaded assets |
| **Launch Latency** | Standard | Instant | Just-in-time initialization |
| **Shell Bootstrap** | ~200ms | ~100ms | Optimized environment provisioning |

Achieved through aggressive stripping of unused features, lazy loading of color schemes, and shell optimizations.

## FAQ

1. **Is there a Windows or Linux version?**

   Not at the moment. Kaku is currently macOS-only while we focus on polishing the macOS experience. Windows and Linux versions may come later once the macOS version is mature.

2. **Can Kaku use transparent windows on macOS?**

   Yes. You can set `window_background_opacity` and optionally `macos_window_background_blur` in `~/.config/kaku/kaku.lua`. Transparent mode now keeps top/right/bottom padding regions visually consistent to avoid transparent gaps.

3. **How do I turn off copy on select?**

   Kaku enables copy on select by default; to disable automatic clipboard copy and copy toast after selection, add `config.copy_on_select = false` to `~/.config/kaku/kaku.lua`.

4. **Can I control working directory inheritance separately for new window, tab, and split?**

   Yes. Use these options in `~/.config/kaku/kaku.lua`:
   `config.window_inherit_working_directory`
   `config.tab_inherit_working_directory`
   `config.split_pane_inherit_working_directory`
   All are enabled by default.

5. **The `kaku` command is missing. How can I recover it and troubleshoot with Kaku Doctor?**

   Open Kaku Doctor from the Shell menu first. This diagnostic path can still run when your shell command entry is missing and will tell you what to repair.

   Then run this command in a terminal to restore the shell entry:

   ```bash
   /Applications/Kaku.app/Contents/MacOS/kaku init --update-only
   exec zsh -l
   ```

   Finally run `kaku doctor` in your terminal to verify everything is healthy.

6. **How can I use Kaku's CLI capabilities (like `split-pane`) from other scripts or tools?**

   Kaku exposes a powerful CLI for interacting with its multiplexer. For example, to split the current pane, run:

   ```bash
   kaku cli split-pane
   ```

   To split it and run a specific command instead of your default shell:

   ```bash
   kaku cli split-pane -- bash -c "echo Hello"
   ```

   You can explore all available CLI commands by running `kaku cli --help` or specifically `kaku cli split-pane --help`. This is very useful when integrating Kaku with workflows or AI tools.

## Contributors

Big thanks to all contributors who helped build Kaku. Go follow them! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## Support

- If Kaku helped you, star the repo or [share it](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20A%20fast%20terminal%20built%20for%20AI%20coding.) with friends.
- Got ideas or found bugs? Open an issue/PR or check [CONTRIBUTING.md](CONTRIBUTING.md) for details.
- Like Kaku? <a href="https://miaoyan.app/cats.html?name=Kaku" target="_blank">Buy Tw93 a Coke</a> to support the project! 🥤 Supporters below.

<a href="https://miaoyan.app/cats.html?name=Kaku"><img src="https://miaoyan.app/assets/sponsors.svg" width="1000" loading="lazy" /></a>

## License

MIT License, feel free to enjoy and participate in open source.
