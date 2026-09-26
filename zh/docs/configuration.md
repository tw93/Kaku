# 配置

字体、主题、标签栏、快捷键和助手怎么改，用设置界面或者 Lua 都行。

### 打开配置

运行 `kaku config` 或按 `Cmd + ,` 打开设置。

### 最常改的几项

先从 `font_size`、`window_background_opacity`、`copy_on_select` 和 `smart_tab_mode` 这几项改起。

## 配置文件

首次启动时 Kaku 会用一份带注释的模板生成 `~/.config/kaku/kaku.lua`，文件里先加载内置默认值，再叠上你自己的改动：

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

> 实际生成的文件里注释示例要多得多，`kaku init` 也会写出同样的文件，大多数时候把想改的那几行取消注释就够了。

设置界面（`kaku config`）改的也是这个文件里的常用设置和 Lua 覆盖，如果启动 Kaku 时用了 `--config-file`，设置改的就是那个文件。

## 外观

**主题**

Kaku 默认跟随 macOS 外观，在 Kaku Dark 和 Kaku Light 之间自动切换，想回到这种方式，在 `kaku config` 里选 Auto 就行。要固定用一套主题：

```lua
config.color_scheme = "Kaku Dark"   -- always dark
config.color_scheme = "Kaku Light"  -- always light
```

不加载内置默认值的独立配置也能用这两个主题名。如果你的配置里写了 `config.colors` 或 `config.window_frame`，这些颜色会盖过主题，`kaku config` 的 Theme 一栏也会标出是哪一项在覆盖。

**颜色覆盖**

有些应用会自己输出十六进制颜色，和主题搭不上，可以把这些颜色换掉。`color_overrides` 管渲染出来的背景，调色板里的 ANSI 背景和真彩色背景都算，`foreground_color_overrides` 只管真彩色文字：

```lua
config.color_overrides = {
  ['#6E6E6E'] = '#3A3942',
}

config.foreground_color_overrides = {
  ['#FFFFDB'] = '#575653',
}
```

**字体**

默认字体是 JetBrains Mono，中日韩字符回退到 PingFang SC，换字体这样写：

```lua
config.font = wezterm.font("Fira Code")
```

连字默认关闭，要重新打开：

```lua
config.harfbuzz_features = {}
```

**字号**

Kaku 会按显示器自动选字号，低分屏 15pt，高分屏 17pt，想自己定就写：

```lua
config.font_size = 16
```

**行高**

```lua
config.line_height = 1.28  -- default
```

