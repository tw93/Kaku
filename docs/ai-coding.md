# Claude Code and Codex in Kaku

How Kaku fits the AI coding tools you already use.

## Set Up the Tools

Run `kaku ai` to open the AI settings panel. It configures external coding tools such as Claude Code, Codex, Gemini CLI, Copilot CLI and Kimi Code, shows their plan usage, and sets up Kaku Assistant. Kaku only changes a tool's settings when you edit them there, and keeps the fields you wrote yourself. It never adds hooks or instructions to your agents on its own.

```bash
kaku ai
```

## Notifications

Claude Code 2.1.295 and later tell Kaku whether they are working, waiting on you or done, so Kaku 0.25.0 or later notifies you with no setting. Leave `preferredNotifChannel` at its default `auto`, since `ghostty` there makes Claude Code post a second notification. With an older Claude Code or Kaku before 0.25.0, the `auto` channel sends nothing here, so set this in `~/.claude/settings.json`:

```json
{ "preferredNotifChannel": "ghostty" }
```

Codex works without changes: it rings the terminal bell and Kaku turns that into a notification. To see Codex's own message instead of a generic one, add this to `~/.codex/config.toml`:

```toml
[tui]
notification_method = "osc9"
```

Notifications appear while Kaku is in the background, not for the pane you are looking at. If none show up at all, choose Kaku > Notification Settings... and turn on Allow Notifications.

Why Claude Code and Codex stay silent in most other terminals, and which setting fixes each one, is explained in [Claude Code notifications not showing](https://kaku.fun/blog/claude-code-notifications).

## Writing Prompts

`Shift + Enter` and `Cmd + Enter` insert a newline, so a long prompt can span several lines before you send it.

`Cmd + Click` opens links and file paths the tool prints, even while it captures the mouse. A path with a location such as `src/main.rs:42:7` opens at that line. To choose the editor, set `file_link_editor`:

```lua
config.file_link_editor = "zed"
-- config.file_link_editor = "cursor --goto"
```

How other terminals handle multi-line prompts, notifications and clickable paths for Claude Code is compared in [the best terminal for Claude Code](https://kaku.fun/blog/best-terminal-for-claude-code).

## Several Agents Side by Side

Split with `Cmd + D` or `Cmd + Shift + D` to run an agent next to a shell, or two agents on different tasks, and press `Cmd + Shift + Enter` to zoom the pane you are reading. Closing a pane or tab where Claude Code, Codex or another long-running program is in the foreground asks first, so a stray `Cmd + W` does not end a session.

## After a Restart

When Kaku restarts, for example to install an update, your tabs and splits come back. Panes that were running Claude Code or Codex keep their last screen, so you can see where each conversation stopped before you resume it.

## From Another Computer

Run `kaku remote` to use the same terminals, including agents that are already running, from a browser on another computer or a phone. Pairing and commands are in the [CLI reference](https://kaku.fun/docs/cli#cli-kaku-remote).

## Kaku Assistant

Kaku also has its own assistant for quick questions and command fixes in the shell. It is optional, uses the AI service you configure, and can reuse your Codex login. See [Features](https://kaku.fun/docs/features#features-kaku-assistant).

---

Source: https://kaku.fun/docs/ai-coding
Site index for LLMs: https://kaku.fun/llms.txt
