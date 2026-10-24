# Config Agent Guide

The `config` crate owns config loading, Lua integration, schema behavior, proxy settings, versioned defaults, and AI configuration.

## Scope

`config` controls:
- loading user and bundled configs
- Lua execution and binding lifecycle
- schema mapping between Lua and Rust
- config subscriptions and reload behavior
- AI model, fast-model, and proxy settings consumed by GUI and CLI flows
- versioned default config and release readiness checks

## Where to Look

- `config/src/config.rs`: load/parse flow
- `config/src/lib.rs`: config API and subscriptions
- `config/src/proxy.rs`: proxy configuration used by AI and network-facing flows
- `config/src/version.rs`: app version and target-triple accessors (`wezterm_version()`), not `config_version`; that number lives only in `assets/shell-integration/config_version.txt`, read by `kaku/src/init.rs` and the bundled `kaku.lua`
- `assets/macos/Kaku.app/Contents/Resources/kaku.lua`: bundled fallback config

## Config TUI (`kaku/src/config_tui/` and `kaku/src/tui_core/`)

The config TUI is the interactive terminal UI for editing Kaku config. It lives in the `kaku` CLI crate, not the `config` crate.

- `kaku/src/config_tui/` - TUI app: `mod.rs` (form state and main loop), `ui.rs` (rendering)
- `kaku/src/tui_core/` - shared TUI primitives, currently `theme.rs` only (`accent`, `bg`, `muted`, `panel`, `primary`, `red`, `success`, `text_fg`)
- `tui_core::theme` is consumed by `config_tui/ui.rs`, `ai_config/tui/ui.rs`, and `tui_splash.rs`. New primitives shared by more than one TUI flow belong in `tui_core`, not copied per feature.

## AI Config TUI (`kaku/src/ai_config/`)

The AI config TUI lives in the `kaku` CLI crate and shares terminal UI primitives with `config_tui`.

- Keep `fast_model`, primary model, and proxy settings aligned between CLI config, GUI AI chat, and documentation.
- Do not duplicate form widgets or debounce behavior outside `kaku/src/tui_core/`.
- Verify AI config changes against both config parsing and the visible TUI flow.

## Practical Rules

- Loading priority: user config first, bundled config second.
- Keep reload-safe behavior for startup hooks and subscriptions.
- `color::is_light_color` re-exports the terminal core's classification so `COLORFGBG`, window chrome and DEC mode 2031 reports use the same palette policy. Keep that shared policy when changing color classification.
- After mutating a live `TermConfig` client palette, call `pane.set_config(config)` so the terminal compares its subscribed palette snapshot and emits mode 2031 notifications. Updating only the provider bypasses the terminal's reload boundary.
- `KAKU_CONFIG_FILE` is an output, not an input. Config loading exports it (`config/src/config.rs`) and `effective_config_file_path()` (`config/src/lib.rs`), the GUI single-instance check (`kaku-gui/src/main.rs`), `assets/shell-integration/setup_{zsh,fish}.sh`, and the bundled `kaku.lua` all read it back. Do not turn it into a config path override; the only supported override is `--config-file` (`CONFIG_FILE_OVERRIDE`).
- Keep bundled fallback config authoritative at `assets/macos/Kaku.app/Contents/Resources/kaku.lua`.
- Kaku Dark, Kaku Light, and the Kaku Theme alias must also resolve for standalone user configs that do not load the bundled Lua defaults. Their `src/scheme_data.rs` mirrors are checked against every bundled palette field by `builtin_kaku_scheme_mirrors_match_complete_bundled_palettes`; synchronize both when changing a Kaku palette.
- User color schemes are discovered, never named in code: `config::user_color_schemes()` reads `<config-dir>/colors/*.toml`, `color_scheme_dirs`, and the user's Lua `color_schemes` entries, and `kaku config` renders that list as-is. A scheme's selected value is its `metadata.name`, falling back to the file name, matching the loader in `config/src/config.rs`. Keep the fixed `Kaku Dark`/`Kaku Light` labels out of the discovered list.
- Preserve compatibility with runtime reload callers that trigger `config::reload()` from GUI-side signals.
- `config::reload()` can return without applying: once a config file is loaded, a watcher reloads 100ms after that file changes, and whichever reload started last wins. A test that rewrites its config file and reloads must set `automatically_reload_config = false` in that file, or a slow runner reads the old values (`ssh_pane_titles_keep_the_formatter_on_hover`). CI runs each test in its own process with nextest, so a flaky global-config test is racing a thread, not another test.
- The current `config_version` is whatever `assets/shell-integration/config_version.txt` says; never trust a number written in a doc. Any version bump must update bundled config, release checks, docs, and migration expectations together, and add a row to `docs/config-versions.md`.
- New config fields are user-facing behavior. Keep them out of pure cleanup/refactor patches unless the maintainer explicitly approved the product change, and update bundled defaults plus documentation in the same change when they do land.
- Keep alternate-screen wheel scroll behavior configurable; terminal and GUI defaults must not diverge.
- Config rewrite guards must validate every assignment the writer can replace, using the same match predicate. A later Lua override can contain user-owned settings even when the first assignment contains only a managed flag; `remote_access_checks_every_assignment_before_rewriting` covers this boundary in `kaku/src/config_tui/mod.rs`. Remote toggles must edit `effective_config_file_path()` through `kaku::utils::write_atomic`, preserving symlinks, permissions and the original bytes on a partial write failure. `kaku/tests/remote_config.rs` exercises the actual CLI with `--config-file`, a stale inherited `KAKU_CONFIG_FILE`, and a real file-size write limit.

## Bundled kaku.lua Pitfalls

- **200-locals hard limit.** LuaJIT caps a chunk at 200 local variables, and the top-level
  chunk of `assets/macos/Kaku.app/Contents/Resources/kaku.lua` is already at capacity.
  Adding one more top-level `local` makes the whole config fail to load at startup. New
  helper functions and values must be nested inside existing functions or tables. After any
  edit, verify the file still loads:

  ```bash
  luajit -e "assert(loadfile('assets/macos/Kaku.app/Contents/Resources/kaku.lua'))"
  ```

  `scripts/check_kaku_lua_loads.sh <path>` wraps the same check and doubles as a PostToolUse hook for edits to this file.

- **PaneInformation is fields-only.** In title-formatting paths (`format-tab-title`,
  `format-window-title`), the `pane` object is a `PaneInformation` userdata that exposes
  fields only, no methods. Method-style access such as `pane:get_foreground_process_name()`
  or capability probes like `if pane.get_xxx then` do not error; they silently evaluate to
  nil, so the guarded branch never runs (#485 shipped dead code this way). Use the
  documented fields, prefer the `WEZTERM_PROG` user var for the running command, and treat
  argv-derived titles as untrusted (argv can leak environment values).

## Cross-References

- [`kaku-gui/AGENTS.md`](../kaku-gui/AGENTS.md) - GUI config consumers and reload signals.
- [`lua-api-crates/AGENTS.md`](../lua-api-crates/AGENTS.md) - Lua APIs that expose config values.
- [`mux/AGENTS.md`](../mux/AGENTS.md) - Alert propagation for config changes.
