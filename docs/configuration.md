# Configuration

Change fonts, themes, the tab bar, shortcuts, and the assistant, in Settings or in Lua.

### Open config

Run `kaku config` or press `Cmd + ,` to open Settings.

### Most common edits

Start with `font_size`, `window_background_opacity`, `copy_on_select`, and [`smart_tab_mode`](https://kaku.fun/docs/features#features-shell-suite).

## Config File

On first launch Kaku creates `~/.config/kaku/kaku.lua` from a commented template. It loads the bundled defaults first and applies your overrides on top:

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

> The generated file, which `kaku init` also writes, has many more commented examples, so most of the time you only uncomment the lines you want.

Settings (`kaku config`) edits common settings and Lua overrides in this same file. If you launched Kaku with `--config-file`, Settings edits that file instead.

## Appearance

**Theme**

New installs start in Kaku Dark, because the generated `kaku.lua` sets `config.color_scheme = "Kaku Dark"`. A config without that line, including any created before V0.21.0, keeps following the macOS appearance and switches between Kaku Dark and Kaku Light. Pick Auto in `kaku config` to follow macOS. To pin one theme:

```lua
config.color_scheme = "Kaku Dark"   -- always dark
config.color_scheme = "Kaku Light"  -- always light
```

Both names also work in a standalone config that does not load the bundled defaults. If your file sets `config.colors` or `config.window_frame`, those colors win over the theme, and the Theme row in `kaku config` names the key that is overriding it.

**Color overrides**

When an app prints its own hex colors that clash with the theme, remap them. `color_overrides` covers rendered backgrounds, both ANSI palette and truecolor, while `foreground_color_overrides` covers truecolor text only:

```lua
config.color_overrides = {
  ['#6E6E6E'] = '#3A3942',
}

config.foreground_color_overrides = {
  ['#FFFFDB'] = '#575653',
}
```

**Font**

The default font is JetBrains Mono, with PingFang SC as the CJK fallback. To change it:

```lua
config.font = wezterm.font("Fira Code")
```

Ligatures are off by default. To turn them back on:

```lua
config.harfbuzz_features = {}
```

**Font size**

Kaku picks 15pt on low-resolution displays and 17pt on high-resolution ones. To set your own:

```lua
config.font_size = 16
```

**Line height**

```lua
config.line_height = 1.28  -- default
```

The default leaves room between lines for easier reading, but character-cell graphics such as QR codes, `neofetch` logos, and TUI charts stretch with the row height. Set it between `1.0` and `1.1` if you want them close to square. More in the [FAQ](https://kaku.fun/docs/faq#faq-qr-codes-and-terminal-graphics-look-vertically-stretched).

**Window transparency**

```lua
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- optional blur (0–100)
```

**Traffic lights (macOS)**

By default the traffic light buttons sit in the tab bar (`INTEGRATED_BUTTONS|RESIZE`). To remove the close, minimize, and zoom buttons while keeping edge resizing and dragging by the tab bar:

```lua
config.window_decorations = "RESIZE"
```

**Padding**

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

Values accept `px`, `pt`, `cell`, and `%`. `px` means physical pixels and does not scale with DPI, so the same number looks smaller on a high-density display. Use `pt` to scale with DPI, for example `top = '15pt'`, or `cell` to size relative to the terminal cell.

## Terminal Behavior

**Cursor**

```lua
config.default_cursor_style = "BlinkingBar"
config.cursor_thickness = "2px"
config.cursor_blink_rate = 500
```

**Scrollback**

```lua
config.scrollback_lines = 10000  -- default
```

**Copy on select**

On by default. To turn it off:

```lua
config.copy_on_select = false
```

**Strip leading whitespace on copy**

When you copy an indented block of lines, such as code, this strips the indentation every line shares so the paste starts at column 0:

```lua
config.copy_strip_leading_whitespace = true  -- default: false
```

**Restore previous session**

Kaku reopens the tabs and panes from your last session on launch, and this is on by default. The session is also saved within a minute of opening or closing a window, tab, or pane, so a crash or force quit still brings back the last layout. Set it to `false` to stop both saving and restoring:

```lua
config.restore_previous_session = false  -- default: true
```

Panes opened through an ssh domain come back in their remote working directory, not a local path with the same name. If a saved window cannot be restored, say the host is unreachable, a notification tells you how many windows were left out, and Kaku keeps the saved session to try again on the next launch.

**Working directory inheritance**

```lua
config.window_inherit_working_directory = true   -- new windows
config.tab_inherit_working_directory = true       -- new tabs
config.split_pane_inherit_working_directory = true -- new splits
```

**Tab bar**

The tab bar stays hidden while only one tab is open, and automatic tab titles show the current directory. You can move the bar, shorten path titles, or show the running command next to the path:

```lua
config.tab_bar_at_bottom = false                   -- move to top
config.tab_title_show_basename_only = true         -- show "dirname" instead of "parent/dirname"
config.tab_title_show_foreground_process = true    -- show "dirname·codex" while commands run
```

When a background tab rings the bell (BEL), its title shows a small dot. To turn that off:

```lua
config.bell_tab_indicator = false
```

The new-tab button is hidden by default. Turn on New Tab Button in `kaku config`, or set `config.show_new_tab_button_in_tab_bar = true`.

**Scrollbar**

Off by default. Turn on Scrollbar in `kaku config`, or in Lua:

```lua
config.enable_scroll_bar = true
```

To have the mouse wheel scroll inside alternate-screen apps such as nano and vim, rather than showing Kaku's main scrollback, enable:

```lua
config.alternate_screen_wheel_scrolls_terminal = true
```

**Selection drag + mouse wheel**

Sets what the mouse wheel does while you hold the left button to drag out a selection. The default since v0.11 is `"Extend"`, which scrolls the scrollback and grows the selection to follow the cursor across screens, the way Safari, TextEdit, VS Code, iTerm2, and `Terminal.app` behave.

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

> **Default change in v0.11**: earlier versions behaved like `"Ignore"`. Set `selection_wheel_scroll_behavior = "Ignore"` to get that back.

**macOS Option key**

Left Option sends Meta, handy for word navigation in Vim and Neovim. Right Option types composed characters.

```lua
config.send_composed_key_when_left_alt_is_pressed = false  -- default: left = Meta
config.send_composed_key_when_right_alt_is_pressed = true  -- default: right = Compose
```

**Close confirmation**

Kaku can ask before closing a window, tab, or pane that still has work running. Each option takes `NeverPrompt`, `SmartPrompt`, or `AlwaysPrompt`, and the bundled config sets all three to `SmartPrompt`:

```lua
config.window_close_confirmation = "SmartPrompt"  -- bundled default
config.tab_close_confirmation = "SmartPrompt"     -- bundled default
config.pane_close_confirmation = "SmartPrompt"    -- bundled default
```

`SmartPrompt` closes right away when every affected pane is sitting at a bare shell prompt, and asks first when an agent or editor such as claude, codex, or vim is still running. `Cmd + Q`, `Cmd + W`, and `Cmd + Shift + W` all follow these settings.

## Updates

By default Kaku checks GitHub for new releases in the background and downloads a newer version when it finds one. It never installs by itself. Installing closes every window and stops running tasks, so the notification asks you first.

To turn off background checks:

```lua
config.check_for_updates = false
```

To change how often it checks (default `10800`, every 3 hours):

```lua
config.check_for_updates_interval_seconds = 86400  -- once a day
```

Whatever these are set to, you can still update by hand with `kaku update` or from the app menu.

## Custom Keybindings

Always **insert** into `config.keys`. Assigning a new table replaces it and drops every Kaku default binding.

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

For every available action, see the [WezTerm KeyAssignment reference](https://wezfurlong.org/wezterm/config/lua/keyassignment/).

## Advanced

**Enterprise proxy headers**

If a corporate proxy or API gateway needs extra HTTP headers, add them to Kaku Assistant's API requests:

```toml
# ~/.config/kaku/assistant.toml
custom_headers = ["X-Customer-ID: your-id", "X-Org: your-org"]
```

`Authorization` and `Content-Type` are reserved and cannot be overridden.

**Extend Command Palette**

You can add your own commands to the Command Palette (`Cmd + Shift + P`) from `kaku.lua`. This one reveals the current directory in Finder:

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

**WezTerm Lua configuration**

Kaku uses WezTerm's configuration system, though some upstream options behave differently or no longer take effect in Kaku. The full upstream reference:

- [WezTerm config options](https://wezfurlong.org/wezterm/config/)
- [WezTerm Lua API](https://wezfurlong.org/wezterm/config/lua/)

---

Source: https://kaku.fun/docs/configuration
Site index for LLMs: https://kaku.fun/llms.txt
