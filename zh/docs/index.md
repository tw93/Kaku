# 安装

用 DMG 或 Homebrew 装好 Kaku，再检查一下 shell 集成。

> 推荐直接下载 DMG 安装；若习惯命令行管理或脚本化部署，可选用 Homebrew。需要从源码编译请参考[贡献文档](https://kaku.fun/zh/docs/contributing)。

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
- 若 Homebrew 安装失败，可先执行 `brew update` 后重试。对于通过 Homebrew 安装的 Kaku，若运行 `kaku update` 提示 checksum 校验错误，请改用 `brew upgrade --cask kaku` 更新。
- 初次配置环境请运行 `kaku init`，会自动配置 zsh/fish 集成；在交互式终端中还会询问是否通过 Homebrew 安装缺失的 Starship、Delta、Lazygit、Yazi 等可选工具。
- AI 功能用不了时，打开 `kaku ai` 检查一下 Auth Type、Base URL、Simple Model、Deep Model 和 API key。
- 提交 issue 时带上安装方式、macOS 版本、Kaku 版本和复现步骤。

[打开 GitHub Issues](https://github.com/tw93/Kaku/issues)

---

Source: https://kaku.fun/zh/docs/
Site index for LLMs: https://kaku.fun/llms.txt
