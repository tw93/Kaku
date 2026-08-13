# Theme Setup UI Prototype

> THROWAWAY PROTOTYPE — three variants of the Kaku theme-setup experience,
> switchable with `?variant=A|B|C`. This does not read or write real config.

Run from the repository root:

```bash
python3 -m http.server 4173 --bind 127.0.0.1 --directory kaku/theme-setup-prototype
```

Then open <http://127.0.0.1:4173/?variant=A>.

- **A — Inspector**: dense multi-select list with persistent details.
- **B — Guided**: staged detection, decisions, preview, and results.
- **C — Workbench**: status-oriented lanes for safe setup and ongoing repair.

Use the bottom switcher or Left/Right Arrow to change variants. All state is
in-memory and resets on reload.
