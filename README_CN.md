<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>一个开箱即用、默认好用、AI 友好的 Mac 终端</em></p>
  <p><a href="README.md">English</a> · 中文 · <a href="README_TW.md">繁體</a> · <a href="README_JA.md">日本語</a> · <a href="README_KR.md">한국어</a> · <a href="README_DE.md">Deutsch</a> · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">官网</a> ·
    <a href="https://kaku.fun/docs/">文档</a> ·
    <a href="https://kaku.fun/compare">对比</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">下载</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku 截图" width="1000" />
</p>

## 缘起

Kaku（書く，かく）在日文中意为“写”，是一款基于 WezTerm 的 macOS 终端，预装了优质字体、主题、Shell 集成与常用 Mac 快捷键，同时保留完整的 Lua 自定义能力，官网为 [kaku.fun](https://kaku.fun)。

作为三部曲中专注于编码的基础终端，Kaku 与规范工程习惯的 [Waza](https://github.com/tw93/Waza) (技)、负责交付文档的 [Kami](https://github.com/tw93/Kami) (紙) 相互配合，让写代码、磨习惯和出文档一气呵成。

## 快速上手

下载 [Kaku DMG](https://github.com/tw93/Kaku/releases/latest)，打开并将 Kaku 拖入“应用程序”目录。也可以使用 Homebrew 安装：

```bash
brew install --cask kaku
```

打开 Kaku 完成 Shell 集成设置。缺少的可选工具可以用 `kaku init` 安装。通过 `kaku --version` 查看当前安装版本。

## 特性

- **开箱即用**：预置 JetBrains Mono 字体，自动切换浅色与深色主题，选中即复制，熟悉的 Mac 原生快捷键
- **标签与分屏**：自由拆分工作区，通过 Tab Navigator 快速定位分屏，重新打开 Kaku 时恢复窗口、分屏和工作目录
- **右键菜单**：粘贴、搜索、唤起 AI 对话，拆分或关闭当前分屏，开关标签页无需死记快捷键
- **可点击链接**：`Cmd + 点击` 直接打开链接和文件路径，终端自动换行的长链接也能完整识别
- **AI 友好**：你的编程工具照常用，另有可选的助手提供命令建议和对话，通过 `kaku ai` 配置你自己的 AI 服务
- **Shell 工具链**：内置 zsh 自动补全、语法高亮和目录快速跳转，并为可选的 Lazygit 和 Yazi 提供快捷键支持
- **Lua 配置**：基于 WezTerm 的 Lua 配置系统，自由定制字体、主题、快捷键与终端行为

## 使用指南

| 操作 | 快捷键 |
| :--- | :--- |
| 新建标签页 | `Cmd + T` |
| 新建窗口 | `Cmd + N` |
| 关闭标签页/分屏 | `Cmd + W` |
| 切换标签页 | `Cmd + Shift + [` / `]` 或 `Cmd + 1-9` |
| 切换分屏 | `Cmd + Opt + 方向键` |
| 垂直分屏 | `Cmd + D` |
| 水平分屏 | `Cmd + Shift + D` |
| 打开设置面板 | `Cmd + ,` |
| AI 面板 | `Cmd + Shift + A` |
| AI 对话 | `Cmd + L` |
| 应用 AI 建议 | `Cmd + Shift + E` |
| 打开 Lazygit | `Cmd + Shift + G` |
| Yazi 文件管理器 | `Cmd + Shift + Y` 或 `y` |
| 清屏 | `Cmd + K` |
| 打开右键菜单 | 在分屏中右键点击 |
| 打开链接或文件路径 | `Cmd + 点击` |

完整快捷键参考：[docs/keybindings.md](docs/keybindings.md)

## Kaku AI

使用 `kaku ai` 配置你自己的 AI 服务即可开启内置助手。Kaku 本身不提供也不中转任何 AI 服务。

- **命令建议**：当命令执行出错时，助手可提供修复建议，按 `Cmd + Shift + E` 将建议粘贴至提示符处供你确认
- **自然语言转命令**：在提示符处输入 `# <描述>` 并回车，助手会将生成的命令填入终端供你确认并执行
- **AI 对话**：按 `Cmd + L` 讨论终端输出或配合项目文件与工具操作。在其他终端中运行 `kaku chat` 可访问相同的会话记录
- **AI 工具配置**：集中管理 Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code 等工具的设置

认证、模型、API 模式和工具设置见 [AI 助手文档](docs/features.md)。

## 常见问题

**是否有 Windows 或 Linux 版本？** 目前没有，Kaku 目前仅支持 macOS。

**Kaku 与 iTerm2、Warp、Ghostty、WezTerm 有什么区别？** 详见 [kaku.fun/compare](https://kaku.fun/compare)。

**是否可以使用透明窗口？** 可以，在 `~/.config/kaku/kaku.lua` 中设置 `config.window_background_opacity` 即可。

**找不到 `kaku` 命令？** 运行 `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`，然后执行 `kaku doctor` 检查。

完整 FAQ：[docs/faq.md](docs/faq.md)

## 文档

- [官网](https://kaku.fun) - 产品网站、安装指引以及中英文文档
- [对比](https://kaku.fun/compare) - Kaku 与 iTerm2、Warp、Ghostty、WezTerm 和系统终端的对比
- [快捷键](docs/keybindings.md) - 完整快捷键参考
- [功能特性](docs/features.md) - AI 助手、lazygit、yazi、远程文件、Shell 工具集
- [配置指南](docs/configuration.md) - 主题、字体、自定义快捷键、Lua API
- [CLI 命令参考](docs/cli.md) - `kaku ai`、`kaku config`、`kaku doctor` 等命令
- [常见问题](docs/faq.md) - 常见疑问与故障排查

## 背景

无论工作还是自己的项目，我都重度依赖命令行，从我做过的 [Mole](https://github.com/tw93/mole) 和 [Pake](https://github.com/tw93/pake) 这些工具也能看出来。

我用了好几年 Alacritty，很看重它的速度和简单，后来工作流慢慢转向 AI 辅助编程，我对标签和分屏用起来顺不顺手有了更高要求。中间也认真用过 Kitty、Ghostty、Warp 和 iTerm2，它们各有长处，但我还是想要一套在性能、默认配置和可控性之间按自己想法取舍的环境。

WezTerm 很稳，也很好折腾，我很感谢它的引擎和生态，于是在它上面做了 Kaku，想要一个够快、细节顺手、打开就能干活的终端。

## 贡献者

非常感谢所有为 Kaku 做出贡献的开发者，欢迎关注他们！❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## 支持

- 最直接的支持方式是购买我制作的 Mac 付费清理工具 [Mole for Mac](https://mole.fit)
- 如果 Kaku 对你有帮助，欢迎给它一个 Star、[分享推荐](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.)，或提交 Issue 与 PR
- 我养了两只猫：汤圆和可乐。如果 Kaku 让你感觉好用，欢迎投喂它们一顿 <a href="https://cats.tw93.fun?name=Kaku" target="_blank">罐头 🥩</a>

<details>
<summary>这些可爱的小伙伴已经投喂过了 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## 协议

MIT License. Please feel free to use and contribute to the development. WezTerm 及内置字体的版权声明见 [NOTICE.md](NOTICE.md)。
