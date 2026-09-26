# Guide

How to use Kaku day to day, from the first window to the AI assistant.

> For every option, see [Features](https://kaku.fun/docs/features), [Keybindings](https://kaku.fun/docs/keybindings), and [Configuration](https://kaku.fun/docs/configuration).

![Kaku terminal showing the tab bar, panes, and shell prompt](https://kaku.fun/shots/kaku-dark.webp)Tabs along the bottom, panes side by side.

## First launch

Open Kaku and you get one window with a shell prompt. There's no setup wizard and no account.

- **Theme follows macOS.** Kaku picks dark or light to match your system and switches when the system does.
- **No title bar.** The close, minimize, and zoom buttons sit in the top-left corner of the window.
- **The tab bar stays out of the way.** It's hidden while you have one tab and appears when you open a second.

If your shell can't find the `kaku` command, run `kaku doctor`. First-run details are on the [Install](https://kaku.fun/docs/) page.

## Tabs & panes

Tabs and splits use the shortcuts you'd expect on a Mac.

| You want to | Press | What happens |
| --- | --- | --- |
| Open a new tab | `Cmd + T` | Opens a tab in the same folder and shows the tab bar. |
| Find a tab or pane | `Cmd + Shift + O` | Opens the Tab Navigator. Type to filter, then jump to any pane. |
| Split left / right | `Cmd + D` | Puts a second pane beside the current one. |
| Split top / bottom | `Cmd + Shift + D` | Puts a second pane below the current one. |
| Move between panes | `Cmd + Opt + arrows` | Moves focus to the pane in that direction. |
| Close pane / tab | `Cmd + W` | Closes the active pane. On a tab's last pane it closes the tab, and on the last tab of the last window it hides Kaku. |
| Reopen a closed tab | `Cmd + Shift + T` | Brings back the last tab you closed, in its old folder. |
| Rename a tab | Double-click its title | Type a name, then press Enter. |

In the Tab Navigator, `Backspace` closes the highlighted tab, with the usual confirmation. It only does that while the filter box is empty, so you can still delete what you typed.

An ssh tab shows the host name instead of a local path, so remote tabs are easy to spot. [Remote Sessions](https://kaku.fun/docs/features#features-remote-sessions) covers what else changes on a remote host.

Kaku reopens with your windows, panes, and each pane's working directory. If one pane can't be saved, the rest still come back.

Right-click inside a pane to paste, search, open AI chat or the command palette, split in any direction, or close the pane. TUIs that use the mouse keep their own mouse handling. You can also turn on a new-tab button in Settings.

## The shell, ready to go

Kaku sets up zsh or fish on first launch, so you get a modern shell without editing config files.

- **Autosuggestions.** As you type, a grey suggestion from your history appears after the cursor.
- **Smart Tab.** `Tab` accepts the grey suggestion when there is one and opens the completion list when there isn't. That's the default, and you can change it in [Configuration](https://kaku.fun/docs/configuration).
- **Jump to folders.** `z proj` takes you to a folder you visit often, no full path needed.
- **Readable output.** Commands and errors are colored as you type, and `git diff` pages through Delta when it's installed.

Selecting text copies it when you release the mouse. `Cmd + Click` opens a file path or URL in its default app. A URL that wraps at the edge of the terminal still opens in full, and text after a real line break isn't pulled into the link.

## The AI assistant

The assistant is off until you turn it on, and until then Kaku sends no AI requests. Once it's on, a failed command can trigger a fix suggestion automatically, while command generation and chat only run when you ask.

To turn it on, run `kaku ai`, enable Kaku Assistant, pick an Auth Type (`codex` reuses your Codex login, `api_key` works with OpenAI-compatible endpoints), and set Simple Model and Deep Model. Then you can use it three ways.

1. **Fix a failed command.** When a command exits with an error, Kaku shows a suggested fix under the prompt. Press `Cmd + Shift + E` to paste it in. Risky commands like `rm -rf` are pasted for you to review and never run on their own.
2. **Plain language to a command.** Type `#` and a sentence, like `# find and kill the process on port 3000`, then press Enter. Kaku turns it into a command and leaves it at the prompt for you to check and run.
3. **Open the chat panel.** `Cmd + L` opens a chat that streams formatted answers, highlights code, and can read the current terminal. From any shell, `k "..."` or `kaku chat` opens the same conversation.

You bring your own AI service. Kaku doesn't provide or relay one. Full setup is in [Features](https://kaku.fun/docs/features).

## Built-in tools

Lazygit and Yazi open in the current pane and drop you back in the shell when you quit. Remote files opens in a new tab.

| Tool | Press | What it does |
| --- | --- | --- |
| Lazygit | `Cmd + Shift + G` | A visual git interface for the current repo. |
| Yazi | `Cmd + Shift + Y` | A file manager that leaves you in the folder you pick. |
| Remote files | `Cmd + Shift + R` | Mounts the current SSH host's files locally and opens them in Yazi. |

If Lazygit or Yazi is missing, run `kaku init` and it offers to install them. Remote files also needs sshfs. [Features](https://kaku.fun/docs/features) covers that, plus themes and behavior.

## Settings & health

- **Settings.** Press `Cmd + ,` or run `kaku config` to open the settings TUI for the scrollbar, Smart Tab mode, font size, window opacity, and more. AI models are set in `kaku ai`.
- **Lua config.** Custom shortcuts and other Lua overrides go in `~/.config/kaku/kaku.lua`. Press `e` in the settings TUI to open it in your editor. See [Configuration](https://kaku.fun/docs/configuration).
- **Check the setup.** Run `kaku doctor` whenever something feels off. It checks the Kaku binary, PATH, and shell integration.

---

Source: https://kaku.fun/docs/guide
Site index for LLMs: https://kaku.fun/llms.txt
