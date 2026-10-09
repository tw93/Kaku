# Changelog

Stable Kaku releases, newest first. Nightly builds stay on GitHub. Subscribe with [RSS](https://kaku.fun/feed.xml).

## V0.22.0 Handy 2026-10-05

1. **Tab Bar**: Tabs are easier to hit with a finger over remote desktop, a + button opens new tabs, and right-clicking a tab offers New Tab, Tab Navigator, and Close Tab.
2. **Typing Lag**: Panes that once showed inline images no longer freeze typing each time Kaku saves the session in the background.
3. **App Titles**: Set `config.tab_title_use_pane_title = true` to show the titles apps like Claude Code set, while tabs you renamed keep your name.
4. **Kitty Keyboard**: With `enable_kitty_keyboard` on, Escape and forward Delete work in Herdr and Neovim, and swapping Backspace and Delete applies there too.
5. **Input Methods**: Set `config.ime_preedit_rendering = 'BuiltinInsert'` to show the text you are composing as inserted text instead of covering the next character.
6. **Powerlevel10k**: Kaku no longer prints anything before the first prompt, so Powerlevel10k instant prompt stops warning.
7. **Command Line**: `kaku start` now opens in the Kaku that is already running instead of launching a second copy, and `--new-tab` adds a tab to the current window.
8. **Split Panes**: The line between top and bottom panes now sits where their backgrounds meet, and with inactive panes dimmed it blends into the background while the dimming reaches the window edges.
9. **Diagnostics**: `kaku doctor` saves a redacted diagnostic bundle you can attach to a bug report.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.22.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.22.0/Kaku.dmg)

---

## V0.21.0 Ink 2026-09-26

1. **Official Homebrew Cask**: Kaku is finally in the official Homebrew cask, so `brew install --cask kaku` installs it directly, `kaku update` recognizes it, and installs from the personal tap keep updating too.
2. **Session Recovery**: Kaku now saves your windows, tabs, and panes while it runs, so a crash or force quit still brings back the last layout.
3. **Context Menu**: Copy appears at the top of the right-click menu when text is selected, so copying with the mouse works even with copy on select turned off.
4. **Tab Bar**: Tabs split into several panes now show their index number too.
5. **Themes**: New installs start in Kaku Dark while existing setups keep their current theme, and bright cyan in Kaku Light is easier to read.
6. **Stability and Security**: A tab that is closing can no longer take every window down, and the TLS library is updated for a security advisory.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.21.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.21.0/Kaku.dmg)

Special thanks to @TeamMeng for their contribution to this release.

---

## V0.20.0 Steady 2026-09-12

1. **Window Behavior**: Native macOS dragging and edge tiling work again, closing windows no longer triggers a delayed repaint crash, and new windows skip repeated waits after graphics initialization fails.
2. **Shell Integration**: Returning to the prompt clears stray mouse reports after a TUI exits unexpectedly, tmux keeps its prompt and Smart Tab, and fish grep completion expands correctly.
3. **AI Model Selection**: Switching providers no longer reuses the previous provider's model list, all returned models are shown, and manually configured models remain available.
4. **AI Chat**: Chats include local project type information without probing local projects during remote sessions, and cancellation interrupts retry waits promptly.
5. **Context Menu**: Right-click to paste, search, open AI chat, or manage panes, with an option in Settings to show the new-tab button.
6. **Theme Settings**: Standalone configurations can use the built-in light and dark themes, Settings saves to the custom configuration used at launch, manual color overrides are shown explicitly, and Fancy tabs use the selected theme.
7. **Link Detection**: Unrelated text after a hard newline stays out of link targets while automatically wrapped URLs keep their complete addresses.
8. **Version Reporting**: The GUI executable reports its package version without initializing a window.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.20.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.20.0/Kaku.dmg)

Special thanks to @elonnzhang, @trxuan, and @yansigit for their contributions to this release.

---

## V0.19.0 Restored 2026-08-24

1. **Pane Input Broadcast Removed**: It was too easy to trigger by accident and could repeat a risky command in unrelated panes, so existing key assignments now do nothing.
2. **Sessions Restore More Completely**: Reopening Kaku brings back your windows, their panes, and each pane's directory, and one pane that fails to save no longer costs you the rest.
3. **Closing No Longer Hits the Wrong Pane**: A close confirmation stays tied to the pane it belongs to, and closing the active tab leaves you on the expected one.
4. **Display and Integration Fixes**: Light-theme selections stay visible, lazygit works in nested shells, clearing history leaves full-screen programs intact, tab renaming no longer freezes titles, and slow synchronized output stops tearing.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.19.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.19.0/Kaku.dmg)

Special thanks to @shlroland and @dufu1991 for their contributions to this release.

---

## V0.18.0 Detached 2026-08-08

