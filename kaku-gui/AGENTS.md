# Kaku GUI Agent Guide

`kaku-gui` owns rendering, window lifecycle, user interaction, AI chat, and the `k` helper binary.

## Scope

`kaku-gui` is responsible for:
- terminal window rendering
- input and mouse handling
- tab/pane UI behavior
- startup lifecycle and app event flow
- AI chat overlays, approval UI, inline status, syntax highlighting, and assistant transport
- the `k` chat CLI (`cli_chat::k_main`; `src/bin/k.rs` is the dev bin target)

## Where to Look

- `src/termwindow/mod.rs`: core window state and action dispatch
- `src/termwindow/render/`: rendering pipeline
- `src/termwindow/mouseevent.rs`: mouse and drag behavior
- `src/termwindow/webgpu.rs`: surface resize and present mode handling
- `src/frontend.rs`: app lifecycle and macOS integration events
- `src/overlay/`: launcher and overlays
- `src/overlay/ai_chat/`: AI chat overlay, markdown rendering, syntax highlighting, and Waza integration
- `src/ai_chat_engine/`: AI chat engine, approval, and compaction flow
- `src/ai_client.rs`, `src/ai_remote.rs`, `src/ai_tools/`: provider and tool integration. Keep tool-name strings stable so persisted conversations replay correctly.
- `src/ai_tools/paths.rs`: sandbox and sensitive-path guards.
- `src/ai_tools/fs.rs`: read, list, write, patch, mkdir, and delete tools.
- `src/ai_tools/shell.rs`: exec, background execution, and polling.
- `src/ai_tools/web.rs`: search, fetch, and URL reading.
- `src/ai_tools/search.rs`: grep and symbol search.
- `src/ai_tools/project.rs`: project summaries and file tree helpers.
- `src/ai_tools/soul.rs`: memory and soul reads.
- `src/ai_tools/registry.rs`: `ToolDef`, `all_tools`, and `to_api_schema`.
- `src/cli_chat/`: command-line chat flow
- `src/thread_util.rs`: macOS worker thread lifetime helpers
- `src/tabbar.rs`: tab bar behavior

## Practical Rules

