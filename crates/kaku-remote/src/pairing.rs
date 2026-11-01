//! Which browsers may open this Mac's terminals through the relay, and how a
//! new one joins.
//!
//! A browser joins by opening a pairing link that `kaku remote` prints: it
//! carries this Mac's public signing key and a one-time secret that expires
//! after `OFFER_LIFETIME`. The browser keeps its own signing key, which it
//! cannot export, and Kaku lists its public half as a paired device. Each
//! connection then runs a fresh ECDH exchange; Kaku signs the exchange first,
//! so a page talks only to the Mac it paired with, and the browser signs it
//! next, so Kaku talks only to a paired device. Removing a device from the
//! list ends its sessions within seconds.
//!
//! A new browser joins only once someone at the Mac confirms it: the page
//! and `kaku remote` show the same six-digit code, derived from the link's
//! secret and the browser's key, and Enter in that terminal lets it in. A
//! link that leaked, screenshots and saved scrollback included, is useless
//! without that keypress, and one link holds one pending browser at a time.

use anyhow::Context;
use base64::Engine;
use ring::aead::{LessSafeKey, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{self, EcdsaKeyPair, KeyPair};
use ring::{agreement, hkdf, hmac};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Sent in the clear by Kaku as a browser connects: the tag and Kaku's
/// ephemeral P-256 public key. The page answers with `PAGE_HELLO` and its
/// own.
pub(crate) const MAC_HELLO: &[u8; 4] = b"KCH3";
pub(crate) const PAGE_HELLO: &[u8; 4] = b"KCB3";
/// An uncompressed P-256 point, as WebCrypto exports a raw public key.
pub(crate) const P256_POINT: usize = 65;
/// How long a pairing link can be used, once.
pub const OFFER_LIFETIME: Duration = Duration::from_secs(10 * 60);
const OFFER_SECRET_LEN: usize = 16;
const TRANSCRIPT_LABEL: &[u8] = b"kaku-remote v3";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .ok()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Mac signing key ───────────────────────────────────────────────────────────

/// A new PKCS#8 ECDSA P-256 key for this Mac, base64url.
pub(crate) fn generate_mac_key() -> anyhow::Result<String> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .map_err(|_| anyhow::anyhow!("generate signing key"))?;
    Ok(b64(pkcs8.as_ref()))
}

pub(crate) struct MacKey {
    pair: EcdsaKeyPair,
    rng: SystemRandom,
}

impl MacKey {
    pub(crate) fn from_text(text: &str) -> anyhow::Result<Self> {
        let pkcs8 = unb64(text).context("decode signing key")?;
        let rng = SystemRandom::new();
        let pair =
            EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8, &rng)
                .map_err(|_| anyhow::anyhow!("signing key is not a P-256 key"))?;
        Ok(Self { pair, rng })
    }

    pub(crate) fn public_key(&self) -> &[u8] {
        self.pair.public_key().as_ref()
    }

    /// A raw r||s signature, the form WebCrypto's ECDSA verifies.
    pub(crate) fn sign(&self, transcript: &[u8]) -> Option<Vec<u8>> {
        let sig = self.pair.sign(&self.rng, transcript).ok()?;
        Some(sig.as_ref().to_vec())
    }
}

pub(crate) fn verify(public_key: &[u8], transcript: &[u8], sig: &[u8]) -> bool {
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key)
        .verify(transcript, sig)
        .is_ok()
}

// ── Per-connection exchange ───────────────────────────────────────────────────

/// What both sides sign and derive from: the Mac's relay id and both
/// ephemeral keys, so neither signature fits another connection or Mac.
pub(crate) fn transcript(host_id: &str, mac_eph: &[u8], page_eph: &[u8]) -> [u8; 32] {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(TRANSCRIPT_LABEL);
    ctx.update(host_id.as_bytes());
    ctx.update(mac_eph);
    ctx.update(page_eph);
    let digest = ctx.finish();
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// One AES-256-GCM key per direction.
pub(crate) struct SessionKeys {
    pub(crate) to_host: LessSafeKey,
    pub(crate) from_host: LessSafeKey,
    /// Set once the page says in its proof that it inflates: Kaku's text
    /// messages then go through this one stream (`share::deflate`).
    pub(crate) deflate: std::sync::OnceLock<parking_lot::Mutex<flate2::Compress>>,
}

/// HKDF-SHA256 over the ECDH secret, salted with the transcript; the same
/// derivation as the page's `crypto.subtle.deriveBits({name: "HKDF"})`.
pub(crate) fn derive_keys(shared: &[u8], transcript: &[u8; 32]) -> Option<SessionKeys> {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, transcript).extract(shared);
    let key = |info: &[u8]| -> Option<LessSafeKey> {
        let info = [info];
        let okm = prk.expand(&info, &AES_256_GCM).ok()?;
        Some(LessSafeKey::new(UnboundKey::from(okm)))
    };
    Some(SessionKeys {
        to_host: key(b"kaku c2h")?,
        from_host: key(b"kaku h2c")?,
        deflate: std::sync::OnceLock::new(),
    })
}

