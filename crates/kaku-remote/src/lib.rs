use anyhow::Context;
use axum::extract::{ws, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use config::{configuration, RgbaColor};
use futures_util::{SinkExt, StreamExt};
use mux::pane::CachePolicy;
use mux::{Mux, MuxNotification};
use parking_lot::Mutex;
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

// ── Token ─────────────────────────────────────────────────────────────────────

static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn get_or_init_token() -> &'static str {
    TOKEN.get_or_init(|| {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("getrandom failed");
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    })
}

// ── Shared state ──────────────────────────────────────────────────────────────

type PaneSenders = Arc<Mutex<HashMap<usize, broadcast::Sender<ScreenUpdate>>>>;
type PaneThrottle = Arc<Mutex<HashMap<usize, Instant>>>;

const CAPTURE_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone)]
struct AppState {
    pane_senders: PaneSenders,
}

/// Maximum bytes accepted in a single client input message.
const MAX_INPUT_LEN: usize = 65_536;

// ── Wire protocol ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ScreenUpdate {
    pub pane_id: usize,
    pub cursor_x: usize,
    pub cursor_y: isize,
    pub cols: usize,
    pub viewport_rows: usize,
    pub lines: Vec<ScreenLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenLine {
    pub row: isize,
    pub text: String,
}

#[derive(Debug, Serialize)]
struct PaneInfo {
    id: usize,
    title: String,
    cwd: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigInfo {
    background: Option<String>,
    foreground: Option<String>,
    cursor_fg: Option<String>,
    cursor_bg: Option<String>,
    ansi: Vec<String>,
    brights: Vec<String>,
    font_family: String,
    font_size: f64,
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "input")]
    Input {
        #[serde(default)]
        pane_id: Option<usize>,
        text: String,
    },
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn rgba_to_hex(c: &RgbaColor) -> String {
    c.to_string()
}

fn opt_hex(c: Option<&RgbaColor>) -> Option<String> {
    c.map(rgba_to_hex)
}

// ── Screen capture ────────────────────────────────────────────────────────────

fn capture_pane(pane_id: usize) -> Option<ScreenUpdate> {
    let mux = Mux::try_get()?;
    let pane = mux.get_pane(mux::pane::PaneId::from(pane_id))?;

    let dims = pane.get_dimensions();
    let cursor = pane.get_cursor_position();

    let start = dims.physical_top;
    let end = dims.physical_top + dims.viewport_rows as isize;
    let (_first, raw_lines) = pane.get_lines(start..end);

    let lines: Vec<ScreenLine> = raw_lines
        .iter()
        .enumerate()
        .map(|(i, line)| ScreenLine {
            row: start + i as isize,
            text: line.as_str().to_string(),
        })
        .collect();

    Some(ScreenUpdate {
        pane_id,
        cursor_x: cursor.x,
        cursor_y: cursor.y,
        cols: dims.cols,
        viewport_rows: dims.viewport_rows,
        lines,
    })
}

// ── HTTP routes ───────────────────────────────────────────────────────────────

