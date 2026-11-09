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
| Kaku-specific utilities | `kaku-ai-utils`, `kaku-unicode-names` (Unicode name table for the character selector, a zstd-compressed `src/names.tsv.zst` decoded on first use; regenerate with `regenerate.sh`, do not hand-edit) |

## Practical Rules

- For macOS notification actions that launch Kaku commands, prefer resolving bundled executables relative to the running `kaku-gui` binary.
- Keep `kaku-ai-utils` provider-agnostic; provider policy belongs in GUI/CLI call sites and config, not in low-level helpers.
- When shared cache helpers affect startup, verify the GUI consumer and avoid invalidating hot-path caches unnecessarily.
- `wezterm-escape-parser` ProgramStatus accepts whitespace around key/value pairs and skips lexically malformed optional pairs without losing valid state. Preserve decoded-text control-character and size checks; permissive pair parsing must not weaken those boundaries. Regressions live in `src/osc.rs` (`program_status_trims_pair_whitespace`, `program_status_skips_malformed_optional_pairs`, and `program_status`).

The web terminal, relay and desktop view package are in `.claude/rules/remote.md` at the repository root, loaded on reading those crates.

## Cross-References

- [`term/AGENTS.md`](../term/AGENTS.md) - Uses codec, vtparse, and color types.
- [`kaku-gui/AGENTS.md`](../kaku-gui/AGENTS.md) - Uses font, input, and surface crates.
- [`lua-api-crates/AGENTS.md`](../lua-api-crates/AGENTS.md) - Uses Lua helper and dynamic binding crates.
