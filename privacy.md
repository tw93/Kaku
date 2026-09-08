# Privacy

Kaku has no account and collects no usage analytics. This page lists every network call the app can make.

## Summary

Kaku is a local macOS application with no Kaku account, sign-up, or usage or crash analytics. Ordinary terminal use does not upload your terminal contents, command history, or files to the project. Commands you run can make their own network requests. Optional AI features send the context they use to the services described below.

## Network calls the app makes

- **Update check.** Kaku periodically asks `api.github.com/repos/tw93/Kaku/releases/latest` whether a newer version exists, and can download the release archive from GitHub in the background. Installing it requires your confirmation. GitHub sees the request as a normal HTTPS request from your IP address. Turn it off with `config.check_for_updates = false` in `~/.config/kaku/kaku.lua`, or change the frequency with `config.check_for_updates_interval_seconds`.
- **AI assistant.** When you enable Kaku Assistant with `kaku ai`, prompts and conversation context go to the AI provider you configure, using your API key or CLI login. Context can include command output and file contents read by the assistant's tools. The project does not relay these requests through its own server; the provider's privacy policy applies. The assistant requires configuration before use. Shell-triggered requests, such as the `#` prompt and automatic command fix, use a local capability check so text merely printed in a pane cannot trigger them.
- **AI web tools.** When the assistant searches or reads the web, search queries and URLs can go to your configured Brave, Tavily, or Pipellm service (`api.search.brave.com`, `api.tavily.com`, `api.pipellm.ai` or `api.pipellm.com`). The default page reader sends the requested URL to `defuddle.md`, falling back to `r.jina.ai`. A custom reader script uses the services that script chooses. The HTTP request tool contacts its target URL directly and sends the requested headers and body. These services receive the requests and their own privacy policies apply.
- **Optional tool install.** `kaku init` asks before installing missing optional tools such as Starship, Delta, Lazygit, and Yazi. If you agree, Homebrew performs the download; Kaku itself fetches nothing.
- **No telemetry.** Kaku sends no heartbeat, install ping, feature usage events, or error reports.

## What is stored on your machine

Kaku keeps its state under `~/.config/kaku/`. That includes `kaku.lua` (your Lua configuration), `assistant.toml` (assistant settings, including the API key you entered), shell integration files, the AI chat conversation and memory files used by `Cmd + L` and `kaku chat`, and a cached provider token when you sign in with a CLI login rather than an API key. These are plain local files owned by your user account. Kaku does not sync this directory to a project server; conversation and memory content used as AI context is sent to your configured provider.

Run `kaku reset` to remove Kaku-managed shell and tmux integration, Kaku-managed git delta defaults, selected Kaku state, and managed theme blocks. Lua you wrote yourself outside the managed blocks is preserved. To remove everything, delete `~/.config/kaku/` and `/Applications/Kaku.app`.

## This website

kaku.fun is a static site. It sets no cookies, runs no analytics or tracking scripts, embeds no third-party widgets, and has no login. It loads no fonts, scripts, or images from third-party CDNs; every asset comes from this domain. The site is hosted on Vercel, which keeps standard server request logs (IP address, user agent, requested path) for operational purposes as its infrastructure provider. Links out to GitHub, X, and other sites are governed by those sites' own policies.

## Questions and changes

Kaku is MIT licensed and fully open source, so every claim on this page can be checked against the code at [github.com/tw93/Kaku](https://github.com/tw93/Kaku). If you find a discrepancy, that is a bug: please [open an issue](https://github.com/tw93/Kaku/issues). Material changes to this page will be described in the release notes for the version they ship with.

Last reviewed for Kaku v0.19.0.

---

Source: https://kaku.fun/privacy
Site index for LLMs: https://kaku.fun/llms.txt
