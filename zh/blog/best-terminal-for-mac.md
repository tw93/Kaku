# 2026 年 Mac 上的终端怎么选

> Terminal.app、iTerm2、Ghostty、WezTerm、Warp、Kitty、Alacritty 和 Kaku，按上手成本、配置方式、AI 工具和价格对比，2026 年 10 月核对。

发布于 2026-09-21 · Tw93 · https://kaku.fun/zh/blog/best-terminal-for-mac

Mac 上没有哪个终端对所有人都是最好的，但只要想清楚自己要什么，选起来其实不难，什么都不想装就继续用 Terminal.app，想要所有功能都在设置窗口里点出来就用 iTerm2。Ghostty 快、原生、配置是纯文本，WezTerm 适合想用 Lua 写配置、在每个平台都用同一份的人。Warp 把 agent 直接做进了终端，我做的 Kaku 是在 WezTerm 基础上为 macOS 和 AI 编程命令行工具改出来的，第一次打开就配好了。下面一个个讲它们各自好在哪、差在哪，版本和价格按 2026-10-17 核对，其中 Kaku 的版本按 2026-11-21 更新。

![从想要什么到先试哪个终端：什么都不想装对应 Terminal.app，所有功能都有图形设置对应 iTerm2，快、原生、文本配置对应 Ghostty，Lua、全平台对应 WezTerm，内置 agent 对应 Warp，为 AI 编程工具准备好对应 Kaku](https://kaku.fun/img/blog/mac-terminal-picker.webp)

*从自己想要什么出发，而不是从功能列表出发。*

## 一张表看完

| 终端 | 版本 | 许可证 | 价格 | 配置方式 | 分屏 |
|---|---|---|---|---|---|
| Terminal.app | 2.15 | 闭源，随 macOS 自带 | 免费 | 设置窗口 | 只能分同一个会话 |
| iTerm2 | 3.7.6 | GPL-2.0 或更高版本 | 免费 | 设置窗口 | 支持 |
| Ghostty | 1.3.1 | MIT | 免费 | 文本文件 | 支持 |
| WezTerm | 20240203，之后只有 nightly | MIT | 免费 | Lua | 支持 |
| Warp | 滚动更新 | 客户端 AGPL-3.0 | 有免费档，付费从每月 20 美元起 | 设置窗口 | 支持 |
| Kitty | 0.49.2 | GPL-3.0 | 免费 | 文本文件 | 支持 |
| Alacritty | 0.17.0 | Apache-2.0 | 免费 | TOML 文件 | 不支持 |
| Kaku | 0.30.0 | MIT | 免费 | Lua | 支持 |

## Terminal.app

每台 Mac 都自带，跑几条命令完全够用，macOS 26 给它翻新了一次，加上了 24 位色和 Powerline 字体，但真要整天待在终端里还是会不够。分屏只能把同一个会话显示两遍，设置存在 plist 里而不是文本文件，AI 编程工具能用上的东西也少，比如 Claude Code 在这里只会发一个响铃字符，还得先把提示音关掉才会发。

## iTerm2

iTerm2 有 profile、trigger、密码管理和 shell 集成，`tmux -CC` 还能把 tmux 的窗口变成原生标签。所有东西都在设置窗口里配，刚开始很友好，想放进 dotfiles 仓库管理就麻烦了，它的 AI 对话要另装一个插件才能用。

## Ghostty

Ghostty 有原生的 macOS 界面，用 Metal 做 GPU 渲染，默认值很合理，配置文件就是一行行 `key = value`，Quick Terminal 按一个快捷键就从屏幕顶部滑下来，常见的桌面通知序列也都支持。配置本身没有脚本能力，设置和快捷键之外的逻辑得放到别处去做。

## WezTerm

用 Lua 脚本配置的终端，配置里可以算东西、响应事件、自己拼一个状态栏，macOS、Linux 和 Windows 用同一份配置，还内置了多路复用。问题在发版节奏上，最后一个稳定版是 2024 年 2 月的，之后的修复只出现在 nightly 里，不过开发一直没停。

## Warp

Warp 围绕 block 重新设计了终端，每条命令和它的输出算一个整体，还内置了能替你执行命令的 agent，客户端从 2026 年 4 月起以 AGPL-3.0 开源。登录不是必须的，AI 功能跟着套餐走，有免费档，往上是每月 20 美元的 Build、200 美元的 Max 和每人 50 美元的 Business。想让 agent 就长在终端里的人适合它，自己带 agent 命令行、只希望终端别碍事的人就没那么合适。

## Kitty 和 Alacritty

两个都很快，都靠键盘操作，都用文本文件配置，Kitty 多了标签、分屏、自己的图片协议，以及叫 kitten 的小扩展，Alacritty 则故意保持精简，没有标签也没有分屏，一般和 tmux 搭着用。

## Kaku

Kaku 是 WezTerm 的分支，保留了它的引擎和 Lua 配置，改的是你还没写任何配置时拿到的东西，自带 JetBrains Mono，Kaku Dark 和 Kaku Light 两套主题可以跟着系统深浅色切换。还有一套 zsh 配置，自动建议、语法高亮和 Starship 提示符都有，在 `.zshrc` 里只占两行。给 AI 编程命令行工具准备的部分大多不用设置，`Cmd + Click` 路径会在编辑器里打开到对应那一行，关闭正在跑 agent 的窗格前会先确认，重启之后标签、分屏和最后一屏都会回来。Claude Code 的通知在 Kaku 0.25.0 及以上配 Claude Code 2.1.295 及以上不用任何设置，更早的版本设成 `ghostty` 通道，见[Claude Code 通知](/zh/blog/claude-code-notifications)。它只支持 macOS，是为了把这一个平台做得更贴合。

```bash
brew install --cask kaku
```

## 如果主要用来跑 Claude Code 或 Codex

这里的每个终端都能跑 AI 编程命令行工具，差别要在长时间会话里才看得出来。`Shift + Enter` 能不能换行，agent 停下来时你能不能收到通知，重启之后会话还在不在，[Claude Code 用什么终端最好](/zh/blog/best-terminal-for-claude-code)那篇一项项讲过。

## 常见问题

### Mac 上最好的免费终端是哪个？

除了 Warp 的付费套餐，这里列的都免费，大多数人其实是在三个里挑，要功能就 iTerm2，要快和简单就 Ghostty，想要为 AI 编程工具配好的就 Kaku。

### 2026 年还值得用 iTerm2 吗？

值得，尤其是离不开 `tmux -CC`、trigger 或 profile 的话，它的主要缺点是设置不好纳入版本管理，用起来也比那些 GPU 优先的终端更重一些。

### Ghostty 比 iTerm2 好吗？

Ghostty 上手更快，用着更原生，iTerm2 功能更多，需要 tmux 集成或 trigger 就留在 iTerm2，否则 Ghostty 更省心。

### 用 Warp 必须登录吗？

不用，登录是可选的，不过 AI 功能和套餐要登录后才能用。
