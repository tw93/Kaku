#!/usr/bin/env python3
"""Name the verification a change needs from the paths it touches, never from who wrote it.

    python3 scripts/risk_tier.py [path ...]

With no paths it reads the change not yet upstream: commits since the merge
base with @{upstream} (origin/main when there is none) plus staged, unstaged
and untracked files. Prints one line per tier the change reaches, highest
first, and nothing when the default checks for the change type (AGENTS.md
Verification) are enough. Advice only: it always exits 0 and never writes a
file, since most of the gates it names need a built app, a release run or the
maintainer, which a script cannot prove ran.
"""

from __future__ import annotations

import datetime as dt
import fnmatch
import subprocess
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Highest first. A path takes every tier it matches. fnmatch's `*` also
# matches `/`, so a directory pattern covers its whole subtree.
TIERS = [
    ("release",
     "make fmt && make check && make test, then make app; Build Validation green on HEAD; "
     "the pre-release smoke checklist in .agents/skills/release/SKILL.md; the maintainer's review of the diff", [
         "scripts/release.sh", "scripts/notarize.sh", "scripts/prep_release.sh", "scripts/nightly.sh",
         "scripts/build.sh", "scripts/check_release_notes.sh", "scripts/check_release_config.sh",
         "scripts/check_config_release_readiness.sh", "kaku/Cargo.toml", "kaku-gui/Cargo.toml",
         "rust-toolchain.toml", ".github/RELEASE_NOTES.md",
     ]),
    ("update",
     "sweep every update entry path (menu Check for Updates, Restart to Update, toast, CLI direct ZIP, brew cask) "
     "and keep user_facing_update_events_route_through_confirm and brew_and_direct_paths_confirm_before_replace green; "
     "update a built app from the previous release by hand", [
         "scripts/update_helper.sh", "kaku/src/update.rs", "kaku-gui/src/update.rs",
         "crates/wezterm-toast-notification/src/macos.rs",
     ]),
    ("workflow",
     "watch the pushed run with gh run watch <id> --exit-status; keep macOS jobs across checks.yml and build-validation.yml at five or fewer", [
         ".github/workflows/*",
     ]),
    ("config",
     "./scripts/check_config_release_readiness.sh and ./scripts/check_release_config.sh; migration code only "
     "for keys the previous release had (docs/config-versions.md)", [
         "assets/shell-integration/config_version.txt", "assets/shell-integration/config_update_highlights.tsv",
         "assets/shell-integration/check_config_version.sh", "assets/macos/Kaku.app/Contents/Resources/kaku.lua",
     ]),
    ("dotfiles",
     "the affected assets/shell-integration/tests/*_smoke.sh, then the list checks.yml runs; shellcheck "
     "--severity=error; write_through under a real write failure in every copy; reset keeps user-owned lines", [
         "assets/shell-integration/setup_zsh.sh", "assets/shell-integration/setup_fish.sh",
         "assets/shell-integration/install_cli_tools.sh", "kaku/src/init.rs", "kaku/src/reset.rs",
         "kaku/src/doctor.rs",
     ]),
]


def git(*args: str) -> list[str]:
    out = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    return out.stdout.splitlines() if out.returncode == 0 else []


def pending_paths() -> list[str]:
    base = (git("merge-base", "HEAD", "@{upstream}") or git("merge-base", "HEAD", "origin/main") or [""])[0]
    paths = set(git("diff", "--name-only", base)) if base else set()
    paths.update(git("ls-files", "--others", "--exclude-standard"))
    return sorted(p for p in paths if p)


def tiers_for(paths: list[str]) -> list[tuple[str, str, list[str]]]:
    hits = []
    for name, gates, patterns in TIERS:
        matched = [p for p in paths if any(fnmatch.fnmatchcase(p, pattern) for pattern in patterns)]
        if matched:
            hits.append((name, gates, matched))
    return hits


# Circuit breaker: once users met BREAKER_ROWS defects within BREAKER_DAYS (docs/incidents.md, by Reported date),
# every change that reaches a tier also needs the full gate, and the layers those defects slipped past are named
# so they are tightened first. It clears on its own as the rows age out; a future date is never counted.
BREAKER_DAYS, BREAKER_ROWS = 7, 5
BREAKER_GATE = '`make check` and the whole `make test` instead of single tests, and the maintainer reading each listed fix before the next release'
LEDGER = ROOT / 'docs/incidents.md'


def recent_incidents(today: dt.date | None = None, ledger: Path | None = None) -> list[tuple[str, str]]:
    """(ID, layer) of every ledger row reported within the last BREAKER_DAYS days."""
    today = today or dt.date.today()
    try:
        text = (ledger or LEDGER).read_text("utf-8")
    except OSError:
        return []
    header: list[str] | None = None
    found = []
    for line in text.split("\n## Incidents", 1)[-1].splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if header is None:
            header = cells
            continue
        if set("".join(cells)) <= set("-: "):
            continue
        row = dict(zip(header, cells))
        try:
            age = (today - dt.date.fromisoformat(row.get("Reported", ""))).days
        except ValueError:
            continue
        if 0 <= age < BREAKER_DAYS:
            found.append((row.get("ID", ""), row.get("Layer", "")))
    return found


def breaker_line(recent: list[tuple[str, str]]) -> str | None:
    if len(recent) < BREAKER_ROWS:
        return None
    ids = ", ".join(ident for ident, _ in recent)
    layers = ", ".join(f"{layer} {count}" for layer, count in Counter(layer for _, layer in recent).most_common())
    return (f"risk: breaker ({len(recent)} incidents in the last {BREAKER_DAYS} days: {ids}): until the rate drops "
            f"every tiered change needs {BREAKER_GATE}; tighten first the layers they slipped past ({layers})")


def main(argv: list[str]) -> int:
    paths = argv or pending_paths()
    hits = tiers_for(paths)
    for name, gates, matched in hits:
        more = f" and {len(matched) - 1} more" if len(matched) > 1 else ""
        print(f"risk: {name} ({matched[0]}{more}) needs {gates}")
    breaker = breaker_line(recent_incidents())
    if hits and breaker:
        print(breaker)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
