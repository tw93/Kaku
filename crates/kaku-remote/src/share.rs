//! One shareable web link per Kaku, reached through the relay.
//!
//! Kaku keeps a share identity on disk: a secret that registers this Mac with
//! the relay and a key that seals every message end to end. The relay knows
//! the Mac only by the SHA-256 of the secret, so holding the link (which
//! carries that id and the key) does not let anyone register as this Mac,
//! and the relay only ever forwards sealed bytes. Resetting the identity
//! retires the old link at once: the running Kaku notices the new file and
//! re-registers under the new id.

use crate::web::{self, WebDirty};
use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const TO_HOST: &[u8] = b"kaku-web c2h";
const FROM_HOST: &[u8] = b"kaku-web h2c";
const KIND_DATA: u8 = 0;
const KIND_ASSET: u8 = 1;
const IDENTITY_CHECK: Duration = Duration::from_secs(3);
/// Pings keep NATs and proxies from dropping an idle link, and their pongs
/// prove the relay is still there.
const KEEPALIVE: Duration = Duration::from_secs(20);
/// With pings every 20s, this long without hearing from the relay means the
/// link died without a close (sleep, network change); reconnect.
const SILENCE_LIMIT: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

// ── Identity ──────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareIdentity {
    /// Registers this Mac with the relay. Never leaves the Mac.
    secret: String,
    /// AES-256-GCM key, base64url. Travels only in the link's fragment.
    key: String,
}

impl ShareIdentity {
    fn generate() -> anyhow::Result<Self> {
        let mut secret = [0u8; 32];
        let mut key = [0u8; 32];
        getrandom::fill(&mut secret).context("random secret")?;
        getrandom::fill(&mut key).context("random key")?;
        Ok(Self {
            secret: hex(&secret),
            key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key),
        })
    }

    /// The relay's name for this Mac: the first 128 bits of SHA-256(secret).
    pub fn id(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, self.secret.as_bytes());
        hex(&digest.as_ref()[..16])
    }

    fn sealing_key(&self) -> anyhow::Result<LessSafeKey> {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&self.key)
            .context("decode share key")?;
        let key = UnboundKey::new(&AES_256_GCM, &raw)
            .map_err(|_| anyhow::anyhow!("share key has the wrong length"))?;
        Ok(LessSafeKey::new(key))
    }

    /// The page link for a relay at `relay_url` (ws:// or wss://).
    pub fn link(&self, relay_url: &str) -> String {
        format!("{}/t/{}/#k={}", http_base(relay_url), self.id(), self.key)
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
    // Write to a sibling and rename, so a reader never sees half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).context("write share identity")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &path).context("save share identity")?;
    Ok(())
}

/// This Kaku's share identity, created on first use.
pub fn load_or_create_identity() -> anyhow::Result<ShareIdentity> {
    if let Some(identity) = read_identity() {
        return Ok(identity);
    }
    let identity = ShareIdentity::generate()?;
    write_identity(&identity)?;
    Ok(identity)
}

/// Replace the identity; the old link stops working once the running Kaku
/// re-registers, within a few seconds.
pub fn reset_identity() -> anyhow::Result<ShareIdentity> {
    let identity = ShareIdentity::generate()?;
    write_identity(&identity)?;
    Ok(identity)
}

// ── Sealing ───────────────────────────────────────────────────────────────────

fn seal(key: &LessSafeKey, rng: &SystemRandom, plain: &[u8]) -> Option<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut nonce).ok()?;
    let mut buf = plain.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(FROM_HOST),
        &mut buf,
    )
    .ok()?;
    let mut out = Vec::with_capacity(NONCE_LEN + buf.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&buf);
    Some(out)
}

