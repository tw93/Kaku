# Install

Add Kaku to your macOS workflow with the DMG or Homebrew, then verify the shell setup.

> Most people should use the DMG. Use Homebrew when you want command-line installs or scripted updates. Developer setup lives in [Contributing](https://kaku.fun/docs/contributing).

## Download DMG

Most users should download the latest DMG from GitHub Releases. Open the image, drag Kaku into Applications, then launch it from the app list.

[Open latest Release](https://github.com/tw93/Kaku/releases/latest)

## Homebrew

If you already manage developer tools with Homebrew, install the official cask.

```bash
brew install --cask kaku
open -a Kaku
kaku doctor
```

Homebrew fits machines that need command-line installation and scripted updates.

If you previously installed the personal tap `tw93/tap/kakuku`, keep upgrading that tap, or move to the official cask:

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## After install

Open Kaku once after installation, then run `kaku doctor`. It checks the app bundle, config directory, PATH, zsh/fish shell integration, and optional tool availability.

```bash
/Applications/Kaku.app/Contents/MacOS/kaku doctor
```

If your shell cannot find `kaku`, restore shell integration with the bundled binary, then restart your login shell:

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## Troubleshooting

- Confirm the app lives at `/Applications/Kaku.app`. Do not run it directly from the DMG.
- If Homebrew install fails, run `brew update` first, then `brew install --cask kaku`. If `kaku update` reports checksum issues on a Homebrew install, run `brew upgrade --cask kaku`.
- For first-time shell tooling, run `kaku init`. It provisions zsh/fish integration and asks before installing missing optional tools such as Starship, Delta, Lazygit, and Yazi through Homebrew.
- If AI features do not work, open `kaku ai` and check Auth Type, Base URL, Simple Model, Deep Model, and API key.
- When filing an issue, include install method, macOS version, Kaku version, and reproduction steps.

[Open GitHub Issues](https://github.com/tw93/Kaku/issues)

---

Source: https://kaku.fun/docs/
Site index for LLMs: https://kaku.fun/llms.txt
