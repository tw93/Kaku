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

- **Zero Config**: Polished defaults with JetBrains Mono, opencode theme, optimized macOS font rendering, smooth animations.
- **Built-in Shell Suite**: Comes pre-loaded with Starship, z, Delta, syntax highlighting, autosuggestions, and autocompletions.
- **Fast & Lightweight**: 40% smaller binary, instant startup, lazy loading, stripped-down GPU-accelerated core.
- **Lua Scripting**: Retains the full power of WezTerm's Lua engine for infinite customization.

## Quick Start

1. [Download Kaku DMG](https://github.com/tw93/Kaku/releases/latest) & Drag to Applications
2. Open Kaku. If macOS blocks the app, go to System Settings → Privacy & Security → click "Open Anyway"
3. On first launch, Kaku will automatically set up your shell environment

## Usage Guide

Kaku comes with intuitive macOS-native shortcuts:

| Action | Shortcut |
| :--- | :--- |
| New Tab | `Cmd + T` |
| New Window | `Cmd + N` |
| Split Pane Vertical | `Cmd + D` |
| Split Pane Horizontal | `Cmd + Shift + D` |
| Zoom/Unzoom Pane | `Cmd + Shift + Enter` |
| Resize Pane | `Cmd + Ctrl + Arrows` |
| Close Tab/Pane | `Cmd + W` |
| Navigate Tabs | `Cmd + [`, `Cmd + ]` or `Cmd + 1-9` |
| Navigate Panes | `Cmd + Opt + Arrows` |
| Clear Screen | `Cmd + R` |
| Font Size | `Cmd + +`, `Cmd + -`, `Cmd + 0` |
| Smart Jump | `z <dir>` |
| Smart Select | `z -l <dir>` |
| Recent Dirs | `z -t` |

## Configuration

Kaku comes with a carefully curated suite of CLI tools, pre-configured for immediate productivity:

- **Starship**: A fast, customizable prompt showing git status, package versions, and execution time.
- **z**: A smarter cd command that learns your most used directories for instant navigation.
- **Delta**: A syntax-highlighting pager for git, diff, and grep output.
- **zsh-completions**: Extended command and subcommand completion definitions.
- **Syntax Highlighting**: Real-time command validation and coloring.
- **Autosuggestions**: Intelligent, history-based completions similar to Fish shell.

### Customization

Kaku is fully configurable via standard Lua scripts and is 100% compatible with WezTerm configuration. It loads configuration files in the following priority order:

1. **Explicit Override**: `KAKU_CONFIG_FILE=/path/to/kaku.lua` (if set).
2. **User Config**: `~/.config/kaku/kaku.lua`.
3. **Bundled Fallback**: `Kaku.app/Contents/Resources/kaku.lua`.

## Why Kaku?

I heavily rely on the CLI for both work and personal projects. Tools I've built, like [Mole](https://github.com/tw93/mole) and [Pake](https://github.com/tw93/pake), reflect this.

I used Alacritty for years, but its lack of multi-tab support became cumbersome for AI-assisted coding. Kitty has some aesthetic and positioning quirks I couldn't get past. Ghostty shows promise but font rendering needs work. Warp feels bloated and requires a login. iTerm2 is reliable but showing its age and harder to deeply customize.

WezTerm is robust and hackable, and I am grateful for its powerful engine. However, I wanted an environment that was ready immediately, without extensive configuration—and something significantly faster and lighter.

So I built Kaku to be that environment: fast, polished, and ready to work.

### Performance

| Metric | Upstream | Kaku | Methodology |
| :--- | :--- | :--- | :--- |
| **Executable Size** | ~67 MB | ~40 MB | Aggressive symbol stripping & feature pruning |
| **Resources Volume** | ~100 MB | ~80 MB | Asset optimization & lazy-loaded assets |
| **Launch Latency** | Standard | Instant | Just-in-time initialization |
| **Shell Bootstrap** | ~200ms | ~100ms | Optimized environment provisioning |

Achieved through aggressive stripping of unused features, lazy loading of color schemes, and shell optimizations.

## Support

- If Kaku helped you, star the repo or [share it](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20A%20fast%20terminal%20built%20for%20AI%20coding.) with friends.
- Got ideas or found bugs? Open an issue/PR or check [CONTRIBUTING.md](CONTRIBUTING.md) for details.
- Like Kaku? <a href="https://miaoyan.app/cats.html?name=Kaku" target="_blank">Buy Tw93 a Coke</a> to support the project!

## License

MIT License, feel free to enjoy and participate in open source.
