# CLI Reference

What the kaku command does, from AI setup to diagnostics and updates.

Run `kaku` with no arguments to pick from the main commands. `kaku start` opens in the running Kaku instance, and `kaku start --new-tab` adds a tab to the current window.

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

Checks the app bundle, PATH, and shell integration. It saves a redacted diagnostic bundle (.zip) on your Mac to attach to bug reports. Run `kaku doctor --fix` to apply safe automatic fixes.

```bash
kaku doctor
kaku doctor --fix
```

## kaku remote

Use this Mac's terminals from a browser on another computer or a phone, through Kaku's relay. Remote access is off until you turn it on.

```bash
kaku remote               # turn remote access on (asks first) and print a pairing link
kaku remote devices       # list paired browsers
kaku remote revoke <id>   # remove a browser; its open sessions end within seconds
kaku remote off           # turn remote access off
kaku remote --reset       # new identity, every paired browser removed
```

Open the link in the browser you want to pair, check that the page and `kaku remote` show the same six-digit code, and press Enter in the terminal to allow it, or Ctrl+C to refuse. A link works once, for 10 minutes, so run `kaku remote` again for each browser. After pairing, bookmark the page: the browser keeps its own key and comes back without a new link. What you type and see is encrypted between the browser and Kaku, and the relay only forwards it. Kaku posts a notification when a paired browser connects again after 30 minutes or more away.

## kaku update

Downloads and installs the latest Kaku release.

```bash
kaku update
```

`kaku --version` prints the installed version. To check the GUI binary without opening a window, run `/Applications/Kaku.app/Contents/MacOS/kaku-gui --version`.

## kaku reset

Removes the shell and tmux integration, git delta defaults, and some of the state that Kaku manages, plus the managed theme blocks in `~/.config/kaku/kaku.lua`. It also deletes Kaku AI memory (`soul/` and `ai_chat_memory.md`), the AI chat onboarding flag, and the `backups/` folder in `~/.config/kaku`. Lua you wrote outside those blocks is kept. Use it with care, and run `kaku init` again to get the shell integration back.

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

Since Kaku 0.29.0, `kaku cli wait` and `kaku cli events` follow the state Claude Code (2.1.295 or later) and Codex (0.163 or later) report: `working`, `blocked` (waiting for you), `done`, `error` or `idle`.

```bash
kaku cli wait --pane-id 5 --for done,blocked,error --timeout 20m  # until pane 5 finishes, needs you or fails
kaku cli events --for blocked --once                              # stop at the first pane that needs you
kaku cli events --json                                            # one JSON line per state change
```

`wait` prints the state that ended it and exits 0, exits 2 on `--timeout`, and exits 1 when it cannot reach Kaku or the pane does not exist. Add `--next` to skip a `done` left from the agent's previous turn.

---

Source: https://kaku.fun/docs/cli
Site index for LLMs: https://kaku.fun/llms.txt
