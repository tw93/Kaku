# Kaku Website Agent Guide

This is the website checkout on the `vercel` branch, not the app source tree. Read `DESIGN.md` for design tokens and source/generated ownership before editing.

- Docs update flow (features, Guide, EN/ZH parity, version bumps): `.agents/skills/product-docs`; `.claude/skills/product-docs` links to it.

- `index.html`, `zh/`, and `docs/` contain the site; `vercel.json` owns routing.
- Prose Markdown twins come from `scripts/build_markdown.py`; `index.md` and `zh/index.md` are hand-maintained exceptions. `scripts/build_feed.py` generates the page feed.
- Run the public-facts CI checks: `python3 scripts/build_markdown.py --check`, `python3 scripts/build_feed.py --check`, `python3 scripts/highlight.py --check`, `python3 scripts/check_public_facts.py`, and `python3 scripts/check_sitemap.py` (Pygments is required). These verify generated files, published app facts, and sitemap lastmod/loc rules, not live website deployment. After HTML edits, regenerate markdown and the page feed, and bump `lastmod` in `sitemap.xml`, `sitemap-index.xml`, and `schemamap.xml` to the content date.
- Known canonical leftover: `docs/index.html`, `zh/index.html`, and `zh/docs/index.html` still declare trailing-slash canonicals while `trailingSlash: false` 308s those URLs to the slashless form, so the declared canonical is not the URL that serves.
- Keep English and Chinese counterparts, public version facts, and generated discovery files aligned with the changed source.
- Preserve unrelated website work. Publishing this branch is a website delivery action; do not use app release commands here.
