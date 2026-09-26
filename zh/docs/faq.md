# FAQ

大家最常问的问题，从安装、配置到字体、滚动和 AI。

## 怎么安装 Kaku？

从 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载 DMG，或运行 `brew install --cask kaku`。

## 安装后先跑什么？

先打开一次 Kaku，再运行 `kaku doctor`。如果 shell 找不到 `kaku`，运行 `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only`，再用 `exec zsh -l` 重启 shell。

## 有 Windows 或 Linux 版本吗？

暂时没有，Kaku 现在只做 macOS，等 Mac 上的体验打磨好了，Windows 和 Linux 可能会再跟上。

## Kaku 和 iTerm2、Warp、Ghostty、WezTerm 有什么区别？

Kaku 是基于 WezTerm 的 Mac 终端，字体、主题、标签页、分屏和 shell 工具装好就配齐了，另有一个可选的 AI 助手，用的是你自己配置的 AI 服务。iTerm2 和 WezTerm 这些得自己一点点拼，Warp 是围绕账号来做的商业 AI 产品，Ghostty 是很快的 GPU 终端，只是预设少一些。哪些情况其实不用换，[对比页](https://kaku.fun/zh/compare)里写得更细。

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

运行 `kaku ai`，Auth Type 保持 API key，手动填上 OpenAI 兼容的 Base URL、API key、Simple Model 和 Deep Model，Base URL 填 API 根地址，比如 `https://api.openai.com/v1`，API Mode 按服务选 `chat_completions` 或 `responses`，用 Responses 的服务如果自带联网搜索，把 Native Web Search 打开就行，不用再单独配搜索服务和它的 API key。

## 怎么恢复默认配置？

```bash
kaku reset
```

它会清掉 Kaku 管理的 shell 和 tmux 集成、git delta 默认配置、部分 Kaku 状态，以及 `~/.config/kaku/kaku.lua` 里由 Kaku 管理的主题块，你在这些块之外自己写的 Lua 会原样保留，之后想把 shell 集成装回来，再跑一次 `kaku init` 就行。

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

全部命令见 [CLI 参考](https://kaku.fun/zh/docs/cli)。

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

Kaku 跟着主题调字重只对默认的 JetBrains Mono 字体栈生效，换成你自己的字体后，Kaku 就不再替你改字重了。

## `window_padding` 改了没效果。

`window_padding` 可以写数字，也可以写带单位的字符串，纯数字按像素算，`24` 和 `'24px'` 是一回事，其他单位还有 `'pt'`、`'%'` 和 `'cell'`：

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

## QR code 和终端图形看起来被纵向拉高。

Kaku 默认 `line_height = 1.28`，是为了让文字读起来不挤，QR code、`neofetch` 图标、TUI 柱状图这类用字符拼出来的图形会跟着行高一起拉伸，所以比没有额外行距的终端高出约 28%。这是排版上的取舍，不是渲染 bug，块字符必须填满整个 cell，TUI 边框和进度条才不会断开。

想让图形接近正方形，可以在 `~/.config/kaku/kaku.lua` 里把行高调低：

```lua
config.line_height = 1.1  -- 或用 1.0 对齐无额外行距的终端
```

不过没有哪个终端能把半块字符拼成的 QR code 画得完全正方，常见等宽字体的 cell 就算在 `line_height = 1.0` 下，高宽比也天然略大于 2:1。

## Claude Code 输出过程中屏幕会跳到顶部。

这是触控板滚动和 Claude Code 流式输出碰在一起的已知问题，中途不小心滚到顶的话，按一下向下方向键或往下滚就能回到当前输出，跳顶本身在最近几个版本里已经修了。

## SSH 会话里按 Cmd+Shift+Y 打开的是本地路径。

`Cmd+Shift+Y` 打开的是本地 yazi，SSH 分屏里要用 `Cmd+Shift+R`，这是 yazi 的远端文件功能，会通过 sshfs 把远端文件系统挂载过来。

## ssh 连着的时候，AI 聊天不读文件。

这是故意的。工具跑在你的 Mac 上，当前目录却在远程主机上，在本地读文件或跑命令只会悄悄落到一个同名的本地路径上，所以在远程分屏里，聊天面板只根据终端里的内容回答，并给出让你在主机上执行的命令，`@cwd` 也因为同样的原因用不了。想让工具处理远程文件，先用 `Cmd + Shift + R` 挂载。

## Kaku 的提示符跑到了别的终端里，或者在别的终端里没了。

每个 zsh 和 fish 都会加载 Kaku 的 shell 集成，但 Starship 提示符和 Smart Tab 只在 Kaku 里启动，从 Kaku 里开的 tmux 会话也算在内，其他终端还是用你原来的提示符。

想在所有终端都用 Kaku 的 Starship 提示符，在 `.zshrc` 里 Kaku 那一行之前加上这句，fish 就在 `config.fish` 里写 `set -gx KAKU_PROMPT_EVERYWHERE 1`：

```zsh
export KAKU_PROMPT_EVERYWHERE=1
```

Smart Tab 始终只在 Kaku 里生效，想关掉就设 `KAKU_SMART_TAB_DISABLE=1`。

## `y` 这个 shell 包装退出时不同步当前目录。

`y` 来自 Kaku 的 fish/zsh shell 集成，集成加载上了它才会在退出时同步目录，可以用 `kaku doctor` 检查，直接跑 `yazi` 不会同步目录。

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
