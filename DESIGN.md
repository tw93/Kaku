# Kaku Static Website Design

## Visual Theme

Kaku shares the Kami parchment system used across the family sites (mole.fit and kin) so the apps read as one series. Warm parchment page surface, logo-green accents, serif type, restrained hairline rules, and dark terminal/screenshot surfaces only where the product needs them. Avoid decorative grids, heavy shadows, oversized marketing type, and card-heavy composition.

## Palette

Token names are Kami's, shared by every family site (`python3 ~/www/Kata/scripts/site-tokens.py` checks them); a different value keeps the name.

- `#f5f4ed` parchment page background (`--parchment`)
- `#faf9f5` elevated and card surface (`--ivory`); `#e8e6dc` warm sand (`--warm-sand`)
- `#363433` primary text (`--near-black`, 长石灰 since 2026-10-19; `#141413` read too black); `#3d3d3a` dim (`--dark-warm`), `#6b6a64` muted (`--stone`)
- `#287A3D` the light logo's green for links, tags, outlines and tints (`--brand`); `#2C7D40` hover (`--brand-light`); `#EDF4ED` tint (`--brand-tint`)
- `#2C2F39` the logo disc's slate for solid fills, the primary button and skip link (`--kaku-slate`); `#3E414C` hover (`--kaku-slate-light`). Chosen 2026-10-18 from rendered candidates: the logo's own two colors, not a single purple
- `#0d7d4d` green for command-success accents only
- `#141318` terminal canvas (`--term`); `#e7e4d9` terminal text
- `#d8d5c8` warm line/divider (`--line`); `#e5e3d8` soft line (`--border-soft`); `#edebe1` faint line (`--line-faint`, shared with mole.fit and hina.day)

## Typography

Serif-first, matching the family. English pages use Charter with Georgia / Palatino fallbacks; Chinese pages put the same Latin faces first, followed by `Kaku Serif SC`, a Source Han Serif SC subset shipped in `fonts/` (TsangerJinKai02 until 2026-10-19, replaced for its personal-use-only license and to match the family), then `Source Han Serif SC` and `Songti SC`; iOS has no Chinese serif, so the face must ship. `python3 scripts/build_fonts.py` rebuilds the subset from every page and every queued blog source, plus the one-glyph `Kaku Mark` for the hero's 書, cut from Source Han Serif TC like the siblings' marks, so English pages load 1 KB instead of the whole subset; `--check` runs in CI. Body, headings, and small labels (eyebrows, captions, nav) all share the serif; `--ui` is an alias of `--serif`, and labels are never uppercased. Reading copy is 17px, secondary copy 16px, labels and meta 13 to 15px, with nothing under 12px outside code. At 640px and below the family's phone ladder sets body 16px (docs, articles, prose pages, release notes, the hero line, home paragraphs and ledes, blog group descriptions, FAQ answers), secondary 15px (contact cards, blog card copy, footer), meta 13px (blog card and related dates), so no home text reads larger than the pages it links to; headings, code blocks and desktop sizes keep their values (17px was tried the same day and read too big). The one block lives at the end of `styles.css`. Monospace (`JetBrains Mono`) is for terminal prompts and code snippets. Do not add a font CDN.

## Components

Buttons are pill-shaped (999px radius), 51px tall on desktop and 42px on phones (Kata `site.md`), with a filled slate primary and a slate outline secondary, so a hero pair reads as one color (a green outline beside the slate fill clashed, 2026-10-18). The hero pair leads with `GitHub` (filled, the repository) and keeps `Download for Mac` / `下载 Mac 版` as the secondary, straight to the dmg (maintainer 2026-10-22): Kaku is free and open source, so most visitors read the README before installing; the install guide stays reachable from the nav's Docs, and the one-word label is the maintainer's call over the family's verb-plus-object rule. Cards are parchment blocks with hairline borders and no decorative shadows. Terminal windows keep dark chrome but no extra visual effects.

## Layout

The chrome aligns with the mole-mac (Kami) site so the two feel like one series:

- The family's sticky nav pill (hina.fun, mole.fit): a translucent ivory track with a hairline border and no shadow, the app mark plus near-black wordmark on the left (no version number; maintainer 2026-10-19), Docs and Blog as quiet pill links, the language toggle plus GitHub/X icons on the right. It is CSS only, so every page's shared nav markup stays as is.
- A left-aligned big-serif hero (near-black `Kaku 書` with no entrance effect; the blur-in rise was removed 2026-10-30), a serif tagline, and a single dotted trust row in place of stat and tag clutter. 書 is the family's one Traditional character per product (Mole 鼴, MiaoYan 語, Pake 殼, Hina 賞); a 5px dot in the logo green orbits it, as on every family site, each with its own path, speed and direction and none drawn (Mole 鼴 a plain circle clockwise, Hina 賞 a tilted moon orbit, Pake 殼 a rounded window, Kaku 書 a ticking cursor, MiaoYan 語 a figure eight); Kaku's ticks around a flat ellipse in 30 steps over 12s clockwise. Rejected 2026-10-18: a green pen drawing a stroke under 書 (too much), a floating pencil or any other small icon (ugly), and a green wordmark.
- The gallery follows mole.fit: one framed capture, then a caption on the left (brand-green 17px title, olive 14px upright line naming the command or shortcut, never italic) and 13px pill tabs on the right, the active one filled in `--kaku-slate`. The caption stays on one line beside the tabs on desktop (maintainer 2026-10-19, after a stacked round was rejected): with eight tabs the caption, title included, has about 450px at 1120px and wider, so shorten a line that does not fit rather than letting it wrap or adding a ninth tab (Light was dropped for this). Clicking the left or right half of the capture steps back or forward, as on mole.fit, and a swipe does the same on phones. Panels crossfade in 220ms, and the other captures load once the page is idle, so a click swaps at once. Below 1120px the caption sits above a horizontally scrolling tab row, with the title on its own line above the caption line. The FAQ is the family's details accordion.
- The comparison with iTerm2, Warp, Ghostty and WezTerm is a home section (`.home-compare`, six rows taken from `compare.md`) between Defaults and First steps, linking to `/compare` for the full table; keep its cells in step with `compare.md` and the `zh/` pair. The hero carries no author signature line.
- Sections separated by generous vertical rhythm (96px) rather than divider lines; only the hero keeps a bottom rule.
- The family footer shared in form with hina.day, pake.run and miaoyan.app: mole.fit's dark band in `--kaku-slate` with Mole's text steps (maintainer 2026-10-18, after an ivory round), `Kaku · 書` with a one-line description on the left (never language links, which live only in the nav; maintainer 2026-10-18), and Product / Project / Connect link columns, every label translated on the Chinese pages, on the right. Every page carries the same block.
- Page heads (`.page-hero h1`) use mole.fit's 32px / 500 dark-warm title and a 16px olive lead, on docs, prose pages and the blog alike.

Widths follow mole.fit: `.wide`, `.hero-wrap` and `.hero-centered` are 1120px with 64px gutters (a 992px column), the nav pill 1020px, and blog prose, the changelog and the roadmap fill the 992px column (maintainer 2026-10-21; prose was about 760px and the changelog 860px). Home section titles are 32px dark-warm (26px under 880px) with 16px olive ledes 12px below and no eyebrow above (maintainer 2026-10-23, with mole.fit and Hina: the content under each title already says what the section is, so the label only repeated it; do not restore one; the 404 hero keeps its 15px stone label), the hero keeps mole.fit's 40px title gap and its hairline inside the column, buttons are 51px tall at weight 600 (on phones 44px, each as wide as its label with 24px sides, maintainer 2026-10-23), sections are 84px apart, and docs headings are dark-warm 500 at 22 / 19 / 17px over a 16px body.

Sidebar-less prose pages (`/about`, `/compare`, `/contact`, `/privacy`) reuse the docs chrome, `page-hero doc-hero` plus `article.doc-content`, with `.prose-doc` spanning the same width as the page hero so the intro and body share one right edge. The comparison page stays the durable home for alternative-intent queries.

## Blog

`/blog` and `/zh/blog` follow mole.fit's blog on Kaku's tokens: an index of parchment cards with the cover flush on top, and an article page with the page hero (breadcrumb, title, date and reading time), a cover cropped to 16:7 on desktop and 16:9 on phones, prose filling the page width, one quiet install card (40px app mark, sentence, primary pill) and three "Keep reading" cards; card and related meta are a mono date plus reading time. Search intent picks the topics, and every post opens with the direct answer before Kaku's part.

