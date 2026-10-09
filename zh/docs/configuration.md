# 配置

字体、主题、标签栏、快捷键和助手怎么改，用设置界面或者 Lua 都行。

### 打开配置

运行 `kaku config` 或按 `Cmd + ,` 打开设置。

### 最常改的几项

先从 `font_size`、`window_background_opacity`、`copy_on_select` 和 [`smart_tab_mode`](https://kaku.fun/zh/docs/features#features-shell-suite) 这几项改起。

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

新安装默认使用 Kaku Dark 主题（首次生成的 `kaku.lua` 包含 `config.color_scheme = "Kaku Dark"`）。若配置中未指定该项（包括 V0.21.0 之前生成的配置），则仍会跟随 macOS 系统外观在 Dark 与 Light 间切换。若需跟随系统，可在 `kaku config` 中选择 Auto。若需固定单一主题：

```lua
config.color_scheme = "Kaku Dark"   -- always dark
config.color_scheme = "Kaku Light"  -- always light
```

不加载内置默认值的独立配置也能用这两个主题名。如果你的配置里写了 `config.colors` 或 `config.window_frame`，这些颜色会盖过主题，`kaku config` 的 Theme 一栏也会标出是哪一项在覆盖。

**颜色覆盖**

部分命令行程序会直接输出特定十六进制颜色，可能与主题风格不协调。可通过颜色覆盖规则予以替换：`color_overrides` 映射背景色（包含调色板 ANSI 背景与 24 位真彩色背景），`foreground_color_overrides` 仅映射真彩色文字：

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

Kaku 会根据显示器分辨率自动适配字号（普通屏 15pt，Retina 高分屏 17pt）。如需手动指定：

```lua
config.font_size = 16
```

**行高**

```lua
config.line_height = 1.28  -- default
```

默认行距适度放宽，文本阅读更为舒适；QR code、`neofetch` 图标及 TUI 图表等字符图形会随之成比例拉伸。若希望图形更接近正方形，可调整为 `1.0` 至 `1.1`，详情见 [FAQ](https://kaku.fun/zh/docs/faq#faq-qr-codes-and-terminal-graphics-look-vertically-stretched)。

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

单位支持 `px`、`pt`、`cell` 和 `%`。其中 `px` 为物理像素，不随 DPI 缩放；若希望随系统缩放，请使用 `pt`（如 `top = '15pt'`）；若需按字符网格单元计算，可使用 `cell`。

## 终端行为

**光标**

```lua
config.default_cursor_style = "BlinkingBar"
config.cursor_thickness = "2px"
config.cursor_blink_rate = 500
```

**输入法（IME）**

默认情况下，输入法正在组合的文字会覆盖光标后的字符。如果希望组合文字以插入方式显示、不遮挡后续字符：

```lua
config.ime_preedit_rendering = "BuiltinInsert"
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

Kaku 默认在启动时自动恢复上次打开的标签页与分屏。在窗口、标签或分屏发生变化后 1 分钟内会自动保存；即使遭遇崩溃或强制退出，也能恢复前次布局。若设为 `false`，则既不保存也不恢复会话：

```lua
config.restore_previous_session = false  -- default: true
```

通过 SSH 域打开的分屏会准确恢复至对应的远程目录，不会误退至本地同名路径。若因主机无法连接等原因导致部分窗口恢复失败，Kaku 会发送通知提示未恢复的窗口数量，并保留已存会话以便下次启动时再次尝试。

**工作目录继承**

```lua
config.window_inherit_working_directory = true   -- new windows
config.tab_inherit_working_directory = true       -- new tabs
config.split_pane_inherit_working_directory = true -- new splits
```

**标签栏**

单标签时标签栏默认隐藏。自动生成的标签标题默认显示当前目录名；你也可以调整标签栏位置、仅显示目录末段，或让它同步 Claude Code 等程序上报的动态标题，亦可在路径旁显示当前正在运行的前台命令：

```lua
config.tab_bar_at_bottom = false                   -- move to top
config.tab_title_show_basename_only = true         -- show "dirname" instead of "parent/dirname"
config.tab_title_show_foreground_process = true    -- show "dirname·codex" while commands run
config.tab_title_use_pane_title = true             -- show titles set by apps like Claude Code
```

开启 `config.tab_title_use_pane_title = true` 后，标签将优先显示 Claude Code 等命令行应用上报的动态标题；手动双击重命名过的标签仍会保留自定义名称。

后台标签响铃（BEL）时，标题上会出现一个小圆点，要关掉：

```lua
config.bell_tab_indicator = false
```

新建标签按钮默认显示，右键标签可以新建、打开标签导航或关闭标签。不需要新建按钮的话，可以在 `kaku config` 里关掉 New Tab Button，或者写 `config.show_new_tab_button_in_tab_bar = false`。

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

**Kitty 键盘协议**

开启后，Neovim 和 Herdr 里的 Esc 与向前删除键可正常识别，交换 Backspace 与 Delete 的设置也会生效：

```lua
config.enable_kitty_keyboard = true
```

**关闭确认**

在关闭仍有任务运行的窗口、标签或分屏时，Kaku 支持弹窗确认。每项均可配置为 `NeverPrompt`、`SmartPrompt` 或 `AlwaysPrompt`，内置默认均为 `SmartPrompt`：

```lua
config.window_close_confirmation = "SmartPrompt"  -- bundled default
config.tab_close_confirmation = "SmartPrompt"     -- bundled default
config.pane_close_confirmation = "SmartPrompt"    -- bundled default
```

设为 `SmartPrompt` 时，若相关分屏均处于空闲的 shell 提示符状态，则直接关闭；若有 Claude Code、Codex、Vim 等交互程序或编辑器在运行，则会弹出确认对话框。快捷键 `Cmd + Q`、`Cmd + W`、`Cmd + Shift + W` 均遵循该规则。

## 更新

Kaku 默认在后台检查 GitHub 新版本并预先完成下载。为避免重启时关闭窗口与中断前台任务，应用不会静默安装，而是通过通知询问是否重启更新。

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