1. **Move a Tab to Its Own Window**: Pull the current tab, and every pane inside it, into a new window from the command palette or the Window menu.
2. **Windows Stay Below the Menu Bar**: Dragging between displays no longer parks a title bar behind the menu bar where the cursor cannot reach it, on side-by-side and stacked arrangements alike.
3. **Updates Ask First**: Restarting from the menu and upgrading through Homebrew both confirm before they act.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.18.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.18.0/Kaku.dmg)

---

## V0.17.0 Linked 2026-08-02

1. **Responses API and Native Search**: Choose `api_mode = "responses"` for Responses-compatible endpoints, and turn on provider-hosted web search without a separate search key.
2. **Pick Your Managed Shell**: `kaku init` can install zsh or fish on purpose, and Kaku keeps that choice across XDG config paths instead of only trusting `$SHELL`.
3. **Richer Terminal Clicks**: Cmd+Click opens bare domains like `github.com`, file links can launch your editor via `config.file_link_editor`, and Option+Click moves the cursor inside the current input line.
4. **Safer AI Tools**: Tool paths, web requests, and code search are harder to abuse; incomplete streams no longer look like successful turns.
5. **Stability Fixes**: Launch no longer dies on oversized draw batches, windows stay below the menu bar, top tabs line up with traffic lights, font fallback is safer, and shell state plus drag-select scroll behave more predictably.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.17.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.17.0/Kaku.dmg)

Special thanks to @ddotz and @F1Justin for their contributions to this release.

---

## V0.16.0 Trusted 2026-07-26

1. **Inline AI Uses Your Provider**: The `#` prompt and the automatic quick-fix now go through the same transport as Cmd+L, so Codex, Copilot, and API-key setups all just work.
2. **Authenticated Controls**: Shell control messages now carry a local capability, so terminal output can no longer borrow your configured credentials to fire assistant requests.
3. **Close Tabs From The Navigator**: Press Backspace on a highlighted tab to close it, with the same confirmation you get everywhere else.
4. **Display Fixes**: Titlebar spacing follows the display, AI chat picks up a live appearance switch, overlays resize after a pane split, and link underlines survive hover.
5. **Fixes**: Starship setup stays inside Kaku instead of taking over your other terminals, white backgrounds stay readable, and menu commands run the bundled CLI.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.16.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.16.0/Kaku.dmg)

Special thanks to @zwrong and @mortalYoung for their contributions to this release.

---

## V0.15.0 Connected 2026-07-18

1. **Remote Tabs**: Tabs connected over ssh now show a dedicated icon with the host name, split tabs reveal which pane is remote, and mosh, autossh, and et sessions are recognized too.
2. **AI in SSH Sessions**: AI chat now understands when the current directory lives on a remote host, stops running local commands against it, and answers from the terminal context instead.
3. **SSH Everywhere**: The fish integration keeps your own ssh function and gains the 1Password fix, env-prefixed ssh aliases work again, and mosh gets the same terminfo fallback as ssh.
4. **Session Restore**: Restored ssh panes return to their remote working directory, and when a window cannot come back Kaku says how many were kept and retries them on the next launch.
5. **Fixes**: Text selection clears right after Ctrl+L instead of lingering, Cmd+, focuses the existing settings window instead of stacking new ones, and AI shell commands work for fish users again.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.15.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.15.0/Kaku.dmg)

Special thanks to @shlroland, @mortalYoung, and @darion-yaphet for their contributions to this release.

---

## V0.14.0 Focused 2026-07-11

1. **Pane Navigation**: Tab Navigator now lists every pane inside split tabs and lets you jump directly to the one you need, while narrow tab titles keep the active pane visible.
2. **Tab Renaming**: The rename dialog has clearer text and cursor behavior, canceling with Escape or the mouse no longer leaves a phantom selection, and automatic split-pane titles stay intact.
3. **Tabs & Selection**: Tab separators are cleaner, selected text is easier to see in dark themes, and double-click selection now respects boundaries between CJK and Latin text.
4. **Session Restore**: Closed tabs stay closed after restart, incomplete restores preserve the original recovery data, and SSH sessions can be restored from saved snapshots again.
5. **Security & Stability**: Dependency advisories for `anyhow` and `crossbeam-epoch` are resolved, while pane actions and mouse releases handle disappearing UI state more safely.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.14.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.14.0/Kaku.dmg)

---

## V0.13.0 Faster 2026-07-05

1. **Startup & Prompt**: Kaku now does less work before the first frame and keeps shell, config, and font caches warm, so new windows and prompts feel faster.
2. **Default Prompt**: The bundled prompt now shows Git status and Node version while keeping the same compact terminal feel.
3. **Tab Titles**: Tabs stay path-focused by default, with an optional Command Tab Titles setting for showing running tools such as `project·claude`; split panes also keep their titles readable.
4. **AI Chat**: Long streaming conversations no longer re-highlight the whole chat on every update, keeping the AI panel smoother during large sessions.
5. **Window & Terminal Stability**: Moving between displays, half-intensity text, resizing terminals smaller, and deferred config setup are handled more reliably.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.13.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.13.0/Kaku.dmg)

