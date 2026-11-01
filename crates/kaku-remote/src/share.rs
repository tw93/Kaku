//! This Kaku, reached through the relay by the browsers paired with it.
//!
//! Kaku keeps a share identity on disk: a secret that registers this Mac with
//! the relay and a key that signs each connection's exchange (see
//! `pairing`). The secret goes only to the relay, over TLS, when this Mac
//! registers; the relay checks it against the id (SHA-256 of the secret) and
//! keeps nothing, so nobody else can register as this Mac, and it only ever
//! forwards sealed bytes. Someone holding the secret could take the id
//! offline, never a terminal: pages trust only this Mac's signing key. Resetting the identity forgets every paired browser: the running
//! Kaku notices the new file and re-registers under the new id.

use crate::pairing::{self, Exchange, MacKey, PageAuth, SessionKeys};
use crate::web::{self, WebDirty};
use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use ring::aead::{Aad, LessSafeKey, Nonce, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const TO_HOST: &[u8] = b"kaku-web c2h";
const FROM_HOST: &[u8] = b"kaku-web h2c";
const KIND_DATA: u8 = 0;
const KIND_ASSET: u8 = 1;
const IDENTITY_CHECK: Duration = Duration::from_secs(3);
/// How often a browser waiting to pair checks for the answer.
const CONFIRM_CHECK: Duration = Duration::from_millis(300);
/// Pings keep NATs and proxies from dropping an idle link, and their pongs
/// prove the relay is still there.
const KEEPALIVE: Duration = Duration::from_secs(20);
/// With pings every 20s, this long without hearing from the relay means the
/// link died without a close (sleep, network change); reconnect.
const SILENCE_LIMIT: Duration = Duration::from_secs(60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const INBOX_TIMEOUT: Duration = Duration::from_secs(2);
/// Input a busy session holds before `INBOX_TIMEOUT` starts counting.
const SESSION_INBOX: usize = 64;
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Longest a connect, TLS and WebSocket handshake may take. A proxy can
/// accept the TCP connection locally and then never answer once its own
/// upstream is gone; without a limit the bridge waited there for hours and
/// the link stayed offline until Kaku restarted.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// After the relay says it is restarting (a deploy), how long to keep
/// retrying every `RESTART_RETRY` instead of backing off: a doubling backoff
/// that misses the first second or two of the new relay could keep this Mac
/// offline for up to `MAX_BACKOFF` after it is back.
const RESTART_WINDOW: Duration = Duration::from_secs(60);
const RESTART_RETRY: Duration = Duration::from_secs(1);

// ── Identity ──────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareIdentity {
    /// Registers this Mac with the relay; sent only to the relay, which keeps
    /// nothing of it.
    secret: String,
    /// This Mac's ECDSA P-256 signing key, PKCS#8 base64url. Never leaves the
    /// Mac; pairing links carry its public half. Identities from before
    /// pairing have none (and a `key` field that is no longer read) and get
    /// one on load.
    #[serde(default)]
    mac_key: String,
}

impl ShareIdentity {
    fn generate() -> anyhow::Result<Self> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).context("random secret")?;
        Ok(Self {
            secret: hex(&secret),
            mac_key: pairing::generate_mac_key()?,
        })
    }

    /// The relay's name for this Mac: the first 128 bits of SHA-256(secret).
    pub fn id(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, self.secret.as_bytes());
        hex(&digest.as_ref()[..16])
    }

    fn mac_key(&self) -> anyhow::Result<MacKey> {
        MacKey::from_text(&self.mac_key)
    }

    /// This Mac's page through the relay at `relay_url` (ws:// or wss://).
    /// Opening it pairs nothing; a paired browser bookmarks it.
    pub fn page_url(&self, relay_url: &str) -> String {
        let base = if relay_url.trim_end_matches('/') == crate::DEFAULT_RELAY {
            crate::DEFAULT_PAGE.to_string()
        } else {
            http_base(relay_url)
        };
        format!("{base}/{}/", self.id())
    }

    /// A link that pairs one more browser, once, within
    /// `pairing::OFFER_LIFETIME`; it replaces any earlier unused one.
    pub fn pairing_link(&self, relay_url: &str) -> anyhow::Result<String> {
        let secret = pairing::new_offer()?;
        // The page learns the key in the handshake; refuse to print a link
        // this Mac could not then answer.
        self.mac_key()?;
        Ok(format!(
            "{}{}",
            self.page_url(relay_url),
            pairing::pairing_fragment(&secret)
        ))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn http_base(relay_url: &str) -> String {
    let url = relay_url.trim_end_matches('/');
    if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        url.to_string()
    }
}

fn identity_path() -> PathBuf {
    config::DATA_DIR.join("remote-share.json")
}

fn read_identity() -> Option<ShareIdentity> {
    let data = std::fs::read_to_string(identity_path()).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_identity(identity: &ShareIdentity) -> anyhow::Result<()> {
    let path = identity_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("create data dir")?;
    }
    let json = serde_json::to_string(identity)?;
    crate::write_private(&path, json.as_bytes()).context("save share identity")
}

fn access_path() -> PathBuf {
    config::DATA_DIR.join("remote-access")
}

/// A short fingerprint of an access code, recorded with each attempt so
/// `kaku remote` knows the relay's answer was to the code it just saved.
pub fn access_fingerprint(code: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, code.trim().as_bytes());
    digest.as_ref()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The fingerprint of the code the latest attempt presented ("" for none).
static TRIED_ACCESS: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

pub(crate) fn tried_access() -> String {
    TRIED_ACCESS.lock().clone()
}

/// The relay's access code, once `kaku remote` asked for it.
fn read_access_code() -> Option<String> {
    let code = std::fs::read_to_string(access_path()).ok()?;
    let code = code.trim();
    (!code.is_empty()).then(|| code.to_string())
}

/// Keep the relay's access code for the bridge to present. The running Kaku
/// notices the new file within a second and tries again.
pub fn save_access_code(code: &str) -> anyhow::Result<()> {
    let code = code.trim();
    anyhow::ensure!(!code.is_empty(), "the access code is empty");
    anyhow::ensure!(
        code.len() <= 256 && code.bytes().all(|b| (b' '..=b'~').contains(&b)),
        "an access code is letters, digits and punctuation"
    );
    let path = access_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("create data dir")?;
    }
    crate::write_private(&path, code.as_bytes()).context("save access code")
}

/// Marks a refusal over the access code in the relay status, which
/// `RelayStatus::needs_access_code` looks for.
pub(crate) const ACCESS_REFUSAL: &str = "access code";

/// This Kaku's share identity, created on first use.
pub fn load_or_create_identity() -> anyhow::Result<ShareIdentity> {
    if let Some(mut identity) = read_identity() {
        if identity.mac_key.is_empty() {
            identity.mac_key = pairing::generate_mac_key()?;
            write_identity(&identity)?;
        }
        return Ok(identity);
    }
    let identity = ShareIdentity::generate()?;
    write_identity(&identity)?;
    Ok(identity)
}

/// Replace the identity and forget every paired browser; their sessions end
/// once the running Kaku re-registers, within a few seconds.
pub fn reset_identity() -> anyhow::Result<ShareIdentity> {
    let identity = ShareIdentity::generate()?;
    write_identity(&identity)?;
    pairing::forget_all_devices()?;
    Ok(identity)
}

// ── Sealing ───────────────────────────────────────────────────────────────────

/// What a message is bound to besides its connection's key: its direction
/// and its position, so the relay cannot reflect, repeat or reorder messages.
/// Each connection has its own keys, so nothing moves between connections.
fn aad(label: &[u8], seq: u64) -> Vec<u8> {
    let mut aad = label.to_vec();
    aad.extend_from_slice(&seq.to_be_bytes());
    aad
}

fn seal(key: &LessSafeKey, rng: &SystemRandom, aad: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut nonce).ok()?;
    let mut buf = plain.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(aad),
        &mut buf,
    )
    .ok()?;
    let mut out = Vec::with_capacity(NONCE_LEN + buf.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&buf);
    Some(out)
}