fn open(key: &LessSafeKey, sealed: &[u8]) -> Option<String> {
    if sealed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut buf = body.to_vec();
    let plain = key
        .open_in_place(nonce, Aad::from(TO_HOST), &mut buf)
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
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

// ── Tunnel ────────────────────────────────────────────────────────────────────

/// Keep this Kaku registered with the relay for as long as the bridge runs.
pub(crate) async fn run(relay_url: String, dirty: WebDirty) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let identity = match load_or_create_identity() {
            Ok(identity) => identity,
            Err(e) => {
                log::warn!("kaku-share: no share identity: {e:#}");
                tokio::time::sleep(MAX_BACKOFF).await;
                continue;
            }
        };
        match serve(&relay_url, &identity, &dirty).await {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(e) => log::info!("kaku-share: relay connection ended: {e:#}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

enum Browser {
    /// Connected, but has not yet sent a message that opens with the key.
    Pending,
    Active(mpsc::Sender<String>),
}

async fn serve(relay_url: &str, identity: &ShareIdentity, dirty: &WebDirty) -> anyhow::Result<()> {
    let key = identity.sealing_key()?;
    let id = identity.id();
    let mut request = format!("{}/host/{id}", relay_url.trim_end_matches('/'))
        .into_client_request()
        .context("relay url")?;
    request.headers_mut().insert(
        "x-kaku-secret",
        identity.secret.parse().context("secret header")?,
    );
    // Keystrokes and echoes are tiny frames; Nagle would hold them back.
    let (socket, _) = tokio_tungstenite::connect_async_with_config(request, None, true)
        .await
        .context("connect to relay")?;
    log::info!("kaku-share: registered with relay as {id}");
    let (mut relay_tx, mut relay_rx) = socket.split();

    // Everything bound for the relay goes through one writer.
    let (to_relay, mut outgoing) = mpsc::channel::<Message>(256);
    let writer = tokio::spawn(async move {
        while let Some(msg) = outgoing.recv().await {
            if relay_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    let key = std::sync::Arc::new(key);
    let mut browsers: HashMap<u32, Browser> = HashMap::new();
    let mut identity_check = tokio::time::interval(IDENTITY_CHECK);
    identity_check.tick().await;
    let mut keepalive = tokio::time::interval(KEEPALIVE);
    keepalive.tick().await;
    let mut last_heard = tokio::time::Instant::now();

    let result = loop {
        tokio::select! {
            msg = relay_rx.next() => {
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => break Err(anyhow::Error::from(e)),
                    None => break Ok(()),
                };
                last_heard = tokio::time::Instant::now();
                match msg {
                    Message::Text(text) => match serde_json::from_str::<RelayMsg>(text.as_str()) {
                        Ok(RelayMsg::Open { c }) => {
                            browsers.insert(c, Browser::Pending);
                        }
                        Ok(RelayMsg::Close { c }) => {
                            browsers.remove(&c);
                        }
                        Ok(RelayMsg::Get { r, p }) => {
                            let _ = to_relay.send(Message::Binary(asset_frame(r, &p).into())).await;
                        }
                        Err(e) => log::debug!("kaku-share: unknown relay message: {e}"),
                    },
                    Message::Binary(bytes) => {
                        if bytes.len() < 5 || bytes[0] != KIND_DATA {
                            continue;
                        }
                        let c = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                        let Some(text) = open(&key, &bytes[5..]) else {
                            // Wrong key or tampered: drop this browser.
                            browsers.remove(&c);
                            let close = format!(r#"{{"t":"close","c":{c}}}"#);
                            let _ = to_relay.send(Message::Text(close.into())).await;
                            continue;
                        };
                        let inbox = match browsers.get(&c) {
                            Some(Browser::Active(inbox)) => inbox.clone(),
                            Some(Browser::Pending) => {
                                let inbox = start_session(c, dirty.clone(), key.clone(), to_relay.clone());
                                browsers.insert(c, Browser::Active(inbox.clone()));
                                inbox
                            }
                            None => continue,
                        };
                        if inbox.send(text).await.is_err() {
                            browsers.remove(&c);
                        }
                    }
                    Message::Close(_) => break Ok(()),
                    _ => {}
                }
            }
            _ = keepalive.tick() => {
                if last_heard.elapsed() > SILENCE_LIMIT {
                    break Err(anyhow::anyhow!("relay went silent"));
                }
                if to_relay.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break Ok(());
                }
            }
            _ = identity_check.tick() => {
                // A reset identity means a new link; re-register under it so
                // the old one stops working.
                if read_identity().as_ref() != Some(identity) {
                    log::info!("kaku-share: share link changed, re-registering");
                    break Ok(());
                }
            }
        }
    };
    drop(browsers);
    writer.abort();
    result
}

/// Start the session for browser `c`; returns where its messages go.
fn start_session(
    c: u32,
    dirty: WebDirty,
    key: std::sync::Arc<LessSafeKey>,
    to_relay: mpsc::Sender<Message>,
) -> mpsc::Sender<String> {
    let (inbox, session_in) = mpsc::channel::<String>(64);
    let (session_out, mut replies) = mpsc::channel::<String>(64);
    tokio::spawn(web::run_session(session_in, session_out, dirty));
    tokio::spawn(async move {
        let rng = SystemRandom::new();
        while let Some(text) = replies.recv().await {
            let Some(sealed) = seal(&key, &rng, text.as_bytes()) else {
                break;
            };
            let msg = Message::Binary(frame(KIND_DATA, c, &sealed).into());
            if to_relay.send(msg).await.is_err() {
                break;
            }
        }
    });
    inbox
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ShareIdentity {
        ShareIdentity::generate().unwrap()
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
    fn link_carries_id_and_key_in_fragment() {
        let a = identity();
        let link = a.link("wss://relay.example.com/");
        assert_eq!(
            link,
            format!("https://relay.example.com/t/{}/#k={}", a.id(), a.key)
        );
        assert!(!link.contains(&a.secret));
    }

    #[test]
    fn sealed_messages_open_only_in_the_other_direction_with_the_key() {
        let a = identity();
        let key = a.sealing_key().unwrap();
        let rng = SystemRandom::new();
        let from_host = seal(&key, &rng, b"hello").unwrap();
        // The page seals with the c2h label; a host-sealed frame must not
        // open as if it came from the page (no reflection).
        assert!(open(&key, &from_host).is_none());

        let mut nonce = [0u8; NONCE_LEN];
        rng.fill(&mut nonce).unwrap();
        let mut buf = b"from page".to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(TO_HOST),
            &mut buf,
        )
        .unwrap();
        let mut from_page = nonce.to_vec();
        from_page.extend_from_slice(&buf);
        assert_eq!(open(&key, &from_page).as_deref(), Some("from page"));

        let other = identity().sealing_key().unwrap();
        assert!(open(&other, &from_page).is_none());
    }
}
