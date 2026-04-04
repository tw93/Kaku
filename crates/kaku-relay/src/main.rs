use axum::{
    Router,
    extract::{Path, State, ws},
    response::Response,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tracing::info;

/// Maximum concurrent pending host connections to prevent memory exhaustion.
const MAX_PENDING: usize = 1_000;

// ── State ─────────────────────────────────────────────────────────────────────

/// A host has connected and is waiting for a client.
/// We hold the oneshot sender; when the client connects it sends its WebSocket
/// through the channel, then the host task starts the forwarding loop.
type HostWaiter = oneshot::Sender<ws::WebSocket>;

#[derive(Clone)]
struct AppState {
    pending: Arc<Mutex<HashMap<String, HostWaiter>>>,
}

// ── Routes ────────────────────────────────────────────────────────────────────

/// Desktop Kaku connects here and waits up to 10 minutes for a mobile client.
async fn host_ws(
    Path(token): Path<String>,
    State(state): State<AppState>,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        let (tx, rx) = oneshot::channel::<ws::WebSocket>();
        {
            let mut map = state.pending.lock();
            if map.len() >= MAX_PENDING {
                info!("host rejected: pending map full ({})", MAX_PENDING);
                return;
            }
            map.insert(token.clone(), tx);
        }

        info!("host connected: {}", &token[..8.min(token.len())]);

        // Wait for the client to connect (10 min timeout)
        match tokio::time::timeout(Duration::from_secs(600), rx).await {
            Ok(Ok(client_socket)) => {
                info!("client paired: {}", &token[..8.min(token.len())]);
                relay_pair(socket, client_socket).await;
            }
            _ => {
                // Timeout or channel dropped — clean up
                state.pending.lock().remove(&token);
                info!("host timed out: {}", &token[..8.min(token.len())]);
            }
        }
    })
}

/// Mobile client connects here.  If no host is waiting yet the client polls
/// for up to 30 s before giving up, giving the desktop time to (re)connect.
async fn client_ws(
    Path(token): Path<String>,
    State(state): State<AppState>,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        let short = &token[..8.min(token.len())];
        info!("client connected: {}", short);

        // Poll every 500 ms for up to 30 s for the host to appear.
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        interval.tick().await; // consume the immediate first tick

        loop {
            if let Some(tx) = state.pending.lock().remove(&token) {
                info!("client paired: {}", short);
                let _ = tx.send(socket);
                return;
            }

            tokio::select! {
                _ = &mut deadline => {
                    info!("client: no host after 30s for {}", short);
                    return;
                }
                _ = interval.tick() => {}
            }
        }
    })
}

async fn health() -> &'static str {
    "ok"
}

// ── Relay ─────────────────────────────────────────────────────────────────────

/// Forward WebSocket messages between host and client until either side closes.
async fn relay_pair(host: ws::WebSocket, client: ws::WebSocket) {
    let (mut host_tx, mut host_rx) = host.split();
    let (mut client_tx, mut client_rx) = client.split();

    // host → client
    let h2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = host_rx.next().await {
            if matches!(msg, ws::Message::Close(_)) {
                break;
            }
            if client_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    // client → host
    let c2h = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_rx.next().await {
            if matches!(msg, ws::Message::Close(_)) {
                break;
            }
            if host_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Stop both directions when either side disconnects
    tokio::select! {
        _ = h2c => {}
        _ = c2h => {}
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kaku_relay=info".into()),
        )
        .init();

    let state = AppState {
        pending: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/h/{token}", get(host_ws))
        .route("/c/{token}", get(client_ws))
        .route("/health", get(health))
        .with_state(state);

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
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("kaku-relay: server error: {}", e);
        std::process::exit(1);
    }
}
