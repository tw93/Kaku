#!/usr/bin/env python3
"""Generate the .md twin of every content page from its HTML source.

Agents that fetch `https://kaku.fun/docs/guide.md` (or send `Accept:
text/markdown`) get the same prose the browser gets, without the chrome.
The homepage twins (`index.md`, `zh/index.md`) are hand written because the
landing page is marketing layout rather than prose; everything else is
generated here so the two never drift.

    python3 scripts/build_markdown.py           # write the .md files
    python3 scripts/build_markdown.py --check   # fail if any is stale
"""

from __future__ import annotations

import argparse
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SITE = "https://kaku.fun"

# Pages whose prose is generated. The homepage twins are excluded on purpose.
PAGES = [
    "docs/index.html",
    "docs/guide.html",
    "docs/features.html",
    "docs/cli.html",
    "docs/configuration.html",
    "docs/keybindings.html",
    "docs/faq.html",
    "docs/contributing.html",
    "roadmap.html",
    "about.html",
    "compare.html",
    "contact.html",
    "privacy.html",
]
PAGES += ["zh/" + p for p in PAGES]

VOID = {"br", "hr", "img", "meta", "link", "input", "source", "col"}
# Content roots, tried in order. First match on a page wins for each root.
ROOTS = [("header", "page-hero"), ("article", "doc-content"), ("div", "release-doc")]
DROP_CLASSES = {"anchor", "doc-pager", "rel-date", "label"}


class Node:
    __slots__ = ("tag", "attrs", "children", "text")

    def __init__(self, tag: str, attrs: dict | None = None, text: str = ""):
        self.tag = tag
        self.attrs = attrs or {}
        self.children: list[Node] = []
        self.text = text

    def has_class(self, name: str) -> bool:
        return name in self.attrs.get("class", "").split()


