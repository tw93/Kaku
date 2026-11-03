# tmux Cheat Sheet: Default Keys, Copy Mode and a Minimal Config

> The default tmux prefix and keys for sessions, windows, panes and copy mode, plus a minimal ~/.tmux.conf and how tmux behaves inside Kaku.

Published 2026-11-03 · Tw93 · https://kaku.fun/blog/tmux-cheat-sheet

Every tmux command starts with the prefix, `Ctrl + B` by default: press it, release it, then press the command key. The ones to learn first are `c` for a new window, `%` and `"` to split a pane, the arrow keys to move between panes, `d` to detach, and `[` for copy mode. Everything else on this page is a variation of those, checked against the [tmux manual](https://man.openbsd.org/tmux) in October 2026. Further down is a short `~/.tmux.conf` that turns on the mouse and moves the prefix somewhere more comfortable.

## How the prefix works

tmux runs a server that owns your shells, and the terminal is a client that draws them. When you press the prefix, the client switches to a key table and the next key is read as a tmux command instead of being sent to the shell. If you ever need to type a literal `Ctrl + B` into a program, press it twice. Claude Code is one such program: its `Ctrl + B` sends a running command to the background, and its docs tell tmux users to press it twice. The `list-keys` view, `prefix` then `?`, shows every binding on your own machine, which matters because a config file can change any of the defaults below.

## Sessions

A session is a set of windows that survives the terminal closing. These are shell commands, not prefix keys:

```bash
tmux new -s work          # start a session named work
tmux new -A -s work       # attach to work if it exists, create it if not
tmux ls                   # list sessions
tmux attach -t work       # attach to an existing session
tmux kill-session -t work # destroy one session
tmux kill-server          # destroy the server and every session
```

Inside tmux, `prefix` then `d` detaches and leaves everything running, `prefix` then `s` picks another session from a list, and `prefix` then `$` renames the current one.

## Windows

A window is one full-screen tab inside a session.

| Keys after the prefix | What it does |
|---|---|
| `c` | New window |
| `,` | Rename the window |
| `n` and `p` | Next and previous window |
| `0` to `9` | Jump to that window number |
| `l` | Back to the previously selected window |
| `w` | Choose a window from a list |
| `&` | Kill the window |

## Panes

A pane is one split of a window. The names are easy to mix up, because `%` splits the pane into left and right halves, and `"` splits it into top and bottom.

| Keys after the prefix | What it does |
|---|---|
| `%` | Split left and right |
| `"` | Split top and bottom |
| Arrow keys | Move to the pane in that direction |
| `o` | Next pane |
| `q` | Show pane numbers briefly |
| `z` | Zoom the pane to full window, press again to restore |
| `x` | Kill the pane |
| `!` | Break the pane out into its own window |
| `Space` | Cycle through the preset layouts |
| `Ctrl + arrow` | Resize by one cell |
| `Alt + arrow` | Resize by five cells |

`Ctrl + arrow` and `Alt + arrow` are pressed after the prefix. If a macOS shortcut or your terminal intercepts them, they never reach tmux, and `prefix` then `?` is a quick way to confirm the binding exists.

## Copy mode

tmux keeps its own scrollback, so the scroll wheel of your terminal does not scroll it. That scrollback holds 2,000 lines per pane by default, the `history-limit` value that `tmux show -g history-limit` prints on tmux 3.8, so the start of a long build log or agent session is gone; `set -g history-limit 50000` in `~/.tmux.conf` keeps more. `prefix` then `[` enters copy mode, and `Page Up` also enters it and scrolls one page. By default the keys are emacs style. Setting `mode-keys` to `vi` switches them, and tmux already picks vi keys if your `VISUAL` or `EDITOR` variable contains vi.

| Key in vi mode | What it does |
|---|---|
| `h` `j` `k` `l` | Move the cursor |
| `Ctrl + U` and `Ctrl + D` | Half page up and down |
| `g` and `G` | Top and bottom of the history |
| `/` and `?` | Search forward and backward |
| `n` and `N` | Next and previous match |
| `Space` | Start a selection |
| `v` | Toggle a rectangular selection |
| `Enter` | Copy the selection and leave copy mode |
| `q` | Leave copy mode |

`prefix` then `]` pastes the most recent buffer. That buffer lives inside tmux, so whether it also reaches the macOS clipboard depends on your terminal and on tmux's clipboard settings, which this page does not cover.

## A minimal ~/.tmux.conf

tmux does not create this file. Create it and add:

```bash
# ~/.tmux.conf
set -g mouse on
set -g mode-keys vi
set -g base-index 1
set -g pane-base-index 1

# Optional: use Ctrl + A as the prefix
set -g prefix C-a
unbind C-b
bind C-a send-prefix
```

`mouse on` turns on mouse support, which the manual says is off by default. `base-index 1` and `pane-base-index 1` start numbering at 1, which matches the keyboard row, and the default is zero. `send-prefix` lets `prefix` then `Ctrl + A` send a literal `Ctrl + A` to the shell, which you need because `Ctrl + A` also means beginning of line. Reload without restarting:

```bash
tmux source-file ~/.tmux.conf
```

If you run Claude Code inside tmux, the [alternatives post](/blog/tmux-alternative-mac) lists the three extra lines for notifications and `Shift + Enter`, so they are not repeated here.

## tmux inside Kaku

tmux runs in Kaku like in any terminal. Kaku does not need the config above, but when tmux is installed, `kaku init` adds one `source-file` line to your `~/.tmux.conf` that points at a managed file, `~/.config/kaku/tmux/kaku.tmux.conf`. That file turns on `mouse`, passes `KAKU_SESSION` through so panes opened after you attach from Kaku keep the Kaku prompt, and lets `Shift` plus the scroll wheel enter copy mode. Do not edit it by hand, since Kaku can overwrite it. Your own settings go in the rest of `~/.tmux.conf`.

Kaku's own `Cmd + D` and `Cmd + Shift + D` splits are separate from tmux panes. Use them for local work, and tmux for sessions that must outlive the terminal; [tmux alternative on Mac](/blog/tmux-alternative-mac) explains where that line falls, and [Claude Code remote](/blog/claude-code-remote) covers reaching a session from elsewhere.

## What goes wrong

- **A key does nothing.** The prefix was not pressed and released first. Hold `Ctrl`, tap `B`, let go of both, then press the next key.
- **The mouse selects text but scrolling does nothing.** Mouse support is off. Add `set -g mouse on` and reload.
- **Config changes seem ignored.** `source-file` runs the file in the running server, so run `tmux source-file ~/.tmux.conf` after every edit, or `tmux kill-server` and start again.
- **A session vanished.** After a reboot, `tmux ls` shows nothing, because the server is a process and does not survive a restart.

## FAQ

### What is the default tmux prefix key?

`Ctrl + B`. Change it with `set -g prefix` in `~/.tmux.conf`.

### How do I scroll in tmux?

Enter copy mode with `prefix` then `[` or with `Page Up`, then use the arrow keys, `Page Up` or the vi keys above. With `mouse on`, the wheel does it.

### How do I close a pane or kill a session?

Type `exit` in the shell, or `prefix` then `x` to kill the pane. `tmux kill-session -t name` ends a session.

### How do I leave tmux running and come back later?

Press `prefix` then `d`, and run `tmux attach` to return. On a server, start with `tmux new -A -s main` so the same command always works.
