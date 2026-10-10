# CLI Reference

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

Open the Kaku configuration TUI for common settings and Lua overrides. It creates `~/.config/kaku/kaku.lua` if it is missing. Inside Kaku, `Cmd + ,` opens the same screen.

```bash
kaku config
```

## kaku doctor

Check the Kaku app bundle, PATH entries, and zsh or fish shell integration. Use this first if something feels broken.

Every run also writes a diagnostic bundle to `~/.local/share/kaku/diagnostics/Kaku-Diagnose.zip`: the report, logs, recent crash reports, a short sample of a running Kaku, and any GUI stalls Kaku recorded on its own. Home paths, the host name, MAC addresses, serial numbers and credential-looking values are replaced first, `assistant.toml` is never read, and nothing leaves your Mac. Attach the zip when you report an issue. If Kaku itself is frozen, run `kaku doctor` from Terminal.app.

```bash
kaku doctor
kaku doctor --shell fish       # check fish even when $SHELL points to zsh
kaku doctor --shell fish --fix # repair the selected integration
```

## kaku update

Check for and install the latest Kaku release.

```bash
kaku update
```

## kaku reset

Remove Kaku-managed shell and tmux integration, Kaku-managed git delta defaults, selected Kaku state, and managed theme blocks in `~/.config/kaku/kaku.lua`. It also deletes Kaku AI memory (`soul/` and `ai_chat_memory.md`), the AI chat onboarding flag, and the `backups/` folder in `~/.config/kaku`. User-authored Lua outside managed blocks is preserved. Use with caution and run `kaku init` again if you want shell integration back.

```bash
kaku reset
kaku reset --shell fish # use fish for restart and restore guidance
```

## kaku init

Set up Kaku's shell integration for zsh or fish. When both shells are installed, an interactive run asks which one to configure. Use `--shell` to make the choice explicit in scripts or when `$SHELL` does not match your daily shell. Also installs optional CLI tools (Starship, Delta, Lazygit, Yazi) via Homebrew.

```bash
kaku init
kaku init --shell fish
kaku init --shell zsh --update-only
```

If the `kaku` command goes missing from your shell, restore it with:

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
