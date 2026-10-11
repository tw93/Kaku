<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>An AI-friendly Mac terminal with sensible defaults, ready out of the box.</em></p>
  <p>English · <a href="README_CN.md">中文</a> · <a href="README_TW.md">繁體</a> · <a href="README_JA.md">日本語</a> · <a href="README_KR.md">한국어</a> · <a href="README_DE.md">Deutsch</a> · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">Website</a> ·
    <a href="https://kaku.fun/docs/">Docs</a> ·
    <a href="https://kaku.fun/compare">Compare</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">Download</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku Screenshot" width="1000" />
</p>

## Why

Kaku (書く, かく) means "to write" in Japanese. It is a macOS terminal based on WezTerm, with fonts, themes, shell integration, and Mac shortcuts already configured, while preserving full Lua customization.

As the coding foundation of the trilogy, Kaku works alongside [Waza](https://github.com/tw93/Waza) (技), built around engineering habits, and [Kami](https://github.com/tw93/Kami) (紙), built for shipping documents, so writing code, building habits, and shipping documents flow from one to the next.

## Quick Start

Download the [Kaku DMG](https://github.com/tw93/Kaku/releases/latest), open it, and drag Kaku into Applications. Or install with Homebrew:

```bash
brew install --cask kaku
```

Open Kaku to set up shell integration, install missing optional tools with `kaku init`, and check your installed version with `kaku --version`.

## Features

- **Ready to use**: JetBrains Mono, automatic dark and light themes, copy on select, and familiar Mac shortcuts
- **Tabs and panes**: Split your workspace, jump between tabs with Tab Navigator, and restore windows, panes, and working directories when you reopen Kaku
- **Right-click menu**: Paste, search, open AI chat, split or close the clicked pane, and open or close tabs without remembering shortcuts
- **Clickable links**: `Cmd + Click` opens URLs and file paths, and URLs automatically wrapped by the terminal keep their full address
- **AI-friendly**: Use your coding tools alongside an optional assistant for command suggestions and chat
- **Shell tools**: Built-in zsh completion, syntax highlighting, and directory jumping, with shortcuts for optional Lazygit and Yazi installations
- **Lua configuration**: Customize fonts, themes, shortcuts, and terminal behavior using WezTerm's Lua configuration system

## Usage Guide

| Action | Shortcut |
| :--- | :--- |
| New Tab | `Cmd + T` |
| New Window | `Cmd + N` |
| Close Tab/Pane | `Cmd + W` |
| Navigate Tabs | `Cmd + Shift + [` / `]` or `Cmd + 1-9` |
| Navigate Panes | `Cmd + Opt + Arrows` |
| Split Pane Vertical | `Cmd + D` |
| Split Pane Horizontal | `Cmd + Shift + D` |
| Open Settings Panel | `Cmd + ,` |
| AI Panel | `Cmd + Shift + A` |
| AI Chat | `Cmd + L` |
| Apply AI Suggestion | `Cmd + Shift + E` |
| Open Lazygit | `Cmd + Shift + G` |
| Yazi File Manager | `Cmd + Shift + Y` or `y` |
| Clear Screen | `Cmd + K` |

## Kaku AI

Configure your own AI service with `kaku ai` to use the built-in assistant. Kaku does not provide or relay the AI service.

- **Command suggestions**: When a command fails, the configured assistant can suggest a fix, and `Cmd + Shift + E` pastes it at the prompt for review
- **Natural language to command**: Type `# <description>` at the prompt and press Enter, and the assistant places the generated command at the prompt for you to review and run
- **Chat**: Press `Cmd + L` to discuss terminal output or work with project files and tools, or run `kaku chat` from another shell to access the same conversation store
- **AI Tools Config**: Manage settings for Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, and more

For authentication, models, API Mode, and tool settings, see the [AI assistant docs](docs/features.md).

## FAQ

- **Is there a Windows or Linux version?** Not currently, Kaku is macOS-only
- **How is Kaku different from iTerm2, Warp, Ghostty, or WezTerm?** See [kaku.fun/compare](https://kaku.fun/compare)
- **Can I use transparent windows?** Yes, set `config.window_background_opacity` in `~/.config/kaku/kaku.lua`
- **The `kaku` command is missing.** Run `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`, then `kaku doctor`

## Docs

- [Keybindings](docs/keybindings.md) - full shortcut reference
- [Features](docs/features.md) - AI assistant, lazygit, yazi, remote files, shell suite
- [Configuration](docs/configuration.md) - themes, fonts, custom keybindings, Lua API
- [CLI Reference](docs/cli.md) - `kaku ai`, `kaku config`, `kaku doctor`, and more
- [FAQ](docs/faq.md) - common questions and troubleshooting

## Background

I heavily rely on the CLI for both work and personal projects. Tools I've built, like [Mole](https://github.com/tw93/mole) and [Pake](https://github.com/tw93/pake), reflect this.

I used Alacritty for years and learned to value speed and simplicity. As my workflow shifted toward AI-assisted coding, I wanted stronger tab and pane ergonomics. I also explored Kitty, Ghostty, Warp, and iTerm2. Each is strong in different areas, but I still wanted a setup that matched my own balance of performance, defaults, and control.

WezTerm is robust and highly hackable, and I am grateful for its engine and ecosystem. So I built Kaku to be that environment: fast, polished, and ready to work.

## Contributors

Big thanks to all contributors who helped build Kaku. Go follow them! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## Support

- The most direct way to support me is getting [Mole for Mac](https://mole.fit), my paid Mac cleanup app
- If Kaku helped you, give it a star, [share it](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.), or open an issue or PR
- I have two cats, TangYuan and Coke, and if Kaku has been helpful to you, feel free to treat them to <a href="https://cats.tw93.fun?name=Kaku" target="_blank">canned food 🥩</a>

<details>
<summary>These lovely people already did 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## License

MIT License. Please feel free to use and contribute to the development. Attribution for WezTerm and the bundled fonts is in [NOTICE.md](NOTICE.md).
