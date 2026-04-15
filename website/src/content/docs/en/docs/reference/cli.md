---
title: CLI Commands
description: Complete reference for the Kaku command-line interface
---

# CLI Reference

Run `kaku` in your terminal to see all available commands.

## kaku ai

Open the AI settings panel inside Kaku. Configure external coding tools (Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, etc.) and Kaku Assistant.

```bash
kaku ai
```

## kaku config

Open the Kaku configuration file (`~/.config/kaku/kaku.lua`) in your default editor. Also accessible from the settings panel with `Cmd + ,`.

```bash
kaku config
```

## kaku doctor

Run diagnostics and verify that Kaku's shell integration, PATH entries, and optional tool installations are healthy. Use this first if something feels broken.

```bash
kaku doctor
```

## kaku update

Check for and install the latest Kaku release.

```bash
kaku update
```

## kaku reset

Reset Kaku's config and state files to defaults. Use with caution — this overwrites `~/.config/kaku/kaku.lua`.

```bash
kaku reset
```

## kaku init

Set up Kaku's shell integration for zsh and/or fish. Creates `~/.config/kaku/zsh/kaku.zsh` and optionally `~/.config/kaku/fish/kaku.fish`. Also installs optional CLI tools (Starship, Delta, Lazygit, Yazi) via Homebrew.

```bash
kaku init
```

If the `kaku` command goes missing from your shell, restore it with:

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## kaku cli

Interact with the Kaku multiplexer from scripts and external tools.

```bash
kaku cli split-pane                          # split current pane
kaku cli split-pane -- bash -c "echo hello"  # split and run a command
kaku cli --help                              # list all subcommands
kaku cli split-pane --help                   # help for a specific subcommand
```

Useful for integrating Kaku with AI tools or shell scripts that need to open panes or tabs programmatically.