---

## V0.12.4 Safer 2026-06-30

1. **Open Links**: Cmd+Click now opens a hovered link even inside mouse-capturing tools like Claude, Codex, or vim, so it works the same everywhere.
2. **Window Tiling**: Windows now honor the macOS native drag-to-edge tiling and snap into the system split layouts.
3. **AI File Edits**: The assistant no longer risks running out of memory when it patches a very large file.
4. **Dependencies**: Updated the bundled git and file-handling libraries to close three reported safety advisories.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.4) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.4/Kaku.dmg)

---

## V0.12.3 Cleaner 2026-06-21

1. **System Proxy**: AI requests to local and self-hosted models no longer fail when a system proxy is on; private and loopback addresses now connect directly.
2. **Terminal Redraw**: Scrolling a selection list in tools like the skills picker no longer leaves garbled rows behind.
3. **Credentials**: API keys and tokens are no longer passed on the command line, so they stay out of process listings.
4. **Transparency**: With window transparency enabled, the top strip and rounded corners match the rest of the window instead of showing a darker band or black corners.
5. **Path Highlight**: An existing directory in the command line no longer carries a stray underline, including the doubled one that appeared on hover.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.3) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.3/Kaku.dmg)

---

## V0.12.2 Steadier 2026-06-13

1. **Sleep Wake**: After the Mac sleeps or an external display is unplugged, Kaku no longer freezes on a stale frame; rendering recovers right away while your tasks keep running.
2. **External Displays**: Dragging a window on an external display no longer makes it jump at the first move, and Window Center plus `--position` land on the right spot when screens use different scales.
3. **Selection**: In mouse-enabled apps such as Claude Code, holding the left button while scrolling no longer paints a selection highlight that cannot be cleared, and a plain click clears any leftover one.
4. **Yazi**: Newer Yazi versions refused to start with config files written by earlier Kaku; those files are now repaired automatically.
5. **Windows**: A new window opened into a fullscreen Space fills the screen right away, and cold start no longer flashes a second empty window.
6. **Updates**: Clicking the update notification now asks for confirmation first, so running tasks are no longer interrupted by a stray click.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.2) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.2/Kaku.dmg)

---

## V0.12.1 Smoother 2026-06-08

1. **Shell Setup**: Upgrading from 0.12 no longer shows `[comment]: command not found` during zsh setup.
2. **Prompt Colors**: Starship, tmux, Powerline, and box-drawing separators keep their intended colors instead of turning into bright blocks.
3. **SmartPrompt**: Cmd+Q no longer asks for confirmation at an idle shell just because background helpers such as `gitstatusd` are running.
4. **Scrolling**: During long AI output, scrolling or resizing no longer jumps to the top of history; Kaku keeps you with the current output.
5. **Smart Tab**: New and bundled configs now let Tab accept the grey autosuggestion first, then fall back to completion. Set `smart_tab_mode = 'completion_first'` to keep the old behavior.
6. **Nightly Package**: The Nightly package is now a signed and notarized DMG, so testing the latest fixes feels like installing a normal release.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.1/Kaku.dmg)

---

## V0.12.0 Sharper ✂️ 2026-06-05

1. **Session Scrollback**: Reopening restores each pane's scrollback up to 1,500 lines, reflowing when the width changed.
2. **Codex Backend**: Kaku Assistant runs on your existing Codex / ChatGPT login, no separate API key needed.
3. **AI Chat**: The chat overlay streams answers in real time, suggests your next message with `/suggest`, and reopens reliably every time.
4. **AI Shell**: The `#` quick-fix adds injection detection, and tool access blocks credential files like `.env` and SSH keys in any directory.
5. **SmartPrompt**: Cmd+Q quits instantly when every pane sits at a shell prompt, and asks first when an agent or editor is running.
6. **macOS Window**: Theme flips refresh all windows at once, fullscreen exit is clean, and a title-bar click no longer maximizes.
7. **Document Open**: PDFs, images, media, archives, and Office files open in their default app instead of VS Code.
8. **Tidy**: `smart_tab_mode` is added, Simplified Chinese localization is removed, the zsh comment highlight is brighter, and new CI gates are in place.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.0/Kaku.dmg)

Special thanks to @t0m-car for the non-fancy tab bar fixes.

---

## V0.11.0 Steady 🧭 2026-05-17

