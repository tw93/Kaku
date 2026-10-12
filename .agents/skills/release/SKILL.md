---
name: release
description: "Run the Kaku macOS release flow end-to-end: preflight, build, notarize, tag, upload to GitHub Releases, dispatch the Homebrew tap, or publish the rolling Nightly preview package. Source of truth is scripts/release.sh and scripts/nightly.sh; this skill records release prerequisites and recovery hints without embedding private machine setup."
when_to_use: "发布, 出版本, 打 tag, release Kaku, ship Kaku, V0.x.x, kaku release, prepare release, dry-run release, resume release, notarize, homebrew tap, nightly, Nightly 包, 更新 nightly, 预览包"
---

# Kaku Release Runbook

`scripts/release.sh` is authoritative. This skill captures the release prerequisites and the order Tang and the agent rely on each release. Exact credential locations and one-time recovery commands live in the ignored `.claude/release.local.md`, never in this tracked file.

Before any stable or Nightly release, run the non-secret checks under **Verify**. The release dry-run is the authoritative credential gate: it accepts either the rcodesign path or the notarytool fallback, including environment overrides. If it passes, do not load machine-specific recovery context. If a signing identity or every notarization path is missing or stale, then check for and read `.claude/release.local.md`; on Tang's release machine it is the required recovery layer. If the local runbook is absent, stop and ask for it instead of guessing a path or replacing credentials.

## TL;DR

```bash
./scripts/release.sh --dry-run                              # preflight only
KAKU_ASC_API_KEY_PATH=/dev/null ./scripts/release.sh        # full release on slow network (recommended)
./scripts/release.sh                                        # full release on fast direct AWS path
./scripts/nightly.sh                                        # rolling Nightly DMG for fixes already on main
```

`release.sh` must run on `main`, with a clean tree, in sync with `origin/main`. `nightly.sh` has no branch or clean-tree gate; it only refuses to publish when HEAD is not yet on `tw93/Kaku`. Both auto-detect the signing identity and read notarization creds from the login keychain.

The `KAKU_ASC_API_KEY_PATH=/dev/null` prefix skips rcodesign and forces the notarytool fallback. Use it whenever direct connectivity to `notary-submissions-prod.s3.amazonaws.com` is slow or proxied (see "rcodesign S3 connect timeout" under Common blockers). The env var only redirects the path inside `notarize.sh`. A stored profile name does not prove that the corresponding notarytool login exists; verify the profile or use the API-key recovery below.

## Nightly preview package

`scripts/nightly.sh` is authoritative for the rolling `nightly` GitHub prerelease. It builds a release-grade universal app, signs, notarizes, staples, copies `dist/Kaku.dmg` to `dist/Kaku-nightly.dmg`, and recreates the `nightly` release. It does not bump versions, create a `V*` tag, update Homebrew, or refresh any stable release surface.

Rules:

1. Only publish Nightly from the commit users should test. The script checks that HEAD exists on `tw93/Kaku` before deleting the old nightly release.
2. Do not tell reporters to try Nightly until `./scripts/nightly.sh` has completed and `gh release view nightly -R tw93/Kaku --json tagName,targetCommitish,publishedAt,assets,url` points at the intended commit with `Kaku-nightly.dmg`.
3. Do not use `--upload-only` unless the existing `dist/Kaku.dmg` is already known to be a signed, notarized, stapled build from the intended commit.
4. Keep public wording precise: "available in the latest Nightly" only after the check above; otherwise say "fixed on main and will be in the next Nightly or release."

## Credential prerequisites

The ignored `.claude/release.local.md` is the canonical map from Tang's local backup paths to these script inputs. The tracked contract is limited to what the scripts consume: either `KAKU_ASC_API_KEY_PATH` or the `kaku-asc-api-key-path` login-keychain item for rcodesign, and either `KAKU_NOTARYTOOL_PROFILE` or the `kaku-notarytool-profile` login-keychain item for the fallback. Do not delete or rewrite the local recovery map merely because its paths are machine-specific.

### Preferred: rcodesign + ASC API key

