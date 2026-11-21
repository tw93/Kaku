# Claude Code Hooks: A Practical Guide with Examples

> Where hooks live, the events that matter, and three working examples: format after an edit, block a command, and notify when Claude finishes.

Published 2026-10-03 · Tw93 · https://kaku.fun/blog/claude-code-hooks

A Claude Code hook is a shell command Claude Code runs when a session event fires, taken from the `hooks` object in a settings JSON file. The useful ones are PreToolUse, which can block or allow a tool call before it runs, PostToolUse, which runs after a tool succeeds, Notification, which runs when Claude Code sends a notification, and Stop, which runs when Claude finishes responding. Exit codes, matchers, and JSON fields below follow the [hooks guide](https://code.claude.com/docs/en/hooks-guide) and the [hooks reference](https://code.claude.com/docs/en/hooks), checked October 2026.

## Where a hook lives

A hook names an event, a matcher, and a handler. `"type": "command"` runs a shell command with the event JSON on stdin.

| File | Scope | Share it? |
|---|---|---|
| `~/.claude/settings.json` | Every project on this machine | No |
| `.claude/settings.json` | This project | Yes, commit it |
| `.claude/settings.local.json` | This project | No. Gitignored when Claude Code saves a setting to it |

User, project, and local hooks are added together, and a managed policy hook still runs unless `disableAllHooks` is set in managed settings too. For one run:

```bash
claude --settings '{"disableAllHooks": true}'
```

`/hooks` lists them read-only, labeled user, project, local, or plugin. If a saved hook is missing, look for a trailing comma or a comment. Settings JSON allows neither.

## The events that matter

![PreToolUse, then the tool, then PostToolUse, and Stop when Claude finishes](https://kaku.fun/img/blog/claude-code-hooks-lifecycle.webp)

*PreToolUse, the tool and PostToolUse repeat for every tool call in a turn. Stop fires once, when the response ends, and Notification fires whenever Claude Code needs you.*

The reference lists more events. These six are the ones used below:

| Event | When it fires |
|---|---|
| `SessionStart` | A session begins or resumes |
| `UserPromptSubmit` | A prompt is submitted, before Claude processes it |
| `PreToolUse` | Before a tool call. Can block it |
| `PostToolUse` | After a tool call succeeds |
| `Notification` | Claude Code sends a notification |
| `Stop` | Claude finishes responding |

`Stop` stays quiet if you interrupt the turn. An API error fires `StopFailure` instead.

`PreToolUse` and `PostToolUse` match the tool name. `Edit|Write` and `Edit, Write` are the same case-sensitive list. A matcher containing any character outside letters, digits, `_`, `-`, spaces, commas, and `|` is treated as an unanchored regular expression, so `Edit.*` also matches `NotebookEdit`. `Stop` ignores a matcher. An empty matcher, `"*"`, or none at all fires every time. Edit and Write pass an absolute `tool_input.file_path`. Bash passes `tool_input.command`.

## Format a file after an edit

`PostToolUse` runs after a successful tool call, so the write has already happened. This Prettier hook formats the file from the project's `.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path' | xargs npx prettier --write"
          }
        ]
      }
    ]
  }
}
```

`jq` reads the path. Install it with `brew install jq` if the hook reports `jq: command not found`. Exit 0 prints nothing in the conversation.

A `Bash` command that rewrites a file does not fire an `Edit|Write` hook. The guide uses `FileChanged` when the hook has to see the file regardless of what wrote it.

## Block a command before it runs

`PreToolUse` runs once the arguments exist, before the tool, and before the permission-mode check. Exit 2 blocks the call, and Claude reads stderr as the reason. Exit 0 leaves the normal permission flow in place. It does not approve the call.

Save this as `.claude/hooks/no-rm.sh`, then `chmod +x` it. It blocks a Bash command that starts with `rm`.

```bash
#!/bin/bash
input=$(cat)
command=$(jq -r '.tool_input.command' <<<"$input")

if [[ "$command" == rm* ]]; then
  echo "Blocked: rm commands are not allowed" >&2
  exit 2
fi

exit 0
```

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/no-rm.sh"
          }
        ]
      }
    ]
  }
}
```

`$CLAUDE_PROJECT_DIR` is the project root from session start. A command has to start with `rm`, so `npm test && rm -rf dist` gets through.