/// Kaku's side of one connection's exchange.
pub(crate) struct Exchange {
    private: agreement::EphemeralPrivateKey,
    public: Vec<u8>,
}

impl Exchange {
    pub(crate) fn new(rng: &SystemRandom) -> Option<Self> {
        let private = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, rng).ok()?;
        let public = private.compute_public_key().ok()?.as_ref().to_vec();
        Some(Self { private, public })
    }

    /// `MAC_HELLO` followed by this side's public key.
    pub(crate) fn hello(&self) -> Vec<u8> {
        let mut out = MAC_HELLO.to_vec();
        out.extend_from_slice(&self.public);
        out
    }

    /// Finish with the page's hello; returns the transcript and the keys.
    pub(crate) fn finish(
        self,
        host_id: &str,
        page_hello: &[u8],
    ) -> Option<([u8; 32], SessionKeys)> {
        let page_eph = page_hello.strip_prefix(PAGE_HELLO.as_slice())?;
        if page_eph.len() != P256_POINT {
            return None;
        }
        let transcript = transcript(host_id, &self.public, page_eph);
        let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, page_eph);
        let keys = agreement::agree_ephemeral(self.private, &peer, |shared| {
            derive_keys(shared, &transcript)
        })
        .ok()??;
        Some((transcript, keys))
    }
}

// ── What the page proves ──────────────────────────────────────────────────────

/// The page's first sealed message: a paired device signing in, or a new one
/// joining with a pairing link.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum PageAuth {
    Auth {
        device: String,
        sig: String,
    },
    Pair {
        device: String,
        sig: String,
        proof: String,
        #[serde(default)]
        name: String,
    },
}

/// Why a page was turned away; each has its own close code so the page can
/// say what happened.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Not a paired device (removed, or the Mac was reset): 4403, after a
    /// sealed `revoked` message, the only thing that makes a page forget
    /// its key.
    UnknownDevice,
    /// The pairing link expired, was used, or is not this Mac's: 4401.
    PairingLink,
    /// Another browser opened the link first and waits for confirmation:
    /// 4429.
    LinkInUse,
    /// Malformed, or a signature that does not match: 4403.
    BadProof,
}

impl Refusal {
    pub(crate) fn close_code(&self) -> u16 {
        match self {
            Refusal::PairingLink => 4401,
            Refusal::LinkInUse => 4429,
            Refusal::UnknownDevice | Refusal::BadProof => 4403,
        }
    }
}

/// Closing code for a pairing the person at the Mac refused.
pub(crate) const DECLINED: u16 = 4409;

/// What a page that proved its key gets.
#[derive(Debug, PartialEq)]
pub(crate) enum Admission {
    /// A paired device signing in.
    Known(Device),
    /// A new browser with a valid link, waiting for Enter in `kaku remote`.
    Pending(Device),
}

/// Check the page's proof against the paired devices and the pairing offer.
pub(crate) fn admit(
    auth: &PageAuth,
    transcript: &[u8; 32],
    store: &mut dyn DeviceStore,
) -> Result<Admission, Refusal> {
    let (device, sig) = match auth {
        PageAuth::Auth { device, sig } | PageAuth::Pair { device, sig, .. } => (device, sig),
    };
    let public = unb64(device).ok_or(Refusal::BadProof)?;
    let sig = unb64(sig).ok_or(Refusal::BadProof)?;
    if public.len() != P256_POINT || !verify(&public, transcript, &sig) {
        return Err(Refusal::BadProof);
    }
    let id = device_id(&public);
    let key = b64(&public);
    match auth {
        PageAuth::Auth { .. } => store
            .seen(&id, &key)
            .map(Admission::Known)
            .ok_or(Refusal::UnknownDevice),
        PageAuth::Pair { proof, name, .. } => {
            if let Some(known) = store.seen(&id, &key) {
                return Ok(Admission::Known(known));
            }
            let proof = unb64(proof).ok_or(Refusal::BadProof)?;
            let secret = store.offer_secret().ok_or(Refusal::PairingLink)?;
            let hkey = hmac::Key::new(hmac::HMAC_SHA256, &secret);
            if hmac::verify(&hkey, transcript, &proof).is_err() {
                return Err(Refusal::PairingLink);
            }
            let device = Device {
                id,
                public_key: key,
                name: clean_name(name),
                paired_at: now(),
                last_seen: now(),
            };
            let code = pairing_code(&secret, &public);
            if !store.hold(&device, &code) {
                return Err(Refusal::LinkInUse);
            }
            Ok(Admission::Pending(device))
        }
    }
}