`rcodesign` is preferred on macOS 26+ because notarytool can SIGBUS. The scripts also accept the notarytool fallback when its prerequisites are available; use the release dry-run to check eligibility and the actual notarization result to verify success. It is not in homebrew core.

```bash
cargo install apple-codesign         # binary: rcodesign, lands in ~/.cargo/bin
```

The private JSON supplied through the environment or keychain path must use rcodesign's native `issuer_id`, `key_id`, and `private_key` format. Do not record its location in tracked docs.

### Fallback: notarytool profile

Create the notarytool profile with the maintainer's private Apple ID, team ID, and app-specific password outside the repository. Pass its profile name through `KAKU_NOTARYTOOL_PROFILE` or the expected login-keychain item; never paste the source values or the import command into a tracked runbook.

### Verify

```bash
security find-identity -v -p codesigning | grep 'Developer ID Application'
gh auth status
./scripts/release.sh --dry-run
```

When the dry-run reports a notarization failure, use `command -v rcodesign`, `security find-generic-password -s 'kaku-asc-api-key-path'`, and `security find-generic-password -s 'kaku-notarytool-profile'` as separate diagnostics. They are not an AND gate: one complete notarization backend is sufficient.

## Pre-release content checklist

Before invoking `release.sh`, confirm in this order:

