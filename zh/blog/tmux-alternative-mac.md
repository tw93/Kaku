# 在 Mac 上还需要 tmux 吗

> tmux 能做到哪些终端做不到的事，原生分屏和会话恢复已经覆盖了哪些，以及远程工作时为什么还值得留着 tmux。

发布于 2026-10-01 · Tw93 · https://kaku.fun/zh/blog/tmux-alternative-mac

在一台主要自己用的 Mac 上，你很可能已经用不着 tmux 了，大多数人开 tmux 图的是三件事，把一个窗口分成好几个窗格，终端关掉之后活儿还在，以及从别的地方接回同一个会话。第一件可以用原生分屏，会话恢复能帮你找回布局，要让交互式会话在终端退出后继续运行，本地和服务器上都可以用 tmux 这类多路复用器。所以比较实在的答案是，本地分屏想不用 tmux 就不用，SSH 会话和终端重启后还得继续跑的任务，继续留着它。这篇讲清楚这条线划在哪，以及要留着的话怎么把 tmux 配好。

## tmux 到底做了什么

tmux 跑着一个服务端，你的 shell 归它管，终端只是负责把它们画出来的客户端，这个设计带来三样东西：

1. **分屏和窗口**，画在同一个终端窗口里。
2. **持久化**，关掉终端或者 SSH 断了，shell 还在 tmux 服务端里跑，`tmux attach` 原样接回来，程序都还在跑。
3. **共享**，从另一个终端或[另一台机器](/zh/blog/claude-code-remote) attach 上去，看到的是同一个会话。

## 现在的终端已经能做到的

**分屏。** iTerm2、Ghostty、WezTerm、Kitty 和 Kaku 在 macOS 上都有原生分屏，由终端自己来画，滚动、选中、复制都是正常的样子，不用进 tmux 的 copy mode。

**重启之后接着用。** 有些终端会在重启后把窗口、标签和分屏恢复回来，Kaku 多做了一步，把屏幕上的内容也存下来。

![Kaku 的会话恢复：跑着 agent 和 shell 的窗格每 60 秒存进一份快照，经过更新或崩溃导致的重启，恢复成原来的文字，不会重新执行任何东西](https://kaku.fun/img/blog/session-restore-flow.webp)

*恢复回来的窗格显示的是原来的内容，程序本身不会被重新启动。*

Kaku 退出时会保存每个窗口、里面的标签、分屏布局、每个窗格的工作目录，以及每个窗格最多 1500 行内容，运行过程中只要布局有变化，它每分钟也会刷新一次快照，所以就算意外退出，也能留下一份比较新的布局。下次启动时窗格带着原来的文字回到原来的目录，起一个新的 shell，之前跑着 Claude Code 或 Codex 的窗格会保留 agent 的最后一屏，能先看到每个对话停在哪里，再用 `claude --continue` 或 `codex resume --last` 接上。0.28.0 之前，恢复带很长滚动记录的会话后，Kaku 可能过一分钟就退出，重新打开又接着崩，遇到这种情况升级就好，不用把会话恢复关掉。

这就是和 tmux 最重要的区别，Kaku 恢复的是你看到的东西而不是进程，终端退出时正在跑的构建之后就不再跑了，而 tmux 能让它继续跑，因为这个进程本来就不属于终端。

**agent。** 对 AI 编程命令行工具来说，真正要紧的状态是对话，而 Claude Code 和 Codex 都会自己保存对话，终端把布局和最后一屏带回来，就已经解决了大半，剩下的交给 agent 自己的恢复命令。

## 什么时候还要留着 tmux

- **通过 SSH 连的远程机器。** tmux 本来就是为这个做的，在服务器上的 tmux 里跑长任务和 agent 会话，连接断了也什么都不会丢。
- **自己 Mac 上的长进程**，终端重启时也必须一直跑着的，比如一个你从来不想重启的开发服务器。
- **两个人在同一个会话里结对**，一起 attach 到同一个 shell 上。
- **建立在 tmux 快捷键和脚本上的工作流**，用顺手了不想放弃。

## 给 Claude Code 配好 tmux

如果你确实在 tmux 里跑 Claude Code，有三个设置能让它正常工作：

```bash
# ~/.tmux.conf
set -g allow-passthrough on
set -s extended-keys on
set -as terminal-features 'xterm*:extkeys'
```

`allow-passthrough` 让加了 tmux passthrough 包装的序列从可见窗格传到外层终端，另外两个让 `Shift + Enter` 作为单独的按键传进来，而不是变成普通的回车，改完用 `tmux source-file ~/.tmux.conf` 重新加载，这些通知最后是怎么走到 macOS 的，可以看这篇[通知指南](/zh/blog/claude-code-notifications)。

装了 tmux 的话，`kaku init` 还会在 `~/.tmux.conf` 里加一行 `source-file`，指向一个由 Kaku 管理的小文件，它打开鼠标支持，让你从 Kaku attach 之后新开的窗格也保留 Kaku 的提示符，还让 `Shift` 加滚轮直接进入 tmux 的回滚。上面那三个设置还是要你自己加。

## 折中的用法

不一定要二选一，比较常见的做法是本地全部用原生分屏和会话恢复，只在 SSH 上去的服务器里用 tmux，在 Kaku 里就是 Mac 上用 `Cmd + D` 和 `Cmd + Shift + D` 分屏，远程用 `ssh server -t tmux new -A -s main`，每次都落到同一个远程会话里。

## 常见问题

### Kaku 能替代 tmux 吗？

本地分屏和重启后找回布局，大体上可以，但它不会让进程在终端退出后继续跑，所以 SSH 会话和长时间运行的任务还是留给 tmux。

### Kaku 里能用 tmux 吗？

能，Kaku 和其他终端一样能跑 tmux，Kaku 管理的那个 tmux 文件还会加上鼠标支持，并让从 Kaku 里启动的 tmux 会话也有 Kaku 的提示符。

### SSH 断了怎么让 Claude Code 继续跑？

在远程机器上的 tmux 里启动它，比如先 `tmux new -A -s claude` 再跑 `claude`，重新连上之后 `tmux attach -t claude` 就能接回来。

### 能关掉 Kaku 的会话恢复吗？

能，在 `kaku.lua` 里设 `config.restore_previous_session = false`，保存和恢复会一起关掉。