const CODE_LABEL: &[u8] = b"kaku-remote pairing code";

/// The six digits the page and `kaku remote` both show for a browser
/// pairing with a link: an HMAC under the link's secret of the browser's
/// key, so it stays the same across that browser's reconnects and differs
/// for any other browser. The page computes it the same way.
pub(crate) fn pairing_code(secret: &[u8], device_public: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut ctx = hmac::Context::with_key(&key);
    ctx.update(CODE_LABEL);
    ctx.update(device_public);
    let tag = ctx.sign();
    let n = u32::from_be_bytes([tag.as_ref()[0], tag.as_ref()[1], tag.as_ref()[2], tag.as_ref()[3]]);
    let n = n % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// A short id for a device: the first 8 hex digits of its key's SHA-256.
pub(crate) fn device_id(public: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, public);
    digest.as_ref()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Unicode format characters (category Cf) that reorder or hide text, so a
/// name cannot pass for another in a notification or the devices list.
pub(crate) fn is_invisible_format(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}')
}

fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|&c| !c.is_control() && !is_invisible_format(c))
        .take(40)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "Browser".to_string()
    } else {
        name.to_string()
    }
}

// ── Devices and the pairing offer on disk ─────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub id: String,
    pub public_key: String,
    pub name: String,
    pub paired_at: u64,
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Serialize, Deserialize)]
struct Offer {
    secret: String,
    expires_at: u64,
}

/// The browser waiting for Enter in `kaku remote`, and the answer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PairRequest {
    pub device: Device,
    pub code: String,
    /// None until `kaku remote` answers.
    #[serde(default)]
    pub allowed: Option<bool>,
}

/// Where `admit` reads and records devices and the offer; files in the data
/// dir for Kaku, memory in tests.
pub(crate) trait DeviceStore {
    /// The paired device with this id and this whole public key as it was
    /// (`last_seen` is the previous visit), marking it seen now. The short id
    /// alone is not enough: it is shown to people and a key matching its 32
    /// bits can be ground offline.
    fn seen(&mut self, id: &str, public_key: &str) -> Option<Device>;
    fn add(&mut self, device: Device);
    /// The current offer's secret, left in place, if one is out and fresh.
    fn offer_secret(&mut self) -> Option<Vec<u8>>;
    /// Make `device` the one browser waiting on the current offer, or keep
    /// it if it already is; false when another browser holds it.
    fn hold(&mut self, device: &Device, code: &str) -> bool;
    /// The answer for the waiting `device`: None while nobody answered,
    /// Some(false) when refused or no longer waiting.
    fn answer(&mut self, device: &Device) -> Option<bool>;
    /// Use up the offer: pair `device` when allowed, then forget the wait.
    fn settle(&mut self, device: &Device, allowed: bool);
}

fn devices_path() -> PathBuf {
    config::DATA_DIR.join("remote-devices.json")
}

fn offer_path() -> PathBuf {
    config::DATA_DIR.join("remote-pairing.json")
}

fn request_path() -> PathBuf {
    config::DATA_DIR.join("remote-pairing-request.json")
}

fn read_request() -> Option<PairRequest> {
    std::fs::read_to_string(request_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
}

/// The browser waiting for an answer on the current, fresh link, if any.
pub fn pending_request() -> Option<PairRequest> {
    DiskStore.offer_secret()?;
    read_request().filter(|r| r.allowed.is_none())
}

/// Answer the browser waiting with `code`, from `kaku remote`. Returns
/// false when it is no longer the one waiting.
pub fn answer_request(code: &str, allowed: bool) -> anyhow::Result<bool> {
    let Some(mut request) = pending_request().filter(|r| r.code == code) else {
        return Ok(false);
    };
    request.allowed = Some(allowed);
    crate::write_private(&request_path(), serde_json::to_string(&request)?.as_bytes())
        .context("save pairing answer")?;
    Ok(true)
}

/// Whether `device` ended up paired, for `kaku remote` after Enter.
pub fn is_paired(public_key: &str) -> bool {
    list_devices().iter().any(|d| d.public_key == public_key)
}

/// Every device paired with this Mac.
pub fn list_devices() -> Vec<Device> {
    std::fs::read_to_string(devices_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Read, change and write the devices file at `path` under an exclusive
/// lock. The GUI updates it on every sign-in while `kaku remote revoke` edits
/// it from another process; without the lock the GUI could write back a list
/// read before the revoke and bring the device back. `change` returns whether
/// to save.
fn update_devices_at<T>(
    path: &std::path::Path,
    change: impl FnOnce(&mut Vec<Device>) -> (bool, T),
) -> anyhow::Result<T> {
    use std::os::unix::io::AsRawFd;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("create data dir")?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))
        .context("open devices lock")?;
    // SAFETY: flock on a file descriptor we own; released when `lock` closes.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        anyhow::bail!("lock paired devices: {}", std::io::Error::last_os_error());
    }
    let mut devices: Vec<Device> = std::fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default();
    let (save, out) = change(&mut devices);
    if save {
        let json = serde_json::to_string_pretty(&devices)?;
        crate::write_private(path, json.as_bytes()).context("save paired devices")?;
    }
    drop(lock);
    Ok(out)
}

