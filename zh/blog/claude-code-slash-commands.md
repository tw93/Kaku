# Claude Code 斜杠命令：常用命令和自定义命令写法

> Claude Code 常用斜杠命令、配套快捷键，以及带参数的自定义命令写法和三个可抄的例子。

发布于 2026-10-22 · Tw93 · https://kaku.fun/zh/blog/claude-code-slash-commands

在 Claude Code 会话里输入 `/` 就能看到所有命令，接着打字母可以过滤。日常最常用的是管上下文的 `/clear` 和 `/compact`，管模型的 `/model` 和 `/effort`，管会话的 `/resume` 和 `/rewind`，看费用的 `/usage`，管规则的 `/permissions` 和 `/hooks`，管 `CLAUDE.md` 的 `/init` 和 `/memory`，以及管服务器的 `/mcp`。想加自己的命令，就把一个 Markdown 文件存成项目里的 `.claude/commands/<name>.md`，或者所有项目通用的 `~/.claude/commands/<name>.md`，Claude Code 会把它变成 `/<name>`。自定义命令后来已经并进了 skill，新写的东西更适合用 skill 的格式。

## 值得记住的命令

下面这份清单对照的是[命令参考](https://code.claude.com/docs/en/commands)（2026 年 10 月 19 日读的）。参考页有一百多条，不是每个命令对每个用户都会出现，取决于平台、套餐和环境。命令只有在放在消息开头时才会运行，后面的文字成为它的参数。

| 命令 | 作用 |
|---|---|
| `/clear [name]` | 用空上下文开一个新对话，带名字可以在 `/resume` 选择器里标记旧对话 |
| `/compact [instructions]` | 把对话总结一遍来释放空间，后面的说明告诉它保留什么 |
| `/context` | 用网格显示什么在占用上下文窗口，并给出建议 |
| `/model [model]` | 切换模型并存为新会话的默认值，在选择器里按 `s` 只对当前会话生效 |
| `/effort [level]` | 设置 effort 级别，从 `low` 到 `max`，或 `auto` |
| `/resume [session]` | 按名字或 ID 回到之前的对话，或打开选择器 |
| `/rewind` | 把代码和对话回滚到某个检查点，别名是 `/checkpoint` 和 `/undo` |
| `/usage` | 会话费用、套餐限额和活动，`/cost` 和 `/stats` 是别名 |
| `/permissions` | 查看和编辑 allow、ask、deny 规则，见[权限](/zh/blog/claude-code-permissions) |
| `/memory` | 编辑 `CLAUDE.md`，开关自动记忆 |
| `/init` | 给项目生成一份起步的 `CLAUDE.md` |
| `/mcp` | 管理 MCP 服务器，有 `reconnect`、`enable`、`disable` 几种写法，见 [MCP 服务器](/zh/blog/claude-code-mcp-servers) |
| `/hooks` | 查看 hook 配置，见 [hooks](/zh/blog/claude-code-hooks) |
| `/plan [description]` | 进入计划模式，可以直接带上任务，见[计划模式](/zh/blog/claude-code-plan-mode) |
| `/diff` | 查看工作区的改动，包括 Claude 的编辑 |
| `/code-review` | 审查当前 diff 或某个 PR 编号，`/review` 是别名，`/security-review` 查安全漏洞 |
| `/btw [question]` | 问一个旁支问题，不加进对话 |
| `/agents` | 只会打印一段提示，让你叫 Claude 创建或管理 subagent，或直接编辑 `.claude/agents/`，见 [subagent](/zh/blog/claude-code-subagents) |
| `/statusline` | 配置状态栏，见[状态栏](/zh/blog/claude-code-statusline) |
| `/doctor` | 检查安装和设置，并给出修复建议 |

其中有两个命令的含义随时间变过。`/agents` 以前会打开一个交互式管理界面，当前版本只打印那段提示，`/cost` 现在只是 `/usage` 的别名，所以如果老教程里的界面你看不到，先查参考页，别急着以为出了问题。

## 搭配命令的快捷键

[交互模式页面](https://code.claude.com/docs/en/interactive-mode)列出了所有快捷键，下面这些能省掉一次打命令：

| 快捷键 | 作用 |
|---|---|
| `Shift+Tab` | 循环切换权限模式，比如 Manual、accept edits、plan 和 auto |
| macOS 上的 `Option+P` | 不清空输入就切换模型 |
| `Esc` | 打断 Claude，已做的工作保留 |
| `Esc` + `Esc` | 清掉输入草稿，输入为空时打开回退菜单 |
| `Ctrl+R` | 反向搜索命令历史 |
| `Ctrl+G` | 用默认文本编辑器编辑提示词 |
| `Ctrl+O` | 切换对话记录视图 |
| `Ctrl+B` | 把正在跑的 Bash 命令和 agent 放到后台，在 tmux 里要按两次 |
| 开头的 `!` | shell 模式，直接跑命令并把输出给 Claude 看 |
| `@` | 文件路径自动补全 |

`Option+P` 要求 Option 键以 Meta 的形式到达终端，Kaku 默认让左 Option 发送 Meta，右 Option 输入组合字符，所以在 Kaku 里不用改任何设置就能用。长提示词想分行，在 Kaku 里按 `Shift+Enter` 或 `Cmd+Enter`。

## 用 Markdown 文件写自定义命令

文档明确说自定义命令已经并进 skill，`.claude/commands/deploy.md` 这个文件和 `.claude/skills/deploy/SKILL.md` 这个 skill 都会创建 `/deploy`，行为也一样，已有的命令文件继续可用。skill 多了放辅助文件的目录和对谁能调用的细致控制，所以新东西用 skill，见 [Claude Code skills](/zh/blog/claude-code-skills)，不过只想存一段提示词的话，命令文件仍然是最短的办法。

名字怎么来：

| 文件 | 命令 |
|---|---|
| `.claude/commands/deploy.md` | `/deploy` |
| `.claude/commands/frontend/component.md` | `/frontend:component` |
| `.claude/skills/deploy/SKILL.md` | `/deploy`，或 `name` 指定的名字 |

作用范围跟着位置走，仓库里 `.claude/` 下的文件对这个仓库的会话生效，提交进去团队也能用，`~/.claude/` 下的文件对这台机器上你所有的项目生效，skill 和命令文件重名时 skill 优先。

命令文件接受和 skill 一样的 frontmatter，只是不含 `name` 和 `paths`，最常用的字段有这些：

- `description` 说明它做什么，显示在 `/` 菜单里。
- `argument-hint` 在自动补全时提示参数，比如 `[issue-number]`。
- `allowed-tools` 让 Claude 在命令运行期间不用询问就能用列出的工具。
- `disable-model-invocation: true` 阻止 Claude 自己调用它，适合有副作用的操作。

skill 文档对 `allowed-tools` 还有两点提醒，命令文件同样适用。一是授权只在执行这条命令的那一轮有效，你发出下一条消息就失效，命令正文倒会一直留在上下文里。二是这个字段不受工作区信任的约束，提交在仓库里的命令文件可以给自己预先批准工具，所以 clone 下来别人的项目，先看看 `.claude/commands/` 里各个文件的 `allowed-tools` 写了什么，再在里面跑 Claude Code。

## 参数

命令名后面写的内容会替换文件里的 `$ARGUMENTS`。要按位置取，`$ARGUMENTS[0]` 或更短的 `$0` 是第一个参数，`$1` 是第二个，多个词的值要用引号括起来，所以 `/migrate "hello world" second` 里 `$0` 就是 `hello world`。文件里如果一个占位符都没有，Claude Code 会在末尾追加 `ARGUMENTS: <你的输入>`，保证 Claude 仍然看到你输入了什么。

写成感叹号加反引号命令的一行，比如 `` !`git diff HEAD` ``，会在 Claude 看到文件之前先运行，并替换成命令的输出，这样模型拿到的是实时数据，不用去猜。

## 三个可以直接抄的命令

`.claude/commands/fix-issue.md`，用 `/fix-issue 123` 运行：

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

`.claude/commands/summarize-changes.md`，用 `/summarize-changes` 运行：

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

`~/.claude/commands/migrate.md`，个人命令，用 `/migrate SearchBar JavaScript TypeScript` 运行：

```markdown
---
description: Migrate a component between frameworks
argument-hint: [component] [from] [to]
---

Migrate the $0 component from $1 to $2. Keep behavior identical and list any
places where the two differ.
```

每个都跑一遍看结果。如果命令没出现在 `/` 菜单里，检查文件名，以及开头的 `---` 是不是文件的第一行，只有那种情况下 Claude Code 才会读 frontmatter，认不出的字段名会被忽略而且不报错。通过 `--add-dir` 加进来的目录里，`.claude/commands/` 下的文件不会被监视，改了要重启会话。

## 常见问题

### 自定义斜杠命令放在哪

项目的放在 `.claude/commands/`，所有项目通用的放在 `~/.claude/commands/`，`.claude/skills/` 下的 skill 目录创建的是同一种命令，也是文档更推荐的格式。

### 斜杠命令和 skill 现在是一回事吗

大体上是，两者都创建一个 `/name` 命令，行为一样，skill 多了辅助文件和调用控制，同名时 skill 优先于命令文件。

### 怎么看到所有命令

输入 `/` 再用字母过滤，或者运行 `/help`，有些命令在你输入完整名字之前不会出现在菜单里。
