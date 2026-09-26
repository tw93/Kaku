# Features

What Kaku does beyond a plain terminal, from right-click actions to the assistant and shell tools.

## Mouse actions

Right-click inside a pane to paste, search, open AI chat or the command palette, split in any direction, or close the clicked pane. Closing asks for confirmation if your settings say so, and a TUI that captures the mouse still gets the right-click itself.

`Cmd + Click` opens URLs and file paths. A URL that wraps at the terminal edge still opens in full, and text after a hard newline never gets pulled into the link.

## Kaku Assistant

Kaku Assistant suggests fixes for failed commands, turns descriptions into commands, and chats about terminal output and project files.

**Setup**

Run `kaku ai` to open the AI settings, enable Kaku Assistant, and fill in these fields:

| Field | Description |
| --- | --- |
| Auth Type | API key or Codex CLI login |
| Simple Model | `#` command generation, command fixes, and lightweight chat |
| Deep Model | Main `Cmd + L` / `k` chat and tool use |
| Base URL | OpenAI-compatible API root, such as `https://api.openai.com/v1` |
| API Key | Your provider's API key, when Auth Type is API key |

For a custom OpenAI-compatible service, keep Auth Type on API key, enter its Base URL, and type the model names yourself.

API-key connections also pick an **API Mode**, `chat_completions` or `responses`. In Responses mode you can turn on **Native Web Search** if the provider supports it.

With Codex auth, Kaku reads your existing Codex connection. **Follow Codex** uses that connection's model, or you can set Simple Model and Deep Model separately in Kaku.

## AI Chat Panel

Press `Cmd + L` to open the AI chat panel. Answers stream in as Markdown with highlighted code blocks, the panel can include terminal context, and with your approval it uses tools for project files, shell commands, web search, and memory. It also tells the model the project type, detected from files like `Cargo.toml` or `package.json` in the current directory.

When the Simple Model and Deep Model differ, `Shift + Tab` switches between them. When they are the same, `/model` lists every model the current provider returns, and your configured model always stays on the list. Type `/suggest` to have Kaku predict the next message you might send.

In a shell, `k` and `kaku chat` pick up the same conversations:

```bash
k "summarize the current project"
kaku chat
```

The shell version is simpler than the panel. It streams plain text and supports `/new`, `/resume`, `/clear`, `/status`, `/memory`, and `/exit`.

**Error recovery**

When a command exits with a non-zero status, Kaku Assistant sends the command, exit code, working directory, and git branch to the model and shows a suggested fix inline. Press `Cmd + Shift + E` to paste it. Dangerous commands such as `rm -rf` or `git reset --hard` are pasted but never run automatically.

It stays quiet for `Ctrl+C` exits, help flags, bare package manager calls, git pull conflicts, and non-shell foreground processes.

**Natural language to command**

Type `# <description>` at the prompt and press Enter. Kaku catches the line before the shell runs it, sends it to the model along with the current directory and git branch, and puts the resulting command back at the prompt for you to review and run.

```
# list all files modified in the last 7 days
# find and kill the process on port 3000
# compress the src folder excluding node_modules
```

`#` generation and automatic fixes use the same provider and credentials as `Cmd + L`, so a Codex or Copilot login works here too, not just an API key.

The `#` prefix works in zsh and fish. Your query stays on screen while the request runs. If the model can't come up with a safe command, it explains why instead. Dangerous commands are loaded but flagged for review, never run automatically.

**assistant.toml fields**

The config file is `~/.config/kaku/assistant.toml`:

| Field | Description |
| --- | --- |
| `enabled` | `true` to enable, `false` to disable |
| `api_key` | API key for your AI service |
| `model` | Simple Model, for `#` command generation, command fixes, and lightweight chat |
| `chat_model` | Deep Model, for main `Cmd + L` / `k` chat and tool use |
| `chat_model_choices` | Optional list of chat models for the panel's model picker, used instead of the provider's list |
| `auto_fix_ignored_exit_codes` | Optional exit codes that never trigger a fix suggestion, e.g. `[2]` |
| `base_url` | OpenAI-compatible API root URL |
| `custom_headers` | Extra HTTP headers for enterprise proxies, e.g. `["X-Customer-ID: your-id"]` |
| `web_search_provider` | Optional search backend: `brave`, `pipellm`, or `tavily` |
| `web_search_api_key` | API key for the selected search backend |
| `web_fetch_script` | Optional custom script that fetches a URL as Markdown |
| `chat_tools_enabled` | Set to `false` for chat services that don't support tool calling |
| `auth_type` | Auth mode: `api_key`, `codex`, or `copilot`. The settings panel offers the first two, `copilot` is set by hand |
| `memory_curator_model` | Optional cheaper model for background memory curation |

Older configs may still have `fast_model`. Kaku reads it as the Simple Model and folds it into `model` the next time you save the assistant settings.

## Window Snapshots

Kaku saves multi-tab and multi-pane window layouts when you close or hide a window. **Shell > Restore Previous Window** or `Cmd + Option + Shift + T` reopens the last one. If the snapshot file is missing or corrupted, Kaku just tells you there is no snapshot to restore.

## AppleScript

Kaku ships a minimal AppleScript dictionary, so it shows up in Script Editor and other automation tools. It is deliberately small and read-only apart from `quit`.

