# CLI Reference

What the kaku command does, from AI setup to diagnostics and updates.

Run `kaku` with no arguments to pick from the main commands.

## kaku ai

Opens the AI settings panel, where you configure Kaku Assistant and external coding tools such as Claude Code, Codex, Gemini CLI, Copilot CLI, and Kimi Code.

```bash
kaku ai
```

## kaku chat

Starts Kaku's AI chat from any shell. It's an alias for the bundled `k` helper, so it works even when `k` isn't on your PATH.

```bash
kaku chat                 # open interactive chat
kaku chat "explain this"  # one-shot prompt
```

It reads `~/.config/kaku/assistant.toml`, shares conversations and memory with the `Cmd + L` overlay, and in interactive mode supports `/new`, `/resume`, `/clear`, `/status`, `/memory`, and `/exit`.

## kaku config

Opens the settings TUI for common options and Lua overrides, creating `~/.config/kaku/kaku.lua` if it doesn't exist yet. Inside Kaku, `Cmd + ,` opens the same screen.

```bash
kaku config
```

## kaku doctor

Checks the app bundle, PATH, and shell integration. Run it first after installing, or whenever something seems broken.

```bash
kaku doctor
```

## kaku update

Downloads and installs the latest Kaku release.

```bash
kaku update
```

`kaku --version` prints the installed version. To check the GUI binary without opening a window, run `/Applications/Kaku.app/Contents/MacOS/kaku-gui --version`.

## kaku reset

Removes the shell and tmux integration, git delta defaults, and some of the state that Kaku manages, plus the managed theme blocks in `~/.config/kaku/kaku.lua`. Lua you wrote outside those blocks is kept. Use it with care, and run `kaku init` again to get the shell integration back.

```bash
kaku reset
```

## kaku init

Sets up or refreshes shell integration for zsh, fish, or both, creating `~/.config/kaku/zsh/kaku.zsh` and optionally `~/.config/kaku/fish/kaku.fish`. In an interactive shell it asks before installing missing optional tools such as Starship, Delta, Lazygit, and Yazi through Homebrew.

```bash
kaku init
```

If the `kaku` command disappears from your shell, refresh the integration without the tool prompts:

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

Drives the Kaku multiplexer from scripts and external tools, such as an AI tool or shell script that needs to open panes or tabs.

```bash
kaku cli split-pane                          # split current pane
kaku cli split-pane -- bash -c "echo hello"  # split and run a command
kaku cli --help                              # list all subcommands
kaku cli split-pane --help                   # help for a specific subcommand
```

---

Source: https://kaku.fun/docs/cli
Site index for LLMs: https://kaku.fun/llms.txt
