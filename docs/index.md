# Install

Install Kaku with the DMG or Homebrew, then check the shell setup.

> Most people should use the DMG. Homebrew suits command-line installs and scripted updates. To build Kaku yourself, see [Contributing](https://kaku.fun/docs/contributing).

## Download DMG

Download the latest DMG from GitHub Releases, open it, drag Kaku into Applications, then launch it from the app list.

[Open latest Release](https://github.com/tw93/Kaku/releases/latest)

## Homebrew

If you already manage developer tools with Homebrew, install the official cask:

```bash
brew install --cask kaku
open -a Kaku
kaku doctor
```

If you installed from the personal tap `tw93/tap/kakuku` earlier, you can keep upgrading it there or switch to the official cask:

```bash
brew uninstall --cask tw93/tap/kakuku
brew install --cask kaku
```

## After install

Open Kaku once, then run `kaku doctor`. It checks the app bundle, PATH, and zsh/fish shell integration.

```bash
/Applications/Kaku.app/Contents/MacOS/kaku doctor
```

If your shell can't find `kaku`, restore the integration with the bundled binary and restart your login shell:

```bash
/Applications/Kaku.app/Contents/MacOS/kaku init --update-only
exec zsh -l
```

## Troubleshooting

- Make sure the app is in `/Applications/Kaku.app` and not running from the mounted DMG.
- If the Homebrew install fails, run `brew update` and try `brew install --cask kaku` again. If `kaku update` reports a checksum error on a Homebrew install, use `brew upgrade --cask kaku` instead.
- To set up the shell for the first time, run `kaku init`. It sets up zsh/fish integration and, in an interactive shell, asks before installing missing optional tools such as Starship, Delta, Lazygit, and Yazi through Homebrew.
- If AI features don't work, open `kaku ai` and check Auth Type, Base URL, Simple Model, Deep Model, and API key.
- When filing an issue, include how you installed Kaku, your macOS and Kaku versions, and steps to reproduce.

[Open GitHub Issues](https://github.com/tw93/Kaku/issues)

---

Source: https://kaku.fun/docs/
Site index for LLMs: https://kaku.fun/llms.txt