fn open(key: &LessSafeKey, aad: &[u8], sealed: &[u8]) -> Option<String> {
    if sealed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut buf = body.to_vec();
    let plain = key.open_in_place(nonce, Aad::from(aad), &mut buf).ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

// ── One browser's connection ──────────────────────────────────────────────────

/// Where a browser's connection is. Kaku sends its hello when the browser
/// connects; the page answers with its own, Kaku signs the exchange, the page
/// proves it is a paired device (or pairs with a link), and only then does a
/// session start. Nothing reaches the terminal before that.
enum Stage {
    Hello(Exchange),
    Proof {
        transcript: [u8; 32],
        keys: Arc<SessionKeys>,
    },
    /// A new browser with a valid link, waiting for Enter in `kaku remote`.
    /// What it sends meanwhile (its hello, attach) is kept for the session.
    Confirm {
        keys: Arc<SessionKeys>,
        device: pairing::Device,
        held: Vec<String>,
        since: tokio::time::Instant,
    },
    Session {
        keys: Arc<SessionKeys>,
        device: String,
        inbox: Option<SessionInput>,
    },
    Gone,
}

struct Browser {
    stage: Stage,
    /// Position of the next message expected from this browser.
    next_in: u64,
    connected_at: tokio::time::Instant,
}

/// What to do with a browser after its latest message.
enum Step {
    Nothing,
    /// Send it these bytes.
    Send(Vec<u8>),
    /// Turn it away with this close code.
    Close(u16),
    /// Send it these bytes, then turn it away with this close code.
    Refuse(Vec<u8>, u16),
    /// It proved it is `device` (`new`: paired just now); start its session,
    /// numbering Kaku's messages from `first_seq`, and hand it `held`.
    Admitted {
        device: pairing::Device,
        new: bool,
        first_seq: u64,
        held: Vec<String>,
    },
    /// A message for its session.
    Input(String),
}

/// Messages a page waiting for confirmation may have kept for its session.
const HELD_WHILE_CONFIRMING: usize = 16;

/// How long a new browser may wait for Enter in `kaku remote`; the link
/// itself expires sooner unless it was printed just before.
const CONFIRM_LIMIT: Duration = pairing::OFFER_LIFETIME;

/// Whether the person at the Mac answered a browser waiting to pair: the
/// step to take, or None while it still waits.
fn resolve_pending(browser: &mut Browser, store: &mut dyn pairing::DeviceStore) -> Option<Step> {
    let Stage::Confirm { device, since, .. } = &browser.stage else {
        return None;
    };
    let answer = match store.answer(device) {
        None if since.elapsed() < CONFIRM_LIMIT => return None,
        None => false,
        Some(allowed) => allowed,
    };
    let Stage::Confirm {
        keys, device, held, ..
    } = std::mem::replace(&mut browser.stage, Stage::Gone)
    else {
        return None;
    };
    store.settle(&device, answer);
    if !answer {
        return Some(Step::Close(pairing::DECLINED));
    }
    browser.stage = Stage::Session {
        keys,
        device: device.id.clone(),
        inbox: None,
    };
    // Message 0 was Kaku's signature, 1 the note that it waits.
    Some(Step::Admitted {
        device,
        new: true,
        first_seq: 2,
        held,
    })
}

/// The page's first message is its hello; one that is not (a `KCB2`
/// challenge, or a sealed message straight away) is a page from before
/// pairing, open across an update. It is told to reload, which shows the
/// new page and its pairing prompt.
const OLD_PAGE: u16 = 4426;

fn advance(
    browser: &mut Browser,
    data: &[u8],
    host_id: &str,
    mac_key: &MacKey,
    rng: &SystemRandom,
    store: &mut dyn pairing::DeviceStore,
) -> Step {
    match std::mem::replace(&mut browser.stage, Stage::Gone) {
        Stage::Hello(exchange) => {
            if !data.starts_with(pairing::PAGE_HELLO) {
                return Step::Close(OLD_PAGE);
            }
            let Some((transcript, keys)) = exchange.finish(host_id, data) else {
                return Step::Close(4403);
            };
            let Some(sig) = mac_key.sign(&transcript) else {
                return Step::Close(4403);
            };
            let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
            // The key, for a page that has only the pairing link, with the
            // tag that lets it check the key against the link's secret.
            let mut auth = serde_json::json!({
                "type": "mac_auth",
                "sig": b64(&sig),
                "key": b64(mac_key.public_key()),
            });
            if let Some(secret) = store.offer_secret() {
                auth["tag"] = b64(&pairing::mac_key_tag(
                    &secret,
                    &transcript,
                    mac_key.public_key(),
                ))
                .into();
            }
            let auth = auth.to_string();
            let Some(sealed) = seal(&keys.from_host, rng, &aad(FROM_HOST, 0), auth.as_bytes())
            else {
                return Step::Close(4403);
            };
            browser.stage = Stage::Proof {
                transcript,
                keys: Arc::new(keys),
            };
            Step::Send(sealed)
        }
        Stage::Proof { transcript, keys } => {
            let Some(text) = open(&keys.to_host, &aad(TO_HOST, browser.next_in), data) else {
                return Step::Close(4403);
            };
            browser.next_in += 1;
            let Ok(auth) = serde_json::from_str::<PageAuth>(&text) else {
                return Step::Close(4403);
            };
            if serde_json::from_str::<PageCaps>(&text).is_ok_and(|caps| caps.deflate) {
                let _ = keys.deflate.set(parking_lot::Mutex::new(flate2::Compress::new(
                    flate2::Compression::default(),
                    false,
                )));
            }
            match pairing::admit(&auth, &transcript, store) {
                Ok(pairing::Admission::Known(device)) => {
                    browser.stage = Stage::Session {
                        keys,
                        device: device.id.clone(),
                        inbox: None,
                    };
                    // Kaku's own signature was message 0.
                    Step::Admitted {
                        device,
                        new: false,
                        first_seq: 1,
                        held: Vec::new(),
                    }
                }
                Ok(pairing::Admission::Pending(device)) => {
                    let note = r#"{"type":"pair_pending"}"#;
                    let Some(sealed) = seal(&keys.from_host, rng, &aad(FROM_HOST, 1), note.as_bytes())
                    else {
                        return Step::Close(4403);
                    };
                    browser.stage = Stage::Confirm {
                        keys,
                        device,
                        held: Vec::new(),
                        since: tokio::time::Instant::now(),
                    };
                    Step::Send(sealed)
                }
                // Sealed, so a page forgets its key only when this Mac says
                // so, never on a close code anyone on the way can send.
                Err(pairing::Refusal::UnknownDevice) => {
                    let note = r#"{"type":"revoked"}"#;
                    match seal(&keys.from_host, rng, &aad(FROM_HOST, 1), note.as_bytes()) {
                        Some(sealed) => Step::Refuse(sealed, 4403),
                        None => Step::Close(4403),
                    }
                }
                Err(refusal) => Step::Close(refusal.close_code()),
            }
        }
        Stage::Confirm {
            keys,
            device,
            mut held,
            since,
        } => {
            let Some(text) = open(&keys.to_host, &aad(TO_HOST, browser.next_in), data) else {
                return Step::Close(4403);
            };
            browser.next_in += 1;
            // Hello and attach come first; past this, what a waiting page
            // sends is not needed and would only pile up.
            if held.len() < HELD_WHILE_CONFIRMING {
                held.push(text);
            }
            browser.stage = Stage::Confirm {
                keys,
                device,
                held,
                since,
            };
            Step::Nothing
        }
        Stage::Session {
            keys,
            device,
            inbox,
        } => {
            let opened = open(&keys.to_host, &aad(TO_HOST, browser.next_in), data);
            browser.stage = Stage::Session {
                keys,
                device,
                inbox,
            };
            match opened {
                Some(text) => {
                    browser.next_in += 1;
                    Step::Input(text)
                }
                // Tampered, replayed or out of order.
                None => Step::Close(4403),
            }
        }
        Stage::Gone => Step::Nothing,
    }
}

/// The relay's refusal as `kaku remote` and `kaku doctor` print it. The
/// relay is not trusted with the terminal: escape sequences in its text
/// (a clipboard write, a title, a link) are dropped, not printed.
fn refusal_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body)
        .chars()
        .filter(|&c| !c.is_control() && !pairing::is_invisible_format(c))
        .take(120)
        .collect::<String>()
        .trim()
        .to_string()
}

/// What else the page's proof says it can do. Older pages say nothing.
#[derive(Deserialize)]
struct PageCaps {
    #[serde(default)]
    deflate: bool,
}

