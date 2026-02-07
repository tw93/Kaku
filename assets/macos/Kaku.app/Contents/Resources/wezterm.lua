-- WezTerm Configuration

local wezterm = require 'wezterm'
local resurrect = wezterm.plugin.require("https://github.com/MLFlexer/resurrect.wezterm")
local config = {}

if wezterm.config_builder then
  config = wezterm.config_builder()
end

-- ===== Session Management (Resurrect Plugin) =====
resurrect.state_manager.set_max_nlines(10000)

-- Auto-save workspace every 5 minutes
wezterm.on('window-config-reloaded', function()
  if wezterm.GLOBAL.resurrect_autosave_started then
    return
  end
  wezterm.GLOBAL.resurrect_autosave_started = true

  local function save_workspace()
    resurrect.state_manager.save_state(resurrect.workspace_state.get_workspace_state())
    wezterm.time.call_after(300, save_workspace)
  end
  wezterm.time.call_after(300, save_workspace)
end)

local function basename(path)
  return path:match('([^/]+)$')
end

local function equal_padding(all)
  return {
    left = all,
    right = all,
    top = all,
    bottom = all,
  }
end

local function padding_matches(current, expected)
  return current
    and current.left == expected.left
    and current.right == expected.right
    and current.top == expected.top
    and current.bottom == expected.bottom
end

local fullscreen_uniform_padding = equal_padding('24px')

local function update_window_config(window, is_full_screen)
  local overrides = window:get_config_overrides() or {}
  if is_full_screen then
    if not padding_matches(overrides.window_padding, fullscreen_uniform_padding) or overrides.hide_tab_bar_if_only_one_tab ~= false then
      overrides.window_padding = fullscreen_uniform_padding
      overrides.hide_tab_bar_if_only_one_tab = false
      window:set_config_overrides(overrides)
    end
    return
  end

  if overrides.window_padding ~= nil or overrides.hide_tab_bar_if_only_one_tab ~= nil then
    overrides.window_padding = nil
    overrides.hide_tab_bar_if_only_one_tab = nil
    window:set_config_overrides(overrides)
  end
end

local function extract_path_from_cwd(cwd)
  if not cwd then
    return ''
  end

  local path = ''
  if type(cwd) == 'table' then
    path = cwd.file_path or cwd.path or tostring(cwd)
  else
    path = tostring(cwd)
  end

  path = path:gsub('^file://[^/]*', ''):gsub('/$', '')
  return path
end

local function tab_path_parts(pane)
  local cwd = pane.current_working_dir
  if not cwd and pane.get_current_working_dir then
    local ok, runtime_cwd = pcall(function()
      return pane:get_current_working_dir()
    end)
    if ok then
      cwd = runtime_cwd
    end
  end

  local path = extract_path_from_cwd(cwd)
  if path == '' then
    return '', ''
  end

  local current = basename(path) or path
  local parent_path = path:match('(.+)/[^/]+$') or ''
  local parent = basename(parent_path) or parent_path
  return parent, current
end

wezterm.on('format-tab-title', function(tab, _, _, _, _, max_width)
  local parent, current = tab_path_parts(tab.active_pane)
  local text = current
  if parent ~= '' and current ~= '' then
    text = parent .. '/' .. current
  end
  if text == '' then
    text = tab.active_pane.title
  end
  if tab.active_pane.is_zoomed then
    text = text .. ' [Z]'
  end
  text = wezterm.truncate_right(text, math.max(8, max_width - 2))

  local fg = tab.is_active and '#edecee' or '#6b6b6b'
  local intensity = tab.is_active and 'Bold' or 'Normal'
  return {
    { Attribute = { Intensity = intensity } },
    { Foreground = { Color = fg } },
    { Text = ' ' .. text .. ' ' },
  }
end)

wezterm.on('update-right-status', function(window)
  local dims = window:get_dimensions()
  update_window_config(window, dims.is_full_screen)
  if not dims.is_full_screen then
    window:set_right_status('')
    return
  end

  local clock_icon = wezterm.nerdfonts.md_clock_time_four_outline
    or wezterm.nerdfonts.md_clock_outline
    or ''
  local text = wezterm.strftime('%H:%M')
  if clock_icon ~= '' then
    window:set_right_status(wezterm.format({
      { Foreground = { Color = '#6b6b6b' } },
      { Text = ' ' .. clock_icon .. ' ' .. text .. ' ' },
    }))
    return
  end
  window:set_right_status(wezterm.format({
    { Foreground = { Color = '#6b6b6b' } },
    { Text = ' ' .. text .. ' ' },
  }))
end)

-- ===== Font =====
config.font = wezterm.font_with_fallback({
  'JetBrains Mono',
  { family = 'Apple Color Emoji', assume_emoji_presentation = true },
})

