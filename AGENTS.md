# Kaku Website Agent Guide

This is the website checkout on the `vercel` branch, not the app source tree. Read `DESIGN.md` for design tokens and source/generated ownership before editing.

- `index.html`, `zh/`, and `docs/` contain the site; `vercel.json` owns routing.
- Prose Markdown twins come from `scripts/build_markdown.py`; `index.md` and `zh/index.md` are hand-maintained exceptions. `scripts/build_feed.py` generates the page feed.
- Verify changed public facts against the published app and check generated outputs; generator success does not verify live pages.
- Keep English and Chinese counterparts, public version facts, and generated discovery files aligned with the changed source.
- Preserve unrelated website work. Publishing this branch is a website delivery action; do not use app release commands here.
