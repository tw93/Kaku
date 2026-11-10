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
- `.agents/skills/` - the canonical and tracked home for project skills: `release` and `bugs`. The website skill `product-docs` lives with the site on the `vercel` worktree (`~/www/kaku-site/.agents/skills/product-docs`). `.claude/` is gitignored except for the force-added files in `.claude/rules/`; its skill entries may be relative symlinks or ignored private overrides. Do not cite, copy, or edit an ignored `.claude/skills/` directory as project truth. Put durable skill changes in `.agents/skills/` first, then update a symlink when that entry is actually a link.
- `.github/workflows/checks.yml` - fast correctness gates on pushes to `main` and on pull requests, with a `paths` filter that skips `**.md` and assets, so a docs-only change gets no CI at all, except the files a later positive pattern brings back because a check reads or the build embeds them (`docs/incidents.md`, `.github/RELEASE_NOTES.md`, `assets/logo.png`); `scripts/test_ci_paths.py` fails when a code file, an `include_bytes!`/`include_str!` target or a listed read file would be skipped. Three macOS jobs: Unit Tests, Clippy (which also carries the source guards and stands in for `cargo check`, scoped to the lint-opt-in crates, not the workspace), and Scripts and Release Checks (browser terminal regressions, release notes, config version, incident ledger, shell smoke tests, shellcheck); Format and Relay Build run on Linux, and so does Security Audit. The runner caps macOS jobs at five across all workflows, and Build Validation takes two, so a sixth macOS job queues.
- `.github/workflows/build-validation.yml` - release-shaped builds with the shipped `release-opt` profile: one job per architecture in parallel (the arm64 one also assembles the bundle), then a `lipo` merge of `kaku-gui`, the one binary the bundle ships; runs on build-pipeline changes, daily (skipped when that commit already passed), or on dispatch. `release.sh` preflight requires its latest run green and the latest Security Audit run not failed, and skips its local fmt/check/test when CI Checks already passed on HEAD.
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

`make build`, `make check`, `make test` and `scripts/build.sh` first run `scripts/prune_target.sh`, which measures `target/` at most once a day and runs `cargo clean` once it reaches `KAKU_TARGET_CAP_GB` (default 40). cargo never drops old artifacts, and the profiles, architectures and remote on/off feature sets here grew it to 92G; a partial, age-based delete was rejected because it can leave fingerprints pointing at missing files. No scheduled job does this, by the maintainer's choice. The dev profile also builds third-party crates without debug info and workspace crates with line tables only (`CARGO_PROFILE_DEV_DEBUG=true` brings back lldb variables for one session).

`make fmt` and `make fmt-check` both shell out to `cargo +nightly fmt`, so they require the nightly toolchain. Use `make app` for GUI, rendering, windowing, and AI overlay verification because it builds the app bundle that users run.

## Working Rules