config.bold_brightens_ansi_colors = false
config.font_size = 17.0
config.line_height = 1.28
config.cell_width = 1.0
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
config.use_cap_height_to_scale_fallback_fonts = false

config.allow_square_glyphs_to_overflow_width = 'WhenFollowedBySpace'
config.custom_block_glyphs = true

config.freetype_load_target = 'Light'
config.freetype_render_target = 'HorizontalLcd'

-- ===== Cursor =====
config.default_cursor_style = 'BlinkingBar'
config.cursor_thickness = '2px'
config.cursor_blink_rate = 0

-- ===== Scrollback =====
config.scrollback_lines = 10000

-- ===== Mouse =====
config.selection_word_boundary = ' \t\n{}[]()"\'-'  -- Smart selection boundaries

-- ===== Window =====
config.window_padding = {
  left = '40px',
  right = '40px',
  top = '70px',
  bottom = '30px',
}

config.initial_cols = 110
config.initial_rows = 22
config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
config.window_background_opacity = 1.0
config.macos_window_background_blur = 0
config.adjust_window_size_when_changing_font_size = false
config.window_frame = {
  font = wezterm.font({ family = 'JetBrains Mono', weight = 'Bold' }),
  font_size = 13.0,
}

config.window_close_confirmation = 'NeverPrompt'

-- ===== Tab Bar =====
config.enable_tab_bar = true
config.tab_bar_at_bottom = true
config.use_fancy_tab_bar = false
config.tab_max_width = 32
config.hide_tab_bar_if_only_one_tab = true
config.show_tab_index_in_tab_bar = true
config.show_new_tab_button_in_tab_bar = false

-- Color scheme for tabs
config.colors = {
  -- Background
  foreground = '#edecee',
  background = '#15141b',

  -- Cursor
  cursor_bg = '#a277ff',
  cursor_fg = '#15141b',
  cursor_border = '#a277ff',

  -- Selection
  selection_bg = '#29263c',
  selection_fg = 'none',

  -- Normal colors (ANSI 0-7)
  ansi = {
    '#110f18',  -- black
    '#ff6767',  -- red
    '#61ffca',  -- green
    '#ffca85',  -- yellow
    '#a277ff',  -- blue
    '#a277ff',  -- magenta
    '#61ffca',  -- cyan
    '#edecee',  -- white
  },

  -- Bright colors (ANSI 8-15)
  brights = {
    '#4d4d4d',  -- bright black
    '#ff6767',  -- bright red
    '#61ffca',  -- bright green
    '#ffca85',  -- bright yellow
    '#a277ff',  -- bright blue
    '#a277ff',  -- bright magenta
    '#61ffca',  -- bright cyan
    '#edecee',  -- bright white
  },

  -- Tab bar colors
  tab_bar = {
    background = '#15141b',

    active_tab = {
      bg_color = '#29263c',
      fg_color = '#edecee',
      intensity = 'Bold',
      underline = 'None',
      italic = false,
      strikethrough = false,
    },

    inactive_tab = {
      bg_color = '#15141b',
      fg_color = '#6b6b6b',
      intensity = 'Normal',
    },

    inactive_tab_hover = {
      bg_color = '#1f1d28',
      fg_color = '#9b9b9b',
      italic = false,
    },

    new_tab = {
      bg_color = '#15141b',
      fg_color = '#6b6b6b',
    },

    new_tab_hover = {
      bg_color = '#1f1d28',
      fg_color = '#9b9b9b',
    },
  },
}

-- ===== Shell =====
config.default_prog = { '/bin/zsh', '-l' }

-- ===== macOS Specific =====
config.send_composed_key_when_left_alt_is_pressed = true
config.send_composed_key_when_right_alt_is_pressed = true
config.native_macos_fullscreen_mode = true
config.quit_when_all_windows_are_closed = false

