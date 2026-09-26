# Kaku vs iTerm2, Warp, Ghostty, and WezTerm

Kaku aims to be the Mac terminal that works the moment you install it. Here is how it stacks up against the usual picks, and when you should stay where you are.

## Start with the job

People switch terminals for a handful of reasons. They want something usable without an afternoon of config, they have a long-lived iTerm2 setup, they want a faster GPU terminal, or they want AI built into the product. Kaku is for the first group. Fonts, themes, tabs, panes, and shell tools are already set up, and AI can plug into your own service.

## What Kaku is good at

- **Works on first launch.** JetBrains Mono, macOS font rendering, light and dark themes that follow the system, copy on select, and Mac-style tab and pane shortcuts are all set.
- **Tabs and panes that survive a relaunch.** Cmd + T opens a tab, Cmd + D splits, and Cmd + Shift + O opens Tab Navigator to find a pane. Quit and reopen, and your windows, panes, and directories are still there. Right-click to copy, paste, search, open chat, or split the pane under the cursor.
- **Shell tools built in.** Completions, syntax highlighting, and `z` for jumping between directories work in every Kaku session. Cmd + Shift + G opens Lazygit, Cmd + Shift + Y opens Yazi, and Cmd + Shift + R mounts files from the current SSH host.
- **Optional AI on your own service.** Failed commands come back with a suggested fix, `#` plus a sentence becomes a command, and Cmd + L and `kaku chat` share one conversation. Suggestions land at the prompt for you to review and never run on their own. There is no Kaku account and no Kaku-hosted model.
- **Easy to troubleshoot.** `kaku doctor` checks the app, PATH, and shell integration, and `kaku config` and `kaku ai` are settings screens right in the terminal.
- **Leaves your data alone.** MIT licensed with no usage analytics, and every network request the app can make is listed on the [privacy page](https://kaku.fun/privacy).

## At a glance

|  | Kaku | iTerm2 | Warp | Ghostty | WezTerm | Terminal.app |
| --- | --- | --- | --- | --- | --- | --- |
| License | MIT | GPL | Commercial | MIT | MIT | Apple |
| Platform | macOS only | macOS | macOS and more | macOS and Linux | Cross-platform | macOS |
| Account | None | None | Account-centered AI | None | None | None |
| Defaults | Opinionated day-one setup | You assemble them | Product defaults | Fast, fewer presets | You assemble them | Stock Apple |
| AI | Optional, your provider | None built in | Built into the product | None built in | None built in | None built in |
| Shell tools | In the Kaku session | You add them | Product set | You add them | You add them | None |
| Install | DMG or official brew cask | Download | Product installer | Download | Download | Built in |
| Checkup | `kaku doctor` | None | Product UI | None | None | None |

## Kaku vs iTerm2

iTerm2 is the long-running macOS replacement for Terminal.app. People stay because they already have profiles, shortcuts, and muscle memory. Kaku suits people who want that setup without building it by hand, with fonts and themes from day one, Mac-native tab and pane shortcuts, session restore, a right-click menu, clickable paths, and Lazygit or Yazi on a shortcut. When PATH or shell integration is off, `kaku doctor` tells you.

If your iTerm2 setup is working well, or you rely on something Kaku does not have, stay on iTerm2. Kaku is not trying to be a full iTerm2 clone.

## Kaku vs Warp

Warp is a commercial, AI-first terminal where the assistant and workflows are the product. Kaku is MIT licensed, has no account, and does not provide or relay an AI service. Run `kaku ai`, point it at your own provider, and you get fixes for failed commands, `#` to write a command from a sentence, and Cmd + L for chat. Nothing runs on its own, since Cmd + Shift + E only drops a suggestion at the prompt. Skip AI entirely and the rest of the terminal works the same.

Pick Warp if you want a hosted AI product in your terminal. Pick Kaku if you want an open-source Mac terminal where AI is optional and runs on your own setup.

## Kaku vs Ghostty

Ghostty is a fast, MIT-licensed GPU terminal. It is a great choice if you already have your own shell tools, fonts, and workflow. Kaku also renders on the GPU, and the difference is what comes already hooked up: JetBrains Mono, automatic themes, session restore, completions and `z`, Lazygit, Yazi, remote files, and the optional assistant.

Stay on Ghostty if you want something leaner and like your current setup. Try Kaku if what you are missing is the out-of-the-box suite, not another config file.

## Kaku vs WezTerm

Kaku is derived from WezTerm. It keeps WezTerm's Lua config and rendering engine, then adds the Mac layer WezTerm leaves to you, with defaults, shell integration, Tab Navigator, window snapshots, the `kaku` CLI, and the optional assistant. Most existing WezTerm configs carry over, though a few upstream options behave differently, so check the [configuration guide](https://kaku.fun/docs/configuration).

Stay on WezTerm if you need Windows or Linux, or want upstream as it is. Kaku is Mac only.

## Kaku vs Terminal.app

Terminal.app ships with every Mac and is fine for the occasional command. Kaku is for people who live in the terminal, with split panes, session restore, clickable paths, a right-click menu, a shell suite, and an optional assistant that uses the provider you configure.

## When not to use Kaku

- **Windows or Linux.** Unsupported, and no date is promised.
- **A hosted or browser terminal.** Kaku is a local Mac app.
- **An API, SDK, or MCP server.** There is none. Automation goes through the local `kaku` CLI.
- **A full iTerm2 or WezTerm replica.** Some features are left out on purpose.

## Install Kaku

Official Homebrew cask:

```bash
brew install --cask kaku
kaku doctor
```

Or download the DMG from [GitHub Releases](https://github.com/tw93/Kaku/releases/latest). The [install guide](https://kaku.fun/docs/) covers post-install checks and moving off the old personal tap.

---

Source: https://kaku.fun/compare
Site index for LLMs: https://kaku.fun/llms.txt