1. **AI Reasoning**: DeepSeek-compatible, GLM, Kimi, and Fireworks reasoning streams now stay hidden while final answers remain clean.
2. **Kaku Chat**: `kaku chat` shows a compact thinking state during hidden reasoning and avoids recording empty AI turns.
3. **Chinese UI**: Kaku now includes Simplified Chinese localization for the app shell, command palette, settings, and AI chat.
4. **Shell Setup**: `kaku init` is safer around read-only config paths, existing jump providers, Cmd+Backspace, Yazi themes, and `#` AI queries.
5. **Session Restore**: New restore settings make window snapshots easier to understand and control from config.
6. **Window Control**: Cmd+W now closes the last fullscreen tab correctly, and title-bar dragging avoids accidental snap or maximize.
7. **Mouse and Tabs**: Top-tab hit testing, integrated title buttons, tab drag animation, and scrollback selection are more stable.
8. **Rendering**: Bar cursors, low-DPI toast text, color emoji sizing, and pane background alignment have been tightened.
9. **AI Transport**: Streaming, IME composition, empty API keys, proxy handling, and shell query setup are more robust across providers.
10. **Maintenance**: Agent guides, config docs, release checks, and contributor metadata are updated for the current maintainer workflow.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.11.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.11.0/Kaku.dmg)

Special thanks to @t0m-car for the low-DPI toast clipping fix.

---

## V0.10.0 Chat 🪄 2026-05-10

1. **AI Chat**: One engine, two surfaces: `Cmd+L` opens the in-terminal panel inside Kaku, while `k` or `kaku chat` drops the same engine into an alternate-screen TUI that works in any terminal or over SSH, with streaming Markdown, syntax highlighting, shell context, project tools, web search, memory, theme detection, safer cancel and approval semantics, and inline `#` queries that land in shell history.
2. **AI Configuration and Safety**: Assistant settings now use Simple and Deep models per task with live model loading, proxy-aware requests, OAuth setup, and broader provider responses, while stricter shell approvals, sensitive-path guards covering search and project tools, tighter file write and patch limits, failed-command context, and clearer parse errors keep the engine inside safe lines.
3. **Smart Close Protection**: `Cmd+W` and `Cmd+Shift+W` now ask before killing a pane that runs claude, codex, cursor-agent, gemini, vim, cargo, npm, or any non-shell process, while bare shells still close silently.
4. **Window Snapshots**: Kaku auto-saves multi-tab and multi-pane layouts, restored via `Cmd+Option+Shift+T`, Shell → Restore Previous Window, or the Command Palette.
5. **AppleScript Dictionary**: Kaku ships a full AppleScript dictionary, so windows, tabs, and panes are scriptable from Shortcuts, Hammerspoon, or any automation tool that drives macOS apps.
6. **Animated Tab Drag**: Drag a tab and the neighbors slide into place instead of snapping, so reordering reads as one fluid gesture.
7. **Softer Dark Theme**: Kaku Dark now uses lower-saturation highlight colors and a slightly dimmed foreground, reducing glare on long sessions while keeping the overall Kaku look.
8. **Cold Start and Shell Speed**: Faster startup through Lua bytecode caching, deferred font and config initialization, and cached shell user vars.
9. **Background Updates**: Updates download in the background and fail closed on checksum mismatch, with more reliable proxy and MacPorts detection along the way.
10. **Bug Fixes**: Fullscreen crashes, display races, resize gaps, cursor reflow, links, selection, light-theme readability, and TUI copy are all addressed.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.10.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.10.0/Kaku.dmg)

Special thanks to @s010s, @SherlockSalvatore, @darion-yaphet, @ddotz, @beautifulrem, @yxspace, and @fanweixiao for their contributions to this release.

---

## V0.9.0 Spark ✨ 2026-04-04

1. **Natural Language to Command**: Type `# <description>` at the prompt, press Enter, and Kaku injects the generated command back ready to run. Saved to shell history. Works in zsh and fish.
2. **Option+Click Cursor Movement**: Click anywhere on the current line to move the cursor to that position. Wide characters and multi-byte input are handled correctly.
3. **Always on Top**: Pin any window above others via the Window menu. Toggle on or off at any time.
4. **Traffic Lights Position**: New `traffic_lights` option in Settings to customize the macOS window control button position.
5. **Performance**: Tab title Lua callbacks are batched with a single Config serialization pass.
6. **Stability Fixes**: Fixed a crash on Option+Click, divide-by-zero in split pane sizing, and unwrap panic in mouse event handling.
7. **Shell and Assistant**: Added MiniMax as a built-in provider preset. Fixed zsh-z update display and heredoc quoting edge cases.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.9.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.9.0/Kaku.dmg)

Special thanks to @fanweixiao and @LanternCX for their contributions to this release.

---

## V0.8.0 Fish 🐟 2026-03-23

