#!/usr/bin/env python3
"""Build the blog (/blog and /zh/blog) from content/blog.

Sources:
  content/blog/index.json        publication queue, one entry per post, in order
  content/blog/<slug>.en.md      English body
  content/blog/<slug>.zh.md      Chinese body

A post is live once its `date` is set and not later than today (Asia/Shanghai).
Posts with `date: null` wait in the queue; scripts/publish_next_post.py dates
the next one each day. Bodies are Markdown; a line of the form
`[[figure:<name>|<alt>|<caption>]]` embeds img/blog/<name>.webp, switching to
<name>-mobile.webp on phones when that file exists.

Writes per language: blog/index.html, blog/<slug>.html, blog/<slug>.md and
blog/feed.xml, plus the blog block of sitemap.xml, llms.txt and zh/llms.txt.

    python3 scripts/build_blog.py           # write
    python3 scripts/build_blog.py --check   # fail if anything is stale
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import sys
from datetime import date, datetime
from email.utils import format_datetime
from pathlib import Path
from zoneinfo import ZoneInfo

import markdown
from markdown.extensions.toc import slugify_unicode

sys.path.insert(0, str(Path(__file__).resolve().parent))
from highlight import BLOCK_RE, render_block  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "content" / "blog"
SITE = "https://kaku.fun"
TZ = ZoneInfo("Asia/Shanghai")
LANGS = ("en", "zh")

UI = {
    "en": {
        "html_lang": "en",
        "prefix": "",
        "blog": "Blog",
        "docs": "Docs",
        "compare": "Compare",
        "changelog": "Changelog",
        "index_title": "Kaku Blog",
        "index_lede": "Notes on Mac terminals, AI coding tools and how Kaku is built, by Tw93.",
        "index_desc": "Guides and notes on Mac terminals, Claude Code, Codex and shell setup, and how Kaku is built.",
        "published": "Published",
        "updated": "Updated",
        "read": "{n} min read",
        "related": "Keep reading",
        "faq": "FAQ",
        "cta": "Kaku is a free, open-source Mac terminal.",
        "cta_btn": "Install Kaku",
        "skip": "Skip to content",
        "feed_title": "Kaku Blog",
        "llms_head": "## Blog",
    },
    "zh": {
        "html_lang": "zh-CN",
        "prefix": "/zh",
        "blog": "博客",
        "docs": "文档",
        "compare": "对比",
        "changelog": "更新日志",
        "index_title": "Kaku 博客",
        "index_lede": "关于 Mac 终端、AI 编程工具和 Kaku 怎么做出来的一些记录，作者 Tw93。",
        "index_desc": "关于 Mac 终端、Claude Code、Codex、shell 配置的指南，以及 Kaku 是怎么做出来的。",
        "published": "发布于",
        "updated": "更新于",
        "read": "约 {n} 分钟",
        "related": "继续阅读",
        "faq": "常见问题",
        "cta": "Kaku 是一个免费开源的 Mac 终端。",
        "cta_btn": "安装 Kaku",
        "skip": "跳到正文",
        "feed_title": "Kaku 博客",
        "llms_head": "## 博客",
    },
}

# Topic clusters, in the index's reading order. Each post's `category` in
# index.json picks one; the index groups by it, the article meta line links to
# its group, related links stay inside it, and BlogPosting carries it as
# articleSection.
CATEGORIES = ("agents", "terminal", "shell")

CATEGORY_COPY = {
    "agents": {
        "en": ("Claude Code and Codex", "Install, configure and run AI coding agents in the terminal."),
        "zh": ("Claude Code 与 Codex", "在终端里安装、配置和使用 AI 编程助手。"),
    },
    "terminal": {
        "en": ("Terminal", "Choose and configure a Mac terminal: fonts, splits, shortcuts."),
        "zh": ("终端", "挑选和配置 Mac 终端，包括字体、分屏和快捷键。"),
    },
    "shell": {
        "en": ("Shell and tools", "Set up zsh, the prompt and the command-line tools around it."),
        "zh": ("Shell 与命令行工具", "配置 zsh、提示符和围绕它的命令行工具。"),
    },
}

# One entry point per cluster, linked first from its siblings once it is live.
CATEGORY_PILLARS = {
    "agents": "install-claude-code-mac",
    "terminal": "best-terminal-for-mac",
    "shell": "zsh-without-oh-my-zsh",
}

FIGURE_RE = re.compile(r"^\[\[figure:([a-z0-9-]+)\|([^|\]]+)\|([^\]]+)\]\]$", re.M)


def today() -> date:
    override = os.environ.get("BLOG_TODAY")
    if override:
        return date.fromisoformat(override)
    return datetime.now(TZ).date()


def load_queue() -> list[dict]:
    queue = json.loads((SRC / "index.json").read_text(encoding="utf-8"))["posts"]
    check_categories(queue)
    return queue


def check_categories(queue: list[dict]) -> None:
    """Every post, queued or live, needs a known category: the index only renders
    the groups in CATEGORIES, so a typo would drop a live post from it silently."""
    bad = sorted(p["slug"] for p in queue if p.get("category") not in CATEGORIES)
    if bad:
        raise SystemExit(f"blog posts with a missing or unknown category: {', '.join(bad)}")


def category_label(category: str, lang: str) -> str:
    return CATEGORY_COPY[category][lang][0]


def live_posts(queue: list[dict], now: date) -> list[dict]:
    live = [p for p in queue if p.get("date") and date.fromisoformat(p["date"]) <= now]
    return sorted(live, key=lambda p: (p["date"], queue.index(p)), reverse=True)


def url(lang: str, path: str = "") -> str:
    base = UI[lang]["prefix"] + "/blog"
    return base + ("/" + path if path else "")


def fmt_date(d: str, lang: str) -> str:
    dt = date.fromisoformat(d)
    if lang == "zh":
        return f"{dt.year}年{dt.month}月{dt.day}日"
    return dt.strftime("%b ") + str(dt.day) + dt.strftime(", %Y")


def reading_minutes(text: str, lang: str) -> int:
    plain = re.sub(r"```.*?```", "", text, flags=re.S)
    plain = FIGURE_RE.sub("", plain)
    # mole.fit's rates: Han at 300 a minute plus Latin words at 200, so a
    # translation shows about the same minutes as its English original (400
    # had Chinese posts reading well under their English).
    words = len(re.findall(r"[A-Za-z0-9]+", plain))
    if lang == "zh":
        return max(1, round(len(re.findall(r"[\u3400-\u4dbf\u4e00-\u9fff]", plain)) / 300 + words / 200))
    return max(1, round(words / 200))


def figure_dims(name: str) -> tuple[int, int]:
    """CSS size of a diagram: the files are saved at twice the size they show at."""
    from PIL import Image

    with Image.open(ROOT / "img" / "blog" / f"{name}.webp") as im:
        return round(im.size[0] / 2), round(im.size[1] / 2)


def figure_html(m: re.Match) -> str:
    name, alt, caption = m.group(1), m.group(2).strip(), m.group(3).strip()
    w, h = figure_dims(name)
    img = (f'<img src="/img/blog/{name}.webp" width="{w}" height="{h}" loading="lazy" '
           f'alt="{html.escape(alt, quote=True)}">')
    if (ROOT / "img" / "blog" / f"{name}-mobile.webp").exists():
        mw, mh = figure_dims(f"{name}-mobile")
        img = (f'<picture><source media="(max-width: 640px)" srcset="/img/blog/{name}-mobile.webp" '
               f'width="{mw}" height="{mh}">{img}</picture>')
    return (f'<figure class="blog-diagram">{img}'
            f"<figcaption>{html.escape(caption)}</figcaption></figure>")


def render_body(text: str) -> str:
    text = FIGURE_RE.sub(figure_html, text)
    md = markdown.Markdown(
        extensions=["extra", "toc", "sane_lists"],
        extension_configs={"toc": {"slugify": slugify_unicode, "permalink": False}},
    )
    out = md.convert(text)
    # External links open in a new tab, like the docs.
    out = re.sub(r'<a href="(https?://(?!kaku\.fun)[^"]+)">',
                 r'<a href="\1" target="_blank" rel="noopener noreferrer">', out)
    return BLOCK_RE.sub(render_block, out)


def faq_pairs(body_html: str, lang: str) -> list[tuple[str, str]]:
    head = re.search(rf'<h2 id="[^"]*">{re.escape(UI[lang]["faq"])}</h2>(.*?)(?=<h2 |\Z)', body_html, re.S)
    if not head:
        return []
    pairs = []
    for q, a in re.findall(r"<h3[^>]*>(.*?)</h3>(.*?)(?=<h3|\Z)", head.group(1), re.S):
        answer = re.sub(r"<[^>]+>", "", a)
        pairs.append((html.unescape(re.sub(r"<[^>]+>", "", q)).strip(), html.unescape(answer).strip()))
    return pairs


# ---------------------------------------------------------------- chrome

def chrome(lang: str, en_path: str, zh_path: str) -> tuple[str, str]:
    """Nav and footer copied from the compare page, pointed at this page."""
    src = (ROOT / ("compare.html" if lang == "en" else "zh/compare.html")).read_text(encoding="utf-8")
    nav = re.search(r'<a class="skip-link".*?</nav>', src, re.S).group(0)
    foot = re.search(r'<footer class="site-footer">.*?</footer>', src, re.S).group(0)
    nav = nav.replace('href="/zh/compare"', f'href="{zh_path}"').replace('href="/compare"', f'href="{en_path}"')
    p = UI[lang]["prefix"]
    nav = re.sub(r'<div class="nav-links">.*?</div>',
                 f'<div class="nav-links"><a href="{p}/docs">{UI[lang]["docs"]}</a>'
                 f'<a href="{p}/compare">{UI[lang]["compare"]}</a>'
                 f'<a href="{p}/changelog">{UI[lang]["changelog"]}</a>'
                 f'<a href="{p}/blog" aria-current="page">{UI[lang]["blog"]}</a></div>', nav)
    return nav, foot


def head(lang: str, *, title: str, desc: str, canonical: str, en_url: str, zh_url: str,
         md_url: str, og_type: str, image: str, image_alt: str, jsonld: dict, extra: str = "") -> str:
    e = lambda s: html.escape(s, quote=True)  # noqa: E731
    return f"""<!doctype html>
