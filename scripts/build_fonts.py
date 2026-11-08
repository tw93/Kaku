#!/usr/bin/env python3
"""Subset Source Han Serif SC for the Chinese text on kaku.fun.

iOS has no Chinese serif, so a stack of system names set every iPhone's
Chinese pages in PingFang, a sans, beside the serif Latin. The site carries
Source Han Serif SC (SIL OFL), the face every family site uses, subsetted to
exactly the CJK characters its pages use. The hero's 書 is a one-glyph cut of
Source Han Serif TC, as on the siblings. Until 2026-10-19 the body used
TsangerJinKai02, whose license is free for personal use only.

    python3 scripts/build_fonts.py          regenerate the subsets
    python3 scripts/build_fonts.py --stamp  rewrite the ?v= font hashes and the Chinese pages' preloads
    python3 scripts/build_fonts.py --check  verify every CJK character on the
                                            pages is covered by the shipped subsets

Run the regenerate step after editing Chinese copy: a character missing from
the subset silently falls back to the system font. --check needs neither the
source fonts nor the network.
"""
import hashlib
import html
import os
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parent.parent
FONT_DIR = root / "fonts"

# The source OTFs (about 24 MB each) are not committed. mole.fit keeps them in
# ~/www/mole-mac/site/.fontcache; otherwise unzip SourceHanSerifSC-Regular.otf
# and -SemiBold.otf from https://github.com/adobe-fonts/source-han-serif/releases
# into .fontcache at the site root.
CACHES = [
    pathlib.Path(os.environ["KAKU_FONT_CACHE"]) if os.environ.get("KAKU_FONT_CACHE") else None,
    root / ".fontcache",
    root.parent / "mole-mac" / "site" / ".fontcache",
]

# Every page: the English pages also set the hero's 書 in this face.
PAGES = [str(path.relative_to(root)) for path in root.rglob("*.html")
         if not any(part.startswith(".") or part == "node_modules" for part in path.relative_to(root).parts)]

# Regular for body text, SemiBold for every weight from 500 up, matching the
# two @font-face rules in styles.css.
SUBSETS = [
    ("regular", "ClearHanSerif-Regular.ttf"),
    ("semibold", "SourceHanSerifSC-SemiBold.otf"),
]

# Latin, digits and ASCII punctuation stay with Charter, so the subset carries
# CJK and CJK punctuation only. Mirrors the unicode-range in styles.css.
CJK_RANGES = [
    (0x2E80, 0x2EFF), (0x3000, 0x303F), (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0xFE30, 0xFE4F), (0xFF00, 0xFFEF),
    # Curly quotes and the ellipsis, drawn full width as Chinese text sets them.
    (0x201C, 0x201D), (0x2026, 0x2026),
]

# Attribute values the browser paints as text.
PAINTED_ATTRS = re.compile(r'\b(?:alt|placeholder|aria-label|title|data-title|data-line)="([^"]*)"')


def in_range(cp):
    return any(lo <= cp <= hi for lo, hi in CJK_RANGES)


def charset():
    chars = set()
    # Queued posts publish one a day without a font rebuild, so their sources
    # are covered ahead of time.
    for source in sorted((root / "content" / "blog").glob("*.md")):
        chars.update(c for c in source.read_text(encoding="utf-8") if in_range(ord(c)))
    for name in PAGES:
        text = (root / name).read_text(encoding="utf-8")
        text = re.sub(r"<script.*?</script>|<style.*?</style>|<head>.*?</head>", " ", text, flags=re.S)
        painted = " ".join(PAINTED_ATTRS.findall(text))
        text = re.sub(r"<[^>]+>", " ", text)
        chars.update(c for c in html.unescape(text + " " + painted) if in_range(ord(c)))
    return chars


# The hero's 書 comes from Source Han Serif TC like the siblings' marks (Mole 鼴,
# Pake 殼). A one-glyph file also keeps English visitors off the whole subset.
MARK = "書"
MARK_SOURCE = "SourceHanSerifTC-Regular.otf"
MARK_FILE = FONT_DIR / "kaku-mark.woff2"
MARK_CACHES = [
    pathlib.Path(os.environ["KAKU_FONT_CACHE"]) if os.environ.get("KAKU_FONT_CACHE") else None,
    root / ".fontcache",
    root.parent / "mole-mac" / "site" / ".fontcache",
]


