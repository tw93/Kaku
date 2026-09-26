# Kaku 对比 iTerm2、Warp、Ghostty 和 WezTerm

Kaku 想做的是装好就能用的 Mac 终端，这页写它和几款常见终端差在哪，以及什么情况下不必换。

## 先看要解决什么

想换终端的理由一般就那几种，不想花一下午配终端，已经有一套顺手的 iTerm2，想要更快的 GPU 终端，或者想要 AI 做进产品里的那种，Kaku 解决的是第一种，字体、主题、标签、分屏和 shell 工具都配好了，AI 服务可以接你自己的。

## Kaku 强在哪

- **打开就能用。**JetBrains Mono、macOS 字体渲染、跟随系统的深浅色、选中即复制，还有 Mac 风格的标签和分屏快捷键，都已经配好。
- **标签和分屏重开还在。**Cmd + T 开标签，Cmd + D 分屏，Cmd + Shift + O 打开 Tab Navigator 找分屏，关掉再打开 Kaku，窗口、分屏和目录都还在，右键还能复制、粘贴、搜索、开 AI 对话，或者从点中的分屏再拆一个。
- **自带一套 shell 工具。**补全、语法高亮和 `z` 跳目录在 Kaku 里直接能用，Cmd + Shift + G 开 Lazygit，Cmd + Shift + Y 开 Yazi，Cmd + Shift + R 挂载当前 SSH 主机的文件。
- **AI 可选，用你自己的服务。**命令报错时给出修复，`#` 加一句话生成命令，Cmd + L 和 `kaku chat` 打开的是同一份对话，建议只会放到命令行给你看，不会自动执行，Kaku 也没有账号和自己的模型。
- **出了问题能自查。**`kaku doctor` 会检查 app、PATH 和 shell 集成，`kaku config` 和 `kaku ai` 都是直接在终端里操作的设置界面。
- **不碰你的数据。**MIT 开源，没有使用统计，App 会发出的网络请求都列在[隐私页](https://kaku.fun/zh/privacy)。

## 对照

|  | Kaku | iTerm2 | Warp | Ghostty | WezTerm | 系统终端 |
| --- | --- | --- | --- | --- | --- | --- |
| 协议 | MIT | GPL | 商业软件 | MIT | MIT | Apple |
| 平台 | 仅 macOS | macOS | macOS 及其他 | macOS 和 Linux | 跨平台 | macOS |
| 账号 | 无 | 无 | AI 围绕账号 | 无 | 无 | 无 |
| 默认值 | 开箱即用 | 自己拼 | 产品默认 | 快，预设少 | 自己拼 | 系统自带 |
| AI | 可选，用你的服务 | 没有内置 | 做进产品里 | 没有内置 | 没有内置 | 没有内置 |
| Shell 工具 | Kaku 会话里自带 | 自己加 | 产品自带 | 自己加 | 自己加 | 无 |
| 安装 | DMG 或官方 brew cask | 下载 | 产品安装器 | 下载 | 下载 | 系统自带 |
| 自检 | `kaku doctor` | 无 | 产品界面 | 无 | 无 | 无 |

## Kaku 和 iTerm2

iTerm2 是 Mac 上用得最久的终端替代，很多人留下是因为配置、快捷键和手感早就调顺了，Kaku 适合不想再从头搭一遍的人，字体主题开箱就有，标签分屏、会话恢复、右键菜单、可点击路径和一键打开的 Lazygit、Yazi 都在，PATH 或 shell 集成出问题时，`kaku doctor` 会直接告诉你。

现有的 iTerm2 配置用得好好的，或者要用 Kaku 还没有的功能，那就继续用 iTerm2，Kaku 也不打算把它完整复刻一遍。

## Kaku 和 Warp

Warp 是商业软件，AI 优先，助手和工作流就是产品本身，Kaku 是 MIT 开源，没有账号，也不提供或中转 AI 服务，运行 `kaku ai` 填上你自己的服务就行，报错修复、`#` 生成命令、Cmd + L 打开对话这些都有，但不会自动执行任何东西，Cmd + Shift + E 只是把建议放到命令行，不配 AI 也不影响其他功能。

想要终端里一整套托管的 AI 产品，选 Warp，想要开源、AI 可选而且只走你自己配置的 Mac 终端，选 Kaku。

## Kaku 和 Ghostty

Ghostty 是很快的 GPU 终端，MIT 开源，如果 shell 工具、字体和工作流你都已经配好了，它很合适，Kaku 同样用 GPU 渲染，区别在于已经接好的那一层，JetBrains Mono、自动主题、会话恢复、补全和 `z`、Lazygit、Yazi、远程文件，还有可选的助手。

想要更轻的终端，现有配置也顺手，就继续用 Ghostty，缺的是开箱那一套、又不想再写一份配置，可以试试 Kaku。

## Kaku 和 WezTerm

Kaku 从 WezTerm 衍生而来，保留了它的 Lua 配置和渲染引擎，再把 WezTerm 留给你自己做的 Mac 这一层补上，包括默认值、shell 集成、Tab Navigator、窗口快照、`kaku` 命令和可选助手，已有的 WezTerm 配置大多能直接用，个别上游选项行为不同，见[配置说明](https://kaku.fun/zh/docs/configuration)。

要在 Windows 或 Linux 上用，或者就想要原版，继续用 WezTerm，Kaku 只做 macOS。

## Kaku 和系统终端

系统自带的终端不用装，偶尔敲几条命令够用了，Kaku 面向天天待在终端里的人，分屏、会话恢复、可点击路径、右键菜单、一套 shell 工具，以及接你自己服务的可选助手。

## 什么时候不要用 Kaku

- **Windows 或 Linux。**不支持，也不承诺时间表。
- **托管或浏览器终端。**Kaku 是本地 Mac 应用。
- **API、SDK 或 MCP server。**都没有，自动化用本地的 `kaku` 命令。
- **完整复刻 iTerm2 或 WezTerm。**有些功能是故意没做的。

## 安装 Kaku

官方 Homebrew cask：

```bash
brew install --cask kaku
kaku doctor
```

也可以从 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载 DMG。[安装说明](https://kaku.fun/zh/docs/)里有装完后的检查步骤，以及从旧的个人 tap 迁移的方法。

---

Source: https://kaku.fun/zh/compare
Site index for LLMs: https://kaku.fun/llms.txt