fn update_devices<T>(change: impl FnOnce(&mut Vec<Device>) -> (bool, T)) -> anyhow::Result<T> {
    update_devices_at(&devices_path(), change)
}

fn revoke_at(path: &std::path::Path, id: &str) -> anyhow::Result<bool> {
    update_devices_at(path, |devices| {
        let before = devices.len();
        devices.retain(|d| d.id != id);
        let removed = devices.len() != before;
        (removed, removed)
    })
}

/// Remove a paired device; its open sessions end within seconds. Returns
/// whether it was paired.
pub fn revoke_device(id: &str) -> anyhow::Result<bool> {
    revoke_at(&devices_path(), id)
}

/// Forget every device and any pending pairing link.
pub fn forget_all_devices() -> anyhow::Result<()> {
    update_devices(|devices| {
        devices.clear();
        (true, ())
    })?;
    let _ = std::fs::remove_file(offer_path());
    let _ = std::fs::remove_file(request_path());
    Ok(())
}

/// Make a new pairing link's secret, replacing any earlier one. Returns it
/// base64url for the link's fragment.
pub(crate) fn new_offer() -> anyhow::Result<String> {
    let mut secret = [0u8; OFFER_SECRET_LEN];
    SystemRandom::new()
        .fill(&mut secret)
        .map_err(|_| anyhow::anyhow!("random pairing secret"))?;
    let offer = Offer {
        secret: b64(&secret),
        expires_at: now() + OFFER_LIFETIME.as_secs(),
    };
    let path = offer_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("create data dir")?;
    }
    crate::write_private(&path, serde_json::to_string(&offer)?.as_bytes())
        .context("save pairing link")?;
    // A browser waiting on the old link waits on nothing now.
    let _ = std::fs::remove_file(request_path());
    Ok(offer.secret)
}

fn seen_at(path: &std::path::Path, id: &str, public_key: &str) -> Option<Device> {
    update_devices_at(path, |devices| {
        match devices
            .iter_mut()
            .find(|d| d.id == id && d.public_key == public_key)
        {
            Some(device) => {
                let found = device.clone();
                device.last_seen = now();
                (true, Some(found))
            }
            None => (false, None),
        }
    })
    .ok()
    .flatten()
}

fn add_at(path: &std::path::Path, device: Device) -> anyhow::Result<()> {
    update_devices_at(path, |devices| {
        // Replace this key's entry only: a different key that shares the
        // 8-digit short id must not push a paired device out.
        devices.retain(|d| d.public_key != device.public_key);
        devices.push(device);
        (true, ())
    })
}

/// The devices file and offer file in Kaku's data dir.
pub(crate) struct DiskStore;

impl DeviceStore for DiskStore {
    fn seen(&mut self, id: &str, public_key: &str) -> Option<Device> {
        seen_at(&devices_path(), id, public_key)
    }

    fn add(&mut self, device: Device) {
        if let Err(e) = add_at(&devices_path(), device) {
            log::warn!("kaku-share: {e:#}");
        }
    }

    fn hold(&mut self, device: &Device, code: &str) -> bool {
        if let Some(waiting) = read_request() {
            return waiting.device.public_key == device.public_key && waiting.allowed != Some(false);
        }
        let request = PairRequest {
            device: device.clone(),
            code: code.to_string(),
            allowed: None,
        };
        match serde_json::to_string(&request) {
            Ok(json) => crate::write_private(&request_path(), json.as_bytes()).is_ok(),
            Err(_) => false,
        }
    }

    fn answer(&mut self, device: &Device) -> Option<bool> {
        if self.offer_secret().is_none() {
            return Some(false);
        }
        match read_request() {
            Some(r) if r.device.public_key == device.public_key => r.allowed,
            _ => Some(false),
        }
    }