class TreeBuilder(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = Node("#root")
        self.stack = [self.root]

    def handle_starttag(self, tag, attrs):
        node = Node(tag, {k: (v or "") for k, v in attrs})
        self.stack[-1].children.append(node)
        if tag not in VOID:
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        self.stack[-1].children.append(Node(tag, {k: (v or "") for k, v in attrs}))

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        for i in range(len(self.stack) - 1, 0, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                return

    def handle_data(self, data):
        self.stack[-1].children.append(Node("#text", text=data))


def find_roots(node: Node, found: dict[tuple[str, str], Node]) -> None:
    for tag, cls in ROOTS:
        if node.tag == tag and node.has_class(cls) and (tag, cls) not in found:
            found[(tag, cls)] = node
    for child in node.children:
        find_roots(child, found)


def esc(text: str) -> str:
    return re.sub(r"([\\`*_\[\]])", r"\\\1", text)


def inline(node: Node) -> str:
    """Render a node's subtree as one line of inline markdown."""
    if node.tag == "#text":
        return esc(node.text)
    if any(node.has_class(c) for c in DROP_CLASSES):
        return ""
    inner = "".join(inline(c) for c in node.children)
    if node.tag == "code":
        # Code spans are literal; undo the escaping the text branch applied.
        raw = "".join(plain(c) for c in node.children)
        fence = "`" * (longest_backtick_run(raw) + 1)
        pad = " " if raw.startswith("`") or raw.endswith("`") else ""
        return f"{fence}{pad}{raw}{pad}{fence}"
    if node.tag in ("strong", "b"):
        return f"**{inner.strip()}**" if inner.strip() else ""
    if node.tag in ("em", "i"):
        return f"*{inner.strip()}*" if inner.strip() else ""
    if node.tag == "a":
        href = absolute(node.attrs.get("href", ""))
        label = inner.strip()
        return f"[{label}]({href})" if href and label else label
    if node.tag == "br":
        return "\n"
    if node.tag == "img":
        alt = esc(node.attrs.get("alt", ""))
        return f"![{alt}]({absolute(node.attrs.get('src', ''))})"
    return inner


def plain(node: Node) -> str:
    if node.tag == "#text":
        return node.text
    return "".join(plain(c) for c in node.children)


def longest_backtick_run(text: str) -> int:
    return max((len(m) for m in re.findall(r"`+", text)), default=0)


def absolute(href: str) -> str:
    return SITE + href if href.startswith("/") else href


def squeeze(text: str) -> str:
    return re.sub(r"[ \t]*\n[ \t]*", " ", text).strip()


def render_table(node: Node) -> str:
    rows: list[list[str]] = []
    header_len = 0

    def walk(n: Node) -> None:
        nonlocal header_len
        if n.tag == "tr":
            cells = [squeeze(inline(c)) for c in n.children if c.tag in ("td", "th")]
            if cells:
                rows.append(cells)
                if all(c.tag == "th" for c in n.children if c.tag in ("td", "th")):
                    header_len = len(cells)
            return
        for c in n.children:
            walk(c)

    walk(node)
    if not rows:
        return ""
    width = max(len(r) for r in rows)
    rows = [r + [""] * (width - len(r)) for r in rows]
    if not header_len:
        rows.insert(0, [""] * width)
    lines = ["| " + " | ".join(rows[0]) + " |", "| " + " | ".join(["---"] * width) + " |"]
    lines += ["| " + " | ".join(r) + " |" for r in rows[1:]]
    return "\n".join(lines)


def render_list(node: Node, ordered: bool, depth: int) -> str:
    out = []
    index = 1
    for item in node.children:
        if item.tag != "li":
            continue
        marker = f"{index}. " if ordered else "- "
        index += 1
        body = render_block(item, depth + 1).strip()
        if not body:
            continue
        pad = " " * len(marker)
        first, *rest = body.split("\n")
        out.append("  " * depth + marker + first)
        out += ["  " * depth + pad + line if line else "" for line in rest]
    return "\n".join(out)


def render_block(node: Node, depth: int = 0) -> str:
    """Render a container node into block-level markdown."""
    blocks: list[str] = []
    buffer: list[str] = []

    def flush() -> None:
        text = squeeze("".join(buffer))
        buffer.clear()
        if text:
            blocks.append(text)

    for child in node.children:
        tag = child.tag
        if tag == "#text":
            buffer.append(inline(child))
            continue
        if any(child.has_class(c) for c in DROP_CLASSES) or tag in ("script", "style", "nav"):
            continue
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            flush()
            level = int(tag[1])
            blocks.append("#" * level + " " + squeeze(inline(child)))
        elif tag == "p":
            flush()
            text = squeeze(inline(child))
            if text:
                blocks.append(text)
        elif tag == "pre":
            flush()
            code = plain(child).strip("\n")
            lang = ""
            for c in child.children:
                if c.tag == "code":
                    lang = c.attrs.get("class", "").replace("language-", "").strip()
            blocks.append(f"```{lang}\n{code}\n```")
        elif tag in ("ul", "ol"):
            flush()
            rendered = render_list(child, tag == "ol", depth)
            if rendered:
                blocks.append(rendered)
        elif tag == "table":
            flush()
            rendered = render_table(child)
            if rendered:
                blocks.append(rendered)
        elif tag == "blockquote":
            flush()
            inner = render_block(child, depth)
            blocks.append("\n".join("> " + l if l else ">" for l in inner.split("\n")))
        elif tag == "hr":
            flush()
            blocks.append("---")
        elif tag == "dl":
            flush()
            for c in child.children:
                if c.tag == "dt":
                    blocks.append("**" + squeeze(inline(c)) + "**")
                elif c.tag == "dd":
                    blocks.append(squeeze(inline(c)))
        elif tag in ("div", "section", "article", "header", "main", "figure", "aside"):
            flush()
            inner = render_block(child, depth)
            if inner:
                blocks.append(inner)
        else:
            buffer.append(inline(child))

    flush()
    return "\n\n".join(b for b in blocks if b)


def page_markdown(path: Path) -> str:
    html = path.read_text(encoding="utf-8")
    builder = TreeBuilder()
    builder.feed(html)
    found: dict[tuple[str, str], Node] = {}
    find_roots(builder.root, found)
    parts = [render_block(found[key]) for key in ROOTS if key in found]
    body = "\n\n".join(p for p in parts if p)
    if not body:
        raise SystemExit(f"{path}: no content root found")

    rel = path.relative_to(ROOT).as_posix()
    url = SITE + "/" + re.sub(r"(index)?\.html$", "", rel)
    lines = [body, "", "---", "", f"Source: {url}", f"Site index for LLMs: {SITE}/llms.txt"]
    text = "\n".join(lines).rstrip() + "\n"
    return re.sub(r"\n{3,}", "\n\n", text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without writing")
    args = parser.parse_args()

    stale = []
    for rel in PAGES:
        src = ROOT / rel
        if not src.exists():
            raise SystemExit(f"missing page: {rel}")
        dst = ROOT / re.sub(r"\.html$", ".md", rel)
        text = page_markdown(src)
        if args.check:
            if not dst.exists() or dst.read_text(encoding="utf-8") != text:
                stale.append(dst.relative_to(ROOT).as_posix())
        else:
            dst.write_text(text, encoding="utf-8")

    if args.check:
        if stale:
            print("stale markdown twins, run scripts/build_markdown.py:", file=sys.stderr)
            for name in stale:
                print(f"  {name}", file=sys.stderr)
            return 1
        print(f"markdown twins up to date ({len(PAGES)} pages)")
    else:
        print(f"wrote {len(PAGES)} markdown twins")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