<html lang="{UI[lang]['html_lang']}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light">
  <title>{e(title)}</title>
  <meta name="description" content="{e(desc)}">
  <meta property="og:title" content="{e(title)}">
  <meta property="og:description" content="{e(desc)}">
  <meta property="og:type" content="{og_type}">
  <meta property="og:image" content="{image}">
  <meta property="og:image:width" content="1600">
  <meta property="og:image:height" content="900">
  <meta property="og:image:alt" content="{e(image_alt)}">
  <meta property="og:url" content="{canonical}">{extra}
  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:image" content="{image}">
  <link rel="icon" type="image/png" sizes="32x32" href="/img/favicon-kaku-32.png"><link rel="icon" type="image/png" sizes="192x192" href="/img/favicon-kaku-192.png"><link rel="apple-touch-icon" href="/img/favicon-kaku-180.png">
  <link rel="canonical" href="{canonical}"><link rel="alternate" type="application/rss+xml" title="Kaku releases" href="https://kaku.fun/feed.xml">
  <link rel="alternate" type="application/rss+xml" title="{e(UI[lang]['feed_title'])}" href="{SITE}{url(lang, 'feed.xml')}">
  <link rel="alternate" hreflang="en" href="{en_url}">
  <link rel="alternate" hreflang="zh-CN" href="{zh_url}">
  <link rel="alternate" hreflang="x-default" href="{en_url}">
  <link rel="alternate" type="text/markdown" href="{md_url}">
  <link rel="stylesheet" href="/styles.css">
  <script type="application/ld+json">
{json.dumps(jsonld, ensure_ascii=False, indent=2)}
  </script>
