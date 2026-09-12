# Kaku Website Agent Guide

This is the website checkout on the `vercel` branch, not the app source tree. Read `DESIGN.md` for design tokens and source/generated ownership before editing.

- `index.html`, `zh/`, and `docs/` contain the site; `vercel.json` owns routing.
- Prose Markdown twins come from `scripts/build_markdown.py`; `index.md` and `zh/index.md` are hand-maintained exceptions. `scripts/build_feed.py` generates the page feed.
- Run the public-facts CI checks: `python3 scripts/build_markdown.py --check`, `python3 scripts/build_feed.py --check`, `python3 scripts/highlight.py --check`, and `python3 scripts/check_public_facts.py` (Pygments is required). These verify generated files and published app facts, not live website deployment.
- Keep English and Chinese counterparts, public version facts, and generated discovery files aligned with the changed source.
- Preserve unrelated website work. Publishing this branch is a website delivery action; do not use app release commands here.
