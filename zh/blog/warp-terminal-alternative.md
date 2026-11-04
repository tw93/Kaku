# Warp 终端的替代品，Mac 上怎么选

> 人们为什么离开 Warp、它仍然擅长什么，以及六个替代终端在 GPU、配置、AI 和许可证上的差别。

发布于 2026-11-04 · Tw93 · https://kaku.fun/zh/blog/warp-terminal-alternative

想离开 Warp，哪个替代品最合适要看你为什么想换。只想要一个没有账号、没有内置 AI 的普通终端，选 Ghostty 或 iTerm2，想用 Lua 来写配置，选 WezTerm，想要第一次打开就能用、AI 部分交给你自己的 Claude Code 或 Codex 去做，那就是我做的 Kaku。如果你要的恰恰是终端里自带的 agent 和 blocks，Warp 仍然更合适。这篇讲 Warp 擅长什么、人们为什么离开它，以及六个替代品在通常起决定作用的几件事上怎么比，事实都来自各个项目自己的页面，2026 年 10 月读的。

## Warp 擅长什么

Warp 在官网上把自己的终端称作[为 AI 辅助软件开发做的开源终端](https://www.warp.dev/)，它的客户端仓库把它描述成 agentic development environment。它和传统终端有两处不同。

第一处是 [blocks](https://docs.warp.dev/terminal/blocks)，每条命令和它的输出被分在一个单元里，可以复制、搜索、过滤、加书签、分享，还能在单元之间跳转，在普通终端里这段输出只是一长串滚动的文字。第二处是内置的 agent，仓库里也说你可以带上自己的 CLI agent，比如 Claude Code 或 Codex。如果你就是冲着这两样来的，下面没有一个替代品复制了它们，留在 Warp 是对的。

## 人们为什么换

下面每一条都对照过 Warp 自己页面现在的说法。

- **账号。** 快速上手文档说注册入口在右上角，也可以跳过直接开始用，所以打开终端不需要登录。
- **遥测。** Warp 的隐私文档说，分析数据有开关，在设置的 Privacy 里关掉 Help improve Warp，关掉遥测之后控制台交互不会存到它的服务器上，同一页还说免费计划里要使用 AI 功能必须开着遥测，所以关掉要付出的代价是那边的 AI 功能。Business 和 Enterprise 计划被描述为受零数据留存协议覆盖。付费计划关掉遥测也照样能用 AI。崩溃报告是另一个开关，叫 Send crash reports，走的是 Sentry。想看 Warp 往外发了什么，在命令面板里搜 Show Warp Network Log，它会插入一条 `tail -f` 网络日志的命令，不过文档也说了，崩溃报告的流量不会出现在这份日志里。
- **AI 是产品的一部分。** Warp 的 agent 是内置的，它的价格页在免费计划之外另卖 agent 用量，如果你本来就在用 Claude Code 或 Codex，内置的 agent 可能一直用不上。
- **它不是一个普通终端。** Warp 把输出分成 blocks，还用自己的输入编辑器顶替了 shell 提示符的位置，想让终端别挡着 [Claude Code](/zh/blog/best-terminal-for-claude-code) 这类程序的话，传统终端更简单。

闭源已经不在理由里了，[warpdotdev/warp 仓库](https://github.com/warpdotdev/warp)写明客户端代码是开源的，UI 框架的几个 crate 是 MIT，其余是 AGPL v3，README 讲的是客户端，没说服务端是不是开源。

## 替代品

| 终端 | GPU 渲染 | 配置 | AI 功能 | 许可证 |
|---|---|---|---|---|
| Warp | macOS 上用 Metal | 设置文件和设置页 | 内置 agent，或带自己的 CLI agent | AGPL v3，UI crate 为 MIT |
| iTerm2 | 可选的 GPU 渲染器 | 设置窗口 | 可选的 AI 插件，用自己的 API key | GPL v2 或更高版本，实际按 v3 |
| Ghostty | macOS 上用 Metal | 文本文件，`key = value` | 没有内置 | MIT |
| WezTerm | GPU 加速 | Lua | 没有内置 | MIT |
| Kitty | 基于 GPU | `kitty.conf` | 没有内置 | GPL-3.0 |
| Alacritty | OpenGL | `alacritty.toml` | 没有内置 | Apache-2.0 |
| Kaku | WezTerm 的渲染器 | Lua，`~/.config/kaku/kaku.lua` | 可选的助手，接你自己的服务商 | MIT |

每个简单说几句：

**iTerm2** 是老牌的 Mac 终端，仓库里的 COPYING 文件写的是 GPL v2 或更高版本，因为依赖了 Apache-2.0 的库，实际按 GPL v3。它的文档写到有 GPU 渲染器，能提升绘制性能但可能更耗电，还有一个 AI 插件，API key 要自己提供，已经有靠得住的 profile 的话就留着。

**Ghostty** 是 MIT 协议，在 macOS 上用 Metal，读 `~/.config/ghostty/config.ghostty` 或 `~/Library/Application Support/com.mitchellh.ghostty/config.ghostty` 里的 `key = value` 文件，用 `Cmd + Shift + ,` 重新加载。它自己没有 AI，适合带自己 agent 的人。

**WezTerm** 是用 Rust 写的跨平台终端和多路复用器，用 Lua 配置，窗格、标签页和窗口在本地和远程主机上都能用，适合喜欢给终端写脚本的人。

**Kitty** 基于 GPU，有自己的 `kitty.conf`，可以用脚本远程控制，还有叫 kittens 的 Python 扩展。

**Alacritty** 刻意做得很小，它的 README 说你在里面找不到标签页和分屏，这些交给窗口管理器或 tmux，想要的话配合 [tmux](/zh/blog/tmux-alternative-mac) 用。

**Kaku** 是基于 WezTerm 的 macOS 终端，字体、主题、shell 集成和 Mac 快捷键都已经配好，同时保留了 Lua 配置。它没有账号，AI 是可选的，`kaku ai` 把助手指到你自己的服务商。命令失败后按 `Cmd + Shift + E` 会直接运行建议的修复，只有 `rm -rf` 这类看起来危险的命令才只粘贴不运行。分屏是 `Cmd + D` 和 `Cmd + Shift + D`，重启后标签页和分屏会回来。它只支持 macOS，没有 blocks，也没有托管的 agent。

想看安装成本和发版节奏的正面对比，[Mac 上的终端怎么选](/zh/blog/best-terminal-for-mac)和 [Ghostty、iTerm2 和 WezTerm 怎么选](/zh/blog/ghostty-vs-iterm2-vs-wezterm)把同样这些终端讲得更细。

## 怎么选

- 想要 blocks 和内置 agent：留在 Warp，遥测要不要开在它的 Privacy 设置里决定。用它的 agent 的话再看一下 Secret Redaction，它默认是关的，打开后会在 AI 请求发出去之前把匹配你设定模式的内容遮掉。
- 想用自己订阅的 AI：列表里的任何终端都能跑 Claude Code 和 Codex，所以按终端本身来选，Kaku 多出的是给它们的[通知](/zh/blog/claude-code-notifications)和 `kaku ai` 配置。
- 想要最少的活动部件：Ghostty。
- 想要设置窗口里一大堆功能：iTerm2。
- 想自己编程：WezTerm，或者想要 WezTerm 的 Lua 加上已经选好的 Mac 默认值，就选 Kaku。
- 想要极简并且用 tmux：Alacritty。

## 切换时容易出的问题

- **blocks 没了。** 没有哪个替代品用同样的方式给命令输出分组，终端里的 shell 历史和搜索能顶替一部分，但顶替不了按命令复制。
- **shell 配置回到 shell 自己。** 在传统终端里，补全、提示符和语法高亮都来自 zsh 本身，[不用 Oh My Zsh 把 zsh 配得又快又好用](/zh/blog/zsh-without-oh-my-zsh)讲了这部分，Kaku 自带 zsh 插件和可选的 Starship。
- **快捷键不一样。** 每个终端有自己的一套，决定之前先看它的快捷键列表。
- **你在 Warp 里的数据。** 放在 Warp Drive 里的东西和从 Warp 分享出去的内容都跟着你的 Warp 账号，卸载之前先把需要的复制出来，删除账号的方法在它的隐私页里。

## 常见问题

### Warp 免费吗？

Warp 有免费计划，它的文档说在那里使用 AI 功能必须开着遥测，计划限制会变，以它的价格页为准。

### 用 Warp 必须有账号吗？

不需要，快速上手文档说可以跳过注册直接开始用。

### 有 Warp 的开源替代品吗？

Warp 自己的客户端就是开源的，AGPL v3 加 MIT。替代品里 Ghostty 和 Kaku 是 MIT，Kitty 是 GPL-3.0，Alacritty 是 Apache-2.0。

### 离开 Warp 之后，哪个终端最适合 Claude Code？

哪个都能跑，区别在多行输入、通知和恢复会话，[Claude Code 用什么终端最好](/zh/blog/best-terminal-for-claude-code)对比过。