</head>
"""


AUTHOR = {"@type": "Person", "name": "Tw93", "url": "https://tw93.fun",
          "sameAs": ["https://x.com/HiTw93", "https://github.com/tw93"]}


def breadcrumb(lang: str, items: list[tuple[str, str]]) -> dict:
    return {"@type": "BreadcrumbList", "itemListElement": [
        {"@type": "ListItem", "position": i + 1, "name": n, "item": u} for i, (n, u) in enumerate(items)]}


def cover_url(slug: str) -> str:
    return f"{SITE}/img/blog/cards/{slug}.webp"


# ---------------------------------------------------------------- pages

def post_minutes(p: dict, lang: str) -> int:
    return reading_minutes((SRC / f"{p['slug']}.{lang}.md").read_text(encoding="utf-8"), lang)


def post_page(p: dict, lang: str, live: list[dict], queue: list[dict]) -> tuple[str, str]:
    meta = p[lang]
    slug = p["slug"]
    src = (SRC / f"{slug}.{lang}.md").read_text(encoding="utf-8")
    body = render_body(src)
    minutes = reading_minutes(src, lang)
    canonical = SITE + url(lang, slug)
    en_url, zh_url = SITE + url("en", slug), SITE + url("zh", slug)
    modified = p.get("modified") or p["date"]
    ui = UI[lang]
    home = SITE + ("/" if lang == "en" else "/zh")
    posting = {
        "@type": "BlogPosting",
        "@id": canonical + "#article",
        "headline": meta["title"],
        "description": meta["description"],
        "url": canonical,
        "mainEntityOfPage": canonical,
        "datePublished": p["date"],
        "dateModified": modified,
        "inLanguage": ui["html_lang"],
        "articleSection": category_label(p["category"], lang),
        "image": cover_url(slug),
        "timeRequired": f"PT{minutes}M",
        "author": AUTHOR,
        "publisher": {"@id": "https://kaku.fun/#organization"},
        "isPartOf": {"@id": SITE + url(lang) + "#blog"},
        "about": {"@id": "https://kaku.fun/#software"},
        "encoding": {"@type": "MediaObject", "encodingFormat": "text/markdown",
                     "contentUrl": canonical + ".md"},
    }
    graph = [breadcrumb(lang, [("Kaku", home), (ui["blog"], SITE + url(lang)), (meta["title"], canonical)]), posting]
    faqs = faq_pairs(body, lang)
    if faqs:
        graph.append({"@type": "FAQPage", "mainEntity": [
            {"@type": "Question", "name": q, "acceptedAnswer": {"@type": "Answer", "text": a}} for q, a in faqs]})
    extra = (f'\n  <meta property="article:published_time" content="{p["date"]}">'
             f'\n  <meta property="article:modified_time" content="{modified}">'
             f'\n  <meta property="article:author" content="Tw93">')
    out = head(lang, title=f"{meta['title']} · {ui['index_title']}", desc=meta["description"],
               canonical=canonical, en_url=en_url, zh_url=zh_url, md_url=canonical + ".md",
               og_type="article", image=cover_url(slug), image_alt=meta["title"],
               jsonld={"@context": "https://schema.org", "@graph": graph}, extra=extra)
    nav, foot = chrome(lang, url("en", slug), url("zh", slug))
    cat = p["category"]
    dates = (f'<span><a href="{url(lang)}#topic-{cat}">{html.escape(category_label(cat, lang))}</a></span>'
             f'<span>{ui["published"]} <time datetime="{p["date"]}">{fmt_date(p["date"], lang)}</time></span>')
    if modified != p["date"]:
        dates += f'<span>{ui["updated"]} <time datetime="{modified}">{fmt_date(modified, lang)}</time></span>'
    dates += f'<span>{ui["read"].format(n=minutes)}</span>'
    related = related_posts(p, live, queue)
    rel_html = ""
    if related:
        items = "".join(
            f'<li><a href="{url(lang, r["slug"])}"><span class="blog-related-cat">{html.escape(category_label(r["category"], lang))}</span>'
            f'<span class="blog-related-name">{html.escape(r[lang]["title"])}</span>'
            f'<span class="blog-related-meta"><time datetime="{r["date"]}">{fmt_date(r["date"], lang)}</time> · {ui["read"].format(n=post_minutes(r, lang))}</span></a></li>'
            for r in related)
        rel_html = (f'\n          <nav class="blog-related" aria-label="{ui["related"]}">'
                    f'<h2 class="blog-related-title">{ui["related"]}</h2><ul class="blog-related-list">{items}</ul></nav>')
    p_ = ui["prefix"]
    out += f"""<body>
  {nav}
  <main id="main-content" class="docs-page blog-page">
    <header class="page-hero doc-hero blog-hero"><div class="wide"><nav class="blog-crumb" aria-label="Breadcrumb"><a href="{url(lang)}">{ui['blog']}</a></nav><h1>{html.escape(meta['title'])}</h1><p class="blog-meta">{dates}</p></div></header>
    <section class="section">
      <div class="wide">
        <article class="doc-content blog-article">
          <figure class="blog-cover" aria-hidden="true"><img src="/img/blog/cards/{slug}.webp" width="1600" height="900" alt="" fetchpriority="high"></figure>
{body}
          <aside class="blog-cta"><img class="blog-cta-icon" src="/img/kaku-mark.png" width="40" height="40" alt=""><p>{ui['cta']}</p><a class="btn btn-primary" href="{p_}/docs">{ui['cta_btn']}</a></aside>{rel_html}
        </article>
      </div>
    </section>
  </main>
  {foot}
