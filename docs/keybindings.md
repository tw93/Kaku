# Keybindings

All keybindings use macOS-native modifier keys. `Opt` = Option/Alt, `Ctrl` = Control.

## Window

| Action | Shortcut |
| :--- | :--- |
| New window | `Cmd + N` |
| Close pane / tab / hide | `Cmd + W` |
| Close current tab | `Cmd + Shift + W` |
| Hide application | `Cmd + H` |
| Minimize window | `Cmd + M` |
| Toggle fullscreen | `Cmd + Ctrl + F` |
| Quit | `Cmd + Q` |
| Toggle global window | `Cmd + Opt + Ctrl + K` |

> `Cmd + W` is smart: closes the active pane if there are multiple panes, closes the tab if there are multiple tabs or windows, otherwise hides the app.

## Tabs

| Action | Shortcut |
| :--- | :--- |
| New tab | `Cmd + T` |
| Switch to tab 1–9 | `Cmd + 1` – `Cmd + 9` |
| Previous tab | `Cmd + Shift + [` |
| Next tab | `Cmd + Shift + ]` |
| Close tab | `Cmd + Shift + W` |
| Reopen closed tab | `Cmd + Shift + T` |
| Rename tab | Double-click tab title |

## Panes

| Action | Shortcut |
| :--- | :--- |
| Split vertical | `Cmd + D` |
| Split horizontal | `Cmd + Shift + D` |
| Toggle split direction | `Cmd + Shift + S` |
| Zoom / unzoom pane | `Cmd + Shift + Enter` |
| Navigate panes | `Cmd + Opt + Arrows` |
| Resize pane | `Cmd + Ctrl + Arrows` |

## Shell Editing

| Action | Shortcut |
| :--- | :--- |
| Jump word left / right | `Opt + Left` / `Opt + Right` |
| Jump to line start / end | `Cmd + Left` / `Cmd + Right` |
| Delete to line start | `Cmd + Backspace` |
| Delete word | `Opt + Backspace` |
| Newline without execute | `Cmd + Enter` or `Shift + Enter` |

## Font Size

| Action | Shortcut |
| :--- | :--- |
| Increase | `Cmd + =` |
| Decrease | `Cmd + -` |
| Reset | `Cmd + 0` |

## Kaku Features

| Action | Shortcut |
| :--- | :--- |
| Clear screen + scrollback | `Cmd + K` |
| Open Settings panel | `Cmd + ,` |
| Open AI panel | `Cmd + Shift + A` |
| Apply Kaku Assistant suggestion | `Cmd + Shift + E` |
| Open lazygit | `Cmd + Shift + G` |
| Open yazi file manager | `Cmd + Shift + Y` |
| Browse remote files (SSH) | `Cmd + Shift + R` |
| Open Doctor panel | `Ctrl + Shift + L` |

## Mouse

| Action | Trigger |
| :--- | :--- |
| Copy selection to clipboard | Release left mouse button after selecting |
| Open link | `Cmd + Click` |
| Move cursor to clicked column | `Opt + Click` (same row, shell prompt only) |

## Custom Keybindings

Add bindings to `~/.config/kaku/kaku.lua` by **appending** to `config.keys`. Do not assign a new table — this would erase Kaku's defaults.

```lua
-- ~/.config/kaku/kaku.lua (after loading bundled config)
table.insert(config.keys, {
  key = 'RightArrow',
  mods = 'CMD|SHIFT',
  action = wezterm.action.ActivatePaneDirection('Right'),
})
```

For the full list of available actions, see [WezTerm KeyAssignment reference](https://wezfurlong.org/wezterm/config/lua/keyassignment/).