/// First byte of a deflated message; JSON starts with `{` and a desktop
/// picture with 0, which stays as it is, being video already.
const DEFLATED: u8 = 1;

/// A text message for a page that inflates: `DEFLATED`, then the message
/// and a newline through the session's one raw deflate stream, flushed at
/// the end. The stream's window spans messages, so a frame that repeats
/// the last one's escapes and text costs a few bytes; the newline, which
/// JSON never contains, ends each message in the page's output.
fn deflate(stream: &parking_lot::Mutex<flate2::Compress>, plain: &[u8]) -> Option<Vec<u8>> {
    let mut stream = stream.lock();
    let input = [plain, b"\n"].concat();
    let mut out = Vec::with_capacity(input.len() / 2 + 64);
    out.push(DEFLATED);
    let start = stream.total_in();
    // The flush is complete once a call leaves room in the buffer.
    loop {
        out.reserve(out.capacity().max(64));
        let done = (stream.total_in() - start) as usize;
        stream
            .compress_vec(&input[done..], &mut out, flate2::FlushCompress::Sync)
            .ok()?;
        if (stream.total_in() - start) as usize == input.len() && out.len() < out.capacity() {
            return Some(out);
        }
    }
}

// ── Relay framing ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum RelayMsg {
    /// A browser opened the page's socket.
    Open { c: u32 },
    /// That browser went away.
    Close { c: u32 },
    /// The relay needs one of the page's files.
    Get { r: u32, p: String },
}

