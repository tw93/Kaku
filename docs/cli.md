# CLI Reference

Run `kaku` in your terminal to see all available commands.

## kaku ai

Open the AI settings panel inside Kaku. Configure external coding tools (Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, etc.) and Kaku Assistant.

```bash
kaku ai
```

## kaku theme

Inspect Kaku's current theme state and the terminal-tool coordination boundary.
Inspecting is read-only; writes require an explicit tool list:

```bash
kaku theme current
kaku theme palette --format json
kaku theme status
```

`status` distinguishes write-capable adapters, Kaku's built-in Yazi
coordination, Codex ANSI inheritance, and Atuin's informational-only status.
Atuin is informational-only in this release and inherits the terminal palette;
Kaku does not write Atuin configuration.
OpenCode uses its documented TUI config (`OPENCODE_TUI_CONFIG` when set, or
`$XDG_CONFIG_HOME/opencode/tui.json`) and a paired light/dark custom theme.
Existing OpenCode selections or theme files require `--take-over opencode`.
Fish uses a native dual-variant `Kaku.theme` plus a small interactive startup
snippet; existing Fish theme assets require `--take-over fish`.
fzf receives managed light/dark option files for new invocations, Starship
receives Kaku palette tables in its active TOML, and btop receives a managed
`Kaku.theme` plus `color_theme = "Kaku"`.

```bash
kaku theme preview --tools atuin --format json
kaku theme apply --tools opencode --take-over opencode
kaku theme apply --tools fish --take-over fish
kaku theme apply --tools fzf --take-over fzf
kaku theme apply --tools starship --take-over starship
kaku theme apply --tools btop --take-over btop
kaku theme remove --tools atuin --format json
```

### Visual smoke test

To inspect the result in a real Kaku window, apply into a temporary XDG
directory, then start a fresh shell with that directory exported:

```bash
tmpdir=$(mktemp -d)
export XDG_CONFIG_HOME="$tmpdir/config"
export XDG_STATE_HOME="$tmpdir/state"
export CLAUDE_CONFIG_DIR="$tmpdir/claude"
export OPENCODE_TUI_CONFIG="$tmpdir/config/opencode/tui.json"
export STARSHIP_CONFIG="$tmpdir/config/starship.toml"

kaku theme apply --tools fish,fzf,starship,btop,claude,opencode --format json
```

Open a new Kaku tab in that same shell and run:

```bash
fish -l                         # prompt and autosuggestions
printf '\033[31mred\033[32m green\033[34m blue\033[0m\n'
printf 'one\ntwo\nthree\n' | FZF_DEFAULT_OPTS_FILE="$XDG_CONFIG_HOME/fzf/kaku-dark.opts" fzf
starship prompt                 # Starship prompt colors
btop --config "$XDG_CONFIG_HOME/btop/btop.conf"
claude                          # Claude Code native theme
opencode                        # OpenCode TUI theme
```

For light-mode verification, switch macOS/Kaku to the light appearance, open
another tab, and repeat the commands. `fzf` and Fish choose their light/dark
files at shell startup; Claude and OpenCode choose the corresponding native
theme files on their next launch. Codex and Atuin are intentionally read-only:
their colors should follow Kaku's ANSI palette without a config write.

Adapters that only inherit the terminal palette (such as Codex) and built-in
integrations (such as Yazi) return informational results and never rewrite
third-party files.

## kaku chat

Start Kaku's standalone AI chat from any shell. This is a discoverable alias for
the bundled `k` helper, so it works even when `k` is not on your PATH.

```bash
kaku chat                 # open interactive chat
kaku chat "explain this"  # one-shot prompt
```

The chat uses `~/.config/kaku/assistant.toml`, shares the same conversation and
memory files as the `Cmd + L` overlay, and supports `/new`, `/resume`, `/clear`,
`/status`, `/memory`, and `/exit` in interactive mode.

## kaku config

Open the Kaku configuration TUI for common settings and Lua overrides. It
ensures `~/.config/kaku/kaku.lua` exists and is also accessible from the
settings panel with `Cmd + ,`.

```bash
kaku config
```

## kaku doctor

Run diagnostics and verify that Kaku's shell integration, PATH entries, and optional tool installations are healthy. Use this first if something feels broken.

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

Remove Kaku-managed shell and tmux integration, Kaku-managed git delta defaults,
selected Kaku state, and managed theme blocks in `~/.config/kaku/kaku.lua`.
User-authored Lua outside managed blocks is preserved. Use with caution and run
`kaku init` again if you want shell integration back.

```bash
kaku reset
kaku reset --shell fish # use fish for restart and restore guidance
```

## kaku init

Set up Kaku's shell integration for zsh or fish. When both shells are installed,
an interactive run asks which one to configure. Use `--shell` to make the choice
explicit in scripts or when `$SHELL` does not match your daily shell. Also
installs optional CLI tools (Starship, Delta, Lazygit, Yazi) via Homebrew.

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