</body>
</html>
"""
    md_twin = (f"# {meta['title']}\n\n> {meta['description']}\n\n"
               f"{ui['published']} {p['date']} · Tw93 · {canonical}\n\n"
               + FIGURE_RE.sub(lambda m: f"![{m.group(2).strip()}]({SITE}/img/blog/{m.group(1)}.webp)\n\n*{m.group(3).strip()}*", src).strip()
               + "\n")
    return out, md_twin


def related_posts(p: dict, live: list[dict], queue: list[dict], limit: int = 3) -> list[dict]:
    """Explicit `related` pins, then the category pillar, then the nearest
    same-category neighbours in queue order, then any other live post. Only live
    posts are candidates, so a queued pin or pillar is skipped, never linked."""
    by_slug = {x["slug"]: x for x in live}
    picked: list[dict] = []

    def add(x: dict | None) -> None:
        if x and x["slug"] != p["slug"] and x not in picked and len(picked) < limit:
            picked.append(x)

    for slug in p.get("related", []):
        add(by_slug.get(slug))
    add(by_slug.get(CATEGORY_PILLARS.get(p["category"], "")))
    order = [x["slug"] for x in queue]
    same = sorted((x for x in live if x["category"] == p["category"]),
                  key=lambda x: order.index(x["slug"]))
    at = next((i for i, x in enumerate(same) if x["slug"] == p["slug"]), -1)
    if at >= 0:
        for d in range(1, len(same)):
            add(same[at - d] if at - d >= 0 else None)
            add(same[at + d] if at + d < len(same) else None)
    for x in live:
        add(x)
    return picked


def index_page(lang: str, live: list[dict]) -> str:
    ui = UI[lang]
    canonical = SITE + url(lang)
    home = SITE + ("/" if lang == "en" else "/zh")
    blog_node = {
        "@type": "Blog",
        "@id": canonical + "#blog",
        "name": ui["index_title"],
        "description": ui["index_desc"],
        "url": canonical,
        "inLanguage": ui["html_lang"],
        "author": AUTHOR,
        "publisher": {"@id": "https://kaku.fun/#organization"},
        "blogPost": [{"@type": "BlogPosting", "headline": p[lang]["title"], "url": SITE + url(lang, p["slug"]),
                      "datePublished": p["date"], "image": cover_url(p["slug"])} for p in live],
    }
    graph = [breadcrumb(lang, [("Kaku", home), (ui["blog"], canonical)]), blog_node]
    image = cover_url(live[0]["slug"]) if live else f"{SITE}/shots/kaku-og.webp"
    out = head(lang, title=ui["index_title"], desc=ui["index_desc"], canonical=canonical,
               en_url=SITE + url("en"), zh_url=SITE + url("zh"), md_url=canonical + "/index.md",
               og_type="website", image=image, image_alt=ui["index_title"],
               jsonld={"@context": "https://schema.org", "@graph": graph})
    nav, foot = chrome(lang, url("en"), url("zh"))
    card = lambda p: (  # noqa: E731
        f'<li class="blog-card"><a href="{url(lang, p["slug"])}">'
        f'<span class="blog-card-media"><img src="/img/blog/cards/{p["slug"]}.webp" width="1600" height="900" alt="" loading="lazy"></span>'
        f'<span class="blog-card-body"><span class="blog-card-title">{html.escape(p[lang]["title"])}</span>'
        f'<span class="blog-card-desc">{html.escape(p[lang]["description"])}</span>'
        f'<span class="blog-card-date"><time datetime="{p["date"]}">{fmt_date(p["date"], lang)}</time> · {UI[lang]["read"].format(n=post_minutes(p, lang))}</span></span></a></li>')
    groups = ""
    for cat in CATEGORIES:
        posts = [p for p in live if p["category"] == cat]
        if not posts:
            continue
        label, desc = CATEGORY_COPY[cat][lang]
        groups += (f'<section id="topic-{cat}" class="blog-group" aria-labelledby="topic-{cat}-title">'
                   f'<div class="blog-group-head"><h2 id="topic-{cat}-title" class="blog-group-title">{html.escape(label)}</h2>'
                   f'<p class="blog-group-description">{html.escape(desc)}</p></div>'
                   f'<ul class="blog-cards">{"".join(card(p) for p in posts)}</ul></section>')
    out += f"""<body>
  {nav}
  <main id="main-content" class="docs-page blog-page">
    <header class="page-hero doc-hero"><div class="wide"><h1>{ui['index_title']}</h1><p>{ui['index_lede']}</p></div></header>
    <section class="section">
      <div class="wide">
        <div class="blog-groups">{groups}</div>
      </div>
    </section>
  </main>
  {foot}
