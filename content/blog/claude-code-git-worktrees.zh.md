`git worktree add ../repo-feature -b feature` 会给同一个仓库再检出一份，每个 Claude Code 会话拿到自己的目录和分支，一个会话做功能，另一个会话修 bug，两边写下去的不是同一批文件。这份检出就是 worktree，也就是链接工作树，Claude Code 自己也能建，命令是 `claude --worktree feature-auth`，目录在 `.claude/worktrees/feature-auth/`，分支名叫 `worktree-feature-auth`。文中的 flag 和清理行为是 2026 年 10 月对着 [git worktree 手册](https://git-scm.com/docs/git-worktree/zh_HANS-CN)、Claude Code 的[常见工作流程](https://code.claude.com/docs/zh-CN/common-workflows)和 [worktrees](https://code.claude.com/docs/zh-CN/worktrees) 页核对过的。

## 为什么两个 agent 不能共用一个检出目录

一个仓库只有一棵主工作树，两个 `claude` 进程从这里启动的话，工作树、index 和文件全是同一份，每次写入都落在同一个地方。

`git worktree add` 会拒绝别的工作树已经检出的分支，除非加上 `--force`，所以每个会话都要有自己的分支和目录。Claude Code 用 worktree 把文件隔开，subagent 是在同一次会话里把活拆开，多个 Claude 会话要一起配合，文档里用的是 agent teams。

## 添加、列出和删除 worktree

[`git worktree`](https://git-scm.com/docs/git-worktree/zh_HANS-CN) 给同一个仓库再挂上一棵工作树，`-b` 会新建分支，你没写提交的时候，起点就是 `HEAD`。

```bash
git worktree add ../repo-feature -b feature
git worktree add ../repo-bugfix fix-issue-456
git worktree list
```

第二种写法检出的是已经存在的分支，`list` 先打印主工作树，再打印每一棵链接工作树，每行后面带着它的提交和分支。

链接工作树有自己的 `HEAD` 和 index，`refs/` 下面的引用是共享的，`refs/bisect`、`refs/worktree` 和 `refs/rewritten` 除外，仓库配置也共享，这棵树里的 `.git` 是一个文件，管理数据放在主仓库的 `$GIT_DIR/worktrees` 下面。

```bash
git worktree remove ../repo-feature
```

`remove` 不会动主工作树，链接工作树里有未跟踪文件，或者已跟踪文件被改过，它也会拒绝，带未提交改动或未跟踪文件的工作树只有加 `--force` 才能删除，这也会丢掉其中未保存的内容，带子模块的也需要这个参数，锁定的工作树得把 `--force` 加两次。目录如果是你亲手删的，管理文件会留到 `git worktree prune`，或者留到 `gc.worktreePruneExpire`。Git 手册里说，同一仓库多份检出这个功能还算试验性的，子模块的支持也不完整。

## 让 Claude Code 自己建 worktree

在仓库目录里，`--worktree` 和 `-w` 是同一个 flag，第二个终端得换一个名字，名字空着的时候 Claude 会自己起一个，比如 `bright-running-fox`，`.claude/worktrees/` 要你自己加进 `.gitignore`。

```bash
claude --worktree feature-auth
```

仓库里得先有一次提交，不然会报 `Failed to resolve base branch "HEAD": git rev-parse failed`。某个目录第一次跑交互式 `claude`，还得先接受工作区信任，否则 `--worktree` 会报错退出，`claude -p --worktree` 会跳过这次检查，退出时留下那棵工作树，以及它加上的 `git worktree lock`。

`worktree.baseRef` 默认是 `"fresh"`，新建的工作树从远端默认分支分出来，一般是 `main`，距离上次 fetch 超过 24 小时的话，Claude Code 会再 fetch 这个分支，最多等 5 秒。设成 `"head"` 就从当前本地 `HEAD` 开始，还没 push 的提交也会带上，这个设置不能写成某个分支名，已有的分支得先用 git 把工作树建好，`cd` 进去再跑 `claude`。

```json
{
  "worktree": {
    "baseRef": "head"
  }
}
```

`claude --worktree "#1234"` 会从 `origin` 把这个 pull request 取到 `.claude/worktrees/pr-<number>/`，`#` 要加引号，这个名字的目录已经在的话，会重新打开那棵工作树。在 `"fresh"` 下，一棵工作树如果还停在 Claude 建的那条分支上，也是干净的，并且自己一次提交都没有，或者对应的 pull request 已经合并、远端分支也删了，重新打开时会被重置到默认分支。

让 Claude 到某棵工作树里干活时会调用 `EnterWorktree`，subagent 要单独一棵的话，在 `.claude/agents/` 里写 `isolation: worktree`，干净做完会删掉，还留着改动的会留到 `cleanupPeriodDays` 那次清理，只有确认不会丢掉这些改动才会删。

退出时 Claude 会看有没有改动、未跟踪文件、子模块里还没提交的内容，以及新提交，没有会话名而且工作树是干净的，目录和分支就一起删掉，会话有名字就会先问。工作树里还有要留的内容，目录和分支都保留，Claude 会打印 `claude --worktree <name> --resume`，选择删除就把目录、分支和这些内容一起去掉。状态如果核对不了，就改成询问，不会直接删。

`git worktree remove` 遇到锁定的工作树会拒绝，得先 `git worktree unlock`。在 worktree 会话里，Claude 改主工作树里的文件会被拦住，在主工作树里跑的 shell 命令，以及把 git 指到那边的命令也会被拦住，包括 `git -C`、`--git-dir`、`GIT_DIR`、`GIT_WORK_TREE` 和 `cd`，`cp` 进主工作树不在这些检查里。在 [hooks](/zh/blog/claude-code-hooks) 里，`${CLAUDE_PROJECT_DIR}` 一直是启动时的项目根，hook 的 `cwd` 才是这棵工作树。

## 每一份检出要单独配

新目录里检出的是已跟踪文件，依赖在这个目录里装，也可以让 Claude 装，写在 `.gitignore` 里的 `.env` 不会跟着进这份检出。

项目根上的 `.worktreeinclude` 会把 gitignore 掉的文件复制进 Claude 用 git 建出来的工作树，语法和 `.gitignore` 一样，已经跟踪的文件永远不会复制。

```text
.env
.env.local
config/secrets.json
```

`--worktree`、subagent 的工作树和桌面 App 都会照这个文件复制，`WorktreeCreate` hook 会跳过这次复制，用 SVN、Perforce 或 Mercurial 代替 git 建工作树时也会跳过，`.env` 就得由这个 hook 自己复制。`git lfs install --local` 之后，还要在工作树里跑 `git lfs pull`，Claude Code 不会跑仓库本地的这个 filter。

## 先合并分支，再删掉 worktree

`git worktree list` 能看到 `worktree-feature-auth`，或者你用 `-b` 传进去的那个分支，提交在这棵工作树里做完，再回到主工作树把这个分支合并进来，而且得赶在 Claude 的清理把分支删掉之前。`git worktree remove` 只删工作树，共享的分支还留在仓库里，Claude 自己的删除会连目录带分支一起删掉。

```bash
git worktree remove ../repo-feature
git worktree remove .claude/worktrees/feature-auth
```

有未提交改动或未跟踪文件时，先保存需要的内容，只有确定要丢弃工作树里剩下的内容，才加 `--force`，git 因为锁定而拒绝的话，先对那个路径跑 `git worktree unlock` 再删，目录如果是你亲手删的，`git worktree prune` 会清掉 `$GIT_DIR/worktrees` 下面那条过期记录。

想回到留下来的那棵工作树：

```bash
claude --worktree feature-auth --resume
```

## 一个 worktree 一个标签

[[figure:claude-code-git-worktrees|同一个仓库的三个 worktree 文件夹，各在自己的分支上，各开在一个跑着 agent 的 Kaku 标签里|三个文件夹共用同一个 .git 目录，所以在一个标签里提交的改动，另外两个标签里也看得到。]]

[新建标签](/zh/docs/keybindings) 是 `Cmd + T`，新标签和分屏会继承当前目录，`config.tab_inherit_working_directory` 和 `config.split_pane_inherit_working_directory` 默认都是 true，所以可以 `cd ../repo-feature`，也可以直接在仓库里跑 `claude --worktree`。`Cmd + Shift + I` 或者双击能改标签名，手动改过的名字会留着，`config.tab_title_use_pane_title` 让标签显示 Claude Code 的标题时也不会被换掉，自动标题显示的是目录，`Cmd + Shift + O` 打开 Tab Navigator。

`Cmd + D` 左右分屏，`Cmd + Shift + D` 上下分屏，`Cmd + Shift + Enter` 放大当前窗格，窗格或标签里 Claude Code 正在前台运行时，关掉之前会先问，内置的 `tab_close_confirmation` 和 `pane_close_confirmation` 都是 `SmartPrompt`，一个标签里有好几个窗格时，`Cmd + W` 关的是当前窗格。

[`kaku start --new-tab`](/zh/docs/cli) 往当前窗口加一个标签，`kaku cli split-pane` 拆开当前窗格，还可以在新窗格里跑一条命令：

```bash
kaku cli split-pane -- bash -c "claude --worktree feature-auth"
```

`split-pane` 会打印新窗格的 id，脚本可以把一个 worktree 交给 agent，再等它干完，Kaku 0.29.0 及以上的 `kaku cli wait` 读的是 Claude Code 2.1.295 及以上上报的状态，结束时打印是哪个状态让它停下，超时就以 2 退出：

```bash
pane=$(kaku cli split-pane --right -- bash -c "claude --worktree feature-auth 'add tests for token refresh'")
kaku cli wait --pane-id "$pane" --for done,blocked,error --timeout 30m
```

`config.restore_previous_session` 默认是 true，下次打开会把标签和窗格带回来，窗口、标签或窗格一打开或关掉，一分钟内会把布局存下来，崩溃或者强制退出也在这个范围里。跑着 Claude Code 的窗格回来时还留着最后一屏，能看出对话停在哪里。Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用任何设置，Claude Code 在等你、做完了或者失败了，后台标签上都会出现一个点，更早的版本把[通知渠道](/zh/blog/claude-code-notifications)设成 `ghostty`，等你和做完这两种状态也会点亮同一个点。Kaku 0.29.0 及以上按 `Cmd + J` 会跳到下一个亮着点的标签，并切到点亮它的那个窗格，一个 worktree 一个 agent 时，正在等你的那个一个键就到。

## 常见问题

### 两个 worktree 能检出同一个分支吗？

`git worktree add` 会拒绝已经检出的分支，`--force` 可以强行检出，`-b` 和 `claude --worktree` 都是新建一条分支，不会再去占用已经检出的那条。

### 新 worktree 里为什么没有 `.env`？

检出里只有已跟踪的文件，写在 `.gitignore` 里的 `.env` 只有 `.worktreeinclude` 写了它才会复制过去，而且只对 Claude Code 用 git 建出来的工作树生效。

### Failed to resolve base branch "HEAD" 是什么意思？

仓库里还一次提交都没有，先在主工作树里提交一次，再跑 `claude --worktree`，这条报错记在[常见工作流程](https://code.claude.com/docs/zh-CN/common-workflows)里。

### 删掉 worktree 会把分支也删掉吗？

Claude 自己的删除会把目录和分支一起删掉，`git worktree remove` 会留下分支，所以先从主工作树把分支合并进来，锁还留着的话，用 `git worktree unlock` 解开。

### isolation 会拦住写进主工作树的每一次写入吗？

主工作树上的 `Edit`、`Write` 和 `NotebookEdit` 会被拦住，在那里跑的 shell 命令，以及把 git 指过去的命令也会被拦住，`cp` 进主工作树不在这些检查里。