    fn settle(&mut self, device: &Device, allowed: bool) {
        // A newer `kaku remote` replaced the link and its wait: leave those.
        let ours = read_request().is_some_and(|r| r.device.public_key == device.public_key);
        if !ours {
            return;
        }
        if allowed {
            self.add(device.clone());
        }
        // One device per link, allowed or not.
        let _ = std::fs::remove_file(offer_path());
        let _ = std::fs::remove_file(request_path());
    }

    fn offer_secret(&mut self) -> Option<Vec<u8>> {
        let offer = std::fs::read_to_string(offer_path())
            .ok()
            .and_then(|data| serde_json::from_str::<Offer>(&data).ok())?;
        if offer.expires_at < now() {
            let _ = std::fs::remove_file(offer_path());
            return None;
        }
        unb64(&offer.secret)
    }
}

/// Ids of the devices paired right now, for ending removed ones' sessions.
pub(crate) fn paired_ids() -> std::collections::HashSet<String> {
    list_devices().into_iter().map(|d| d.id).collect()
}

/// A paired device away this long says so on the Mac when it connects
/// again; reloads and reconnects within a session stay quiet.
const QUIET_RETURN: u64 = 30 * 60;

/// Whether a paired device that last connected at `last_seen` is back
/// after long enough to tell the person at the Mac.
pub(crate) fn back_after_a_while(last_seen: u64, now: u64) -> bool {
    now.saturating_sub(last_seen) >= QUIET_RETURN
}

/// Tell the person at the Mac that a paired device is using it again.
pub(crate) fn announce_return(device: &Device) {
    if !back_after_a_while(device.last_seen, now()) {
        return;
    }
    let title = format!("{} is using this Mac's terminals", device.name);
    let message = format!(
        "Connected through Kaku remote. To unpair it, run: kaku remote revoke {}",
        device.id
    );
    promise::spawn::spawn_into_main_thread(async move {
        wezterm_toast_notification::persistent_toast_notification(&title, &message);
    })
    .detach();
}

/// The page link's fragment that pairs one more browser, valid for
/// `OFFER_LIFETIME`: the one-time secret alone. The Mac's key is not in it;
/// Kaku sends the key in its handshake with `mac_key_tag`, which only a
/// holder of this secret can check, and the relay never sees the fragment.
pub(crate) fn pairing_fragment(secret: &str) -> String {
    format!("#{secret}")
}

/// Kept apart from the page's own pairing proof, an HMAC of the transcript
/// alone under the same secret.
const MAC_KEY_LABEL: &[u8] = b"kaku-remote mac key";

