# Claude Code 通知不弹出，在 Mac 上怎么修

> 为什么 Claude Code 和 Codex 在大多数终端里不发通知，每种情况该改哪个设置，以及终端通知序列是怎么走到 macOS 的。

发布于 2026-09-20 · Tw93 · https://kaku.fun/zh/blog/claude-code-notifications

如果 Claude Code 做完了或者在等你授权，却从来不提醒你，可以先看通知通道，它默认只在 iTerm2、Ghostty 和 Kitty 这三个终端里发桌面通知，其他终端里你不告诉它用哪个通道，它就一声不吭，只有 Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用任何设置。Codex 的默认行为正好相反，它会响终端铃声，有的终端会把铃声变成通知，有的只闪一下。这篇讲每种情况该改哪个设置，两者都会被卡住的 macOS 权限，以及从转义序列到通知中心中间发生了什么。

## Claude Code 的快速修法

打开 `~/.claude/settings.json`，把 `preferredNotifChannel` 设成你的终端认识的通道，Claude Code 接受的值有 `auto`、`terminal_bell`、`iterm2`、`iterm2_with_bell`、`kitty`、`ghostty` 和 `notifications_disabled`。

| 终端 | 怎么设 |
|---|---|
| iTerm2 | Claude Code 里不用改，在 iTerm2 的 Settings > Profiles > Terminal 里勾上 Notification Center Alerts，再在 Filter Alerts 里打开 Send escape sequence-generated alerts |
| Ghostty、Kitty | 不用改，`auto` 就能用 |
| Kaku | 0.25.0 及以上配 Claude Code 2.1.295 及以上不用改，更早的版本设成 `"ghostty"` |
| Terminal.app | `auto` 只在关掉提示音时发响铃字符，`/terminal-setup` 会帮你关掉 |
| WezTerm、Warp、VS Code | `"terminal_bell"`，或者加一个 Notification hook |

Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上版本什么都不用改，再设成 `ghostty` 反而会多出一条重复通知，更早的版本只需要改这一行：

```json
{ "preferredNotifChannel": "ghostty" }
```

版本用 `claude --version` 看，stable 渠道比 latest 晚几个版本，Homebrew 的 `claude-code` cask 跟的就是它，2026 年 10 月中旬还停在 2.1.292，所以用 stable 的话先留着这一行，等它到 2.1.295 再删。

## Codex 的快速修法

Codex 在 Kaku 里不用改，它默认需要你的时候会响终端铃声，Kaku 会把后台窗口里的铃声变成一条通知，只是内容比较笼统，取的是上一条命令或者窗格标题。想看到 Codex 自己写的消息，就在 `~/.codex/config.toml` 里换成 OSC 9 序列：

```toml
[tui]
notification_method = "osc9"
```

`notification_method` 可以是 `auto`、`osc9` 或 `bel`，旁边还有两个值得知道的设置，`notifications` 控制开关，也可以只对某几类事件开，`notification_condition` 决定只在终端没获得焦点时提醒，也就是默认值，还是任何时候都提醒。

Kaku 里 Claude Code 和 Codex 的其他设置，比如 `kaku ai`、几个 agent 分屏并排跑、重启后会话还在，写在 [AI 编程工具](/zh/docs/ai-coding)那一页。

## 一条都收不到的时候

设置没问题还是什么都没有，下一个要怀疑的就是 macOS。终端发出的通知是由终端这个 App 来显示的，所以得给终端本身授权，在系统设置 > 通知里找到你的终端，打开允许通知，Kaku 里点 Kaku > Notification Settings 就能直接跳到这一页。

这个权限 macOS 只问一次，如果哪次点了不允许，之后它不会再问，后面的每条通知都会被悄无声息地丢掉。Kaku 会检查这种情况，有通知因此被丢掉时，下次你切回 Kaku 会提示一句。

还有两个常见原因：

