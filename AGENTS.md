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

- Work on the current branch unless the maintainer asks for a branch or worktree.
- Keep changes inside one crate or subsystem when the problem allows it.
- Upstream: Kaku forked WezTerm at `05343b38` (2026-01-17; term/, mux/, the escape parser and wezterm-font matched it byte for byte). On 2026-10-14 upstream main up to `e2a98e1b` was reviewed and its macOS-relevant fixes absorbed (commits citing upstream shas, e.g. DECRQCRA, RIS reset, OSC 8 `;`, tmux -CC input, workspace repaint). Start the next sync from `e2a98e1b`; map `wezterm-gui` to `kaku-gui` and top-level crates to `crates/`, and judge applicability rather than applying patches literally.
- Kaku is macOS-only. Windows code (cfg(windows) branches, ConPTY/GDI/WSL paths, winapi and the other Windows-only dependencies) was removed on 2026-10-13; do not bring it back when porting upstream WezTerm fixes, port only the macOS-relevant half. `wsl_domains`, `win32_system_backdrop` and `win32_acrylic_accent_color` stay as deprecated config keys so shared configs still load (`windows_only_keys_still_load`). Vendored crates under `deps/` keep their upstream Windows code untouched.
- Inspect public APIs and cross-crate boundaries before changing shared behavior.
- Kaku must not automatically inject agent hooks, instructions, or settings. Explicit edits through `kaku ai` may update the selected provider's configuration (including Claude and Codex settings), preserving user-owned fields. Agent-facing help belongs in documentation. The `kaku init` source line in `.zshrc` is managed by `assets/shell-integration/setup_zsh.sh` and checked by `doctor.rs`; `reset.rs` must keep every user-owned line around it.
- Narrowing a default that existing users already depend on must ship the other camp's explicit path in the same change. Shell integration scopes the Starship prompt to `TERM_PROGRAM=Kaku` OR (`$TMUX` and `KAKU_SESSION`), with `KAKU_PROMPT_EVERYWHERE=1` as the opt-in for every terminal; Smart Tab stays Kaku-only (#503). The predicate is duplicated inside each installer, three times in `setup_zsh.sh` and four in `setup_fish.sh`, so grep both files and change every copy. `assets/shell-integration/tests/prompt_scope_smoke.sh` gates the placements in CI via `.github/workflows/checks.yml`, and `docs/faq.md` is the user-facing half; a change that re-narrows the scope must update both installers, the smoke and the doc together.
- Tab indicator decision (2026-08-24): the tab bar draws a dot only for `Progress::Paused | Progress::Error` (attention, `kaku-gui/src/tabbar.rs`); a running pane deliberately shows nothing, the running-state dot was dropped in `4f297556`. Do not bring it back.
- Retro tab bar chrome (2026-09-09, #548): the bundled config (`assets/macos/Kaku.app/Contents/Resources/kaku.lua`) sets `use_fancy_tab_bar = false`, so the shipped bar is the retro one even though the `config/src/config.rs` schema default is true. `kaku-gui/src/tabbar.rs` composes a `Line` of cells, so a control there is a character on the text baseline competing with the title for the one trailing cell the rule above spends on attention; drawn controls belong in the box-model renderers (`make_x_button` in `kaku-gui/src/termwindow/render/fancy_tab_bar.rs`, and the opt-in touch renderer in `kaku-gui/src/termwindow/touch.rs`). The per-tab close mark added in `cef3a371` (three cells, " X ", over the title end) was reverted in `732d2082` for that reason. On 2026-10-12 it was tried again as a single hover `×` in the status cell, with one and then two cells of padding per side; the maintainer judged it worse than none (a grid glyph cannot sit centered between title and edge) and removed it: the desktop layout closes tabs from the tab's right-click menu, middle-click and Cmd+W, while the web terminal keeps its hover close icon, which is drawn and centered. Answer tab bar chrome requests with the pointers that already exist: the new-tab button is on by default (bundled `show_new_tab_button_in_tab_bar = true`), right-click on a tab opens a tab menu with New Tab, Tab Navigator and Close Tab (`tab_context_menu_actions` in `kaku-gui/src/termwindow/mouseevent.rs`), middle-click closes a tab, and the Shell menu lists New Tab and Close Tab. For touch over remote desktop, a plain left press (a tap) in the padding strip above a bottom bar, and in Touch Mode up to `BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT` into the last terminal row, still hits tabs and the new-tab button, while middle and right presses there stay with the terminal, so the bar itself stays one row. The reach into the row is Touch Mode only (#606): with a mouse, a click on the lower half of the last row switched tabs. Do not re-add a native close mark: if one is ever wanted, it has to be a drawn, centered icon like the fancy bar's `make_x_button`, not a grid character. The desktop bottom retro bar is one terminal row with line_height (`tab_bar_pixel_height_impl`), the same as a top bar; the compact natural height read as cramped. Touch mode (2026-10-14) is a per-window toggle with one entry, Window > Touch Mode; the on-bar Touch button and the titlebar entry were removed as clutter. It keeps the normal bar height and look and only adds a drawn, centered close icon to every retro tab, over cells the bar keeps at the tab's end (`close_slots` in `kaku-gui/src/tabbar.rs`). The separate 40pt touch bar, its Menu button and panels were removed; do not bring them back. A long press (left press held 450ms, under 24pt of movement, acted on at release) replays as a right click in every mode, so tab and terminal context menus cover touch; UU was measured to forward a long press as a held left button, never a right click.
- Line height decision (2026-07): the bundled default stays `line_height = 1.28`. Character-cell graphics (QR codes, `neofetch` logos, TUI bar charts) scale with the row height, so they render about 28% taller than at `line_height = 1.0`, and that trade-off is accepted. Rendering block glyphs at their natural cell height was tried in `d810b13` and reverted in `3f131ea`: block characters must fill the whole cell or box borders and QR codes open horizontal seams. Do not propose lowering the default or re-attempting natural-height block rendering; point users at the `line_height` entries in `docs/faq.md` and `docs/configuration.md`. The block cursor is a separate case and correctly keeps using `natural_cell_height` (see `kaku-gui/AGENTS.md`).
- Draft issue and PR replies unless the maintainer has already approved the exact public action.
- Do not modify files outside this repository without showing the intended change and getting explicit confirmation.
- Do not add instructions for the removed `website/` tree unless that directory exists in the current worktree.
- The marketing and docs site lives on the `vercel` branch (linked worktree at `~/www/kaku-site`), not on `main`. It follows the design guide at `~/www/kaku-site/DESIGN.md`; verify changes with screenshots at 375px / 1280px and deploy by pushing the `vercel` branch (Vercel serves kaku.fun).
- Keep private credentials, local keychain paths, and machine-specific release notes out of public repository docs.
- **Do not propose UI i18n / multi-language menus / a `config.language` setting.** The `rust-i18n` based Chinese UI localization (PR #362, commit `f6cfb4b`) was reverted in `b4d779a` on 2026-05-18; `language` remains in the config schema as a deprecated field for backward compat only. UI strings (menus, confirm dialogs, config TUI copy) stay as English literal strings. New UI surfaces should not introduce translation keys, locale-aware formatting, or "what if a user wants Chinese" abstractions. If a user requests a non-English UI, route to the assistant config / AI chat surface; those already accept non-English content.
- **Kaku stays free, including Kaku Assistant and remote access (maintainer 2026-10-18).** Kaku Assistant runs on the user's own API key or Codex login, so it has no cost to meter. The relay (`crates/kaku-relay`, one Fly machine) is paid for by the maintainer as a thank-you to the community, and `docs/faq.md` says so. Do not propose paid tiers, usage-based billing, accounts or usage metering for the relay; when it nears the concurrency limit in `fly.toml`, load test and move to a larger machine instead.
- **Do not pre-bake provider abstractions in `kaku/src/ai_config/`.** Kaku Assistant parsing, field presentation, and persistence live in `kaku/src/ai_config/tui/providers/assistant.rs`; the TUI retains its event loop and shared data types. Future extraction should move one concrete provider at a time into that existing directory. Do not add an unused trait, a `ProviderKind` enum, or stub modules. Save Copilot for last because its OAuth flow is the abstraction stress test.
- CLI Codex readiness and GUI connection parsing both consume `tests/fixtures/codex-connection.json`. Add boundary cases there when either policy changes, and run both crates' tests.
- Bundle layout (2026-10-29): the bundle ships one binary. `Contents/MacOS/kaku` and `Contents/MacOS/k` are relative symlinks to `kaku-gui`, which picks by the name it was started under (`multicall_entry` in `kaku-gui/src/main.rs`): `kaku-gui` is the GUI, `k` the chat CLI (`kaku_gui_lib::cli_chat::k_main`), and any other name the CLI (`kaku::cli_main`, the `kaku` crate is a lib), as the standalone CLI ran under whatever name a user linked it to. The separate binaries were 93% and 87% the same code. `proc_pidpath` resolves the symlink, so `LocalProcessInfo::executable_path` (`crates/procinfo/src/macos.rs`) maps a `kaku-gui` process back to `kaku` or `k` by its argv0 with the same rule; change both together. Scripts that exec the CLI must not resolve the symlink (`test_main_menu.py` uses `absolute()`). `build.sh` builds only `--bin kaku-gui`; the `kaku` and `k` bin targets remain for `cargo run` and tests. No ANGLE dylibs ship: the WezTerm-era copy was x86_64-only, so Apple Silicon always fell back to CGL, and only Intel users who set `prefer_egl = true` with the OpenGL front end lost it (they get CGL, the Intel default). The Unicode name table is a compressed data file. Do not reintroduce a separate CLI binary or bundle `Frameworks/` without measuring the package size.
- Official Homebrew token is `kaku` (`brew install --cask kaku`). `tw93/tap/kakuku` remains the personal tap for people who installed before the official cask. Do not tell users the Homebrew `kaku` token is a different app.
- Incidents and risk (2026-10-26): a fix for a defect that reached a tagged release adds its `docs/incidents.md` row in the same commit, naming the fix and introducing commits, the earliest layer that should have caught it and the test, smoke script or workflow step that catches it now; `scripts/check_incidents.py` (in `make check` and the Scripts and Release Checks job, with `scripts/test_review_gates.py`) fails a row whose commit, layer or catcher does not hold. A defect nothing catches yet gets a test that fails on the old code before its row lands. The verification a change needs follows its paths, not its author: `scripts/risk_tier.py` (printed by `make check`; pass your own files in a shared worktree) names the extra gates for release scripts and version files, the in-app updater, workflows, config versioning and the installers that edit user dotfiles.

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
| CLI main menu input | `make app`, then `python3 scripts/test_main_menu.py` (real PTY, q/Q/Esc exit) |
| Config release change | `./scripts/check_config_release_readiness.sh` and `./scripts/check_release_config.sh` |
| Release note change | `./scripts/check_release_notes.sh` |
| Release-adjacent change | `make fmt && make check && make test`, then `make app` |
| `crates/kaku-relay` change | `cargo check --locked --manifest-path crates/kaku-relay/Cargo.toml` (it is in `workspace.exclude`, so `make check` and `make test` never see it; CI's `relay-check` job is the only gate) |

For GUI or rendering issues, read `kaku-gui/AGENTS.md` first and verify with `make app`, not only `make dev`.

The maintainer's installed `/Applications/Kaku.app` is the private package with the remote desktop view (2026-10-26). A request to replace their local app means building that package from current main, the helper first and then `CARGO_FEATURES=kaku-gui/desktop` with `KAKU_DESKTOP_HELPER` (exact command in `crates/AGENTS.md`), and checking that `Contents/MacOS/kaku-desktop` is in the bundle before the swap; never swap in a Release, Nightly or plain `build.sh` package, since that removes the desktop view.

Restarting Kaku (2026-10-14, narrowed 2026-10-21): run `scripts/restart_kaku.sh` only when the maintainer asks for a restart in the current message (重启 Kaku). Swapping a new build into `/Applications`, or an earlier request in the session, is not a request to restart: finish the swap, then ask and wait, since the restart ends the maintainer's own windows and any agent session inside Kaku. It restarts into `/Applications/Kaku.app` the way an in-app update does (TERM, then reopen; tabs return from the periodic session snapshot) and detaches first. It stops every running Kaku GUI, a copy opened from `dist/` included, but sends TERM only to the one that owns `last_session.json` (its pid names the snapshot's `content_dir`) and SIGKILLs the rest after it exits, so a stray copy cannot overwrite the tabs when it quits. An agent session running inside Kaku ends with it, so write the reply first, tell the maintainer to reopen the session with `claude -c` (or the equivalent), and make the script the last action. Its log is `~/Library/Logs/kaku-restart.log`. The detached run is marked with a `--detached` argument, never an environment variable: `/usr/bin/open` hands the caller's environment to the app, so a variable reached every shell in the reopened Kaku and the next restart skipped detaching and died with it (`test_leaked_detach_variable_still_detaches`). An absent snapshot or empty process list must still reach the reopen step; `scripts/test_restart_kaku.py` exercises those branches and owner-only saving with fake processes, never the running app.

Standing authorization (2026-10-19): after a verified change to `crates/kaku-remote/assets/web` is pushed, publish the relay page with `scripts/publish_web_page.sh` without asking. Confirm the deploy reached `t.kaku.fun` by fetching `https://t.kaku.fun/index.html` and finding a string the change added, and check the response's CSP when the change touches the script or the publish script.

When a check needs clicks on the built app, hand the maintainer an isolated launch command (`open -na dist/Kaku.app --env HOME=<scratch home with a copy of their kaku.lua> --env TMPDIR=<scratch dir>/`) and numbered taps with the expected result for each, instead of driving synthetic mouse events: the maintainer's own Kaku takes focus back and the test window may sit on another Space. Static screenshots with `screencapture -l` are still fine when the window is on screen. A shell running inside Kaku carries `KAKU_CONFIG_FILE`, `KAKU_CONFIG_DIR`, `KAKU_SESSION`, `KAKU_UNIX_SOCKET`, `KAKU_ZSH_DIR` and `XDG_CONFIG_HOME`, and `open` hands them to the app, so a test instance launched without `env -u` for each reads the maintainer's real config and session, and may connect to the relay; shell smoke tests run from such a shell need the same unsets. Remote state lives in `$TMPDIR/kaku-remote.json` and `kaku-remote-relay.json`, which a scratch HOME does not isolate, so every test instance gets a scratch `TMPDIR` too, remote on or off: one with remote on overwrites the maintainer's copies, and on 2026-10-25 test instances with remote off, from builds before the state file recorded its owner's pid, deleted the maintainer's `kaku-remote.json`, after which `kaku remote` failed until their Kaku restarted. If that happens anyway, say that their Kaku needs a restart for `kaku remote` and `kaku doctor` to read its own state again. Look up the test window by the test instance's pid and capture nothing when that yields no id, since an empty `-l` once captured the maintainer's own window. Native fullscreen does not capture reliably this way; ask the maintainer for that screenshot instead. Four traps from the 2026-10-19 site captures: do not pass `--args start` to `open`, since AppKit hands bare arguments to `application_open_files`, the first pane runs `start ; exit` and the window closes before it is seen; drop `config.remote` from a copied `kaku.lua`, or after a config reload the test instance can attach to the maintainer's mux and `send-text` lands in their pane; `yazi` in a test pane is the shell wrapper, which, like Cmd+Shift+Y, rewrites the maintainer's `~/.config/yazi/theme.toml` from the test config's `color_scheme`, so run the Yazi binary itself with `YAZI_CONFIG_HOME` on a scratch copy; and size or move the test window with one System Events expression per action (`set size of window 1 of (first process whose unix id is <pid>)`), since a process reference kept in a variable resolves by name and reached the maintainer's `kaku-gui` window. Check the result per pid with `CGWindowListCopyWindowInfo`. For a remote capture, the scratch `HOME` and `TMPDIR` are both required: pairing state, the identity and `remote.lock` live under `$HOME/Library/Application Support/kaku` and the relay files under `$TMPDIR`, so only both keep the maintainer's remote untouched; end with `kaku remote off` under the same environment. In Safari, address a window you opened by its AppleScript `id`, never `front window`, which on 2026-10-19 resolved to the maintainer's Start Page window and resized it; editing the address bar opens history and Favorites over the page, so capture with the field unfocused (Tab moves focus into the page).

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

- Single-crate behavior belongs in that crate's `AGENTS.md`.
- Cross-crate behavior should update every affected subsystem guide.
- Build, CI, release, and maintainer workflow changes belong in this root file.
- Shared agent instructions belong in tracked docs. Personal overrides belong in ignored local files.
- One-off review reports, scorecards, and diagnostic snapshots are evidence, not durable project docs. Extract stable rules or verification gates into `AGENTS.md`, `CLAUDE.md`, subsystem guides, scripts, or tests, then remove the transient report.
- Do not hide user-visible behavior changes inside maintainability or cleanup patches. New UI, config fields, defaults, or workflow permissions should be split into their own change unless the maintainer explicitly approved that scope.
