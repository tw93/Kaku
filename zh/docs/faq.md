# 常见问答

大家最常问的问题，从安装、配置到字体、滚动和 AI。

## 怎么安装 Kaku？

从 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载 DMG，或运行 `brew install --cask kaku`。

## 安装后先跑什么？

先打开一次 Kaku，再运行 `kaku doctor`。如果 shell 找不到 `kaku`，运行 `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only`，再用 `exec zsh -l` 重启 shell。

## 有 Windows 或 Linux 版本吗？

暂时没有。Kaku 目前专注于 macOS 体验的打磨，Windows 和 Linux 暂无具体时间表。

## Kaku 和 iTerm2、Warp、Ghostty、WezTerm 有什么区别？

Kaku 是基于 WezTerm 的 macOS 终端，字体、主题、标签、分屏与 shell 套件开箱即用，并内建可选的 AI 助手（直连你自己的服务）。iTerm2 和 WezTerm 通常需要繁琐的手动配置；Warp 是云端 Agent 托管型终端；Ghostty 则是纯粹轻快、预设较少的 GPU 终端。详细差异与选型建议可参考[对比页](https://kaku.fun/zh/compare)。

## 怎么开半透明窗口？

在 `~/.config/kaku/kaku.lua` 里加：

```lua
local config = require("kaku").config
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- 可选模糊度，0–100
return config
```

## 怎么关掉「选中即复制」？

```lua
config.copy_on_select = false
```

## 怎么自定义快捷键？

追加到 `config.keys`，不要整体覆盖：

```lua
config.keys[#config.keys + 1] = {
  key = "RightArrow",
  mods = "CMD|SHIFT",
  action = wezterm.action.ActivatePaneDirection("Right"),
}
```

更多示例见 [快捷键](https://kaku.fun/zh/docs/keybindings) 和 [配置](https://kaku.fun/zh/docs/configuration)。

## 工作目录继承能控制吗？

可以，窗口、标签、分屏各自独立：

```lua
config.window_inherit_working_directory = true
config.tab_inherit_working_directory = true
config.split_pane_inherit_working_directory = true
```

三个默认都开。

## 怎么关掉 Kaku Assistant？

运行 `kaku ai` 打开 Assistant 设置，把 Enabled 关掉。或者直接改 `~/.config/kaku/assistant.toml`：

```toml
enabled = false
```

## 怎么用自定义的 LLM 服务？

运行 `kaku ai`，Auth Type 保持 API key，填入兼容 OpenAI 协议的 Base URL、API key 以及 Simple Model 和 Deep Model。Base URL 填写 API 根地址（如 `https://api.openai.com/v1`）。API Mode 根据服务端支持选 `chat_completions` 或 `responses`。若所选模型支持原生联网搜索，开启 Native Web Search 即可，无需单独配置搜索服务。

## 怎么恢复默认配置？

```bash
kaku reset
```

该命令会清理 Kaku 管理的 shell 与 tmux 集成、git delta 默认配置、部分状态文件，以及 `~/.config/kaku/kaku.lua` 中由 Kaku 维护的主题块。你在这些块之外自定义的 Lua 配置会原样保留。后续若需重新安装 shell 集成，重新执行 `kaku init` 即可。

## `kaku` 命令找不到了，怎么恢复？

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

然后跑一下 `kaku doctor` 检查安装状态。

## 怎么在脚本里用 Kaku 的 CLI？

```bash
kaku cli split-pane
kaku cli split-pane -- bash -c "echo hello"
kaku cli --help
```

全部命令见 [命令参考](https://kaku.fun/zh/docs/cli)。

## 怎么显示滚动条？

打开 `kaku config` 切换滚动条选项，或者改 `~/.config/kaku/kaku.lua`：

```lua
config.enable_scroll_bar = true
```

## 怎么在 nano、vim 这类全屏终端应用里滚动？

打开备用屏幕的滚轮转发：

```lua
config.alternate_screen_wheel_scrolls_terminal = true
```

## 怎么改字体？我改了字体没生效。

要显式设置 `config.font`：

```lua
config.font = wezterm.font('Your Font Name')
```

Kaku 仅会对默认的 JetBrains Mono 字体栈随主题动态微调字重；使用自定义字体时，该行为自动停用。

## `window_padding` 改了没效果。

`window_padding` 可以写数字，也可以写带单位的字符串，纯数字按像素算，`24` 和 `'24px'` 是一回事，其他单位还有 `'pt'`、`'%'` 和 `'cell'`：

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

## QR code 和终端图形看起来被纵向拉高。

Kaku 默认采用 `line_height = 1.28`，让终端文本阅读更加舒适。QR code、`neofetch` 图标、TUI 柱状图等字符图形会随行高成比例拉伸，高度会比无额外行距的终端高出约 28%。这是兼顾文本排版与块字符无缝连接（避免 TUI 边框断裂）的折中设计，并非渲染 Bug。

想让图形接近正方形，可以在 `~/.config/kaku/kaku.lua` 里把行高调低：

```lua
config.line_height = 1.1  -- 或用 1.0 对齐无额外行距的终端
```

不过没有哪个终端能把半块字符拼成的 QR code 画得完全正方，常见等宽字体的 cell 就算在 `line_height = 1.0` 下，高宽比也天然略大于 2:1。

## Claude Code 输出过程中屏幕会跳到顶部。

这是此前触控板滚动与 Claude Code 流式输出并发时的偶现表现。若在流式输出中不慎滚到顶部，按下方向键或继续下滚即可回到当前最新输出。该问题在近期版本中已修复。

## SSH 会话里按 Cmd+Shift+Y 打开的是本地路径。

`Cmd+Shift+Y` 打开的是本地 yazi，SSH 分屏里要用 `Cmd+Shift+R`，这是 yazi 的远端文件功能，会通过 sshfs 把远端文件系统挂载过来。

## ssh 连着的时候，AI 聊天不读文件。

这是有意设计的安全边界。助手工具运行在你的 Mac 本地，而终端当前目录位于远程主机，直接在本地读写文件或执行命令容易误伤同名本地路径。因此在远程窗格中，AI 仅基于终端屏幕输出回答，并生成供你在远程主机执行的命令；`@cwd` 同样不可用。若需要 AI 读取或编辑远程文件，请先通过 `Cmd + Shift + R` 挂载远程目录。

## Kaku 的提示符跑到了别的终端里，或者在别的终端里没了。

zsh 与 fish 环境均会加载 Kaku 的 shell 集成，但 Starship 提示符与 Smart Tab 仅在 Kaku（及从 Kaku 启动的 tmux 会话）中生效，其他终端环境仍保留你的原有提示符。

想在所有终端都用 Kaku 的 Starship 提示符，在 `.zshrc` 里 Kaku 那一行之前加上这句，fish 就在 `config.fish` 里写 `set -gx KAKU_PROMPT_EVERYWHERE 1`：

```zsh
export KAKU_PROMPT_EVERYWHERE=1
```

Smart Tab 始终只在 Kaku 里生效，想关掉就设 `KAKU_SMART_TAB_DISABLE=1`。

## `y` 这个 shell 包装退出时不同步当前目录。

`y` 函数由 Kaku 的 shell 集成提供，成功加载后方可在退出时同步工作目录（可通过 `kaku doctor` 检查集成状态）。直接执行 `yazi` 原生命令不会同步目录。

## 怎么用 Homebrew 安装或升级 Kaku？

装官方 cask：

```bash
brew install --cask kaku
```

升级用 `brew upgrade --cask kaku`。

如果还在用以前的 tap `tw93/tap/kakuku`，可以继续升级那个 tap，也可以迁到官方 cask：

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## Claude Code 的通知不出现。

Kaku 可能没拿到通知权限。打开 System Settings > Notifications > Kaku，开启 Allow Notifications，然后重启 Kaku。

## 怎么修改全局快捷键？

`Cmd + Opt + Ctrl + K` 在任何应用里都能呼出或隐藏 Kaku，它跟着当前键盘布局走，用 Colemak 或 Dvorak 时就是打出 K 的那个键，想换组合的话在 `kaku config` 里改 Global Hotkey，或者直接写进 Lua：

```lua
config.macos_global_hotkey = { key = 'j', mods = 'CMD|OPT|CTRL' }
```

## Kaku 能和 yabai、AeroSpace 这类平铺窗口管理器一起用吗？

能用。如果一直闪，通常是平铺 WM 和 Kaku 的全屏、resize 逻辑在打架，把 Kaku 的原生全屏关掉（`config.native_macos_fullscreen_mode = false`），或者在平铺 WM 的管理列表里排除 Kaku，一般就好了。

---

Source: https://kaku.fun/zh/docs/faq
Site index for LLMs: https://kaku.fun/llms.txt
