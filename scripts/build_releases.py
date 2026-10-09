#!/usr/bin/env python3
"""Generate the stable-release feed, changelog pages, and releases.json.

Nightly and other prereleases stay out. The source is the public GitHub
releases for tw93/Kaku. Notes before the current heading style use
"What's New" or a level-2 "Changelog"; both are accepted.

    python3 scripts/build_releases.py
    python3 scripts/build_releases.py --check
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import subprocess
import sys
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime
from email.utils import format_datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPOSITORY = "tw93/Kaku"
SITE = "https://kaku.fun"
FEED_URL = f"{SITE}/feed.xml"
DATA_URL = f"{SITE}/feeds/releases.json"
CHANGELOG = f"{SITE}/changelog"
CHANGELOG_ZH = f"{SITE}/zh/changelog"
TAG = re.compile(r"^V\d+\.\d+\.\d+$")
ITEM = re.compile(r"^(\d+)\.\s+\*\*(.+?)\*\*\s*[:：]\s*(.*)$")
FEED_LINK = (
    '<link rel="alternate" type="application/rss+xml" '
    'title="Kaku releases" href="https://kaku.fun/feed.xml">'
)
FOOTER_OLD = (
    'href="https://github.com/tw93/Kaku/releases" target="_blank" '
    'rel="noopener noreferrer">Changelog</a>'
)
ROADMAP_EN_OLD = (
    'Release notes are on <a href="https://github.com/tw93/Kaku/releases" '
    'target="_blank" rel="noopener noreferrer">GitHub Releases</a>'
)
ROADMAP_EN_NEW = 'Release notes are on <a href="/changelog">the changelog</a>'
ROADMAP_ZH_OLD = (
    '发布说明在 <a href="https://github.com/tw93/Kaku/releases" '
    'target="_blank" rel="noopener noreferrer">GitHub Releases</a>'
)
ROADMAP_ZH_NEW = '发布说明在 <a href="/zh/changelog">更新日志</a>'
SKIP_HTML = {"kaku-site-overview.html"}


def clean(text: str) -> str:
    text = text.replace(" \u2014 ", ", ")
    text = text.replace("\u2014", ", ")
    return re.sub(r" {2,}", " ", text).strip()


def token() -> str | None:
    for key in ("GITHUB_TOKEN", "GH_TOKEN"):
        value = os.environ.get(key)
        if value:
            return value
    try:
        out = subprocess.check_output(
            ["gh", "auth", "token"], text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return None
    return out or None


def fetch_releases() -> list[dict]:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "kaku-site-releases",
    }
    auth = token()
    if auth:
        headers["Authorization"] = f"Bearer {auth}"
    releases: list[dict] = []
    page = 1
    while True:
        request = urllib.request.Request(
            f"https://api.github.com/repos/{REPOSITORY}/releases?per_page=100&page={page}",
            headers=headers,
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            batch = json.load(response)
        if not batch:
            break
        releases.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    return releases


def heading_kind(title: str, version: str) -> str | None:
    lowered = title.lower()
    if lowered in {"changelog", "what's new"}:
        return "en"
    if title == "更新日志":
        return "zh"
    if version not in lowered and version not in title:
        return None
    if lowered.startswith("what's new") or lowered.startswith("changelog"):
        return "en"
    if title.startswith("更新日志"):
        return "zh"
    return None


def section(body: str, kind: str, version: str) -> str | None:
    marks = list(re.finditer(r"^#{2,3} +(.+?)\s*$", body, re.M))
    for index, mark in enumerate(marks):
        if heading_kind(mark.group(1).strip(), version) != kind:
            continue
        end = marks[index + 1].start() if index + 1 < len(marks) else len(body)
        return body[mark.end() : end]
    return None


def parse_section(text: str | None) -> tuple[list[dict], list[str], list[str]]:
    items: list[dict] = []
    thanks: list[str] = []
    notes: list[str] = []
    if not text:
        return items, thanks, notes
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line == "---" or line.startswith(">") or line.startswith("<"):
            continue
        if raw[:1].isspace() and items:
            items[-1]["text"] = clean(items[-1]["text"] + " " + line)
            continue
        matched = ITEM.match(line)
        if matched:
            items.append({"label": clean(matched.group(2)), "text": clean(matched.group(3))})
            continue
        plain = re.match(r"^(\d+)\.\s+(.+)$", line)
        if plain:
            items.append({"text": clean(plain.group(2))})
            continue
        cleaned = clean(line)
        if cleaned.lower().startswith("special thanks"):
            thanks.append(cleaned)
            continue
        notes.append(cleaned)
    return items, thanks, notes


def codename(name: str, tag: str) -> str:
    version = tag[1:]
    for prefix in (f"{tag} ", f"v{version} ", f"V{version} "):
        if name.startswith(prefix):
            return name[len(prefix) :].strip()
    return ""


def dmg_url(release: dict) -> str | None:
    for asset in release.get("assets") or []:
        if asset.get("name") == "Kaku.dmg" and asset.get("browser_download_url"):
            return asset["browser_download_url"]
    return None


def stable_releases(payload: list[dict]) -> list[dict]:
    chosen = []
    for release in payload:
        tag = release.get("tag_name") or ""
        if release.get("draft") or release.get("prerelease") or not TAG.fullmatch(tag):
            continue
        body = release.get("body") or ""
        version = tag[1:]
        items, thanks_en, notes_en = parse_section(section(body, "en", version))
        items_zh, thanks_zh, notes_zh = parse_section(section(body, "zh", version))
        if not items:
            raise SystemExit(f"{tag} has no English changelog items")
        published = release.get("published_at") or release.get("created_at")
        if not published:
            raise SystemExit(f"{tag} has no published_at")
        name = clean(release.get("name") or tag)
        row = {
            "version": version,
            "tag": tag,
            "name": name,
            "codename": codename(name, tag),
            "publishedAt": published,
            "url": f"{CHANGELOG}#v{version}",
            "urlZh": f"{CHANGELOG_ZH}#v{version}",
            "github": f"https://github.com/{REPOSITORY}/releases/tag/{tag}",
            "items": items,
            "itemsZh": items_zh,
        }
        download = dmg_url(release)
        if download:
            row["download"] = download
        thanks = thanks_en or thanks_zh
        if thanks:
            row["thanks"] = " ".join(thanks)
        notes = notes_en or notes_zh
        if notes:
            row["notes"] = notes
        chosen.append(row)
    chosen.sort(key=lambda row: row["publishedAt"], reverse=True)
    if not chosen:
        raise SystemExit("no stable V* releases")
    if any("nightly" in row["tag"].lower() for row in chosen):
        raise SystemExit("nightly leaked into the stable release list")
    return chosen


def html_text(text: str) -> str:
    parts = re.split(r"`([^`]+)`", text)
    rendered = []
    for index, part in enumerate(parts):
        escaped = html.escape(part, quote=False)
        if index % 2:
            rendered.append(f"<code>{escaped}</code>")
        else:
            rendered.append(escaped)
    return "".join(rendered)


def item_html(items: list[dict], colon: str) -> str:
    gap = "" if colon == "：" else " "
    lines = ["<ol>"]
    for item in items:
        text = html_text(item["text"])
        label = item.get("label")
        if label:
            lines.append(f"<li><b>{html_text(label)}</b>{colon}{gap}{text}</li>")
        else:
            lines.append(f"<li>{text}</li>")
    lines.append("</ol>")
    return "\n".join(lines)


def release_html(row: dict, zh: bool) -> str:
    version = row["version"]
    date = row["publishedAt"][:10]
    items = row["itemsZh"] if zh and row["itemsZh"] else row["items"]
    colon = "：" if zh and row["itemsZh"] else ":"
    links = [f'<a href="{html.escape(row["github"])}">GitHub</a>']
    if row.get("download"):
        links.append(f'<a href="{html.escape(row["download"])}">DMG</a>')
    parts = [
        f'<h2 id="v{version}">{html.escape(row["name"])} <span class="release-date">{date}</span></h2>',
        item_html(items, colon),
        f'<p class="release-more">{" · ".join(links)}</p>',
    ]
    if row.get("thanks"):
        parts.append(f'<p class="release-more">{html_text(row["thanks"])}</p>')
    if row.get("notes"):
        parts.append(
            '<p class="release-more">'
            + " ".join(html_text(note) for note in row["notes"])
            + "</p>"
        )
    return "\n".join(parts)


def json_ld(title: str, description: str, url: str, language: str, home: str) -> str:
    payload = {
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "BreadcrumbList",
                "itemListElement": [
                    {"@type": "ListItem", "position": 1, "name": "Kaku", "item": home},
                    {"@type": "ListItem", "position": 2, "name": title, "item": url},
                ],
            },
            {
                "@type": "TechArticle",
                "@id": url + "#article",
                "headline": title,
                "name": title,
                "description": description,
                "url": url,
                "inLanguage": language,
                "isPartOf": {"@id": "https://kaku.fun/#website"},
                "about": {"@id": "https://kaku.fun/#software"},
                "publisher": {"@id": "https://kaku.fun/#organization"},
                "author": {"@type": "Person", "name": "Tw93", "url": "https://tw93.fun"},
                "encoding": {
                    "@type": "MediaObject",
                    "encodingFormat": "text/markdown",
                    "contentUrl": url + ".md",
                },
            },
        ],
    }
    return json.dumps(payload, ensure_ascii=False, indent=2)


def render_page(donor: str, releases: list[dict], zh: bool) -> str:
    if zh:
        title = "更新日志"
        description = "Kaku 正式版更新，从新到旧。Nightly 仍在 GitHub，可以用 RSS 订阅。"
        url = CHANGELOG_ZH
        language = "zh-CN"
        home = "https://kaku.fun/zh/"
        intro = (
            'Kaku 正式版更新，从新到旧。Nightly 仍在 GitHub，可以用 <a href="/feed.xml">RSS</a> 订阅。'
        )
        page_en, page_zh = "/changelog", "/zh/changelog"
    else:
        title = "Changelog"
        description = "Stable Kaku releases, newest first. Nightly builds stay on GitHub. Subscribe with RSS."
        url = CHANGELOG
        language = "en"
        home = "https://kaku.fun/"
        intro = (
            'Stable Kaku releases, newest first. Nightly builds stay on GitHub. '
            'Subscribe with <a href="/feed.xml">RSS</a>.'
        )
        page_en, page_zh = "/changelog", "/zh/changelog"

    page = donor
    page = page.replace("https://kaku.fun/zh/roadmap.md", "https://kaku.fun/zh/changelog.md")
    page = page.replace("https://kaku.fun/zh/roadmap", "https://kaku.fun/zh/changelog")
    page = page.replace("https://kaku.fun/roadmap.md", "https://kaku.fun/changelog.md")
    page = page.replace("https://kaku.fun/roadmap", "https://kaku.fun/changelog")
    page = re.sub(r"<title>.*?</title>", f"<title>{title} · Kaku</title>", page, count=1)
    page = re.sub(
        r'<meta name="description" content="[^"]*">',
        f'<meta name="description" content="{html.escape(description, quote=True)}">',
        page,
        count=1,
    )
    script = (
        '<script type="application/ld+json">\n'
        + json_ld(title, description, url, language, home)
        + "\n  </script>"
    )
    page = re.sub(
        r'<script type="application/ld\+json">.*?</script>',
        script,
        page,
        count=1,
        flags=re.S,
    )

    def retarget(match: re.Match[str]) -> str:
        nav = match.group(0)
        nav = nav.replace('href="/zh/roadmap"', f'href="{page_zh}"')
        nav = nav.replace('href="/roadmap"', f'href="{page_en}"')
        return nav

    page = re.sub(r'<nav class="site-nav".*?</nav>', retarget, page, count=1, flags=re.S)
    blocks = ["<hr>\n" + release_html(row, zh) if index else release_html(row, zh) for index, row in enumerate(releases)]
    main = (
        '<main id="main-content"><section class="section"><div class="wide release-doc">\n'
        f"<h1>{title}</h1>\n"
        f'<p class="release-sub">{intro}</p>\n'
        + "\n".join(blocks)
        + "\n</div></section></main>"
    )
    page = re.sub(r'<main id="main-content">.*?</main>', main, page, count=1, flags=re.S)
    return page


def rss(releases: list[dict]) -> str:
    def cdata(text: str) -> str:
        return "<![CDATA[" + text.replace("]]>", "]]]]><![CDATA[>") + "]]>"

    items = []
    for row in releases:
        published = datetime.fromisoformat(row["publishedAt"].replace("Z", "+00:00"))
        body = ["<h2>Changelog</h2>", item_html(row["items"], ":")]
        if row["itemsZh"]:
            body += ["<h2>更新日志</h2>", item_html(row["itemsZh"], "：")]
        if row.get("thanks"):
            body.append(f"<p>{html_text(row['thanks'])}</p>")
        if row.get("notes"):
            body.append("<p>" + " ".join(html_text(note) for note in row["notes"]) + "</p>")
        items.append(
            "\n".join(
                [
                    "<item>",
                    f"<title>{html.escape(row['name'])}</title>",
                    f"<link>{html.escape(row['url'])}</link>",
                    f'<guid isPermaLink="true">{html.escape(row["url"])}</guid>',
                    f"<pubDate>{format_datetime(published)}</pubDate>",
                    f"<description>{cdata(''.join(body))}</description>",
                    "</item>",
                ]
            )
        )
    newest = datetime.fromisoformat(releases[0]["publishedAt"].replace("Z", "+00:00"))
    document = "\n".join(
        [
            '<?xml version="1.0" encoding="UTF-8"?>',
            '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">',
            "<channel>",
            "<title>Kaku releases</title>",
            f"<link>{CHANGELOG}</link>",
            "<description>Stable Kaku releases. Nightly builds are not included.</description>",
            "<language>en</language>",
            f"<lastBuildDate>{format_datetime(newest)}</lastBuildDate>",
            f'<atom:link href="{FEED_URL}" rel="self" type="application/rss+xml"/>',
            *items,
            "</channel>",
            "</rss>",
            "",
        ]
    )
    ET.fromstring(document)
    return document


def data_document(releases: list[dict]) -> str:
    payload = {
        "product": "Kaku",
        "feed": FEED_URL,
        "changelog": CHANGELOG,
        "changelogZh": CHANGELOG_ZH,
        "source": f"https://github.com/{REPOSITORY}/releases",
        "releases": releases,
    }
    return json.dumps(payload, ensure_ascii=False, indent=2) + "\n"


def changelog_href(path: Path) -> str:
    return "/zh/changelog" if "zh" in path.relative_to(ROOT).parts else "/changelog"


def release_notes_url(path: Path) -> str | None:
    rel = path.relative_to(ROOT).as_posix()
    if rel == "index.html":
        return CHANGELOG
    if rel == "zh/index.html":
        return CHANGELOG_ZH
    return None


def patch_html(path: Path, text: str) -> str:
    if FEED_LINK not in text:
        if '<link rel="canonical"' in text:
            text = re.sub(
                r'(<link rel="canonical" href="[^"]+">)',
                r"\1" + FEED_LINK,
                text,
                count=1,
            )
        elif '<link rel="stylesheet"' in text:
            text = text.replace(
                '<link rel="stylesheet" href="/styles.css">',
                FEED_LINK + '<link rel="stylesheet" href="/styles.css">',
                1,
            )
        else:
            raise SystemExit(f"{path.relative_to(ROOT)} has nowhere to put the feed link")
    dest = changelog_href(path)
    footer = f'href="{dest}">Changelog</a>'
    if FOOTER_OLD in text:
        text = text.replace(FOOTER_OLD, footer)
    elif footer not in text:
        raise SystemExit(f"{path.relative_to(ROOT)} is missing the changelog footer link")
    text = text.replace(ROADMAP_EN_OLD, ROADMAP_EN_NEW)
    text = text.replace(ROADMAP_ZH_OLD, ROADMAP_ZH_NEW)
    notes = release_notes_url(path)
    if notes and '"releaseNotes"' not in text:
        text, count = re.subn(
            r'("softwareVersion": "[^"]+",)\n',
            lambda match: match.group(1) + f'\n        "releaseNotes": "{notes}",\n',
            text,
            count=1,
        )
        if count != 1:
            raise SystemExit(f"{path.relative_to(ROOT)} is missing softwareVersion")
    return text


def html_paths() -> list[Path]:
    paths = []
    for path in sorted(ROOT.rglob("*.html")):
        rel = path.relative_to(ROOT)
        if path.name in SKIP_HTML or "kaku-site-overview" in rel.parts:
            continue
        if any(part.startswith(".") for part in rel.parts):
            continue
        paths.append(path)
    return paths


def build() -> dict[Path, str]:
    releases = stable_releases(fetch_releases())
    files: dict[Path, str] = {
        ROOT / "feed.xml": rss(releases),
        ROOT / "feeds" / "releases.json": data_document(releases),
        ROOT / "changelog.html": render_page(
            (ROOT / "roadmap.html").read_text(encoding="utf-8"), releases, False
        ),
        ROOT / "zh" / "changelog.html": render_page(
            (ROOT / "zh" / "roadmap.html").read_text(encoding="utf-8"), releases, True
        ),
    }
    for path, text in list(files.items()):
        if path.suffix == ".html":
            files[path] = patch_html(path, text)
    for path in html_paths():
        if path in files:
            continue
        files[path] = patch_html(path, path.read_text(encoding="utf-8"))
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without writing")
    args = parser.parse_args()
    files = build()
    stale = []
    for path, text in files.items():
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(path.relative_to(ROOT).as_posix())
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    if args.check:
        if stale:
            print("stale release files, run scripts/build_releases.py:", file=sys.stderr)
            for name in stale:
                print(f"  {name}", file=sys.stderr)
            return 1
        print(f"release feed up to date ({len(files)} files)")
        return 0
    print(f"wrote {len(files)} release files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
