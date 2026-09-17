# Kaku vs iTerm2, Warp, Ghostty, and WezTerm

A macOS terminal alternative, compared by the job you actually have, not by a feature checklist.

## Start with the job

Searching for a macOS terminal alternative usually means one of these jobs: get a usable terminal without an afternoon of config, keep a long iTerm2 setup, try a faster GPU terminal, or use an AI-heavy product. Kaku is built for the first job. It is a WezTerm-based Mac terminal with fonts, themes, tabs, panes, and shell tools already wired, plus an optional assistant that talks to the AI service you configure yourself.

Other terminals stay better when the job is different. This page says when to stay put.

## At a glance

|  | Kaku | iTerm2 | Warp | Ghostty | WezTerm | Terminal.app |
| --- | --- | --- | --- | --- | --- | --- |
| License | MIT | GPL | Commercial | MIT | MIT | Apple |
| Platform | macOS only | macOS | macOS and more | macOS and Linux | Cross-platform | macOS |
| Account | None | None | Account-centered AI | None | None | None |
| Defaults | Opinionated day-one setup | You assemble them | Product defaults | Fast, fewer presets | You assemble them | Stock Apple |
| AI | Optional, your provider | None built in | Built into the product | None built in | None built in | None built in |

## Kaku vs iTerm2

iTerm2 is the long-running macOS replacement for Terminal.app. People stay because they already have profiles, shortcuts, and muscle memory. Kaku is the better fit when you want fonts, dark and light themes, Mac-native tab and pane shortcuts, and a curated shell suite on first launch, without rebuilding that stack.

Stay on iTerm2 if a large existing config is doing real work, or if you need a feature Kaku has not copied. Kaku does not try to be a complete iTerm2 clone.

## Kaku vs Warp

Warp is a commercial, AI-first terminal. Its assistant and workflows are part of the product. Kaku is MIT licensed, has no Kaku account, and does not provide or relay an AI service. You run `kaku ai` and point it at your own provider. Nothing auto-runs from a suggestion; Cmd + Shift + E pastes a fix for you to review.

Choose Warp if you want a hosted AI product in the terminal. Choose Kaku if you want an open-source Mac terminal where AI stays optional and local to your config.

## Kaku vs Ghostty

Ghostty is a fast, MIT-licensed GPU terminal. It is a strong pick when you want a modern emulator and are happy to bring your own shell tools, fonts, and editor-style workflow. Kaku is also GPU-accelerated, but it ships JetBrains Mono, automatic themes, Lazygit, Yazi, remote files, and the optional assistant already hooked up.

Stay on Ghostty if you want a thinner terminal and already like your own setup. Reach for Kaku if the missing piece is the day-one suite, not another config file.

## Kaku vs WezTerm

Kaku is derived from WezTerm. It keeps WezTerm's Lua configuration system and a fast engine, then adds Mac defaults, shell integration, and the optional assistant. If you already have a WezTerm config you like, you can reuse much of it; some upstream options behave differently, so check the [configuration guide](https://kaku.fun/docs/configuration).

Stay on WezTerm if you need Windows or Linux, or if you want upstream as-is. Kaku is macOS only.

## Kaku vs Terminal.app

Terminal.app is already on the Mac and needs no extra install. It is enough for occasional commands. Kaku is for daily terminal work: tabs, split panes, session restore, clickable paths, and the optional assistant.

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
