# Privacy

Kaku has no account and collects no usage data. This page lists every network request the app can make.

## Summary

Kaku runs locally on your Mac. There is no sign-up and no usage or crash reporting. Using it as a terminal sends none of your terminal contents, command history, or files to the project, though the commands you run can reach the network on their own. The optional AI features send the context they use to the services listed below.

## Network calls the app makes

- **Update check.** Kaku periodically asks GitHub (`api.github.com/repos/tw93/Kaku/releases/latest`) for the latest version and can download the new release from GitHub in the background. It installs only after you confirm. GitHub sees an ordinary HTTPS request from your IP address. Turn it off with `config.check_for_updates = false` in `~/.config/kaku/kaku.lua`, or change how often it runs with `config.check_for_updates_interval_seconds`.
- **AI assistant.** Once you set up Kaku Assistant with `kaku ai`, prompts and conversation context go straight to the AI provider you chose, using your API key or an existing sign-in. Context can include command output and file contents read by the assistant's tools. Nothing passes through a project server, and the provider's privacy policy applies. Shell-triggered requests, such as the `#` prompt and automatic command fix, must pass a local capability check, so text that merely appears in a pane cannot trigger them.
- **Usage in `kaku ai`.** When you open `kaku ai`, it shows plan usage for AI coding tools you already have installed, such as Claude Code, Codex, Copilot, and Kimi. It reuses their existing sign-ins to ask those providers directly (`api.anthropic.com`, `chatgpt.com`, `api.github.com`, `api.kimi.com`), renews an expired token with the provider when needed, and caches the results in `~/.cache/kaku/`. Model lists come from the public catalog at `models.dev`, a request that carries no account data.
- **AI web tools.** When the assistant searches or reads the web, queries and URLs can go to the Brave, Tavily, or Pipellm service you configured (`api.search.brave.com`, `api.tavily.com`, `api.pipellm.ai` or `api.pipellm.com`). The default page reader sends the URL to `defuddle.md` and falls back to `r.jina.ai`. A custom reader script uses whatever services it calls. The HTTP request tool contacts its target URL directly with the headers and body the assistant asked for. Each of these services has its own privacy policy.
- **Optional tool install.** First-run setup, `kaku init`, and shell integration updates ask before they run, and can then install missing optional tools such as Starship, Delta, Lazygit, and Yazi with Homebrew, which does the download. If Homebrew itself is missing, Kaku asks separately before running the official Homebrew installer from `raw.githubusercontent.com`.
- **No telemetry.** Kaku sends no heartbeat, install ping, usage events, or error reports.

## What is stored on your machine

Kaku keeps your settings and AI data in `~/.config/kaku/`. That includes `kaku.lua` (your Lua configuration), `assistant.toml` (assistant settings, including any API key you entered), shell integration files, the saved session layout, the conversation and memory files behind `Cmd + L` and `kaku chat`, and a cached provider token if you signed in instead of using an API key. Logs, caches, downloaded updates, and images you paste into the terminal go to `~/Library/Application Support/kaku/`, `~/Library/Caches/kaku/`, `~/.cache/kaku/`, and `~/.local/share/kaku/`. These are ordinary files owned by your user account, and Kaku never syncs them to a project server. Only conversation and memory content used as AI context goes to your configured provider.

`kaku reset` removes Kaku-managed shell and tmux integration, the git delta defaults Kaku set, selected Kaku state, and managed theme blocks. Lua you wrote outside those blocks stays. To remove everything, delete the directories above and `/Applications/Kaku.app`.

## This website

kaku.fun is a static site. It sets no cookies, runs no analytics or tracking scripts, embeds no third-party widgets, and has no login. Fonts, scripts, and images all load from this domain, not from a third-party CDN. The site is hosted on Vercel, which keeps standard request logs (IP address, user agent, requested path) to run its service. Links to GitHub, X, and other sites fall under those sites' own policies.

## Questions and changes

Kaku is MIT licensed, so you can check every claim here against the code at [github.com/tw93/Kaku](https://github.com/tw93/Kaku). If something does not match, it is a bug, and an [issue](https://github.com/tw93/Kaku/issues) is welcome. Material changes to this page will be noted in the release notes of the version that ships them.

Last reviewed for Kaku v0.20.0.

---

Source: https://kaku.fun/privacy
Site index for LLMs: https://kaku.fun/llms.txt
