人不在跑着 Claude Code 的那台 Mac 旁边时，想接着干活大概有三种办法，Anthropic 的 Remote Control 让你在 claude.ai/code 或手机上的 Claude App 里操作 Mac 上的会话，云端会话干脆把活挪到 Anthropic 的机器上，家里什么都不用开着，`kaku remote` 则是在另一台电脑的浏览器里打开 Mac 的终端，所有标签和分屏都在。内容按 2026 年 10 月的 Claude Code 2.1.302 核对过。

## 先看结论

| | Remote Control | 云端会话 | kaku remote |
|---|---|---|---|
| Claude 跑在哪 | 你的 Mac | Anthropic 的机器 | 你的 Mac |
| 你看到的 | Claude 的对话 | Claude 的对话 | 整个终端 |
| 在哪用 | claude.ai/code、Claude App | claude.ai/code、Claude App、命令行 | 电脑或手机上的浏览器 |
| 需要什么 | Claude 付费套餐，Mac 醒着 | Claude 付费套餐，GitHub 仓库 | Kaku 开着，Mac 醒着 |
| 其他程序 | 看不到 | 看不到 | 都在，shell、编辑器、别的 agent |

## Remote Control，手机上接着同一个会话

Remote Control 让 Claude Code 继续在你的 Mac 上跑，用的还是你的文件、工具和凭据，你只是在 claude.ai/code 或 Claude 手机 App 里看它、回它，开会话时把它打开就行：

```bash
claude --remote-control
```

也可以在项目里跑 `claude remote-control`，之后从 claude.ai/code 或 App 里新开的会话都会跑在这台 Mac 上，`claude remote-control --continue` 会接回这个目录上次开的那个。它要求 Claude 付费套餐，用 claude.ai 账号登录，API key 不支持，而且你想连多久，Mac 就得醒着联网多久。

活儿离不开这台机器的时候用它最合适，比如要连本地数据库、要走内网、构建只能在这台机器上跑，你在手机上要做的只是批准一步，或者发下一条指令。

## 云端会话，什么都不用开着

云端会话把 Claude Code 跑在 Anthropic 提供的虚拟机上，一般是对着一个 GitHub 仓库干活，在没有 git remote 的仓库里，或者没装 Claude GitHub App 的仓库里跑 `claude --cloud`，它会把本地仓库打包传上去，不过开 pull request 还是要 GitHub，可以从 claude.ai/code 或 App 里开，也可以在终端里开：

```bash
claude --cloud "fix the flaky login test"
```

因为它不跑在你的 Mac 上，合上笔记本它也不会停，想回到本地接着做，`claude --teleport` 会把云端会话拉回你的终端。代价在环境上，会话能看到的是仓库，不是你的机器，只在本地才有的东西它都碰不到。

## kaku remote，浏览器里的整个终端

上面两种看到的都是 Claude 的对话，可有时候你需要的是终端里的其他东西，比如旁边分屏里开发服务器的日志、另一个标签里的 Codex，或者一个随手查东西的 shell，`kaku remote` 则是直接在另一台电脑的浏览器里打开终端本身。

[[figure:remote-access-architecture|另一台电脑上的浏览器经过中继连到你 Mac 上的 Kaku，你输入的和看到的内容从浏览器到 Kaku 全程加密|中继只负责转发连接，读不到里面的内容。]]

打开远程访问并配对一个浏览器：

```bash
kaku remote
```

Kaku 会先问一句再打开远程访问，然后给出一个配对链接，在要配对的那个浏览器里打开它，确认页面和终端里显示的六位数字一样，再回到终端按 Enter 放它进来，按 Ctrl+C 就是拒绝，链接只能用一次，10 分钟后失效，所以每台设备都要重新跑一次 `kaku remote`，配对之后把页面加进书签，下次直接回来，不用新链接。

你输入的和看到的内容在这个浏览器和 Kaku 之间是加密的，中间的中继只负责转发，新浏览器非得有人在 Mac 前按下这个 Enter 才进得来，链接就算随截图或者滚动记录漏出去也没用，已经配对的浏览器隔了 30 分钟以上再连回来，Kaku 会在 Mac 上发一条通知，访问管理都在 Mac 这边：

```bash
kaku remote devices        # 列出已配对的浏览器
kaku remote revoke <id>    # 移除一个，它打开的会话几秒内结束
kaku remote off            # 关掉远程访问
kaku remote --reset        # 移除所有已配对的浏览器
```

这个页面在电脑浏览器里能用，Kaku 0.29.0 起手机上也有了自己的布局，标签收进侧边抽屉，每个 agent 的话题和状态都列在里面，快捷键栏里有 Esc、方向键、Tab、Shift + Tab 和 Ctrl + C，键盘只从这一栏最前面的键盘按钮弹出来，滑动终端时不会再弹。配对没有二维码，链接得自己弄到手机上，比如用隔空投送发过去再打开，和 Remote Control 一样，它连的是你真正的那台 Mac，所以 Mac 要醒着，Kaku 要开着。

## 该用哪个

- **在手机上批准一步：** Remote Control。
- **合上笔记本也要继续跑的活：** 云端会话。
- **在另一台电脑上用这台 Mac 的终端，agent 和其他东西都要：** `kaku remote`。

它们也能搭着用，在 Kaku 里用 `claude --remote-control` 开的会话，白天在手机上回几句，晚上换到另一张桌子前，在浏览器里接着用整个终端。

## 常见问题

### 能在手机上用 Claude Code 吗？

可以，在 Mac 上用 `claude --remote-control` 开会话，再到 Claude App 或 claude.ai/code 里打开它，云端会话也能在 App 里开和跟进，想用整个终端而不只是一段对话，就在手机浏览器里打开配对过的 `kaku remote` 页面，Kaku 0.29.0 起它在手机上有专门的布局。

### 合上笔记本之后 Claude Code 还会跑吗？

本地会话包括开了 Remote Control 的，都会在 Mac 睡眠时停下，云端会话跑在 Anthropic 的机器上，会继续跑。

### 能不能直接 SSH 到 Mac 上跑 Claude Code？

可以，前提是你配好了 SSH，也有办法连到这台 Mac，最好在 tmux 里跑，断线了会话还在，要让[通知](/zh/blog/claude-code-notifications)通过 tmux passthrough 回到你连过去的终端，发送端得给序列加上包装，tmux 得允许它通过，外层终端也得支持对应的通知通道。

### kaku remote 能用 Codex 和别的命令行工具吗？

能，它显示的是终端本身，Kaku 标签里不管跑着 Claude Code、Codex、编辑器还是 shell，浏览器里就是什么。
