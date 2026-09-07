# Guide

A plain-language tour of Kaku: what you see, where to click, and what each shortcut does.

> New to Kaku? This page walks through the app the way you actually use it, from the first window to the AI assistant. For full reference, see [Features](https://kaku.fun/docs/features), [Keybindings](https://kaku.fun/docs/keybindings), and [Configuration](https://kaku.fun/docs/configuration).

![Kaku terminal showing the tab bar, panes, and shell prompt](https://kaku.fun/shots/kaku-dark.webp)One Kaku window: tabs along the bottom, panes side by side, and a ready-to-go shell.

## First launch

Open Kaku and you get a single terminal window with a shell prompt, ready to type. There is no setup wizard and no account to create.

- **Theme follows macOS.** Kaku starts in dark or light to match your system appearance, and switches automatically when the system does.
- **Window controls sit in the tab bar.** The macOS close, minimize, and zoom buttons are tucked into the top-left of the tab strip, so the window stays clean.
- **The tab bar hides itself.** With one tab open there is nothing extra on screen; it appears the moment you open a second tab.

If your shell cannot find the `kaku` command afterwards, run `kaku doctor` to check the setup. See [Install](https://kaku.fun/docs/) for first-run details.

## Tabs & panes

Tabs and splits use the macOS shortcuts you already expect, so you rarely need the menu.

| You want to | Press | What happens |
| --- | --- | --- |
| Open a new tab | `Cmd + T` | Adds a tab in the same folder and reveals the tab bar. |
| Find a tab or pane | `Cmd + Shift + O` | Opens Tab Navigator; type to filter and jump straight to any pane. |
| Split left / right | `Cmd + D` | Splits the current pane into two side by side. |
| Split top / bottom | `Cmd + Shift + D` | Stacks a second pane below the current one. |
| Move between panes | `Cmd + Opt + arrows` | Moves focus to the pane in that direction. |
| Close pane / tab | `Cmd + W` | Closes the active pane, or the tab if it is the last pane, otherwise hides the app. |
| Reopen a closed tab | `Cmd + Shift + T` | Restores the last tab you closed, in its old folder. |
| Rename a tab | Double-click its title | Type a name; press Enter to keep it. |

Inside the Tab Navigator, `Backspace` on the highlighted tab closes it, with the same confirmation you get anywhere else. It only closes while the filter box is empty, so you can still delete what you typed.

When you ssh somewhere, that tab shows the host name instead of a local path, so a remote tab is easy to spot. See [Remote Sessions](https://kaku.fun/docs/features#features-remote-sessions) for what else changes on a remote host.

When you reopen Kaku, it restores your windows, their panes, and each pane's working directory. If one pane cannot be saved, the rest of the session still returns.

## The shell, ready to go

Kaku sets up a curated zsh (and fish) on first launch, so the shell feels modern without you editing config files.

- **Autosuggestions.** As you type, a grey suggestion from your history appears ahead of the cursor.
- **Smart Tab.** Press `Tab` to accept that grey suggestion when one is showing, or to open the completion list when it is not. This is the default; you can switch it in [Configuration](https://kaku.fun/docs/configuration).
- **Jump to folders.** Type `z proj` to jump straight to a folder you visit often, no full path needed.
- **Readable output.** Commands and errors are colored as you type, and `git diff` is paged through Delta when it is installed.

Two touches save time without a keystroke: selecting text copies it to the clipboard as soon as you release the mouse, and `Cmd + Click` opens a file path or URL in its default app.

## The AI assistant

The assistant is optional. Until you enable it, Kaku sends no AI requests. Once enabled, failed commands can trigger an automatic fix suggestion; command generation and chat still start only when you ask. Turn it on once with `kaku ai`: enable Kaku Assistant, choose Auth Type (`codex` to reuse your Codex login, or `api_key` for OpenAI-compatible endpoints), then set Simple Model and Deep Model. After that there are three ways to use it.

1. **Fix a failed command.** When a command exits with an error, Kaku drafts a fix and shows it just under the prompt. Press `Cmd + Shift + E` to paste the suggestion in. Risky commands like `rm -rf` are pasted for review but never run on their own.
2. **Plain language to a command.** Type `#` and a sentence, such as `# find and kill the process on port 3000`, then press Enter. Kaku turns it into a real command and drops it at the prompt, ready for you to check and run.
3. **Open the chat panel.** Press `Cmd + L` for a chat that streams formatted answers, highlights code, and can read the current terminal context. From any shell, `k "..."` or `kaku chat` opens the same conversation.

Requests only go to the AI service you configured, and full setup details are in [Features](https://kaku.fun/docs/features).

## Built-in tools

A few terminal tools are one shortcut away. Each opens in the current pane and returns you to the shell when you quit.

| Tool | Press | What it does |
| --- | --- | --- |
| Lazygit | `Cmd + Shift + G` | A visual git interface for the current repo. |
| Yazi | `Cmd + Shift + Y` | A file manager that drops you into the folder you pick. |
| Remote files | `Cmd + Shift + R` | Mounts the current SSH host's files locally and browses them. |

If a tool is missing, run `kaku init` and Kaku offers to install it. Theme and behavior details are in [Features](https://kaku.fun/docs/features).

## Settings & health

Kaku ships with sensible defaults, so you can change as little or as much as you like.

- **Quick settings.** Press `Cmd + ,` to open the settings panel for common options like the scrollbar, Smart Tab mode, and AI models.
- **Full config.** Run `kaku config` to open the configuration TUI, where you can set font size, window opacity, custom shortcuts, and Lua overrides. See [Configuration](https://kaku.fun/docs/configuration).
- **Check the setup.** Run `kaku doctor` any time something feels off; it verifies the app bundle, PATH, shell integration, and optional tools.

That is the whole loop: install, optionally set up AI, then tune the terminal to taste. Browse the rest of the docs whenever you need a specific detail.

---

Source: https://kaku.fun/docs/guide
Site index for LLMs: https://kaku.fun/llms.txt
