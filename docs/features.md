# Features

## Kaku Assistant

Kaku Assistant has two modes: automatic error recovery and on-demand command generation from natural language.

**Setup**

Run `kaku ai` to open the AI settings panel. Enable Kaku Assistant, pick a provider, and enter your API key.

| Provider | Base URL | Models |
| :--- | :--- | :--- |
| OpenAI | `https://api.openai.com/v1` | (free text) |
| Custom | (manual) | (free text) |

Selecting a provider auto-fills the base URL and populates the model dropdown.

## AI Chat Panel

Press `Cmd + L` to open the built-in AI chat panel. It streams Markdown answers,
highlights code blocks, can include terminal context, and can use approved tools
for project files, shell commands, web search, and memory. Press `Shift + Tab`
inside the panel to toggle between the Simple Model and Deep Model when they
are different.

From a shell, use `k` or `kaku chat` for the same conversation store:

```bash
k "summarize the current project"
kaku chat
```

The standalone CLI is intentionally simpler than the overlay: it streams plain
terminal text and supports `/new`, `/resume`, `/clear`, `/status`, `/memory`,
and `/exit`.

**Error recovery**

When a command exits with a non-zero status, Kaku Assistant automatically sends the failed command, exit code, working directory, and git branch to the LLM and displays a suggested fix inline. Press `Cmd + Shift + E` to paste the suggestion into the terminal. Dangerous commands (e.g. `rm -rf`, `git reset --hard`) are pasted but never auto-executed.

The assistant does not trigger on: `Ctrl+C` exits, help flags, bare package manager calls, git pull conflicts, or non-shell foreground processes.

**Natural language to command**

Type `# <description>` at the prompt and press Enter to generate a shell command from plain English. Kaku intercepts the line before the shell sees it, sends your query along with the current directory and git branch to the LLM, and injects the resulting command back into the prompt ready to review and run.

```
# list all files modified in the last 7 days
# find and kill the process on port 3000
# compress the src folder excluding node_modules
```

The `#` prefix works in both zsh and fish. The original query stays visible while the request is in flight. If the model cannot produce a safe command, it injects a short explanation instead. Dangerous commands are loaded but flagged for review, never auto-executed.

**assistant.toml fields**

The config lives at `~/.config/kaku/assistant.toml`:

| Field | Description |
| :--- | :--- |
| `enabled` | `true` to enable, `false` to disable |
| `api_key` | Your provider API key |
| `model` | Simple Model for `#` command generation, command fixes, and lightweight chat |
| `chat_model` | Deep Model for primary `Cmd + L` / `k` chat and tool use |
| `chat_model_choices` | Optional curated list of chat models for the overlay picker |
| `auto_fix_ignored_exit_codes` | Optional exit codes that should not trigger automatic command-fix suggestions, e.g. `[2]` |
| `base_url` | OpenAI-compatible API root URL |
| `custom_headers` | Extra HTTP headers for enterprise proxies, e.g. `["X-Customer-ID: your-id"]` |
| `web_search_provider` | Optional search backend: `brave`, `pipellm`, or `tavily` |
| `web_search_api_key` | API key for the selected search backend |
| `web_fetch_script` | Optional custom URL-to-Markdown fetch script |
| `chat_tools_enabled` | Set to `false` to disable tool calling for chat providers without tool support |
| `auth_type` | Advanced auth mode, e.g. `api_key`, `copilot`, or `codex` |
| `memory_curator_model` | Optional cheaper model for background memory curation |

Older configs may still contain `fast_model`; Kaku treats it as the Simple Model
and folds it back into `model` the next time the assistant settings are saved.

---

## Window Snapshots

Kaku saves multi-tab and multi-pane window layouts automatically when you close
or hide a window. Use **Shell > Restore Previous Window** or
`Cmd + Option + Shift + T` to reopen the last saved layout. Kaku tolerates
missing or corrupted snapshot files and simply reports that no snapshot is
available.

---

## AppleScript

Kaku ships a minimal AppleScript dictionary so it shows up in Script Editor and other automation tools. The exposed surface is intentionally small and read-only apart from `quit`.

