"""Rewrite the homepage gallery block in index.html and zh/index.html.

Run after adding, removing or replacing a capture in shots/home/: it refreshes
the captions, tabs and the ?v= content hashes in both languages.
"""
import hashlib, html, pathlib, re
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
SHOTS = [
    ("ai", "AI coding", "AI 编程",
     "Claude Code at work, its topic shown at the top right",
     "Claude Code 在项目里干活，窗口右上角显示它正在做什么"),
    ("panes", "Panes", "分屏",
     "Press Cmd + D to split, and tabs sit along the bottom",
     "按 Cmd + D 分屏，标签排在窗口底部"),
    ("assistant", "Kaku AI", "AI 聊天",
     "Press Cmd + L to ask, then copy or insert the command",
     "按 Cmd + L 直接问，回答里的命令可以复制或插入"),
    ("commands", "Commands", "一句话命令",
     "Type ## and a sentence, then pick a command",
     "输入 ## 加一句话，给出几条命令供你挑"),
    ("remote", "Remote", "远程",
     "Run kaku remote to continue in a browser",
     "运行 kaku remote，在浏览器里接着用 Mac 上的会话"),
    ("files-dark", "Files", "文件管理",
     "Press Cmd + Shift + Y to browse files and code with Yazi",
     "按 Cmd + Shift + Y 用 Yazi 浏览文件和代码"),
    ("settings", "Settings", "设置",
     "Run kaku config to change fonts, themes and windows",
     "运行 kaku config 调整字体、主题和窗口"),
    ("cli", "CLI", "命令行",
     "Run kaku for chat, AI setup, checkups and updates",
     "运行 kaku，聊天、AI 配置、自检和更新都在这里"),
]


# Phones get the same capture cropped to the window plus a 16px margin (the window sits at
# +92+98 and is 2400 x 1544 px on the 2584 x 1741 plate), so the terminal text is not
# shrunk by the wallpaper around it. Nothing inside the window is cut.
PHONE_BOX = (76, 82, 2508, 1658)
PHONE_WIDTHS = (800, 1200)


def make_phone(key):
    src = Image.open(ROOT / "shots/home" / f"{key}.webp").convert("RGB").crop(PHONE_BOX)
    w, h = src.size
    for width in PHONE_WIDTHS:
        src.resize((width, round(h * width / w)), Image.LANCZOS).save(
            ROOT / "shots/home" / f"{key}-phone-{width}w.webp", "WEBP", quality=82, method=6)


def ver(name):
    data = (ROOT / "shots/home" / f"{name}.webp").read_bytes()
    return hashlib.sha256(data).hexdigest()[:12]


def block(lang):
    zh = lang == "zh"
    panels, tabs = [], []
    for i, (key, en_tab, zh_tab, en_line, zh_line) in enumerate(SHOTS):
        tab, line = (zh_tab, zh_line) if zh else (en_tab, en_line)
        v = ver(key)
        make_phone(key)
        pv = ver(f"{key}-phone-1200w")
        base = f"/shots/home/{key}"
        srcset = f"{base}-1080w.webp?v={v} 1080w, {base}-1980w.webp?v={v} 1980w, {base}.webp?v={v} 2584w"
        phone = ", ".join(f"{base}-phone-{w}w.webp?v={pv} {w}w" for w in PHONE_WIDTHS)
        first = i == 0
        load = 'fetchpriority="high"' if first else 'loading="lazy"'
        state = ' is-active' if first else ''
        hidden = '' if first else ' aria-hidden="true"'
        panels.append(
            f'          <figure class="gallery-panel{state}" id="shot-{key}" role="tabpanel" aria-labelledby="tab-{key}"{hidden}>'
            f'<picture><source media="(max-width: 640px)" srcset="{phone}" sizes="calc(100vw - 32px)" width="2432" height="1576">'
            f'<img src="{base}-1980w.webp?v={v}" srcset="{srcset}" '
            f'sizes="(max-width: 1128px) calc(100vw - 32px), 1000px" width="2584" height="1741" {load} decoding="async" alt="{html.escape(line)}"></picture></figure>')
        sel = 'true' if first else 'false'
        ti = '' if first else ' tabindex="-1"'
        cls = ' class="is-active"' if first else ''
        tabs.append(
            f'            <button id="tab-{key}" type="button" role="tab"{cls} aria-selected="{sel}" aria-controls="shot-{key}"{ti} '
            f'data-title="{html.escape(tab)}" data-line="{html.escape(line)}">{html.escape(tab)}</button>')
    label = "Kaku 截图" if zh else "Kaku screenshots"
    tablabel = "选择一张 Kaku 截图" if zh else "Choose a Kaku screenshot"
    first_tab, first_line = (SHOTS[0][2], SHOTS[0][4]) if zh else (SHOTS[0][1], SHOTS[0][3])
    return (
        f'<section class="section showcase-section" aria-label="{label}"><div class="wide">\n'
        f'      <div class="gallery" data-gallery>\n'
        f'        <div class="gallery-frame">\n' + "\n".join(panels) + "\n        </div>\n"
        f'        <div class="gallery-caption" aria-live="polite"><p class="title" data-gallery-title>{html.escape(first_tab)}</p>'
        f'<p class="line" data-gallery-line>{html.escape(first_line)}</p></div>\n'
        f'        <div class="gallery-tabs" role="tablist" aria-label="{tablabel}">\n' + "\n".join(tabs) + "\n        </div>\n"
        f'      </div>\n    </div></section>')


for lang, rel in (("en", "index.html"), ("zh", "zh/index.html")):
    p = ROOT / rel
    s = p.read_text(encoding="utf-8")
    new, n = re.subn(r'<section class="section showcase-section".*?</section>', lambda m: block(lang), s, count=1, flags=re.S)
    assert n == 1, rel
    p.write_text(new, encoding="utf-8")
    print("rewrote", rel)
