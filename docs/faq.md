# FAQ

## Is there a Windows or Linux version?

Not currently. Kaku is macOS-only while the macOS experience is being polished. Windows and Linux may come later.

## How is Kaku different from iTerm2, Warp, Ghostty, or WezTerm?

Kaku is a WezTerm-based Mac terminal with fonts, themes, tabs, panes, and shell tools already set, plus an optional assistant that uses the AI service you configure. See the comparison on the website: https://kaku.fun/compare

## Can I use a transparent window?

Yes. Add to `~/.config/kaku/kaku.lua`:

```lua
local config = require("kaku").config
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- optional blur, 0–100
return config
```

## How do I turn off copy on select?

```lua
config.copy_on_select = false
```

## How do I customize keybindings?

Append to `config.keys`, do not replace it:

```lua
config.keys[#config.keys + 1] = {
  key = "RightArrow",
  mods = "CMD|SHIFT",
  action = wezterm.action.ActivatePaneDirection("Right"),
}
```

See [keybindings.md](keybindings.md) and [configuration.md](configuration.md) for more examples.

## Can I control working directory inheritance?

Yes, individually for windows, tabs, and splits:

```lua
config.window_inherit_working_directory = true
config.tab_inherit_working_directory = true
config.split_pane_inherit_working_directory = true
```

All are enabled by default.

## How do I disable Kaku Assistant?

Run `kaku ai`, open Kaku Assistant settings, and set Enabled to Off. Or edit `~/.config/kaku/assistant.toml` directly:

```toml
enabled = false
```

## How do I use a custom LLM provider?

Run `kaku ai`, keep Auth Type set to API key, and enter your Base URL, API Key,
Simple Model, and Deep Model manually. Choose **API Mode** `chat_completions`
for `/v1/chat/completions`, or `responses` for `/v1/responses`. If the Responses
provider supports hosted search, set **Native Web Search** to On; no separate
search provider or search API key is required.

## How do I restore default config?

```bash
kaku reset
```

This removes Kaku-managed shell and tmux integration, Kaku-managed git delta
defaults, selected Kaku state, and managed theme blocks in
`~/.config/kaku/kaku.lua`. User-authored Lua outside managed blocks is
preserved. Run `kaku init` again if you want shell integration back.

## The `kaku` command is missing. How do I recover it?

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

Then run `kaku doctor` to verify everything is healthy.

## How do I use Kaku's CLI from scripts?

```bash
kaku cli split-pane
kaku cli split-pane -- bash -c "echo hello"
kaku cli --help
```

See [cli.md](cli.md) for full reference.

## How do I enable the scrollbar?

Open `kaku config` and toggle the scrollbar option, or add to `~/.config/kaku/kaku.lua`:

```lua
config.enable_scroll_bar = true
```

## How do I scroll inside nano, vim, or another full-screen terminal app?

Enable alternate-screen wheel forwarding:

```lua
config.alternate_screen_wheel_scrolls_terminal = true
```

## How do I change the font? My font change isn't taking effect.

Font changes require explicitly setting `config.font` in your config:

```lua
config.font = wezterm.font('Your Font Name')
```

Note: Kaku's theme-aware font weight system only applies to the default JetBrains Mono stack. Once you set a custom font, Kaku will no longer override its weight automatically.

## My `window_padding` change isn't working.

`window_padding` takes a number or a string with a unit. Plain numbers are pixels, so `24` and `'24px'` mean the same thing, and the other units are `'pt'`, `'%'`, and `'cell'`:

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

## The screen jumps to the top while Claude Code is generating output.

This is a known interaction between trackpad scroll and Claude Code's streaming output. If you accidentally scroll to the top mid-stream, pressing the down arrow or scrolling back down returns you to the current output. A fix for the jump behavior has been tracked and shipped in recent releases.

## Cmd+Shift+Y sends a local path when inside an SSH session.

