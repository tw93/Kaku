//! Browser terminal served by the remote bridge.
//!
//! The page mirrors a pane by rendering Kaku's own terminal model to ANSI
//! frames, so the browser never parses the PTY stream itself: terminal queries
//! (DA, CPR, OSC color) keep being answered once, by Kaku. Keys go back through
//! `Pane::key_down` so cursor-key mode and keyboard protocols match what the
//! app asked Kaku for; text and pastes take the same paths Kaku's GUI uses.

use crate::{check_token, WsQuery};
use axum::extract::{ws, ConnectInfo, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use mux::pane::{CloseReason, Pane, PaneId};
use mux::Mux;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;
use termwiz::cell::{Blink, CellAttributes, Intensity, Underline};
use termwiz::color::ColorAttribute;
use termwiz::input::{KeyCode, Modifiers};
use termwiz::surface::{CursorShape, CursorVisibility};
use tokio::sync::{broadcast, mpsc};
use wezterm_term::{MouseButton, MouseEvent, MouseEventKind, TerminalSize};

const INDEX_HTML: &str = include_str!("../assets/web/index.html");
const XTERM_JS: &str = include_str!("../assets/web/xterm.js");
const XTERM_CSS: &str = include_str!("../assets/web/xterm.css");
// Kaku's bundled terminal font: Regular for text, Medium for bold, matching
// the dark-theme weights in the bundled kaku.lua.
const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");
const FONT_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Medium.ttf");

/// Frames are coalesced to this interval; the trailing edge always flushes,
/// so the last output of a burst is never dropped.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// Re-render this often even without output, to pick up cursor moves,
/// title changes and resizes that do not produce `PaneOutput`.
const IDLE_REFRESH: Duration = Duration::from_secs(1);
const MAX_INPUT_LEN: usize = 65_536;
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

/// Tab changes the page asks for. The GUI performs them through the same
/// key assignments as its own Cmd+T and Close Tab, so cwd inheritance and
/// the closed-tab history behave as they do on the Mac.
#[derive(Debug, Clone, Copy)]
pub enum TabAction {
    /// Open a tab in the window holding this pane.
    New { pane_id: usize },
    /// Close the tab holding this pane. The page has already asked per
    /// tab_close_confirmation, so the Mac must not prompt again: its prompt
    /// would only be visible on the Mac.
    Close { pane_id: usize },
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

pub(crate) fn notify_output(dirty: &WebDirty, pane_id: usize) {
    if let Some(tx) = dirty.lock().get(&pane_id) {
        let _ = tx.send(());
    }
}

pub(crate) fn pane_removed(dirty: &WebDirty, pane_id: usize) {
    dirty.lock().remove(&pane_id);
}

// ── Static assets ─────────────────────────────────────────────────────────────

/// The page and its files, by path relative to the page. Served locally and,
/// through the relay, from this Kaku, so the page always matches the build.
pub(crate) fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match path {
        "" | "index.html" => ("text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        "assets/xterm.js" => ("text/javascript; charset=utf-8", XTERM_JS.as_bytes()),
        "assets/xterm.css" => ("text/css; charset=utf-8", XTERM_CSS.as_bytes()),
        "assets/font-regular.ttf" => ("font/ttf", FONT_REGULAR),
        "assets/font-medium.ttf" => ("font/ttf", FONT_MEDIUM),
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

/// Loopback-only page that hands the browser on this Mac a link carrying the
/// token in the URL fragment, which browsers never send to the server.
pub(crate) async fn route_link(ConnectInfo(addr): ConnectInfo<SocketAddr>) -> Response {
    if !addr.ip().is_loopback() {
        return (StatusCode::FORBIDDEN, "only accessible from this Mac").into_response();
    }
    let token = crate::get_or_init_token();
    let cfg = config::configuration();
    let port = cfg.remote.port;
    let lan = if cfg.remote.bind == "127.0.0.1" || cfg.remote.bind == "localhost" {
        None
    } else {
        crate::lan_ip()
    };
    let local = format!("http://127.0.0.1:{port}/#t={token}");
    let mut body = format!(
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kaku Web</title>
<style>body{{font:15px -apple-system,sans-serif;background:#16161a;color:#ddd;margin:40px auto;max-width:640px;padding:0 16px}}
a{{color:#9ecbff;word-break:break-all}}p{{color:#999}}</style>
<h2>Kaku Web</h2>
<p>This Mac</p><a href="{local}">{local}</a>"#
    );
    if let Some(ip) = lan {
        let url = format!("http://{ip}:{port}/#t={token}");
        let _ = write!(
            body,
            r#"<p>Other computers on this network</p><a href="{url}">{url}</a>"#
        );
    }
    body.push_str("<p>The link stops working when Kaku restarts.</p>");
    Html(body).into_response()
}

#[derive(Serialize, PartialEq)]
struct TabInfo {
    tab_id: usize,
    pane_id: usize,
    title: String,
    active: bool,
    /// Whether closing should ask first, per tab_close_confirmation.
    confirm_close: bool,
}

/// Tabs in tab bar order across windows, each pointing at its active pane.
fn collect_tabs() -> Vec<TabInfo> {
    let Some(mux) = Mux::try_get() else {
        return Vec::new();
    };
    let titles = TAB_TITLES.lock().clone();
    let policy = config::configuration().tab_close_confirmation;
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
            tabs.push(TabInfo {
                tab_id,
                pane_id: pane.pane_id().into(),
                title,
                active: idx == active_idx,
                confirm_close: policy
                    .should_prompt(true, || tab.can_close_without_prompting(CloseReason::Tab)),
            });
        }
    }
    tabs
}

// ── Wire protocol ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebClientMsg {
    /// First message through the relay; proves the page holds the key.
    Hello,
    /// Heartbeat; answered with `pong` so the page can tell a dead link
    /// from a quiet terminal, and so idle-timeout proxies see traffic.
    Ping,
    /// Start mirroring a pane; the rest of the messages act on it.
    Attach {
        pane_id: usize,
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
    /// The grid the browser can show at Kaku's font size. While the page is
    /// driving the pane, its tab takes this size; local input on the Mac
    /// takes it back.
    Resize {
        cols: usize,
        rows: usize,
    },
    /// Open a tab next to the one being viewed.
    NewTab,
    /// Close a tab; the page has already confirmed with the user.
    CloseTab {
        tab_id: usize,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebServerMsg<'a> {
    Config(&'a crate::ConfigInfo),
    Tabs {
        tabs: &'a [TabInfo],
    },
    Frame {
        cols: usize,
        rows: usize,
        title: &'a str,
        scrolled: bool,
        data: &'a str,
    },
    /// The attached pane is gone; the page picks another tab. It names the
    /// pane because the page may already have moved on after a tabs update,
    /// and must not drop the tab it moved to.
    Closed {
        pane_id: usize,
    },
    Pong,
}

// ── Frame rendering ───────────────────────────────────────────────────────────

/// Where the browser is looking: following the live screen, or pinned to a
/// stable row so new output does not move the history being read.
#[derive(Clone, Copy, Default)]
struct View {
    pinned_top: Option<isize>,
    /// Grid size this browser asked for, re-applied on its next input in
    /// case the Mac took the tab back meanwhile.
    claimed: Option<(usize, usize)>,
}

/// Size the tab holding `pane_id` to the browser's grid. Runs on the GUI
/// thread, where the window's own resizes happen.
fn claim_tab_size(pane_id: usize, cols: usize, rows: usize) {
    promise::spawn::spawn_into_main_thread(async move {
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
    })
    .detach();
}

struct Frame {
    cols: usize,
    rows: usize,
    title: String,
    scrolled: bool,
    /// One self-contained string per row: it positions itself and starts
    /// from reset attributes, so any subset of rows can be sent alone.
    lines: Vec<String>,
    /// Cursor position, shape and visibility, applied after the rows.
    cursor: String,
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

    let mut rendered = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut out = String::with_capacity(dims.cols + 16);
        let _ = write!(out, "\x1b[{};1H", row + 1);
        let mut current: Option<Sgr> = None;
        if let Some(line) = lines.get(row) {
            let mut col = 0;
            for cell in line.visible_cells() {
                if col >= dims.cols {
                    break;
                }
                let sgr = Sgr::from_attrs(cell.attrs());
                if current.as_ref() != Some(&sgr) {
                    sgr.write(&mut out);
                    current = Some(sgr);
                }
                let text = cell.str();
                let width = cell.width().max(1);
                if text.is_empty() {
                    out.push(' ');
                } else {
                    out.push_str(text);
                }
                col += width;
                // xterm.js has its own width table (Unicode 6 by default), so
                // for symbols and emoji it can disagree with Kaku about how
                // many columns a glyph takes and shift the rest of the row.
                // Pin the next column to Kaku's answer.
                if !text.is_ascii() && col < dims.cols {
                    let _ = write!(out, "\x1b[{}G", col + 1);
                }
            }
        }
        // Clear the rest of the row with the default background.
        out.push_str("\x1b[0m\x1b[K");
        rendered.push(out);
    }

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

    Frame {
        cols: dims.cols,
        rows,
        title: pane.get_title(),
        scrolled: view.pinned_top.is_some(),
        lines: rendered,
        cursor: cursor_out,
    }
}

/// What the browser already shows, so the next frame only carries changes.
#[derive(Default)]
struct Shown {
    cols: usize,
    title: String,
    scrolled: bool,
    lines: Vec<String>,
    cursor: String,
}

impl Shown {
    /// The bytes that bring the browser from what it shows to `frame`, or
    /// None when nothing changed.
    fn update(&mut self, frame: Frame) -> Option<(Frame, String)> {
        let full = self.cols != frame.cols
            || self.lines.len() != frame.lines.len()
            || self.scrolled != frame.scrolled;
        let mut data = String::from("\x1b[?25l");
        let mut changed = false;
        for (idx, line) in frame.lines.iter().enumerate() {
            if full || self.lines.get(idx) != Some(line) {
                data.push_str(line);
                changed = true;
            }
        }
        if !changed && self.cursor == frame.cursor && self.title == frame.title {
            return None;
        }
        data.push_str(&frame.cursor);
        self.cols = frame.cols;
        self.title.clone_from(&frame.title);
        self.scrolled = frame.scrolled;
        self.cursor.clone_from(&frame.cursor);
        self.lines.clone_from(&frame.lines);
        Some((frame, data))
    }
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
#[derive(PartialEq)]
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

impl Sgr {
    fn from_attrs(attrs: &CellAttributes) -> Self {
        Self {
            fg: attrs.foreground(),
            bg: attrs.background(),
            intensity: attrs.intensity(),
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

fn handle_input(pane: &Arc<dyn Pane>, msg: WebClientMsg, view: &mut View) {
    let interactive = matches!(
        msg,
        WebClientMsg::Text { .. } | WebClientMsg::Key { .. } | WebClientMsg::Paste { .. }
    );
    if interactive {
        if let Some((cols, rows)) = view.claimed {
            claim_tab_size(pane.pane_id().into(), cols, rows);
        }
    }
    let result = match msg {
        WebClientMsg::Text { data } => {
            view.pinned_top = None;
            if data.len() > MAX_INPUT_LEN {
                return;
            }
            pane.writer()
                .write_all(data.as_bytes())
                .map_err(anyhow::Error::from)
        }
        WebClientMsg::Paste { data } => {
            view.pinned_top = None;
            if data.len() > MAX_INPUT_LEN {
                return;
            }
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
        WebClientMsg::Scroll { lines } if pane.is_alt_screen_active() => {
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
        WebClientMsg::CloseTab { tab_id } => {
            let target = Mux::try_get()
                .and_then(|mux| mux.get_tab(mux::tab::TabId::from(tab_id)))
                .and_then(|tab| tab.get_active_pane());
            if let Some(target) = target {
                dispatch_tab_action(TabAction::Close {
                    pane_id: target.pane_id().into(),
                });
            }
            Ok(())
        }
        WebClientMsg::Hello | WebClientMsg::Ping | WebClientMsg::Attach { .. } => Ok(()),
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

/// One browser, whatever carries its messages: the local WebSocket or the
/// relay tunnel. `inbox` yields the page's JSON messages and `outbox` takes
/// ours; the session ends when either side goes away.
pub(crate) async fn run_session(
    mut inbox: mpsc::Receiver<String>,
    outbox: mpsc::Sender<String>,
    dirty: WebDirty,
) {
    if let Some(json) = encode(&WebServerMsg::Config(&crate::config_info())) {
        if outbox.send(json).await.is_err() {
            return;
        }
    }

    let mut attached: Option<usize> = None;
    let mut updates: Option<broadcast::Receiver<()>> = None;
    let mut view = View::default();
    let mut shown = Shown::default();
    let mut last_tabs: Option<Vec<TabInfo>> = None;
    let mut dirty_frame = false;
    let mut frame_tick = tokio::time::interval(FRAME_INTERVAL);
    frame_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut idle = tokio::time::interval(IDLE_REFRESH);

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
                let msg = match serde_json::from_str::<WebClientMsg>(&text) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::debug!("kaku-remote web: bad message: {e}");
                        continue;
                    }
                };
                if matches!(msg, WebClientMsg::Ping) {
                    let Some(json) = encode(&WebServerMsg::Pong) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    continue;
                }
                if let WebClientMsg::Attach { pane_id } = msg {
                    attached = Some(pane_id);
                    updates = Some(subscribe(&dirty, pane_id));
                    view.pinned_top = None;
                    shown = Shown::default();
                    dirty_frame = true;
                    continue;
                }
                let Some(pane) = attached.and_then(lookup_pane) else { continue };
                handle_input(&pane, msg, &mut view);
                dirty_frame = true;
            }
            update = pane_update => match update {
                Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => dirty_frame = true,
                Err(broadcast::error::RecvError::Closed) => updates = None,
            },
            _ = idle.tick() => {
                dirty_frame = attached.is_some();
                // Tab state lives on the GUI thread; the close-confirmation
                // check reads the Lua config, which only that thread may touch.
                let tabs = promise::spawn::spawn_into_main_thread(async { collect_tabs() }).await;
                if last_tabs.as_ref() != Some(&tabs) {
                    let Some(json) = encode(&WebServerMsg::Tabs { tabs: &tabs }) else { continue };
                    if outbox.send(json).await.is_err() {
                        break;
                    }
                    last_tabs = Some(tabs);
                }
            }
            _ = frame_tick.tick(), if dirty_frame => {
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
                let msg = WebServerMsg::Frame {
                    cols: frame.cols,
                    rows: frame.rows,
                    title: &frame.title,
                    scrolled: frame.scrolled,
                    data: &data,
                };
                let Some(json) = encode(&msg) else { continue };
                if outbox.send(json).await.is_err() {
                    break;
                }
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
    if !check_token(&query, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    upgrade.on_upgrade(move |socket| handle_local_ws(socket, dirty))
}

async fn handle_local_ws(socket: ws::WebSocket, dirty: WebDirty) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<String>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let session = tokio::spawn(run_session(in_rx, out_tx, dirty));
    let writer = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if ws_tx.send(ws::Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            ws::Message::Text(text) if in_tx.send(text.to_string()).await.is_err() => break,
            ws::Message::Close(_) => break,
            _ => {}
        }
    }
    drop(in_tx);
    let _ = session.await;
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::color::SrgbaTuple;

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
        Sgr::from_attrs(&attrs).write(&mut out);
        assert_eq!(out, "\x1b[0;1;7;38;5;2;48;2;255;0;127m");
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
        Sgr::from_attrs(&CellAttributes::default()).write(&mut out);
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

    fn frame(cols: usize, lines: &[&str], cursor: &str) -> Frame {
        Frame {
            cols,
            rows: lines.len(),
            title: "t".into(),
            scrolled: false,
            lines: lines.iter().map(|l| l.to_string()).collect(),
            cursor: cursor.into(),
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
    fn key_names_map_to_kaku_key_codes() {
        assert_eq!(key_code("Enter"), Some(KeyCode::Enter));
        assert_eq!(key_code("ArrowUp"), Some(KeyCode::UpArrow));
        assert_eq!(key_code("F5"), Some(KeyCode::Function(5)));
        assert_eq!(key_code("c"), Some(KeyCode::Char('c')));
        assert_eq!(key_code("F99"), None);
        assert_eq!(key_code("Unidentified"), None);
    }
}
