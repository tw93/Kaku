# CLI Reference

Open AI settings, chat, config, diagnostics, update, and multiplexer commands from the shell.

Run `kaku` in your terminal to see all available commands.

## kaku ai

Open the AI settings panel inside Kaku. Configure external coding tools (Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, etc.) and Kaku Assistant.

```bash
kaku ai
```

## kaku chat

Start Kaku's standalone AI chat from any shell. This is a discoverable alias for the bundled `k` helper, so it works even when `k` is not on your PATH.

```bash
kaku chat                 # open interactive chat
kaku chat "explain this"  # one-shot prompt
```

The chat uses `~/.config/kaku/assistant.toml`, shares the same conversation and memory files as the `Cmd + L` overlay, and supports `/new`, `/resume`, `/clear`, `/status`, `/memory`, and `/exit` in interactive mode.

## kaku config

Open the Kaku configuration TUI for common settings and Lua overrides. It ensures `~/.config/kaku/kaku.lua` exists and is also accessible from the settings panel with `Cmd + ,`.

```bash
kaku config
```

## kaku doctor

Run diagnostics and verify that Kaku's app bundle, shell integration, PATH entries, and optional tools are healthy. Use this first after installation or when something feels broken.

```bash
kaku doctor
```

## kaku update

Check for and install the latest Kaku release.

```bash
kaku update
```

Check the installed version with `kaku --version`. The bundled GUI executable also supports `/Applications/Kaku.app/Contents/MacOS/kaku-gui --version` without opening a window.

## kaku reset

Remove Kaku-managed shell and tmux integration, Kaku-managed git delta defaults, selected Kaku state, and managed theme blocks in `~/.config/kaku/kaku.lua`. User-authored Lua outside managed blocks is preserved. Use with caution and run `kaku init` again if you want shell integration back.

```bash
kaku reset
```

## kaku init

Set up or refresh Kaku's shell integration for zsh and/or fish. Creates `~/.config/kaku/zsh/kaku.zsh` and optionally `~/.config/kaku/fish/kaku.fish`. In an interactive shell, it asks before installing missing optional CLI tools such as Starship, Delta, Lazygit, and Yazi via Homebrew.

```bash
kaku init
```

If the `kaku` command goes missing from your shell, refresh integration without optional tool prompts:

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

Interact with the Kaku multiplexer from scripts and external tools.

```bash
kaku cli split-pane                          # split current pane
kaku cli split-pane -- bash -c "echo hello"  # split and run a command
kaku cli --help                              # list all subcommands
kaku cli split-pane --help                   # help for a specific subcommand
```

Useful for integrating Kaku with AI tools or shell scripts that need to open panes or tabs programmatically.

---

Source: https://kaku.fun/docs/cli
Site index for LLMs: https://kaku.fun/llms.txt