1. `kaku/Cargo.toml` and `kaku-gui/Cargo.toml` versions match (e.g. `0.10.0`).
2. `assets/shell-integration/config_version.txt` is the intended schema version, with highlight rows in the matching docs.
3. `.github/RELEASE_NOTES.md` first heading is `# V<version> <suffix>` (uppercase V, used as the GitHub Release title).
4. Both English `Changelog` and Chinese `更新日志` sections in `RELEASE_NOTES.md` cover the same items. Register: one full stop per item, declarative middle register, neither literary nor conversational; the item count follows the number of distinct user-visible outcomes (the previous release's count is history, not a quota), and length is measured against the previous release before publishing.
5. Any pending fixes are committed and pushed to `origin/main`.

Use `./scripts/prep_release.sh <bump>` to draft the version bump and notes when starting from an older tag. Tang typically edits the resulting `.github/RELEASE_NOTES.md` by hand to apply the announcement-writing style (community first, 2 to 4 highlights, user-experience framing).

## Pre-release smoke checklist (runtime-only hotspots)

These are the areas that produce the most post-release bug reports and that CI cannot see (visual layout, native AppKit, shell-in-user-env). Run them by hand in the built `dist/Kaku.app` before tagging. Automated coverage already exists for the testable slices: tab width-budget + hover hit-testing have unit tests in `kaku-gui/src/tabbar.rs`, and `local`-outside-function / shell syntax is gated by shellcheck + the setup_zsh smoke. The list below is what still needs a human.

1. **macOS window** (#408, #414): on first launch, a single click on the title bar / top inset must not maximize the window; drag the window while it fills the desktop; flip system light/dark and confirm all windows refresh; enter and exit fullscreen cleanly.
2. **Tab bar** (#409, #435, #439, #443, #445, #447): check both `tab_bar_at_bottom` true/false; open enough tabs to overflow a narrow window and confirm titles truncate but every tab stays clickable; rename a tab, then click another to switch and confirm no position scramble; confirm a tmux/shell status prompt renders in the bar without clipping. Exercise the status dot through idle, running, pending or attention, and cleared states on active and inactive tabs, with the tab bar at both the top and bottom; idle must not leave a dot behind.
3. **Shell setup** (#420, #432, #441, #450): from a clean `HOME`, run `kaku init` and confirm z / syntax-highlight / autosuggestions are active in a fresh shell; run `kaku init --update-only` and confirm it exits clean; open a new shell and confirm `~/.config/kaku/zsh/kaku.zsh` sources with no error.
4. **AI chat** (#418, #431): run `kaku chat`, quit, then run it again in the same window and confirm it reopens.
5. **Render timing / stale drawable** (#452, #458): on the bundled WebGpu backend, sleep the Mac then wake it and confirm the window repaints instead of freezing on the old frame while keystrokes still reach the shell; connect or disconnect an external display and confirm no frozen frame or geometry jump; open a new window straight into fullscreen and resize it, confirming it fills without a stale first frame.
6. **Hyperlinks** (#547): Cmd-hover a URL that the terminal soft-wraps and confirm both rows target the full URL. Then print a URL exactly as wide as the terminal, followed by `\r\n` and unindented unrelated text; the target must exclude that text. Terminal and surface tests cover the grouping; check the built app hover target too.

7. **Mouse recovery** (#551): in both zsh and fish, run `printf '\033[?1003;1006h'` and return to the prompt; moving the mouse must not insert escape sequences. Also keep a mouse-enabled TUI running and confirm mouse input still works until it exits.

8. **Settings lifecycle and custom configs** (#545): start the app with a minimal standalone `--config-file`, switch between Kaku Light and Kaku Dark, and confirm Settings edits that file and the main window follows. Save and exit Settings repeatedly; no destroyed-window repaint panic should appear.

9. **Tab bar pointer paths** (V0.22.0): with 2+ tabs and the bar at the bottom, tap just above a tab and confirm it switches; middle-click to paste on the last terminal row and confirm no tab closes; right-click an inactive tab, choose Close Tab several times and confirm the app stays up and closes that tab while you stay on yours.
10. **Split panes with dimming** (#562): set `inactive_pane_hsb = { brightness = 0.7 }`, split top|bottom with the tab bar at top and at bottom, focus each pane, and confirm the dimmed background stops exactly at the line with no undimmed strip on the right or bottom edge; split into three panes and confirm two dimmed neighbours still show a faint line; drag the line and confirm it does not jump a row on grab.

When a release fixes a bug outside this list, add the reproduction here so the next release re-checks it.

## Verification commands

| Scope | Command |
|---|---|
| Format | `make fmt-check` |
| Compile | `make check` |
| Tests | `make test` |
| Release notes version match | `./scripts/check_release_notes.sh` |
| Config schema versioning | `./scripts/check_release_config.sh` |
| Config release readiness | `./scripts/check_config_release_readiness.sh` |
| Full preflight | `./scripts/release.sh --dry-run` |

`release.sh --dry-run` stops after preflight and does not run source checks. The normal release reuses successful CI Checks for the current HEAD; otherwise `stage:checks` runs the local checks below, as described in `AGENTS.md`. A passing dry-run alone is not source-check evidence.

## Stage map

`./scripts/release.sh` runs these stages in order. Each is timed and labeled `[stage:<name>]`.

1. **preflight**: clean git on `main` in sync with `origin/main`, version consistency, gh auth, release notes, config, latest `Build Validation` workflow run on `main` green (an in-progress run only warns), profile, signing identity, notarization creds.
2. **stage:checks**: reuse successful CI Checks on the current HEAD, or run `make fmt-check && make check && make test` locally. `RUN_CLIPPY=1` adds local clippy in either case. `SKIP_TESTS=1` skips local tests (avoid).
3. **stage:build**: `./scripts/build.sh`, `PROFILE=release-opt`, `BUILD_ARCH=universal`. Output: `dist/Kaku.app`, `dist/Kaku.dmg`, `dist/kaku_for_update.zip`, `dist/kaku_for_update.zip.sha256`.
4. **stage:notarize**: `./scripts/notarize.sh`. Tries rcodesign first; falls back to notarytool if rcodesign fails and a notarytool profile exists.
5. **stage:tag**: `git tag -a V<version> -m 'Release V<version>'` then `git push origin V<version>`. Idempotent: reuses an existing tag at HEAD instead of dying.
6. **stage:upload**: `gh release create V<version>` (or `gh release edit` if it already exists) with the dmg, zip, and sha256. Title is taken from the first `# ` line of `RELEASE_NOTES.md`.
7. **stage:homebrew-tap**: `repository_dispatch` (`kaku_release_published`) to `tw93/homebrew-tap` triggering `bump.yml`. Polls `Casks/kakuku.rb` through the GitHub contents API for the new version (default 12 attempts, 15s apart). The official `homebrew/cask` token is `kaku` and is bumped by Homebrew livecheck from the GitHub release; this stage only updates the personal tap for existing `tw93/tap/kakuku` installs.

## Resume after failure

`release.sh` accepts these flags to skip already-completed stages:

| Flag | Skips | Use when |
|---|---|---|
| `--notarize-only` | build | Build succeeded; notarization failed or was interrupted. |
| `--upload-only` | build, notarize | Notarized `dist/Kaku.app` and `Kaku.dmg` exist on disk; only need to tag + upload + tap. |
| `--tap-only` | build, notarize, upload | GitHub release exists; only the Homebrew tap dispatch needs to rerun. |

Resume flags require the corresponding artifacts in `dist/` to still be present.

## Common blockers

- **`Local main is not synchronized with origin/main`**: push the pending commit. `release.sh` requires the tag to point at a commit that exists on origin.
- **`rcodesign` not found**: `cargo install apple-codesign`, then verify with `which rcodesign`. Do not try `brew install rcodesign`; it is not a core formula.
- **`No Developer ID Application certificate found`**: re-import the certificate from the maintainer-approved private backup through Keychain Access, using the password from the private runbook. Never add either location to this file.
- **`Notarization credentials not found`**: follow the ignored private setup runbook to provision one of the environment/keychain inputs described under Credential prerequisites, then rerun `./scripts/release.sh --dry-run`. The script otherwise prompts interactively, which fails in non-interactive shells.
- **rcodesign S3 connect timeout (3.1s)**: rcodesign uploads the dmg to `notary-submissions-prod.s3.amazonaws.com` via the AWS SDK for Rust, which has a hardcoded 3.1s connect timeout and does not honor `*_proxy` env vars. On networks where direct connect to AWS S3 is slow or proxied (typical for mainland China), it fails consistently with `s3 upload error: HTTP connect timeout occurred after 3.1s`. The error is independent of credentials. Fix: prefix the run with `KAKU_ASC_API_KEY_PATH=/dev/null` to make `notarize.sh` skip rcodesign and use notarytool directly. Both keychain entries should remain set.
- **`No Keychain password item found for profile`**: a profile-name entry alone does not establish a working login. An installed Apple `notarytool` can reuse the existing ASC `.p8` key with `--key`, `--key-id`, and `--issuer`, without creating a Keychain login. Resolve the inputs from the private runbook and keep their values out of displayed commands and logs. For repeated S3 upload stalls, try `--no-s3-acceleration`. Keep the signed artifact fixed while changing submission transport. After `Accepted`, retrieve the submission log and verify its SHA-256 against the submitted DMG, then run `./scripts/notarize.sh --staple-only` and `./scripts/nightly.sh --upload-only` from the same source commit. See [Apple's notarization API workflow](https://developer.apple.com/documentation/notaryapi/submitting-software-for-notarization-over-the-web).
- **Homebrew tap verifier timed out at 12/12**: the tap workflow commits `kakuku <version>` to `tw93/homebrew-tap` main on success; verify with `gh api repos/tw93/homebrew-tap/commits/main --jq .commit.message`. The verifier reads the cask through the GitHub contents API (`Accept: application/vnd.github.raw`), not the Fastly-cached `download_url`, so a timeout means the `bump.yml` run is slow or failed: open the run URL the script printed. If the tap commit is present the release is complete. Widen the window with `HOMEBREW_TAP_VERIFY_ATTEMPTS` / `HOMEBREW_TAP_VERIFY_SLEEP_SECONDS`, or rerun `./scripts/release.sh --tap-only`; `REQUIRE_HOMEBREW_TAP_UPDATE=0` only downgrades the failure to a warning.
- **`rcodesign notarization failed` right after the log shows `"status": "Accepted"`**: Apple accepted; the step died stapling. rcodesign cannot staple an unsigned DMG, which is what `build.sh` produces, and the script used to pass `--staple` (removed 2026-09-10). Do not touch credentials. Recover with `./scripts/notarize.sh --staple-only` then `./scripts/nightly.sh --upload-only`.
- **`codesign: A timestamp was expected but was not found`** (or `timestamp service is not available`): the timestamp request failed during signing; this message alone does not establish a certificate fault or identify the network cause. `build.sh` already retries timestamp failures before aborting. After those retries fail, stop the release, perform read-only network diagnosis, and report the blocked signing step. Resume only after the maintainer resolves the network problem; do not rerun the build or change proxy settings to retry the same failure.
- **`codesign: code object is not signed at all, In subcomponent: .../MacOS/k`** while signing `kaku-gui`: `kaku-gui` is the bundle's main executable and codesign validates its siblings as nested code, so every other executable in `Contents/MacOS` must already be signed. `build.sh` signs `kaku` and `k` before `kaku-gui` since 2026-09-10; if a new helper binary is added to the bundle, add it to that loop before `kaku-gui`.
- **`stapler` fails with CloudKit timeout, exit code 68**: the log reads `The staple and validate action failed! Error 68` right after `Notarization accepted`. Notarization already succeeded; only ticket retrieval from `api.apple-cloudkit.com` timed out. Same family as the rcodesign S3 timeout, but intermittent rather than systematic, so do NOT re-notarize. Check reachability (`curl -s -o /dev/null -w '%{http_code}' --max-time 20 https://api.apple-cloudkit.com/`; a 400 means reachable), then run `./scripts/notarize.sh --staple-only` and resume with `./scripts/release.sh --upload-only`. The staple-only path regenerates `kaku_for_update.zip` and its sha256 on purpose: attaching the ticket changes the app, so the in-app update archive must be rebuilt after stapling, never before.
- **Tag already exists on origin at a different SHA**: do not force-push tags. Pick the next patch number, bump versions, and start over.

## Environment variable overrides

| Variable | Default | Purpose |
|---|---|---|
| `KAKU_SIGNING_IDENTITY` | auto-detect | Override the Developer ID Application identity. |
| `KAKU_ASC_API_KEY_PATH` | keychain item `kaku-asc-api-key-path` | Path to rcodesign ASC API key JSON. |
| `KAKU_NOTARYTOOL_PROFILE` | keychain item `kaku-notarytool-profile` | notarytool keychain profile name. |
| `HOMEBREW_TAP_TOKEN` | `gh auth token` | GitHub token for tap dispatch. |
| `REQUIRE_HOMEBREW_TAP_UPDATE` | `1` | Set to `0` to allow release to succeed when tap dispatch fails. |
| `HOMEBREW_TAP_VERIFY_ATTEMPTS` | `12` | Polls of the tap cask before the verifier gives up. |
| `HOMEBREW_TAP_VERIFY_SLEEP_SECONDS` | `15` | Seconds between tap cask polls. |
| `RUN_CLIPPY` | `0` | Set to `1` to run `cargo clippy` during stage:checks. |
| `SKIP_TESTS` | `0` | Set to `1` to skip `make test` during stage:checks. |
| `OUT_DIR` | `<repo>/dist` | Override artifact output directory. |
| `PROFILE` | `release-opt` | Cargo profile. Only `release` and `release-opt` are accepted. |
| `BUILD_ARCH` | `universal` | Passed through to `build.sh`. |

## After release

- GitHub Release URL: `https://github.com/tw93/Kaku/releases/tag/V<version>`.
- Official Homebrew cask users get the new version after Homebrew livecheck/bump; verify with `brew update && brew info --cask kaku`. Personal-tap users get it once the tap workflow finishes; verify with `brew info --cask tw93/tap/kakuku`.
- In-app updates (`kaku-gui/src/update.rs`, Kaku's own updater, not Sparkle) download `kaku_for_update.zip` and its `.sha256` from the latest GitHub Release.
- Add the six positive reactions to the new release; this is part of shipping. Skip `-1` and `confused`.

  ```bash
  id=$(gh api repos/tw93/Kaku/releases/tags/V<version> --jq .id)
  for c in +1 laugh heart hooray rocket eyes; do
      gh api -X POST "repos/tw93/Kaku/releases/$id/reactions" -f content="$c"
  done
  ```

For the announcement post (X / WeChat): community first, 2-4 highlights, user-experience framing, one opinionated sentence. The release notes file is a different artifact; do not paste it as the announcement.
