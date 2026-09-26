# FAQ

Answers to what people ask most, from install and setup to fonts, scrolling, and AI.

## How do I install Kaku?

Download the DMG from [GitHub Releases](https://github.com/tw93/Kaku/releases/latest), or install with `brew install --cask kaku`.

## What should I run after installing?

Open Kaku once, then run `kaku doctor`. If the shell cannot find `kaku`, run `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only` and restart the shell with `exec zsh -l`.

## Is there a Windows or Linux version?

Not yet. Kaku is macOS-only while the Mac version gets polished, and Windows and Linux may come later.

## How is Kaku different from iTerm2, Warp, Ghostty, or WezTerm?

Kaku is a WezTerm-based Mac terminal with fonts, themes, tabs, panes, and shell tools set up out of the box, plus an optional assistant that uses the AI service you configure. iTerm2 and WezTerm expect you to assemble more of that yourself. Warp is a commercial, account-centered AI product. Ghostty is a fast GPU terminal with fewer presets. See the [comparison page](https://kaku.fun/compare) for when to stay put.

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

See [Keybindings](https://kaku.fun/docs/keybindings) and [Configuration](https://kaku.fun/docs/configuration) for more examples.

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

Run `kaku ai`, keep Auth Type set to API key, and enter the OpenAI-compatible Base URL, API key, Simple Model, and Deep Model manually. Enter the API root, such as `https://api.openai.com/v1`, and choose the API Mode your provider speaks, `chat_completions` or `responses`. If a Responses provider supports hosted search, turn on Native Web Search, and you will not need a separate search provider or search API key.

## How do I restore default config?

```bash
kaku reset
```

This removes the shell and tmux integration and git delta defaults that Kaku manages, selected Kaku state, and the managed theme blocks in `~/.config/kaku/kaku.lua`. Lua you wrote outside those blocks is kept. Run `kaku init` again to get shell integration back.

## The `kaku` command is missing. How do I recover it?

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

Then run `kaku doctor` to check the setup.

## How do I use Kaku's CLI from scripts?

```bash
kaku cli split-pane
kaku cli split-pane -- bash -c "echo hello"
kaku cli --help
```

Every command is listed in the [CLI Reference](https://kaku.fun/docs/cli).

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

Set `config.font` explicitly:

```lua
config.font = wezterm.font('Your Font Name')
```

Kaku's theme-aware font weights only apply to the default JetBrains Mono stack. Once you set your own font, Kaku stops overriding its weight.

## My `window_padding` change isn't working.

`window_padding` takes a number or a string with a unit. Plain numbers are pixels, so `24` and `'24px'` mean the same thing, and the other units are `'pt'`, `'%'` and `'cell'`:

```lua
config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
```

## QR codes and terminal graphics look vertically stretched.

Kaku's default `line_height = 1.28` favors comfortable text spacing. Terminal graphics built from characters, such as QR codes, `neofetch` logos, and TUI bar charts, scale with the row height, so they render about 28% taller than in terminals with no extra line spacing. This is a typography trade-off, not a rendering bug. Block characters must fill the whole cell so TUI borders and progress bars stay seamless.

For near-square graphics, lower the line height in `~/.config/kaku/kaku.lua`:

```lua
config.line_height = 1.1  -- or 1.0 to match terminals without extra spacing
```

No terminal renders half-block QR codes perfectly square, because with common monospace fonts the cell is naturally a bit taller than 2:1 even at `line_height = 1.0`.

## The screen jumps to the top while Claude Code is generating output.

This is a known interaction between trackpad scrolling and Claude Code's streaming output. If you scroll to the top mid-stream, press the down arrow or scroll back down to return to the current output. Recent releases include a fix for the jump itself.

## Cmd+Shift+Y sends a local path when inside an SSH session.

`Cmd+Shift+Y` is for local yazi. Inside an SSH pane, use `Cmd+Shift+R` instead, the yazi remote-files feature, which mounts the remote filesystem over sshfs.

## AI chat won't read files while I'm connected over ssh.

That is deliberate. The tools run on your Mac, but the working directory belongs to the remote host, so reading or running things locally would silently hit a same-named local path. In a remote pane the panel answers from the terminal context and suggests commands for you to run on the host instead. `@cwd` is unavailable there for the same reason. To use the tools on remote files, mount them first with `Cmd + Shift + R`.

## Kaku's prompt shows up in other terminals, or is missing from them.

Every zsh and fish loads Kaku's shell integration, but the Starship prompt and Smart Tab only start inside Kaku, including tmux sessions started from Kaku. Other terminals keep whatever prompt you already had.

To use Kaku's Starship prompt in every terminal, add this before the Kaku line in your `.zshrc` (`set -gx KAKU_PROMPT_EVERYWHERE 1` in `config.fish`):

```zsh
export KAKU_PROMPT_EVERYWHERE=1
```

Smart Tab stays Kaku-only, and `KAKU_SMART_TAB_DISABLE=1` turns it off.

## The `y` shell wrapper doesn't sync my directory on exit.

The `y` wrapper comes from Kaku's fish/zsh shell integration, so it only syncs the directory once that integration is sourced. Check with `kaku doctor`. A bare `yazi` call will not sync the directory.

## How do I install or update Kaku with Homebrew?

Install the official cask:

```bash
brew install --cask kaku
```

Upgrade it with `brew upgrade --cask kaku`.

If you still have the older tap `tw93/tap/kakuku`, keep upgrading that tap, or move to the official cask:

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## Claude Code notifications don't appear.

Kaku may not have notification permission. Open System Settings > Notifications > Kaku, turn on Allow Notifications, then restart Kaku.

## How do I change the global hotkey?

`Cmd + Opt + Ctrl + K` shows or hides Kaku from any app. It follows your current keyboard layout, so on Colemak or Dvorak it is whichever key types K. To pick another combination, change Global Hotkey in `kaku config`, or set it in Lua:

```lua
config.macos_global_hotkey = { key = 'j', mods = 'CMD|OPT|CTRL' }
```

## Can I use Kaku with tiling window managers (yabai, AeroSpace)?

Yes, Kaku works with yabai and AeroSpace. Constant flickering usually means the tiling WM is fighting Kaku's fullscreen and resize logic. Turning off Kaku's native fullscreen (`config.native_macos_fullscreen_mode = false`) or excluding Kaku from the WM's managed windows usually fixes it.

---

Source: https://kaku.fun/docs/faq
Site index for LLMs: https://kaku.fun/llms.txt