fn frame(kind: u8, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(kind);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn asset_frame(request: u32, path: &str) -> Vec<u8> {
    let (status, content_type, body): (u16, &str, &[u8]) = match web::asset(path) {
        Some((content_type, body)) => (200, content_type, body),
        None => (404, "", b""),
    };
    let mut payload = Vec::with_capacity(3 + content_type.len() + body.len());
    payload.extend_from_slice(&status.to_be_bytes());
    payload.push(content_type.len() as u8);
    payload.extend_from_slice(content_type.as_bytes());
    payload.extend_from_slice(body);
    frame(KIND_ASSET, request, &payload)
}

struct AssetRequests {
    requests: mpsc::Sender<(u32, String)>,
    worker: tokio::task::JoinHandle<()>,
}

impl AssetRequests {
    fn new(output: mpsc::Sender<Outbound>) -> Self {
        Self::waiting(output, WRITE_TIMEOUT)
    }

    fn waiting(output: mpsc::Sender<Outbound>, patience: Duration) -> Self {
        let (requests, mut pending) = mpsc::channel::<(u32, String)>(16);
        let worker = tokio::spawn(async move {
            while let Some((request, path)) = pending.recv().await {
                // Keep only names while the socket is busy. Construct the
                // asset only after reserving its one slot in the bulk queue.
                // A busy socket drops this request; only a closed one ends
                // the worker, or every later file would go unanswered.
                let slot = match tokio::time::timeout(patience, output.reserve()).await {
                    Ok(Ok(slot)) => slot,
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                };
                slot.send(Outbound::Protocol(Message::Binary(
                    asset_frame(request, &path).into(),
                )));
            }
        });
        Self { requests, worker }
    }

    fn try_send(&self, request: u32, path: String) -> bool {
        self.requests.try_send((request, path)).is_ok()
    }
}

impl Drop for AssetRequests {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

// ── Tunnel ────────────────────────────────────────────────────────────────────

/// Keep this Kaku registered with the relay for as long as the bridge runs.
pub(crate) async fn run(relay_url: String, dirty: WebDirty) {
    let mut backoff = Duration::from_secs(1);
    let mut restart_until: Option<tokio::time::Instant> = None;
    loop {
        let identity = match load_or_create_identity() {
            Ok(identity) => identity,
            Err(e) => {
                log::warn!("kaku-share: no share identity: {e:#}");
                tokio::time::sleep(MAX_BACKOFF).await;
                continue;
            }
        };
        let mut connected_at = None;
        match serve(&relay_url, &identity, &dirty, &mut connected_at).await {
            Ok(ended) => {
                backoff = Duration::from_secs(1);
                if ended == Ended::RelayRestarting {
                    log::info!("kaku-share: relay is restarting, retrying every second");
                    restart_until = Some(tokio::time::Instant::now() + RESTART_WINDOW);
                }
                crate::record_relay_status(&relay_url, false, None);
            }
            Err(e) => {
                log::info!("kaku-share: relay connection ended: {e:#}");
                crate::record_relay_status(&relay_url, false, Some(format!("{e:#}")));
            }
        }
        backoff = retry_delay(
            backoff,
            connected_at.map(|at: tokio::time::Instant| at.elapsed()),
        );
        if restart_until.is_some_and(|until| tokio::time::Instant::now() < until) {
            tokio::time::sleep(RESTART_RETRY).await;
            continue;
        }
        restart_until = None;
        if wait_unless_access_changes(backoff).await {
            backoff = Duration::from_secs(1);
        } else {
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}

/// Sleep `delay`, or less when the saved access code is not the one the
/// last attempt presented, so the person typing it is not left waiting out a
/// 30 s backoff. Compared with what was tried, not with the file at the
/// start of the wait, so a code saved during the attempt also wakes it.
/// True when it did.
async fn wait_unless_access_changes(delay: Duration) -> bool {
    let saved = || {
        read_access_code()
            .map(|c| access_fingerprint(&c))
            .unwrap_or_default()
    };
    let deadline = tokio::time::Instant::now() + delay;
    while tokio::time::Instant::now() < deadline {
        if saved() != tried_access() {
            return true;
        }
        let step = (deadline - tokio::time::Instant::now()).min(Duration::from_secs(1));
        tokio::time::sleep(step).await;
    }
    false
}

/// How a relay connection that did not fail ended.
#[derive(Debug, PartialEq)]
enum Ended {
    Closed,
    /// The relay closed with 1012 (service restart) on its way down.
    RelayRestarting,
}

impl Ended {
    fn from_close(frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> Self {
        use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
        match frame {
            Some(frame) if frame.code == CloseCode::Restart => Ended::RelayRestarting,
            _ => Ended::Closed,
        }
    }
}

fn retry_delay(previous: Duration, connected_for: Option<Duration>) -> Duration {
    if connected_for.is_some_and(|duration| duration >= KEEPALIVE) {
        Duration::from_secs(1)
    } else {
        previous
    }
}

/// How long a refused page gets to read its sealed note before it is closed.
/// The relay ends a browser's socket ahead of whatever is still queued for
/// it, so a close sent right behind the note dropped the note.
const REFUSE_GRACE: Duration = Duration::from_secs(1);

/// Send a refused page its sealed note now and its close code after
/// `REFUSE_GRACE`. False when the relay output queue is full.
fn refuse(to_relay: &crate::queue::Sender<Outbound>, c: u32, note: &[u8], code: u16) -> bool {
    let note = Outbound::Protocol(Message::Binary(frame(KIND_DATA, c, note).into()));
    if to_relay.interactive.try_send(note).is_err() {
        return false;
    }
    let to_relay = to_relay.clone();
    tokio::spawn(async move {
        tokio::time::sleep(REFUSE_GRACE).await;
        // Fails only when the relay link ended meanwhile, taking the page.
        let _ = to_relay
            .interactive
            .send(Outbound::Protocol(close_browser(c, code)))
            .await;
    });
    true
}

fn close_browser(c: u32, code: u16) -> Message {
    Message::Text(
        serde_json::json!({ "t": "close", "c": c, "code": code })
            .to_string()
            .into(),
    )
}

async fn deliver_input(inbox: &mpsc::Sender<String>, text: String, limit: Duration) -> bool {
    matches!(
        tokio::time::timeout(limit, inbox.send(text)).await,
        Ok(Ok(()))
    )
}

/// A browser owns its pending work. Removing it cancels queued input and
/// output immediately, including when its session is waiting for the GUI.
struct SessionInput {
    inbox: mpsc::Sender<String>,
    active: Arc<AtomicBool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for SessionInput {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn input_forwarder(
    c: u32,
    session: mpsc::Sender<String>,
    ended: mpsc::UnboundedSender<u32>,
    limit: Duration,
) -> SessionInput {
    let (inbox, mut pending) = mpsc::channel(64);
    let task = tokio::spawn(async move {
        while let Some(text) = pending.recv().await {
            if !deliver_input(&session, text, limit).await {
                let _ = ended.send(c);
                break;
            }
        }
    });
    SessionInput {
        inbox,
        active: Arc::new(AtomicBool::new(true)),
        tasks: vec![task],
    }
}

enum Outbound {
    Protocol(Message),
    Session {
        c: u32,
        keys: Arc<SessionKeys>,
        seq: Arc<AtomicU64>,
        active: Arc<AtomicBool>,
        plain: Vec<u8>,
    },
}

impl Outbound {
    /// Sequence numbers belong to the socket order, after priority selection.
    fn message(self, rng: &SystemRandom) -> Option<Message> {
        match self {
            Self::Protocol(msg) => Some(msg),
            Self::Session {
                c,
                keys,
                seq,
                active,
                plain,
            } => {
                if !active.load(Ordering::SeqCst) {
                    return None;
                }
                let seq = seq.fetch_add(1, Ordering::Relaxed);
                // Compressed here, in the order the page opens them.
                let plain = match keys.deflate.get() {
                    Some(stream) if plain.first() != Some(&0) => deflate(stream, &plain)?,
                    _ => plain,
                };
                let sealed = seal(&keys.from_host, rng, &aad(FROM_HOST, seq), &plain)?;
                Some(Message::Binary(frame(KIND_DATA, c, &sealed).into()))
            }
        }
    }
}

type RelaySocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_relay(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    limit: Duration,
) -> anyhow::Result<RelaySocket> {
    // Keystrokes and echoes are tiny frames; Nagle would hold them back.
    let connected = tokio::time::timeout(
        limit,
        tokio_tungstenite::connect_async_with_config(request, None, true),
    )
    .await
    .map_err(|_| anyhow::anyhow!("relay handshake timed out after {limit:?}"))?;
    match connected {
        Ok((socket, _)) => Ok(socket),
        // The relay says why in plain text: an access code it needs or did
        // not accept, a full address, a busy relay.
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            let why = refusal_text(response.body().as_deref().unwrap_or_default());
            anyhow::bail!("relay refused this Mac ({}): {why}", response.status())
        }
        Err(e) => Err(anyhow::Error::new(e).context("connect to relay")),
    }
}

async fn serve(
    relay_url: &str,
    identity: &ShareIdentity,
    dirty: &WebDirty,
    connected_at: &mut Option<tokio::time::Instant>,
) -> anyhow::Result<Ended> {
    let mac_key = identity.mac_key()?;
    let id = identity.id();
    let mut request = format!("{}/host/{id}", relay_url.trim_end_matches('/'))
        .into_client_request()
        .context("relay url")?;
    request.headers_mut().insert(
        "x-kaku-secret",
        identity.secret.parse().context("secret header")?,
    );
    let code = read_access_code();
    *TRIED_ACCESS.lock() = code.as_deref().map(access_fingerprint).unwrap_or_default();
    if let Some(code) = code {
        if let Ok(value) = code.parse() {
            request.headers_mut().insert("x-kaku-access", value);
        }
    }
    let socket = connect_relay(request, CONNECT_TIMEOUT).await?;
    *connected_at = Some(tokio::time::Instant::now());
    // The id is half of every link to this Mac; the log, which goes into
    // `kaku doctor` bundles, keeps only enough of it to tell links apart.
    log::info!("kaku-share: registered with relay as {}…", &id[..8.min(id.len())]);
    crate::record_relay_status(relay_url, true, None);
    let (mut relay_tx, mut relay_rx) = socket.split();

    // Everything bound for the relay goes through one writer.
    // One bulk item ahead of the socket. A second ciphertext FIFO here
    // would undo priority selection on a slow connection.
    let (to_relay, mut outgoing) = crate::queue::channel_with_capacity::<Outbound>(256, 1);
    let assets = AssetRequests::new(to_relay.bulk.clone());
    let mut writer = tokio::spawn(async move {
        let rng = SystemRandom::new();
        while let Some(msg) = outgoing.recv().await {
            let Some(msg) = msg.message(&rng) else {
                continue;
            };
            if !matches!(
                tokio::time::timeout(WRITE_TIMEOUT, relay_tx.send(msg)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });

    let rng = SystemRandom::new();
    let mut browsers: HashMap<u32, Browser> = HashMap::new();
    let (ended_tx, mut ended_rx) = mpsc::unbounded_channel();
    let mut identity_check = tokio::time::interval(IDENTITY_CHECK);
    identity_check.tick().await;
    // Short, so Enter in `kaku remote` lets the browser in at once; it reads
    // a file only while a browser waits.
    let mut confirm_check = tokio::time::interval(CONFIRM_CHECK);
    confirm_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut keepalive = tokio::time::interval(KEEPALIVE);
    keepalive.tick().await;
    let mut last_heard = tokio::time::Instant::now();

    let result = 'connection: loop {
        tokio::select! {
            _ = &mut writer => break Err(anyhow::anyhow!("relay writer stopped or timed out")),
            Some(c) = ended_rx.recv() => {
                browsers.remove(&c);
                if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, 4413))).is_err() {
                    break Err(anyhow::anyhow!("relay output queue is full"));
                }
            }
            msg = relay_rx.next() => {
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => break Err(anyhow::Error::from(e)),
                    None => break Ok(Ended::Closed),
                };
                last_heard = tokio::time::Instant::now();
                match msg {
                    Message::Text(text) => match serde_json::from_str::<RelayMsg>(text.as_str()) {
                        Ok(RelayMsg::Open { c }) => {
                            let Some(exchange) = Exchange::new(&rng) else { continue };
                            let hello = exchange.hello();
                            if to_relay.interactive.try_send(Outbound::Protocol(Message::Binary(frame(KIND_DATA, c, &hello).into()))).is_err() {
                                break Err(anyhow::anyhow!("relay output queue is full"));
                            }
                            browsers.insert(c, Browser {
                                stage: Stage::Hello(exchange),
                                next_in: 0,
                                connected_at: tokio::time::Instant::now(),
                            });
                        }
                        Ok(RelayMsg::Close { c }) => {
                            browsers.remove(&c);
                        }
                        Ok(RelayMsg::Get { r, p }) => {
                            // Anyone who knows this Mac's id can ask for the
                            // old relay's page files. Queue only a bounded
                            // number of names; excess requests time out there
                            // without crowding out interactive replies here.
                            let _ = assets.try_send(r, p);
                        }
                        Err(e) => log::debug!("kaku-share: unknown relay message: {e}"),
                    },
                    Message::Binary(bytes) => {
                        if bytes.len() < 5 || bytes[0] != KIND_DATA {
                            continue;
                        }
                        let c = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                        let Some(browser) = browsers.get_mut(&c) else { continue };
                        let step = advance(browser, &bytes[5..], &id, &mac_key, &rng, &mut pairing::DiskStore);
                        let text = match step {
                            Step::Nothing => continue,
                            Step::Send(bytes) => {
                                if to_relay.interactive.try_send(Outbound::Protocol(Message::Binary(frame(KIND_DATA, c, &bytes).into()))).is_err() {
                                    break Err(anyhow::anyhow!("relay output queue is full"));
                                }
                                continue;
                            }
                            Step::Close(code) => {
                                browsers.remove(&c);
                                if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, code))).is_err() {
                                    break Err(anyhow::anyhow!("relay output queue is full"));
                                }
                                continue;
                            }
                            Step::Refuse(bytes, code) => {
                                browsers.remove(&c);
                                if !refuse(&to_relay, c, &bytes, code) {
                                    break Err(anyhow::anyhow!("relay output queue is full"));
                                }
                                continue;
                            }
                            Step::Admitted { device, new, first_seq, held } => {
                                if !begin_session(c, browser, &device, new, first_seq, held, dirty, &to_relay, &ended_tx) {
                                    browsers.remove(&c);
                                    if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, 4413))).is_err() {
                                        break Err(anyhow::anyhow!("relay output queue is full"));
                                    }
                                }
                                continue;
                            }
                            Step::Input(text) => text,
                        };
                        let Stage::Session { inbox: Some(inbox), .. } = &browser.stage else { continue };
                        // A slow browser waits in its own ordered worker,
                        // never in the shared relay receive loop. Overload is
                        // an explicit disconnect, never a silently lost key.
                        if inbox.inbox.try_send(text).is_err() {
                            browsers.remove(&c);
                            if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, 4413))).is_err() {
                                break Err(anyhow::anyhow!("relay output queue is full"));
                            }
                        }
                    }
                    Message::Close(frame) => break Ok(Ended::from_close(frame.as_ref())),
                    _ => {}
                }
            }
            _ = keepalive.tick() => {
                if last_heard.elapsed() > SILENCE_LIMIT {
                    break Err(anyhow::anyhow!("relay went silent"));
                }
                if to_relay.interactive.try_send(Outbound::Protocol(Message::Ping(Vec::new().into()))).is_err() {
                    break Ok(Ended::Closed);
                }
            }
            _ = confirm_check.tick() => {
                let mut ended = Vec::new();
                for (&c, browser) in browsers.iter_mut() {
                    match resolve_pending(browser, &mut pairing::DiskStore) {
                        None => {}
                        Some(Step::Admitted { device, new, first_seq, held }) => {
                            let started = begin_session(c, browser, &device, new, first_seq, held, dirty, &to_relay, &ended_tx);
                            if !started {
                                ended.push((c, 4413));
                            }
                        }
                        Some(Step::Close(code)) => ended.push((c, code)),
                        Some(_) => {}
                    }
                }
                for (c, code) in ended {
                    browsers.remove(&c);
                    if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, code))).is_err() {
                        break 'connection Err(anyhow::anyhow!("relay output queue is full"));
                    }
                }
            }
            _ = identity_check.tick() => {
                let paired = pairing::paired_ids();
                let ended: Vec<(u32, u16)> = browsers.iter().filter_map(|(&c, browser)| match &browser.stage {
                    // Removed with `kaku remote revoke`: end its session.
                    Stage::Session { device, .. } if !paired.contains(device) => Some((c, 4403)),
                    Stage::Session { .. } => None,
                    // Waiting for Enter has its own limit.
                    Stage::Confirm { .. } => None,
                    _ if browser.connected_at.elapsed() >= AUTH_TIMEOUT => Some((c, 4408)),
                    _ => None,
                }).collect();
                for (c, code) in ended {
                    browsers.remove(&c);
                    if to_relay.interactive.try_send(Outbound::Protocol(close_browser(c, code))).is_err() {
                        break 'connection Err(anyhow::anyhow!("relay output queue is full"));
                    }
                }
                // A reset identity means a new link; re-register under it so
                // the old one stops working.
                if read_identity().as_ref() != Some(identity) {
                    log::info!("kaku-share: share link changed, re-registering");
                    break Ok(Ended::Closed);
                }
            }
        }
    };
    drop(browsers);
    writer.abort();
    result
}