- Keep rendering decisions in rendering modules, not scattered across event code.
- Minimum-contrast correction must preserve Half intensity when it replaces a dim foreground. Reapply dim only to the replacement color; do not dim an unchanged foreground twice or apply it to selection and block-cursor foreground overrides.
- Paint-time Kaku TUI detection must reject non-Kaku foreground executables using the cached leader name before requesting the full process tree. Keep the executable and argument confirmation for `kaku ai` and `kaku config`; cover pane, window and tab-bar consumers together (#583).
- Keep initial window dimension math in `TermWindow::new_window()` consistent with `get_os_border()` behavior.
- In-app update must confirm before replacing the running app. User-facing paths (menu Check for Updates, menu Restart to Update, toast click) go through `check_for_updates_from_menu()` / `confirm_and_apply_update()` in `src/frontend.rs`, not straight to `restart_to_update()`. Exploratory menu check opens a tab without `KAKU_UPDATE_AUTO_CONFIRM`; only the overlay-confirmed path may auto-confirm. CLI direct and brew paths confirm in `kaku/src/update.rs`. Guards: `frontend::tests::user_facing_update_events_route_through_confirm` and `update::imp::tests::brew_and_direct_paths_confirm_before_replace`.
- Keep `KAKU_CONFIG_CHANGED` fast path in `emit_user_var_event()` before pane ownership checks.
- Pass fullscreen state into `TabBarState::new()` and preserve fullscreen-specific title button spacing behavior.
- Keep AI overlay state changes cheap enough for live terminal interaction; avoid blocking render/input loops on provider calls.
- Preserve approval prompts and inline AI status placement when touching AI chat or shell flows.
- Chat permission summaries retain the complete target and command, with terminal controls escaped. The native review pane scrolls through all details and refuses approval when the pane is too small to read. Notifications may shorten their preview; the approval payload must not. Chat rendering, picking and mouse hit-testing share `msg_area_height`, `picker_height` and `visible_start`. Preserve a scrolled reader's position as output grows. Code Copy uses the original block, never wrapped display rows; Insert only pastes a completed, control-free single shell line into the original shell pane and never sends Enter.
- AI HTTP transport must preserve both sides of proxy behavior: external API hosts should honor detected system proxy settings, while loopback, private LAN, link-local, CGNAT/Tailscale-style, `.local`, `NO_PROXY`, and macOS ExceptionsList model endpoints should bypass the proxy and connect directly. Do not describe this as full SOCKS support unless reqwest SOCKS support is actually enabled and smoke-tested. Exception: the `http_request` tool always connects directly to the public IP addresses resolved and validated before approval, for both HTTP and HTTPS. It must never pass that hostname to a proxy for independent resolution; users on proxy-only networks receive a connection error instead of weakening the SSRF boundary.
- Wrap macOS worker thread spawns in the existing autorelease-pool helper so shutdown does not reintroduce use-after-free crashes.

- Remote pane operations in `src/remote_tabs.rs` retain a stable `PaneId`. Do not send a remote split through `spawn_command_impl`, which resolves the active pane again after queuing. Browser-confirmed closes must bypass `AlwaysPrompt` without switching to another pane, and preserve the last-pane tab restore bookkeeping. The isolated remote live smoke covers both paths.

## Known Pitfalls

- During window drag, terminal move/wheel events may need suppression.
- A tab drag belongs to one press/release gesture. A new press or motion without the left button exclusively held cancels stale state, including when AppKit swallowed the previous release. Preserve the ordinary release handler for settle animation. For unexplained tab reordering, `debug_key_events` records tab press geometry, motion and reorder decisions without terminal text (#584).
- WebGPU surface reconfigure should happen only on meaningful state changes.
- Finder "open with" behavior is handled in `frontend.rs` event flow.
- Option+Click cursor movement fires only when: no mouse grab, no alt screen, click is on the same row as the cursor. It sends Left/Right arrow sequences proportional to the column delta; see `mouseevent.rs`.
- Block cursor height uses `natural_cell_height` (ignoring `line_height`) so Nvim visual selections match WezTerm proportions.
- Top border clearance for macOS integrated buttons is state-sensitive:
  - top tab bar visible -> add small gap
  - top tab bar hidden or bottom tab bar -> add larger clearance for traffic lights
- Overlay geometry must update when panes are split, resized, closed, or moved.
- Alternate-screen wheel behavior is config-controlled; do not hard-code one terminal behavior across normal and alternate screens.
- Inline `#` AI query status belongs below the buffer, not as a toast, so terminal content remains stable.
- `fast_model` and proxy config are part of the AI UX contract; verify both when changing provider selection or transport.
- Per-pane overlays (AI chat) render in place of their underlying pane and live in `pane_state`, not the tab's pane tree. They have no window handle, so their output only repaints the window through `PaneOutput` -> `mux_pane_output_event` -> `is_pane_visible` -> `window.invalidate()`. `is_pane_visible` (`termwindow/mod.rs`) MUST treat an active per-pane overlay pane as visible, and the overlay's `render()` MUST `term.flush()`, or streamed output and the loading spinner freeze until an input event forces a repaint. Symptom-to-cause: if `top` refreshes on its own but the AI chat needs a click, the break is one of these two.
- Scrollback viewport pruning has two policies in `termwindow/mod.rs`: passive pruning during output uses `normalize_viewport` and snaps to the bottom (`None`), while explicit user-driven scroll requests use `normalize_interactive_viewport` and clamp to `scrollback_top`. During a selection drag, only the pane named by `MouseCapture::TerminalPane` may pin a pruned viewport; sibling panes must retain the passive #448 behavior.
- Tab bar rendering and the tab rename editor must read `resolved_palette.tab_bar`, which includes the selected color scheme and user overrides. Reading `config.colors.tab_bar` alone ignores scheme colors and makes Fancy tabs unreadable in light themes.
- Tab bar sizing and hit-testing have pure seams in `src/tabbar.rs`: `tab_width_budget` (per-tab column budget that drives truncation) and `is_tab_hover` (clickable span). The truncation and position regressions (#439/#443/#445) came from layout logic with no test seam, so when you change tab sizing, truncation, or click regions, extend their unit tests instead of only eyeballing the running app. `tab_width_budget` locks the invariant that a tab is never squeezed below 1 column. Retro hover columns come from window pixel coordinates via `MouseInputState::tab_bar_mouse_column`, matching the bar's zero x origin; never reuse terminal `last_mouse_coords`, which includes content padding and selection rounding (#626).
- Launcher entries that target a pane must retain its stable `PaneId`, not a tab or topological pane index. The launcher snapshot can outlive pane exits and split-tree reindexing; resolve the `PaneId` against the live mux only after the overlay closes.
- Session snapshots skip a pane's content while it is in the alternate screen (`f4aca502`), so a vim or htop screen never lands in restored history. Claude Code and Codex are the exception (`foreground_is_agent_cli` in `tabbar.rs`): their last screen is kept, since it is the conversation. Restore never re-runs the program.
- A partially failed startup session restore must preserve the original session envelope and scrollback sidecars. Only mark a snapshot consumed after every saved window restores successfully; otherwise exit-time saving can overwrite the only complete copy.
- Session sidecars compact vector-backed lines before checking visible images, since wide-character continuation cells can retain hidden Kitty placements. Strip remaining images and recompress before encoding. Menu restore guards preserve prior dirty state on failure and carry successful changes through overlapping restores; startup-only restores remain pristine. Exit and update checkpoints capture cwd/output even without structural changes, only in the session-owning process and never during an incomplete restore. SIGTERM requests main-thread event-loop shutdown, then uses the normal exit checkpoint; its listener remains registered through repeated TERM signals. Register it only after existing-instance handoff, never in CLI-only commands (#579).
- The terminal grid's top origin has one source: `TermWindow::terminal_first_row_offset()` (OS top inset plus a shown top tab bar). Pane backgrounds, split lines and mouse row mapping take it from there so they match where text is drawn; a background that skipped the 6px inset under a top tab bar sat above its split line (#562). A source guard in `render/split.rs` fails if `pane.rs`, `split.rs` or `mouseevent.rs` computes its own. `update_text_cursor` (IME candidate rect) still uses a separate origin and sits `border.top` high; that is a known, unfixed gap.
- Window padding (2026-10-18): with managed padding (no `config.window_padding` in the user's file) every vertical gap derives from the top padding S in `effective_vertical_padding_with_policy`: content keeps S below window chrome and S/2 from the bottom edge or a visible tab bar, top or bottom, fullscreen included; only a maximized window without a bottom bar runs content to the bottom edge. Rows rarely fill the height, and `padding_left_top` splits that leftover evenly between top and bottom (`split_row_slack`, inside the OS borders), so the gap to the bar no longer jumps between 1pt and a full row. Every consumer (pane rendering, split lines, mouse hit testing) reads `padding_left_top`; do not add a second slack policy in `resize.rs` (the old rebalance there only ever reached the resize increments). The tab bar itself stays one terminal row. Explicit user padding is used as written. Hotkey windows are also excluded from `split_row_slack`: their drawer policy owns the top inset, including an explicit zero. Cover hotkey, ordinary and fullscreen states when changing slack allocation.
- Split geometry follows mux: `split.top = first.rows + gutter / 2` and the pane backgrounds meet at `first.rows + gutter / 2.0`. With the bundled even row gutter (`split_pane_gap = 2`) the line belongs on the top edge of row `split.top`, with an odd gutter on its middle; `split_row_center_offset` places it and `split_drag_row` measures drags against the drawn line so a grab does not jump a row. Left|right splits always have an odd column gutter. In opaque windows the outer panes' backgrounds fill the right and bottom padding (transparent windows leave those strips to the fill strips in `paint.rs`). With `inactive_pane_hsb` dimming on, the split line takes the active pane's background, so the dimming marks focus and the line only separates two dimmed neighbours; do not remove the line, three-pane layouts need it.
- Touch mode (`termwindow/touch.rs`) is per-window UI state, not a persisted config override. It changes nothing but a drawn close button on each retro tab: tab bar height, desktop tab metrics and terminal font/line height stay the same. Long press works in every mode: `track_long_press` follows a plain left press and, on a release at least 450ms later within 24pt of the press (8pt cancelled ordinary presses over UU) (motion and the release point are both checked), replays it as a right click after the event finishes (on release, so AppKit menu tracking never sees the lift as a pick). The menu it opens keeps the system font and gives each item a 30pt title line (`LONG_PRESS_MENU_ROW_PT`, rows measured 36pt against the system 24pt; 42pt read as too large) through `Menu::set_row_height`, a fixed paragraph line height with the baseline raised to center the title; the earlier 18pt font (0bed9c94) grew the text, which looked coarse over UU, while rows barely followed it, and an item image does not grow a row at all. A mouse right click keeps the system size. A modal clears the pending press, so dismissing the rename editor never opens a menu, and a press that reached a tab through the bottom bar touch band aims its right click at that tab. A press over a pane that grabs the mouse is never converted unless it is on a tab, and the original press and release are handled as usual.
- The bottom tab bar touch band (`mouseevent.rs`) is the padding strip between the last row and the bar, plus `BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT` of the last row only while Touch Mode is on (`bottom_tab_bar_touch_extension_px`, #606: with a mouse, a click on the lower half of the last row switched tabs). It accepts only a plain left press (`press_takes_touch_extension`) and then owns that gesture's motion and release (`touch_band_press`). A middle press there is a paste into the last row and must never reach the tab bar's middle-click close. Known trade-off in Touch Mode: a left tap in the lower 12pt of the last row over a tab goes to the tab even when a TUI (tmux status line, htop) grabs the mouse.
- Agent title (2026-10-18): while Claude Code, Codex or Kimi runs in the active pane (`foreground_agent_cli` in `src/tabbar.rs`), `src/termwindow/render/agent_title.rs` draws the agent's mark and the topic from its OSC 0/2 title at the top right: centered on the traffic-light line in the integrated-buttons strip, or 16pt below the top edge when fullscreen (the same distance as in a window, lifted only to stay above the first row), never under a top tab bar. `show_agent_title = false` (Agent Title in `kaku config`) turns it off; the default is on. Spinner glyphs and a leading agent name are stripped so the text stays still. When the bottom of the pane already draws the exact topic inside a rule (Claude Code's named-session border, `rule_carries_topic`), the window and the page's bar keep only the mark, while the page's tab list and browser title keep the topic; without that border, or when it differs, the text stays, since the strip is then the only place the topic shows (2026-11-09). Strip a name only at a complete boundary (end, whitespace, colon, pipe or middle dot), preserving topics such as `Codexification` and `Claude Codebase`; do not retry a shorter name after a longer partial match. The tab title is untouched on purpose: a tab is too narrow and would change width with every update. The marks in `src/termwindow/render/agent_marks.rs` are generated from Lobe Icons SVGs by `scripts/svg_to_poly.py`; regenerate rather than hand-edit. The remote page shows the same title: `tabbar.rs` publishes each tab's agent and topic through `kaku_remote::publish_agent_title` (honoring `show_agent_title`), and `AGENT_MARKS` in `crates/kaku-remote/assets/web/index.html` holds the same paths as SVG data on the 120 grid, so a regenerated mark goes into both files.
- Tests that call `config::set_config_file_override` and `config::reload()` can race the config file watcher: `reload()` returns early when another reload starts, leaving the previous config. Write the config as few times as possible inside one test (loop the cheap cases inside one write), and do not split such a test into siblings that run in parallel.

## Cross-References

- [`mux/AGENTS.md`](../mux/AGENTS.md) - Tab and pane abstractions consumed by GUI.
- [`termwiz/AGENTS.md`](../termwiz/AGENTS.md) - TUI primitives used in overlays.
- [`config/AGENTS.md`](../config/AGENTS.md) - Config loading and reload signals.
