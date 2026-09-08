# 配置

Lua 配置、外观、终端行为、键盘设置和 Assistant 选项。

### 打开配置

用 `kaku config` TUI 或 `Cmd + ,`。Kaku 会先加载默认值，再应用你的覆盖。

### 最常改的几项

先看 `font_size`、`window_background_opacity`、`copy_on_select` 和 `smart_tab_mode`。

## 配置文件

首次启动时，Kaku 会自动创建带注释模板的 `~/.config/kaku/kaku.lua`。运行 `kaku config` 或按 `Cmd + ,` 调整常用设置和 Lua 覆盖。

这个文件先加载 Kaku 内置的默认值，再把你的自定义覆盖叠在上面：

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

-- Your overrides go here:
config.font_size = 16
config.window_background_opacity = 0.95

return config
```

> 带全部注释示例的完整模板由 `kaku init` 自动生成。大多数用户只需要取消注释想改的那几行。

## 外观

**主题**

Kaku 默认跟随 macOS 外观，在 Kaku Dark 和 Kaku Light 之间自动切换。在 `kaku config` 里选择 Auto 即可恢复这一行为。要固定主题：

```lua
config.color_scheme = "Kaku Dark"   -- always dark
config.color_scheme = "Kaku Light"  -- always light
```

**颜色覆盖**

重映射特定的十六进制颜色，让那些自己输出颜色的应用也保持主题一致。`color_overrides` 作用于渲染出来的背景，包括基于调色板的 ANSI 背景和真彩色背景。`foreground_color_overrides` 只作用于真彩色文字：

```lua
config.color_overrides = {
  ['#6E6E6E'] = '#3A3942',
}

config.foreground_color_overrides = {
  ['#FFFFDB'] = '#575653',
}
```

**字体**

Kaku 默认使用 JetBrains Mono，CJK 回退到 PingFang SC。换字体：

```lua
config.font = wezterm.font("Fira Code")
```

Kaku 默认关闭连字。重新开启：

```lua
config.harfbuzz_features = {}
```

**字号**

Kaku 会根据你的显示器自动选 15pt（低分辨率）或 17pt（高分辨率）。手动覆盖：

```lua
config.font_size = 16
```

**行高**

```lua
config.line_height = 1.28  -- default
```

默认值优先保证文字阅读舒适。QR code、`neofetch` 图标、TUI 图表这类字符图形会跟着行高变高；如果想接近正方形，可以设为 `1.0` 到 `1.1`。细节见 [FAQ](https://kaku.fun/zh/docs/faq#faq-qr-codes-and-terminal-graphics-look-vertically-stretched)。

**窗口透明度**

```lua
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- optional blur (0–100)
```

**红绿灯按钮（macOS）**

默认情况下，Kaku 用 `INTEGRATED_BUTTONS|RESIZE` 把 macOS 的红绿灯按钮嵌进标签栏区域。要隐藏红绿灯、同时保留边缘缩放和标签栏拖动：

```lua
config.window_decorations = "RESIZE"
```

`RESIZE` 保留从边缘缩放窗口和用标签栏拖动的能力，只移除关闭/最小化/缩放按钮。

**内边距**

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

尺寸支持 `px`、`pt`、`cell` 和 `%` 四种单位。`px` 是物理像素，不随屏幕 DPI 缩放，同样的数值在高分屏上会显得更小。希望随 DPI 缩放用 `pt`，随终端单元格大小用 `cell`，例如 `top = '15pt'`。

## 终端行为

**光标**

```lua
config.default_cursor_style = "BlinkingBar"
config.cursor_thickness = "2px"
config.cursor_blink_rate = 500
```

**回滚缓冲**

```lua
config.scrollback_lines = 10000  -- default
```

**选中即复制**

默认开启。关闭：

```lua
config.copy_on_select = false
```

**复制时去掉行首空白**

复制带缩进的多行文本（比如从代码块里）时，去掉共有的行首空白， 让粘贴出来的内容从第 0 列开始：

```lua
config.copy_strip_leading_whitespace = true  -- default: false
```

**恢复上次会话**

启动时重新打开上次会话的标签和分屏。默认开启；设为 `false` 可关闭会话的保存与恢复：

```lua
config.restore_previous_session = false  -- default: true
```

通过 ssh 域打开的分屏会回到原来的远程目录，而不是本地一个同名路径。如果某个窗口恢复不了，比如主机连不上，Kaku 会用通知说明有几个窗口没能恢复，并保留已保存的会话，下次启动再试一次。

**工作目录继承**

```lua
config.window_inherit_working_directory = true   -- new windows
config.tab_inherit_working_directory = true       -- new tabs
config.split_pane_inherit_working_directory = true -- new splits
```

**标签栏**

只有一个标签时会隐藏。自动生成的标签标题默认显示当前目录；你可以改位置、缩短路径，或在路径旁显示前台命令：

```lua
config.tab_bar_at_bottom = false                   -- move to top
config.tab_title_show_basename_only = true         -- show "dirname" instead of "parent/dirname"
config.tab_title_show_foreground_process = true    -- show "dirname·codex" while commands run
```

后台标签触发 BEL 时，标题默认会显示一个小圆点。要关闭：

```lua
config.bell_tab_indicator = false
```

**滚动条**

默认关闭。通过 `kaku config`（切换滚动条样式选项）或在 Lua 里开启：

```lua
config.enable_scroll_bar = true
```

如果你想让鼠标滚轮在 nano、vim 这类备用屏幕应用里滚动，而不是去翻 Kaku 主回滚缓冲，开启：

```lua
config.alternate_screen_wheel_scrolls_terminal = true
```

**拖选 + 鼠标滚轮**

控制按住鼠标左键拖出选区时滚轮的行为。默认是 `"Extend"`（Kaku v0.11+）， 和 Safari、TextEdit、VS Code、iTerm2、`Terminal.app` 这些 macOS `NSTextView` 应用一致：滚轮滚动回滚缓冲，选区跟着光标跨屏增长。

```lua
-- Default (recommended): scroll AND extend the selection so you can grab
-- text that spans more than one screen of output.
config.selection_wheel_scroll_behavior = "Extend"

