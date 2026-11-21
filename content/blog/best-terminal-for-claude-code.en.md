Claude Code runs in any terminal that can show a modern TUI, so the question is not which terminal can run it but which one gets out of the way during a long session. Five things decide that: typing a multi-line prompt without sending it early, hearing back when Claude stops and waits, opening the files and links it prints, keeping the session through a restart, and not losing it to a stray `Cmd + W`. Ghostty and iTerm2 handle most of these well. Kaku, the terminal I make, supports all five, and on 0.25.0 or later with Claude Code 2.1.295 or later its notifications need no configuration.

[[figure:claude-code-session-loop|A Claude Code session as a loop: you write a prompt, Claude works, it asks for input or finishes, you read the result and open files, and the session survives a restart|The five places where the terminal either helps a Claude Code session or gets in its way.]]

## Writing a prompt that spans several lines

Prompts for an agent are rarely one line. You paste an error, add a file path, then describe what you want, and pressing Enter in the middle sends half a prompt.

Claude Code's own docs list where `Shift + Enter` inserts a newline without setup: Ghostty, Kitty, iTerm2, WezTerm, Warp, Apple Terminal and Windows Terminal. VS Code, Cursor, Zed and Alacritty before 0.16 need `/terminal-setup` once. Two fallbacks work everywhere: `Ctrl + J`, or a backslash followed by Enter.

In Kaku, both `Shift + Enter` and `Cmd + Enter` insert a newline, with no setup needed.

## Knowing when Claude is waiting for you

A desktop notification lets you know when Claude needs input while you are in another window, but only if the terminal and Claude Code agree on how to send one.

Claude Code's `preferredNotifChannel` setting defaults to `auto`, and `auto` only sends desktop notifications in iTerm2, Ghostty and Kitty. In Terminal.app it sends the bell character, and only when the audible bell is off. Everywhere else it does nothing, which is why people on WezTerm, Warp or a terminal Claude Code does not recognize see no notification at all.

Kaku 0.25.0 or later needs no configuration with Claude Code 2.1.295 or later: Claude Code tells Kaku when it is waiting on you or done, and Kaku turns that into a notification. On older versions, Kaku understands the sequence Claude Code sends to Ghostty, so one line in `~/.claude/settings.json` is enough:

```json
{ "preferredNotifChannel": "ghostty" }
```

Kaku shows the notification only while it is in the background, so the pane you are already looking at does not ping you. If nothing appears at all, check Kaku > Notification Settings and turn on Allow Notifications.

## Opening what Claude prints

Claude Code prints file paths, often with a line number, and links to docs or pull requests. Two details decide whether you can click them.

First, Claude Code captures the mouse while it runs, and in many terminals a plain click then goes to the TUI instead of opening the link. Kaku opens links and paths with `Cmd + Click` even while a program has the mouse.

Second, a path like `src/main.rs:42:7` should open your editor at that line. Kaku sends local paths to the editor you set, with the line and column appended:

```lua
config.file_link_editor = "zed"
-- config.file_link_editor = "cursor --goto"
```

Without that setting, it falls back to VS Code if found, then `$VISUAL` and `$EDITOR`.

## Keeping the session through a restart

Agent sessions run for hours, and the terminal will restart in that time, for an update if nothing else. Two layers matter here. Claude Code keeps the conversation itself, and `claude --continue` picks up the most recent one in the current directory. The terminal has to keep everything around it: which tabs and splits you had, which directory each pane was in, and what was on screen.

Kaku saves the window layout while it runs and restores tabs and splits when it starts again. For panes that were running Claude Code or Codex, it also keeps the last screen, so before you resume you can see where each conversation stopped. The update itself is safe to interrupt: since 0.28.0, Restart to Update always leaves a working Kaku.app in Applications, even when it stops halfway.

Closing by accident is the other half. In Kaku, closing a pane or tab whose foreground program is Claude Code, Codex, an editor or a build asks for confirmation first, while a plain shell closes without a prompt.

## Running more than one

Once one session works, the next step is two: one agent refactoring while another writes tests, or an agent next to a shell where you run the app. That is a job for splits. In Kaku, `Cmd + D` splits to the right, `Cmd + Shift + D` splits down, and `Cmd + Shift + Enter` zooms the pane you want to read, then restores it.

## Picking up from another computer

Sometimes the session is on the Mac at your desk and you are not. Anthropic offers Remote Control, which lets you steer a running Claude Code session from claude.ai or the Claude app on a paid plan, and cloud sessions that run on Anthropic's machines instead of yours. Kaku has `kaku remote`, which opens the Mac's terminals in a browser on another computer or a phone, including every pane that is already running Claude Code. The [CLI reference](/docs/cli#cli-kaku-remote) covers pairing.

## How common terminals compare

Checked on 2026-10-17 against Claude Code's terminal docs.

| Terminal | Shift + Enter newline | Claude Code notifications with default settings |
|---|---|---|
| Terminal.app | Works | Bell character only, and only when the audible bell is off |
| iTerm2 | Works | Yes, after enabling escape sequence alerts in its settings |
| Ghostty | Works | Yes |
| Kitty | Works | Yes |
| WezTerm | Works | No, set `terminal_bell` or add a hook |
| Warp | Works | No, set `terminal_bell` or add a hook |
| Kaku | Works, `Cmd + Enter` too | Yes on 0.25.0 with Claude Code 2.1.295 or later, `ghostty` channel before that |

Everything Kaku sets up for Claude Code, Codex and other AI coding tools, from `kaku ai` to restoring sessions, is collected on the [AI coding tools](/docs/ai-coding) page of the docs.

## FAQ

### Why don't Claude Code notifications show up?

With the default `auto` channel, Claude Code only notifies iTerm2, Ghostty and Kitty. Other terminals need `preferredNotifChannel` set in `~/.claude/settings.json`, except Kaku 0.25.0 or later with Claude Code 2.1.295 or later, which needs no configuration. [Claude Code notifications](/blog/claude-code-notifications) lists the value for each terminal and the tmux line.

### Can I keep using tmux with Claude Code?

Yes. `Shift + Enter` inside tmux needs `set -s extended-keys on` and `set -as terminal-features 'xterm*:extkeys'`, and notifications sent through tmux passthrough need `set -g allow-passthrough on` plus a sender that wraps the sequences for tmux.