1. **Fish Shell Support**: Full fish shell integration via `kaku init`, including Starship prompt, Yazi launcher, theme sync, and conf.d entrypoint.
2. **Bell Tab Indicator**: Background tabs show a bell prefix when tasks finish, with optional Dock badge and toggleable tab prefix.
3. **Remember Last Directory**: Kaku restores the last working directory when opening new tabs or windows. Can be disabled via `kaku config`.
4. **Update & Doctor in Tabs**: `kaku update` and `kaku doctor` now open in a dedicated tab instead of blocking the current session.
5. **Basename-only Tab Titles**: New `tab_title_basename_only` option to show just the directory name instead of the full path.
6. **Scrollback Fix**: Fixed viewport jumping to top during rapid output, snapping to bottom after scrolling up, and viewport jumping when using Claude Code.
7. **Bug Fixes & Close Shortcuts**: Fixed window hide, Cmd+Click links, clipboard paste, emoji width, and SSH alias conflicts. Close dialogs now support Enter to confirm and Esc to cancel. Fixed Cmd+Q crash (RefCell double-borrow) and transparent corner arcs on macOS 26.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.8.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.8.0/Kaku.dmg)

Special thanks to @mystersu, @ddotz, @rookie-ricardo, @s010s, @anzksdk, @cynosurech, and @XinCao for their contributions to this release.

---

## V0.7.1 Flow 🌊 2026-03-13

1. **Auto Theme & UI Refresh**: Kaku now auto-switches between dark and light modes with macOS, with improved transparency rendering and Yazi theme sync.
2. **Safer Close & Interaction**: Added tab and pane close confirmations, refreshed overlay styling, and fixed title bar interaction so double-click zoom no longer interferes with window dragging.
3. **Better Settings Workflow**: `kaku config` now provides clearer grouped sections, a pinned footer with contextual key hints, safer config parsing, and more reliable reload behavior.
4. **Improved AI Configuration**: `kaku ai` now supports Antigravity model setup, quota tracking, background loading, and more reliable OAuth token refresh.
5. **Optional Rounded Scrollbars**: Added custom rounded scrollbars that can be enabled from `kaku config`.
6. **Pane Input Broadcast**: Added pane input broadcast modes for synchronized typing across panes, with safeguards to ensure overlay input is never broadcast by mistake.
7. **File & Editor Workflow**: Improved file path link opening, added a remote files shortcut for SSH sessions, and better respects the `$EDITOR` environment for config editing flows.
8. **Bug Fixes & Stability**: Fixed AeroSpace/yabai resize flicker, hardened managed shell and tmux integration, scoped Smart Tab to Kaku sessions by default, and improved Starship RPROMPT fallback behavior.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.7.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.7.1/Kaku.dmg)

Special thanks to @frankekn, @crossly, @iwen-conf, and @zxh326 for their contributions to this release.

---

## V0.6.0 Clarity ☀️ 2026-03-08

1. **Light Theme Improvements**: Kaku now brings a more polished light theme with dynamic font weight, improved ANSI colors, and Claude Code-specific color overrides, including better quote background rendering.
2. **AI Usage Visibility**: The AI settings panel now shows usage summaries and remaining quota more clearly, with added Kimi Code support, Kimi usage tracking, background loading, and more reliable Claude OAuth token refresh persistence. Custom request headers (`custom_headers`) can now also be set in the config file for private or proxied deployments.
3. **Interactive Settings TUI**: `kaku config` is now a richer interactive settings editor with save-on-exit behavior, theme-aware updates, and more reliable config syncing.
4. **Better Tab Workflow**: Tabs can now be reordered with drag and drop, renamed inline by double-clicking, reopened with `Cmd + Shift + T` after closing, or renamed programmatically via the `kaku set-tab-title` command.
5. **Path Hyperlinks**: Terminal output now supports opening file path hyperlinks directly, with better handling for relative paths and decoded file paths.
6. **Smarter Shell Integration**: Improved first-run shell setup without Homebrew, fixed zsh source line escaping, repaired Starship right prompt leakage after `Ctrl + C`, removed the forced `TERM=kaku` setting that broke the Delete key over SSH, and made AI shell hooks more reliable when `TERM` falls back to `xterm-256color`.
7. **Selection and UI Polish**: Preserved text selection during terminal output, fixed stale window padding overrides, improved fullscreen inset and padding alignment, and unified selection colors across displays.
8. **macOS Input and Window Fixes**: Fixed non-Latin IME blocking `Cmd + alnum` shortcuts, fixed `Cmd + Shift + Symbol` key bindings, improved dead key handling, fixed Turkish keyboard tilde input, and improved fullscreen, maximize, display refresh, and global hotkey restore behavior.
9. **Terminal, Memory, and Notification Improvements**: Added a Dock badge for unread bell notifications, improved scrollback memory with lazy allocation, capped background image and gradient render cache sizes for more stable long-session memory usage, and further improved update actions, menu command handling, and terminal interaction reliability.
10. **Release and Upgrade Hardening**: Hardened the automated release pipeline, centralized config migration metadata, and bumped the config migration version to `14` so existing users receive the required shell integration refresh and prompt helper fix.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.6.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.6.0/Kaku.dmg)