To decide in JSON, exit 0 and print this object alone. `permissionDecision` belongs inside `hookSpecificOutput`. At the top level, Claude Code ignores it and shows no error.

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked by hook"
  }
}
```

`"deny"` cancels the call. `"allow"` skips the prompt, and deny and ask rules, including managed deny lists, still apply. `"ask"` shows the prompt. Exit 2 still blocks even if the JSON says `"allow"`, and `"deny"` still blocks in bypass permissions mode. Matching hooks run in parallel. The strictest answer wins: deny, then defer, then ask, then allow.

## Notify when Claude finishes

`Stop` fires whenever a response ends, not only when the task is done, and stays quiet if you interrupt. Exit 2 tells Claude to keep going. A notification hook exits 0.

Put the sequence in `terminalSequence`. Hooks have no `/dev/tty`, so Claude Code writes it, including inside tmux and GNU screen, only while an interactive session is on screen. Allowed values are OSC 0, 1, 2, 9, 99, 777, and the bell. OSC 9 is the iTerm2, ConEmu, Windows Terminal, and WezTerm notification. OSC 777 is the urxvt, Ghostty, and Warp notification. Anything else is dropped.

`last_assistant_message` in the input is the whole reply, too long for a notification, so the script sends a fixed line, built with `printf` and wrapped in JSON by `jq -nc`:

```bash
#!/bin/bash
seq=$(printf '\033]777;notify;%s;%s\007' "Claude Code" "Finished")
jq -nc --arg seq "$seq" '{terminalSequence: $seq}'
```

`Stop` has no matcher. Register the script without one:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/notify-stop.sh"
          }
        ]
      }
    ]
  }
}
```

A `Notification` hook can return the same field, and `preferredNotifChannel` set to `notifications_disabled` does not stop it. The input includes `message` and `notification_type`. `permission_prompt` fires after about six seconds without typing. `idle_prompt` fires about 60 seconds after the response if you have not typed.

## See what a hook did

If `/hooks` lists the hook but it never runs, the matcher missed the tool name. `Ctrl + O` opens the transcript. Success is blank, a block shows the reason, and any exit other than 0 or 2 is usually a non-blocking error: the action continues, with `Failed with non-blocking status code:` and the first stderr line.

```bash
echo '{"tool_name":"Bash","tool_input":{"command":"ls"}}' | ./.claude/hooks/no-rm.sh
echo $?
```

`claude --debug` writes `~/.claude/debug/<session-id>.txt` and prints nothing in the terminal. `claude --debug-file /tmp/claude.log` picks the path, and `/debug` turns logging on mid-session. `Hook output does not start with {` means stdout was not one JSON object, often because a shell profile line printed first, so the JSON never applies. `Hook JSON output had unrecognized keys` means a field was at the wrong level.

## Hooks run with your permissions

A command hook runs as your user and can read, change, or delete any file your account can. Read it before it sits on every tool call, and quote every variable you expand.

An interactive session waits for the workspace trust dialog before running hooks from any settings file, including `~/.claude/settings.json`. `claude -p` skips the dialog and treats the folder as trusted, so a committed `.claude/settings.json` hook runs. On a repository you did not write, read `.claude/` first, or pass `--settings '{"disableAllHooks": true}'`.

## Hooks and Kaku

Kaku never adds hooks or instructions to your agents. `kaku ai` changes a tool's settings only when you edit them there, and it keeps the fields you wrote yourself.

Claude Code's own alerts are a separate switch: Kaku 0.25.0 or later with Claude Code 2.1.295 or later needs no setting, and older versions need `"preferredNotifChannel": "ghostty"` in `~/.claude/settings.json`, as [Claude Code notifications](/blog/claude-code-notifications) explains.

The Stop script is separate from that channel. It asks Claude Code to write OSC 777, the Ghostty notification on the allowlist. On Kaku 0.25.0 or later with Claude Code 2.1.295 or later, Kaku already notifies when Claude finishes, so the Stop hook above adds a second notification there. It is meant for older versions or another terminal. OSC 9 is allowlisted too, and Kaku shows it as the tool's own message when Codex has `notification_method = "osc9"`.

A script outside the session can react to Claude finishing without any hook. On Kaku 0.29.0 or later, `kaku cli events` reads the status Claude Code 2.1.295 or later reports and prints a line whenever a pane changes state:

```bash
kaku cli events --for done,blocked --json
```

Each line is one JSON object with `pane_id`, `title`, `from` and `to`. `--once` exits at the first match, and `kaku cli wait --pane-id <id>` blocks on a single pane.

## FAQ

### Does exit code 0 allow the command?

No. Exit 0 on `PreToolUse` leaves the normal permission check in place. Print `"permissionDecision": "allow"` inside `hookSpecificOutput`, and exit 0, to skip the prompt. Deny rules still apply.

### Why did the formatter miss a file?

`Edit|Write` runs only after the Edit or Write tool. A `Bash` rewrite does not fire it, and `PostToolUse` cannot undo a write that already finished.

### Why does Claude keep working after a Stop hook?

Exit 2, or `{"decision": "block", "reason": "..."}`, tells Claude to continue. After eight blocks in a row with no tool call in between, Claude Code ends the turn. Exit 0 to notify. If you do mean to block, check `stop_hook_active` first and exit 0 when it is already `true`, so the hook does not block forever.

### Will I see the notification in the window I am using?

Kaku shows them while it is in the background, not for the pane you are looking at. `terminalSequence` is written only while an interactive session is on screen.

### Does Kaku install these hooks?

No. Kaku never adds hooks or instructions on its own. You write the `hooks` object in Claude Code's settings, and `kaku ai` keeps fields you wrote yourself.
