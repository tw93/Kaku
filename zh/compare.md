# Kaku 对比 iTerm2、Warp、Ghostty 和 WezTerm

按你真正要做的事来选 macOS 终端，而不是对照功能清单打钩。

## 先看要解决什么

搜 macOS 终端替代，通常是这几件事：不想花一下午配终端、已经有一套 iTerm2、想换更快的 GPU 终端，或者想要重度 AI 产品。Kaku 做的是第一件事。它基于 WezTerm，字体、主题、标签页、分屏和 shell 工具开箱就有，也可以接你自己配置的 AI 服务。

活儿不同，别的终端仍然更合适。下面写清楚什么时候不该换。

## 对照

|  | Kaku | iTerm2 | Warp | Ghostty | WezTerm | 系统终端 |
| --- | --- | --- | --- | --- | --- | --- |
| 协议 | MIT | GPL | 商业软件 | MIT | MIT | Apple |
| 平台 | 仅 macOS | macOS | macOS 及其他 | macOS 和 Linux | 跨平台 | macOS |
| 账号 | 无 | 无 | AI 围绕账号 | 无 | 无 | 无 |
| 默认值 | 开箱即用 | 自己拼 | 产品默认 | 快，预设少 | 自己拼 | 系统自带 |
| AI | 可选，用你的服务 | 没有内置 | 做进产品里 | 没有内置 | 没有内置 | 没有内置 |

## Kaku 和 iTerm2

iTerm2 是长期存在的 macOS 终端替代。很多人留下，是因为配置、快捷键和手感都已经定了。Kaku 更适合想要第一次打开就有字体、深浅色主题、Mac 风格标签和分屏，以及一套配好的 shell 工具，而不想再搭一遍的人。

如果现有 iTerm2 配置正在干活，或者你需要 Kaku 还没做的功能，继续用 iTerm2。Kaku 不打算做成完整的 iTerm2 复制品。

## Kaku 和 Warp

Warp 是商业、AI 优先的终端，助手和工作流是产品本身的一部分。Kaku 用 MIT 协议，没有 Kaku 账号，也不提供或中转 AI 服务。你运行 `kaku ai`，指向自己的服务。建议不会自动执行，Cmd + Shift + E 只是把修复粘贴出来给你看。

想要终端里的托管 AI 产品，选 Warp。想要开源的 Mac 终端，AI 可选、只活在你自己的配置里，选 Kaku。

## Kaku 和 Ghostty

Ghostty 是很快的 MIT GPU 终端。如果你想要现代模拟器，并且自己已经有 shell 工具、字体和工作流，它很合适。Kaku 同样 GPU 加速，但 JetBrains Mono、自动主题、Lazygit、Yazi、远程文件和可选助手已经接好。

想要更薄的终端、现有配置也顺手，继续用 Ghostty。缺的是开箱那一套，而不是再写一份配置，再用 Kaku。

## Kaku 和 WezTerm

Kaku 从 WezTerm 衍生而来。它保留 WezTerm 的 Lua 配置和快速引擎，再补上 Mac 默认值、shell 集成和可选助手。已有 WezTerm 配置大多能复用，部分上游选项行为不同，请看[配置说明](https://kaku.fun/zh/docs/configuration)。

需要 Windows 或 Linux，或者就要上游原样，继续用 WezTerm。Kaku 只做 macOS。

## Kaku 和系统终端

系统自带终端不用再装，偶尔敲几条命令够用。Kaku 面向天天待在终端的人：标签页、分屏、会话恢复、可点击路径，以及可选助手。

## 什么时候不要用 Kaku

- **Windows 或 Linux。**不支持，也不承诺时间表。
- **托管或浏览器终端。**Kaku 是本地 Mac 应用。
- **API、SDK 或 MCP server。**这些都不存在。自动化走本地 `kaku` 命令。
- **完整的 iTerm2 或 WezTerm 复制品。**有些上游和 iTerm2 功能是故意没做的。

## 安装 Kaku

官方 Homebrew cask：

```bash
brew install --cask kaku
kaku doctor
```

也可以从 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载 DMG。[安装说明](https://kaku.fun/zh/docs/)里有检查步骤，以及旧个人 tap 的迁移。

---

Source: https://kaku.fun/zh/compare
Site index for LLMs: https://kaku.fun/llms.txt
