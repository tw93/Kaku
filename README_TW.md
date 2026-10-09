<div align="center">
  <img src="https://gw.alipayobjects.com/zos/k/6h/dwarf.svg" width="120" />
  <h1>Kaku</h1>
  <p><b>專為 AI 程式設計打造的高效能 Mac 終端機，開箱即用</b></p>
  <p><a href="README.md">English</a> · <a href="README_CN.md">中文</a> · 繁體 · <a href="README_JA.md">日本語</a> · <a href="README_KR.md">한국어</a> · <a href="README_DE.md">Deutsch</a> · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">官網</a> ·
    <a href="https://kaku.fun/docs/">文件</a> ·
    <a href="https://kaku.fun/compare">比較</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">下載</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku 截圖" width="1000" />
</p>

## 緣起

Kaku（書く，かく）在日文中意為「寫」。它是一款基於 WezTerm 的 macOS 終端機，預裝了優質字型、主題、Shell 整合與常用的 Mac 快捷鍵。如果你想自訂，Lua 設定依然完全可用。產品官網是 [kaku.fun](https://kaku.fun)。

三部曲之一：[Kaku](https://github.com/tw93/Kaku) (書く) 編寫程式碼，[Waza](https://github.com/tw93/Waza) (技) 磨練習慣，[Kami](https://github.com/tw93/Kami) (紙) 交付文件。可以把它們看作一家人：Kaku 是父親，Waza 是姐姐，Kami 是妹妹。

## 快速上手

下載 [Kaku DMG](https://github.com/tw93/Kaku/releases/latest)，打開並將 Kaku 拖入「應用程式」目錄。也可以使用 Homebrew 安裝：

```bash
brew install --cask kaku
```

打開 Kaku 完成 Shell 整合設定。缺少的常用工具可透過 `kaku init` 一鍵安裝。透過 `kaku --version` 查看目前安裝版本。

## 特性

- **開箱即用**：預置 JetBrains Mono 字型，自動切換淺色與深色主題，選取即複製，熟悉的 Mac 原生快捷鍵
- **分頁與分屏**：自由拆分工作區，透過 Tab 導覽器快速定位分屏，重啟時自動恢復視窗、分屏與工作目錄
- **右鍵選單**：貼上、搜尋、喚起 AI 對話，拆分或關閉當前分屏，開關分頁無需死記快捷鍵
- **可點擊連結**：`Cmd + 點擊` 直接開啟連結與檔案路徑，終端機自動換行的長連結也能完整識別
- **AI 友善**：支援搭配 AI 助手獲取指令建議與互動對話，透過 `kaku ai` 設定屬於你自己的 AI 服務
- **Shell 工具鏈**：內建 zsh 自動補齊、語法高亮與目錄快速跳轉，並為可選的 Lazygit 和 Yazi 提供快捷鍵支援
- **Lua 設定**：基於 WezTerm 的 Lua 設定系統，自由自訂字型、主題、快捷鍵與終端機行為

## 使用指南

| 操作 | 快捷鍵 |
| :--- | :--- |
| 新建分頁 | `Cmd + T` |
| 新建視窗 | `Cmd + N` |
| 關閉分頁/分屏 | `Cmd + W` |
| 切換分頁 | `Cmd + Shift + [` / `]` 或 `Cmd + 1-9` |
| 切換分屏 | `Cmd + Opt + 方向鍵` |
| 垂直分屏 | `Cmd + D` |
| 水平分屏 | `Cmd + Shift + D` |
| 開啟設定面板 | `Cmd + ,` |
| AI 面板 | `Cmd + Shift + A` |
| AI 對話 | `Cmd + L` |
| 套用 AI 建議 | `Cmd + Shift + E` |
| 開啟 Lazygit | `Cmd + Shift + G` |
| Yazi 檔案管理器 | `Cmd + Shift + Y` 或 `y` |
| 清除螢幕 | `Cmd + K` |
| 開啟右鍵選單 | 在分屏中點擊右鍵 |
| 開啟連結或檔案路徑 | `Cmd + 點擊` |

完整快捷鍵參考：[docs/keybindings.md](docs/keybindings.md)

## Kaku AI

使用 `kaku ai` 設定你自己的 AI 服務即可開啟內建助手。Kaku 本身不提供也不中轉任何 AI 服務。

- **指令建議**：當指令執行出錯時，助手可提供修復建議，按 `Cmd + Shift + E` 將建議貼至提示符處供你確認
- **自然語言轉指令**：在提示符處輸入 `# <描述>` 並按 Enter，助手會將產生的指令填入終端機供你確認並執行
- **AI 對話**：按 `Cmd + L` 討論終端機輸出或配合專案檔案與工具操作。在其他終端機中執行 `kaku chat` 可存取相同的對話紀錄
- **AI 工具設定**：集中管理 Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code 等工具的設定

更多關於認證、模型、API 模式與工具設定，請參閱 [AI 助手文件](docs/features.md)。

## 常見問題

**是否有 Windows 或 Linux 版本？** 目前沒有，Kaku 目前僅支援 macOS。

**Kaku 與 iTerm2、Warp、Ghostty、WezTerm 有什麼區別？** 詳見 [kaku.fun/compare](https://kaku.fun/compare)。

**是否可以使用透明視窗？** 可以，在 `~/.config/kaku/kaku.lua` 中設定 `config.window_background_opacity` 即可。

**找不到 `kaku` 指令？** 執行 `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`，然後執行 `kaku doctor` 檢查。

完整 FAQ：[docs/faq.md](docs/faq.md)

## 文件

- [官網](https://kaku.fun) - 產品網站、安裝指引以及中英文文件
- [比較](https://kaku.fun/compare) - Kaku 與 iTerm2、Warp、Ghostty、WezTerm 和系統終端機的比較
- [快捷鍵](docs/keybindings.md) - 完整快捷鍵參考
- [功能特性](docs/features.md) - AI 助手、lazygit、yazi、遠端檔案、Shell 工具集
- [設定指南](docs/configuration.md) - 主題、字型、自訂快捷鍵、Lua API
- [CLI 指令參考](docs/cli.md) - `kaku ai`、`kaku config`、`kaku doctor` 等指令
- [常見問題](docs/faq.md) - 常見疑問與疑難排解

## 背景

無論是工作還是個人專案，我都極其依賴命令列，我做過的許多工具（如 [Mole](https://github.com/tw93/mole) 和 [Pake](https://github.com/tw93/pake)）都體現了這一點。

我用了多年 Alacritty，很看重它的極致速度與純粹。隨著工作流程向 AI 輔助程式設計遷移，我對分頁與多螢幕工作區的人體工學有了更高要求。我也深入嘗試過 Kitty、Ghostty、Warp 和 iTerm2，它們各有千秋，但我依然想要一個在效能、預設體驗與自主可控之間達到理想平衡的開發環境。

WezTerm 極其堅固且具備出色的擴充性，非常感謝它的底層引擎和社群生態。我打造 Kaku，就是為了擁有這樣一個環境：極致輕快、精緻順手、開箱即戰。

## 貢獻者

非常感謝所有為 Kaku 做出貢獻的開發者，歡迎關注他們！❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## 支持

- 最直接的支持方式是購買我製作的 Mac 付費清理工具 [Mole for Mac](https://mole.fit)
- 如果 Kaku 對你有幫助，歡迎給它一個 Star、[分享推薦](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20A%20blazing-fast%20Mac%20terminal%2C%20built%20for%20coding%20with%20AI.)，或提交 Issue 與 PR
- 我有兩隻貓：湯圓、可樂，若 Kaku 讓你的生活更美好，歡迎<a href="https://cats.tw93.fun?name=Kaku" target="_blank">請牠們吃罐頭 🥩</a>

<details>
<summary>這些可愛的小夥伴已經投餵過了 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## 授權

MIT License，歡迎享受並參與開源。WezTerm 及內建字型的版權宣告見 [NOTICE.md](NOTICE.md)。
