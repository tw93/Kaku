---
title: Lua 配置
description: Kaku 的 Lua 配置方式，完全兼容 WezTerm
---

# 配置

## 配置文件

Kaku 首次启动时会在 `~/.config/kaku/kaku.lua` 自动生成一份带注释的模板。可通过 `kaku config` 或 `Cmd + ,` 打开。

该文件会先加载 Kaku 内建的默认值，再把你的覆盖配置叠加在上面：

```lua
local wezterm = require 'wezterm'

local function resolve_bundled_config()
  local resource_dir = wezterm.executable_dir:gsub('MacOS/?$', 'Resources')
  local bundled = resource_dir .. '/kaku.lua'
  local f = io.open(bundled, 'r')
  if f then f:close(); return bundled end
  return '/Applications/Kaku.app/Contents/Resources/kaku.lua'
end

local config = {}
local bundled = resolve_bundled_config()
if bundled then
  local ok, loaded = pcall(dofile, bundled)
  if ok and type(loaded) == 'table' then config = loaded end
end

-- 在这里写你的覆盖配置：
config.font_size = 16
config.window_background_opacity = 0.95

return config
```

> 完整的模板（包含所有可选配置项的注释示例）由 `kaku init` 自动生成。大多数用户只需要取消注释自己想要修改的那几行即可。

---

## 外观

**主题**

Kaku 会跟随 macOS 系统的明暗模式自动切换。手动覆盖：

```lua
config.color_scheme = "Kaku Dark"   -- 始终使用深色
config.color_scheme = "Kaku Light"  -- 始终使用浅色
```

**颜色覆盖**

对特定十六进制颜色进行重映射，让输出自定义颜色的应用与主题保持一致：

```lua
config.color_overrides = {
  ['#6E6E6E'] = '#3A3942',
}
```

**字体**

Kaku 默认使用 JetBrains Mono，CJK 回退字体为 PingFang SC。修改字体：

```lua
config.font = wezterm.font("Fira Code")
```

Kaku 默认关闭连字。重新启用：

```lua
config.harfbuzz_features = {}
```

**字号**

Kaku 会根据显示器分辨率自动选择 15px（低分）或 17px（高分）。手动覆盖：

```lua
config.font_size = 16
```

**行高**

```lua
config.line_height = 1.28  -- 默认值
```

**窗口透明度**

```lua
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- 可选毛玻璃（0–100）
```

**交通灯按钮（macOS）**

Kaku 默认通过 `INTEGRATED_BUTTONS|RESIZE` 把 macOS 交通灯按钮嵌入到标签栏区域。如果你想隐藏交通灯，同时保留拖拽标签栏和从窗口边缘调整大小的能力：

```lua
config.window_decorations = "RESIZE"
```

`RESIZE` 会保留从窗口边缘调整大小、通过标签栏拖拽窗口的能力，只是去掉了关闭/最小化/最大化按钮。

**内边距**

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

---

## 终端行为

**光标**

```lua
config.default_cursor_style = "BlinkingBar"
config.cursor_thickness = "2px"
config.cursor_blink_rate = 500
```

**回滚缓冲**

```lua
config.scrollback_lines = 10000  -- 默认值
```

**选中即复制**

默认启用。关闭：

```lua
config.copy_on_select = false
```

**工作目录继承**

```lua
config.window_inherit_working_directory = true     -- 新窗口
config.tab_inherit_working_directory = true        -- 新标签
config.split_pane_inherit_working_directory = true -- 新分屏
```

**标签栏**

只有一个标签时自动隐藏。修改位置，或只显示当前目录名：

```lua
config.tab_bar_at_bottom = false            -- 移到顶部
config.tab_title_show_basename_only = true  -- 显示 "dirname" 而不是 "parent/dirname"
```

**滚动条**

默认关闭。可以通过 `kaku config`（切换滚动条样式选项）或者在 Lua 中开启：

```lua
config.enable_scroll_bar = true
```

**macOS Option 键**

左 Option 发送 Meta（对 Vim/Neovim 的单词导航很有用）。右 Option 发送 compose 字符。

```lua
config.send_composed_key_when_left_alt_is_pressed = false  -- 默认：左 = Meta
config.send_composed_key_when_right_alt_is_pressed = true  -- 默认：右 = Compose
```

---

## 自定义快捷键

永远使用**追加**的方式写入 `config.keys`，不要整体替换。替换会清空所有 Kaku 默认快捷键。

```lua
-- 向右切换 pane
table.insert(config.keys, {
  key = 'RightArrow',
  mods = 'CMD|SHIFT',
  action = wezterm.action.ActivatePaneDirection('Right'),
})

-- 水平分屏
table.insert(config.keys, {
  key = 'Enter',
  mods = 'CMD|OPT',
  action = wezterm.action.SplitHorizontal({ domain = 'CurrentPaneDomain' }),
})
```

完整的 action 列表：[WezTerm KeyAssignment 参考](https://wezfurlong.org/wezterm/config/lua/keyassignment/)。

---

## 进阶

**企业代理请求头**

为 Kaku Assistant 的 API 请求添加自定义 HTTP 头（用于企业代理或 API 网关）：

```toml
# ~/.config/kaku/assistant.toml
custom_headers = ["X-Customer-ID: your-id", "X-Org: your-org"]
```

注意：`Authorization` 和 `Content-Type` 是保留字段，不能被覆盖。

**完整的 WezTerm Lua API**

Kaku 复用了 WezTerm 的配置系统，任何 WezTerm 配置项都可以直接用在 `kaku.lua` 中。完整参考见：

- [WezTerm 配置项](https://wezfurlong.org/wezterm/config/)
- [WezTerm Lua API](https://wezfurlong.org/wezterm/config/lua/)