- Keep changes inside one crate or subsystem when the problem allows it.
- Upstream: Kaku forked WezTerm at `05343b38` (2026-01-17; term/, mux/, the escape parser and wezterm-font matched it byte for byte). On 2026-10-14 upstream main up to `e2a98e1b` was reviewed and its macOS-relevant fixes absorbed (commits citing upstream shas, e.g. DECRQCRA, RIS reset, OSC 8 `;`, tmux -CC input, workspace repaint). Start the next sync from `e2a98e1b`; map `wezterm-gui` to `kaku-gui` and top-level crates to `crates/`, and judge applicability rather than applying patches literally.
- Kaku is macOS-only. Windows code (cfg(windows) branches, ConPTY/GDI/WSL paths, winapi and the other Windows-only dependencies) was removed on 2026-10-13; do not bring it back when porting upstream WezTerm fixes, port only the macOS-relevant half. `wsl_domains`, `win32_system_backdrop` and `win32_acrylic_accent_color` stay as deprecated config keys so shared configs still load (`windows_only_keys_still_load`). Vendored crates under `deps/` keep their upstream Windows code untouched.
- Inspect public APIs and cross-crate boundaries before changing shared behavior.
- Kaku must not automatically inject agent hooks, instructions, or settings. Explicit edits through `kaku ai` may update the selected provider's configuration (including Claude and Codex settings), preserving user-owned fields. Agent-facing help belongs in documentation. The `kaku init` source line in `.zshrc` is managed by `assets/shell-integration/setup_zsh.sh` and checked by `doctor.rs`; `reset.rs` must keep every user-owned line around it.
- Narrowing a default that existing users already depend on must ship the other camp's explicit path in the same change. Shell integration scopes the Starship prompt to `TERM_PROGRAM=Kaku` OR (`$TMUX` and `KAKU_SESSION`), with `KAKU_PROMPT_EVERYWHERE=1` as the opt-in for every terminal; Smart Tab stays Kaku-only (#503). The predicate is duplicated inside each installer, three times in `setup_zsh.sh` and four in `setup_fish.sh`, so grep both files and change every copy. `assets/shell-integration/tests/prompt_scope_smoke.sh` gates the placements in CI via `.github/workflows/checks.yml`, and `docs/faq.md` is the user-facing half; a change that re-narrows the scope must update both installers, the smoke and the doc together.
- Tab indicator decision (2026-08-24): the tab bar draws a dot only for `Progress::Paused | Progress::Error` (attention, `kaku-gui/src/tabbar.rs`); a running pane deliberately shows nothing, the running-state dot was dropped in `4f297556`. Do not bring it back.
- Line height decision (2026-07): the bundled default stays `line_height = 1.28`. Character-cell graphics (QR codes, `neofetch` logos, TUI bar charts) scale with the row height, so they render about 28% taller than at `line_height = 1.0`, and that trade-off is accepted. Rendering block glyphs at their natural cell height was tried in `d810b13` and reverted in `3f131ea`: block characters must fill the whole cell or box borders and QR codes open horizontal seams. Do not propose lowering the default or re-attempting natural-height block rendering; point users at the `line_height` entries in `docs/faq.md` and `docs/configuration.md`. The block cursor is a separate case and correctly keeps using `natural_cell_height` (see `kaku-gui/AGENTS.md`).
- The marketing and docs site lives on the `vercel` branch (linked worktree at `~/www/kaku-site`), not on `main`. It follows the design guide at `~/www/kaku-site/DESIGN.md`; verify changes with screenshots at 375px / 1280px and deploy by pushing the `vercel` branch (Vercel serves kaku.fun).
- **Do not propose UI i18n / multi-language menus / a `config.language` setting.** The `rust-i18n` based Chinese UI localization (PR #362, commit `f6cfb4b`) was reverted in `b4d779a` on 2026-05-18; `language` remains in the config schema as a deprecated field for backward compat only. UI strings (menus, confirm dialogs, config TUI copy) stay as English literal strings. New UI surfaces should not introduce translation keys, locale-aware formatting, or "what if a user wants Chinese" abstractions. If a user requests a non-English UI, route to the assistant config / AI chat surface; those already accept non-English content.
- **Kaku stays free, including Kaku Assistant and remote access (maintainer 2026-10-18).** Kaku Assistant runs on the user's own API key or Codex login, so it has no cost to meter. The relay (`crates/kaku-relay`, one Fly machine) is paid for by the maintainer as a thank-you to the community, and `docs/faq.md` says so. Do not propose paid tiers, usage-based billing, accounts or usage metering for the relay; when it nears the concurrency limit in `fly.toml`, load test and move to a larger machine instead.
- **Do not pre-bake provider abstractions in `kaku/src/ai_config/`.** Kaku Assistant parsing, field presentation, and persistence live in `kaku/src/ai_config/tui/providers/assistant.rs`; the TUI retains its event loop and shared data types. Future extraction should move one concrete provider at a time into that existing directory. Do not add an unused trait, a `ProviderKind` enum, or stub modules. Save Copilot for last because its OAuth flow is the abstraction stress test.
- CLI Codex readiness and GUI connection parsing both consume `tests/fixtures/codex-connection.json`. Add boundary cases there when either policy changes, and run both crates' tests.
- Bundle layout (2026-10-29): the bundle ships one binary. `Contents/MacOS/kaku` and `Contents/MacOS/k` are relative symlinks to `kaku-gui`, which picks by the name it was started under (`multicall_entry` in `kaku-gui/src/main.rs`): `kaku-gui` is the GUI, `k` the chat CLI (`kaku_gui_lib::cli_chat::k_main`), and any other name the CLI (`kaku::cli_main`, the `kaku` crate is a lib), as the standalone CLI ran under whatever name a user linked it to. The separate binaries were 93% and 87% the same code. `proc_pidpath` resolves the symlink, so `LocalProcessInfo::executable_path` (`crates/procinfo/src/macos.rs`) maps a `kaku-gui` process back to `kaku` or `k` by its argv0 with the same rule; change both together. Scripts that exec the CLI must not resolve the symlink (`test_main_menu.py` uses `absolute()`). `build.sh` builds only `--bin kaku-gui`; the `kaku` and `k` bin targets remain for `cargo run` and tests. No ANGLE dylibs ship: the WezTerm-era copy was x86_64-only, so Apple Silicon always fell back to CGL, and only Intel users who set `prefer_egl = true` with the OpenGL front end lost it (they get CGL, the Intel default). The Unicode name table is a compressed data file. Do not reintroduce a separate CLI binary or bundle `Frameworks/` without measuring the package size.
- Official Homebrew token is `kaku` (`brew install --cask kaku`). `tw93/tap/kakuku` remains the personal tap for people who installed before the official cask. Do not tell users the Homebrew `kaku` token is a different app.
- Incidents and risk (2026-10-26): a fix for a defect that reached a tagged release adds its `docs/incidents.md` row in the same commit, naming the fix and introducing commits, the earliest layer that should have caught it and the test, smoke script or workflow step that catches it now; `scripts/check_incidents.py` (in `make check` and the Scripts and Release Checks job, with `scripts/test_review_gates.py`) fails a row whose commit, layer or catcher does not hold. A defect nothing catches yet gets a test that fails on the old code before its row lands. The verification a change needs follows its paths, not its author: `scripts/risk_tier.py` (printed by `make check`; pass your own files in a shared worktree) names the extra gates for release scripts and version files, the in-app updater, workflows, config versioning and the installers that edit user dotfiles.

- Do not modify files outside this repository without showing the intended change and getting explicit confirmation.
- Do not hide user-visible behavior changes inside maintainability or cleanup patches. New UI, config fields, defaults, or workflow permissions should be split into their own change unless the maintainer explicitly approved that scope.

## Maintainer Follow-up

- Do not close issues or PRs on local green alone. For fixes pushed to `main`, wait for the new GitHub Actions run on `main` to pass before posting fixed/closed replies.
- If an accepted PR's equivalent fix lands outside the contributor branch, acknowledge the contribution and explain which user-visible fix was delivered before closing it; keep commit hashes and internal attribution bookkeeping out of the public reply.
- The latest stable tag is `git tag --list 'V*' --sort=-version:refname | head -1`; without the `V*` filter the rolling `nightly` tag sorts first. Map each report to `git log <tag>..HEAD`, and treat a closed issue as a claim until the fix commit or remaining gap is named.
- Shell integration fixes run the affected `assets/shell-integration/tests/*_smoke.sh`, then the list `.github/workflows/checks.yml` runs under `Shell integration smoke tests`. Watch the pushed `main` run with `gh run watch <id> --exit-status`.
- The rolling `nightly` release is not rebuilt by push. Before sending users to Nightly, run or verify `./scripts/nightly.sh` and confirm `gh release view nightly --json tagName,targetCommitish,publishedAt,assets,url` points at the fix commit and includes `Kaku-nightly.dmg`.
- Default issue-closure pipeline once a fix is verified: commit the fix, refresh the `nightly` release assets per the rule above, reply in the reporter's language with the Nightly download or in-app update path, then close if the current request already authorizes closure; otherwise propose it and wait for authorization. Do not promise a specific packaged-release date in replies.
- For several unrelated bugs, prefer one commit per issue or behavior, and put the issue number in the subject: `fix(scope): #123 short summary`.
- Merge contributor PRs with `gh pr merge --merge`, never squash, so the contributor's own commits and authorship stay in history; add no trailer. When `maintainerCanModify` is true, push fix commits to the contributor's branch rather than rewriting their existing commits, and approve a first-time contributor's workflow run with `gh api -X POST repos/tw93/Kaku/actions/runs/<id>/approve` before expecting CI.

## Investigation Order

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

## Where the detail lives

Claude Code loads each `.claude/rules/*.md` when a file matching its `paths:` is read. On another runtime, read the matching file before working there.

- `.claude/rules/tab-bar.md`: read before proposing any control in the tab bar; it records what was tried and rejected.
- `.claude/rules/remote.md`: web terminal, relay and the desktop view package. Read it before proposing any remote access design; registration tokens and pairing QR codes were rejected.

## Verification

| Change type | Command |
|---|---|
| Rust compile check | `make check` |
| Rust logic change | `make test` |
| Formatting | `make fmt-check` |
| Lints | `cargo clippy --locked --all-targets -p kaku -p kaku-gui -p mux -p config -- -D warnings` (`make check` does not run clippy, so a change that passes locally can still turn CI's Clippy job red) |
| GUI or rendering change | `make app` |
| CLI main menu input | `make app`, then `python3 scripts/test_main_menu.py` (real PTY, q/Q/Esc exit) |
| Config release change | `./scripts/check_config_release_readiness.sh` and `./scripts/check_release_config.sh` |
| Release note change | `./scripts/check_release_notes.sh` |
| Release-adjacent change | `make fmt && make check && make test`, then `make app` |
| `crates/kaku-relay` change | `cargo check --locked --manifest-path crates/kaku-relay/Cargo.toml` (it is in `workspace.exclude`, so `make check` and `make test` never see it; CI's `relay-check` job is the only gate) |

For GUI or rendering issues, read `kaku-gui/AGENTS.md` first and verify with `make app`, not only `make dev`.

The maintainer's installed `/Applications/Kaku.app` is the private package with the remote desktop view (2026-10-26). A request to replace their local app means building that package from current main, the helper first and then `CARGO_FEATURES=kaku-gui/desktop` with `KAKU_DESKTOP_HELPER` (exact command in `.claude/rules/remote.md`), and checking that `Contents/MacOS/kaku-desktop` is in the bundle before the swap; never swap in a Release, Nightly or plain `build.sh` package, since that removes the desktop view.

Restarting Kaku (2026-10-14, narrowed 2026-10-21): run `scripts/restart_kaku.sh` only when the maintainer asks for a restart in the current message (重启 Kaku). Swapping a new build into `/Applications`, or an earlier request in the session, is not a request to restart: finish the swap, then ask and wait, since the restart ends the maintainer's own windows and any agent session inside Kaku. An agent session running inside Kaku ends with it, so write the reply first, tell the maintainer to reopen the session with `claude -c` (or the equivalent), and make the script the last action. Its log is `~/Library/Logs/kaku-restart.log`.

Standing authorization (2026-10-19): after a verified change to `crates/kaku-remote/assets/web` is pushed, publish the relay page with `scripts/publish_web_page.sh` without asking. Confirm the deploy reached `t.kaku.fun` by fetching `https://t.kaku.fun/index.html` and finding a string the change added, and check the response's CSP when the change touches the script or the publish script.

When a check needs clicks on the built app, hand the maintainer an isolated launch command and numbered taps instead of driving synthetic mouse events; the command, and what a shell inside Kaku can and cannot test, are in `docs/testing-built-app.md`.

## Current Risk Areas

- In-app update replace/restart must confirm on every entry path (menu Check for Updates, menu Restart to Update, toast, CLI direct ZIP, brew cask). Do not set `KAKU_UPDATE_AUTO_CONFIRM` for exploratory menu check; only the overlay-confirmed path may auto-confirm. Sibling entry points tend to regress independently; after changing one path, sweep the matrix and keep the guards in `kaku-gui` (`user_facing_update_events_route_through_confirm`) and `kaku` (`brew_and_direct_paths_confirm_before_replace`) green. Details: `kaku-gui/AGENTS.md` and `.agents/skills/bugs/SKILL.md`.
- AI chat and shell flows are active product surfaces. Preserve `fast_model`, proxy config, inline `#` query status, syntax highlighting, approval flow, and conversation state behavior.
- `config_version` bumps every release; the source of truth is `assets/shell-integration/config_version.txt` and the gate is `scripts/check_release_config.sh`. Config schema changes must update bundled defaults, docs, release checks, and migration behavior together. Per-version history and the migration rule (only keys that existed in the previous released version need migration code) live in `docs/config-versions.md`. Do not hardcode the current version number in agent guides; it goes stale between releases. A config_version bump is also the only thing that regenerates an existing install's `~/.config/kaku/zsh/kaku.zsh`, so a shell-integration fix reaches stable users through the bump; a Nightly built without one needs `kaku init --update-only`, and replies pointing at that Nightly must say so.
- GUI regressions can come from overlay resize, pane split/removal, macOS worker thread lifetime, WebGPU surface reconfigure, tab bar spacing, and alternate-screen wheel scroll behavior.
- Startup performance depends on caching shell user vars, Lua bytecode, early appearance queries, GLSL version, and built-in fonts. Do not invalidate those caches without measurement.
- `kaku doctor` ends every run by writing `Kaku-Diagnose.zip` (`kaku/src/diagnostics.rs`). It also collects the `kaku-hang-*.txt` stack samples that `kaku-gui/src/stall_watchdog.rs` writes when the GUI thread stays blocked past 1s, so rename that prefix in both files together. A freeze where keys, scrolling and tab switches do nothing but menus still open means main-thread tasks stopped running (seen after update notification clicks in V0.25.0 and V0.27.0, root cause still open): the watchdog then logs `GUI thread still blocked: observer last fired ...` and asks `window::rescue_main_thread_queue` to drain through the main dispatch queue, and `run loop observer silent` means the CFRunLoop observer in `window/src/spawn.rs` had died and every later wakeup goes through the dispatch queue. Quote those lines when the report comes back. Everything in the bundle goes through its redactor, which also hides typed text in `debug_key_events` lines (printable keys, physical key codes, composed and IME text) including committed `KeyCode::Composed` text as well as `DeadKeyStatus::Composing`, while keeping Ctrl and Cmd keys readable for key reports, `assistant.toml` is never read, and large files (session content can be gigabytes, #559) are listed by size, not copied.
- Notification actions that call back into Kaku should resolve bundled executables relative to the running app, not an assumed system path.
- Known high-regression zones: theme/config TUI initialization (`kaku/src/ai_config/tui/`), macOS window geometry (`window/src/os/macos/window.rs`, `kaku-gui/src/termwindow/resize.rs`), and menubar initialization timing (`window/src/os/macos/menu.rs`). Changes touching these must ship with a regression test or assertion.
- `assets/shell-integration` scripts run in the user's shell, not just at build time. Bash heredocs that generate zsh (e.g. `setup_zsh.sh` writing `kaku.zsh`) expand backticks and `$(...)` at generation time, so escape any that must reach the output literally (#450), and never put `local` outside a function (#432/#441). An expired `.zcompdump` needs `compinit && touch` after a successful full check: `compinit` can leave an unchanged dump's timestamp untouched. Keep the failure path stale and run `assets/shell-integration/tests/compinit_cache_smoke.sh`, which checks the next shell returns to `-C`. CI gates this: shellcheck (`--severity=error`, catches SC2168) over the bash scripts plus a `zsh -n` parse check of the generated `kaku.zsh` in the setup smoke.
- User-owned configuration symlinks must survive updates, and failed writes must leave their targets unchanged. Resolve the final target and finish a sibling temporary file before replacing it; preserve target permissions. The shell `write_through` helper has four copies across the zsh/fish installers and their generated Yazi wrappers. Test all four under a real write failure, not only successful linked-file updates (#581, #582).

## Release Notes

Tag format is `V0.x.x`. `scripts/release.sh` is the source of truth for tagged releases. The GitHub Release title comes from the first heading in `.github/RELEASE_NOTES.md`.

Before drafting release notes, read the previous formal release (`gh release view <latest-tag>`) and treat it as the format template: title `V{version} {Codename}` with a one-word codename, a centered logo header with the product tagline, `### Changelog` as numbered `**Label**: one sentence` items, a `### 更新日志` section whose Chinese items map one-to-one by number, and a `> https://github.com/tw93/Kaku` footer. After publishing, add all six positive reactions (`+1`, `laugh`, `heart`, `hooray`, `rocket`, `eyes`) to the release via `gh api` and read them back to confirm; never add `-1` or `confused`.

## Pre-release Runtime Smoke

CI gates fmt/check/clippy/tests but cannot see visual layout, native AppKit, render timing, or shell-in-user-env behavior, which is where most post-release reports come from. The checklist lives in one place: `.agents/skills/release/SKILL.md` «Pre-release smoke checklist», covering macOS window, tab bar, shell setup, AI chat, and render timing, with the issue numbers each item guards and a note on which slices now have automated coverage. Run it by hand in the built `dist/Kaku.app` before tagging, and add the reproduction there when a release fixes a bug outside the list.

Releases that touch windowing, titlebar coloring, tab bar layout, or transparency also need the config matrix in `docs/release-checklist.md`, which enumerates the tab position / tab style / opacity / window state combinations to check by hand and names the `update_titlebar_background()` regression guard.

## Documentation Maintenance

- Build, CI, release, and maintainer workflow changes belong in this root file.