Special thanks to @frankekn, @ddnio, @GavinRiver, and @nguyenphutrong for their contributions to this release. If Kaku helps you, please consider giving it a star and recommending it to your friends.

---

## V0.5.1 Kindness 🌴 2026-02-28

1. Fixed shell alias conflict, `y` launcher no longer clashes with existing aliases like `alias y=yarn`
2. Fixed SSH remote display, sessions now forward `TERM=xterm-256color` to prevent broken rendering on servers without Kaku's terminfo
3. Fixed `Cmd + Shift + ,` keybinding not passing through to tmux and other terminal apps
4. Fixed CLI panic when running `kaku cli split-pane`
5. Fixed misleading "error" toast appearing in AI assistant during analysis
6. Fixed update notification appearing on startup even when check\_for\_updates is disabled

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.5.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.5.1/Kaku.dmg)

---

## V0.5.0 Yohaku 🪽 2026-02-27

1. **AI Shell Error Fixer**: When a command fails, Kaku sends it to an AI model and shows a fix suggestion right in the terminal. Press `Cmd + Shift + E` to apply it.
2. **Yazi Built In**: Press `Cmd + Shift + Y` or type `y` to open Yazi. Layout and theme config are set up automatically on first run. `cd + Tab` also falls back to zsh-z history when no filesystem match is found.
3. **Command Palette**: Press `Cmd + Shift + P` for quick command search with fuzzy matching and native text editing support.
4. **Kaku Doctor**: Run `kaku doctor` to check your setup and fix common issues interactively.
5. **Global Hotkey**: Press `Ctrl + Opt + Cmd + K` to show or hide Kaku from anywhere. Configurable via `macos_global_hotkey`.
6. **Shell Text Editing**: Use `Cmd + A` to select all, `Shift + Arrow` to extend selection, and type to replace, just like a native text editor, right in your shell prompt.
7. **Expanded AI Config**: `kaku ai` now covers Kaku Assistant, Factory Droid, and opencode.jsonc all in one place.
8. **Bell Indicator**: Tabs show a blinking dot when they receive a bell while you are in another tab.
9. **Better Scrolling and Input**: Smoother trackpad scrolling in `less` and `man` with DECSET 1007 alternate scroll support. IME popup positioning is fixed for CJK input methods like Typeless.
10. **Native Window Shadow**: Window shadow is restored on macOS after being disabled to work around a GPU usage issue in earlier builds.
11. **Faster Startup and Smaller Bundle**: Lua bytecode caching and deferred module loading cut launch time. Fat LTO and single codegen unit reduce binary size.
12. **Shell Integration Fix**: Kaku's shell integration now loads in all terminal environments instead of only when `$TERM` is set to `kaku`. Plugins already loaded by your own config are no longer duplicated.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.5.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.5.0/Kaku.dmg)

Special thanks to @jaost, @nguyenphutrong, @mickyyy68, and @guohai for their contributions to this release. If Kaku helps you, please consider giving it a star and recommending it to your friends.

---

## V0.4.0 AIIIIIII 🥂 2026-02-19

1. **New `kaku ai` Command**: Added `kaku ai` to manage your current AI coding tool configs in one place, with improved editor interaction, account display, and OAuth re-auth flow.
2. **Better Startup and Rendering on macOS**: Kaku now defaults to WebGpu with OpenGL fallback, delivers better rendering quality, and reduces memory usage from about 200MB to about 80MB in typical scenarios.
3. **Lazygit Workflow Built In**: Added `Cmd + Shift + G`, in-terminal launch flow, and contextual hints for git repositories.
4. **Split UX Enhancements**: Added a current pane marker so you can spot the active pane at a glance. Also added `split_thickness` config and precise split-line intersection alignment.
5. **Better Window and Pane Controls**: `Cmd + W` closing logic now works as expected across panes, tabs, and multi-window setups. Added `Cmd + Opt + Arrows` to jump between panes.
6. **Shell Setup Improved**: Refined first-run and update shell flow with optional Homebrew CLI tool install and smarter Tab completion behavior.
7. **SSH Remote and 1Password Support**: Remote SSH sessions now force `TERM=xterm-256color` to avoid terminfo issues. Shell integration also auto-detects the 1Password SSH agent and adds `IdentitiesOnly=yes` to stop auth failures from too many offered keys.
8. **Per-Pane Encoding Support**: Added pane-level encoding switching with support for `UTF-8`, `GBK`, `GB18030`, `Big5`, `EUC-KR`, and `Shift-JIS`.
9. **URL Scheme for Pane Activation**: Added `kaku://open-tab?tty=<device>` so external tools like scripts or AI agents can jump straight to a specific terminal pane by TTY name.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.4.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.4.0/Kaku.dmg)

