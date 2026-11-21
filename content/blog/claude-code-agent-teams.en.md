Checked in October 2026, Claude Code agent teams are experimental and stay off until you set `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` to `1` in `~/.claude/settings.json` or in your shell. You then ask that interactive session to spawn named teammates: it remains the lead, each teammate is a separate Claude Code instance with its own context window, and token use grows with every teammate you leave running. In Kaku they show up in the in-process panel under the prompt. A pane per teammate means running Claude Code inside tmux with `teammateMode` set to `auto` or `tmux`. The split-pane backends the docs name are tmux and iTerm2 with the `it2` CLI, and Kaku's `Cmd + D` opens a second pane that you control.

## When a team earns the tokens

A [subagent](https://code.claude.com/docs/en/sub-agents) returns a summary to the caller. A teammate messages the lead and the other teammates directly, and, when the session has the Task tools, claims work from a shared task list. The docs put teams on jobs where the workers need to compare notes: several reviewers on one change, a module split across owners, a bug chased with competing hypotheses, or a change that spans frontend, backend, and tests.

Sequential work, edits to the same files, and tasks with lots of dependencies fit one session or a subagent better. Separate checkouts that you drive yourself belong in [git worktrees](https://code.claude.com/docs/en/worktrees).

## Turn teams on

Put the variable in the `env` block of `~/.claude/settings.json`, or export it in the shell. Without it, Claude Code sets up no team at session start, writes no team directories, and does not spawn or propose teammates.

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

Enabling teams changes ordinary delegation. Claude names subagents on its own so it can message them later, and in an interactive session a named subagent launches as a teammate, including when you never asked for a team. A fork, or an Agent-tool call that passes `isolation`, stays a subagent. `claude -p` and Agent SDK sessions do not spawn teammates at all.

Set the variable to `0` in `~/.claude/settings.json` and the next named subagent is a subagent again. Saving reapplies `env` values, so you do not need a new session. A `0` in the user file overrides a shell export. Project settings, local settings, a `--settings` payload, and managed settings can still set `1`. Managed settings apply last.

While the session is open, `~/.claude/teams/session-XXXXXXXX/config.json` should exist. The eight characters are the start of the session ID, the lead's `members` entry has agent type `team-lead`, and the file holds runtime state such as tmux pane IDs, so the next state update overwrites any hand edit. Claude Code deletes that directory when the session ends. `~/.claude/tasks/session-XXXXXXXX/` stays until `cleanupPeriodDays` removes it, 30 days by default. `.claude/teams/teams.json` in a project is an ordinary file.

## How a team works

The lead starts a teammate by calling the Agent tool with a `name`, and Claude Code does not ask you to confirm. The teammate loads `CLAUDE.md`, MCP servers, and skills like a normal session, or the same restricted list if you started the lead with `--setting-sources`. The lead's conversation does not carry over, so paths, constraints, and the names you will use later have to be in the spawn prompt.

Each mailbox is `~/.claude/teams/{team-name}/inboxes/{agent-name}.json`. A message counts as sent only after that write succeeds, so a full disk or an unwritable directory returns an error and delivers nothing. The receiver is told the message came from another Claude session, which means a teammate cannot approve a permission prompt for you or relay a denied action to another teammate.

The task list is a second channel, and newer models do not get it unless you opt in. From Claude Code v2.1.268, `TaskCreate`, `TaskGet`, `TaskList`, and `TaskUpdate` are on by default only for Opus 4.7, Sonnet 4.6, Haiku 4.5, and earlier models. On later models, or an unrecognized model ID, start with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`. An in-process teammate follows the lead. A split-pane teammate is its own process, so its own model decides. Without the tools, coordination is messages only. With them, unfinished dependencies block a claim, and file locking stops two teammates taking the same task.

A session has one team. Only its lead can spawn teammates, and the lead role stays with that session. An in-process teammate's own subagents run in the foreground, because its background work cannot outlive the lead's process: a definition with `background: true` returns an error, and a `run_in_background: true` request errors or runs in the foreground.

## Where teammates show up in Kaku

In the panel under the prompt, Up and Down select a row and Enter opens that transcript, so typing reaches the teammate. Escape clears the selection, and with a transcript open it interrupts the teammate's turn. `x` stops the selected teammate. `Ctrl+T` toggles the task list.

Once every row is idle, idle rows hide after 30 seconds and return on the next turn. The teammate is still running, and a message by name brings the row back. Past three idle teammates, the extras collapse into one row such as `2 idle agents`, which Enter expands.

Split panes need tmux, or iTerm2 with the [`it2` CLI](https://github.com/mkusaka/it2) and the Python API enabled under Settings, General, Magic. The docs also exclude VS Code's integrated terminal, Windows Terminal, and Ghostty. `tmux -CC` is the iTerm2 entrypoint they suggest.

Kaku 0.25.0 sets `TERM_PROGRAM` to `Kaku`, and Kaku is not one of the split-pane backends the docs name, so in Kaku split panes go through tmux. `teammateMode` defaults to `"in-process"`. `"auto"` splits only when you are already inside tmux, or when the terminal is iTerm2 with `it2` on `PATH` or with tmux installed. `"tmux"` requests split panes and picks tmux or iTerm2 from the terminal. `"iterm2"` uses `it2` and errors with the install command if `it2` is missing. `claude --teammate-mode auto` overrides the file for one session. The flag is experimental and is absent from `claude --help`.

In Kaku, set `teammateMode` to `auto` or `tmux`, then run `claude` inside tmux. `which tmux` checks `PATH`. `tmux` with no arguments creates a one-window session. `tmux ls` lists a session left behind, and `tmux kill-session -t <session-name>` ends it.

`Cmd + D`, `Cmd + Shift + D`, and `Cmd + Shift + Enter` on the [AI coding page](/docs/ai-coding) split and zoom Kaku panes. Claude Code does not place teammates in them. They are for sessions you start yourself. With one such session per tab, Claude Code 2.1.295 or later lights a dot on a background tab when it is waiting, done or failed, and on Kaku 0.29.0 or later `Cmd + J` jumps to the next tab whose dot is lit.

With a transcript open, plain text and skills go to that teammate, and built-in commands go to the lead. `/compact`, `/clear`, and `/rewind` affect the lead, so Claude Code asks first. `/model` and `/fast` do not run here. Model and fast mode are fixed at spawn, so name the model in the prompt. Otherwise `CLAUDE_CODE_SUBAGENT_MODEL` applies when it is not `inherit`. A leftover `teammateDefaultModel` is ignored since v2.1.234.

## Steer the lead

Name the roles in the prompt so later messages have a stable address:

```text
Spawn three teammates for this PR: security, performance, and test coverage.
Name them security, perf, and tests. Use Sonnet for each.
```

A subagent type works as a teammate if you name it: "Spawn a teammate using the security-reviewer agent type." The definition's `skills` field is not applied. In-process, the body is appended to the default system prompt and `SendMessage` is added. In a split pane, the body replaces the default system prompt.

Teammates inherit the lead's permission mode, except `dontAsk`. `--dangerously-skip-permissions` on the lead applies to every teammate. You can change a mode after spawn, and you cannot set a different mode per teammate at spawn time. Permission prompts arrive in the lead session. With the lead already in [plan mode](/blog/claude-code-plan-mode), Claude Code approves the teammate's plan as soon as the request arrives, and later edits still prompt.

To stop one, tell the lead to ask that teammate to shut down. It may reject with an explanation, and it finishes the current tool call first.

## What goes wrong

Subagents use the same panel, so a populated panel is not proof of a team. If Claude spawned subagents, ask again and say agent team. The config file above is the check.

`/resume` and `/rewind` do not restore in-process teammates. The lead may then message teammates that no longer exist. Tell it to spawn new ones.

A task left in progress blocks anything that depends on it, even when the files are already changed. Tell the lead to update the task or nudge the teammate. If the lead starts editing before the teammates finish, tell it to wait.

Two teammates writing one file overwrite each other, so give each a different set of files. Pre-approve common commands before you spawn, or every permission prompt stops the lead. A teammate that hits an error may stop where it is. Open the transcript, or spawn a replacement. Exit code 2 from `TeammateIdle`, `TaskCreated`, or `TaskCompleted` sends feedback and blocks that idle transition, creation, or completion.

## What you pay for

Start with 3 to 5 teammates and 5 to 6 tasks each. Research, review, and new features usually earn the extra tokens, and a routine edit stays cheaper as one session.

An in-process teammate's requests sit outside the main conversation's cache bucket and hold for five minutes by default, including on a Claude subscription. `subagentPromptCacheTtl` set to `1h` keeps the cache for an hour, and the API bills those writes at a higher rate. The key needs Claude Code v2.1.242 or later.

## FAQ

### Does Kaku draw a pane for each teammate?

You get the in-process panel. A pane per teammate comes from tmux inside that Kaku pane with `teammateMode` set to `auto` or `tmux`, or from iTerm2 with `it2`.

### Does `claude --help` list teammate mode?

`--teammate-mode` is experimental and absent from `claude --help`. `claude --teammate-mode auto` still overrides `teammateMode` for that session.

### Why is there no shared task list?

On models newer than Opus 4.7, Sonnet 4.6, and Haiku 4.5, the Task tools stay off unless you opt in, for example by starting Claude Code with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`. The team then coordinates by mailbox messages.
