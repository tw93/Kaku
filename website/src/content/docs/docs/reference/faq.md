---
title: FAQ
description: Kaku 常见问题解答
---

# FAQ

## 有 Windows 或 Linux 版本吗？

暂时没有。在 macOS 体验打磨完成之前，Kaku 只支持 macOS。Windows 和 Linux 可能会在之后推出。

## 可以使用透明窗口吗？

可以，在 `~/.config/kaku/kaku.lua` 中添加：

```lua
local config = require("kaku").config
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- 可选毛玻璃，0–100
return config
```

## 怎么关闭"选中即复制"？

```lua
config.copy_on_select = false
```

## 怎么自定义快捷键？

向 `config.keys` 中**追加**，不要整体替换：

```lua
config.keys[#config.keys + 1] = {
  key = "RightArrow",
  mods = "CMD|SHIFT",
  action = wezterm.action.ActivatePaneDirection("Right"),
}
```

更多示例见 [快捷键](/Kaku/docs/config/keybindings/) 和 [Lua 配置](/Kaku/docs/config/lua/)。

## 可以控制工作目录的继承行为吗？

可以，窗口、标签、分屏分别独立控制：

```lua
config.window_inherit_working_directory = true
config.tab_inherit_working_directory = true
config.split_pane_inherit_working_directory = true
```

以上都默认启用。

## 怎么禁用 Kaku Assistant？

运行 `kaku ai`，打开 Kaku Assistant 的设置页，把 Enabled 关掉。或者直接编辑 `~/.config/kaku/assistant.toml`：

```toml
enabled = false
```

## 怎么使用自定义 LLM Provider？

运行 `kaku ai`，从 Provider 下拉菜单选择 "Custom"，手动填入 Base URL 和 API Key。URL 必须兼容 OpenAI 协议（`/v1/chat/completions`）。

## 怎么恢复默认配置？

```bash
kaku reset
```

此命令会用默认值覆盖 `~/.config/kaku/kaku.lua`。

## `kaku` 命令丢失了，怎么恢复？

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

然后运行 `kaku doctor` 检查一切是否正常。

## 怎么从脚本里使用 Kaku CLI？

```bash
kaku cli split-pane
kaku cli split-pane -- bash -c "echo hello"
kaku cli --help
```

完整参考见 [CLI 命令](/Kaku/docs/reference/cli/)。

## 怎么开启滚动条？

打开 `kaku config` 切换滚动条选项，或在 `~/.config/kaku/kaku.lua` 中加上：

```lua
config.enable_scroll_bar = true
```

## 我改了字体，但没生效？

字体修改需要在配置中显式设置 `config.font`：

```lua
config.font = wezterm.font('Your Font Name')
```

注意：Kaku 的主题感知字重系统只对默认的 JetBrains Mono 字体栈生效。一旦你设置了自定义字体，Kaku 就不会再自动覆盖字重了。

## 我改了 `window_padding` 但不生效？

`window_padding` 的值需要带 `'px'` 单位后缀：

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

纯数字（不带 `'px'`）会被解释为"终端字符单元"，通常不是你想要的结果。

## Claude Code 输出时屏幕会跳到最上面。

这是触控板滚动和 Claude Code 流式输出之间的已知交互问题。如果在生成过程中不小心滚到了顶部，按向下键或滚回底部即可回到当前输出。这一跳动行为的修复已经在近期版本中发布。

## 在 SSH 会话中按 Cmd+Shift+Y 发送的是本地路径？

yazi 的远程文件功能（`Cmd+Shift+R`）才是为 SSH 会话设计的，它会通过 sshfs 挂载远程文件系统。`Cmd+Shift+Y` 用于本地 yazi。在 SSH pane 中请使用 `Cmd+Shift+R`。

## shell 包装命令 `y` 退出后没有同步目录。

请确认 Kaku 的 fish/zsh shell 集成已加载，可用 `kaku doctor` 检查。`y` 包装命令依赖 shell 初始化脚本——直接运行 `yazi` 不会同步目录。

## Homebrew 找不到对应的二进制 / 更新错了 kaku。

Homebrew 上有另一个早期的同名包 `kaku`，跟 Kaku 无关。请通过 tap 安装以避免冲突：

```bash
brew install tw93/tap/kakuku
```

如果 `kaku update` 报 checksum 错误，直接用 `brew upgrade tw93/tap/kakuku`。

## Claude Code 的通知没有弹出？

Kaku 的通知权限可能没有授予。打开 系统设置 > 通知 > Kaku，启用"允许通知"，然后重启 Kaku。

## 在 Colemak 等非 QWERTY 键盘上全局热键失效。

`Cmd + Opt + Ctrl + K` 使用的是 QWERTY 物理位置上的 K 键。在 Colemak 上对应的是另一个键，可在配置里重新映射：

```lua
table.insert(config.keys, {
  key = 'k',  -- 改成你布局下对应的物理键
  mods = 'CMD|OPT|CTRL',
  action = wezterm.action.EmitEvent('toggle-global-window'),
})
```

## 可以和平铺窗口管理器（yabai、AeroSpace）一起使用吗？

Kaku 兼容 yabai 和 AeroSpace。如果遇到持续闪烁，通常是因为平铺 WM 和 Kaku 的全屏/调整大小逻辑在打架。关闭 Kaku 的原生全屏（`config.native_macos_fullscreen_mode = false`），或者在平铺 WM 的管理窗口列表里把 Kaku 排除掉，一般就能解决。