Thanks to @WongLoki, @liu-kan, @OiAnthony, @XueshiQiao, and @CTHua for the contributions.

---

## V0.3.1 New Year 🎋 2026-02-16

1. **Clear Screen with Cmd+K**: Use `Cmd+K` to clear the screen. `Cmd+R` is still supported for compatibility.
2. **Toggle Split Direction**: Press `Cmd+Shift+S` to toggle a split pane between horizontal and vertical layout. Zoom state and minimum size are properly handled.
3. **SSH Compatibility Fix**: SSH sessions now use `xterm-256color` as TERM, both for built-in SSH domains and when running `ssh` directly in the terminal. Remote hosts no longer fail due to missing `kaku` terminfo.
4. **macOS Dictation Support**: Fixed `selected_range` to return a valid cursor position, enabling macOS dictation and voice input to work correctly.
5. **Tab Completion Restored**: Removed Smart Tab behavior. Tab key now shows the completion list as expected. Use Right Arrow to accept autosuggestions.
6. **Auto-Create User Config**: `kaku init` now automatically creates `~/.config/kaku/kaku.lua` if it doesn't exist.
7. **OpenCode Theme Setup**: `kaku init` can optionally set up a Kaku-matching theme for OpenCode, with user confirmation before writing.
8. **Bluetooth Permission**: Added `NSBluetoothAlwaysUsageDescription` to Info.plist for terminal-based BLE development workflows.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.3.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.3.1/Kaku.dmg)

Thanks to @GavinRiver for the contribution. If you find Kaku helpful, feel free to star the project and share it with your friends.

---

## V0.3.0 Happy 🥙 2026-02-16

1. **Smoother Fullscreen**: Transitions are now stable with less flicker. Padding stays correct when you split panes or create tabs in fullscreen. Fullscreen tab bar visibility and split rendering were refined for cleaner layout.
2. **Window Restore Stability**: Kaku now persists window size and position during move/resize, with display-aware restore for better multi-monitor behavior.
3. **Toast Notifications**: Copy text and see `Copied!` appear at the bottom right. Reload config and get instant confirmation. You now know when things actually work.
4. **SSH Host in Tabs**: When connected to a remote server, the tab shows the hostname. No more guessing which tab is which server.
5. **Finder Integration**: Right-click any folder in Finder and select `Open in Kaku`. Terminal opens with that folder as your working directory. Service open requests are now queued until GUI is ready for more reliable startup behavior.
6. **Default Terminal**: Set Kaku as your system default via menu bar `Kaku` → `Set as Default Terminal`. Shell scripts and terminal file types will open in Kaku instead of Terminal.app.
7. **Shell History Peek**: In vim or tmux, scroll up to view your shell history without leaving. Scroll down to return. No need to quit vim to check previous output.
8. **Image Paste**: Copy images from other apps and paste directly into terminal apps in Kaku. Images save to temp and the path auto-inserts.
9. **Selection Autoscroll**: Drag to select text and move upward beyond the viewport, the view now auto-scrolls so you can select more content above. Fixed a bug where this stopped working during drag capture.
10. **Transparent Window Fix**: Fixed edge artifacts when using window background opacity. Gaps now fill properly with background color.
11. **Multi-File Dock Drop**: Drag multiple files or folders to the Kaku Dock icon. Each opens in its own tab.
12. **Better Font Rendering**: Fixed compressed text on macOS and wide glyphs getting squeezed. Icon fonts like Nerd Fonts now display at proper width, and low-resolution displays now use a smaller default font size for better readability.
13. **Bar Cursor**: Changed from block to bar cursor, more common in modern terminals. Supports Vim mode cursor switching between normal and insert modes. Cursor blinking behavior is restored with a tuned default blink rate (`500ms`) and improved inactive-pane cursor rendering.
14. **Clearer Split Labels**: Menu now shows `Split Pane Top/Bottom` and `Split Pane Left/Right` instead of confusing `Vertically/Horizontally`.
15. **Safer `kaku update`**: Prevents upgrading the wrong Homebrew package and only probes caskroom-managed installs.
16. **\[color\_scheme\](cci:1://file:///Users/tw93/www/Kaku/config/src/config.rs:1469:4-1525:5) Override Fix**: User-defined \[color\_scheme\](cci:1://file:///Users/tw93/www/Kaku/config/src/config.rs:1469:4-1525:5) now correctly overrides the bundled `Kaku` theme.
17. **Lower GPU Usage**: Disabled window shadow by default to reduce GPU overhead on macOS, and the window looks even cleaner.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.3.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.3.0/Kaku.dmg)

