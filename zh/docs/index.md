# 安装

用 DMG 或 Homebrew 装好 Kaku，再检查一下 shell 集成。

> 大多数人装 DMG 就行，想用命令行安装或脚本化更新再选 Homebrew，要自己编译 Kaku 可以看[贡献文档](https://kaku.fun/zh/docs/contributing)。

## 下载 DMG

从 GitHub Releases 下载最新的 DMG，打开后把 Kaku 拖进 Applications，再从应用列表启动。

[打开最新 Release](https://github.com/tw93/Kaku/releases/latest)

## Homebrew

平时用 Homebrew 管理开发工具的话，直接装官方 cask：

```bash
brew install --cask kaku
open -a Kaku
kaku doctor
```

之前从个人 tap `tw93/tap/kakuku` 装的，可以继续从那里升级，也可以换到官方 cask：

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## 安装后

装好后先打开一次 Kaku，再运行 `kaku doctor`，它会检查 app bundle、PATH 和 zsh/fish 的 shell 集成。

```bash
/Applications/Kaku.app/Contents/MacOS/kaku doctor
```

如果 shell 里找不到 `kaku`，用应用自带的二进制恢复 shell 集成，再重开登录 shell：

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## 排查

- 确认应用在 `/Applications/Kaku.app`，不要直接从 DMG 里运行。
- Homebrew 装不上时先跑 `brew update`，再重新 `brew install --cask kaku`，用 Homebrew 装的 Kaku 如果 `kaku update` 报 checksum 错误，改用 `brew upgrade --cask kaku`。
- 第一次配置 shell 时运行 `kaku init`，它会配好 zsh/fish 集成，在交互式 shell 里还会问你要不要用 Homebrew 装上缺失的 Starship、Delta、Lazygit、Yazi 这些可选工具。
- AI 功能用不了时，打开 `kaku ai` 检查一下 Auth Type、Base URL、Simple Model、Deep Model 和 API key。
- 提交 issue 时带上安装方式、macOS 版本、Kaku 版本和复现步骤。

[打开 GitHub Issues](https://github.com/tw93/Kaku/issues)

---

Source: https://kaku.fun/zh/docs/
Site index for LLMs: https://kaku.fun/llms.txt