def subset_path(weight):
    return FONT_DIR / f"kaku-serif-sc-{weight}.woff2"


def source_dir():
    for path in CACHES:
        if path and all((path / src).exists() for _, src in SUBSETS):
            return path
    return None


def regenerate():
    cache = source_dir()
    if cache is None:
        print("missing Source Han Serif SC: put SourceHanSerifSC-Regular.otf and -SemiBold.otf in .fontcache "
              "(https://github.com/adobe-fonts/source-han-serif/releases)", file=sys.stderr)
        return 1
    FONT_DIR.mkdir(exist_ok=True)
    chars = charset()
    text_file = FONT_DIR / ".charset.txt"
    text_file.write_text("".join(sorted(chars)), encoding="utf-8")
    try:
        for weight, src in SUBSETS:
            out = subset_path(weight)
            subprocess.run([
                sys.executable, "-m", "fontTools.subset", str(cache / src),
                f"--text-file={text_file}",
                "--flavor=woff2",
                "--layout-features=kern,liga,halt",
                "--no-hinting",
                "--desubroutinize",
                f"--output-file={out}",
            ], check=True)
            rename_reserved(out)
            print(f"  fonts/{out.name}  {len(chars)} glyphs  {out.stat().st_size / 1024:.0f} KB")
        mark_cache = next((p for p in MARK_CACHES if p and (p / MARK_SOURCE).exists()), None)
        if mark_cache is None:
            print(f"missing {MARK_SOURCE}: put it in .fontcache "
                  "(https://github.com/adobe-fonts/source-han-serif/releases)", file=sys.stderr)
            return 1
        subprocess.run([
            sys.executable, "-m", "fontTools.subset", str(mark_cache / MARK_SOURCE),
            f"--text={MARK}", "--flavor=woff2", "--no-hinting", "--desubroutinize",
            f"--output-file={MARK_FILE}",
        ], check=True)
        rename_reserved(MARK_FILE)
        print(f"  fonts/{MARK_FILE.name}  1 glyph  {MARK_FILE.stat().st_size / 1024:.0f} KB")
    finally:
        text_file.unlink(missing_ok=True)
    return 0


# Source is a Reserved Font Name under the OFL and a subset is a Modified
# Version, so a cut font may not keep Adobe's names (2026-10-30). The copyright
# and licence records (name IDs 0, 13, 14) stay as they are.
PRODUCT = "Kaku"
RESERVED = ("Source Han Serif", "SourceHanSerif")


def _renamed(text):
    return text.replace(RESERVED[0], f"{PRODUCT} Serif").replace(RESERVED[1], f"{PRODUCT}Serif")


def _cff_dicts(font):
    top = font["CFF "].cff.topDictIndex[0]
    return [top] + list(getattr(top, "FDArray", None) or [])


def rename_reserved(path):
    from fontTools.ttLib import TTFont
    font = TTFont(path, recalcTimestamp=False)
    for rec in font["name"].names:
        if rec.nameID not in (0, 13, 14):
            rec.string = _renamed(rec.toUnicode())
    if "CFF " in font:
        cff = font["CFF "].cff
        cff.fontNames = [_renamed(n) for n in cff.fontNames]
        for d in _cff_dicts(font):
            for key in ("FullName", "FamilyName", "FontName"):
                if isinstance(getattr(d, key, None), str):
                    setattr(d, key, _renamed(getattr(d, key)))
    font.save(path)


def reserved_names(path):
    from fontTools.ttLib import TTFont
    font = TTFont(path)
    names = [r.toUnicode() for r in font["name"].names if r.nameID not in (0, 13, 14)]
    if "CFF " in font:
        names += font["CFF "].cff.fontNames
        names += [getattr(d, k) for d in _cff_dicts(font) for k in ("FullName", "FamilyName", "FontName")
                  if isinstance(getattr(d, k, None), str)]
    return [n for n in names if _renamed(n) != n]


# styles.css asks for each font as `/fonts/<name>?v=<content hash>`, and the
# Chinese pages preload the two serif cuts at that same URL, so vercel.json can
# cache /fonts as immutable and a regenerated font still reaches every visitor.
FONT_URL = re.compile(r'url\("/fonts/([\w.-]+\.woff2)(?:\?v=([0-9a-f]+))?"\)')
PRELOAD_LINK = re.compile(r'[ \t]*<link rel="preload" as="font"[^>]*>\n')
STYLESHEET = '<link rel="stylesheet" href="/styles.css">'


