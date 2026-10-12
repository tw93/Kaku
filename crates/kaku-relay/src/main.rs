//! Relay between a Kaku on someone's Mac and the browsers opening its web link.
//!
//! The Mac connects out to `/host/{id}` and proves it owns `id`, which is the
//! first 128 bits of SHA-256 of a secret only the Mac holds. Browsers load
//! `/t/{id}/`, whose files the relay fetches from that Mac, and open
//! `/t/{id}/ws`. Messages between a browser and its Mac are sealed end to end
//! with a key carried in the link's fragment, so the relay only numbers the
//! browsers and forwards bytes it cannot read.

use axum::body::Bytes;
use axum::extract::{ws, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

const MAX_HOSTS: usize = 10_000;
const MAX_BROWSERS_PER_HOST: usize = 8;
const MAX_MESSAGE: usize = 4 << 20;
/// Messages queued toward one socket before it counts as stuck.
const QUEUE: usize = 256;
const ASSET_TIMEOUT: Duration = Duration::from_secs(10);
const KIND_DATA: u8 = 0;
const KIND_ASSET: u8 = 1;

// ── State ─────────────────────────────────────────────────────────────────────

struct Asset {
    status: u16,
    content_type: String,
    body: Bytes,
}

struct Host {
    /// Distinguishes this registration from a later one under the same id.
    generation: u64,
    to_host: mpsc::Sender<ws::Message>,
    browsers: Mutex<HashMap<u32, mpsc::Sender<ws::Message>>>,
    assets: Mutex<HashMap<u32, oneshot::Sender<Asset>>>,
    next_id: AtomicU32,
}

#[derive(Clone, Default)]
struct AppState {
    hosts: Arc<Mutex<HashMap<String, Arc<Host>>>>,
    generation: Arc<AtomicU64>,
}

impl AppState {
    fn host(&self, id: &str) -> Option<Arc<Host>> {
        self.hosts.lock().get(id).cloned()
    }
}

fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

fn owns(id: &str, secret: &str) -> bool {
    let digest = Sha256::digest(secret.as_bytes());
    let expected: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    expected == id
}

/// The fly edge that accepted the connection, so latency can be reasoned
/// about from logs: a Mac and its browsers entering far apart cost a long
/// hop per keystroke. Only the region code is recorded.
fn edge(headers: &HeaderMap) -> String {
    headers
        .get("fly-region")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string()
}

/// Browser and OS family from the User-Agent, enough to tell a laptop from
/// a phone in the logs without recording anything that identifies a person.
fn device(headers: &HeaderMap) -> &'static str {
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let os = if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Mac OS X") || ua.contains("Macintosh") {
        "macOS"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "other"
    };
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        "Chrome"
    } else if ua.contains("Firefox/") || ua.contains("FxiOS/") {
        "Firefox"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "other"
    };
    match (browser, os) {
        ("Chrome", "Windows") => "Chrome Windows",
        ("Chrome", "macOS") => "Chrome macOS",
        ("Chrome", "Android") => "Chrome Android",
        ("Chrome", "iPhone") => "Chrome iPhone",
        ("Edge", "Windows") => "Edge Windows",
        ("Edge", "macOS") => "Edge macOS",
        ("Safari", "macOS") => "Safari macOS",
        ("Safari", "iPhone") => "Safari iPhone",
        ("Safari", "iPad") => "Safari iPad",
        ("Firefox", "Windows") => "Firefox Windows",
        ("Firefox", "macOS") => "Firefox macOS",
        (_, "iPhone") => "iPhone",
        (_, "iPad") => "iPad",
        (_, "Android") => "Android",
        (_, "Windows") => "Windows",
        (_, "macOS") => "macOS",
        _ => "other",
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

// ── Host side ─────────────────────────────────────────────────────────────────

async fn host_ws(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    let secret = headers
        .get("x-kaku-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !valid_id(&id) || !owns(&id, secret) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if state.hosts.lock().len() >= MAX_HOSTS && state.host(&id).is_none() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    upgrade
        .max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| serve_host(socket, id, edge(&headers), state))
}

async fn serve_host(socket: ws::WebSocket, id: String, edge: String, state: AppState) {
    let (mut sink, mut stream) = socket.split();
    let (to_host, mut outgoing) = mpsc::channel::<ws::Message>(QUEUE);
    let host = Arc::new(Host {
        generation: state.generation.fetch_add(1, Ordering::Relaxed),
        to_host,
        browsers: Mutex::new(HashMap::new()),
        assets: Mutex::new(HashMap::new()),
        next_id: AtomicU32::new(1),
    });
    // The newest registration wins; the old one's browsers are dropped.
    if let Some(old) = state.hosts.lock().insert(id.clone(), host.clone()) {
        old.browsers.lock().clear();
    }
    info!("host {} registered via edge {}", short(&id), edge);

    let writer = tokio::spawn(async move {
        while let Some(msg) = outgoing.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            ws::Message::Binary(bytes) if bytes.len() >= 5 => {
                let kind = bytes[0];
                let target = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                match kind {
                    KIND_DATA => {
                        let tx = host.browsers.lock().get(&target).cloned();
                        if let Some(tx) = tx {
                            let payload = bytes.slice(5..);
                            if tx.try_send(ws::Message::Binary(payload)).is_err() {
                                // A browser that cannot keep up is let go.
                                host.browsers.lock().remove(&target);
                            }
                        }
                    }
                    KIND_ASSET => {
                        if let Some(asset) = parse_asset(bytes.slice(5..)) {
                            if let Some(waiter) = host.assets.lock().remove(&target) {
                                let _ = waiter.send(asset);
                            }
                        }
                    }
                    _ => {}
                }
            }
            ws::Message::Text(text) => {
                // The Mac rejected a browser (its key did not match).
                if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(text.as_str()) {
                    if map.get("t").and_then(|t| t.as_str()) == Some("close") {
                        if let Some(c) = map.get("c").and_then(|c| c.as_u64()) {
                            host.browsers.lock().remove(&(c as u32));
                        }
                    }
                }
            }
            ws::Message::Close(_) => break,
            _ => {}
        }
    }

    writer.abort();
    let mut hosts = state.hosts.lock();
    if hosts
        .get(&id)
        .is_some_and(|current| current.generation == host.generation)
    {
        hosts.remove(&id);
    }
    drop(hosts);
    host.browsers.lock().clear();
    info!("host {} gone", short(&id));
}

fn parse_asset(payload: Bytes) -> Option<Asset> {
    if payload.len() < 3 {
        return None;
    }
    let status = u16::from_be_bytes([payload[0], payload[1]]);
    let type_len = payload[2] as usize;
    if payload.len() < 3 + type_len {
        return None;
    }
    let content_type = String::from_utf8(payload[3..3 + type_len].to_vec()).ok()?;
    Some(Asset {
        status,
        content_type,
        body: payload.slice(3 + type_len..),
    })
}

// ── Browser side ──────────────────────────────────────────────────────────────

const OFFLINE_PAGE: &str = "<!doctype html><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Kaku</title><body style=\"font:15px -apple-system,sans-serif;background:#16161a;\
color:#999;display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">\
This Kaku is offline. Open Kaku on the Mac and reload.</body>";

async fn fetch_asset(state: &AppState, id: &str, path: &str) -> Response {
    let Some(host) = state.host(id) else {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            OFFLINE_PAGE,
        )
            .into_response();
    };
    let request = host.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    host.assets.lock().insert(request, tx);
    let get = serde_json::json!({ "t": "get", "r": request, "p": path }).to_string();
    if host
        .to_host
        .try_send(ws::Message::Text(get.into()))
        .is_err()
    {
        host.assets.lock().remove(&request);
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    match tokio::time::timeout(ASSET_TIMEOUT, rx).await {
        Ok(Ok(asset)) if asset.status == 200 => (
            [
                (header::CONTENT_TYPE, asset.content_type),
                // The page always comes fresh from the Mac; xterm.js and the
                // fonts are ~1 MB that rarely change, so a reload skips them.
                (
                    header::CACHE_CONTROL,
                    if path == "index.html" {
                        "no-cache"
                    } else {
                        "max-age=3600"
                    }
                    .to_string(),
                ),
                (header::REFERRER_POLICY, "no-referrer".to_string()),
            ],
            asset.body,
        )
            .into_response(),
        Ok(Ok(_)) => StatusCode::NOT_FOUND.into_response(),
        _ => {
            host.assets.lock().remove(&request);
            StatusCode::GATEWAY_TIMEOUT.into_response()
        }
    }
}

async fn page_redirect(Path(id): Path<String>) -> Response {
    // Keep the trailing slash so the page's relative paths resolve under it;
    // the browser carries the fragment (the key) over the redirect.
    Redirect::permanent(&format!("/t/{id}/")).into_response()
}

async fn page(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    fetch_asset(&state, &id, "index.html").await
}

async fn page_asset(
    Path((id, name)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    let safe = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_');
    if !valid_id(&id) || !safe {
        return StatusCode::NOT_FOUND.into_response();
    }
    fetch_asset(&state, &id, &format!("assets/{name}")).await
}

async fn browser_ws(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    let Some(host) = state.host(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if host.browsers.lock().len() >= MAX_BROWSERS_PER_HOST {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    upgrade
        .max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| async move {
            info!(
                "browser joined host {} via edge {} from {}",
                short(&id),
                edge(&headers),
                device(&headers)
            );
            serve_browser(socket, host).await
        })
}

async fn serve_browser(socket: ws::WebSocket, host: Arc<Host>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut outgoing) = mpsc::channel::<ws::Message>(QUEUE);
    let c = host.next_id.fetch_add(1, Ordering::Relaxed);
    host.browsers.lock().insert(c, tx);
    let open = serde_json::json!({ "t": "open", "c": c }).to_string();
    if host
        .to_host
        .send(ws::Message::Text(open.into()))
        .await
        .is_err()
    {
        host.browsers.lock().remove(&c);
        return;
    }

    // Ends when the host drops this browser (its sender goes away).
    let writer = tokio::spawn(async move {
        while let Some(msg) = outgoing.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    loop {
        tokio::select! {
            msg = stream.next() => {
                let Some(Ok(msg)) = msg else { break };
                match msg {
                    ws::Message::Binary(payload) => {
                        let mut frame = Vec::with_capacity(5 + payload.len());
                        frame.push(KIND_DATA);
                        frame.extend_from_slice(&c.to_be_bytes());
                        frame.extend_from_slice(&payload);
                        if host.to_host.try_send(ws::Message::Binary(frame.into())).is_err() {
                            break;
                        }
                    }
                    ws::Message::Close(_) => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if !host.browsers.lock().contains_key(&c) {
                    break;
                }
            }
        }
    }

    host.browsers.lock().remove(&c);
    writer.abort();
    let close = serde_json::json!({ "t": "close", "c": c }).to_string();
    let _ = host.to_host.try_send(ws::Message::Text(close.into()));
}

async fn health() -> &'static str {
    "ok"
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn app(state: AppState) -> Router {
    Router::new()
        .route("/host/{id}", get(host_ws))
        .route("/t/{id}", get(page_redirect))
        .route("/t/{id}/", get(page))
        .route("/t/{id}/ws", get(browser_ws))
        .route("/t/{id}/assets/{name}", get(page_asset))
        .route("/health", get(health))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kaku_relay=info".into()),
        )
        .init();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("kaku-relay listening on {}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kaku-relay: failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    // Keystrokes and echoes are tiny frames; Nagle would hold them back.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
        if let Err(e) = tcp.set_nodelay(true) {
            tracing::warn!("set_nodelay failed: {e}");
        }
    });
    if let Err(e) = axum::serve(listener, app(AppState::default())).await {
        eprintln!("kaku-relay: server error: {}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_browser_and_os_only() {
        let ua = |s: &str| {
            let mut h = HeaderMap::new();
            h.insert(axum::http::header::USER_AGENT, s.parse().unwrap());
            device(&h)
        };
        assert_eq!(ua("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36"), "Chrome macOS");
        assert_eq!(ua("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1"), "Safari iPhone");
        assert_eq!(ua("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0"), "Edge Windows");
        assert_eq!(device(&HeaderMap::new()), "other");
    }

    #[test]
    fn only_the_secret_owner_matches_an_id() {
        let secret = "a".repeat(64);
        let digest = Sha256::digest(secret.as_bytes());
        let id: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        assert!(valid_id(&id));
        assert!(owns(&id, &secret));
        assert!(!owns(&id, "b"));
        assert!(!owns(&id, ""));
    }

    #[test]
    fn asset_frames_round_trip() {
        let mut payload = vec![0, 200, 9];
        payload.extend_from_slice(b"text/html");
        payload.extend_from_slice(b"<p>hi</p>");
        let asset = parse_asset(Bytes::from(payload)).unwrap();
        assert_eq!(asset.status, 200);
        assert_eq!(asset.content_type, "text/html");
        assert_eq!(&asset.body[..], b"<p>hi</p>");
        assert!(parse_asset(Bytes::from_static(&[0, 200, 50])).is_none());
    }

    #[test]
    fn ids_must_be_128_bit_hex() {
        assert!(!valid_id("short"));
        assert!(!valid_id(&"g".repeat(32)));
        assert!(valid_id(&"0f".repeat(16)));
    }
}
