# The Best Terminal for Mac in 2026: Eight Options Compared

> Terminal.app, iTerm2, Ghostty, WezTerm, Warp, Kitty, Alacritty and Kaku compared on setup, configuration, AI tools and price, checked October 2026.

Published 2026-09-21 · Tw93 · https://kaku.fun/blog/best-terminal-for-mac

There is no single best terminal for Mac, but the choice gets simple once you know what you want from it. If you want nothing to install, keep Terminal.app. If you want every feature behind a settings window, use iTerm2. Ghostty is the fast, native pick with a plain text config, WezTerm suits people who want to script their terminal in Lua on every platform, and Warp builds an agent into the terminal. Kaku, which I make, is WezTerm reshaped for macOS and AI coding CLIs, set up from the first launch. The rest of this post explains each one and where it falls short, with versions and prices checked on 2026-10-17 and Kaku's version updated on 2026-11-21.

![A mapping from what you want to which terminal to start with: nothing to install leads to Terminal.app, every feature in a GUI to iTerm2, fast and native with a text config to Ghostty, Lua on every platform to WezTerm, an agent built in to Warp, ready for AI coding CLIs to Kaku](https://kaku.fun/img/blog/mac-terminal-picker.webp)

*Start from what you want, not from a feature list.*

## At a glance

| Terminal | Version | License | Price | Configured with | Splits |
|---|---|---|---|---|---|
| Terminal.app | 2.15 | Proprietary, ships with macOS | Free | Settings window | Same session only |
| iTerm2 | 3.7.6 | GPL-2.0 or later | Free | Settings window | Yes |
| Ghostty | 1.3.1 | MIT | Free | Text file | Yes |
| WezTerm | 20240203, then nightly builds | MIT | Free | Lua | Yes |
| Warp | Rolling | AGPL-3.0 client | Free plan, paid plans from $20 a month | Settings window | Yes |
| Kitty | 0.49.2 | GPL-3.0 | Free | Text file | Yes |
| Alacritty | 0.17.0 | Apache-2.0 | Free | TOML file | No |
| Kaku | 0.30.0 | MIT | Free | Lua | Yes |

## Terminal.app

It is already on every Mac and it is fine for running a few commands. macOS 26 gave it a refresh with 24-bit color and Powerline fonts, but it still falls behind once you live in the terminal: a split only shows the same session twice, its settings live in a plist rather than a text file, and AI coding tools do less with it. Claude Code, for example, only sends the bell character, and only after you turn off the audible bell.

## iTerm2

iTerm2 has profiles, triggers, a password manager, shell integration, and `tmux -CC` integration that turns tmux windows into native tabs. Everything is configured in a settings window, which is friendly at first and harder to keep in a dotfiles repository. Its AI chat works through a separate plugin.

## Ghostty

Ghostty has a native macOS interface, GPU rendering with Metal, sensible defaults, and a config file of `key = value` lines. Its Quick Terminal drops down from the top of the screen with a hotkey, and it supports the common desktop notification sequences. The config itself has no scripting layer, so logic beyond settings and keybindings has to live somewhere else.

## WezTerm

A terminal configured with a Lua script, which means your config can compute things, react to events and build its own status bar. It runs on macOS, Linux and Windows with the same config, and it has a multiplexer built in. The catch is the release cadence: the last stable release is from February 2024, and newer fixes only ship in nightly builds, although development continues.

## Warp

Warp rethinks the terminal around blocks, each command and its output as one unit, with an agent built in that can run commands for you. The client has been open source under AGPL-3.0 since April 2026. Signing in is optional, and the AI features come with plans: a free tier, then Build at $20 a month, Max at $200 and Business at $50 per user. It suits people who want the agent in the terminal itself, less so people who bring their own agent CLI and want the terminal to stay out of the way.

## Kitty and Alacritty

Both are fast, keyboard-driven and configured with text files. Kitty adds tabs, splits, its own image protocol and small extensions called kittens. Alacritty stays minimal on purpose, with no tabs or splits, and is usually paired with tmux.

## Kaku

Kaku is a fork of WezTerm that keeps its engine and Lua config, and changes what you get before writing any config. It ships JetBrains Mono, Kaku Dark and Kaku Light themes that can follow the system appearance, and a zsh setup with autosuggestions, syntax highlighting and a Starship prompt that is two lines in your `.zshrc`. The parts aimed at AI coding CLIs mostly work without setup: `Cmd + Click` on paths to open your editor at the right line, a confirmation before closing a pane that is running an agent, and tabs, splits and last screens that come back after a restart. Claude Code notifications need no setting on Kaku 0.25.0 or later with Claude Code 2.1.295 or later, and older versions set the `ghostty` channel, as [Claude Code notifications](/blog/claude-code-notifications) explains. It is macOS only, which is the trade-off for fitting in there.

```bash
brew install --cask kaku
```

## If you mostly run Claude Code or Codex

Any of these terminals can run an AI coding CLI. The differences show up over long sessions: whether `Shift + Enter` adds a line, whether you get notified when the agent stops, and whether the session survives a restart. [The best terminal for Claude Code](/blog/best-terminal-for-claude-code) goes through those one by one.

## FAQ

### What is the best free terminal for Mac?

Every terminal in this list except Warp's paid plans is free. For most people the choice is between iTerm2 for features, Ghostty for speed and simplicity, and Kaku for a setup that is ready for AI coding tools.

### Is iTerm2 still worth using in 2026?

Yes, especially if you rely on `tmux -CC`, triggers or profiles. Its main drawbacks are a settings model that is hard to version and a heavier feel than the GPU-first terminals.

### Is Ghostty better than iTerm2?

Ghostty is faster to set up and feels more native, while iTerm2 has more features. If you need tmux integration or triggers, stay on iTerm2; otherwise Ghostty is easier to live with.

### Do I need to sign in to use Warp?

No. Signing in is optional, though you need an account for the AI features and plans.
