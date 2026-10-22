# Claude Code Slash Commands: The Ones That Matter and How to Write Your Own

> The Claude Code slash commands worth knowing, their keyboard shortcuts, and how to write custom commands with arguments, plus three to copy.

Published 2026-10-22 · Tw93 · https://kaku.fun/blog/claude-code-slash-commands

Type `/` in a Claude Code session to see every command, then keep typing letters to filter. The ones that matter day to day are `/clear` and `/compact` for context, `/model` and `/effort` for the model, `/resume` and `/rewind` for sessions, `/usage` for cost, `/permissions` and `/hooks` for rules, `/init` and `/memory` for `CLAUDE.md`, and `/mcp` for servers. To add your own, save a Markdown file as `.claude/commands/<name>.md` in a project or `~/.claude/commands/<name>.md` for every project, and Claude Code turns it into `/<name>`. Custom commands have since been merged into skills, which is the format to prefer for new work.

## The commands worth knowing

This list follows the [commands reference](https://code.claude.com/docs/en/commands), read on 19 October 2026. The reference has over a hundred entries, and not every command shows for every user, since availability depends on platform, plan and environment. A command only runs when it starts your message, and the text after it becomes its arguments.

| Command | What it does |
|---|---|
| `/clear [name]` | Start a new conversation with empty context. A name labels the old one in the `/resume` picker |
| `/compact [instructions]` | Summarize the conversation to free space. Instructions tell it what to keep |
| `/context` | Show what is filling the context window as a grid, with suggestions |
| `/model [model]` | Switch model and save it as the default for new sessions. Press `s` on a row in the picker to switch for this session only |
| `/effort [level]` | Set the effort level, from `low` to `max`, or `auto` |
| `/resume [session]` | Return to an earlier conversation by name or ID, or open the picker |
| `/rewind` | Roll code and conversation back to a checkpoint. Aliases are `/checkpoint` and `/undo` |
| `/usage` | Session cost, plan limits and activity. `/cost` and `/stats` are aliases |
| `/permissions` | View and edit allow, ask and deny rules, see the [permissions](/blog/claude-code-permissions) |
| `/memory` | Edit `CLAUDE.md` files and toggle auto memory |
| `/init` | Generate a starter `CLAUDE.md` for the project |
| `/mcp` | Manage MCP servers, with `reconnect`, `enable` and `disable` forms, see [MCP servers](/blog/claude-code-mcp-servers) |
| `/hooks` | View hook configuration, see [hooks](/blog/claude-code-hooks) |
| `/plan [description]` | Enter plan mode, optionally with a task already attached, see [plan mode](/blog/claude-code-plan-mode) |
| `/diff` | Review the working tree changes, including Claude's edits |
| `/code-review` | Review the current diff, or a PR number. `/review` is an alias and `/security-review` checks for vulnerabilities |
| `/btw [question]` | Ask a side question without adding it to the conversation |
| `/agents` | Prints a reminder to ask Claude to create or manage subagents, or to edit `.claude/agents/`. See [subagents](/blog/claude-code-subagents) |
| `/statusline` | Configure the status line, see [status line](/blog/claude-code-statusline) |
| `/doctor` | Check installation and settings, and propose fixes |

Two of these changed meaning over time. `/agents` used to open an interactive manager, and on current versions it only prints that reminder. `/cost` is now just an alias for `/usage`. If an older tutorial shows a screen you do not get, check the reference before assuming something is broken.

## Keyboard shortcuts that pair with them

The [interactive mode page](https://code.claude.com/docs/en/interactive-mode) lists the shortcuts. These are the ones that save typing a command:

| Shortcut | Does |
|---|---|
| `Shift+Tab` | Cycle permission modes, such as Manual, accept edits, plan and auto |
| `Option+P` on macOS | Switch model without clearing your prompt |
| `Esc` | Interrupt Claude, keeping the work done so far |
| `Esc` + `Esc` | Clear a typed draft, or open the rewind menu on an empty prompt |
| `Ctrl+R` | Reverse search of your command history |
| `Ctrl+G` | Edit the prompt in your default text editor |
| `Ctrl+O` | Toggle the transcript viewer |
| `Ctrl+B` | Background running Bash commands and agents. Inside tmux, press it twice |
| `!` at the start | Shell mode, run a command and show its output to Claude |
| `@` | Autocomplete a file path |

`Option+P` needs the Option key to reach the terminal as Meta. Kaku sends Meta from the left Option key by default and types composed characters from the right one, so it works there with no change. A long prompt can span lines with `Shift+Enter` or `Cmd+Enter` in Kaku.

## Custom commands as Markdown files

The docs state that custom commands have been merged into skills: a file at `.claude/commands/deploy.md` and a skill at `.claude/skills/deploy/SKILL.md` both create `/deploy` and work the same way. Existing command files keep working. Skills add a folder for supporting files and finer control over who can invoke them, so use one for new work, as [Claude Code skills](/blog/claude-code-skills) explains. A command file is still the shortest way to save one prompt.

How the name is chosen:

| File | Command |
|---|---|
| `.claude/commands/deploy.md` | `/deploy` |
| `.claude/commands/frontend/component.md` | `/frontend:component` |
| `.claude/skills/deploy/SKILL.md` | `/deploy`, or whatever `name` sets |

Scope follows location. A file under `.claude/` in a repository applies to sessions in that repository, and committing it shares it with the team. A file under `~/.claude/` applies to all your projects on that machine. If a skill and a command file share a name, the skill wins.

A command file accepts the same frontmatter as a skill except `name` and `paths`. The fields you will use most:

- `description` says what it does, shown in the `/` menu.
- `argument-hint` shows expected arguments during autocomplete, such as `[issue-number]`.
- `allowed-tools` lets Claude use listed tools without asking while the command runs.
- `disable-model-invocation: true` stops Claude from running it on its own, which suits anything with side effects.

The skills docs, which command files follow, add two cautions about `allowed-tools`. The grant lasts only for the turn that runs the command and clears when you send your next message, while the command text stays in context. And workspace trust does not gate it, so a command file checked into a repository can pre-approve tools for itself: read the `allowed-tools` lines in a cloned project's `.claude/commands/` before running Claude Code there.

## Arguments

Whatever follows the command name replaces `$ARGUMENTS` in the file. For positions, `$ARGUMENTS[0]` or the shorter `$0` is the first argument, and `$1` the second. Wrap multi-word values in quotes, so `/migrate "hello world" second` makes `$0` equal `hello world`. If the file has no placeholder at all, Claude Code appends `ARGUMENTS: <your input>` to the end so Claude still sees what you typed.

A line written as an exclamation mark and a backtick command, such as `` !`git diff HEAD` ``, runs before Claude sees the file and is replaced with the command's output. That gives the model live data instead of asking it to guess.

## Three commands to copy

`.claude/commands/fix-issue.md`, run as `/fix-issue 123`:

```markdown
---
description: Fix a GitHub issue by number
argument-hint: [issue-number]
allowed-tools: Bash(gh *)
disable-model-invocation: true
---

Fix GitHub issue $ARGUMENTS. Start with `gh issue view $ARGUMENTS`, find the
cause, make the smallest fix and run the tests.
```

`.claude/commands/summarize-changes.md`, run as `/summarize-changes`:

```markdown
---
description: Summarize uncommitted changes and flag risks
---

## Current changes

!`git diff HEAD`

## Instructions

Summarize the changes above in two or three bullet points, then list risks such as
missing error handling, hardcoded values or tests that need updating. If the diff
is empty, say there are no uncommitted changes.
```

`~/.claude/commands/migrate.md`, a personal command run as `/migrate SearchBar JavaScript TypeScript`:

```markdown
---
description: Migrate a component between frameworks
argument-hint: [component] [from] [to]
---

Migrate the $0 component from $1 to $2. Keep behavior identical and list any
places where the two differ.
```

Run each once and check the result. If a command does not appear in the `/` menu, check the file name and that the opening `---` is the very first line of the file. Claude Code reads frontmatter only in that case, and ignores field names it does not recognize without an error. Command files in a directory added with `--add-dir` are not watched, so restart the session after editing them.

## FAQ

### Where do custom slash commands go?

In `.claude/commands/` for a project, or `~/.claude/commands/` for every project. A skill folder under `.claude/skills/` creates the same kind of command and is the format the docs prefer.

### Are slash commands and skills the same thing now?

Mostly. Both create a `/name` command and work the same way. Skills add supporting files and invocation control, and a skill wins over a command file of the same name.

### How do I see all commands?

Type `/` and filter with letters, or run `/help`. Some commands are hidden from the menu until you type their full name.
