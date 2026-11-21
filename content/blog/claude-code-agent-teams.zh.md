2026 年 10 月对照官方文档核对过，Claude Code 的 agent teams 还是实验功能，默认关着，要在 `~/.claude/settings.json` 里把 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` 设成 `1`，或者在 shell 里导出同一个变量。然后在当前这个交互式会话里让 Claude 拉起几个有名字的队友。这个会话自己当 lead，每个队友都是单独的 Claude Code 实例，带着自己的上下文窗口，token 会跟着还在跑的队友数量一起涨。在 Kaku 里，队友出现在输入框下面的进程内面板中，想让每个队友占一个窗格，就把 `teammateMode` 设成 `auto` 或 `tmux`，再在 tmux 里面启动 Claude Code。文档点名的分屏后端是 tmux，以及装了 `it2` 的 iTerm2，Kaku 自己的 `Cmd + D` 分出来的是你控制的第二个窗格。

## 什么时候值得开一队

[subagent](https://code.claude.com/docs/zh-CN/sub-agents) 干完把一份总结交回调用方，队友则直接给 lead 和其他队友发消息，会话里有 Task 工具时，还会从一份共享任务列表里认领工作。文档把队伍放在需要互相核对的活上，比如几个人同时审同一处改动，一个模块拆给不同的人，一个 bug 用互相冲突的假设去追，或者一次改动同时跨前端、后端和测试。

前后依赖很密的活、大家改同一批文件的活、一步接一步的活，留在一个会话里，或者交给 subagent 更合适。你想自己开几个会话、每人一份检出，用 [git worktree](https://code.claude.com/docs/zh-CN/worktrees)，不用 lead 来派任务。

## 把开关打开

把变量写进 `~/.claude/settings.json` 的 `env`，或者在 shell 里导出。没有这个变量时，会话一开始就不会建队伍，不会写队伍目录，Claude 也不会提议或拉起队友。

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

打开之后，平常的委派也会变。Claude 会自己给 subagent 起名字，方便之后发消息，在交互式会话里，只要起了名字，这个 subagent 就会作为队友启动，哪怕你没提过要组队。fork 出来的调用，以及 Agent 工具调用本身带了 `isolation` 的那次，仍然是 subagent。`claude -p` 和 Agent SDK 会话不会拉起队友。

把变量在 `~/.claude/settings.json` 里改成 `0`，下一次被点名的 subagent 就回到 subagent。保存后 `env` 会重新套用，不用新开会话。用户文件里的 `0` 会盖过 shell 里的导出。项目设置、本地设置、`--settings` 传入的配置，以及托管设置，仍然可以把变量设回 `1`，托管设置最后生效。

会话还开着的时候，应该能看到 `~/.claude/teams/session-XXXXXXXX/config.json`，这八个字符是会话 ID 的开头，`members` 里 lead 那条的 agent 类型是 `team-lead`，文件里还有 tmux 窗格 ID 这类运行时状态，下一次状态更新会盖掉你手改的内容。会话结束时，Claude Code 会删掉这个目录。`~/.claude/tasks/session-XXXXXXXX/` 会留下来，直到 `cleanupPeriodDays` 清掉，默认 30 天。项目里的 `.claude/teams/teams.json` 只是普通文件，不当成队伍配置。

## 一队是怎么跑起来的

lead 通过 Agent 工具带上 `name` 来启动队友，Claude Code 不会先问你确不确认。队友会像普通会话一样加载 `CLAUDE.md`、MCP 服务器和 skills，lead 若是用 `--setting-sources` 启动的，队友就用同一份受限来源。lead 的对话历史不会带过去，路径、约束、以及你之后要用来称呼他们的名字，都得写在启动时那段提示里。

每个邮箱是 `~/.claude/teams/{team-name}/inboxes/{agent-name}.json`。写入成功，这条消息才算发出去，磁盘满了或者目录不可写，发送方会收到错误，对端什么也收不到。接收方会被告知这是另一个 Claude 会话发来的，所以队友不能替你批准权限提示，也不能把被拒绝的操作转给另一个队友来绕过检查。

共享任务列表是另一条通道，比较新的模型默认没有，除非你自己打开。从 Claude Code v2.1.268 起，`TaskCreate`、`TaskGet`、`TaskList`、`TaskUpdate` 默认只给 Opus 4.7、Sonnet 4.6、Haiku 4.5 以及更早的模型。更新的模型，或者 Claude Code 不认识的模型 ID，启动前加上 `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` 才会有这些工具。进程内的队友跟着 lead 的会话走，分屏里的队友是自己的进程，由它自己的模型决定有没有这些工具。没有工具时，协调只靠消息。有工具时，依赖还没做完的任务不能被认领，认领时用文件锁，避免两个队友拿走同一项。

一个会话只有一队，只有这个 lead 能再拉队友，lead 的角色也留在这个会话上。进程内队友自己拉起的 subagent 都在前台跑，因为队友的后台工作活不过 lead 进程，定义里写了 `background: true` 会报错，请求里写了 `run_in_background: true` 则会报错，或者改在前台运行。

## 队友在 Kaku 里显示在哪

进程内是默认方式，任何终端都能用。输入框下面的面板里，上下方向键选中一行，Enter 打开那份记录，之后打的字会发给这个队友。Escape 取消选中，正在看记录时，Escape 会打断这个队友当前这一轮。选中的那一行上按 `x` 可以停掉队友，`Ctrl+T` 切换任务列表。

面板里每一行都空闲之后，空闲行过 30 秒会藏起来，下一轮再出现，队友本身还在跑，按名字发一条消息可以把这一行叫回来。同时空闲的队友超过三个时，多出来的会收成一行，比如五个空闲时显示 `2 idle agents`，Enter 可以展开。

分屏模式要求 tmux，或者 iTerm2 加上 [`it2` CLI](https://github.com/mkusaka/it2)，并在 iTerm2 的 Settings、General、Magic 里打开 Python API。文档还写了，VS Code 集成终端、Windows Terminal 和 Ghostty 不支持这种分屏。`tmux -CC` 是文档建议的 iTerm2 入口。

Kaku 0.25.0 给自己启动的 shell 设置 `TERM_PROGRAM=Kaku`，Kaku 也不在文档点名的分屏后端里，所以在 Kaku 里分屏要走 tmux。`teammateMode` 的默认值是 `"in-process"`。`"auto"` 只在你已经处于 tmux 里，或者终端是 iTerm2 且 `PATH` 上有 `it2` 或装了 tmux 时，才改用分屏，其他情况留在进程内。`"tmux"` 要求分屏，再按当前终端在 tmux 和 iTerm2 之间选。`"iterm2"` 用 `it2` 去开 iTerm2 自己的窗格，找不到 `it2` 时会报错，并给出安装命令。`claude --teammate-mode auto` 只覆盖这一次会话。这个参数是实验性的，`claude --help` 里没有。

在 Kaku 里要分屏队友，把 `teammateMode` 设成 `auto` 或 `tmux`，再在 tmux 里运行 `claude`。`which tmux` 用来确认 `PATH`。不带参数运行 `tmux` 会建一个只有一个窗口的会话。Claude Code 退出后若 tmux 会话还在，`tmux ls` 能列出来，`tmux kill-session -t <session-name>` 把它结束。

[AI 编程这一页](/zh/docs/ai-coding) 上的 `Cmd + D` 是垂直分屏，`Cmd + Shift + D` 是水平分屏，`Cmd + Shift + Enter` 放大正在看的窗格。这些是 Kaku 自己的窗格，Claude Code 不会把队友放进去，适合你自己另开的会话，一个标签放一个会话的话，Claude Code 2.1.295 及以上在等你、做完或出错时，后台标签上会亮一个点，Kaku 0.29.0 及以上按 `Cmd + J` 就跳到下一个亮着点的标签。

进程内记录打开时，普通文字和 skills 发给这个队友，内置命令发给 lead。`/compact`、`/clear`、`/rewind` 改的是 lead 的对话，所以会先问你。`/model` 和 `/fast` 在这个视图里不会执行。队友的模型和 fast mode 在启动时就定了，要指定就写在启动提示里，否则在 `CLAUDE_CODE_SUBAGENT_MODEL` 不是 `inherit` 时用这个变量。v2.1.234 起，设置里残留的 `teammateDefaultModel` 会被忽略。

## 让 lead 去调度

在提示里把角色的名字定下来，后面发消息才有稳定的称呼。

```text
Spawn three teammates for this PR: security, performance, and test coverage.
Name them security, perf, and tests. Use Sonnet for each.
```

已经写好的 subagent 类型也可以当队友，只要点名，比如 `Spawn a teammate using the security-reviewer agent type`。定义里的 `skills` 字段不会套到队友上，队友从项目和用户设置里加载 skills。进程内时，定义正文追加到默认系统提示后面，并加上 `SendMessage`。分屏时，正文会换掉默认系统提示。

队友继承 lead 的权限模式，但 `dontAsk` 不继承。lead 若带了 `--dangerously-skip-permissions`，每个队友也同样跳过权限。启动之后可以改某一个队友的模式，启动时不能给每个队友单独指定。权限提示出现在 lead 的会话里。lead 已经在 [plan mode](/zh/blog/claude-code-plan-mode) 时再拉起队友，Claude Code 会在请求一到就在 lead 会话里批准这份计划，之后的编辑和命令仍然走原来的权限提示。

要停掉一个，让 lead 叫那个队友关闭。队友可以拒绝并说明原因，而且会先把当前这次工具调用做完。

## 容易出问题的地方

subagent 和队友出现在同一个面板里，面板上有几行，说明不了队伍已经建好。Claude 拉起来的若是 subagent，就再要一次，并写明要 agent team。上面的 `config.json` 才是核对的地方。

`/resume` 和 `/rewind` 不会恢复进程内的队友。恢复之后，lead 可能还去给已经不在的队友发消息，让它重新拉一批就行。

任务做完了却还停在进行中，依赖它的任务就会一直被挡住，哪怕文件已经改好。让 lead 去改任务状态，或者催一下那个队友。lead 自己先动手改代码时，告诉它等队友做完。

两个队友写同一个文件会互相覆盖，每人分一组不同的文件。常见命令先批准掉，不然每个权限提示都会把 lead 停住。队友碰到错误可能会停在那里，打开它的记录补指示，或者再拉一个来接手。`TeammateIdle`、`TaskCreated`、`TaskCompleted` 这几个 hook 以退出码 2 结束时，会把反馈送回去，并拦住这次转为空闲、这次创建或这次完成。

## token 花在哪

可以从 3 到 5 个队友、每人大约 5 到 6 项任务开始，调研、审查和新功能通常值得多花这些 token，日常的小改动用一个会话更省。

进程内队友的请求不在主对话那一档缓存里，默认保留五分钟，Claude 订阅也一样。把 `subagentPromptCacheTtl` 设成 `1h` 可以保留一小时，API 对这种一小时缓存写入收得更高。这个设置需要 Claude Code v2.1.242 或更新的版本。

## 常见问题

### Kaku 会给每个队友一个窗格吗？

Kaku 里看到的是进程内面板。每个队友一个窗格，需要满足其一：窗格里跑着 tmux 且 `teammateMode` 设成 `auto` 或 `tmux`，或者用装了 `it2` 的 iTerm2。

### `claude --help` 里有 teammate mode 吗？

`--teammate-mode` 是实验参数，`claude --help` 里没有。`claude --teammate-mode auto` 仍然会盖过这一次会话的 `teammateMode`。

### 为什么没有共享任务列表？

比 Opus 4.7、Sonnet 4.6、Haiku 4.5 更新的模型上，Task 工具默认关着，要自己打开，比如启动时带上 `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`。这时队伍靠邮箱消息来协调。
