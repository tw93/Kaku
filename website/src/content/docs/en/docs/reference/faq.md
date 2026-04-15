---
title: FAQ
description: Frequently asked questions about Kaku
---

# FAQ

## Is there a Windows or Linux version?

Not currently. Kaku is macOS-only while the macOS experience is being polished. Windows and Linux may come later.

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

Append to `config.keys` — do not replace it:

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

Run `kaku ai`, select "Custom" from the Provider dropdown, and enter your base URL and API key manually. The URL must be OpenAI-compatible (`/v1/chat/completions`).

## How do I restore default config?

```bash
kaku reset
```

This overwrites `~/.config/kaku/kaku.lua` with defaults.

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

## How do I change the font? My font change isn't taking effect.

Font changes require explicitly setting `config.font` in your config:

```lua
config.font = wezterm.font('Your Font Name')
```

Note: Kaku's theme-aware font weight system only applies to the default JetBrains Mono stack. Once you set a custom font, Kaku will no longer override its weight automatically.

## My `window_padding` change isn't working.

`window_padding` values require a `'px'` unit suffix:

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

Plain numbers (without `'px'`) are interpreted as terminal cell units, which may not match your intent.

## The screen jumps to the top while Claude Code is generating output.

This is a known interaction between trackpad scroll and Claude Code's streaming output. If you accidentally scroll to the top mid-stream, pressing the down arrow or scrolling back down returns you to the current output. A fix for the jump behavior has been tracked and shipped in recent releases.

## Cmd+Shift+Y sends a local path when inside an SSH session.

The yazi remote-files feature (`Cmd+Shift+R`) is designed for SSH sessions and mounts the remote filesystem via sshfs. `Cmd+Shift+Y` is for local yazi. Use `Cmd+Shift+R` when you are inside an SSH pane.

## The `y` shell wrapper doesn't sync my directory on exit.

Make sure the Kaku fish/zsh shell integration is sourced. Check with `kaku doctor`. The `y` wrapper requires the shell init to be loaded — a bare `yazi` call will not sync the directory.

## Homebrew can't find the binary / wrong Kaku gets updated.

There is an older unrelated package named `kaku` on Homebrew. Install Kaku with the tap to avoid conflicts:

```bash
brew install tw93/tap/kakuku
```

If you see checksum errors with `kaku update`, use `brew upgrade tw93/tap/kakuku` directly.

## Claude Code notifications don't appear.

Kaku's notification permission may not be granted. Go to System Settings > Notifications > Kaku and enable Allow Notifications. Then restart Kaku.

## The global hotkey doesn't work on non-QWERTY keyboards (e.g. Colemak).

`Cmd + Opt + Ctrl + K` uses the physical QWERTY K position. On Colemak, this corresponds to a different key. Remap it in your config:

```lua
table.insert(config.keys, {
  key = 'k',  -- adjust to your layout's physical key
  mods = 'CMD|OPT|CTRL',
  action = wezterm.action.EmitEvent('toggle-global-window'),
})
```

## Can I use Kaku with tiling window managers (yabai, AeroSpace)?

Kaku is compatible with yabai and AeroSpace. If you see continuous flickering, it is usually caused by the tiling WM fighting with Kaku's fullscreen/resize logic. Disabling Kaku's native fullscreen (`config.native_macos_fullscreen_mode = false`) or excluding Kaku from the tiling WM's managed window list typically resolves it.
