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
- `.agents/skills/` - the canonical and tracked home for project skills: `release` and `bugs`. The website skill `product-docs` lives with the site on the `vercel` worktree (`~/www/kaku-site/.agents/skills/product-docs`). `.claude/` is gitignored except for the force-added `.claude/rules/macos.md`; its skill entries may be relative symlinks or ignored private overrides. Do not cite, copy, or edit an ignored `.claude/skills/` directory as project truth. Put durable skill changes in `.agents/skills/` first, then update a symlink when that entry is actually a link.
- `.github/workflows/checks.yml` - fast correctness gates on pushes to `main` and on pull requests, with `paths-ignore` for `**.md` and assets, so a docs-only change gets no CI at all. Five macOS jobs, the runner's concurrency cap: Format, Unit Tests, Relay Build, Clippy (which also carries the source guards and stands in for `cargo check`, scoped to the lint-opt-in crates, not the workspace), and Scripts and Release Checks (release notes, config version, shell smoke tests, shellcheck). Adding a sixth macOS job makes one queue.
- `.github/workflows/build-validation.yml` - release-shaped builds with the shipped `release-opt` profile: one job per architecture in parallel (the arm64 one also assembles the bundle), then a `lipo` merge of `kaku`, `kaku-gui` and `k`; runs on build-pipeline changes, daily (skipped when that commit already passed), or on dispatch. `release.sh` preflight requires its latest run green and the latest Security Audit run not failed, and skips its local fmt/check/test when CI Checks already passed on HEAD.
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

`make fmt` and `make fmt-check` both shell out to `cargo +nightly fmt`, so they require the nightly toolchain. Use `make app` for GUI, rendering, windowing, and AI overlay verification because it builds the app bundle that users run.

## Working Rules

