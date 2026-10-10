# FAQ

## Is there a Windows or Linux version?

Not currently, Kaku is macOS-only.

## How is Kaku different from iTerm2, Warp, Ghostty, or WezTerm?

Kaku is a WezTerm-based Mac terminal with fonts, themes, tabs, panes, and shell tools already set, plus an optional assistant that uses the AI service you configure. See the comparison on the website: https://kaku.fun/compare

## Can I use a transparent window?

Yes. Add these lines below `-- User overrides:` in `~/.config/kaku/kaku.lua`, which already loads the bundled defaults into `config` and returns it:

```lua
config.window_background_opacity = 0.92
config.macos_window_background_blur = 20  -- optional blur, 0–100
```

## How do I turn off copy on select?

```lua
config.copy_on_select = false
```

## How do I customize keybindings?

Append to `config.keys`, do not replace it, as shown in [keybindings.md](keybindings.md#custom-keybindings). [configuration.md](configuration.md#custom-keybindings) has more examples.

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

Run `kaku ai`, keep Auth Type set to `api_key`, and enter your Base URL, API Key, Simple Model, and Deep Model manually. Choose **API Mode** `chat_completions` for `/v1/chat/completions`, or `responses` for `/v1/responses`. If the Responses provider supports hosted search, set **Native Web Search** to On; no separate search provider or search API key is required.

## How do I restore default config?

Run `kaku reset`. It removes Kaku-managed integration and state, including Kaku AI memory and the `backups/` folder, while user-authored Lua outside managed blocks is preserved; see [cli.md](cli.md#kaku-reset) for the full list. Run `kaku init` again if you want shell integration back.

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

Since V0.12.1 the viewport no longer jumps to the top during streaming output. If you scroll up mid-stream, pressing the down arrow or scrolling back down returns you to the current output.

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

## QR codes and terminal graphics look vertically stretched.

Kaku's default `line_height = 1.28` favors comfortable text spacing. Terminal graphics built from characters, such as QR codes, `neofetch` logos, and TUI bar charts, scale with the row height, so they render about 28% taller than in terminals with no extra line spacing. This is a typography trade-off, not a rendering bug: block characters must fill the whole cell so TUI borders and progress bars stay seamless.

If you want near-square graphics, lower the line height in `~/.config/kaku/kaku.lua`:

```lua
config.line_height = 1.1  -- or 1.0 to match terminals without extra spacing
```

Note that no terminal renders half-block QR codes perfectly square: with common monospace fonts the cell is naturally a bit taller than 2:1 even at `line_height = 1.0`.

## Can I use Kaku with tiling window managers (yabai, AeroSpace)?

Kaku is compatible with yabai and AeroSpace. If you see continuous flickering, it is usually caused by the tiling WM fighting with Kaku's fullscreen/resize logic. Disabling Kaku's native fullscreen (`config.native_macos_fullscreen_mode = false`) or excluding Kaku from the tiling WM's managed window list typically resolves it.