-- Scroll the scrollback but leave the selection range untouched.
config.selection_wheel_scroll_behavior = "ScrollOnly"

-- Drop the wheel event entirely. This is the legacy Kaku v0.10 behavior;
-- selecting text that does not fit on one screen requires releasing the
-- mouse, scrolling, and re-selecting.
config.selection_wheel_scroll_behavior = "Ignore"
```

> **v0.11 默认值变更**：更早的 Kaku 版本表现得像设了 `"Ignore"`。设 `selection_wheel_scroll_behavior = "Ignore"` 可恢复旧行为。

**macOS Option 键**

左 Option 发送 Meta（对 Vim/Neovim 的按词移动很有用）。右 Option 发送 compose 字符。

```lua
config.send_composed_key_when_left_alt_is_pressed = false  -- default: left = Meta
config.send_composed_key_when_right_alt_is_pressed = true  -- default: right = Compose
```

**关闭确认**

在你关闭一个仍有任务在跑的窗口、标签或分屏前，Kaku 可以先问一下。 每个选项都接受 `NeverPrompt`、`SmartPrompt` 或 `AlwaysPrompt`；内置配置把三个都设为 `SmartPrompt`：

```lua
config.window_close_confirmation = "SmartPrompt"  -- bundled default
config.tab_close_confirmation = "SmartPrompt"     -- bundled default
config.pane_close_confirmation = "SmartPrompt"    -- bundled default
```

当所有受影响的分屏都停在裸 shell 提示符上时，`SmartPrompt` 会立即关闭； 当还有 agent 或编辑器（claude、codex、vim 等）在跑时，它会先问一下。`Cmd + Q`、`Cmd + W`、 `Cmd + Shift + W` 都遵循这些设置。

## 更新

Kaku 默认会在后台检查 GitHub 的新版本，发现后安静地下载，但绝不会自动安装。通知会先让你确认，因为应用更新会关闭所有窗口并停止正在运行的任务。

完全关闭后台检查：

```lua
config.check_for_updates = false
```

调整检查频率（默认 `10800`，即每 3 小时一次）：

```lua
config.check_for_updates_interval_seconds = 86400  -- once a day
```

无论这些选项如何设置，你都可以随时运行 `kaku update`，或从应用菜单手动更新。

## 自定义快捷键

始终往 `config.keys` 里**追加**，不要替换它。替换会清掉 Kaku 的全部默认值。

```lua
-- Navigate pane right
table.insert(config.keys, {
  key = 'RightArrow',
  mods = 'CMD|SHIFT',
  action = wezterm.action.ActivatePaneDirection('Right'),
})

-- Split pane horizontally
table.insert(config.keys, {
  key = 'Enter',
  mods = 'CMD|OPT',
  action = wezterm.action.SplitHorizontal({ domain = 'CurrentPaneDomain' }),
})
```

可用动作的完整列表见 [WezTerm KeyAssignment 参考](https://wezfurlong.org/wezterm/config/lua/keyassignment/)。

## 高级

**企业代理请求头**

给 Kaku Assistant 的 API 请求加自定义 HTTP 头（用于企业代理或 API 网关）：

```toml
# ~/.config/kaku/assistant.toml
custom_headers = ["X-Customer-ID: your-id", "X-Org: your-org"]
```

注意：`Authorization` 和 `Content-Type` 是保留头，无法覆盖。

**扩展 Command Palette**

在 `kaku.lua` 里给 Command Palette（`Cmd + Shift + P`）添加自定义命令：

```lua
wezterm.on('augment-command-palette', function(window, pane)
  if not pane then return {} end

  local cwd_obj = pane:get_current_working_dir()
  if not cwd_obj then return {} end

  -- Finder can only reveal local paths. file_path is already URL-decoded,
  -- so directories containing spaces or non-ASCII characters work too.
  local host = cwd_obj.host
  if cwd_obj.scheme ~= 'file'
      or (host and host ~= '' and host ~= 'localhost' and host ~= wezterm.hostname()) then
    return {}
  end
  local cwd = cwd_obj.file_path
  if not cwd then return {} end

  return {
    {
      brief = 'Reveal in Finder',
      doc = 'Reveal current directory in Finder',
      action = wezterm.action_callback(function()
        wezterm.run_child_process({ 'open', '-R', cwd })
      end),
    },
  }
end)
```

**WezTerm Lua 配置**

Kaku 沿用 WezTerm 的配置系统。部分上游配置项在 Kaku 中行为不同，或已不再生效。完整参考见：

- [WezTerm 配置项](https://wezfurlong.org/wezterm/config/)
- [WezTerm Lua API](https://wezfurlong.org/wezterm/config/lua/)

---

Source: https://kaku.fun/zh/docs/configuration
Site index for LLMs: https://kaku.fun/llms.txt