The yazi remote-files feature (`Cmd+Shift+R`) is designed for SSH sessions and mounts the remote filesystem via sshfs. `Cmd+Shift+Y` is for local yazi. Use `Cmd+Shift+R` when you are inside an SSH pane.

## Kaku's prompt shows up in other terminals, or is missing from them.

Kaku's shell integration is loaded by every zsh and fish, but the Starship prompt and Smart Tab only start inside Kaku, including tmux sessions started from Kaku. Other terminals keep whatever prompt you already had.

If you use Kaku's Starship as your one prompt everywhere, add this before the Kaku line in your `.zshrc` (`set -gx KAKU_PROMPT_EVERYWHERE 1` in `config.fish`):

```sh
export KAKU_PROMPT_EVERYWHERE=1
```

Smart Tab stays Kaku-only; `KAKU_SMART_TAB_DISABLE=1` turns it off.

## The `y` shell wrapper doesn't sync my directory on exit.

Make sure the Kaku fish/zsh shell integration is sourced. Check with `kaku doctor`. The `y` wrapper requires the shell init to be loaded. A bare `yazi` call will not sync the directory.

## How do I install or update Kaku with Homebrew?

Install the official cask:

```bash
brew install --cask kaku
```

Homebrew-managed installs update with `brew upgrade --cask kaku`, or with `kaku update` (it detects the cask).

If you still have the older tap `tw93/tap/kakuku`, keep upgrading that tap, or move to the official cask:

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## Claude Code notifications don't appear.

Kaku's notification permission may not be granted. Go to System Settings > Notifications > Kaku and enable Allow Notifications. Then restart Kaku.

## How do I change the global hotkey?

`Cmd + Opt + Ctrl + K` shows or hides Kaku from any app. It follows your current keyboard layout, so on Colemak or Dvorak it is whichever key types K. To pick another combination, change Global Hotkey in `kaku config`, or set it in Lua:

```lua
config.macos_global_hotkey = { key = 'j', mods = 'CMD|OPT|CTRL' }
```

By default (`macos_global_hotkey_mode = 'Drawer'`), the hotkey creates a dedicated terminal session on first use, then shows or hides that same session. Ordinary windows and their running tasks are not moved or hidden. The top-down drawer starts at the screen's top edge, covers the menu bar, and spans the display width and one third of its height. It has no native title bar, traffic-light buttons, or window-rounded corners. Opening Kaku from the Dock opens ordinary windows, not the drawer. The drawer is not restored as an ordinary window after restarting Kaku, and its geometry does not overwrite ordinary window geometry. Use `macos_global_hotkey_mode = 'Ordinary'` to toggle the selected ordinary window instead of invoking the dedicated drawer: Kaku selects the key ordinary window, then the main ordinary window, then the first available ordinary window, or creates one if none exists. This mode does not hide the entire application or toggle every window.

To reveal the hotkey window on the display containing the mouse pointer:

```lua
config.macos_global_hotkey_follow_mouse = true
```

This defaults to `false`, preserving AppKit's main-display behavior. The display is chosen each time the hotkey reveals the window; moving the pointer does not move a visible window, and hiding it does not move it to another display. If no display contains the pointer, Kaku falls back to the main display.

### Inherited appearance and drawer overrides

The drawer inherits the ordinary base configuration, including font, theme, opacity, blur, tab bar, scrollbar, and `window_padding`. This is configuration inheritance, not a copy of a particular ordinary window's live session, position, fullscreen state, or font zoom. Ordinary title-bar spacing does not apply to the drawer.

Set only the appearance fields you want to differ:

```lua
config.macos_global_hotkey_appearance = {
  font_size = 16,
  window_background_opacity = 0.85,
  macos_window_background_blur = 28,
  hide_tab_bar_if_only_one_tab = true,
}
```