默认行距偏松，读文字舒服一些，代价是 QR code、`neofetch` 图标、TUI 图表这类用字符拼出来的图形会跟着行高拉长，想让它们接近正方形可以设成 `1.0` 到 `1.1`，细节见 [FAQ](https://kaku.fun/zh/docs/faq#faq-qr-codes-and-terminal-graphics-look-vertically-stretched)。

**窗口透明度**

```lua
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- optional blur (0–100)
```

**红绿灯按钮（macOS）**

默认用 `INTEGRATED_BUTTONS|RESIZE` 把红绿灯按钮嵌在标签栏里。想去掉关闭、最小化、缩放这三个按钮，同时保留拖边缘缩放和按住标签栏拖动窗口：

```lua
config.window_decorations = "RESIZE"
```

**内边距**

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

单位支持 `px`、`pt`、`cell` 和 `%`。`px` 是物理像素，不跟着 DPI 缩放，同一个数在高分屏上看着会更小，想随 DPI 缩放就用 `pt`，比如 `top = '15pt'`，想按终端字符格的大小算就用 `cell`。

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

默认开启，要关掉：

```lua
config.copy_on_select = false
```

**复制时去掉行首空白**

复制一段带缩进的多行文本时，比如代码，把每行共有的行首空白去掉，粘贴出来就从第 0 列开始：

```lua
config.copy_strip_leading_whitespace = true  -- default: false
```

**恢复上次会话**

Kaku 启动时会重新打开上次的标签和分屏，这个功能默认开启，设为 `false` 就既不保存也不恢复会话：

```lua
config.restore_previous_session = false  -- default: true
```

通过 ssh 域打开的分屏会回到原来的远程目录，不会落到本地的同名路径上。某个窗口恢复不了时，比如主机连不上，Kaku 会发通知告诉你有几个窗口没恢复，已保存的会话留着，下次启动再试一次。

**工作目录继承**

```lua
config.window_inherit_working_directory = true   -- new windows
config.tab_inherit_working_directory = true       -- new tabs
config.split_pane_inherit_working_directory = true -- new splits
```

**标签栏**

只有一个标签时标签栏会隐藏，自动生成的标签标题默认显示当前目录，你可以调整标签栏位置、缩短路径，或者在路径旁边显示正在跑的命令：

```lua
config.tab_bar_at_bottom = false                   -- move to top
config.tab_title_show_basename_only = true         -- show "dirname" instead of "parent/dirname"
config.tab_title_show_foreground_process = true    -- show "dirname·codex" while commands run
```

后台标签响铃（BEL）时，标题上会出现一个小圆点，要关掉：

```lua
config.bell_tab_indicator = false
```

新建标签按钮默认不显示，可以在 `kaku config` 里打开 New Tab Button，或者写 `config.show_new_tab_button_in_tab_bar = true`。

**滚动条**

默认关闭，可以在 `kaku config` 里打开 Scrollbar，或者在 Lua 里写：

```lua
config.enable_scroll_bar = true
```

想让鼠标滚轮在 nano、vim 这类备用屏幕应用里滚动，而不是去翻 Kaku 的主回滚缓冲，打开这一项：

```lua
config.alternate_screen_wheel_scrolls_terminal = true
```

**拖选 + 鼠标滚轮**

控制按住左键拖选时滚轮的行为。从 v0.11 起默认是 `"Extend"`，滚轮会滚动回滚缓冲，选区跟着光标跨屏延伸，和 Safari、TextEdit、VS Code、iTerm2、`Terminal.app` 的表现一样。

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

> **v0.11 默认值变更**：更早的版本相当于设了 `"Ignore"`，想要回旧行为就设 `selection_wheel_scroll_behavior = "Ignore"`。

**macOS Option 键**

左 Option 发送 Meta，在 Vim、Neovim 里按词移动时用得上，右 Option 用来输入组合字符。

```lua
config.send_composed_key_when_left_alt_is_pressed = false  -- default: left = Meta
config.send_composed_key_when_right_alt_is_pressed = true  -- default: right = Compose
```

**关闭确认**

关闭还有任务在跑的窗口、标签或分屏前，Kaku 可以先问一句。每一项都可以设为 `NeverPrompt`、`SmartPrompt` 或 `AlwaysPrompt`，内置配置三项都是 `SmartPrompt`：

```lua
config.window_close_confirmation = "SmartPrompt"  -- bundled default
config.tab_close_confirmation = "SmartPrompt"     -- bundled default
config.pane_close_confirmation = "SmartPrompt"    -- bundled default
```

设成 `SmartPrompt` 时，受影响的分屏都停在空的 shell 提示符上就直接关，还有 claude、codex、vim 这类 agent 或编辑器在跑就先问。`Cmd + Q`、`Cmd + W`、`Cmd + Shift + W` 都按这些设置来。

## 更新

Kaku 默认会在后台检查 GitHub 上的新版本，有新版就先下载好，但不会自己装，因为装更新会关掉所有窗口、停掉正在跑的任务，所以通知会先问你。

要关掉后台检查：

```lua
config.check_for_updates = false
```

调整检查频率，默认 `10800`，也就是每 3 小时一次：

```lua
config.check_for_updates_interval_seconds = 86400  -- once a day
```

不管这些选项怎么设，都可以随时运行 `kaku update` 或从应用菜单手动更新。

## 自定义快捷键

往 `config.keys` 里只能**追加**，不要整个替换，替换会把 Kaku 的默认快捷键全部清掉。

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

所有可用动作见 [WezTerm KeyAssignment 参考](https://wezfurlong.org/wezterm/config/lua/keyassignment/)。

## 高级

**企业代理请求头**

如果公司的代理或 API 网关要求带额外的 HTTP 头，可以加到 Kaku Assistant 的 API 请求里：

```toml
# ~/.config/kaku/assistant.toml
custom_headers = ["X-Customer-ID: your-id", "X-Org: your-org"]
```

`Authorization` 和 `Content-Type` 是保留头，不能覆盖。

**扩展 Command Palette**

在 `kaku.lua` 里可以给 Command Palette（`Cmd + Shift + P`）加自己的命令，下面这个例子会在 Finder 里定位当前目录：

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

Kaku 沿用 WezTerm 的配置系统，不过有些上游配置项在 Kaku 里表现不同，或者已经不生效了。完整参考见：

- [WezTerm 配置项](https://wezfurlong.org/wezterm/config/)
- [WezTerm Lua API](https://wezfurlong.org/wezterm/config/lua/)

---

Source: https://kaku.fun/zh/docs/configuration
Site index for LLMs: https://kaku.fun/llms.txt
