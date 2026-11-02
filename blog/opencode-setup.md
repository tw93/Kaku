# OpenCode Setup on Mac: Install, Providers, AGENTS.md and Permissions

> Install OpenCode on a Mac, connect a provider with /connect, write AGENTS.md with /init, set permissions, and use it in Kaku.

Published 2026-11-02 · Tw93 · https://kaku.fun/blog/opencode-setup

OpenCode is an open source terminal coding agent from [opencode.ai](https://opencode.ai). To set it up on a Mac, install it with the official script, run `opencode` inside a project, connect a provider with `/connect`, then run `/init` so it writes an `AGENTS.md` for the repository. Its two main differences from Claude Code are that it is not tied to one model vendor, and that it allows most actions by default until you tell it to ask. This post covers OpenCode 2, the version the project's README now installs, and goes through the install, the config file, agents and permissions, and how it behaves inside Kaku. The commands follow the official docs as read in October 2026.

## Which project this is

Several things have carried the name opencode. The one this post covers is the agent at opencode.ai, whose repository is [github.com/anomalyco/opencode](https://github.com/anomalyco/opencode) under the MIT license; older links to `sst/opencode` land on that repository. A different Go project, `opencode-ai/opencode`, was [archived in September 2025](https://github.com/opencode-ai/opencode), and its README says development continued as Crush. If a tutorial tells you to install from that repository, it is describing the older project.

## Install

The [OpenCode 2 docs](https://opencode.ai/v2/docs/) list several routes. On a Mac the common ones are:

```bash
# Official script
curl -fsSL https://opencode.ai/v2/install | bash

# Homebrew, from the project's own tap
brew install anomalyco/tap/opencode-v2

# npm
npm install -g @opencode/cli
```

The README recommends the `anomalyco/tap` formula over the plain `opencode` formula in Homebrew's core repository, which is updated less often. OpenCode 1 still gets releases and still installs from `opencode.ai/install`, `anomalyco/tap/opencode` and the `opencode-ai` npm package, which older tutorials show. Both versions use the `opencode` command, so keep one: the V2 script replaces a V1 binary, and the docs ask you to remove a V1 installed by a package manager first. Check the result with `opencode --version`.

## Connect a provider

Start OpenCode in a project and run `/connect`:

```bash
cd ~/code/my-project
opencode
```

Inside the terminal UI, `/connect` lists the providers, and you pick one and follow its prompts, usually pasting an API key. `opencode auth login` does the same from the shell, and `opencode auth list` shows what is set up. Saved keys go into OpenCode's local database, `~/.local/share/opencode/opencode.db` by default, and a V2 install imports the keys an older version kept in `auth.json`. Then `/models` picks a model from the providers you connected, and `opencode models` lists the available models.

Some providers offer a browser sign-in instead of a key. For OpenAI, `/connect` also offers a sign-in with your ChatGPT account, alongside the API key.

OpenCode also sells model access of its own, OpenCode Go, a subscription for coding models its team has tested, and the OpenCode Console. You do not need either.

## The terminal UI

Running `opencode` with no arguments opens the terminal UI. These pieces are worth learning first:

- **`Shift + Tab`** cycles through the primary agents, `build` and `plan`. `/agents` lists them.
- **`@`** fuzzy-searches files in the project and attaches one to your message.
- **`!`** at the start of an empty prompt enters shell mode, and the command's output stays in the session.
- **`/undo` and `/redo`** roll back a message together with its file changes, and restore it. Restoring files needs the project to be a Git repository; outside one, `/undo` only rolls back the conversation.
- **`/sessions`** lists and switches sessions, and `/new` starts a fresh one. The default leader key is `ctrl+x`, so `ctrl+x u` is undo and `ctrl+x m` opens the model list.

From the shell, `opencode run "your prompt"` runs one prompt without the interface, and `opencode -c` continues the last session.

## Rules in AGENTS.md

Run `/init` inside a project and OpenCode reads the repository and writes `AGENTS.md`, asking a few questions if the code does not answer them. If the file exists, it improves it in place. The docs suggest committing it, since it holds the build commands and conventions the agent should follow.

Global rules go in `~/.config/opencode/AGENTS.md`. OpenCode loads that file plus every `AGENTS.md` from the current directory up to the project root, and combines them rather than letting one replace another. The `instructions` key in the config can add more files, such as `CONTRIBUTING.md` or a glob like `docs/guidelines/*.md`.

People coming from Claude Code should know one change: OpenCode 2 reads only `AGENTS.md` and no longer falls back to `CLAUDE.md`. Either move your rules into `AGENTS.md`, or list `CLAUDE.md` under `instructions`. One `AGENTS.md` can serve OpenCode, [Codex](/blog/codex-cli-setup) and others, while Claude Code keeps reading its own `CLAUDE.md`.

## The config file

OpenCode merges settings from several places instead of replacing one with another. The ones you will use are the global `~/.config/opencode/opencode.json` and an `opencode.json` in the project; the project file overrides the global one for keys that conflict. Both also accept the `.jsonc` extension if you want comments.

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-5",
  "permissions": [
    { "action": "edit", "resource": "*", "effect": "ask" },
    { "action": "shell", "resource": "*", "effect": "ask" }
  ]
}
```

The `$schema` line gives your editor validation, and `model` is the main model, written as `provider/model`. The model name here comes from the docs' own example, so replace it with something `opencode models` actually lists for your providers.

## Agents and permissions

Two primary agents ship with it. `build` is the default and runs its tools without asking. `plan` explores and plans: it cannot edit your project files, and its shell commands follow your permission rules. There are also two subagents a primary agent can hand work to: `general` for research and multi-step work, and `explore` for searching and reading code without editing. To write your own, put a Markdown file in `~/.config/opencode/agents/` or `.opencode/agents/`; the file name becomes the agent name. The same idea exists in [Claude Code subagents](/blog/claude-code-subagents).

Each permission rule has an `action`, a `resource` and an `effect`, and the effect is `allow`, `ask` or `deny`. By default almost everything is allowed, including paths outside the project, and reading `.env` files asks first. A stricter setup looks like this:

```json
{
  "permissions": [
    { "action": "*", "resource": "*", "effect": "ask" },
    { "action": "read", "resource": "*", "effect": "allow" },
    { "action": "shell", "resource": "git status *", "effect": "allow" },
    { "action": "external_directory", "resource": "*", "effect": "ask" }
  ]
}
```

The last matching rule wins, so the broad rule goes first and the exceptions after it. The prompt answers are allow once, always allow, which is saved for the project, or reject, and starting with `opencode --auto` approves requests that would ask, while explicit `deny` rules still apply. Always allow saves the pattern the tool proposes rather than the exact request, so approving `git status --short` can save `git status *`. Saved approvals never override a configured `deny`, and reject also turns down every other request still pending in that session. Config files from OpenCode 1 that use `permission` and `bash` still load, but new rules are easier to read in the form above.

Rules for one agent go under `agents.<id>.permissions` and are appended after the top-level ones, so they win for that agent, and a custom subagent runs on its own rules rather than a subset of its parent's. Shell rules match the command text, and OpenCode's guess at which directories a command touches is best effort, so the docs recommend a short allowlist of shell commands over patterns that try to catch every dangerous one.

## How it differs from Claude Code and Codex

| | OpenCode | Claude Code | Codex CLI |
|---|---|---|---|
| Models | Many providers, chosen with `/connect` | Anthropic's models | A ChatGPT plan or an OpenAI API key |
| Edits and shell commands | Allowed until you set `ask` | Ask first in the default mode | Set by `approval_policy` and `sandbox_mode` |
| Rules file | `AGENTS.md` | `CLAUDE.md` | See the [Codex setup post](/blog/codex-cli-setup) |

OpenCode is MIT licensed and Codex CLI is Apache-2.0. The choice is usually about the model and the review habit. Pick OpenCode if you want one terminal tool across several providers. Pick Claude Code if you rely on its [hooks](/blog/claude-code-hooks) and [plan mode](/blog/claude-code-plan-mode), which this post does not map onto OpenCode. [Codex vs Claude Code](/blog/codex-vs-claude-code) covers those two in more depth.

## Running it in Kaku

OpenCode is a normal terminal program, so it works in Kaku like in any other terminal. What Kaku adds is around it. `Cmd + D` and `Cmd + Shift + D` split the window, which suits `build` in one pane and a shell in the next, and `Cmd + Shift + Enter` zooms the pane you are reading. `Shift + Enter` and `Cmd + Enter` insert a newline in a prompt. Closing a pane where a long-running program is in the foreground asks first. After a restart, Kaku brings back your tabs and splits, but not OpenCode's screen or process, since it keeps the last screen of a full-screen program only for Claude Code and Codex, so start OpenCode again with `opencode -c`.

`kaku ai` sets up tools such as Claude Code, Codex, Gemini CLI and Kimi Code, and OpenCode is not among them at version 0.25.0, so its provider and config stay in OpenCode's own files.

## What goes wrong

- **You installed the wrong project or version.** Check `opencode --version`, and that the install line came from opencode.ai or the `anomalyco` repository.
- **The model list is empty.** `/models` only shows providers you connected. Run `opencode auth list` to see what is stored.
- **Edits happen without asking.** That is the default. Add the `permissions` rules above, or switch to `plan` with `Shift + Tab`.
- **`/undo` brings back the prompt but not the files.** File restore needs a Git repository.
- **Your CLAUDE.md is ignored.** OpenCode 2 reads only `AGENTS.md`; list `CLAUDE.md` under `instructions` or move the rules.

## FAQ

### Is OpenCode free?

The software is MIT licensed. You still pay whichever provider you connect, or sign in with an account a provider offers.

### Where is the OpenCode config file on Mac?

`~/.config/opencode/opencode.json` for your user, and `opencode.json` in a project for that project. Saved provider keys live in `~/.local/share/opencode/opencode.db`.

### Does OpenCode read CLAUDE.md?

OpenCode 2 does not read it on its own. Add it under `instructions` in `opencode.json`, or move its content into `AGENTS.md`.

### How do I continue the last session?

`opencode -c` from the shell, or `/sessions` inside the interface.
