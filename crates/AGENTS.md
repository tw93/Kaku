# Crates Agent Guide

`crates/` contains shared utility crates used across the workspace. Inspect the target crate's `Cargo.toml` before editing because Kaku keeps some WezTerm-derived crates and adds its own helpers over time.

## Scope

Key groups:

| Group | Crates |
|-------|--------|
| Terminal core | `codec`, `color-types`, `vtparse`, `wezterm-cell`, `wezterm-escape-parser`, `wezterm-surface` |
| GUI and input | `wezterm-font`, `wezterm-input-types`, `wezterm-gui-subcommands`, `wezterm-open-url`, `wezterm-toast-notification` |
| Mux, client, and remote | `wezterm-client`, `wezterm-mux-server-impl`, `wezterm-ssh`, `wezterm-uds`, `kaku-relay`, `kaku-remote` |
| Platform and process | `pty`, `filedescriptor`, `procinfo`, `env-bootstrap`, `umask`, `async_ossl` |
| Lua and config support | `luahelper`, `wezterm-dynamic` |
| Shared utilities | `promise`, `base91`, `bidi`, `bintree`, `frecency`, `lfucache`, `rangeset`, `ratelim`, `tabout`, `wezterm-blob-leases`, `wezterm-char-props`, `wezterm-version` |
| Kaku-specific utilities | `kaku-ai-utils`, `kaku-unicode-names` (generated Unicode name table lifted out of `kaku-gui/src`; regenerate with `ucd-generate`, do not hand-edit) |

## Practical Rules

- For macOS notification actions that launch Kaku commands, prefer resolving bundled executables relative to the running `kaku-gui` binary.
- Keep `kaku-ai-utils` provider-agnostic; provider policy belongs in GUI/CLI call sites and config, not in low-level helpers.
- When shared cache helpers affect startup, verify the GUI consumer and avoid invalidating hot-path caches unnecessarily.

## Web Terminal and Relay

`kaku-remote` serves a browser terminal (`src/web.rs`, page in `assets/web/`) and `kaku-relay` lets it be reached from outside the LAN (`src/share.rs` on the Mac side). Both ship only with the `remote` cargo feature. Invariants:

- One session per browser, carried either by the local WebSocket (`#t=<token>` link) or by the relay (`#k=<key>` link). `run_session` only sees two string channels; keep transport logic out of it.
- The page mirrors Kaku's own terminal model as ANSI frames rather than replaying the PTY stream, so terminal queries are answered once, by Kaku. Keys go through `Pane::key_down`, pastes through `send_paste`.
- Anything that reads the Lua config (for example `can_close_without_prompting`) must run on the GUI thread via `promise::spawn::spawn_into_main_thread`; calling it from the server's tokio threads panics.
- Relay ids are the first 128 bits of SHA-256 of a secret that never leaves the Mac, so a link holder cannot register as that Mac. Messages are AES-256-GCM sealed with the key from the link fragment, with direction labels so a frame cannot be reflected. The relay forwards bytes it cannot read and fetches the page's files from the Mac, so the page always matches the Kaku build.
- One link per Kaku, persisted at `DATA_DIR/remote-share.json` (0600). `kaku remote --reset` rotates it; the running bridge notices within seconds and re-registers, which retires the old link.

## Cross-References

- [`term/AGENTS.md`](../term/AGENTS.md) - Uses codec, vtparse, and color types.
- [`kaku-gui/AGENTS.md`](../kaku-gui/AGENTS.md) - Uses font, input, and surface crates.
- [`lua-api-crates/AGENTS.md`](../lua-api-crates/AGENTS.md) - Uses Lua helper and dynamic binding crates.