```applescript
tell application "Kaku"
  get name        -- "Kaku"
  get version     -- e.g. "0.10.0"
  get frontmost   -- true / false
  quit            -- optional `saving ask|yes|no`
end tell
```

Open `/Applications/Kaku.app` in Script Editor → File → Open Dictionary to browse the full dictionary. There is no `do script` verb — Kaku does not expose shell execution to AppleScript.

---

## Lazygit Integration

Press `Cmd + Shift + G` to launch lazygit in the current pane. Kaku auto-detects the lazygit binary from PATH or common Homebrew locations.

When a git repo has uncommitted changes and lazygit has not been used in that directory yet, Kaku shows a one-time hint to remind you it is available.

Install lazygit with `brew install lazygit` or via `kaku init`.

---

## Yazi File Manager

Press `Cmd + Shift + Y` to launch yazi in the current pane. The shell wrapper `y` also launches yazi and syncs the shell working directory on exit.

**Theme sync**: Kaku automatically updates `~/.config/yazi/theme.toml` to match the active color scheme (Kaku Dark or Kaku Light). No manual yazi theme setup needed.

Install yazi with `brew install yazi` or via `kaku init`.

---

## Remote Files

Press `Cmd + Shift + R` to mount the current SSH session's remote filesystem locally via `sshfs` and open it in yazi.

Kaku auto-detects the SSH target from the active pane. The mount lives at `~/Library/Caches/dev.kaku/sshfs/<host>`.

Requirements: `sshfs` installed (`brew install macfuse sshfs`) and passwordless SSH auth (key-based) for the remote host.

---

## Shell Suite

Kaku ships a curated set of shell plugins that load automatically inside Kaku sessions.

**Zsh plugins (built-in)**

- **z**: Smarter `cd` that learns your most-used directories. Use `z <dir>`, `z -l <dir>` to list matches, `z -t` for recent directories.
- **zsh-completions**: Extended completions for common CLI tools.
- **zsh-syntax-highlighting**: Real-time command coloring and error highlighting.
- **zsh-autosuggestions**: Fish-style history-based completions as you type.

**Fish support**

Run `kaku init` to provision `~/.config/kaku/fish/kaku.fish` for fish users. `kaku doctor` verifies both zsh and fish integration paths.

**Optional tools (installed via `kaku init`)**

- **Starship**: Fast, customizable prompt with git and environment info.
- **Delta**: Syntax-highlighting pager for git diff and grep.
- **Lazygit**: Terminal git UI.
- **Yazi**: Terminal file manager.

**Smart Tab**

Kaku's Smart Tab overrides the Tab key in zsh to provide smarter completion behavior. It supports three modes:

| Mode | Behavior | Environment Variable |
| :--- | :--- | :--- |
| Completion First (default) | Tab shows the completion list; use `->` to accept autosuggestions | - |
| Suggestion First | Tab accepts autosuggestions when available, falls back to completion | `KAKU_TAB_ACCEPT_SUGGEST_FIRST=1` |
| Off | Disables Smart Tab entirely, restoring native zsh Tab behavior | `KAKU_SMART_TAB_DISABLE=1` |

You can also set the mode via `kaku config` (the **Smart Tab** option under Behavior) or in `kaku.lua`:

```lua
config.smart_tab_mode = "completion_first"   -- default
config.smart_tab_mode = "suggestion_first"   -- Tab accepts autosuggestions first
config.smart_tab_mode = "off"                -- disable Smart Tab
```

If you prefer environment variables (for example, because you share your zshrc across terminals), add one of these before sourcing the Kaku shell integration:

```zsh
export KAKU_TAB_ACCEPT_SUGGEST_FIRST=1  # suggestion-first mode
# or
export KAKU_SMART_TAB_DISABLE=1         # disable Smart Tab
```

```fish
set -gx KAKU_SMART_TAB_DISABLE 1
```

Environment variables set in your shell rc take precedence over `kaku.lua` settings. Smart Tab is only active inside Kaku sessions (`TERM_PROGRAM=Kaku`).
