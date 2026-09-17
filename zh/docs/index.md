# 安装

用 DMG 或 Homebrew 把 Kaku 加入 macOS 工作流，再检查 shell 集成。

> 大多数用户直接用 DMG。已经用 Homebrew 管理软件时再选 Homebrew。开发者环境放在 [贡献文档](https://kaku.fun/zh/docs/contributing)。

## 下载 DMG

推荐大多数用户直接从 GitHub Releases 下载最新 DMG，下载后打开镜像，把 Kaku 拖到 Applications，再从应用列表启动。

[打开最新 Release](https://github.com/tw93/Kaku/releases/latest)

## Homebrew

如果你已经用 Homebrew 管理开发工具，装官方 cask 即可。

```bash
brew install --cask kaku
open -a Kaku
kaku doctor
```

Homebrew 适合需要命令行安装和自动化更新的机器。

如果之前装的是个人 tap `tw93/tap/kakuku`，可以继续用那个 tap 升级，也可以迁到官方 cask：

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## 安装后

安装完成后先打开一次 Kaku，再运行 `kaku doctor`，会检查 app bundle、配置目录、PATH、zsh/fish shell 集成和可选工具可用性。

```bash
/Applications/Kaku.app/Contents/MacOS/kaku doctor
```

如果 shell 里找不到 `kaku`，用应用内置二进制恢复 shell 集成，然后重开登录 shell：

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## 排查

- 确认应用在 `/Applications/Kaku.app`，不要直接从 DMG 里运行。
- Homebrew 安装失败时，先运行 `brew update`，再执行 `brew install --cask kaku`。如果 Homebrew 安装的 Kaku 在 `kaku update` 时遇到 checksum 问题，直接运行 `brew upgrade --cask kaku`。
- 首次配置 shell 工具可以运行 `kaku init`，它会准备 zsh/fish 集成，并在交互式 shell 里询问是否用 Homebrew 安装缺失的 Starship、Delta、Lazygit、Yazi 等可选工具。
- AI 功能不可用时，打开 `kaku ai` 检查 Auth Type、Base URL、Simple Model、Deep Model 和 API key。
- 提交 issue 时带上安装方式、macOS 版本、Kaku 版本和复现步骤。

[打开 GitHub Issues](https://github.com/tw93/Kaku/issues)

---

Source: https://kaku.fun/zh/docs/
Site index for LLMs: https://kaku.fun/llms.txt
