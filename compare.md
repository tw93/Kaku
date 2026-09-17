# Kaku vs iTerm2, Warp, Ghostty, and WezTerm

Kaku is the macOS terminal that is ready on day one. Here is how that compares, and when another app is still the better fit.

## Start with the job

Searching for a macOS terminal alternative usually means one of these jobs: get a usable terminal without an afternoon of config, keep a long iTerm2 setup, try a faster GPU terminal, or use an AI-heavy product. Kaku is built for the first job. It is a WezTerm-based Mac terminal with fonts, themes, tabs, panes, and shell tools already wired, plus an optional assistant that talks to the AI service you configure yourself.

Other terminals stay better when the job is different. This page says when to stay put.

## What Kaku is good at

These are the reasons to pick Kaku, not a claim that it wins every row against every terminal.

- **Work on first launch.** JetBrains Mono, macOS font rendering, automatic dark and light themes, copy on select, and Mac tab and pane shortcuts are already set.
- **Tabs and panes that come back.** New tab Cmd + T, split Cmd + D, find a pane with Tab Navigator Cmd + Shift + O. Reopen Kaku and the windows, panes, and directories return. Right-click pastes, searches, opens chat, or splits the clicked pane.
- **A shell suite inside the app.** Completions, syntax highlighting, and `z` directory jumping load in Kaku sessions. Cmd + Shift + G is Lazygit, Cmd + Shift + Y is Yazi, Cmd + Shift + R mounts the current SSH host.
- **Optional AI on your provider.** A failed command can draft a fix. `#` plus a sentence becomes a command. Cmd + L or `kaku chat` opens the same conversation. Suggestions paste for review and never auto-run. There is no Kaku account and no Kaku-operated model.
- **A local CLI that checks itself.** `kaku doctor` reports the app, PATH, and shell integration. `kaku config` and `kaku ai` are TUIs. Official install is `brew install --cask kaku`.
- **Quiet about your machine.** MIT licensed, no usage analytics. Every network call the app can make is listed on the [privacy page](https://kaku.fun/privacy).

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

iTerm2 is the long-running macOS replacement for Terminal.app. People stay because they already have profiles, shortcuts, and muscle memory. Kaku is the better fit when you want that stack without assembling it: fonts and themes on day one, Mac-native tab and pane shortcuts, session restore, a right-click menu, clickable paths, and Lazygit or Yazi on a shortcut. `kaku doctor` tells you if PATH or shell integration is wrong, which iTerm2 does not do for you.

Stay on iTerm2 if a large existing config is doing real work, or if you need a feature Kaku has not copied. Kaku does not try to be a complete iTerm2 clone.

## Kaku vs Warp

Warp is a commercial, AI-first terminal. Its assistant and workflows are part of the product. Kaku is MIT licensed, has no Kaku account, and does not provide or relay an AI service. You run `kaku ai` and point it at your own provider. A failed command can draft a fix, `#` turns a sentence into a command, and Cmd + L opens chat. Nothing auto-runs; Cmd + Shift + E pastes a suggestion for you to review. The rest of the terminal still works if you never configure AI.

Choose Warp if you want a hosted AI product in the terminal. Choose Kaku if you want an open-source Mac terminal where AI stays optional and local to your config.

## Kaku vs Ghostty

Ghostty is a fast, MIT-licensed GPU terminal. It is a strong pick when you want a modern emulator and are happy to bring your own shell tools, fonts, and editor-style workflow. Kaku is also GPU-accelerated. The difference is what is already hooked up: JetBrains Mono, automatic themes, session restore, completions and `z`, Lazygit, Yazi, remote files, and the optional assistant.

Stay on Ghostty if you want a thinner terminal and already like your own setup. Reach for Kaku if the missing piece is the day-one suite, not another config file.

## Kaku vs WezTerm

Kaku is derived from WezTerm. It keeps WezTerm's Lua configuration system and a fast engine, then adds the Mac-facing layer WezTerm leaves to you: opinionated defaults, shell integration, Tab Navigator, window snapshots, the `kaku` CLI, and the optional assistant. Official install is the Homebrew cask `kaku`. If you already have a WezTerm config you like, you can reuse much of it; some upstream options behave differently, so check the [configuration guide](https://kaku.fun/docs/configuration).

Stay on WezTerm if you need Windows or Linux, or if you want upstream as-is. Kaku is macOS only.

## Kaku vs Terminal.app

Terminal.app is already on the Mac and needs no extra install. It is enough for occasional commands. Kaku is for daily terminal work: split panes, session restore, clickable paths, a right-click menu, a shell suite, and an optional assistant that uses the provider you configure.

## When not to use Kaku

- **Windows or Linux.** Unsupported, and no date is promised.
- **A hosted or browser terminal.** Kaku is a local Mac app.
- **An API, SDK, or MCP server.** Those do not exist. Automation goes through the local `kaku` CLI.
- **A full iTerm2 or WezTerm replica.** Some upstream and iTerm2 features are absent on purpose.

## Install Kaku

Official Homebrew cask:

```bash
brew install --cask kaku
kaku doctor
```

Or download the DMG from [GitHub Releases](https://github.com/tw93/Kaku/releases/latest). The [install guide](https://kaku.fun/docs/) covers verification and the older personal tap.

---

Source: https://kaku.fun/compare
Site index for LLMs: https://kaku.fun/llms.txt