/// Start the session of browser `c`, which just proved it is `device`, and
/// tell the relay it is a paired browser, so sockets that only know the id
/// cannot push it out. False when its session could not take what it sent
/// while it waited.
#[allow(clippy::too_many_arguments)]
fn begin_session(
    c: u32,
    browser: &mut Browser,
    device: &pairing::Device,
    new: bool,
    first_seq: u64,
    held: Vec<String>,
    dirty: &WebDirty,
    to_relay: &crate::queue::Sender<Outbound>,
    ended: &mpsc::UnboundedSender<u32>,
) -> bool {
    if new {
        log::info!("kaku-share: paired {} ({})", device.name, device.id);
    } else {
        pairing::announce_return(device);
    }
    let Stage::Session { keys, inbox, .. } = &mut browser.stage else {
        return false;
    };
    let session = start_session(c, keys.clone(), first_seq, dirty.clone(), to_relay.clone(), ended.clone());
    for text in held {
        if session.inbox.try_send(text).is_err() {
            return false;
        }
    }
    *inbox = Some(session);
    let ok = serde_json::json!({ "t": "ok", "c": c }).to_string();
    let _ = to_relay
        .interactive
        .try_send(Outbound::Protocol(Message::Text(ok.into())));
    true
}

/// Start the session for browser `c`; returns where its messages go. Kaku's
/// messages to it are numbered from `first_seq`.
fn start_session(
    c: u32,
    keys: Arc<SessionKeys>,
    first_seq: u64,
    dirty: WebDirty,
    to_relay: crate::queue::Sender<Outbound>,
    ended: mpsc::UnboundedSender<u32>,
) -> SessionInput {
    // The forwarder's timeout starts only once this queue is full. With one
    // slot, a session busy for two seconds (a config reload resolving fonts)
    // closed the browser with two keys pending; 64 is what V0.25.0 allowed.
    // Removing the browser aborts the session, so these are dropped with it.
    let (inbox, session_in) = mpsc::channel::<String>(SESSION_INBOX);
    let mut input = input_forwarder(c, inbox, ended.clone(), INBOX_TIMEOUT);
    let (session_out, replies) = crate::queue::channel::<String>(64);
    let (frames_out, mut frames) = mpsc::channel::<Vec<u8>>(4);
    input.tasks.push(tokio::spawn(web::run_session(
        session_in,
        session_out,
        frames_out,
        dirty,
    )));
    let seq = Arc::new(AtomicU64::new(first_seq));
    // Independent producers: waiting for space for a file or picture must
    // not prevent this browser's terminal replies reaching the writer.
    for (mut rx, destination) in [
        (replies.interactive, to_relay.interactive.clone()),
        (replies.bulk, to_relay.bulk.clone()),
    ] {
        let keys = keys.clone();
        let seq = seq.clone();
        let active = input.active.clone();
        let ended = ended.clone();
        input.tasks.push(tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                let msg = Outbound::Session {
                    c,
                    keys: keys.clone(),
                    seq: seq.clone(),
                    active: active.clone(),
                    plain: text.into_bytes(),
                };
                if !matches!(
                    tokio::time::timeout(WRITE_TIMEOUT, destination.send(msg)).await,
                    Ok(Ok(()))
                ) {
                    let _ = ended.send(c);
                    break;
                }
            }
        }));
    }
    let active = input.active.clone();
    input.tasks.push(tokio::spawn(async move {
        while let Some(plain) = frames.recv().await {
            let msg = Outbound::Session {
                c,
                keys: keys.clone(),
                seq: seq.clone(),
                active: active.clone(),
                plain,
            };
            if !matches!(
                tokio::time::timeout(WRITE_TIMEOUT, to_relay.send_bulk(msg)).await,
                Ok(Ok(()))
            ) {
                let _ = ended.send(c);
                break;
            }
        }
    }));
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn legacy_asset_requests_wait_behind_bulk_and_preserve_response_order() {
        let (tx, mut rx) = crate::queue::channel_with_capacity(8, 1);
        tx.send_bulk(Outbound::Protocol(Message::Ping(Vec::new().into())))
            .await
            .unwrap();
        let assets = AssetRequests::new(tx.bulk.clone());
        for id in 1..=3 {
            assert!(
                assets.try_send(id, "assets/icon.png".into()),
                "accepted Get {id}"
            );
        }
        tx.send(Outbound::Protocol(Message::Text("key echo".into())))
            .await
            .unwrap();
        let rng = SystemRandom::new();
        assert!(matches!(
            rx.recv().await.unwrap().message(&rng),
            Some(Message::Text(_))
        ));
        assert!(matches!(
            rx.recv().await.unwrap().message(&rng),
            Some(Message::Ping(_))
        ));
        for id in 1..=3 {
            let Some(Message::Binary(bytes)) =
                tokio::time::timeout(Duration::from_secs(1), rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
                    .message(&rng)
            else {
                panic!("asset frame");
            };
            assert_eq!(bytes.as_ref(), asset_frame(id, "assets/icon.png"));
        }
        drop(assets);
        drop(tx);
        assert!(tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn legacy_asset_requests_are_bounded_and_cancelled_with_the_connection() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(Outbound::Protocol(Message::Ping(Vec::new().into())))
            .await
            .unwrap();
        let assets = AssetRequests::new(tx.clone());
        for id in 0..16 {
            assert!(assets.try_send(id, "assets/xterm.js".into()));
        }
        assert!(!assets.try_send(16, "assets/xterm.js".into()));
        tokio::task::yield_now().await;
        // The worker is waiting for the occupied bulk slot. Ending serve
        // cancels its reservation and every pending name, not just new Gets.
        drop(assets);
        drop(tx);
        assert!(matches!(
            rx.recv().await,
            Some(Outbound::Protocol(Message::Ping(_)))
        ));
        assert!(tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn legacy_asset_requests_outlive_one_busy_socket() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(Outbound::Protocol(Message::Ping(Vec::new().into())))
            .await
            .unwrap();
        let assets = AssetRequests::waiting(tx.clone(), Duration::from_millis(50));
        assert!(assets.try_send(1, "assets/icon.png".into()));
        // Get 1 gives up on the full queue; the worker must stay for Get 2.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(matches!(
            rx.recv().await,
            Some(Outbound::Protocol(Message::Ping(_)))
        ));
        assert!(assets.try_send(2, "assets/icon.png".into()));
        let rng = SystemRandom::new();
        let Some(Message::Binary(bytes)) = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap()
            .message(&rng)
        else {
            panic!("asset frame");
        };
        assert_eq!(bytes.as_ref(), asset_frame(2, "assets/icon.png"));
    }

    fn identity() -> ShareIdentity {
        ShareIdentity::generate().unwrap()
    }

    #[test]
    fn a_healthy_connection_resets_error_backoff() {
        assert_eq!(
            retry_delay(MAX_BACKOFF, Some(KEEPALIVE)),
            Duration::from_secs(1)
        );
        assert_eq!(retry_delay(MAX_BACKOFF, None), MAX_BACKOFF);
        assert_eq!(
            retry_delay(MAX_BACKOFF, Some(Duration::from_millis(10))),
            MAX_BACKOFF
        );
    }

    #[test]
    fn rejection_carries_a_distinguishable_code() {
        let Message::Text(text) = close_browser(7, 4403) else {
            panic!("control frame")
        };
        let json: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        assert_eq!(json["code"], 4403);
        assert_eq!(json["c"], 7);
    }

    #[tokio::test]
    async fn input_bursts_wait_for_a_busy_session_without_losing_keys() {
        let (tx, mut rx) = mpsc::channel(64);
        let reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let mut received = Vec::new();
            while let Some(text) = rx.recv().await {
                received.push(text);
            }
            received
        });
        for i in 0..200 {
            assert!(deliver_input(&tx, i.to_string(), INBOX_TIMEOUT).await);
        }
        drop(tx);
        assert_eq!(
            reader.await.unwrap(),
            (0..200).map(|n| n.to_string()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn a_stalled_session_cannot_block_control_work_forever() {
        let (tx, _rx) = mpsc::channel(1);
        tx.send("first".into()).await.unwrap();
        assert!(!deliver_input(&tx, "second".into(), Duration::from_millis(20)).await);
    }

    #[tokio::test]
    async fn a_session_busy_past_the_timeout_keeps_its_browser() {
        let (session, mut rx) = mpsc::channel(SESSION_INBOX);
        let (ended, mut ended_rx) = mpsc::unbounded_channel();
        let input = input_forwarder(3, session, ended, Duration::from_millis(50));
        for n in 0..8 {
            input.inbox.try_send(n.to_string()).unwrap();
        }
        // The session reads nothing for three timeouts, then catches up.
        tokio::time::sleep(Duration::from_millis(150)).await;
        for n in 0..8 {
            assert_eq!(rx.recv().await, Some(n.to_string()));
        }
        assert!(ended_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn stalled_browser_does_not_delay_another_and_times_out_alone() {
        let (blocked, _blocked_rx) = mpsc::channel(1);
        blocked.send("already full".into()).await.unwrap();
        let (ready, mut ready_rx) = mpsc::channel(1);
        let (ended, mut ended_rx) = mpsc::unbounded_channel();
        let a = input_forwarder(1, blocked, ended.clone(), Duration::from_millis(100));
        let b = input_forwarder(2, ready, ended, Duration::from_millis(100));
        a.inbox.try_send("pending".into()).unwrap();
        b.inbox.try_send("key".into()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(50), ready_rx.recv())
                .await
                .unwrap(),
            Some("key".into()),
        );
        assert_eq!(ended_rx.recv().await, Some(1));
        assert!(ended_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn per_browser_worker_preserves_bursts_and_cancels_pending_input() {
        let (session, mut rx) = mpsc::channel(1);
        let (ended, mut ended_rx) = mpsc::unbounded_channel();
        let input = input_forwarder(7, session, ended, INBOX_TIMEOUT);
        for n in 0..64 {
            input.inbox.try_send(n.to_string()).unwrap();
        }
        // Full means the caller must close 4413, not discard just this key.
        assert!(input.inbox.try_send("overflow".into()).is_err());
        for n in 0..64 {
            assert_eq!(rx.recv().await, Some(n.to_string()));
        }
        drop(input);
        assert_eq!(rx.recv().await, None);
        assert!(ended_rx.try_recv().is_err());

        let (session, mut rx) = mpsc::channel(1);
        session.send("already delivered".into()).await.unwrap();
        let (ended, _) = mpsc::unbounded_channel();
        let input = input_forwarder(8, session, ended, INBOX_TIMEOUT);
        input.inbox.try_send("must be cancelled".into()).unwrap();
        tokio::task::yield_now().await;
        drop(input);
        assert_eq!(rx.recv().await, Some("already delivered".into()));
        assert_eq!(rx.recv().await, None);
    }

    #[tokio::test]
    async fn priority_is_selected_before_encryption_and_revoke_discards_queued_output() {
        let (_, keys) = exchanged();
        let keys = Arc::new(keys);
        let seq = Arc::new(AtomicU64::new(1));
        let active = Arc::new(AtomicBool::new(true));
        let message = |text: &str| Outbound::Session {
            c: 7,
            keys: keys.clone(),
            seq: seq.clone(),
            active: active.clone(),
            plain: text.as_bytes().to_vec(),
        };
        let (tx, mut rx) = crate::queue::channel(8);
        tx.send_bulk(message("file start")).await.unwrap();
        tx.send_bulk(message("file chunk")).await.unwrap();
        tx.send_bulk(message("file end")).await.unwrap();
        tx.send(message("key echo")).await.unwrap();
        let rng = SystemRandom::new();
        for (n, expected) in ["key echo", "file start", "file chunk", "file end"]
            .iter()
            .enumerate()
        {
            let Some(Message::Binary(bytes)) = rx.recv().await.unwrap().message(&rng) else {
                panic!("session message");
            };
            assert_eq!(
                open(&keys.from_host, &aad(FROM_HOST, n as u64 + 1), &bytes[5..]).as_deref(),
                Some(*expected)
            );
        }
        tx.send_bulk(message("stale picture")).await.unwrap();
        active.store(false, Ordering::SeqCst);
        assert!(rx.recv().await.unwrap().message(&rng).is_none());
        assert_eq!(seq.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn a_relay_that_never_answers_the_handshake_is_given_up_on() {
        // A listener that accepts and stays silent, like a local proxy whose
        // upstream died: the connect succeeds, the handshake never ends.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _held = tokio::spawn(async move {
            let mut kept = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                kept.push(socket);
            }
        });
        let request = format!("ws://{addr}/host/x").into_client_request().unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            connect_relay(request, Duration::from_millis(300)),
        )
        .await
        .expect("connect_relay must give up on its own");
        let Err(err) = result else {
            panic!("a silent relay must not connect");
        };
        assert!(format!("{err:#}").contains("timed out"), "{err:#}");
    }

    #[test]
    fn id_is_a_hash_of_the_secret() {
        let a = identity();
        assert_eq!(a.id().len(), 32);
        assert_eq!(a.id(), a.id());
        assert!(!a.id().contains(&a.secret[..16]));
        assert_ne!(a.id(), identity().id());
    }

    #[test]
    fn the_pairing_fragment_carries_only_the_one_time_secret() {
        let a = identity();
        let mac = a.mac_key().unwrap();
        assert_eq!(
            a.page_url("wss://relay.example.com/"),
            format!("https://relay.example.com/{}/", a.id())
        );
        // The default relay's page lives on its own origin.
        assert_eq!(
            a.page_url("wss://r.kaku.fun"),
            format!("https://t.kaku.fun/{}/", a.id())
        );
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        // The secret alone; the Mac's key travels in the handshake.
        let fragment = pairing::pairing_fragment("c2VjcmV0");
        assert_eq!(fragment, "#c2VjcmV0");
        assert!(!fragment.contains(&b64(mac.public_key())));
        assert!(!fragment.contains(&a.secret) && !fragment.contains(&a.mac_key));
    }

    #[test]
    fn sealed_messages_open_only_in_their_direction_and_place() {
        let rng = SystemRandom::new();
        let keys = exchanged().1;
        let msg = seal(&keys.to_host, &rng, &aad(TO_HOST, 0), b"ls\r").unwrap();
        assert_eq!(
            open(&keys.to_host, &aad(TO_HOST, 0), &msg).as_deref(),
            Some("ls\r")
        );
        // Reflected back as if the Mac sent it, or replayed later.
        assert!(open(&keys.from_host, &aad(FROM_HOST, 0), &msg).is_none());
        assert!(open(&keys.to_host, &aad(TO_HOST, 1), &msg).is_none());
        // Another connection's keys open nothing.
        assert!(open(&exchanged().1.to_host, &aad(TO_HOST, 0), &msg).is_none());
    }

    use crate::pairing::tests::{MemoryStore, TestPage};
    use crate::pairing::DeviceStore;

    fn exchanged() -> ([u8; 32], SessionKeys) {
        let rng = SystemRandom::new();
        let mac = Exchange::new(&rng).unwrap();
        let page = TestPage::new();
        mac.finish("host", &page.hello()).unwrap()
    }

    struct Fixture {
        identity: ShareIdentity,
        mac_key: MacKey,
        store: MemoryStore,
        rng: SystemRandom,
    }

    /// A browser connected to Kaku, holding the keys its page derived.
    struct Connected {
        browser: Browser,
        keys: SessionKeys,
        transcript: [u8; 32],
        device: MacKey,
        sent: u64,
        /// Kaku's first message, its signature and key.
        mac_auth: serde_json::Value,
    }

    impl Fixture {
        fn new() -> Self {
            let identity = identity();
            let mac_key = identity.mac_key().unwrap();
            Self {
                identity,
                mac_key,
                store: MemoryStore::default(),
                rng: SystemRandom::new(),
            }
        }

        fn step(&mut self, browser: &mut Browser, data: &[u8]) -> Step {
            let id = self.identity.id();
            advance(
                browser,
                data,
                &id,
                &self.mac_key,
                &self.rng,
                &mut self.store,
            )
        }

        /// Connect a page and check Kaku's signature the way the page does.
        fn connect(&mut self) -> Connected {
            let exchange = Exchange::new(&self.rng).unwrap();
            let mac_hello = exchange.hello();
            let mut browser = Browser {
                stage: Stage::Hello(exchange),
                next_in: 0,
                connected_at: tokio::time::Instant::now(),
            };
            let page = TestPage::new();
            let Step::Send(signed) = self.step(&mut browser, &page.hello()) else {
                panic!("Kaku answers the page's hello with its signature");
            };
            let (transcript, keys, device) = page.finish(&self.identity.id(), &mac_hello);
            let auth = open(&keys.from_host, &aad(FROM_HOST, 0), &signed).unwrap();
            let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
            assert_eq!(auth["type"], "mac_auth");
            let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(auth["sig"].as_str().unwrap())
                .unwrap();
            assert!(pairing::verify(
                self.mac_key.public_key(),
                &transcript,
                &sig
            ));
            Connected {
                browser,
                keys,
                transcript,
                device,
                sent: 0,
                mac_auth: auth,
            }
        }

        fn send(&mut self, conn: &mut Connected, text: &str) -> Step {
            let sealed = seal(
                &conn.keys.to_host,
                &self.rng,
                &aad(TO_HOST, conn.sent),
                text.as_bytes(),
            )
            .unwrap();
            conn.sent += 1;
            self.step(&mut conn.browser, &sealed)
        }

        fn pair(&mut self, conn: &Connected) -> String {
            let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
            let id = pairing::device_id(conn.device.public_key());
            self.store.add(pairing::Device {
                id: id.clone(),
                public_key: b64(conn.device.public_key()),
                name: String::new(),
                paired_at: 0,
                last_seen: 0,
            });
            id
        }
    }

    fn sign_in(conn: &Connected) -> String {
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        serde_json::json!({
            "type": "auth",
            "device": b64(conn.device.public_key()),
            "sig": b64(&conn.device.sign(&conn.transcript).unwrap()),
        })
        .to_string()
    }

    fn join(conn: &Connected, secret: &[u8]) -> String {
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        let proof = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret),
            &conn.transcript,
        );
        serde_json::json!({
            "type": "pair",
            "device": b64(conn.device.public_key()),
            "sig": b64(&conn.device.sign(&conn.transcript).unwrap()),
            "proof": b64(proof.as_ref()),
            "name": "Safari on iPhone",
        })
        .to_string()
    }

    #[test]
    fn a_paired_browser_reaches_its_session_and_nothing_replays() {
        let mut fx = Fixture::new();
        let mut conn = fx.connect();
        let id = fx.pair(&conn);
        let Step::Admitted {
            device,
            new,
            first_seq,
            ..
        } = ({
            let m = sign_in(&conn);
            fx.send(&mut conn, &m)
        })
        else {
            panic!("a paired device signs in");
        };
        assert_eq!((device.id, new, first_seq), (id, false, 1));
        // Its session takes input in order; a replayed message ends it.
        assert!(matches!(
            fx.send(&mut conn, r#"{"type":"ping"}"#),
            Step::Input(_)
        ));
        conn.sent -= 1;
        assert!(matches!(
            fx.send(&mut conn, r#"{"type":"ping"}"#),
            Step::Close(4403)
        ));
    }

    /// A page that says `deflate` in its proof gets its session's text
    /// through one stream; each message inflates whole on its own, in
    /// order, as the page's DecompressionStream reads them.
    #[tokio::test]
    async fn a_refused_page_gets_its_note_before_the_close() {
        let (to_relay, mut out) = crate::queue::channel::<Outbound>(8);
        assert!(refuse(&to_relay, 7, b"note", 4403));
        let Some(Outbound::Protocol(Message::Binary(note))) = out.interactive.recv().await else {
            panic!("the sealed note goes out first");
        };
        assert_eq!(&note[5..], b"note");
        assert!(
            out.interactive.try_recv().is_err(),
            "the relay drops a note with a close right behind it"
        );
        let close = tokio::time::timeout(REFUSE_GRACE * 3, out.interactive.recv()).await;
        let Ok(Some(Outbound::Protocol(Message::Text(close)))) = close else {
            panic!("the close follows the grace period");
        };
        assert!(close.contains(r#""code":4403"#), "{close}");
    }

    #[test]
    fn a_page_that_inflates_gets_each_message_deflated_whole() {
        for capable in [false, true] {
            let mut fx = Fixture::new();
            let mut conn = fx.connect();
            fx.pair(&conn);
            let mut auth: serde_json::Value = serde_json::from_str(&sign_in(&conn)).unwrap();
            if capable {
                auth["deflate"] = true.into();
            }
            assert!(matches!(
                fx.send(&mut conn, &auth.to_string()),
                Step::Admitted { .. }
            ));
            let Stage::Session { keys, .. } = &conn.browser.stage else {
                panic!("in session");
            };
            assert_eq!(keys.deflate.get().is_some(), capable);
        }

        let stream = parking_lot::Mutex::new(flate2::Compress::new(
            flate2::Compression::default(),
            false,
        ));
        let frame = r#"{"type":"frame","seq":7,"data":"\u001b[28;1H\u001b[0;38;5;9m✳ Recombobulating… (2m 3s)"}"#;
        let big = format!(r#"{{"type":"tabs","pad":"{}"}}"#, "x".repeat(200_000));
        let mut inflate = flate2::Decompress::new(false);
        let mut sizes = Vec::new();
        for msg in [frame, frame, big.as_str(), frame] {
            let out = deflate(&stream, msg.as_bytes()).unwrap();
            assert_eq!(out[0], DEFLATED);
            sizes.push(out.len());
            let mut text = Vec::with_capacity(msg.len() + 64);
            inflate
                .decompress_vec(&out[1..], &mut text, flate2::FlushDecompress::Sync)
                .unwrap();
            assert_eq!(text, [msg.as_bytes(), b"\n"].concat());
        }
        // The second copy of a frame costs a fraction of the first.
        assert!(sizes[1] * 4 < sizes[0], "{sizes:?}");
    }

    /// What Kaku sent the page as message `seq`, opened the way the page
    /// opens it.
    fn opened(conn: &Connected, sealed: &[u8], seq: u64) -> serde_json::Value {
        let text = open(&conn.keys.from_host, &aad(FROM_HOST, seq), sealed).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn a_pairing_link_waits_for_enter_then_the_browser_signs_in_without_it() {
        let mut fx = Fixture::new();
        fx.store.offer = Some((vec![1u8; 16], false));
        let mut conn = fx.connect();
        let Step::Send(note) = ({
            let m = join(&conn, &[1u8; 16]);
            fx.send(&mut conn, &m)
        }) else {
            panic!("a valid link waits for confirmation");
        };
        assert_eq!(opened(&conn, &note, 1)["type"], "pair_pending");
        // The page's hello and attach wait with it; nothing is paired yet.
        assert!(matches!(
            fx.send(&mut conn, r#"{"type":"hello","frame_ack":true}"#),
            Step::Nothing
        ));
        assert!(fx.store.devices.is_empty());
        assert!(resolve_pending(&mut conn.browser, &mut fx.store).is_none());
        let code = fx.store.request.as_ref().unwrap().code.clone();
        assert_eq!(
            code,
            pairing::pairing_code(&[1u8; 16], conn.device.public_key())
        );
        // Enter in `kaku remote`.
        fx.store.request.as_mut().unwrap().allowed = Some(true);
        let Some(Step::Admitted {
            device,
            new,
            first_seq,
            held,
        }) = resolve_pending(&mut conn.browser, &mut fx.store)
        else {
            panic!("an allowed browser starts its session");
        };
        assert!(new);
        assert_eq!(first_seq, 2);
        assert_eq!(held, vec![r#"{"type":"hello","frame_ack":true}"#.to_string()]);
        assert_eq!(device.name, "Safari on iPhone");
        assert!(fx.store.devices.contains_key(&device.id));
        let mut again = fx.connect();
        again.device = conn.device;
        let Step::Admitted {
            device: same, new, ..
        } = ({
            let m = sign_in(&again);
            fx.send(&mut again, &m)
        })
        else {
            panic!("the paired browser signs in later");
        };
        assert_eq!((same.id, new), (device.id, false));
    }

    #[test]
    fn a_pairing_refused_on_the_mac_closes_with_its_own_code() {
        let mut fx = Fixture::new();
        fx.store.offer = Some((vec![1u8; 16], false));
        let mut conn = fx.connect();
        let m = join(&conn, &[1u8; 16]);
        assert!(matches!(fx.send(&mut conn, &m), Step::Send(_)));
        fx.store.request.as_mut().unwrap().allowed = Some(false);
        assert!(matches!(
            resolve_pending(&mut conn.browser, &mut fx.store),
            Some(Step::Close(pairing::DECLINED))
        ));
        assert!(fx.store.devices.is_empty());
    }

    #[test]
    fn a_second_browser_on_a_link_in_use_is_turned_away() {
        let mut fx = Fixture::new();
        fx.store.offer = Some((vec![1u8; 16], false));
        let mut first = fx.connect();
        let m = join(&first, &[1u8; 16]);
        assert!(matches!(fx.send(&mut first, &m), Step::Send(_)));
        let mut second = fx.connect();
        let m = join(&second, &[1u8; 16]);
        assert!(matches!(fx.send(&mut second, &m), Step::Close(4429)));
    }

    /// Only this sealed note makes a page forget its key; a bare 4403 close
    /// can come from anyone on the way.
    #[test]
    fn a_removed_browser_is_told_so_sealed() {
        let mut fx = Fixture::new();
        let mut conn = fx.connect();
        let m = sign_in(&conn);
        let Step::Refuse(note, 4403) = fx.send(&mut conn, &m) else {
            panic!("an unknown device gets the sealed note, then 4403");
        };
        assert_eq!(opened(&conn, &note, 1)["type"], "revoked");
    }

    #[test]
    fn kaku_vouches_for_its_key_to_the_holder_of_the_pairing_link_only() {
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        let mut fx = Fixture::new();
        // No link out: the key, but nothing a page could check it against.
        let conn = fx.connect();
        assert_eq!(conn.mac_auth["key"], b64(fx.mac_key.public_key()));
        assert!(conn.mac_auth.get("tag").is_none());
        // A link out: a tag under its secret over this transcript and the key.
        fx.store.offer = Some((vec![9u8; 16], false));
        let mut conn = fx.connect();
        let tag = conn.mac_auth["tag"].as_str().unwrap().to_string();
        let expect = pairing::mac_key_tag(&[9u8; 16], &conn.transcript, fx.mac_key.public_key());
        assert_eq!(tag, b64(&expect));
        // Another secret, another connection, or another key gives another tag.
        assert_ne!(
            b64(&pairing::mac_key_tag(
                &[8u8; 16],
                &conn.transcript,
                fx.mac_key.public_key()
            )),
            tag
        );
        let other = fx.connect();
        assert_ne!(other.mac_auth["tag"].as_str().unwrap(), tag);
        let stranger = identity().mac_key().unwrap();
        assert_ne!(
            b64(&pairing::mac_key_tag(
                &[9u8; 16],
                &conn.transcript,
                stranger.public_key()
            )),
            tag
        );
        // Once the link is used, Kaku no longer vouches with it.
        let Step::Send(_) = ({
            let m = join(&conn, &[9u8; 16]);
            fx.send(&mut conn, &m)
        }) else {
            panic!("the link waits for confirmation");
        };
        fx.store.request.as_mut().unwrap().allowed = Some(true);
        assert!(matches!(
            resolve_pending(&mut conn.browser, &mut fx.store),
            Some(Step::Admitted { new: true, .. })
        ));
        assert!(fx.connect().mac_auth.get("tag").is_none());
    }

    #[test]
    fn an_unpaired_browser_never_reaches_a_session() {
        let mut fx = Fixture::new();
        let mut conn = fx.connect();
        assert!(matches!(
            {
                let m = sign_in(&conn);
                fx.send(&mut conn, &m)
            },
            Step::Refuse(_, 4403)
        ));
        // Skipping the proof and going straight to the terminal is refused too.
        let mut conn = fx.connect();
        assert!(matches!(
            fx.send(&mut conn, r#"{"type":"hello","frame_ack":true}"#),
            Step::Close(4403)
        ));
    }

    #[test]
    fn a_used_or_wrong_pairing_link_gets_its_own_close_code() {
        let mut fx = Fixture::new();
        let mut conn = fx.connect();
        assert!(matches!(
            {
                let m = join(&conn, &[1u8; 16]);
                fx.send(&mut conn, &m)
            },
            Step::Close(4401)
        ));
        fx.store.offer = Some((vec![1u8; 16], true));
        let mut conn = fx.connect();
        assert!(matches!(
            {
                let m = join(&conn, &[1u8; 16]);
                fx.send(&mut conn, &m)
            },
            Step::Close(4401)
        ));
    }

    #[test]
    fn a_relays_refusal_cannot_write_escape_sequences_to_the_terminal() {
        let body = b"  access code \x1b]52;c;ZWNobyBoaQ==\x07wrong\x1b[2J\r\n";
        assert_eq!(refusal_text(body), "access code ]52;c;ZWNobyBoaQ==wrong[2J");
        assert!(!refusal_text(body).contains('\x1b'));
    }

    #[test]
    fn a_page_from_before_pairing_is_told_to_reload() {
        let mut fx = Fixture::new();
        let exchange = Exchange::new(&fx.rng).unwrap();
        let mut browser = Browser {
            stage: Stage::Hello(exchange),
            next_in: 0,
            connected_at: tokio::time::Instant::now(),
        };
        let mut old = b"KCB2".to_vec();
        old.extend_from_slice(&[0u8; 16]);
        assert!(matches!(fx.step(&mut browser, &old), Step::Close(OLD_PAGE)));
    }

    #[test]
    fn only_a_restart_close_means_the_relay_is_restarting() {
        use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
        use tokio_tungstenite::tungstenite::protocol::CloseFrame;
        let close = |code| CloseFrame {
            code,
            reason: "".into(),
        };
        assert_eq!(
            Ended::from_close(Some(&close(CloseCode::Restart))),
            Ended::RelayRestarting
        );
        assert_eq!(Ended::from_close(Some(&close(CloseCode::Normal))), Ended::Closed);
        assert_eq!(Ended::from_close(Some(&close(CloseCode::Away))), Ended::Closed);
        assert_eq!(Ended::from_close(None), Ended::Closed);
    }
}