- Work on the current branch unless the maintainer asks for a branch or worktree.
- Keep changes inside one crate or subsystem when the problem allows it.
- Inspect public APIs and cross-crate boundaries before changing shared behavior.
- Kaku must not automatically inject agent hooks, instructions, or settings. Explicit edits through `kaku ai` may update the selected provider's configuration (including Claude and Codex settings), preserving user-owned fields. Agent-facing help belongs in documentation. The `kaku init` source line in `.zshrc` is managed by `assets/shell-integration/setup_zsh.sh` and checked by `doctor.rs`; `reset.rs` must keep every user-owned line around it.
- Narrowing a default that existing users already depend on must ship the other camp's explicit path in the same change. Shell integration scopes the Starship prompt to `TERM_PROGRAM=Kaku` OR (`$TMUX` and `KAKU_SESSION`), with `KAKU_PROMPT_EVERYWHERE=1` as the opt-in for every terminal; Smart Tab stays Kaku-only (#503). The predicate is duplicated inside each installer, three times in `setup_zsh.sh` and four in `setup_fish.sh`, so grep both files and change every copy. `assets/shell-integration/tests/prompt_scope_smoke.sh` gates the placements in CI via `.github/workflows/checks.yml`, and `docs/faq.md` is the user-facing half; a change that re-narrows the scope must update both installers, the smoke and the doc together.
- Tab indicator decision (2026-08-24): the tab bar draws a dot only for `Progress::Paused | Progress::Error` (attention, `kaku-gui/src/tabbar.rs`); a running pane deliberately shows nothing, the running-state dot was dropped in `4f297556`. Do not bring it back.
- Retro tab bar chrome (2026-09-09, #548): the bundled config (`assets/macos/Kaku.app/Contents/Resources/kaku.lua`) sets `use_fancy_tab_bar = false`, so the shipped bar is the retro one even though the `config/src/config.rs` schema default is true. `kaku-gui/src/tabbar.rs` composes a `Line` of cells, so a control there is a character on the text baseline competing with the title for the one trailing cell the rule above spends on attention; only the fancy bar can host a drawn control (`make_x_button` in `kaku-gui/src/termwindow/render/fancy_tab_bar.rs`). The per-tab close mark added in `cef3a371` was reverted in `732d2082` for that reason, and `show_close_tab_button_in_tabs` stays fancy-bar-only. Answer tab bar chrome requests with the pointers that already exist: the new-tab button is on by default (bundled `show_new_tab_button_in_tab_bar = true`), right-click on a tab opens a tab menu with New Tab, Tab Navigator and Close Tab (`tab_context_menu_actions` in `kaku-gui/src/termwindow/mouseevent.rs`), middle-click closes a tab, and the Shell menu lists New Tab and Close Tab. For touch over remote desktop, a plain left press (a tap) up to `BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT` above a bottom bar still hits tabs and the new-tab button, while middle and right presses there stay with the terminal, so the bar itself stays one row. Do not re-add a grid-cell close mark without solving that cell budget first.
- Line height decision (2026-07): the bundled default stays `line_height = 1.28`. Character-cell graphics (QR codes, `neofetch` logos, TUI bar charts) scale with the row height, so they render about 28% taller than at `line_height = 1.0`, and that trade-off is accepted. Rendering block glyphs at their natural cell height was tried in `d810b13` and reverted in `3f131ea`: block characters must fill the whole cell or box borders and QR codes open horizontal seams. Do not propose lowering the default or re-attempting natural-height block rendering; point users at the `line_height` entries in `docs/faq.md` and `docs/configuration.md`. The block cursor is a separate case and correctly keeps using `natural_cell_height` (see `kaku-gui/AGENTS.md`).
- Draft issue and PR replies unless the maintainer has already approved the exact public action.
- Do not modify files outside this repository without showing the intended change and getting explicit confirmation.
- Do not add instructions for the removed `website/` tree unless that directory exists in the current worktree.
- The marketing and docs site lives on the `vercel` branch (linked worktree at `~/www/kaku-site`), not on `main`. It follows the design guide at `~/www/kaku-site/DESIGN.md`; verify changes with screenshots at 375px / 1280px and deploy by pushing the `vercel` branch (Vercel serves kaku.fun).
- Keep private credentials, local keychain paths, and machine-specific release notes out of public repository docs.
- **Do not propose UI i18n / multi-language menus / a `config.language` setting.** The `rust-i18n` based Chinese UI localization (PR #362, commit `f6cfb4b`) was reverted in `b4d779a` on 2026-05-18; `language` remains in the config schema as a deprecated field for backward compat only. UI strings (menus, confirm dialogs, config TUI copy) stay as English literal strings. New UI surfaces should not introduce translation keys, locale-aware formatting, or "what if a user wants Chinese" abstractions. If a user requests a non-English UI, route to the assistant config / AI chat surface; those already accept non-English content.
- **Do not pre-bake provider abstractions in `kaku/src/ai_config/`.** Kaku Assistant parsing, field presentation, and persistence live in `kaku/src/ai_config/tui/providers/assistant.rs`; the TUI retains its event loop and shared data types. Future extraction should move one concrete provider at a time into that existing directory. Do not add an unused trait, a `ProviderKind` enum, or stub modules. Save Copilot for last because its OAuth flow is the abstraction stress test.
- CLI Codex readiness and GUI connection parsing both consume `tests/fixtures/codex-connection.json`. Add boundary cases there when either policy changes, and run both crates' tests.
- Official Homebrew token is `kaku` (`brew install --cask kaku`). `tw93/tap/kakuku` remains the personal tap for people who installed before the official cask. Do not tell users the Homebrew `kaku` token is a different app.

## Maintainer Follow-up

- For current issue and PR sweeps, read live GitHub state first with `gh issue list` and `gh pr list`; refresh once more before final conclusions or public actions.
- Before commenting on or closing an item, confirm its title, state, and author with `gh issue view` or `gh pr view`.
- The latest stable tag is `git tag --list 'V*' --sort=-version:refname | head -1`; without the `V*` filter the rolling `nightly` tag sorts first. Map each report to `git log <tag>..HEAD`, and treat a closed issue as a claim until the fix commit or remaining gap is named.
- Shell integration fixes run the affected `assets/shell-integration/tests/*_smoke.sh`, then the list `.github/workflows/checks.yml` runs under `Shell integration smoke tests`. Watch the pushed `main` run with `gh run watch <id> --exit-status`.
- Do not close issues or PRs on local green alone. For fixes pushed to `main`, wait for the new GitHub Actions run on `main` to pass before posting fixed/closed replies.
- The rolling `nightly` release is not rebuilt by push. Before sending users to Nightly, run or verify `./scripts/nightly.sh` and confirm `gh release view nightly --json tagName,targetCommitish,publishedAt,assets,url` points at the fix commit and includes `Kaku-nightly.dmg`.
- Default issue-closure pipeline once a fix is verified: commit the fix, refresh the `nightly` release assets per the rule above, reply in the reporter's language with the Nightly download or in-app update path, then close if the current request already authorizes closure; otherwise propose it and wait for authorization. Do not promise a specific packaged-release date in replies.
- Before pushing `main`, run `git fetch origin main` and verify `origin/main` has not moved unexpectedly. If it moved, stop and review `origin/main..HEAD` before pushing.
- For several unrelated bugs, prefer one commit per issue or behavior, and put the issue number in the subject: `fix(scope): #123 short summary`.
- Merge contributor PRs with `gh pr merge --merge`, never squash, so the contributor's own commits and authorship stay in history; add no trailer. When `maintainerCanModify` is true, push fix commits to the contributor's branch rather than rewriting their existing commits, and approve a first-time contributor's workflow run with `gh api -X POST repos/tw93/Kaku/actions/runs/<id>/approve` before expecting CI.
- If an accepted PR's equivalent fix lands outside the contributor branch, acknowledge the contribution and explain which user-visible fix was delivered before closing it; keep commit hashes and internal attribution bookkeeping out of the public reply.

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

For AI transport bugs around custom `base_url`, keep both proxy paths true: external API hosts should use detected system proxy settings, while loopback, private LAN, link-local, CGNAT/Tailscale-style, `.local`, `NO_PROXY`, and macOS ExceptionsList model endpoints should connect directly. Verify with the macOS system proxy enabled and a loopback or internal OpenAI-compatible smoke before saying it is fixed. Do not claim general SOCKS support unless that transport is actually implemented and verified.

For `Ctrl+letter` not working in a raw-mode TUI (the most common shape: `Ctrl+C` / `Ctrl+R` works in plain shell but not inside a TUI overlay), inspect in this order:

1. AppKit menu `keyEquivalent` intercepting `keyDown` before the terminal sees it. Enable `config.debug_key_events = true`, restart the app, then `grep 'key_event.*CTRL' ~/.local/share/kaku/kaku-gui-log-<pid>.txt`. If the log shows only `key_is_down: false` and no matching `key_is_down: true`, the AppKit menu absorbed the event; do not chase termwiz or PTY.
2. Cooked-mode tests (`cat -v` showing `^C`) do **not** rule out menu interception. Reproduce inside a raw-mode TUI before forming a hypothesis.
3. Only after step 1 rules out menu interception, inspect termwiz encoding (`termwiz/src/input.rs`), then PTY / termios state.

For TUI display corruption after interactive CLIs re-render prompts or selection lists, first capture a minimal ANSI transcript. Add a terminal-core regression around cursor-up (`CSI n A`), full-line erase (`CSI 2K`), cursor-down (`CSI 1B`), wrapped rows, and styled prompt symbols. If the core transcript passes but the built app differs from Terminal.app, inspect GUI width, cell metrics, resize, and wrapping inputs rather than changing terminal semantics blindly.

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
| macOS platform | `.claude/rules/macos.md` | AppKit menu / keyEquivalent traps, menubar init timing |

## Verification

| Change type | Command |
|---|---|
| Rust compile check | `make check` |
| Rust logic change | `make test` |
| Formatting | `make fmt-check` |
| Lints | `cargo clippy --locked --all-targets -p kaku -p kaku-gui -p mux -p config -- -D warnings` (`make check` does not run clippy, so a change that passes locally can still turn CI's Clippy job red) |
| GUI or rendering change | `make app` |
| Config release change | `./scripts/check_config_release_readiness.sh` and `./scripts/check_release_config.sh` |
| Release note change | `./scripts/check_release_notes.sh` |
| Release-adjacent change | `make fmt && make check && make test`, then `make app` |
| `crates/kaku-relay` change | `cargo check --locked --manifest-path crates/kaku-relay/Cargo.toml` (it is in `workspace.exclude`, so `make check` and `make test` never see it; CI's `relay-check` job is the only gate) |

For GUI or rendering issues, read `kaku-gui/AGENTS.md` first and verify with `make app`, not only `make dev`.

When a check needs clicks on the built app, hand the maintainer an isolated launch command (`open -na dist/Kaku.app --env HOME=<scratch home with a copy of their kaku.lua>`) and numbered taps with the expected result for each, instead of driving synthetic mouse events: the maintainer's own Kaku takes focus back and the test window may sit on another Space. Static screenshots with `screencapture -l` are still fine when the window is on screen.

## Current Risk Areas

- In-app update replace/restart must confirm on every entry path (menu Check for Updates, menu Restart to Update, toast, CLI direct ZIP, brew cask). Do not set `KAKU_UPDATE_AUTO_CONFIRM` for exploratory menu check; only the overlay-confirmed path may auto-confirm. Sibling entry points tend to regress independently; after changing one path, sweep the matrix and keep the guards in `kaku-gui` (`user_facing_update_events_route_through_confirm`) and `kaku` (`brew_and_direct_paths_confirm_before_replace`) green. Details: `kaku-gui/AGENTS.md` and `.agents/skills/bugs/SKILL.md`.
- AI chat and shell flows are active product surfaces. Preserve `fast_model`, proxy config, inline `#` query status, syntax highlighting, approval flow, and conversation state behavior.
- `config_version` bumps every release; the source of truth is `assets/shell-integration/config_version.txt` and the gate is `scripts/check_release_config.sh`. Config schema changes must update bundled defaults, docs, release checks, and migration behavior together. Per-version history and the migration rule (only keys that existed in the previous released version need migration code) live in `docs/config-versions.md`. Do not hardcode the current version number in agent guides; it goes stale between releases. A config_version bump is also the only thing that regenerates an existing install's `~/.config/kaku/zsh/kaku.zsh`, so a shell-integration fix reaches stable users through the bump; a Nightly built without one needs `kaku init --update-only`, and replies pointing at that Nightly must say so.
- GUI regressions can come from overlay resize, pane split/removal, macOS worker thread lifetime, WebGPU surface reconfigure, tab bar spacing, and alternate-screen wheel scroll behavior.
- Startup performance depends on caching shell user vars, Lua bytecode, early appearance queries, GLSL version, and built-in fonts. Do not invalidate those caches without measurement.
- `kaku doctor` ends every run by writing `Kaku-Diagnose.zip` (`kaku/src/diagnostics.rs`). It also collects the `kaku-hang-*.txt` stack samples that `kaku-gui/src/stall_watchdog.rs` writes when the GUI thread stays blocked past 1s, so rename that prefix in both files together. Everything in the bundle goes through its redactor, `assistant.toml` is never read, and large files (session content can be gigabytes, #559) are listed by size, not copied.
- Notification actions that call back into Kaku should resolve bundled executables relative to the running app, not an assumed system path.
- Known high-regression zones: theme/config TUI initialization (`kaku/src/ai_config/tui/`), macOS window geometry (`window/src/os/macos/window.rs`, `kaku-gui/src/termwindow/resize.rs`), and menubar initialization timing (`window/src/os/macos/menu.rs`). Changes touching these must ship with a regression test or assertion.
- `assets/shell-integration` scripts run in the user's shell, not just at build time. Bash heredocs that generate zsh (e.g. `setup_zsh.sh` writing `kaku.zsh`) expand backticks and `$(...)` at generation time, so escape any that must reach the output literally (#450), and never put `local` outside a function (#432/#441). CI gates this: shellcheck (`--severity=error`, catches SC2168) over the bash scripts plus a `zsh -n` parse check of the generated `kaku.zsh` in the setup smoke.

## Release Notes

Tag format is `V0.x.x`. `scripts/release.sh` is the source of truth for tagged releases. The GitHub Release title comes from the first heading in `.github/RELEASE_NOTES.md`.

Before drafting release notes, read the previous formal release (`gh release view <latest-tag>`) and treat it as the format template: title `V{version} {Codename}` with a one-word codename, a centered logo header with the product tagline, `### Changelog` as numbered `**Label**: one sentence` items, a `### 更新日志` section whose Chinese items map one-to-one by number, and a `> https://github.com/tw93/Kaku` footer. After publishing, add all six positive reactions (`+1`, `laugh`, `heart`, `hooray`, `rocket`, `eyes`) to the release via `gh api` and read them back to confirm; never add `-1` or `confused`.

## Pre-release Runtime Smoke

CI gates fmt/check/clippy/tests but cannot see visual layout, native AppKit, render timing, or shell-in-user-env behavior, which is where most post-release reports come from. The checklist lives in one place: `.agents/skills/release/SKILL.md` «Pre-release smoke checklist», covering macOS window, tab bar, shell setup, AI chat, and render timing, with the issue numbers each item guards and a note on which slices now have automated coverage. Run it by hand in the built `dist/Kaku.app` before tagging, and add the reproduction there when a release fixes a bug outside the list.

Releases that touch windowing, titlebar coloring, tab bar layout, or transparency also need the config matrix in `docs/release-checklist.md`, which enumerates the tab position / tab style / opacity / window state combinations to check by hand and names the `update_titlebar_background()` regression guard.

## Documentation Maintenance

- Single-crate behavior belongs in that crate's `AGENTS.md`.
- Cross-crate behavior should update every affected subsystem guide.
- Build, CI, release, and maintainer workflow changes belong in this root file.
- Shared agent instructions belong in tracked docs. Personal overrides belong in ignored local files.
- One-off review reports, scorecards, and diagnostic snapshots are evidence, not durable project docs. Extract stable rules or verification gates into `AGENTS.md`, `CLAUDE.md`, subsystem guides, scripts, or tests, then remove the transient report.
- Do not hide user-visible behavior changes inside maintainability or cleanup patches. New UI, config fields, defaults, or workflow permissions should be split into their own change unless the maintainer explicitly approved that scope.
