#!/usr/bin/env python3
"""Fail when Kaku's public facts drift from its latest GitHub release."""

from __future__ import annotations

import json
import os
import re
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPOSITORY = "tw93/Kaku"


def latest_release_version() -> str:
    override = os.environ.get("KAKU_LATEST_RELEASE")
    if override:
        return override.lstrip("Vv")

    request = urllib.request.Request(
        f"https://api.github.com/repos/{REPOSITORY}/releases/latest",
        headers={"Accept": "application/vnd.github+json", "User-Agent": "kaku-site-fact-check"},
    )
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=15) as response:
        payload = json.load(response)
    return payload["tag_name"].lstrip("Vv")


def public_text() -> str:
    paths = [
        *ROOT.glob("*.html"),
        *ROOT.glob("*.md"),
        *ROOT.glob("*.txt"),
        *ROOT.glob("*.json"),
        *ROOT.glob("docs/**/*"),
        *ROOT.glob("zh/**/*"),
    ]
    return "\n".join(
        path.read_text(encoding="utf-8")
        for path in paths
        if path.is_file() and path.suffix in {".html", ".md", ".txt", ".json"}
    )


def main() -> int:
    agent = json.loads((ROOT / "agent.json").read_text(encoding="utf-8"))
    site_version = agent["product"]["version"]
    release_version = latest_release_version()
    if site_version != release_version:
        raise SystemExit(
            f"agent.json publishes {site_version}, but the latest GitHub release is {release_version}"
        )

    llms_full = (ROOT / "llms-full.txt").read_text(encoding="utf-8")
    if f"Latest version: {site_version}" not in llms_full:
        raise SystemExit("llms-full.txt does not publish the agent.json version")
    if "Pane input broadcast was removed in v0.19.0 for safety" not in llms_full:
        raise SystemExit("llms-full.txt must state that pane input broadcast was removed")

    removed = {item["name"]: item for item in agent.get("removedFeatures", [])}
    if removed.get("pane input broadcast", {}).get("removedIn") != "0.19.0":
        raise SystemExit("agent.json must keep the pane input broadcast removal fact")

    text = public_text()
    active_broadcast_claims = [
        r"broadcast input across panes",
        r"Broadcast input to (?:current|all)",
        r"广播输入到",
        r"输入广播到",
    ]
    for pattern in active_broadcast_claims:
        if re.search(pattern, text, re.IGNORECASE):
            raise SystemExit(f"removed pane input broadcast is still advertised: {pattern}")

    legacy = "https://yobi.tw93.fun/projects/kaku"
    if legacy in text:
        raise SystemExit(f"legacy human product URL remains: {legacy}")

    print(f"public facts match Kaku {site_version}; removed features are not advertised")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
