#!/usr/bin/env python3
"""Fail when sitemap locs or lastmod drift from the live site contract.

Vercel serves the site with trailingSlash=false, so only the origin URL may
end with a slash. lastmod must not be older than the newest committed change
to public HTML or agent-facing indexes. Shallow clones skip the date check,
matching mole's site-lastmod-ok exemption.
"""

from __future__ import annotations

import re
import subprocess
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORIGIN = "https://kaku.fun/"
CONTENT_PATHS = [
    "index.html",
    "zh/index.html",
    "about.html",
    "compare.html",
    "contact.html",
    "privacy.html",
    "roadmap.html",
    "docs",
    "zh/docs",
    "zh/about.html",
    "zh/compare.html",
    "zh/contact.html",
    "zh/privacy.html",
    "zh/roadmap.html",
    "llms.txt",
    "llms-full.txt",
    "docs/llms.txt",
    "zh/llms.txt",
    "agent.json",
]


def xml_pairs(path: Path, loc_tag: str, wrapper: str) -> list[tuple[str, str]]:
    text = path.read_text(encoding="utf-8")
    blocks = re.findall(rf"<{wrapper}>(.*?)</{wrapper}>", text, re.S)
    rows = []
    for block in blocks:
        loc = re.search(rf"<{loc_tag}>\s*([^<\s]+)\s*</{loc_tag}>", block)
        lastmod = re.search(r"<lastmod>\s*([0-9]{4}-[0-9]{2}-[0-9]{2})\s*</lastmod>", block)
        if not loc or not lastmod:
            raise SystemExit(f"{path.name} has a {wrapper} without {loc_tag}/lastmod")
        rows.append((loc.group(1), lastmod.group(1)))
    if not rows:
        raise SystemExit(f"{path.name} has no {wrapper} entries")
    return rows


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def newest_content_date() -> date | None:
    shallow = git("rev-parse", "--is-shallow-repository")
    if shallow.returncode == 0 and shallow.stdout.strip() == "true":
        print("sitemap-lastmod-skipped shallow-clone")
        return None

    log = git("log", "-1", "--format=%cs", "--", *CONTENT_PATHS)
    if log.returncode != 0 or not log.stdout.strip():
        print("sitemap-lastmod-skipped (no content history in this checkout)")
        return None
    return date.fromisoformat(log.stdout.strip())


def main() -> int:
    robots = (ROOT / "robots.txt").read_text(encoding="utf-8")
    if "https://kaku.fun/sitemap.xml" not in robots:
        raise SystemExit("robots.txt must announce https://kaku.fun/sitemap.xml")

    urls = xml_pairs(ROOT / "sitemap.xml", "loc", "url")
    for loc, lastmod in urls:
        if loc == ORIGIN:
            continue
        if loc.endswith("/"):
            raise SystemExit(f"sitemap loc {loc} has a trailing slash; Vercel 308s it")
        if not loc.startswith("https://kaku.fun/"):
            raise SystemExit(f"sitemap loc is outside kaku.fun: {loc}")
        date.fromisoformat(lastmod)

    compare_locs = {loc for loc, _ in urls}
    for required in ("https://kaku.fun/compare", "https://kaku.fun/zh/compare"):
        if required not in compare_locs:
            raise SystemExit(f"sitemap.xml is missing {required}")

    newest = newest_content_date()
    if newest is not None:
        oldest = min(date.fromisoformat(lastmod) for _, lastmod in urls)
        if oldest < newest:
            raise SystemExit(
                f"sitemap lastmod {oldest.isoformat()} is older than content {newest.isoformat()}"
            )

    index_rows = xml_pairs(ROOT / "sitemap-index.xml", "loc", "sitemap")
    if index_rows[0][0] != "https://kaku.fun/sitemap.xml":
        raise SystemExit("sitemap-index.xml must point at https://kaku.fun/sitemap.xml")
    sitemap_newest = max(date.fromisoformat(lastmod) for _, lastmod in urls)
    index_lastmod = date.fromisoformat(index_rows[0][1])
    if index_lastmod < sitemap_newest:
        raise SystemExit("sitemap-index.xml lastmod is older than sitemap.xml")

    schema_rows = xml_pairs(ROOT / "schemamap.xml", "loc", "url")
    if newest is not None:
        schema_oldest = min(date.fromisoformat(lastmod) for _, lastmod in schema_rows)
        if schema_oldest < newest:
            raise SystemExit(
                f"schemamap lastmod {schema_oldest.isoformat()} is older than content {newest.isoformat()}"
            )

    print(f"sitemap locs and lastmod match the site contract ({len(urls)} urls)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