/// Proves to a page holding the pairing link that `public` is this Mac's
/// key: an HMAC under the link's secret of the label, this connection's
/// transcript and the key, so a relay without the secret cannot vouch for
/// its own key, and a tag from another connection does not match.
pub(crate) fn mac_key_tag(secret: &[u8], transcript: &[u8], public: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut ctx = hmac::Context::with_key(&key);
    ctx.update(MAC_KEY_LABEL);
    ctx.update(transcript);
    ctx.update(public);
    ctx.sign().as_ref().to_vec()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    pub(crate) struct MemoryStore {
        pub(crate) devices: HashMap<String, Device>,
        pub(crate) offer: Option<(Vec<u8>, bool)>,
        pub(crate) request: Option<PairRequest>,
    }

    impl DeviceStore for MemoryStore {
        fn seen(&mut self, id: &str, public_key: &str) -> Option<Device> {
            self.devices
                .get(id)
                .filter(|d| d.public_key == public_key)
                .cloned()
        }
        fn add(&mut self, device: Device) {
            self.devices.insert(device.id.clone(), device);
        }
        fn offer_secret(&mut self) -> Option<Vec<u8>> {
            match &self.offer {
                Some((secret, false)) => Some(secret.clone()),
                _ => None,
            }
        }
        fn hold(&mut self, device: &Device, code: &str) -> bool {
            match &self.request {
                Some(r) => r.device.public_key == device.public_key && r.allowed != Some(false),
                None => {
                    self.request = Some(PairRequest {
                        device: device.clone(),
                        code: code.to_string(),
                        allowed: None,
                    });
                    true
                }
            }
        }
        fn answer(&mut self, device: &Device) -> Option<bool> {
            if self.offer_secret().is_none() {
                return Some(false);
            }
            match &self.request {
                Some(r) if r.device.public_key == device.public_key => r.allowed,
                _ => Some(false),
            }
        }
        fn settle(&mut self, device: &Device, allowed: bool) {
            if allowed {
                self.add(device.clone());
            }
            if let Some((_, used)) = &mut self.offer {
                *used = true;
            }
            self.request = None;
        }
    }

    /// A browser for tests: its own exchange and signing key, done with ring
    /// the way the page does it with WebCrypto.
    pub(crate) struct TestPage {
        exchange: Exchange,
        key: MacKey,
    }

    impl TestPage {
        pub(crate) fn new() -> Self {
            let rng = SystemRandom::new();
            Self {
                exchange: Exchange::new(&rng).unwrap(),
                key: MacKey::from_text(&generate_mac_key().unwrap()).unwrap(),
            }
        }

        pub(crate) fn hello(&self) -> Vec<u8> {
            let mut out = PAGE_HELLO.to_vec();
            out.extend_from_slice(&self.exchange.public);
            out
        }

        /// Finish against Kaku's hello; the page's keys are the Mac's with
        /// directions swapped, so return them as the Mac names them.
        pub(crate) fn finish(
            self,
            host_id: &str,
            mac_hello: &[u8],
        ) -> ([u8; 32], SessionKeys, MacKey) {
            let mac_eph = mac_hello.strip_prefix(MAC_HELLO.as_slice()).unwrap();
            let transcript = transcript(host_id, mac_eph, &self.exchange.public);
            let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, mac_eph);
            let keys = agreement::agree_ephemeral(self.exchange.private, &peer, |shared| {
                derive_keys(shared, &transcript)
            })
            .unwrap()
            .unwrap();
            (transcript, keys, self.key)
        }
    }

    fn auth(key: &MacKey, transcript: &[u8; 32]) -> PageAuth {
        PageAuth::Auth {
            device: b64(key.public_key()),
            sig: b64(&key.sign(transcript).unwrap()),
        }
    }

    fn pair(key: &MacKey, transcript: &[u8; 32], secret: &[u8]) -> PageAuth {
        let proof = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, secret), transcript);
        PageAuth::Pair {
            device: b64(key.public_key()),
            sig: b64(&key.sign(transcript).unwrap()),
            proof: b64(proof.as_ref()),
            name: "Chrome on Windows".into(),
        }
    }

    fn exchange(host: &str) -> ([u8; 32], [u8; 32], MacKey) {
        let rng = SystemRandom::new();
        let mac = Exchange::new(&rng).unwrap();
        let page = TestPage::new();
        let page_hello = page.hello();
        let mac_hello = mac.hello();
        let (mac_t, _) = mac.finish(host, &page_hello).unwrap();
        let (page_t, _, key) = page.finish(host, &mac_hello);
        (mac_t, page_t, key)
    }

    #[test]
    fn both_sides_derive_the_same_transcript_and_keys() {
        let rng = SystemRandom::new();
        let mac = Exchange::new(&rng).unwrap();
        let page = TestPage::new();
        let (page_hello, mac_hello) = (page.hello(), mac.hello());
        let (mac_t, mac_keys) = mac.finish("host", &page_hello).unwrap();
        let (page_t, page_keys, _) = page.finish("host", &mac_hello);
        assert_eq!(mac_t, page_t);
        // What the page seals toward the Mac opens with the Mac's key.
        let nonce = [0u8; 12];
        let mut msg = b"ls\r".to_vec();
        page_keys
            .to_host
            .seal_in_place_append_tag(
                ring::aead::Nonce::assume_unique_for_key(nonce),
                ring::aead::Aad::empty(),
                &mut msg,
            )
            .unwrap();
        let plain = mac_keys
            .to_host
            .open_in_place(
                ring::aead::Nonce::assume_unique_for_key(nonce),
                ring::aead::Aad::empty(),
                &mut msg,
            )
            .unwrap();
        assert_eq!(plain, b"ls\r");
    }

    #[test]
    fn a_malformed_page_hello_is_refused() {
        let rng = SystemRandom::new();
        assert!(Exchange::new(&rng)
            .unwrap()
            .finish("h", b"KCB2aaaa")
            .is_none());
        let mut short = PAGE_HELLO.to_vec();
        short.extend_from_slice(&[4u8; 10]);
        assert!(Exchange::new(&rng).unwrap().finish("h", &short).is_none());
    }

    #[test]
    fn a_paired_device_signs_in_and_an_unknown_one_does_not() {
        let (transcript, _, key) = exchange("host");
        let mut store = MemoryStore::default();
        assert_eq!(
            admit(&auth(&key, &transcript), &transcript, &mut store),
            Err(Refusal::UnknownDevice)
        );
        let id = device_id(key.public_key());
        store.add(Device {
            id: id.clone(),
            public_key: b64(key.public_key()),
            name: "Safari on iPhone".into(),
            paired_at: 0,
            last_seen: 0,
        });
        assert_eq!(
            admit(&auth(&key, &transcript), &transcript, &mut store).map(known_id),
            Ok(id)
        );
    }

    fn known_id(admission: Admission) -> String {
        match admission {
            Admission::Known(d) => d.id,
            Admission::Pending(d) => format!("pending {}", d.id),
        }
    }

    /// The short id is shown in notifications and `kaku remote devices`, and
    /// a key whose SHA-256 starts with the same 32 bits can be ground
    /// offline. Signing in must take the paired key itself.
    #[test]
    fn a_key_that_only_shares_the_short_id_does_not_sign_in_or_pair_over_it() {
        let (transcript, _, key) = exchange("host");
        let paired = TestPage::new()
            .finish(
                "host",
                &Exchange::new(&SystemRandom::new()).unwrap().hello(),
            )
            .2;
        let mut store = MemoryStore::default();
        // A paired device whose id the attacker's key happens to share.
        store.add(Device {
            id: device_id(key.public_key()),
            public_key: b64(paired.public_key()),
            name: "Safari on iPhone".into(),
            paired_at: 0,
            last_seen: 0,
        });
        assert_eq!(
            admit(&auth(&key, &transcript), &transcript, &mut store).map(known_id),
            Err(Refusal::UnknownDevice)
        );
        // Without a pairing link it cannot pair either, and the paired device
        // is left as it was.
        assert_eq!(
            admit(
                &pair(&key, &transcript, b"no offer"),
                &transcript,
                &mut store
            )
            .map(known_id),
            Err(Refusal::PairingLink)
        );
        assert_eq!(
            store.devices[&device_id(key.public_key())].public_key,
            b64(paired.public_key())
        );
    }

    /// A sign-in that read the list before a revoke must not write the
    /// device back after it.
    #[test]
    fn a_revoked_device_stays_revoked_while_it_keeps_signing_in() {
        let dir = std::env::temp_dir().join(format!("kaku-devices-{}", std::process::id()));
        let path = dir.join("remote-devices.json");
        let device = Device {
            id: "abcd1234".into(),
            public_key: "key".into(),
            name: "Chrome".into(),
            paired_at: 0,
            last_seen: 0,
        };
        for _ in 0..200 {
            update_devices_at(&path, |devices| {
                devices.clear();
                devices.push(device.clone());
                (true, ())
            })
            .unwrap();
            let busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let toucher = {
                let (path, busy) = (path.clone(), busy.clone());
                std::thread::spawn(move || {
                    while busy.load(std::sync::atomic::Ordering::SeqCst) {
                        let _ = seen_at(&path, "abcd1234", "key");
                    }
                })
            };
            assert!(revoke_at(&path, "abcd1234").unwrap());
            busy.store(false, std::sync::atomic::Ordering::SeqCst);
            toucher.join().unwrap();
            assert!(
                seen_at(&path, "abcd1234", "key").is_none(),
                "the revoked device came back"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_device_sharing_a_short_id_does_not_replace_the_paired_one() {
        let dir = std::env::temp_dir().join(format!("kaku-collide-{}", std::process::id()));
        let path = dir.join("remote-devices.json");
        let device = |key: &str| Device {
            id: "abcd1234".into(),
            public_key: key.into(),
            name: "Chrome".into(),
            paired_at: 0,
            last_seen: 0,
        };
        add_at(&path, device("owner")).unwrap();
        add_at(&path, device("ground")).unwrap();
        assert!(seen_at(&path, "abcd1234", "owner").is_some());
        assert!(seen_at(&path, "abcd1234", "ground").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_signature_for_another_connection_does_not_sign_in() {
        // A relay that runs its own exchange with each side gets different
        // transcripts, so it cannot pass the page's signature on.
        let (mac_t, _, _) = exchange("host");
        let (other_t, _, key) = exchange("host");
        assert_ne!(mac_t, other_t);
        let mut store = MemoryStore::default();
        store.add(Device {
            id: device_id(key.public_key()),
            public_key: b64(key.public_key()),
            name: String::new(),
            paired_at: 0,
            last_seen: 0,
        });
        assert_eq!(
            admit(&auth(&key, &other_t), &mac_t, &mut store),
            Err(Refusal::BadProof)
        );
    }

    #[test]
    fn a_pairing_link_adds_one_device_once_it_is_allowed() {
        let secret = vec![7u8; OFFER_SECRET_LEN];
        let mut store = MemoryStore {
            offer: Some((secret.clone(), false)),
            ..Default::default()
        };
        let (t1, _, first) = exchange("host");
        let Ok(Admission::Pending(waiting)) = admit(&pair(&first, &t1, &secret), &t1, &mut store)
        else {
            panic!("a valid link should wait for confirmation");
        };
        assert_eq!(waiting.name, "Chrome on Windows");
        // Nothing is paired before Enter.
        assert!(store.devices.is_empty());
        assert_eq!(store.answer(&waiting), None);
        // Another browser with the same link while the first one waits.
        let (t2, _, second) = exchange("host");
        assert_eq!(
            admit(&pair(&second, &t2, &secret), &t2, &mut store),
            Err(Refusal::LinkInUse)
        );
        // The first one reconnecting keeps its place and its code.
        let (t3, _, _) = exchange("host");
        assert_eq!(
            admit(&pair(&first, &t3, &secret), &t3, &mut store),
            Ok(Admission::Pending(waiting.clone()))
        );
        store.request.as_mut().unwrap().allowed = Some(true);
        assert_eq!(store.answer(&waiting), Some(true));
        store.settle(&waiting, true);
        assert!(store.devices.contains_key(&waiting.id));
        // The link is used up.
        let (t4, _, third) = exchange("host");
        assert_eq!(
            admit(&pair(&third, &t4, &secret), &t4, &mut store),
            Err(Refusal::PairingLink)
        );
        // The paired browser coming back with its link still open signs in.
        let (t5, _, _) = exchange("host");
        assert!(matches!(
            admit(&pair(&first, &t5, &secret), &t5, &mut store),
            Ok(Admission::Known(_))
        ));
    }

    #[test]
    fn a_refused_pairing_adds_nothing_and_uses_up_the_link() {
        let secret = vec![7u8; OFFER_SECRET_LEN];
        let mut store = MemoryStore {
            offer: Some((secret.clone(), false)),
            ..Default::default()
        };
        let (t, _, key) = exchange("host");
        let Ok(Admission::Pending(waiting)) = admit(&pair(&key, &t, &secret), &t, &mut store)
        else {
            panic!("a valid link should wait for confirmation");
        };
        store.request.as_mut().unwrap().allowed = Some(false);
        assert_eq!(store.answer(&waiting), Some(false));
        store.settle(&waiting, false);
        assert!(store.devices.is_empty());
        let (t2, _, _) = exchange("host");
        assert_eq!(
            admit(&pair(&key, &t2, &secret), &t2, &mut store),
            Err(Refusal::PairingLink)
        );
    }

    #[test]
    fn the_code_follows_the_browser_key_and_the_link() {
        let a = pairing_code(&[7u8; 16], &[4u8; 65]);
        assert_eq!(a, pairing_code(&[7u8; 16], &[4u8; 65]));
        assert_ne!(a, pairing_code(&[7u8; 16], &[5u8; 65]));
        assert_ne!(a, pairing_code(&[8u8; 16], &[4u8; 65]));
        assert_eq!(a.len(), 7);
        assert_eq!(&a[3..4], " ");
        assert!(a.chars().filter(|c| *c != ' ').all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn a_wrong_pairing_secret_adds_nothing() {
        let mut store = MemoryStore {
            offer: Some((vec![7u8; OFFER_SECRET_LEN], false)),
            ..Default::default()
        };
        let (t, _, key) = exchange("host");
        assert_eq!(
            admit(&pair(&key, &t, &[8u8; OFFER_SECRET_LEN]), &t, &mut store),
            Err(Refusal::PairingLink)
        );
        assert!(store.devices.is_empty());
        // The real link still works afterwards.
        assert!(matches!(
            admit(&pair(&key, &t, &[7u8; OFFER_SECRET_LEN]), &t, &mut store),
            Ok(Admission::Pending(_))
        ));
    }

    #[test]
    fn only_a_device_back_after_a_while_is_announced() {
        assert!(back_after_a_while(0, QUIET_RETURN));
        assert!(back_after_a_while(1_000, 1_000 + QUIET_RETURN));
        // A reload or reconnect within the half hour stays quiet.
        assert!(!back_after_a_while(1_000, 1_000 + QUIET_RETURN - 1));
        assert!(!back_after_a_while(1_000, 900));
    }

    #[test]
    fn device_names_are_bounded_and_printable() {
        assert_eq!(clean_name("  "), "Browser");
        assert_eq!(clean_name("Chrome\u{7}\non Windows"), "Chromeon Windows");
        assert_eq!(clean_name(&"x".repeat(100)).len(), 40);
        // Right-to-left overrides and zero-width characters cannot disguise
        // one name as another.
        assert_eq!(clean_name("invoice\u{202E}gpj.app"), "invoicegpj.app");
        assert_eq!(clean_name("Chr\u{200B}ome"), "Chrome");
    }
}
