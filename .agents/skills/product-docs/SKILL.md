---
name: product-docs
description: "Keep the Kaku website (kaku.fun, this vercel-branch checkout) accurate and user-facing: gather verified features from the app's current commit, refresh the docs and the plain-language Guide, keep EN/ZH parity, bump version pointers on release, and verify before handing the diff to the maintainer."
when_to_use: "update website, update docs, 更新官网, 完善官网, 产品说明, 使用文档, 上手指南, product guide, document a feature, refresh kaku.fun, write user docs, site docs, 写文档, 写官网"
---

# Kaku Product Docs

Use this skill to write or update the user-facing product documentation on the Kaku website. The site is the deliverable; this skill is the repeatable flow for keeping it correct and adding the next feature without drift.

The audience is normal users, not contributors. Explain what each page/button/shortcut **is**, how to **reach** it, and what happens **after** you use it. Avoid implementation detail. No em dashes, no emoji.

## Where things live

- **This checkout** is the `vercel` branch; Vercel serves `kaku.fun` from it. The app source is the `main` worktree of the same repository (`git worktree list`, usually `~/www/kaku`); read feature facts there.
- **Design system**: Kami parchment system. Read `DESIGN.md` before any visual change. Match the existing pages; do not invent new components.
- **Doc pages** (EN under `docs/`, ZH under `zh/docs/`, kept in lockstep):
  Install (`index.html`), Guide (`guide.html`), Features (`features.html`), CLI Reference (`cli.html`), Configuration (`configuration.html`), Keybindings (`keybindings.html`), FAQ (`faq.html`), Contributing (`contributing.html`).
- **Compare** lives at `/compare` and `/zh/compare`, same chrome as About. That is the home for iTerm2 / Warp / Ghostty / WezTerm / Terminal.app questions. Do not add a Mole-style `/blog` tree unless there are several dated essays to publish.
- **Guide vs Features**: Guide is the narrative onboarding walkthrough (first launch, then tabs/panes, shell, AI, tools, settings). Features is feature-by-feature reference. Keep the Guide short and link out to the reference pages; do not duplicate config tables there.
- **Screenshots**: `shots/kaku-dark.webp` and `shots/kaku-light.webp` (1920x1192). Reuse them with `<figure class="shot">`. Kaku is a native terminal, so CDP/browser screenshots do not apply to the app; only capture new app shots if the maintainer explicitly asks (build `make app` in the app worktree or use `/Applications/Kaku.app`, then `screencapture`).

## Source of truth: verify, never infer

User-facing docs are public. Do not copy feature claims from a subagent summary or from a feature's name. Confirm each claim against the app worktree before publishing:

- Defaults and behavior flips: grep `config/src/config.rs` (and its tests, e.g. `*_defaults_to_*`) and bundled config in `assets/`. Example caught this way: v23 flipped `smart_tab_mode` default to `suggestion_first`; the site still said `completion_first`.
- Shell behavior: `assets/shell-integration/setup_zsh.sh`.
- Keybindings / menus: the existing `keybindings.html` is the maintainer's authored ground truth; reuse its wording rather than re-deriving.
- The existing site docs are authoritative for tone and naming. When you add something new, verify it; when you echo something existing, match it.

## Workflow

1. **Scope**: read `git log`/`gh release list` for the current version and what changed. Read the app worktree's nearest crate `AGENTS.md` for feature notes (e.g. `config_version` changes).
2. **Inventory the site**: read the target page(s) raw to clone exact markup. Note `site-nav`, `docs-sidebar > docs-menu`, `section-toc`, `doc-content`, `doc-pager`, `footer`. ZH footer/skip-link wording differs from EN; copy the ZH variants verbatim from an existing ZH page.
3. **Write/edit content** in plain language. New page: clone an existing doc page's full scaffold, change `<title>`/`description`/`canonical`/`hreflang`/hero/`docs-menu` current item/`section-toc`/`doc-pager`. Anchors use a `page-` prefix (`guide-...`).
4. **Wire navigation** (a new page touches every doc page):
   - Insert the new `<a href="/docs/<page>"><span>Label</span></a>` into the `docs-menu` block of all EN doc pages, and the `/zh/docs/...` variant into all ZH pages. The page where the neighbor link carries `aria-current="page"` needs a separate edit (the attribute breaks a naive anchor match).
   - Fix the `doc-pager` prev/next chain on the two neighbor pages only.
   - Add both URLs to `sitemap.xml` and a line to `llms.txt` (and `llms-full.txt` if it enumerates pages). A new page also needs the `<link rel="alternate" type="text/markdown">` head tag (`DESIGN.md`).
5. **Version pointers** (only when a release shipped): bump the current-version pointer, not historical references.
   - Bump: nav `>v0.X.Y</a>` (every page), JSON-LD `"softwareVersion"` (both `index.html`), AppleScript `get version` example, `llms-full.txt` `Latest version:`, and the version in `agent.json`.
   - Leave alone: roadmap narrative ("V0.X.0 is out", "after V0.X.0") and any "this version added" history.
   - Confirm the release is actually public first (`gh release list`), or the site will document an unreleased default.
6. **Verify before declaring done**: run the checks listed in `AGENTS.md` (regenerate markdown and feed, then every `--check`, `check_public_facts.py`, `check_sitemap.py`), plus:
   - Internal links resolve under `cleanUrls` (every `/docs/x` has `docs/x.html`); new/edited HTML is well-formed.
   - EN and ZH parity: same sections, same anchors, same tables.
   - Browser screenshot of each changed page at desktop and 375px (`DESIGN.md`). On a machine without Chromium, serve a temp root (`cp docs/<page>.html /tmp/prev/index.html`, symlink `styles.css`, `shots`, `img` into it, `python3 -m http.server <port> --directory /tmp/prev`), open it in the preview browser, and restart the preview between pages because it caches the loaded page.
7. **Hand off**: show the diff. Do NOT commit or push the `vercel` branch unless the maintainer says so this turn. Pushing `vercel` deploys to production immediately.

## Adding the next feature later

- Decide the home: a daily-use behavior becomes a bullet/step in the **Guide**; a configurable surface or new tool becomes a section in **Features** (plus Keybindings/Configuration if it adds a shortcut or option).
- Always update EN and ZH together. Mirror anchors and tables exactly.
- Re-run the verify checklist. Keep diffs minimal and atomic per behavior.
