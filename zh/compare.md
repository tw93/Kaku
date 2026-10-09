# Kaku 对比 iTerm2、Warp、Ghostty 和 WezTerm

Kaku 想做的是装好就能用的 Mac 终端，这页写它和几款常见终端差在哪，以及什么情况下不必换。

## 先看要解决什么

想换终端的理由通常就几种：不想花一下午折腾配置、已经有一套顺手的 iTerm2、想要更轻快的 GPU 渲染终端，或是想要内建 AI 的现代工具。Kaku 专注解决第一种：开箱即用，字体、主题、标签、分屏与 Shell 工具提前调顺，AI 助手也随时能接上你自己的服务。

## Kaku 的优势

- **打开就能用。**JetBrains Mono、macOS 字体渲染、跟随系统的深浅色、选中即复制，还有 Mac 风格的标签和分屏快捷键，都已经配好。
- **标签和分屏重开还在。**Cmd + T 开标签，Cmd + D 分屏，Cmd + Shift + O 打开 Tab Navigator 查找分屏。关掉再打开 Kaku，窗口、分屏和目录都会恢复；右键还能快速复制、粘贴、搜索、发起 AI 对话，或直接拆分当前分屏。
- **自带一套 shell 工具。**补全、语法高亮和 `z` 跳目录在 Kaku 里直接能用，Cmd + Shift + G 开 Lazygit，Cmd + Shift + Y 开 Yazi，Cmd + Shift + R 挂载当前 SSH 主机的文件。
- **AI 可选，用你自己的服务。**命令报错时给出修复建议，输入 `#` 加一句话生成命令，Cmd + L 与 `kaku chat` 共享同一份对话记录。所有建议只填入命令行供你确认，绝不自动执行；Kaku 本身无需账号，也不提供或中转模型。
- **出了问题能自查。**`kaku doctor` 会检查 app、PATH 和 shell 集成，`kaku config` 和 `kaku ai` 都是直接在终端里操作的设置界面。
- **不碰你的数据。**MIT 开源，没有使用统计，App 会发出的网络请求都列在[隐私页](https://kaku.fun/zh/privacy)。

![kaku doctor 输出，所有检查都通过](https://kaku.fun/shots/compare-doctor.webp)`kaku doctor` 一次查完 app、PATH 和 shell 集成

## 对照

|  | Kaku | iTerm2 | Warp | Ghostty | WezTerm | 系统终端 |
| --- | --- | --- | --- | --- | --- | --- |
| 协议 | MIT | GPL | 客户端 AGPL，后端闭源 | MIT | MIT | Apple |
| 平台 | 仅 macOS | macOS | macOS、Linux、Windows | macOS 和 Linux | macOS、Linux、Windows | macOS |
| 账号 | 无 | 无 | 可选 | 无 | 无 | 无 |
| 默认值 | 开箱即用 | 自己拼 | 产品默认 | 快，预设少 | 自己拼 | 系统自带 |
| AI | 可选，用你的服务 | 可选插件，用你的 key | 内置，跑在 Warp 的服务器上 | 没有内置 | 没有内置 | 没有内置 |
| Shell 工具 | Kaku 会话里自带 | 自己加 | 产品自带 | 自己加 | 自己加 | 无 |
| 安装 | DMG 或官方 brew cask | 下载 | 产品安装器 | 下载 | 下载 | 系统自带 |
| 稳定版（2026 年 10 月） | 0.22.0 | 3.7.3 | 每周构建 | 1.3.1 | 20240203，之后只有 nightly | 随 macOS 更新 |
| 自检 | `kaku doctor` | 无 | 产品界面 | 无 | 无 | 无 |

## Kaku 和 iTerm2

iTerm2 是 Mac 上历史悠久的经典终端，很多人留下来是因为配置、快捷键和使用习惯早已磨合顺手。Kaku 适合不想从头折腾配置的人：字体和主题开箱调好，标签分屏、会话恢复、右键菜单、可点击路径，以及 Lazygit、Yazi 快捷调用都已备齐；当 PATH 或 shell 集成异常时，`kaku doctor` 会直接帮你排查诊断。

iTerm2 自己也一直在更新，3.5 加了 AI 对话，要单独装插件、填自己的 API key，2026 年 9 月的 3.7 又加了标签分组和 Claude Code 集成。

![Kaku 窗口，三个标签，其中一个拆成三个分屏](https://kaku.fun/shots/compare-panes.webp)三个标签加一组分屏，退出重开还是原样

现有的 iTerm2 配置用得好好的，或者要用 Kaku 还没有的功能，那就继续用 iTerm2，Kaku 也不打算把它完整复刻一遍。

## Kaku 和 Warp

Warp 目前定位为 Agent 驱动的开发环境，Agent 是其核心产品。客户端虽已按 AGPL 开源，但 Agent 运行在 Warp 云端服务器上，无法直接复用已有的 Claude 或 Codex 独立订阅，付费方案 20 美元/月起。Kaku 则是纯粹的本地终端：MIT 开源、无需账号，也不提供或中转任何 AI 服务。运行 `kaku ai` 配置自己的服务即可使用报错修复、`#` 自然语言生成命令与 Cmd + L 对话；所有建议仅填入命令行供你确认，绝不自动执行；不配 AI 也完全不影响纯终端使用。

想要终端里一整套托管的 AI 产品，选 Warp，想要开源、AI 可选而且只走你自己配置的 Mac 终端，选 Kaku。

## Kaku 和 Ghostty

Ghostty 是一款非常轻快的 GPU 加速终端，MIT 开源，采用 macOS 原生控件呈现标签与分屏，并提供从菜单栏呼出的 Quick Terminal 和窗口恢复。如果你已经调优好自己的 shell 工具、字体和工作流，Ghostty 是极佳的选择。Kaku 同样采用 GPU 渲染，核心差异在于省去了从零装配的成本：JetBrains Mono、跟随系统的深浅色主题、智能补全与 `z` 跳转、Lazygit / Yazi 整合、远程文件挂载以及按需开启的 AI 助手均已开箱就绪。

![在 Kaku 里运行的 Lazygit 提交记录](https://kaku.fun/shots/compare-lazygit.webp)Cmd + Shift + G 在当前仓库打开 Lazygit

想要更轻的终端，现有配置也顺手，就继续用 Ghostty，缺的是开箱那一套、又不想再写一份配置，可以试试 Kaku。

## Kaku 和 WezTerm

Kaku 衍生自 WezTerm，完整保留了其成熟的 Lua 配置体系与渲染引擎，同时补齐了原生 macOS 体验所缺失的环节：更符合 Mac 习惯的默认值、完善的 shell 集成、Tab Navigator、窗口快照、`kaku` 命令行套件以及可选的 AI 助手。已有的 WezTerm 配置大多可以直接沿用（个别选项差异可参考[配置说明](https://kaku.fun/zh/docs/configuration)）。此外，上游自 2024 年 2 月后暂未发布新的正式稳定版，改动仅进入 nightly。

![kaku config 设置界面，包含主题、字体和窗口选项](https://kaku.fun/shots/compare-config.webp)`kaku config` 改常用设置，不用打开 Lua 文件

要在 Windows 或 Linux 上用，或者就想要原版，继续用 WezTerm，Kaku 只做 macOS。

## Kaku 和系统终端

系统自带的终端无需安装，偶尔执行简单命令足够好用。Kaku 则面向天天待在命令行里的重度用户：原生分屏、会话恢复、可点击路径、右键菜单、开箱即用的 shell 套件，以及随手可用且直连个人配置的 AI 助手。

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
