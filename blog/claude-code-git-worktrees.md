# Run Several Claude Code Sessions at Once with Git Worktrees

> Give each Claude Code session its own checkout with git worktree or claude --worktree, set each one up, merge the work back and clean up.

Published 2026-10-07 · Tw93 · https://kaku.fun/blog/claude-code-git-worktrees

`git worktree add ../repo-feature -b feature` gives each session its own checkout of the same repository, so one Claude Code session can build a feature while another fixes a bug. Claude Code can create that checkout with `claude --worktree feature-auth`, at `.claude/worktrees/feature-auth/` on a branch named `worktree-feature-auth`. Flags and cleanup were checked October 2026 against the [git worktree manual](https://git-scm.com/docs/git-worktree), Claude Code's [common workflows](https://code.claude.com/docs/en/common-workflows), and the [worktrees](https://code.claude.com/docs/en/worktrees) page.

## Why one checkout breaks with two agents

A repository has one main working tree. Two `claude` processes started there share that tree, the index, and the files, so each write lands in the same place.

`git worktree add` refuses a branch another worktree already has checked out, unless you pass `--force`. Each session needs its own branch and directory. Claude Code uses worktrees for that file isolation. Subagents split work inside one session, and the same docs point to agent teams to coordinate several Claude sessions.

## Add, list, and remove a worktree

[`git worktree`](https://git-scm.com/docs/git-worktree) attaches another working tree to the same repository. `-b` creates the branch, starting at `HEAD` if you do not specify a commit.

```bash
git worktree add ../repo-feature -b feature
git worktree add ../repo-bugfix fix-issue-456
git worktree list
```

The second form checks out a branch that already exists. `list` prints the main worktree first, then each linked one, with its commit and branch.

A linked worktree has its own `HEAD` and index. Refs under `refs/` are shared, except `refs/bisect`, `refs/worktree`, and `refs/rewritten`. The repository config is shared. In the linked tree, `.git` is a file, and admin data lives under the main repository's `$GIT_DIR/worktrees`.

```bash
git worktree remove ../repo-feature
```

`remove` refuses the main worktree. It also refuses a linked worktree with untracked files or modifications to tracked files. `--force` removes an unclean worktree, or one with submodules. A locked worktree requires `--force` twice. If you delete the folder by hand, the admin files remain until `git worktree prune`, or until `gc.worktreePruneExpire`. Git calls multiple checkouts experimental, and its submodule support is incomplete.

## Let Claude Code create the worktree

From the repository, `--worktree` and `-w` are the same flag. A second terminal takes a different name. Without a name, Claude picks one such as `bright-running-fox`. Add `.claude/worktrees/` to `.gitignore`.

```bash
claude --worktree feature-auth
```

The repository needs a commit. Otherwise it fails with `Failed to resolve base branch "HEAD": git rev-parse failed`. The first interactive `claude` session in a directory has to accept workspace trust, or `--worktree` exits with an error. `claude -p --worktree` skips that check and, on exit, leaves the worktree and the `git worktree lock` it took.

New worktrees branch from the remote default branch, usually `main` (`worktree.baseRef` `"fresh"`). If the last fetch was more than 24 hours ago, Claude Code fetches that branch, capped at five seconds. `"head"` starts at the current local `HEAD` and carries unpushed commits. The setting cannot name a branch. For an existing branch, add the worktree with git, `cd` into it, and run `claude`.

```json
{
  "worktree": {
    "baseRef": "head"
  }
}
```

`claude --worktree "#1234"` fetches that pull request from `origin` into `.claude/worktrees/pr-<number>/`. Quote the `#`. If a worktree directory with that name already exists, Claude reopens that worktree. On `"fresh"`, a clean worktree still on Claude's branch resets to the default branch if it has no commits of its own, or if its pull request was merged and the remote branch deleted.

Asking Claude to work in a worktree calls `EnterWorktree`. `isolation: worktree` under `.claude/agents/` gives a subagent its own worktree, removed after a clean finish. A worktree that still has changes stays until the `cleanupPeriodDays` sweep can remove it without losing the work.

On exit, Claude looks for changes, untracked files, uncommitted submodule work, and new commits. A clean worktree from an unnamed session is removed with its branch. A named session asks before removing. Keeping a worktree that has work leaves the directory and the branch, and Claude prints `claude --worktree <name> --resume`. Removing deletes the directory, the branch, and the work. If the state cannot be verified, Claude asks instead of deleting.

`git worktree remove` refuses a locked worktree until it is unlocked with `git worktree unlock`. Edits to the main checkout are blocked, and so are shell commands that run there or point git there (`git -C`, `--git-dir`, `GIT_DIR`, `GIT_WORK_TREE`, or `cd`). `cp` into the main checkout is outside those checks. In [hooks](/blog/claude-code-hooks), `${CLAUDE_PROJECT_DIR}` stays at the startup project root. The hook's `cwd` is the worktree.

## Set up each checkout

The new directory is a checkout of tracked files. Install dependencies there, or ask Claude to. A gitignored `.env` is not in the checkout.

A `.worktreeinclude` at the project root copies gitignored files into worktrees Claude creates with git. It uses `.gitignore` syntax, and tracked files are never copied.

```text
.env
.env.local
config/secrets.json
```

This covers `--worktree`, subagent worktrees, and the desktop app. A `WorktreeCreate` hook skips that copy, including when SVN, Perforce, or Mercurial replace git, so the hook copies `.env` itself. After `git lfs install --local`, run `git lfs pull` in the worktree. Claude Code skips that repository-local filter.

## Merge the branch, then remove the worktree

`git worktree list` shows `worktree-feature-auth`, or the branch you passed to `-b`. Commit in the worktree and merge that branch from the main checkout before Claude's cleanup deletes it. `git worktree remove` removes the working tree and leaves the shared branch. Claude's remove deletes the directory and the branch together.

```bash
git worktree remove ../repo-feature
git worktree remove .claude/worktrees/feature-auth
```

If there are uncommitted changes or untracked files, save what you need first. Use `--force` only when you intend to discard what remains in that worktree. If git refuses because the worktree is locked, run `git worktree unlock` on that path, then remove it. After you delete a folder yourself, `git worktree prune` drops the stale entry under `$GIT_DIR/worktrees`.

To return to a worktree you kept:

```bash
claude --worktree feature-auth --resume
```

## One tab per worktree

![Three worktree folders of one repository, each on its own branch and open in its own Kaku tab running an agent](https://kaku.fun/img/blog/claude-code-git-worktrees.webp)

*All three folders share one .git directory, so a commit made in one tab is visible from the others.*

[New tab](/docs/keybindings) is `Cmd + T`. New tabs and splits inherit the current directory (`config.tab_inherit_working_directory` and `config.split_pane_inherit_working_directory` default to true), so `cd ../repo-feature` or run `claude --worktree` from the repo. `Cmd + Shift + I` or a double-click renames the tab, and a hand-set name stays when `config.tab_title_use_pane_title` shows Claude Code's title. Automatic titles show the directory. `Cmd + Shift + O` opens the Tab Navigator.

`Cmd + D` splits side by side, `Cmd + Shift + D` top and bottom, and `Cmd + Shift + Enter` zooms the pane. Closing a pane or tab asks first while Claude Code is in the foreground. Bundled `tab_close_confirmation` and `pane_close_confirmation` are `SmartPrompt`, and `Cmd + W` closes the active pane when the tab has several.

[`kaku start --new-tab`](/docs/cli) adds a tab to the current window. `kaku cli split-pane` splits the current pane and can run a command in the new one:

```bash
kaku cli split-pane -- bash -c "claude --worktree feature-auth"
```

`split-pane` prints the new pane's id, so a script can hand a worktree to an agent and wait for it. On Kaku 0.29.0 or later, `kaku cli wait` follows the status Claude Code 2.1.295 or later reports, prints the state that ended the wait, and exits 2 when the timeout runs out:

```bash
pane=$(kaku cli split-pane --right -- bash -c "claude --worktree feature-auth 'add tests for token refresh'")
kaku cli wait --pane-id "$pane" --for done,blocked,error --timeout 30m
```

`config.restore_previous_session` defaults to true, so the next launch reopens the tabs and panes. The layout is saved within a minute of opening or closing a window, tab, or pane, which covers a crash or force quit. A Claude Code pane comes back with its last screen, which shows where the conversation stopped. On Kaku 0.25.0 or later with Claude Code 2.1.295 or later, a background tab shows a dot when Claude Code is waiting, finished, or failed, with no setup. On earlier versions, the `ghostty` [notification channel](/blog/claude-code-notifications) lights the same dot for waiting and finished. On Kaku 0.29.0 or later, `Cmd + J` jumps to the next tab whose dot is lit and focuses the pane that lit it, so with one agent per worktree the one waiting on you is a single key away.

## FAQ

### Can two worktrees check out the same branch?

`git worktree add` refuses a branch that is already checked out. `--force` overrides that, and both `-b` and `claude --worktree` avoid it by creating a branch.

### Why is `.env` missing from the new worktree?

The checkout contains tracked files. A gitignored `.env` is copied only when `.worktreeinclude` lists it, and only for worktrees Claude Code creates with git.

### What does Failed to resolve base branch "HEAD" mean?

The repository has no commit yet. Commit once in the main checkout, then run `claude --worktree` again. [Common workflows](https://code.claude.com/docs/en/common-workflows) records that error.

### Does removing a worktree delete the branch?

Claude's remove deletes the directory and the branch. `git worktree remove` leaves the branch, so merge from the main checkout first. Clear a leftover lock with `git worktree unlock`.

### Does isolation block every write to the main checkout?

`Edit`, `Write`, and `NotebookEdit` on the main checkout are blocked, and so are shell commands that run there or redirect git there. `cp` into the main checkout is not blocked.