Thanks to @mickyyy68, @Viking602, @GavinRiver, @QuentinHsu for their contributions. If you find Kaku helpful, feel free to star the project and share it with your friends.

---

## V0.2.0 Craft 🍺 2026-02-13

1. **Universal Binary**: One app for both Apple Silicon and Intel Macs. No need to choose between architectures. Optimized Rust build for smaller size and better performance.
2. **Apple Notarized**: After 3 years of failed applications, I finally wrote to Cook to complain. Problem solved. Developer ID acquired. 698 RMB well spent. Just so you can open Kaku without security warnings on day one.
3. **Fixed Config Loading**: User config at `~/.config/kaku/kaku.lua` now loads correctly instead of being overridden by defaults.
4. **Homebrew Support**: Install with `brew install tw93/tap/kakuku` and upgrade with `brew upgrade kakuku`.
5. **Fullscreen Time Display**: A subtle clock in the bottom-right corner during fullscreen, even when deep in AI coding flow, keep track of time and get some rest. Your health comes first. Plus smoother fullscreen transitions.
6. **Smarter Window Controls**: Double-click title bar or tab area to toggle zoom. Better window restoration, improved title bar drag to prevent text selection scrolling, configurable split pane gap, and cleaner alignment.
7. **Unified CLI**: New `kaku` command with `init`, `update`, `reset`, `config` and more for quick setup and maintenance.
8. **Enhanced Git Delta**: Better theme alignment, side-by-side diff by default, and cleaner headers for easier code review.
9. **Chinese Path Support**: Tab titles now display Chinese characters correctly instead of URL-encoded strings.
10. **Persistent Sessions**: `Cmd+W` closes current pane or tab; when only one remains, hides the window instead of quitting.
11. **Better UX**: Font zoom and window size persist across restarts. Smarter Tab completion prioritizes filesystem matches.
12. **Visual Polish**: Fixed split pane alignment and underline overflow, eliminated tab switching stutter, upgraded to Unicode v14 for better emoji support.
13. **Menu Bar**: Improved menu bar with Command Palette, Settings, Check for Updates, and native notifications.
14. **Built-in Updater**: Automatic update notifications. Click "Kaku" → "Check for Updates" in menu bar, or run `kaku update` in terminal.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.2.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.2.0/Kaku.dmg)

Since the changes in this version are pretty crazy, please let me know if you run into any problems. Thanks to @yikZero, @iwen-conf, @GavinRiver, @dongowu, @yamamel for their contributions. If you find Kaku helpful, feel free to star this project.

---

## V0.1.1 Easy to use 🍤 2026-02-09

1. **Crisp Font Rendering**: Optimized macOS font rasterization settings (enabled Light Hinting) for clearer, sharper text, especially on Retina displays.
2. **Kaku Theme**: Introducing the signature Kaku Theme, a high-contrast, AI-optimized dark theme designed for long coding sessions with Claude/Codex.
3. **Enhanced Onboarding**: The first-run wizard now offers to apply the Kaku Theme automatically, giving you a perfect environment instantly.
4. **Zero Config Polish**: Improved default settings and streamlined the initial setup process for an even smoother out-of-the-box experience.
5. **Debug Overlay**: Added a new debug overlay for easier troubleshooting of shell integration and configuration issues.
6. **Shell Integration**: Launched a robust `setup_zsh.sh` script with common aliases and git shortcuts to supercharge your terminal workflow.
7. **Tab Bar Improvements**: Custom tab bar now supports path-based titles and visual indicators; fixed runtime errors with `get_current_working_dir`.
8. **Under the Hood**: Automated version synchronization and various internal improvements for stability. Addressed macOS permission prompts for a smoother experience. Renamed internal project references to `kaku` for consistent branding.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.1.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.1.1/Kaku.dmg)

If macOS blocks the app, go to System Settings → Privacy & Security → click Open Anyway.

---

## V0.1.0 Freshmen 🧝‍♀️ 2026-02-08

1. First Release: Kaku is now live! A fast, out-of-the-box terminal built for AI coding, deeply customized for macOS with GPU acceleration.
2. Built-in Shell Suite: Comes with pre-configured Starship prompt, z smart directory jumper, Syntax Highlighting, Autosuggestions.
3. Smart Onboarding: Features an intelligent first-run wizard that detects your environment, safely backs up existing configs.
4. Native Experience: Optimized for macOS with smooth animations, intuitive shortcuts, split pane management, and focus mode.
5. Universal Binary: Fully optimized support for both Apple Silicon and Intel Macs in a single lightweight DMG.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.1.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.1.0/Kaku.dmg)

If macOS blocks the app, go to System Settings → Privacy & Security → click "Open Anyway".

---

Source: https://kaku.fun/changelog
Site index for LLMs: https://kaku.fun/llms.txt