-- ===== Key Bindings =====
config.keys = {
  -- Cmd+R: clear screen + scrollback
  {
    key = 'r',
    mods = 'CMD',
    action = wezterm.action.Multiple({
      wezterm.action.SendKey({ key = 'l', mods = 'CTRL' }),
      wezterm.action.ClearScrollback('ScrollbackAndViewport'),
    }),
  },

  -- Cmd+Q: quit
  {
    key = 'q',
    mods = 'CMD',
    action = wezterm.action.QuitApplication,
  },

  -- Cmd+N: new window
  {
    key = 'n',
    mods = 'CMD',
    action = wezterm.action.SpawnWindow,
  },

  -- Cmd+W: close current tab/pane
  {
    key = 'w',
    mods = 'CMD',
    action = wezterm.action.CloseCurrentTab({ confirm = false }),
  },

  -- Cmd+Shift+W: close current pane
  {
    key = 'W',
    mods = 'CMD|SHIFT',
    action = wezterm.action.CloseCurrentPane({ confirm = false }),
  },

  -- Cmd+T: new tab
  {
    key = 't',
    mods = 'CMD',
    action = wezterm.action.SpawnTab('CurrentPaneDomain'),
  },

  -- Cmd+Ctrl+F: toggle fullscreen
  {
    key = 'f',
    mods = 'CMD|CTRL',
    action = wezterm.action.ToggleFullScreen,
  },

  -- Cmd+M: minimize window
  {
    key = 'm',
    mods = 'CMD',
    action = wezterm.action.Hide,
  },

  -- Cmd+H: hide application
  {
    key = 'h',
    mods = 'CMD',
    action = wezterm.action.HideApplication,
  },

  -- Cmd+Shift+R: reload configuration
  {
    key = 'R',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ReloadConfiguration,
  },
  {
    key = '.',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ReloadConfiguration,
  },

  -- Cmd+Equal/Minus/0: adjust font size
  {
    key = '=',
    mods = 'CMD',
    action = wezterm.action.IncreaseFontSize,
  },
  {
    key = '-',
    mods = 'CMD',
    action = wezterm.action.DecreaseFontSize,
  },
  {
    key = '0',
    mods = 'CMD',
    action = wezterm.action.ResetFontSize,
  },

  -- Alt+Left / Alt+Right: word jump
  {
    key = 'LeftArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'b', mods = 'ALT' }),
  },
  {
    key = 'RightArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'f', mods = 'ALT' }),
  },

  -- Cmd+Left / Cmd+Right: line start/end
  {
    key = 'LeftArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'a', mods = 'CTRL' }),
  },
  {
    key = 'RightArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'e', mods = 'CTRL' }),
  },

  -- Cmd+Backspace: delete to line start
  {
    key = 'Backspace',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'u', mods = 'CTRL' }),
  },

  -- Alt+Backspace: delete word
  {
    key = 'Backspace',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'w', mods = 'CTRL' }),
  },

  -- Cmd+D: vertical split
  {
    key = 'd',
    mods = 'CMD',
    action = wezterm.action.SplitHorizontal({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+D: horizontal split
  {
    key = 'D',
    mods = 'CMD|SHIFT',
    action = wezterm.action.SplitVertical({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+[ / ]: prev/next tab
  {
    key = '[',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(-1),
  },
  {
    key = ']',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(1),
  },

  -- Cmd+Option+Arrow: navigate between splits
  {
    key = 'LeftArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Left'),
  },
  {
    key = 'RightArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Right'),
  },
  {
    key = 'UpArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Up'),
  },
  {
    key = 'DownArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Down'),
  },

  -- Cmd+1~9: switch tab
  {
    key = '1',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(0),
  },
  {
    key = '2',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(1),
  },
  {
    key = '3',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(2),
  },
  {
    key = '4',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(3),
  },
  {
    key = '5',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(4),
  },
  {
    key = '6',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(5),
  },
  {
    key = '7',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(6),
  },
  {
    key = '8',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(7),
  },
  {
    key = '9',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(8),
  },

  -- Cmd+Enter / Shift+Enter: newline without execute
  {
    key = 'Enter',
    mods = 'CMD',
    action = wezterm.action.SendString('\n'),
  },
  {
    key = 'Enter',
    mods = 'SHIFT',
    action = wezterm.action.SendString('\n'),
  },

  -- Resurrect plugin keybindings
  {
    key = 's',
    mods = 'CMD|SHIFT',
    action = wezterm.action_callback(function()
      resurrect.save_state(resurrect.workspace_state.get_workspace_state())
    end),
  },
  {
    key = 'l',
    mods = 'CMD|SHIFT',
    action = wezterm.action_callback(function(win, pane)
      resurrect.fuzzy_load(win, pane, function(id)
        local state = resurrect.load_state(id, "workspace")
        resurrect.workspace_state.restore_workspace(state, {
          relative = true,
          restore_text = true,
          on_pane_restore = resurrect.tab_state.default_on_pane_restore,
        })
      end)
    end),
  },
}

-- Copy on select (equivalent to Kitty's copy_on_select)
config.mouse_bindings = {
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'NONE',
    action = wezterm.action.CompleteSelectionOrOpenLinkAtMouseCursor('ClipboardAndPrimarySelection'),
  },
}

-- ===== Performance =====
config.enable_scroll_bar = false
config.front_end = 'OpenGL'
config.webgpu_power_preference = 'HighPerformance'
config.animation_fps = 60
config.max_fps = 60

return config