```applescript
tell application "Kaku"
  get name        -- "Kaku"
  get version     -- e.g. "0.20.0"
  get frontmost   -- true / false
  quit            -- optional `saving ask|yes|no`
end tell
```

To browse the full dictionary, choose File > Open Dictionary in Script Editor and pick `/Applications/Kaku.app`. There is no `do script` verb, so AppleScript cannot run shell commands through Kaku.

## Lazygit Integration

Press `Cmd + Shift + G` to open lazygit in the current pane. Kaku finds the binary on PATH or in the usual Homebrew locations.

When a git repo has uncommitted changes and you haven't used lazygit in that directory yet, Kaku shows a one-time hint.

Install lazygit with `brew install lazygit` or through `kaku init`.

## Yazi File Manager

Press `Cmd + Shift + Y` to open yazi in the current pane. The `y` shell wrapper also opens yazi and syncs the shell's working directory when you quit.

**Theme sync**: Kaku keeps `~/.config/yazi/theme.toml` matched to the active color scheme (Kaku Dark or Kaku Light), so yazi needs no theme setup.

Install yazi with `brew install yazi` or through `kaku init`.

## Remote Sessions

When a pane is connected to another machine, its tab shows an ssh glyph and the host name instead of a local path, so remote tabs stand out at a glance. In a split tab only the remote pane puts its host name in the title, so you can tell which side is local. Kaku recognizes `ssh`, `mosh`, `autossh`, and `et` sessions.

In a remote pane the AI chat panel turns off local file and shell tools, because the working directory belongs to the other host and running them here would hit same-named local paths. It also skips detecting the local project type. The panel answers from what is on screen and suggests commands for you to run on the remote host. `@cwd` is unavailable there and says so instead of attaching the wrong directory.

The bundled zsh, fish, and bash integrations wrap `ssh` so it falls back to `xterm-256color` when the remote host has no `kaku` terminfo entry, and `mosh` gets the same fallback. If you define your own `ssh` function, Kaku leaves it alone.

## Remote Files

Press `Cmd + Shift + R` to mount the current SSH session's remote filesystem with `sshfs` and open it in yazi.

Kaku picks up the SSH target from the active pane and mounts it at `~/Library/Caches/dev.kaku/sshfs/<host>`.

Requirements: `sshfs` installed (`brew install macfuse sshfs`) and key-based SSH auth to the remote host.

## Shell Suite

Kaku bundles a set of shell plugins that load automatically in Kaku sessions.

**Zsh plugins (built-in)**

- **zsh-z**: Smarter `cd` that learns your most-used directories and reuses your existing `~/.z` history. `z <dir>` jumps, `z -l <dir>` lists matches, and `z -t` shows recent directories. When filesystem completion finds nothing, `cd` + Tab falls back to this history.
- **zsh-completions**: Extra completions for common CLI tools.
- **fast-syntax-highlighting**: Live command coloring and error highlighting, with richer colors and faster startup than zsh-syntax-highlighting.
- **zsh-autosuggestions**: Fish-style suggestions from your history as you type.

**Fish support**

For fish, `kaku init` sets up `~/.config/kaku/fish/kaku.fish`, and `kaku doctor` checks both the zsh and fish integration.

**Optional tools (installed via `kaku init`)**

- **Starship**: Fast, customizable prompt with git and environment info. It only applies inside Kaku, so your other terminals keep their own prompt. To use it in every terminal, set `KAKU_PROMPT_EVERYWHERE=1` before the Kaku line in your shell rc.
- **Delta**: Syntax-highlighting pager for git diff and grep.
- **Lazygit**: Terminal git UI.
- **Yazi**: Terminal file manager.

**Smart Tab**

Smart Tab takes over the Tab key in zsh for smarter completion. It has three modes:

| Mode | Behavior | Environment Variable |
| --- | --- | --- |
| Suggestion First (default) | Tab accepts the grey autosuggestion when one is showing, otherwise opens the completion list | None |
| Completion First | Tab opens the completion list first, and `->` accepts the autosuggestion | `KAKU_TAB_ACCEPT_SUGGEST_FIRST=0` |
| Off | Turns Smart Tab off and restores native zsh Tab behavior | `KAKU_SMART_TAB_DISABLE=1` |

You can also set the mode in `kaku config` (**Smart Tab** under Behavior) or in `kaku.lua`:

```lua
config.smart_tab_mode = "suggestion_first"   -- default: Tab accepts autosuggestions first
config.smart_tab_mode = "completion_first"   -- Tab shows the completion list first
config.smart_tab_mode = "off"                -- disable Smart Tab
```

If you'd rather use environment variables, for example because one zshrc serves several terminals, add one of these before the line that sources Kaku's shell integration:

```zsh
export KAKU_TAB_ACCEPT_SUGGEST_FIRST=0  # completion-first (suggestion-first is the default)
# or
export KAKU_SMART_TAB_DISABLE=1         # disable Smart Tab
```

Environment variables in your shell rc override `kaku.lua`. Smart Tab only runs in Kaku sessions (`TERM_PROGRAM=Kaku`, or tmux started from a Kaku shell).

---

Source: https://kaku.fun/docs/features
Site index for LLMs: https://kaku.fun/llms.txt