- **Covers** are illustrations from an image model (the Codex CLI's image tool), not screenshots: matte paper cutouts on `#F5F4ED` parchment, navy `#1B365D`, pale grey-blue `#DCE3EE` and `#EEF2F7`, off-white, at most one small muted green `#0D7D4D` accent, with no text, logos, real UI or window chrome. Keep the subject between y 100 and 800 of the 1600x900 canvas, measured with a threshold mask rather than `-trim` because the paper grain defeats it. Convert with `magick in.png -resize 1600x900^ -gravity center -extent 1600x900` and `cwebp -q 86`.
- **Diagrams** explain a mechanism only. Each one is data in `scripts/build_blog_diagrams.py`, drawn as a desktop layout and a stacked `-mobile` layout in JetBrains Mono, saved at 2x and shown at their own width up to the column, so a narrow diagram is never stretched. Labels stay English and short; the caption carries the explanation in each language.

## Product screenshots

Use genuine, current app output with syntax colors and the real startup marks of Claude Code and Codex. Gallery captures follow Mole's plate procedure (`mole-mac/.claude/rules/site-design.md`): a Kaku test instance at the bundled font and a 1200 × 772 pt window, captured by window id with `screencapture -o -x -l` (2400 × 1544 px, found by the test instance's pid, frontmost so the traffic lights are colored), then composited onto Mole's macOS wallpaper plate `mole-mac/site/img/en/clean.jpg` at `+92+98`, giving 2584 × 1741. Every shot keeps that same window size so the text reads at one scale. Files live in `shots/home/` as `<key>.webp` plus `-1080w` and `-1980w` variants for `srcset`. Gallery images are cached immutably: after replacing an image, run `python3 scripts/build_gallery.py`, which holds the captions and rewrites the gallery block and its `v` content hashes in both languages. The test instance must not reach the maintainer's config, session or relay: unset the `KAKU_*`, `WEZTERM_*` and `TERM_PROGRAM*` variables, point `XDG_CONFIG_HOME` at a scratch config, and drive it with `WEZTERM_UNIX_SOCKET`. Check the product itself while capturing: report awkward layout, clipping, weak contrast, or rough interaction, fix within the authorized scope, then recapture. Never hide product defects through cropping or fabricated output.

## Agent surface

The site serves a machine-readable twin of itself so AI agents do not have to scrape the HTML:

- **Markdown twins.** Every content page has a `.md` sibling. `scripts/build_markdown.py` generates them from the HTML, so never hand-edit a generated `.md`. The two homepage twins, `index.md` and `zh/index.md`, are the exception: they are hand written, because the landing page is marketing layout rather than prose. `vercel.json` serves markdown on `Accept: text/markdown` for `/` and `/zh`, aliases `/llms.md` to `/index.md`, and sets `Vary: Accept`.
- **Page feed.** `scripts/build_feed.py` emits `feeds/pages.jsonl`, one schema.org `WebPage` per page, indexed by `schemamap.xml` and announced by the `Link: rel="describedby"` response header. Keep `Schemamap:` out of `robots.txt`: Search Console flags it as a syntax error, and every sitemap submitted while it was there stayed "Couldn't fetch" (2026-10-15).
- **Release feed.** `scripts/build_releases.py` writes `feed.xml`, `feeds/releases.json`, and the English and Chinese changelog pages from stable GitHub releases. Nightly builds are not included. Do not hand-edit those files.
- **Hand-maintained.** `llms.txt` (plus the scoped `docs/llms.txt` and `zh/llms.txt`), `agent.json` (served at `/?mode=agent`), and `.well-known/agent-skills/index.json`. Keep the version number, CLI surface, and docs list in these in step with the docs when they change.
- **JSON-LD.** The homepages carry a `@graph` with `SoftwareApplication`, `Organization`, `WebSite`, and a `WebPage` with `speakable`. Every other content page carries `BreadcrumbList` plus `TechArticle`. New pages need both, and a `<link rel="alternate" type="text/markdown">` in `<head>`.

Kaku has no public API, SDK, or standalone cloud terminal. Optional remote access connects browsers to a running Mac through a relay. Do not add OpenAPI specs, OAuth metadata, MCP server cards, or `auth.md`: they would describe endpoints that do not exist.

## Verification

Verify with a static server, desktop screenshot, 375px mobile screenshot, and link checking across Chinese and English pages before pushing. Generated files must be current:

```bash
python3 scripts/build_markdown.py --check
python3 scripts/build_feed.py --check
python3 scripts/build_releases.py --check
```

Routing and header behavior (`Accept` negotiation, `?mode=agent`, `Link`, `Vary`) can only be exercised with `vercel dev`, and `vercel dev` ignores the `has` property, so the two `has`-based rewrites are confirmed only on a real deployment.