</body>
</html>
"""
    return out


def index_md(lang: str, live: list[dict]) -> str:
    ui = UI[lang]
    lines = [f"# {ui['index_title']}", "", ui["index_lede"], ""]
    for cat in CATEGORIES:
        posts = [p for p in live if p["category"] == cat]
        if not posts:
            continue
        label, desc = CATEGORY_COPY[cat][lang]
        lines += [f"## {label}", "", desc, ""]
        for p in posts:
            lines.append(f"- [{p[lang]['title']}]({SITE}{url(lang, p['slug'])}.md) ({p['date']}): {p[lang]['description']}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def feed_xml(lang: str, live: list[dict]) -> str:
    ui = UI[lang]
    items = []
    for p in live:
        link = SITE + url(lang, p["slug"])
        pub = format_datetime(datetime.fromisoformat(p["date"] + "T09:00:00").replace(tzinfo=TZ))
        items.append(f"""    <item>
      <title>{html.escape(p[lang]['title'])}</title>
      <link>{link}</link>
      <guid isPermaLink="true">{link}</guid>
      <pubDate>{pub}</pubDate>
      <dc:creator>Tw93</dc:creator>
      <description>{html.escape(p[lang]['description'])}</description>
    </item>""")
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>{ui['feed_title']}</title>
    <link>{SITE}{url(lang)}</link>
    <atom:link href="{SITE}{url(lang, 'feed.xml')}" rel="self" type="application/rss+xml"/>
    <description>{html.escape(ui['index_desc'])}</description>
    <language>{ui['html_lang']}</language>
{chr(10).join(items)}
  </channel>
</rss>
"""