Supported appearance fields are `font`, `font_rules`, `font_size`, `line_height`, `color_scheme`, `colors`, `window_background_opacity`, `text_background_opacity`, `macos_window_background_blur`, `window_frame`, `enable_tab_bar`, `hide_tab_bar_if_only_one_tab`, `tab_bar_at_bottom`, `use_fancy_tab_bar`, and `enable_scroll_bar`. Omitted fields, or fields set to `nil`, inherit the base configuration. Explicit `false` and `0` are overrides, not inheritance markers. Color overrides preserve unspecified base color fields; font rules and window frames use their usual replacement semantics.

Drawer content padding can be overridden one side at a time. For example, to use compact padding without changing ordinary windows:

```lua
config.macos_global_hotkey_padding = {
  left = '16px', right = '16px', top = '8px', bottom = '8px',
}
```

These values are an example, not separate product defaults. Omitted sides inherit the corresponding `window_padding` side, so `{ top = 0 }` changes only the top. Set `macos_global_hotkey_padding = nil` to restore inheritance. The same units as `window_padding` are supported (`px`, `pt`, `%`, `cell`); percentages are evaluated against the current drawer's full dimensions. Zero disables that side's content padding (an enabled scrollbar still reserves one cell when right padding is zero).

Base configuration reloads update inherited fields, and removing an override restores inheritance. Explicit per-window overrides from `window:set_config_overrides()` take precedence over the drawer appearance layer. Drawer font zoom is stored separately from ordinary-window zoom; an explicit drawer `font_size` takes precedence over saved drawer zoom when the window is created. Changing the configured drawer font size resets its live zoom without changing ordinary-window zoom.

### Drawer behavior

These behaviors have drawer-specific defaults rather than inheriting ordinary-window geometry:

```lua
config.macos_global_hotkey_height_percent = 100 / 3
config.macos_global_hotkey_animation_duration_ms = 200
config.macos_global_hotkey_hide_on_focus_loss = true
-- Inherit macos_fullscreen_extend_behind_notch unless explicitly set:
-- config.macos_global_hotkey_extend_behind_notch = false
```

Height is a percentage of the physical display frame in the range `(0, 100]`. The drawer remains full-width and top-aligned. Animation duration is in milliseconds, from 0 (no animation) to 10000; the macOS Reduce Motion setting always disables animation. Turning off focus-loss hiding pins the drawer until you hide it with the hotkey.

On a notched display, Kaku adds the current display's safe-area height above the content; external displays without a notch get no extra menu-bar clearance. This safe area is separate from configured top padding, including when top is zero. `macos_global_hotkey_extend_behind_notch = false` enables notch clearance independently of ordinary fullscreen windows; `true` disables it. When omitted, it inherits `macos_fullscreen_extend_behind_notch`. Moving the drawer to another display or unplugging it recalculates the safe area. None of these settings changes the system menu bar or display configuration.

## QR codes and terminal graphics look vertically stretched.

Kaku's default `line_height = 1.28` favors comfortable text spacing. Terminal graphics built from characters, such as QR codes, `neofetch` logos, and TUI bar charts, scale with the row height, so they render about 28% taller than in terminals with no extra line spacing. This is a typography trade-off, not a rendering bug: block characters must fill the whole cell so TUI borders and progress bars stay seamless.

If you want near-square graphics, lower the line height in `~/.config/kaku/kaku.lua`:

```lua
config.line_height = 1.1  -- or 1.0 to match terminals without extra spacing
```

Note that no terminal renders half-block QR codes perfectly square: with common monospace fonts the cell is naturally a bit taller than 2:1 even at `line_height = 1.0`.

## Can I use Kaku with tiling window managers (yabai, AeroSpace)?

Kaku is compatible with yabai and AeroSpace. If you see continuous flickering, it is usually caused by the tiling WM fighting with Kaku's fullscreen/resize logic. Disabling Kaku's native fullscreen (`config.native_macos_fullscreen_mode = false`) or excluding Kaku from the tiling WM's managed window list typically resolves it.
