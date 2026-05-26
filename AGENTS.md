# Kaku Agent Guide

Kaku is a macOS-native terminal emulator derived from WezTerm and shaped around AI-assisted terminal workflows. This guide is the shared operating context for agents working in this repository.

## Repository Map

- `kaku/` - CLI entry points, command flows, and user-facing configuration commands.
- `kaku-gui/` - GUI, rendering, window lifecycle, input, mouse handling, AI chat, and the `k` helper binary.
- `mux/` - tabs, panes, domains, and client/server state.
- `term/` - terminal emulation and screen buffer behavior.
- `termwiz/` - terminal UI primitives.
- `config/` - Lua config loading, schema behavior, proxy settings, and versioned defaults.
- `window/` - platform windowing layer.
- `lua-api-crates/` - Rust-to-Lua API bindings.
- `crates/` - shared utility crates, including Kaku-specific AI helpers.
- `assets/` - app resources, bundled config, shell integration, and vendor assets.
- `scripts/` - build, release, and validation helpers.
- `docs/` - user and developer documentation.
- `.github/workflows/ci.yml` - primary GitHub Actions workflow.
- `.github/RELEASE_NOTES.md` - source for the GitHub Release title and body.

## Commands

```bash
make fmt
make fmt-check
make check
make test
make dev
make app
./scripts/build.sh
./scripts/check_config_release_readiness.sh
./scripts/check_release_config.sh
./scripts/check_release_notes.sh
```

`make fmt` requires the nightly Rust toolchain. Use `make app` for GUI, rendering, windowing, and AI overlay verification because it builds the app bundle that users run.

## Working Rules

- Work on the current branch unless the maintainer asks for a branch or worktree.
- Keep changes inside one crate or subsystem when the problem allows it.
- Prefer targeted `rg` searches over repository-wide scans.
- Inspect public APIs and cross-crate boundaries before changing shared behavior.
- Draft issue and PR replies unless the maintainer has already approved the exact public action.
- Do not modify files outside this repository without showing the intended change and getting explicit confirmation.
- Do not add instructions for the removed `website/` tree unless that directory exists in the current worktree.
- Keep private credentials, local keychain paths, and machine-specific release notes out of public repository docs.

## Maintainer Follow-up

- For current issue and PR sweeps, read live GitHub state first with `gh issue list` and `gh pr list`; refresh once more before final conclusions or public actions.
- Before commenting on or closing an item, confirm its title, state, and author with `gh issue view` or `gh pr view`.
- Do not close issues or PRs on local green alone. For fixes pushed to `main`, wait for the new GitHub Actions run on `main` to pass before posting fixed/closed replies.
- Before pushing `main`, run `git fetch origin main` and verify `origin/main` has not moved unexpectedly. If it moved, stop and review `origin/main..HEAD` before pushing.
- If an accepted PR's equivalent fix lands on `main` outside the contributor branch, state the landed commit and co-author status in the PR before closing it.

## Investigation Order

When scope is incomplete, inspect in this order:

1. User-provided repro, failing command, or failing test.
2. Entry point for the behavior, usually `kaku/src/main.rs`, `kaku/src/cli/`, or `kaku-gui/src/main.rs`.
3. Owning subsystem document and target crate.
4. Immediate cross-crate boundary used by the call path.
5. Narrow tests, fixtures, snapshots, or scripts that reproduce the behavior.

For AI-facing behavior, inspect in this order:

1. CLI and assistant configuration under `kaku/src/ai_config/`, `kaku/src/assistant_config.rs`, and `config/src/proxy.rs`.
2. GUI AI state and transport under `kaku-gui/src/ai_*`, `kaku-gui/src/ai_chat_engine/`, and `kaku-gui/src/cli_chat/`.
3. Overlay UI under `kaku-gui/src/overlay/ai_chat/`.
4. Shared helpers in `crates/kaku-ai-utils/`.

## Subsystem Guides

| Subsystem | Guide | Scope |
|---|---|---|
| GUI | `kaku-gui/AGENTS.md` | Rendering, window lifecycle, input, mouse |
| Mux | `mux/AGENTS.md` | Tabs, panes, domains, client/server |
| Terminal | `term/AGENTS.md` | VT emulation, screen buffer |
| Config | `config/AGENTS.md` | Lua loading, schema, config reload |
| Termwiz | `termwiz/AGENTS.md` | TUI primitives and widgets |
| Lua API | `lua-api-crates/AGENTS.md` | Rust-to-Lua bindings |
| Crates | `crates/AGENTS.md` | Shared utility crates |

## Verification

| Change type | Command |
|---|---|
| Rust compile check | `make check` |
| Rust logic change | `make test` |
| Formatting | `make fmt-check` |
| GUI or rendering change | `make app` |
| Config release change | `./scripts/check_config_release_readiness.sh` and `./scripts/check_release_config.sh` |
| Release note change | `./scripts/check_release_notes.sh` |
| Release-adjacent change | `make fmt && make check && make test`, then `make app` |

For GUI or rendering issues, read `kaku-gui/AGENTS.md` first and verify with `make app`, not only `make dev`.

## Current Risk Areas

- AI chat and shell flows are active product surfaces. Preserve `fast_model`, proxy config, inline `#` query status, syntax highlighting, approval flow, and conversation state behavior.
- Config release work currently centers on `config_version` 21. Config schema changes must update bundled defaults, docs, release checks, and migration behavior together. v21 adds `smart_tab_mode`, introduces the optional `SmartPrompt` value for `window_close_confirmation` (default stays `NeverPrompt`), and accepts the removed `language` option as a deprecated field for backward compat.
- GUI regressions can come from overlay resize, pane split/removal, macOS worker thread lifetime, WebGPU surface reconfigure, tab bar spacing, and alternate-screen wheel scroll behavior.
- Startup performance depends on caching shell user vars, Lua bytecode, early appearance queries, GLSL version, and built-in fonts. Do not invalidate those caches without measurement.
- Notification actions that call back into Kaku should resolve bundled executables relative to the running app, not an assumed system path.

## Release Notes

Tag format is `V0.x.x`. `scripts/release.sh` is the source of truth for tagged releases. The GitHub Release title comes from the first heading in `.github/RELEASE_NOTES.md`.

## Documentation Maintenance

- Single-crate behavior belongs in that crate's `AGENTS.md`.
- Cross-crate behavior should update every affected subsystem guide.
- Build, CI, release, and maintainer workflow changes belong in this root file.
- Shared agent instructions belong in tracked docs. Personal overrides belong in ignored local files.
- One-off review reports, scorecards, and diagnostic snapshots are evidence, not durable project docs. Extract stable rules or verification gates into `AGENTS.md`, `CLAUDE.md`, subsystem guides, scripts, or tests, then remove the transient report.
- Do not hide user-visible behavior changes inside maintainability or cleanup patches. New UI, config fields, defaults, or workflow permissions should be split into their own change unless the maintainer explicitly approved that scope.