# ---------------------------------------------------------------- shared files

def replace_block(text: str, start: str, end: str, inner: str, path: str) -> str:
    pattern = re.compile(re.escape(start) + r".*?" + re.escape(end), re.S)
    if not pattern.search(text):
        raise SystemExit(f"{path} is missing the {start} … {end} markers")
    return pattern.sub(lambda _: start + inner + end, text)


def sitemap_block(live: list[dict]) -> str:
    rows = []
    newest = max((p.get("modified") or p["date"] for p in live), default=None)
    if newest:
        for lang in LANGS:
            rows.append(f"  <url><loc>{SITE}{url(lang)}</loc><lastmod>{newest}</lastmod><priority>0.7</priority></url>")
    for p in live:
        mod = p.get("modified") or p["date"]
        for lang in LANGS:
            rows.append(f"  <url><loc>{SITE}{url(lang, p['slug'])}</loc><lastmod>{mod}</lastmod><priority>0.6</priority></url>")
    return "\n" + "\n".join(rows) + ("\n  " if rows else "\n  ")


def llms_block(lang: str, live: list[dict]) -> str:
    ui = UI[lang]
    if not live:
        return "\n"
    sep = ": " if lang == "en" else "："
    lines = [ui["llms_head"], ""]
    for p in live:
        lines.append(f"- [{p[lang]['title']}]({SITE}{url(lang, p['slug'])}.md){sep}{p[lang]['description']}")
    return "\n" + "\n".join(lines) + "\n\n"