- **tmux。** 通过 tmux passthrough 发送通知时，要在 `~/.tmux.conf` 里加上 `set -g allow-passthrough on`，再用 `tmux source-file ~/.tmux.conf` 重新加载，发送端也得给序列加上 tmux 的 passthrough 包装，`on` 只对可见窗格生效。
- **你正看着它。** 大部分终端包括 Kaku 都不会对眼前这个窗口弹通知，Codex 默认也只在终端没获得焦点时提醒，测试时记得把窗口放到后台。

## 通知是怎么从 agent 走到 macOS 的

Claude Code 和 Codex 都不直接跟通知中心打交道，它们往终端里写一小段转义序列，跟输出颜色是一个路子，接下来怎么处理由终端决定。

![Claude Code、Codex 和你自己的脚本把序列或铃声发给 Kaku，Kaku 只在处于后台时把它变成 macOS 通知](https://kaku.fun/img/blog/claude-code-notifications.webp)

*不管从哪来，最后都是一段终端读得懂的序列，再由终端变成通知。*

常用的序列有几种，OSC 9 最早出自 iTerm2，只带一行文字，OSC 777 带标题和正文，单独一个铃声字符则什么内容都不带，只表示有事发生。Ghostty 和 Kaku 两种都认，Kitty 用的是自己的 OSC 99，各家不统一，这也是为什么 Claude Code 要维护一串通道而不是一个开关。Kaku 0.25.0 及以上还认 OSC 7501，也就是 Claude Code 2.1.295 及以上在等你、做完或出错时发出的状态报告，所以不用设通道，同一份报告还会在后台标签上点亮一个点，Kaku 0.29.0 及以上按 `Cmd + J` 就能跳到下一个亮着点的标签。

在 Kaku 里，OSC 9 和 OSC 777 的通知要经过 `notification_handling` 这项设置：

```lua
-- 默认：只在 Kaku 窗口处于后台时提醒
config.notification_handling = "AlwaysShow"
-- 没在看的标签或窗格也提醒：
-- config.notification_handling = "SuppressFromFocusedTab"
-- config.notification_handling = "SuppressFromFocusedPane"
-- 从不提醒：
-- config.notification_handling = "NeverShow"
```

铃声不经过 `notification_handling`，只要 Kaku 窗口在后台，它就一定会变成通知，还会累加到 Dock 图标的角标上，通知消失了也能看出刚才有东西响过。

## 给自己的脚本加通知

既然通知只是一段序列，任何脚本都能发，长时间的构建或测试跑完时提醒一下很好用：

```bash
# 一行文字（OSC 9）
printf '\e]9;%s\a' "Tests finished"

# 标题加正文（OSC 777）
printf '\e]777;notify;%s;%s\a' "Build" "Release build finished"
```

这两个例子放在 tmux 外面跑，测试时把终端放到后台，要在 tmux 里用，还得给序列加上 passthrough 包装，并打开上面那项设置。

## 常见问题

### 为什么 Claude Code 在 iTerm2 里有通知，在我的终端里没有？

`preferredNotifChannel` 保持 `auto` 时，Claude Code 只给 iTerm2、Ghostty 和 Kitty 发桌面通知，把通道明确设上就行，0.25.0 之前的 Kaku 里用 `ghostty`，其他终端可以用 `terminal_bell`。

### 通过 SSH 跑的 Claude Code 也能通知吗？

可以，序列会沿着 SSH 连接回到你 Mac 上的终端，由它弹出通知，前提是这个终端认识你选的通道。

### 能不能在 Claude 等我的时候也放个声音？

可以，在 `~/.claude/settings.json` 里加一个 [Notification hook](https://code.claude.com/docs/en/terminal-config#play-a-sound-with-a-notification-hook)，它会和自带的通知同时运行：

```json
{
  "hooks": {
    "Notification": [
      { "hooks": [{ "type": "command", "command": "afplay /System/Library/Sounds/Glass.aiff" }] }
    ]
  }
}
```

### 为什么正在看的窗口也会弹通知？

有些终端每条都弹，Kaku 默认的 `AlwaysShow` 只在窗口处于后台时提醒，`Suppress` 开头的几个选项会把没在看的标签和窗格也算进去。
