# Claude Code subagents 是什么，什么时候用

> subagent 和 skill、agent teams 有什么区别，怎么定义一个，适合什么活，以及在上下文和 token 上的代价。

发布于 2026-10-11 · Tw93 · https://kaku.fun/zh/blog/claude-code-subagents

[Claude Code 的 subagent](https://code.claude.com/docs/en/sub-agents) 是一个单独写了系统提示词的帮手，有自己的工具列表和自己的上下文窗口。定义它只要在 `~/.claude/agents/` 或 `.claude/agents/` 里放一个 Markdown 文件，Claude 把任务交给它，它干完只把总结带回来。用它的目的就是让搜索结果、日志和整页整页的文件内容别堆进你的主对话里。

## subagent、skill 和 agent teams 的区别

[skill](/zh/blog/claude-code-skills) 跑在主对话里，是一段可以反复用的流程，Claude 把它加载进你当前所在的上下文。subagent 则是从零开始，看不到你的对话历史，看不到你调用过的 skill，也看不到 Claude 已经读过的文件，Claude 给它写一段委派说明，它照着这段说明干活，最后交回一个结果。两者在两个方向上可以结合，带 `context: fork` 的 skill 会把自己的正文当成一个 subagent 的任务来跑，而写了 `skills` 字段的 subagent 会在启动时把这些 skill 的完整内容预先装进来。

[agent teams](https://code.claude.com/docs/en/agent-teams) 是第三种东西，几个 Claude Code 会话一起干活，其中一个当 lead，队友之间可以直接发消息，还共用一份任务列表。2026 年 10 月核对过，它目前是实验功能，默认关着，要用 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` 打开。打开之后，Claude 委派时只要给 subagent 起了名字，它就会作为队友启动，而不是普通的 subagent。文档对取舍说得很直白，subagent 把总结交回调用方，花得少，每个队友却是一个单独的 Claude 实例，token 消耗随着活跃队友的数量一起涨。

| | 跑在哪 | 能不能看到你的对话 | 交回什么 |
|---|---|---|---|
| Skill | 主对话 | 能 | 不单独交回 |
| Subagent | 自己的上下文窗口 | 不能，只看到 Claude 的委派说明 | 一份总结 |
| Agent team | 各自独立的会话 | 不能 | 队友之间自己协调 |

## 自带的几个 subagent

就算一个都没定义，你其实已经在用 subagent 了，Claude Code 自带下面这几个：

- **Explore**：只读，用来找文件和搜代码，Claude 调它时会指定 quick、medium 或 very thorough 三种细致程度之一。
- **Plan**：只读，在 [plan mode](/zh/blog/claude-code-plan-mode) 里帮 Claude 做调研，让探索的输出留在另一个上下文窗口里，你看方案时主对话还是干净的。
- **general-purpose**：subagent 能用的工具它都有，适合既要探索又要改代码的任务。
- **claude**、**statusline-setup** 和 **claude-code-guide**：Claude 自己会去调的辅助角色，分别兜底杂项任务、处理 `/statusline`、回答 Claude Code 本身的用法问题。

Explore 和 Plan 启动时不带你的 `CLAUDE.md`，也不带 git 状态快照，这样调研又快又便宜，它们还是一次性的，不返回 agent ID，所以 Claude 之后没法让它们接着干。

## 自己写一个

subagent 就是一个带 YAML frontmatter 的 Markdown 文件，必填的只有 `name` 和 `description`，正文会变成它的系统提示词：

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

放在 `~/.claude/agents/` 里每个项目都能用，放在 `.claude/agents/` 里就跟着这个仓库一起提交，两边重名时项目里的那个优先。Claude Code 会盯着这两个目录，改完文件下一次委派就用上新定义，不用重启，唯一例外是会话启动时如果 `agents` 目录原本不存在，那就先建好第一个文件，再重启一次。

值得知道的字段大概是这些：

| 字段 | 作用 |
|---|---|
| `tools` | 允许清单，不写就继承 subagent 能用的全部工具 |
| `disallowedTools` | 禁止清单，先于 `tools` 生效 |
| `model` | `sonnet`、`opus`、`haiku`、`fable`、完整模型 ID，或者 `inherit` |
| `permissionMode` | `default`、`acceptEdits`、`auto`、`dontAsk`、`bypassPermissions` 或 `plan` |
| `maxTurns` | 跑到这么多轮就停下 |
| `skills` | 启动时完整预装的 skill |
| `memory` | `user`、`project` 或 `local`，一个跨对话保留下来的目录 |
| `background` | 设成 `true`，Claude 想放到前台也照样留在后台 |
| `isolation` | 设成 `worktree`，在一个临时的 git worktree 里跑 |
| `hooks` | 只在这个 subagent 运行期间生效的 hook |

多个单词的字段名用 camelCase，必须一字不差，Claude Code 碰到不认识的字段会直接忽略，不报错，没有 `description` 或者 YAML 解析不了的文件也会被悄悄跳过，想知道原因就用 `claude --debug` 启动，原因会写进 debug 日志。

再说 `/agents`，现在的版本里它只会打印一句提示，让你去问 Claude 或者直接改那两个目录，v2.1.197 及更早的版本里它才是一个带 Running 和 Library 两个标签的向导，所以眼下最快的办法是把想要的 subagent 描述给 Claude，告诉它存到哪，再打开它写出来的文件看一遍。

要用的时候，在提示词里点它的名字，Claude 一般就会委派过去，想确保一定是它，就输入 `@` 从补全里选，`claude --agent code-reviewer` 则会让整个会话都以这个 agent 的身份跑，工具、模型和系统提示词都换成它的。

## 用得上的地方

文档里特别挑出来的有三种场景，共同点都是过程输出很多，真正要的答案很短。

**搜索和调研。** 让 subagent 去找某个配置值在哪里被读取，或者让三个 subagent 各管一块，同时去调研 auth、database 和 API 这几个模块，读过的文件都留在它们各自的窗口里，回到你这边的只是一段简短的结论，前提是这几条调研路线互相不依赖。

**代码审查。** 给 reviewer 配 `tools: Read, Glob, Grep, Bash`，它没有 Edit 和 Write，但 Bash 仍能运行改文件的命令，想让它只读，还得限制允许执行的命令。也可以串起来用，先让 reviewer 找性能问题，再让另一个 subagent 去修，中间由 Claude 把相关的发现传过去。

**跑测试和看日志。** `Use a subagent to run the test suite and report only the failing tests with their error messages` 是最典型的例子，几千行测试输出留在 subagent 那边，回到主对话的只有两行失败信息。

要是担心它的改动跟你手上的活撞在一起，`isolation: worktree` 会给它一份单独的仓库副本，默认从你的默认分支拉出来，它什么都没改的话这个 worktree 会自动清掉，整个会话都这样跑的做法写在 [worktree 那篇](/zh/blog/claude-code-git-worktrees) 里。

## 反而碍事的时候

任务需要你来回调整好几轮，或者规划、实现和测试几个阶段共用大量上下文，又或者只是一个很快的小改动，这些情况都留在主对话里更好，subagent 启动时手里只有 Claude 对任务的那段总结，前几轮很可能在重新摸索你早就知道的东西，这部分时间是白花的。`CLAUDE.md` 里有哪条规则必须让它知道，比如 `ignore the vendor/ directory`，委派时就再说一遍，因为设了 `omitClaudeMd` 的 subagent 和自带的 Explore、Plan 都看不到它。

如果只是想问一个对话里已经有的东西，用 `/btw` 更合适，它能看到你完整的上下文，但没有任何工具权限，答案也不会写进历史。

## 成本和上下文

subagent 省的是上下文，不是钱，它自己发请求，跟主对话算在同一份用量额度里，文档也提醒过，开很多个 subagent、每个都交回很详细的结果，照样会把主上下文塞满，而且每个 subagent 跑的时候都在花自己的 token。

能调的地方有这么几处：

- 偏重探索的 subagent 设 `model: haiku`，用户级或项目级里放一个名字叫 `Explore` 的 subagent 会覆盖自带的那个，这样默认的搜索 agent 也能换到更便宜的模型上。
- `description` 写短一些，它们一直待在上下文里给 Claude 做路由用，自定义 subagent 的描述加起来超过 15,000 token，Claude Code 启动时会给出警告。
- 任务确实需要整段对话时用 fork，`/subtask draft unit tests for the parser changes so far` 会启动一个继承你全部历史的 subagent，还能复用父会话的 prompt cache，比一个需要同样背景的新 subagent 花得少。
- 心里有个上限，subagent 最多能往下嵌套三层，同时在跑的默认上限是 20 个，可以用 `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` 改。

## 在 Kaku 里盯着并行的活

后台 subagent 会出现在 Claude Code 输入框下面的面板里，`/tasks` 也能列出来，其中哪个需要权限时，提示会出现在主会话里，并写明是哪个 subagent 在问，这时候主会话就停下来等你了。同时开好几个 Claude Code 会话时，可以每个放进 [Kaku](/zh/docs/ai-coding) 的一个标签，或者用 `Cmd + D` 和 `Cmd + Shift + D` 分屏，正在看的那个用 `Cmd + Shift + Enter` 放大，agent teams 的分屏显示需要 tmux 或 iTerm2，所以在不开 tmux 的 Kaku 里，队友会显示在 Claude Code 自己的那个面板里。

Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用任何设置，更早的版本要在 `~/.claude/settings.json` 里设 `"preferredNotifChannel": "ghostty"`，原因写在 [Claude Code 通知](/zh/blog/claude-code-notifications)那篇。标签上的小点也是这样分的，0.25.0 之前靠 `ghostty` 通道在 Claude Code 等你或做完时亮起，0.25.0 起不用设置就会亮，还多了出错这一种状态，0.29.0 起按 `Cmd + J` 能直接跳到下一个亮着点的标签。

## 常见问题

### subagent 文件放在哪？

放在 `~/.claude/agents/` 里每个项目都能用，放在 `.claude/agents/` 里只给这一个仓库用，两个目录都会递归扫描，建子文件夹没问题，认的是 `name` 字段，跟文件名无关。

### subagent 能看到我的对话吗？

看不到，它拿到的是自己的系统提示词、Claude 写的委派说明、你的 `CLAUDE.md`（Explore、Plan 和设了 `omitClaudeMd` 的除外）、一份 git 状态快照，以及预装的 skill，只有用 `/subtask` 启动的 fork 才会继承对话历史。

### 用 subagent 会让 Claude Code 更便宜吗？

单靠它不会，它能让主上下文保持精简，但每个 subagent 都在同一份用量额度里花自己的 token，真正能把开销降下来的是把搜索交给 `model: haiku`，再让它交回来的总结尽量短。

### 该用 subagent 还是 agent team？

只关心结果时用 subagent，几个队友之间需要互相分享发现、互相挑刺时才用 agent team，前提是你能接受多出来的 token 和那个实验开关，怎么打开、队友在 Kaku 里显示在哪，见 [Claude Code agent teams](/zh/blog/claude-code-agent-teams)。

### Claude 为什么不调我写的 subagent？

Claude 是按 `description` 来路由的，写得具体一些，想让它不问就主动用，可以加上 `use proactively`，再检查文件里 `name` 和 `description` 都在、YAML 能正常解析，实在不行就用 `@` 点名，这样一定会跑它。
