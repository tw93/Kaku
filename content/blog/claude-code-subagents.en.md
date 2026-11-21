A [Claude Code subagent](https://code.claude.com/docs/en/sub-agents) is a helper with its own system prompt, tool list and context window. You define one as a Markdown file in `~/.claude/agents/` or `.claude/agents/`, Claude hands it a task, and only its summary comes back. This keeps search results, logs and file dumps out of your main conversation.

## Subagents, skills and agent teams

A [skill](/blog/claude-code-skills) runs in the main conversation. It is a reusable procedure that Claude loads into your current context. A subagent starts fresh: it does not see your conversation history, the skills you invoked or the files Claude already read. Claude writes it a delegation message, the subagent works from that, and it returns one result. They also connect in both directions: a skill with `context: fork` runs its body as a subagent's task, and a subagent with a `skills` field preloads the full content of those skills at startup.

[Agent teams](https://code.claude.com/docs/en/agent-teams) are a third approach: several Claude Code sessions, one acting as lead, with teammates that message each other directly and share a task list. As of October 2026, they are experimental and off by default. You turn them on with `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`, and once they are on, a subagent that Claude names launches as a teammate instead. The docs are direct about the trade-off: a subagent hands a summary back to its caller and costs less, while each teammate is a separate Claude instance, so token use grows with every active teammate.

| | Runs in | Sees your conversation | Returns |
|---|---|---|---|
| Skill | Main conversation | Yes | Nothing separate |
| Subagent | Its own context window | No, only Claude's delegation message | A summary |
| Agent team | Separate sessions | No | Teammates coordinate among themselves |

## The built-in subagents

You already use subagents without defining any. Claude Code ships these:

- **Explore**: read-only, for file discovery and code search. Claude asks it for a thoroughness level of quick, medium or very thorough.
- **Plan**: read-only research during [plan mode](/blog/claude-code-plan-mode), so exploration stays out of the main context while you review the plan.
- **general-purpose**: every tool available to subagents, for tasks that need both exploration and edits.
- **claude**, **statusline-setup** and **claude-code-guide**: helpers Claude calls on its own, for catch-all tasks, for `/statusline`, and for questions about Claude Code itself.

Explore and Plan start without your `CLAUDE.md` files or the git status snapshot, which keeps research fast and inexpensive. They are also one-shot: they return no agent ID, so Claude cannot resume them later.

## Defining your own

A subagent is a Markdown file with YAML frontmatter. Only `name` and `description` are required, and the body becomes the system prompt:

```markdown
---
name: code-reviewer
description: Reviews code for quality and best practices. Use proactively after code changes.
tools: Read, Glob, Grep, Bash
model: sonnet
---

You are a code reviewer. Run git diff, focus on the modified files,
and report issues ordered by priority with a suggested fix for each.
```

Save it in `~/.claude/agents/` to use it in every project, or in `.claude/agents/` to commit it with one repository. If two files share a name, the project file takes precedence over the user file. Claude Code watches both directories, so an edit takes effect on the next delegation without a restart. The exception is a directory that did not exist when the session started: create the first file, then restart.

The fields worth knowing:

| Field | What it does |
|---|---|
| `tools` | Allowlist. Omit it and the subagent inherits every tool available to subagents |
| `disallowedTools` | Denylist, applied before `tools` |
| `model` | `sonnet`, `opus`, `haiku`, `fable`, a full model ID, or `inherit` |
| `permissionMode` | `default`, `acceptEdits`, `auto`, `dontAsk`, `bypassPermissions` or `plan` |
| `maxTurns` | Stops the subagent after that many agentic turns |
| `skills` | Skills to preload in full at startup |
| `memory` | `user`, `project` or `local`, a directory that survives across conversations |
| `background` | `true` keeps it in the background even when Claude asks for the foreground |
| `isolation` | `worktree` runs it in a temporary git worktree |
| `hooks` | Hooks that run only while this subagent is active |

Field names are camelCase and must match exactly, because Claude Code ignores unrecognized fields without an error. A file with no `description`, or with YAML that does not parse, is skipped silently. `claude --debug` writes the reason to the debug log.

The `/agents` command in current releases only prints a reminder to ask Claude or edit the two directories yourself. On v2.1.197 and earlier, it opened a wizard with Running and Library tabs. Now you describe the subagent to Claude, say where to save it, and review the file it writes.

To call one, name it in your prompt and Claude usually delegates, or `@`-mention it to guarantee that the subagent runs. `claude --agent code-reviewer` runs the whole session as that agent, with its tools, model and system prompt.

## Where they help

The docs single out three patterns where the output is big and the answer is small.

**Search and research.** Ask a subagent to find where a config value is read, or to research the auth, database and API modules in parallel with one subagent each. The file reads stay in the subagents' context windows, and you get back a short answer, as long as the research paths do not depend on each other.

**Review.** A reviewer with `tools: Read, Glob, Grep, Bash` has no Edit or Write tool, but Bash can still run commands that change files. Keeping it read-only also requires restrictions on the commands it may execute. You can also chain them: a reviewer finds performance issues, then another subagent fixes them, with Claude passing the relevant findings along.

**Test runs and logs.** "Use a subagent to run the test suite and report only the failing tests with their error messages" is the clearest case. Thousands of lines of test output stay out of your context, and two lines of failures come back.

For edits that might collide with your own work, `isolation: worktree` gives the subagent its own copy of the repository, branched from your default branch. The worktree is removed if it makes no changes. The [worktrees post](/blog/claude-code-git-worktrees) covers running whole sessions that way.

## Where they get in the way

Stay in the main conversation when the task needs frequent back-and-forth, when planning, implementation and testing share a lot of context, or when it is a quick targeted change. A subagent starts with only Claude's summary of the task, so it may spend its first turns rediscovering what you know. If a rule in your `CLAUDE.md` must reach it, such as "ignore the `vendor/` directory", say it again when you delegate, since a subagent with `omitClaudeMd` or the built-in Explore and Plan never see it.

For a question about something already in the conversation, use `/btw`: it sees your full context, has no tools, and stays out of history.

## Cost and context

A subagent saves context, not money. It sends its own requests, which count toward the same usage limits as your main conversation. The docs warn that many subagents each returning detailed results can still fill your main context while each spends its own tokens.

A few levers:

- Set `model: haiku` for exploration-heavy subagents. A user or project subagent named `Explore` overrides the built-in, so you can move the default search agent to a cheaper model.
- Keep descriptions short. They sit in context so Claude can route, and Claude Code warns at startup if combined custom descriptions exceed 15,000 tokens.
- Use a fork when the task needs the whole conversation. `/subtask draft unit tests for the parser changes so far` starts a subagent that inherits your history and reuses the parent's prompt cache, so it costs less than a fresh subagent that needs the same background.
- Know the ceilings: subagents can nest three layers below the main conversation, and 20 running at once is the default concurrency limit, set with `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`.

## Watching parallel work in Kaku

Background subagents show up in the panel under Claude Code's prompt, and `/tasks` lists them. When one needs permission, the prompt surfaces in the main session and names the subagent that asked, so the session waits on you. When you run more than one Claude Code session, put each in its own [Kaku](/docs/ai-coding) tab, or split with `Cmd + D` and `Cmd + Shift + D` and zoom the one you are reading with `Cmd + Shift + Enter`. Agent teams' split-pane display needs tmux or iTerm2, so in Kaku without tmux, teammates show in Claude Code's own in-process panel.

Notifications require no extra configuration on Kaku 0.25.0 or later with Claude Code 2.1.295 or later. Earlier versions require `"preferredNotifChannel": "ghostty"` in `~/.claude/settings.json`; [Claude Code notifications](/blog/claude-code-notifications) explains why. The tab status dot follows the same version cutoff: before 0.25.0, the `ghostty` channel lit it when Claude Code waited or finished, while 0.25.0 lights it without setup and adds the failed state. On 0.29.0 or later, `Cmd + J` jumps to the next tab whose dot is lit.

## FAQ

### Where do subagent files go?

`~/.claude/agents/` for every project on your machine, `.claude/agents/` for one repository. Both are scanned recursively, so subfolders are fine, and identity comes from the `name` field, not the filename.

### Can a subagent see my conversation?

No. It gets its own system prompt, Claude's delegation message, your `CLAUDE.md` files (except Explore, Plan and subagents with `omitClaudeMd`), a git status snapshot and any preloaded skills. Only a fork, started with `/subtask`, inherits the history.

### Do subagents make Claude Code cheaper?

Not by themselves. Each one spends its own tokens against the same usage limits. Routing search to `model: haiku` and keeping summaries short lower the bill.

### Should I use a subagent or an agent team?

A subagent when only the result matters. An agent team when teammates need to share findings and challenge each other, and you accept the extra tokens and the experimental flag. [Claude Code agent teams](/blog/claude-code-agent-teams) covers turning them on and where teammates show up in Kaku.

### Why is Claude not using my subagent?

Claude routes on the `description`. Make it specific, add "use proactively" if you want it picked without asking, and check that the file has both `name` and `description` and that the YAML parses correctly. `@`-mentioning the subagent forces it.