async fn route_config(
    Query(query): Query<WsQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !check_token(&query, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    let cfg = configuration();
    let palette = &cfg.resolved_palette;

    let ansi_colors = palette
        .ansi
        .map(|arr| arr.iter().map(|c| c.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    let bright_colors = palette
        .brights
        .map(|arr| arr.iter().map(|c| c.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    let font_family = cfg
        .font
        .font
        .first()
        .map(|f| f.family.clone())
        .unwrap_or_else(|| "JetBrains Mono".to_string());

    let info = ConfigInfo {
        background: opt_hex(palette.background.as_ref()),
        foreground: opt_hex(palette.foreground.as_ref()),
        cursor_fg: opt_hex(palette.cursor_fg.as_ref()),
        cursor_bg: opt_hex(palette.cursor_bg.as_ref()),
        ansi: ansi_colors,
        brights: bright_colors,
        font_family,
        font_size: cfg.font_size,
    };

    match serde_json::to_value(&info) {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => {
            log::error!("kaku-remote: failed to serialize config: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "serialization error").into_response()
        }
    }
}

async fn route_panes(
    Query(query): Query<WsQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !check_token(&query, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    let panes: Vec<PaneInfo> = Mux::try_get()
        .map(|mux| {
            mux.iter_panes()
                .into_iter()
                .map(|p| {
                    let cwd = p
                        .get_current_working_dir(CachePolicy::FetchImmediate)
                        .map(|u| u.to_string());
                    PaneInfo {
                        id: p.pane_id().into(),
                        title: p.get_title(),
                        cwd,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    match serde_json::to_value(&panes) {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => {
            log::error!("kaku-remote: failed to serialize panes: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "serialization error").into_response()
        }
    }
}

fn check_token(query: &WsQuery, headers: &axum::http::HeaderMap) -> bool {
    let expected = get_or_init_token();
    headers
        .get("x-kaku-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == expected)
        .unwrap_or(false)
        || query.token.as_deref() == Some(expected)
}

// ── QR code endpoint ──────────────────────────────────────────────────────────

/// Returns an HTML page with an inline SVG QR code for the connection URL.
/// The URL encodes: kakuremote://host:port?token=xxx
/// Restricted to localhost: the token must never be served to LAN peers.
async fn route_qr(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Response {
    if !addr.ip().is_loopback() {
        return (StatusCode::FORBIDDEN, "QR endpoint only accessible from localhost").into_response();
    }
    route_qr_inner().await.into_response()
}

async fn route_qr_inner() -> Html<String> {
    let cfg = configuration();
    let port = cfg.remote.port;
    let token = get_or_init_token();

    let host = lan_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let url = format!("kakuremote://{}:{}?token={}", host, port, token);

    let svg_string = QrCode::new(url.as_bytes())
        .map(|code| {
            code.render::<svg::Color>()
                .min_dimensions(280, 280)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_else(|_| "<p>QR generation failed</p>".to_string());

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Kaku Remote – Connect</title>
  <style>
    body {{ font-family: -apple-system, sans-serif; background: #1a1a1a; color: #eee;
            display: flex; flex-direction: column; align-items: center;
            justify-content: center; min-height: 100vh; margin: 0; padding: 24px; box-sizing: border-box; }}
    h1 {{ font-size: 1.4rem; margin-bottom: 8px; }}
    p  {{ color: #aaa; font-size: 0.85rem; margin-bottom: 24px; text-align: center; }}
    .qr {{ background: #fff; border-radius: 12px; padding: 16px; }}
    code {{ font-size: 0.75rem; color: #888; margin-top: 16px; word-break: break-all; max-width: 320px; text-align: center; }}
  </style>
</head>
<body>
  <h1>Kaku Remote</h1>
  <p>Open the Kaku Remote iOS app and tap <strong>Scan QR</strong></p>
  <div class="qr">{}</div>
  <code>{}</code>
</body>
</html>"#,
        svg_string, url
    );

    Html(html)
}

/// Best-effort: return the first non-loopback IPv4 address.
fn lan_ip() -> Option<String> {
    use std::net::UdpSocket;
    // Connect to a public address without sending data — just to discover
    // the local interface IP the OS would route through.
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let local = sock.local_addr().ok()?;
    Some(local.ip().to_string())
}

// ── WebSocket handler ─────────────────────────────────────────────────────────

async fn route_ws(
    Path(pane_id): Path<usize>,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    ws_upgrade: ws::WebSocketUpgrade,
) -> Response {
    if !check_token(&query, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }

    let rx = {
        let mut senders = state.pane_senders.lock();
        let tx = senders
            .entry(pane_id)
            .or_insert_with(|| broadcast::channel(64).0);
        tx.subscribe()
    };

    ws_upgrade.on_upgrade(move |socket| handle_ws(socket, pane_id, rx))
}

async fn handle_ws(
    socket: ws::WebSocket,
    pane_id: usize,
    mut rx: broadcast::Receiver<ScreenUpdate>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial screen snapshot
    if let Some(update) = capture_pane(pane_id) {
        if let Ok(json) = serde_json::to_string(&update) {
            if let Err(e) = sender.send(ws::Message::Text(json.into())).await {
                log::debug!(
                    "kaku-remote: failed to send initial snapshot for pane {}: {:?}",
                    pane_id,
                    e
                );
            }
        }
    }

    // Forward screen updates → WebSocket
    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    if let Ok(json) = serde_json::to_string(&update) {
                        if sender.send(ws::Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Forward client input → pane
    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            ws::Message::Text(t) => t.to_string(),
            ws::Message::Close(_) => break,
            _ => continue,
        };

        if let Ok(ClientMsg::Input { text: input, .. }) = serde_json::from_str(&text) {
            if input.len() > MAX_INPUT_LEN {
                log::warn!("kaku-remote: input too large ({} bytes), dropping", input.len());
                continue;
            }
            if let Some(mux) = Mux::try_get() {
                if let Some(pane) = mux.get_pane(mux::pane::PaneId::from(pane_id)) {
                    if let Err(e) = pane.writer().write_all(input.as_bytes()) {
                        log::debug!(
                            "kaku-remote: failed to write input to pane {}: {}",
                            pane_id,
                            e
                        );
                    }
                }
            }
        }
    }

    send_task.abort();
}

// ── Mux subscriber ────────────────────────────────────────────────────────────

fn on_pane_output(pane_id: usize, senders: PaneSenders, throttle: PaneThrottle) {
    let tx = {
        let guard = senders.lock();
        guard.get(&pane_id).cloned()
    };
    if let Some(tx) = tx {
        if tx.receiver_count() == 0 {
            return;
        }
        {
            let mut times = throttle.lock();
            let now = Instant::now();
            if let Some(last) = times.get(&pane_id) {
                if now.duration_since(*last) < CAPTURE_INTERVAL {
                    return;
                }
            }
            times.insert(pane_id, now);
        }
        if let Some(update) = capture_pane(pane_id) {
            if let Err(e) = tx.send(update) {
                log::debug!(
                    "kaku-remote: failed to broadcast update for pane {}: {:?}",
                    pane_id,
                    e
                );
            }
        }
    }
}

// ── State file (written by GUI, read by CLI) ──────────────────────────────────

fn state_path() -> std::path::PathBuf {
    std::env::temp_dir().join("kaku-remote.json")
}

fn write_state(port: u16, token: &str, tunnel_relay: Option<&str>) {
    let mut val = serde_json::json!({
        "port": port,
        "token": token,
    });
    if let Some(relay) = tunnel_relay {
        val["tunnel_relay"] = serde_json::Value::String(relay.to_string());
    }
    if let Ok(json) = serde_json::to_string(&val) {
        let path = state_path();
        if let Err(e) = std::fs::write(&path, json) {
            log::warn!("kaku-remote: failed to write state file: {}", e);
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

#[derive(serde::Deserialize)]
pub struct RemoteState {
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub tunnel_relay: Option<String>,
}

pub fn read_state() -> anyhow::Result<RemoteState> {
    let data = std::fs::read_to_string(state_path())
        .with_context(|| "Kaku remote bridge not running (state file not found)")?;
    serde_json::from_str(&data).context("invalid state file")
}

pub fn render_qr_terminal(host: &str, port: u16, token: &str) -> String {
    let url = format!("kakuremote://{}:{}?token={}", host, port, token);
    let qr = match QrCode::new(url.as_bytes()) {
        Ok(q) => q,
        Err(_) => return "Failed to generate QR code".to_string(),
    };
    let rendered = qr
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Dark)
        .light_color(qrcode::render::unicode::Dense1x2::Light)
        .quiet_zone(false)
        .build();
    format!("{}\n{}", rendered.trim_end(), url)
}

// ── Relay tunnel ──────────────────────────────────────────────────────────────

/// Render a TUI QR code for the relay connection URL.
/// URL scheme: `kakuremote://relay?server=<host>&token=<token>`
pub fn render_relay_qr_terminal(relay_server: &str, token: &str) -> String {
    let host = relay_server
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let url = format!("kakuremote://relay?server={}&token={}", host, token);
    let qr = match QrCode::new(url.as_bytes()) {
        Ok(q) => q,
        Err(_) => return "Failed to generate QR code".to_string(),
    };
    let rendered = qr
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Dark)
        .light_color(qrcode::render::unicode::Dense1x2::Light)
        .quiet_zone(false)
        .build();
    format!("{}\n{}", rendered.trim_end(), url)
}

/// Start the outbound relay tunnel.  Spawns a dedicated thread with its own
/// tokio runtime that maintains a persistent WSS connection to the relay host
/// endpoint, forwarding pane screen updates and routing client input back.
pub fn start_tunnel(tunnel_url: String) {
    let token = get_or_init_token().to_string();

    // One broadcast channel for all pane updates; tunnel subscribes per session.
    let (tx, _) = broadcast::channel::<ScreenUpdate>(128);
    let tx = Arc::new(tx);
    let tx_for_sub = tx.clone();

    if let Some(mux) = Mux::try_get() {
        mux.subscribe(move |notification| {
            if let MuxNotification::PaneOutput(pane_id) = notification {
                let pid: usize = pane_id.into();
                if tx_for_sub.receiver_count() > 0 {
                    if let Some(update) = capture_pane(pid) {
                        if let Err(e) = tx_for_sub.send(update) {
                            log::debug!("kaku-tunnel: failed to send update: {:?}", e);
                        }
                    }
                }
            }
            true
        });
    }

    std::thread::Builder::new()
        .name("kaku-tunnel".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("kaku-tunnel tokio runtime");
            rt.block_on(async move {
                let host_url = format!("{}/h/{}", tunnel_url, token);
                loop {
                    let rx = tx.subscribe();
                    log::info!("kaku-tunnel: connecting to {}", host_url);
                    match run_tunnel_session(&host_url, rx).await {
                        Ok(()) => log::info!("kaku-tunnel: session ended, reconnecting..."),
                        Err(e) => {
                            log::warn!("kaku-tunnel: error: {}, reconnecting in 5s", e)
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            });
        })
        .expect("spawn kaku-tunnel thread");
}

async fn run_tunnel_session(
    url: &str,
    mut rx: broadcast::Receiver<ScreenUpdate>,
) -> anyhow::Result<()> {
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let (ws_stream, _) = connect_async(url).await.context("tunnel connect")?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Send initial snapshots for all active panes so the client has content
    // immediately on connect.
    if let Some(mux) = Mux::try_get() {
        for pane in mux.iter_panes() {
            let pid: usize = pane.pane_id().into();
            if let Some(update) = capture_pane(pid) {
                if let Ok(json) = serde_json::to_string(&update) {
                    ws_tx.send(Message::Text(json.into())).await?;
                }
            }
        }
    }

    // Forward screen updates → relay → client
    let fwd = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    if let Ok(json) = serde_json::to_string(&update) {
                        if ws_tx.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Handle input from client (via relay) → pane PTY
    while let Some(msg) = ws_rx.next().await {
        match msg? {
            Message::Text(text) => {
                if let Ok(ClientMsg::Input {
                    pane_id,
                    text: input,
                }) = serde_json::from_str::<ClientMsg>(text.as_str())
                {
                    if input.len() > MAX_INPUT_LEN {
                        log::warn!("kaku-tunnel: input too large ({} bytes), dropping", input.len());
                        continue;
                    }
                    if let Some(mux) = Mux::try_get() {
                        // Route to the specified pane, falling back to the first pane.
                        let target = pane_id
                            .and_then(|id| mux.get_pane(mux::pane::PaneId::from(id)))
                            .or_else(|| mux.iter_panes().into_iter().next());
                        if let Some(pane) = target {
                            if let Err(e) = pane.writer().write_all(input.as_bytes()) {
                                log::debug!("kaku-remote: failed to write to pane: {}", e);
                            }
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    fwd.abort();
    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn start() {
    // install_default() returns Err if a provider is already installed; that is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cfg = configuration();
    if !cfg.remote.enabled {
        return;
    }

    let port = cfg.remote.port;
    let bind = cfg.remote.bind.clone();
    let tunnel = cfg.remote.tunnel;
    let tunnel_url = cfg.remote.tunnel_url.clone();
    let token = get_or_init_token();

    log::info!("kaku-remote: starting on {}:{} token={}", bind, port, token);

    let pane_senders: PaneSenders = Arc::new(Mutex::new(HashMap::new()));
    let senders_for_sub = pane_senders.clone();
    let pane_throttle: PaneThrottle = Arc::new(Mutex::new(HashMap::new()));
    let throttle_for_sub = pane_throttle.clone();

    if let Some(mux) = Mux::try_get() {
        mux.subscribe(move |notification| {
            match notification {
                MuxNotification::PaneOutput(pane_id) => {
                    on_pane_output(pane_id.into(), senders_for_sub.clone(), throttle_for_sub.clone());
                }
                MuxNotification::PaneRemoved(pane_id) => {
                    let id: usize = pane_id.into();
                    senders_for_sub.lock().remove(&id);
                    throttle_for_sub.lock().remove(&id);
                }
                _ => {}
            }
            true
        });
    }

    if tunnel {
        start_tunnel(tunnel_url.clone());
    }

    let tunnel_relay_opt: Option<String> = if tunnel { Some(tunnel_url) } else { None };
    std::thread::Builder::new()
        .name("kaku-remote".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("kaku-remote tokio runtime");

            rt.block_on(async move {
                let state = AppState { pane_senders };

                let app = Router::new()
                    .route("/api/config", get(route_config))
                    .route("/api/panes", get(route_panes))
                    .route("/qr", get(route_qr))
                    .route("/ws/{pane_id}", get(route_ws))
                    .with_state(state);

                let addr: SocketAddr = match format!("{}:{}", bind, port).parse() {
                    Ok(a) => a,
                    Err(e) => {
                        log::error!("kaku-remote: invalid bind address: {e}");
                        return;
                    }
                };

                log::info!("kaku-remote: listening on http://{}", addr);
                write_state(port, token, tunnel_relay_opt.as_deref());

                let listener = match tokio::net::TcpListener::bind(addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        log::error!("kaku-remote: failed to bind {addr}: {e}");
                        return;
                    }
                };
                if let Err(e) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                {
                    log::error!("kaku-remote: server error: {e}");
                }
            });
        })
        .expect("spawn kaku-remote thread");
}
