Claude Code 在哪个终端里都能跑，只要能显示现代的 TUI 就行，所以真正值得问的不是哪个终端能跑，而是一个会话跑上几个小时的时候，哪个终端最不碍事。决定这件事的其实就五样，长提示词能不能分行写完再发，Claude 停下来等你的时候你能不能知道，它打印出来的文件和链接能不能直接点开，终端重启之后会话还在不在，以及会不会手滑一个 `Cmd + W` 就把它关了。Ghostty 和 iTerm2 大部分都做得不错，我做的 Kaku 支持这五样，0.25.0 及以上配 Claude Code 2.1.295 及以上，通知也不用任何设置。

[[figure:claude-code-session-loop|Claude Code 会话的循环，写提示词，Claude 开始干活，它提问或做完，你回来看结果并打开文件，终端重启后会话还在|终端会在这五个地方帮上忙，或者添乱。]]

## 提示词要能分行写

给 agent 的提示词很少只有一行，常常是先贴一段报错，再补一个文件路径，最后才写要它做什么，写到一半按了回车，发出去的就是半句话。

Claude Code 自己的文档列了哪些终端里 `Shift + Enter` 不用设置就能换行，包括 Ghostty、Kitty、iTerm2、WezTerm、Warp、Apple Terminal 和 Windows Terminal，VS Code、Cursor、Zed 以及 0.16 之前的 Alacritty 需要先跑一次 `/terminal-setup`。还有两个到哪都能用的办法，`Ctrl + J`，或者先打一个反斜杠再回车。

在 Kaku 里 `Shift + Enter` 和 `Cmd + Enter` 都是换行，不用提前跑任何命令。

## Claude 在等你的时候要能知道

Claude 问了一个问题，你却在另一个窗口里，桌面通知能让你及时回来，前提是终端和 Claude Code 对怎么发通知有共识。

Claude Code 的 `preferredNotifChannel` 默认是 `auto`，而 `auto` 只会在 iTerm2、Ghostty 和 Kitty 里发桌面通知，在 Terminal.app 里是发一个响铃字符，还得是关掉了提示音才发，其他终端里它什么都不做，这就是为什么用 WezTerm、Warp 或者 Claude Code 不认识的终端时，一条通知都收不到。

Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上版本不用任何设置，Claude Code 会告诉 Kaku 自己在等你还是已经跑完，Kaku 再把它变成通知。更早的版本里，Kaku 能识别 Claude Code 发给 Ghostty 的那段序列，在 `~/.claude/settings.json` 里加一行就行：

```json
{ "preferredNotifChannel": "ghostty" }
```

通知只在 Kaku 处于后台时出现，正在看的那个窗格不会再弹一次，如果一条都收不到，点 Kaku > Notification Settings 把 Allow Notifications 打开。

## 它打印出来的东西要能点开

Claude Code 会打印文件路径，常常还带着行号，也会给出文档或者 PR 的链接，能不能直接点开主要看两个细节。

第一个是 Claude Code 运行时会占用鼠标，很多终端里这时普通点击会被它收走，链接打不开，Kaku 里 `Cmd + Click` 即使在程序占着鼠标时也能打开链接和路径。

第二个是像 `src/main.rs:42:7` 这样的路径，应该直接用编辑器打开到那一行，Kaku 会把本地路径连同行号列号交给你设置的编辑器：

```lua
config.file_link_editor = "zed"
-- config.file_link_editor = "cursor --goto"
```

没设置的话，找得到 VS Code 就用 VS Code，再往后是 `$VISUAL` 和 `$EDITOR`。

## 重启之后会话还要在

agent 会话动辄跑几个小时，这期间终端难免要重启，哪怕只是装个更新，这里分两层。对话本身由 Claude Code 保存，`claude --continue` 会接上当前目录里最近的那次对话，对话之外的东西得靠终端，开着哪些标签和分屏，每个窗格在哪个目录，屏幕上原来是什么。

Kaku 运行时会定时保存窗口布局，再启动时把标签和分屏都恢复回来，之前在跑 Claude Code 或 Codex 的窗格还会保留最后一屏，接着聊之前能先看到每个对话停在哪里，更新本身半路断掉也没关系，0.28.0 起用 Restart to Update 更新时，就算中途被打断，应用程序文件夹里也总会留着一个能用的 Kaku.app。

另一半是别误关，Kaku 里关闭前台在跑 Claude Code、Codex、编辑器或者构建任务的窗格和标签时会先确认一下，普通的 shell 就直接关，不多问。

## 同时跑好几个

一个会话顺了之后，下一步往往是两个，一个 agent 在重构，另一个在补测试，或者一边跑 agent 一边留个 shell 跑应用，这就要靠分屏。Kaku 里 `Cmd + D` 向右分，`Cmd + Shift + D` 向下分，想细看哪个窗格就按 `Cmd + Shift + Enter` 放大，再按一次还原。

## 在另一台电脑上接着用

有时候会话跑在桌上那台 Mac 里，人却不在旁边。Anthropic 自己提供了 Remote Control，付费套餐可以在 claude.ai 或 Claude App 里操作一个正在跑的 Claude Code 会话，还有跑在 Anthropic 机器上的云端会话。Kaku 这边是 `kaku remote`，可以在另一台电脑或手机的浏览器里打开这台 Mac 的终端，正在跑 Claude Code 的窗格也都在里面，配对方法见[命令参考](/zh/docs/cli#cli-kaku-remote)。

## 常见终端对比

以下按 2026-10-17 的 Claude Code 终端文档核对。

| 终端 | Shift + Enter 换行 | 默认设置下的 Claude Code 通知 |
|---|---|---|
| Terminal.app | 可以 | 只发响铃字符，且要关掉提示音 |
| iTerm2 | 可以 | 可以，需在设置里打开转义序列提醒 |
| Ghostty | 可以 | 可以 |
| Kitty | 可以 | 可以 |
| WezTerm | 可以 | 不行，要设 `terminal_bell` 或加 hook |
| Warp | 可以 | 不行，要设 `terminal_bell` 或加 hook |
| Kaku | 可以，`Cmd + Enter` 也行 | 可以，0.25.0 配 Claude Code 2.1.295 及以上，更早的版本设成 `ghostty` 通道 |

Kaku 给 Claude Code、Codex 这些 AI 编程工具准备的东西，从 `kaku ai` 到重启后恢复会话，都收在文档的 [AI 编程工具](/zh/docs/ai-coding)那一页。

## 常见问题

### 为什么收不到 Claude Code 的通知？

默认的 `auto` 通道只给 iTerm2、Ghostty 和 Kitty 发通知，其他终端都要在 `~/.claude/settings.json` 里设好 `preferredNotifChannel`，只有 Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用设。每个终端该设成什么，tmux 里要加哪一行，[Claude Code 通知](/zh/blog/claude-code-notifications)那篇都列了。

### 还能在 tmux 里用 Claude Code 吗？

可以，tmux 里要让 Shift + Enter 生效需要 `set -s extended-keys on` 和 `set -as terminal-features 'xterm*:extkeys'`，通过 tmux passthrough 发通知时，还需要 `set -g allow-passthrough on`，发送端也得给序列加上 tmux 的包装。