def font_hash(name):
    return hashlib.sha256((FONT_DIR / name).read_bytes()).hexdigest()[:12]


def font_preload(indent="  "):
    """The Chinese pages load their two faces first, so the first paint of text
    waits for the real face rather than a system one (maintainer 2026-11-08)."""
    return "".join(
        f'{indent}<link rel="preload" as="font" type="font/woff2" href="/fonts/{subset_path(weight).name}'
        f'?v={font_hash(subset_path(weight).name)}" crossorigin fetchpriority="high">\n'
        for weight, _ in SUBSETS
    )


def chinese_pages():
    pages = (path for path in root.rglob("*.html")
             if not any(part.startswith(".") or part == "node_modules" for part in path.relative_to(root).parts))
    return [path for path in pages if re.search(r'<html[^>]*\blang="zh', path.read_text(encoding="utf-8")[:400])]


def stamp():
    path = root / "styles.css"
    css = path.read_text(encoding="utf-8")
    new = FONT_URL.sub(lambda m: f'url("/fonts/{m.group(1)}?v={font_hash(m.group(1))}")', css)
    if new != css:
        path.write_text(new, encoding="utf-8")
    links = font_preload()
    for page in chinese_pages():
        text = page.read_text(encoding="utf-8")
        new = re.sub(r"[ \t]*" + re.escape(STYLESHEET), lambda _: links + f"  {STYLESHEET}", PRELOAD_LINK.sub("", text), count=1)
        if new != text:
            page.write_text(new, encoding="utf-8")
    return 0


def stale_font_urls(css):
    return [m.group(1) for m in FONT_URL.finditer(css)
            if not (FONT_DIR / m.group(1)).exists() or m.group(2) != font_hash(m.group(1))]


def stale_preload_pages():
    links = font_preload()
    return [str(page.relative_to(root)) for page in chinese_pages()
            if "".join(m.group(0) for m in PRELOAD_LINK.finditer(page.read_text(encoding="utf-8"))) != links]


def song_faces_not_blocking(css):
    """A Chinese face that is not `block` paints the system face first, which is
    the flash first visits showed (maintainer 2026-11-08)."""
    faces = re.findall(r'@font-face \{[^}]*"%s Serif SC"[^}]*\}' % PRODUCT, css)
    return not faces or any("font-display: block" not in face or "local(" in face for face in faces)


def check():
    from fontTools.ttLib import TTFont

    needed = charset()
    failed = False
    for weight, _ in SUBSETS:
        out = subset_path(weight)
        if not out.exists():
            print(f"missing font subset: fonts/{out.name}", file=sys.stderr)
            failed = True
            continue
        covered = set()
        for table in TTFont(out)["cmap"].tables:
            covered.update(table.cmap)
        missing = {c for c in needed if ord(c) not in covered}
        if missing:
            sample = "".join(sorted(missing)[:20])
            print(f"font subset out of date: fonts/{out.name} is missing "
                  f"{len(missing)} character(s): {sample}", file=sys.stderr)
            print("run: python3 scripts/build_fonts.py", file=sys.stderr)
            failed = True
    if not MARK_FILE.exists():
        print(f"missing font subset: fonts/{MARK_FILE.name}", file=sys.stderr)
        failed = True
    for path in sorted(FONT_DIR.glob("*.woff2")):
        found = reserved_names(path)
        if found:
            print(f"fonts/{path.name} still carries a reserved font name ({found[0]}); regenerate it", file=sys.stderr)
            failed = True
    css = (root / "styles.css").read_text(encoding="utf-8")
    stale = stale_font_urls(css)
    if stale:
        print(f"styles.css has stale font URLs ({', '.join(stale)}); run: python3 scripts/build_fonts.py --stamp", file=sys.stderr)
        failed = True
    stale = stale_preload_pages()
    if stale:
        print(f"{len(stale)} Chinese page(s) lack the current font preloads, first {stale[0]}; run: python3 scripts/build_fonts.py --stamp", file=sys.stderr)
        failed = True
    if song_faces_not_blocking(css):
        print(f'styles.css: every "{PRODUCT} Serif SC" face must be font-display: block with no local() source', file=sys.stderr)
        failed = True
    if failed:
        return 1
    print("site-fonts-ok")
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    sys.exit(check() if "--check" in args else stamp() if "--stamp" in args else regenerate())