def outputs() -> dict[Path, str]:
    queue = load_queue()
    live = live_posts(queue, today())
    files: dict[Path, str] = {}
    for lang in LANGS:
        base = ROOT / url(lang).lstrip("/")
        files[base / "index.html"] = index_page(lang, live)
        files[base / "index.md"] = index_md(lang, live)
        files[base / "feed.xml"] = feed_xml(lang, live)
        for p in live:
            page, twin = post_page(p, lang, live, queue)
            files[base / f"{p['slug']}.html"] = page
            files[base / f"{p['slug']}.md"] = twin
    sm = (ROOT / "sitemap.xml").read_text(encoding="utf-8")
    files[ROOT / "sitemap.xml"] = replace_block(sm, "<!-- blog:start -->", "<!-- blog:end -->", sitemap_block(live), "sitemap.xml")
    # A scheduled post also changes the sitemap and its discovery indexes.
    newest = max(re.findall(r"<lastmod>([0-9-]+)</lastmod>", files[ROOT / "sitemap.xml"]))
    index = (ROOT / "sitemap-index.xml").read_text(encoding="utf-8")
    files[ROOT / "sitemap-index.xml"] = re.sub(
        r"<lastmod>([0-9-]+)</lastmod>",
        lambda m: f"<lastmod>{max(m[1], newest)}</lastmod>", index,
    )
    schema = (ROOT / "schemamap.xml").read_text(encoding="utf-8")
    blog_date = max((p.get("modified") or p["date"] for p in live), default=None)
    if blog_date:
        schema = re.sub(
            r"(<loc>https://kaku.fun/(?:feeds/pages.jsonl|llms.txt)</loc>\s*<lastmod>)([0-9-]+)(</lastmod>)",
            lambda m: m[1] + max(m[2], blog_date) + m[3], schema,
        )
    files[ROOT / "schemamap.xml"] = schema
    for lang, path in (("en", "llms.txt"), ("zh", "zh/llms.txt")):
        t = (ROOT / path).read_text(encoding="utf-8")
        files[ROOT / path] = replace_block(t, "<!-- blog:start -->", "<!-- blog:end -->", llms_block(lang, live), path)
    return files


def stale_outputs(files: dict[Path, str]) -> list[Path]:
    """Generated blog files on disk that no live post owns any more."""
    known = set(files)
    stale = []
    for lang in LANGS:
        d = ROOT / url(lang).lstrip("/")
        if d.exists():
            stale += [f for f in d.iterdir() if f.is_file() and f not in known]
    return stale


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    files = outputs()
    stale = stale_outputs(files)
    if args.check:
        bad = [p for p, c in files.items() if not p.exists() or p.read_text(encoding="utf-8") != c]
        for p in bad + stale:
            print(f"stale: {p.relative_to(ROOT)}")
        print(f"blog: {len(files)} generated files, {len(bad) + len(stale)} stale")
        return 1 if bad or stale else 0
    for p, c in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        if not p.exists() or p.read_text(encoding="utf-8") != c:
            p.write_text(c, encoding="utf-8")
    for p in stale:
        p.unlink()
    live = sum(1 for k in files if k.suffix == ".html" and k.name != "index.html") // 2
    print(f"blog: {live} live post(s), {len(files)} files written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
