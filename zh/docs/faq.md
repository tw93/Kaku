# FAQ

安装、配置、快捷键、AI 设置、滚动、字体、Homebrew 和通知相关问题。

## 怎么安装 Kaku？

从 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载 DMG，或运行 `brew install tw93/tap/kakuku`。注意要用 tap 包，不要装到 Homebrew 上那个无关的 `kaku` 包。

## 安装后先跑什么？

先打开一次 Kaku，再运行 `kaku doctor`。如果 shell 找不到 `kaku`，运行 `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only`，再用 `exec zsh -l` 重启 shell。

## 有 Windows 或 Linux 版本吗？

暂时没有。Kaku 现在只做 macOS，等 macOS 体验打磨稳定后再考虑 Windows 和 Linux。

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

运行 `kaku ai`，Auth Type 保持 API key，手动填写 OpenAI 兼容 Base URL、API key、Simple Model 和 Deep Model。Base URL 填 API 根地址，例如 `https://api.openai.com/v1`，API Mode 按服务选择 `chat_completions` 或 `responses`。

## 怎么恢复默认配置？

```bash
kaku reset
```

它会移除 Kaku 管理的 shell 和 tmux 集成、Kaku 管理的 git delta 默认值、部分 Kaku 状态，以及 `~/.config/kaku/kaku.lua` 里的托管主题块。托管块之外的用户 Lua 会保留。如果还想恢复 shell 集成，再运行 `kaku init`。

## `kaku` 命令找不到了，怎么恢复？

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

然后跑 `kaku doctor` 验证一切正常。

## 怎么在脚本里用 Kaku 的 CLI？

```bash
kaku cli split-pane
kaku cli split-pane -- bash -c "echo hello"
kaku cli --help
```

完整参考见 [CLI 参考](https://kaku.fun/zh/docs/cli)。

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

字体改动需要显式设置 `config.font`：

```lua
config.font = wezterm.font('Your Font Name')
```

提示：Kaku 主题感知的字重系统只对默认的 JetBrains Mono 字体栈生效。设了自定义字体之后，Kaku 不会再自动覆盖字重。

## `window_padding` 改了没效果。

`window_padding` 的值需要带 `'px'` 单位：

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

纯数字（不带 `'px'`）会被当成终端 cell 单位，通常不是你想要的。

## QR code 和终端图形看起来被纵向拉高。

Kaku 默认的 `line_height = 1.28` 优先保证文字阅读舒适。QR code、`neofetch` 图标、TUI 柱状图这类由字符组成的图形会跟着行高缩放，所以会比无额外行距的终端高约 28%。这是排版取舍，不是渲染 bug：块字符必须填满整个 cell，TUI 边框和进度条才不会断开。

如果你想让图形接近正方形，可以在 `~/.config/kaku/kaku.lua` 里降低行高：

```lua
config.line_height = 1.1  -- 或用 1.0 对齐无额外行距的终端
```

注意，没有终端能把半块字符组成的 QR code 渲染成完全正方形：常见等宽字体的 cell 即使在 `line_height = 1.0` 时也天然略高于 2:1。

## Claude Code 输出过程中屏幕会跳到顶部。

这是触控板滚动和 Claude Code 流式输出之间的已知交互。如果中途不小心滚到了顶，按向下方向键或往下滚就能回到当前输出。跳跃行为在最近几个版本里已经修。

## SSH 会话里按 Cmd+Shift+Y 打开的是本地路径。

yazi 远端文件功能（`Cmd+Shift+R`）是为 SSH 会话设计的，通过 sshfs 挂载远端文件系统。`Cmd+Shift+Y` 是本地 yazi。SSH 分屏里要用 `Cmd+Shift+R`。

## ssh 连着的时候，AI 聊天不读文件。

这是故意的。工具跑在你的 Mac 上，但当前目录属于远程主机，在本地读或跑只会悄悄打到一个同名的本地路径上。在远程分屏里，聊天面板基于终端上的内容回答，并给出你能在主机上执行的命令；`@cwd` 出于同样原因不可用。想让工具处理远程文件，先用 `Cmd + Shift + R` 挂载。

## `y` 这个 shell 包装退出时不同步当前目录。

确认 shell 集成已 source，用 `kaku doctor` 检查。`y` 包装依赖 shell init，直接调用 `yazi` 不同步目录。

## Homebrew 找不到二进制 / 升级错了 kaku 包。

Homebrew 上有一个同名但无关的旧 `kaku` 包。装 Kaku 要用 tap 才能避免冲突：

```bash
brew install tw93/tap/kakuku
```

如果 `kaku update` 出 checksum 错误，直接用 `brew upgrade tw93/tap/kakuku`。

## Claude Code 的通知不出现。

Kaku 可能没拿到通知权限。打开 System Settings > Notifications > Kaku，开启 Allow Notifications，然后重启 Kaku。

## 全局快捷键在非 QWERTY 键位（比如 Colemak）上不工作。

`Cmd + Opt + Ctrl + K` 用的是物理 QWERTY 的 K 位置，Colemak 上对应的是不同键位。在配置里 remap：

```lua
table.insert(config.keys, {
  key = 'k',  -- 改成你布局上的物理键
  mods = 'CMD|OPT|CTRL',
  action = wezterm.action.EmitEvent('toggle-global-window'),
})
```

## Kaku 能和 yabai、AeroSpace 这类平铺窗口管理器一起用吗？

Kaku 兼容 yabai 和 AeroSpace。如果遇到持续闪烁，通常是平铺 WM 和 Kaku 的全屏/resize 逻辑在打架。把 Kaku 的原生全屏关掉（`config.native_macos_fullscreen_mode = false`）或者在平铺 WM 的管理列表里排除 Kaku，通常就能解决。

---

Source: https://kaku.fun/zh/docs/faq
Site index for LLMs: https://kaku.fun/llms.txt
