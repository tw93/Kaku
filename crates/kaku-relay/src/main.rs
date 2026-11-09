//! Relay between a Kaku on someone's Mac and the browsers opening its web link.
//!
//! The Mac connects out to `/host/{id}` and proves it owns `id`, which is the
//! first 128 bits of SHA-256 of a secret only the Mac holds. Browsers load
//! `/t/{id}/`, whose files the relay fetches from that Mac, and open
//! `/t/{id}/ws`. Messages between a browser and its Mac are sealed end to end
//! with a key carried in the link's fragment, so the relay only numbers the
//! browsers and forwards bytes it cannot read.

mod usage;

use axum::extract::{ws, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

const MAX_HOSTS: usize = 10_000;
const MAX_BROWSERS_PER_HOST: usize = 8;
/// Browser sockets per client address for one host, so one address cannot
/// take every slot with sockets that never authenticate.
const MAX_BROWSERS_PER_ADDR: usize = 4;
/// Registrations kept per id: the serving one and one standby (a second
/// Kaku on the same Mac). A newer one pushes the oldest out.
const MAX_REGISTRATIONS_PER_ID: usize = 2;
/// Macs one address may keep registered at once; an office NAT can hold a
/// few dozen Kaku users.
const MAX_HOSTS_PER_ADDR: usize = 32;
/// New relay sockets (Macs and browsers) one address may open per minute.
const CONNECTS_PER_MINUTE: u32 = 120;
/// Wrong access codes one address may try before it waits out the window.
const ACCESS_FAILURES: u32 = 10;
const ACCESS_WINDOW: Duration = Duration::from_secs(10 * 60);
/// A Mac sends frames of a whole screen; a page sends keys, pastes capped at
/// 64 KB and 64 KB upload chunks.
const MAX_HOST_MESSAGE: usize = 4 << 20;
const MAX_BROWSER_MESSAGE: usize = 512 << 10;
/// Messages queued toward one socket before it counts as stuck.
const QUEUE: usize = 256;
/// Bytes queued toward one Mac. Counting messages alone let a host that
/// stopped reading hold hundreds of megabytes.
const HOST_QUEUE_BYTES: usize = 8 << 20;
/// Bytes queued toward one browser. Only a count bounded it before, and 256
/// Mac messages of up to 4 MB each let one stalled page hold a gigabyte.
const BROWSER_QUEUE_BYTES: usize = 8 << 20;
/// Bytes queued toward every socket together. Per-socket budgets alone add
/// up past the machine's 256 MB once enough sockets stall at once; past this
/// the socket that wants to queue more is let go.
const GLOBAL_QUEUE_BYTES: usize = 96 << 20;
/// The Mac pings every 20 s and a page every 20 s (a background tab about
/// once a minute); silence past these means a half-open socket.
const HOST_SILENCE: Duration = Duration::from_secs(75);
const BROWSER_SILENCE: Duration = Duration::from_secs(150);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const KIND_DATA: u8 = 0;
/// What a browser the Mac has not admitted may pass on to it. Before the
/// Mac's `ok` a page sends at most four messages: the 69-byte `KCB3` hello,
/// its sealed proof (under 1 KB), then the `hello` and `attach` it queued
/// while connecting, which can reach the relay before `ok` does. This is
/// twice that. Past it the relay stops reading that browser until the Mac
/// admits it, so a socket that only knows the id cannot flood the Mac's one
/// relay link, and nothing a slow admission overlaps is lost.
const PENDING_MESSAGES: u32 = 8;
const PENDING_BYTES: usize = 16 << 10;
/// Longest a browser may stay unadmitted. Pairing can wait for the user to
/// confirm it on the Mac for as long as the pairing offer lives (10 minutes);
/// the Mac's own timeout ends a socket that never proves itself well before.
const PENDING_LIMIT: Duration = Duration::from_secs(11 * 60);
/// Bytes per second one Mac's link carries in each direction, with a burst.
/// 24 Mbps sustained is above what the desktop view's H.264 pictures and a
/// 50 MB download need (a download's 256 KB window is the slower limit over
/// most links), and the burst lets a whole file through at line speed; what
/// it stops is a Mac and its own browser using the relay as a 150 Mbps pipe.
/// A link starts with only `HOST_RATE_START`, so reconnecting does not buy a
/// fresh burst. Over the rate the relay reads that side more slowly instead
/// of dropping it.
const HOST_RATE: f64 = (3 << 20) as f64;
const HOST_BURST: f64 = (32 << 20) as f64;
const HOST_RATE_START: f64 = (1 << 20) as f64;
/// New browser sockets one host takes, from addresses none of its admitted
/// browsers came from: a burst for a few tabs reconnecting at once, then one
/// every two seconds. Refused before the upgrade, so they cannot push a
/// browser out of a full host (`make_room`).
const JOIN_BURST: f64 = 12.0;
const JOIN_RATE: f64 = 0.5;
/// Addresses remembered per host as having had an admitted browser.
const MAX_TRUSTED_ADDRS: usize = 64;
/// Time to read a request's headers; a client that trickles them in holds a
/// connection for at most this long.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(20);
/// How often the relay logs how many Macs and browsers it carries and saves
/// the anonymous counts (`usage.rs`). A Mac stays connected for days without
/// registering again, so each round also counts the ones online for today.
const USAGE_EVERY: Duration = Duration::from_secs(10 * 60);
/// How long a stopping relay waits for its Macs to take the restart close
/// before it exits; Fly gives a stopping machine 5 s before it kills it.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// A token bucket. `take` may go into debt and returns how long the caller
/// should wait for it to clear, which paces a stream without refusing a
/// message larger than what is left; `try_take` refuses instead.
struct TokenBucket {
    tokens: f64,
    at: Instant,
    rate: f64,
    burst: f64,
}

impl TokenBucket {
    fn new(start: f64, rate: f64, burst: f64, now: Instant) -> Self {
        Self {
            tokens: start,
            at: now,
            rate,
            burst,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.at = self.at.max(now);
    }

    fn take(&mut self, n: f64, now: Instant) -> Duration {
        self.refill(now);
        self.tokens -= n;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / self.rate)
        }
    }

    fn try_take(&mut self, n: f64, now: Instant) -> bool {
        self.refill(now);
        if self.tokens < n {
            return false;
        }
        self.tokens -= n;
        true
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

/// The web terminal's page and files, built into the relay from the same
/// sources Kaku serves on the LAN. Every Mac's page shares this origin, so
/// it must not be whatever each Mac sends: a Mac whose owner altered its
/// page could otherwise use the keys a browser holds for other Macs.
fn page_file(name: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match name {
        "index.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/index.html"),
        ),
        "xterm.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/xterm.js"),
        ),
        "xterm-webgl.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/xterm-webgl.js"),
        ),
        "xterm-unicode11.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/xterm-unicode11.js"),
        ),
        "desktop.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/desktop.js"),
        ),
        "xterm.css" => (
            "text/css; charset=utf-8",
            include_bytes!("../../kaku-remote/assets/web/xterm.css"),
        ),
        "icon.png" => (
            "image/png",
            include_bytes!("../../kaku-remote/assets/web/icon.png"),
        ),
        "font-regular.woff2" => (
            "font/woff2",
            include_bytes!("../../kaku-remote/assets/web/font-regular.woff2"),
        ),
        "font-medium.woff2" => (
            "font/woff2",
            include_bytes!("../../kaku-remote/assets/web/font-medium.woff2"),
        ),
        "font-symbols.woff2" => (
            "font/woff2",
            include_bytes!("../../kaku-remote/assets/web/font-symbols.woff2"),
        ),
        _ => return None,
    })
}

/// Bytes waiting in one socket's outgoing queue, within its own budget and
/// the relay-wide one. Whatever is still queued when the last holder drops it
/// (the socket ended with messages unsent) goes back to the relay-wide count.
struct Queued {
    bytes: AtomicUsize,
    cap: usize,
    global: Arc<AtomicUsize>,
}

impl Queued {
    fn new(cap: usize, global: &Arc<AtomicUsize>) -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            cap,
            global: global.clone(),
        }
    }

    fn reserve(&self, len: usize) -> bool {
        if self.bytes.fetch_add(len, Ordering::Relaxed) + len > self.cap {
            self.bytes.fetch_sub(len, Ordering::Relaxed);
            return false;
        }
        if self.global.fetch_add(len, Ordering::Relaxed) + len > GLOBAL_QUEUE_BYTES {
            self.global.fetch_sub(len, Ordering::Relaxed);
            self.bytes.fetch_sub(len, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn release(&self, len: usize) {
        self.bytes.fetch_sub(len, Ordering::Relaxed);
        self.global.fetch_sub(len, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn load(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }
}

impl Drop for Queued {
    fn drop(&mut self) {
        self.global
            .fetch_sub(*self.bytes.get_mut(), Ordering::Relaxed);
    }
}

struct BrowserLink {
    data: mpsc::Sender<ws::Message>,
    queued: Arc<Queued>,
    close: watch::Sender<Option<u64>>,
    addr: String,
    /// The Mac admitted it (`{"t":"ok"}`): a paired browser, not a socket
    /// that only knows the id.
    admitted: bool,
    /// Wakes the browser's reader, paused at `PENDING_MESSAGES` or
    /// `PENDING_BYTES`, once the Mac admits it.
    admit: Arc<tokio::sync::Notify>,
    /// Messages passed on to the Mac before admission.
    sent: u32,
}

/// Make room for one more browser of a host at `MAX_BROWSERS_PER_HOST`: a
/// socket the Mac has not admitted is let go (4413), so sockets that only
/// know the id cannot hold every slot and lock the owner out. The one let go
/// has come the shortest way through the handshake, the oldest of those
/// first: the Mac closes a socket whose hello or proof is wrong, so one
/// still open after both is a real browser, perhaps waiting for its pairing
/// to be confirmed on the Mac, and new sockets from someone who only knows
/// the id never push it out. False when every slot holds an admitted browser.
fn make_room(browsers: &mut HashMap<u32, BrowserLink>) -> bool {
    if browsers.len() < MAX_BROWSERS_PER_HOST {
        return true;
    }
    let Some(oldest) = browsers
        .iter()
        .filter(|(_, b)| !b.admitted)
        .map(|(c, b)| (b.sent.min(2), *c))
        .min()
        .map(|(_, c)| c)
    else {
        return false;
    };
    if let Some(link) = browsers.remove(&oldest) {
        let _ = link.close.send(Some(4413));
    }
    true
}

struct Host {
    /// Distinguishes this registration from a later one under the same id.
    generation: u64,
    to_host: mpsc::Sender<ws::Message>,
    /// Bytes waiting in `to_host`.
    queued: Queued,
    /// Ends this registration when newer ones push it out.
    evicted: tokio::sync::Notify,
    browsers: Mutex<HashMap<u32, BrowserLink>>,
    next_id: AtomicU32,
    /// The client address it registered from (`MAX_HOSTS_PER_ADDR`).
    addr: String,
    /// Bytes from its browsers to it, and from it to them (`HOST_RATE`).
    upstream: Mutex<TokenBucket>,
    downstream: Mutex<TokenBucket>,
    /// New browser sockets (`JOIN_BURST`), and the addresses its admitted
    /// browsers came from, which join without spending it.
    joins: Mutex<TokenBucket>,
    trusted: Mutex<std::collections::HashSet<String>>,
}

fn message_len(msg: &ws::Message) -> usize {
    match msg {
        ws::Message::Binary(b) => b.len(),
        ws::Message::Text(t) => t.len(),
        _ => 0,
    }
}

impl Host {
    fn new(
        generation: u64,
        to_host: mpsc::Sender<ws::Message>,
        addr: String,
        global: &Arc<AtomicUsize>,
    ) -> Self {
        let now = Instant::now();
        let rate = || {
            Mutex::new(TokenBucket::new(
                HOST_RATE_START,
                HOST_RATE,
                HOST_BURST,
                now,
            ))
        };
        Self {
            generation,
            to_host,
            queued: Queued::new(HOST_QUEUE_BYTES, global),
            evicted: tokio::sync::Notify::new(),
            browsers: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            addr,
            upstream: rate(),
            downstream: rate(),
            joins: Mutex::new(TokenBucket::new(JOIN_BURST, JOIN_RATE, JOIN_BURST, now)),
            trusted: Mutex::new(Default::default()),
        }
    }

    /// Whether a new browser socket from `addr` may join. Addresses an
    /// admitted browser came from always may, so whoever spends the budget
    /// cannot lock the owner's own browsers out. Local runs (`-`) are not
    /// limited.
    fn allow_join(&self, addr: &str, now: Instant) -> bool {
        addr == "-" || self.trusted.lock().contains(addr) || self.joins.lock().try_take(1.0, now)
    }

    fn trust(&self, addr: &str) {
        let mut trusted = self.trusted.lock();
        if trusted.len() < MAX_TRUSTED_ADDRS {
            trusted.insert(addr.to_string());
        }
    }

    /// Queue a message for the Mac within the byte budgets.
    fn send(&self, msg: ws::Message) -> bool {
        let len = message_len(&msg);
        if !self.queued.reserve(len) {
            return false;
        }
        if self.to_host.try_send(msg).is_err() {
            self.queued.release(len);
            return false;
        }
        true
    }
}

impl BrowserLink {
    /// Queue a message for the browser within the byte budgets.
    fn send(&self, msg: ws::Message) -> bool {
        let len = message_len(&msg);
        if !self.queued.reserve(len) {
            return false;
        }
        if self.data.try_send(msg).is_err() {
            self.queued.release(len);
            return false;
        }
        true
    }
}

#[derive(Clone, Default)]
struct AppState {
    /// Every connected registration per id; the last one serves browsers.
    /// Two Kakus can share an identity (a second instance on the same Mac):
    /// when the newer one leaves, the older one, still connected, takes over
    /// instead of the link reading offline while a Kaku is running.
    hosts: Arc<Mutex<HashMap<String, Vec<Arc<Host>>>>>,
    generation: Arc<AtomicU64>,
    /// The access code a Mac must present to register; unset, none is asked.
    access: Option<Arc<AccessCode>>,
    limits: Arc<Mutex<AddrLimits>>,
    /// Where the page lives when it is not served here
    /// (`KAKU_RELAY_PAGE_ORIGIN`, e.g. `https://t.kaku.fun`): page addresses
    /// redirect there and browsers are admitted only from it. Unset (local
    /// runs, tests), the relay serves its own copy to any origin.
    page_origin: Option<Arc<str>>,
    /// Bytes queued toward every socket (`GLOBAL_QUEUE_BYTES`).
    queued_bytes: Arc<AtomicUsize>,
    /// Anonymous counts per day and overall; unset, nothing is kept.
    usage: Option<Arc<Mutex<usage::Usage>>>,
}

impl AppState {
    fn note_mac(&self, id: &str, used: bool) {
        if let Some(usage) = &self.usage {
            usage.lock().note(id, used, &usage::today());
        }
    }

    /// Count every Mac online for today, and return the counts so far.
    fn note_online(&self, day: &str) -> Option<usage::Summary> {
        let usage = self.usage.as_ref()?;
        let online: Vec<(String, bool)> = self
            .hosts
            .lock()
            .iter()
            .filter_map(|(id, all)| {
                let used = all.last()?.browsers.lock().values().any(|b| b.admitted);
                Some((id.clone(), used))
            })
            .collect();
        let mut usage = usage.lock();
        for (id, used) in online {
            usage.note(&id, used, day);
        }
        Some(usage.summary(day))
    }

    /// Write the counts out, away from the threads carrying traffic.
    async fn save_usage(&self) {
        let Some(usage) = &self.usage else {
            return;
        };
        let (path, text) = {
            let usage = usage.lock();
            (usage.path().to_path_buf(), usage.snapshot())
        };
        match tokio::task::spawn_blocking(move || usage::write_file(&path, &text)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!("usage counts not saved: {error}"),
            Err(error) => warn!("usage counts not saved: {error}"),
        }
    }

    /// Tell every registered Mac the relay is going away for a restart
    /// (close 1012), so it retries every second until the new relay is up
    /// instead of backing off. Returns how many registrations were told.
    fn announce_restart(&self) -> usize {
        let hosts: Vec<Arc<Host>> = self.hosts.lock().values().flatten().cloned().collect();
        hosts
            .iter()
            .filter(|host| {
                host.send(ws::Message::Close(Some(ws::CloseFrame {
                    code: 1012,
                    reason: "relay restarting".into(),
                })))
            })
            .count()
    }

    /// Macs online (ids, not registrations), browsers connected to them,
    /// and of those the ones their Mac admitted.
    fn usage(&self) -> (usize, usize, usize) {
        let hosts = self.hosts.lock();
        let (mut browsers, mut admitted) = (0, 0);
        for host in hosts.values().filter_map(|all| all.last()) {
            let links = host.browsers.lock();
            browsers += links.len();
            admitted += links.values().filter(|b| b.admitted).count();
        }
        (hosts.len(), browsers, admitted)
    }

    fn host(&self, id: &str) -> Option<Arc<Host>> {
        self.hosts
            .lock()
            .get(id)
            .and_then(|all| all.last().cloned())
    }

    /// Make `host` the one serving `id`; returns the one it displaced.
    /// Admission and insertion share the lock, including after an upgrade.
    fn register(&self, id: &str, host: Arc<Host>) -> Result<Option<Arc<Host>>, StatusCode> {
        let mut hosts = self.hosts.lock();
        Self::check_registration(&hosts, id, &host.addr)?;
        let all = hosts.entry(id.to_string()).or_default();
        let displaced = all.last().cloned();
        all.push(host);
        while all.len() > MAX_REGISTRATIONS_PER_ID {
            let oldest = all.remove(0);
            oldest.evicted.notify_one();
        }
        Ok(displaced)
    }

    fn check_registration(
        hosts: &HashMap<String, Vec<Arc<Host>>>,
        id: &str,
        addr: &str,
    ) -> Result<(), StatusCode> {
        if hosts.len() >= MAX_HOSTS && !hosts.contains_key(id) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let from_addr = hosts
            .iter()
            .filter(|(other, all)| other.as_str() != id && all.iter().any(|h| h.addr == addr))
            .count();
        if addr != "-" && from_addr >= MAX_HOSTS_PER_ADDR {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        Ok(())
    }

    /// Drop this registration; returns how many remain for `id`.
    fn unregister(&self, id: &str, generation: u64) -> usize {
        let mut hosts = self.hosts.lock();
        let Some(all) = hosts.get_mut(id) else {
            return 0;
        };
        all.retain(|h| h.generation != generation);
        let left = all.len();
        if left == 0 {
            hosts.remove(id);
        }
        left
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

/// The access code Macs present to register (`KAKU_RELAY_ACCESS_CODE`, a
/// fly secret), kept as its SHA-256 and compared in constant time.
struct AccessCode {
    digest: Vec<u8>,
}

impl AccessCode {
    fn new(code: &str) -> Self {
        Self {
            digest: Sha256::digest(code.trim().as_bytes()).to_vec(),
        }
    }

    fn from_env() -> Option<Self> {
        let code = std::env::var("KAKU_RELAY_ACCESS_CODE").ok()?;
        (!code.trim().is_empty()).then(|| Self::new(&code))
    }

    fn admits(&self, presented: &str) -> bool {
        let digest = Sha256::digest(presented.trim().as_bytes());
        digest
            .iter()
            .zip(&self.digest)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

/// Per-address budgets: new sockets per minute, and wrong access codes per
/// `ACCESS_WINDOW`. Local runs have no client address (`-`) and are not
/// limited, so tests can open as many sockets as they need.
#[derive(Default)]
struct AddrLimits {
    connects: Counts,
    failures: Counts,
}

/// Counts per address within a window, for one budget.
#[derive(Default)]
struct Counts {
    map: HashMap<String, (Instant, u32)>,
    /// When this map was last swept; at most once a second, or a flood of
    /// fresh sources would make every connect scan the whole map. Each map
    /// keeps its own, so sweeping one never postpones the other's sweep.
    swept_at: Option<Instant>,
}

impl Counts {
    fn sweep(&mut self, window: Duration, now: Instant) {
        if self.map.len() > 10_000
            && self
                .swept_at
                .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1))
        {
            self.map
                .retain(|_, (start, _)| now.duration_since(*start) < window);
            self.swept_at = Some(now);
        }
    }

    fn count(&mut self, addr: &str, window: Duration, now: Instant) -> u32 {
        self.sweep(window, now);
        let entry = self.map.entry(addr.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1
    }
}

impl AddrLimits {
    fn allow_connect(&mut self, addr: &str, now: Instant) -> bool {
        // Wrong codes are only counted when they happen, so a map full of
        // them is also swept here rather than left to the next failure.
        self.failures.sweep(ACCESS_WINDOW, now);
        addr == "-"
            || self.connects.count(addr, Duration::from_secs(60), now) <= CONNECTS_PER_MINUTE
    }

    fn access_locked(&self, addr: &str, now: Instant) -> bool {
        addr != "-"
            && self.failures.map.get(addr).is_some_and(|(start, n)| {
                now.duration_since(*start) < ACCESS_WINDOW && *n >= ACCESS_FAILURES
            })
    }

    fn note_access_failure(&mut self, addr: &str, now: Instant) {
        if addr != "-" {
            self.failures.count(addr, ACCESS_WINDOW, now);
        }
    }
}

/// Fly puts the real client address in `fly-client-ip`; without it (local
/// runs) every client shares the `-` bucket.
fn client_addr(headers: &HeaderMap) -> String {
    source_of(
        headers
            .get("fly-client-ip")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-"),
    )
}

/// The client a budget is kept for: an IPv4 address, or an IPv6 /64. r.kaku.fun
/// answers on IPv6 too, and one IPv6 client holds a whole /64 to pick source
/// addresses from, so counting full addresses let it start fresh on every
/// request. Anything unparsable is kept as it came.
fn source_of(raw: &str) -> String {
    match raw.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
        Ok(std::net::IpAddr::V4(v4)) => v4.to_string(),
        Err(_) => raw.to_string(),
    }
}

/// A refusal the Mac shows its user: the status and why, as plain text.
fn refuse(status: StatusCode, why: &'static str) -> Response {
    (status, why).into_response()
}

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
    let addr = client_addr(&headers);
    let now = Instant::now();
    if !state.limits.lock().allow_connect(&addr, now) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many connections");
    }
    if let Some(access) = &state.access {
        if state.limits.lock().access_locked(&addr, now) {
            return refuse(StatusCode::TOO_MANY_REQUESTS, "too many wrong access codes");
        }
        match headers.get("x-kaku-access").and_then(|v| v.to_str().ok()) {
            None => return refuse(StatusCode::UNAUTHORIZED, "access code required"),
            Some(code) if !access.admits(code) => {
                state.limits.lock().note_access_failure(&addr, now);
                info!(host = short(&id), "wrong access code");
                return refuse(StatusCode::UNAUTHORIZED, "wrong access code");
            }
            Some(_) => {}
        }
    }
    if let Err(status) = AppState::check_registration(&state.hosts.lock(), &id, &addr) {
        return if status == StatusCode::TOO_MANY_REQUESTS {
            refuse(status, "too many Macs from this address")
        } else {
            status.into_response()
        };
    }
    upgrade
        // A frame is buffered whole before the message cap is checked, so
        // cap frames too, or one 16 MB frame header pins 16 MB per socket.
        .max_frame_size(MAX_HOST_MESSAGE)
        .max_message_size(MAX_HOST_MESSAGE)
        .on_upgrade(move |socket| serve_host(socket, id, edge(&headers), addr, state))
}

async fn serve_host(
    socket: ws::WebSocket,
    id: String,
    edge: String,
    addr: String,
    state: AppState,
) {
    let (mut sink, mut stream) = socket.split();
    let (to_host, mut outgoing) = mpsc::channel::<ws::Message>(QUEUE);
    let host = Arc::new(Host::new(
        state.generation.fetch_add(1, Ordering::Relaxed),
        to_host,
        addr,
        &state.queued_bytes,
    ));
    // The newest registration serves; the previous one stays connected as a
    // standby, and its browsers reconnect to this one.
    match state.register(&id, host.clone()) {
        Ok(Some(old)) => old.browsers.lock().clear(),
        Ok(None) => {}
        Err(_) => {
            // Other handshakes can consume the last slot during the upgrade.
            let _ = tokio::time::timeout(WRITE_TIMEOUT, sink.send(browser_close(4413))).await;
            return;
        }
    }
    info!("host {} registered via edge {}", short(&id), edge);
    state.note_mac(&id, false);

    let writer_host = host.clone();
    let mut writer = tokio::spawn(async move {
        while let Some(msg) = outgoing.recv().await {
            writer_host.queued.release(message_len(&msg));
            if !matches!(
                tokio::time::timeout(WRITE_TIMEOUT, sink.send(msg)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });

    let connected_at = tokio::time::Instant::now();
    let ended = loop {
        let msg = tokio::select! {
            _ = &mut writer => break "writer-stopped",
            _ = host.evicted.notified() => break "evicted",
            _ = tokio::time::sleep(HOST_SILENCE) => break "silent",
            next = stream.next() => match next {
                Some(Ok(msg)) => msg,
                Some(Err(error)) => {
                    info!(host = short(&id), error = %error, "host transport failed");
                    break "socket-error";
                }
                None => break "eof",
            }
        };
        match msg {
            // Older Kakus also answer page file requests (kind 1); the relay
            // serves its own copy now and asks for none.
            ws::Message::Binary(bytes) if bytes.len() >= 5 && bytes[0] == KIND_DATA => {
                // Over `HOST_RATE` the Mac is read more slowly; a wait stays
                // within a few seconds, well inside its write timeout.
                let wait = host
                    .downstream
                    .lock()
                    .take(bytes.len() as f64, Instant::now());
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
                let target = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                let mut browsers = host.browsers.lock();
                if let Some(link) = browsers.get(&target) {
                    let payload = bytes.slice(5..);
                    if !link.send(ws::Message::Binary(payload)) {
                        // A browser that cannot keep up is let go.
                        browsers.remove(&target);
                    }
                }
            }
            ws::Message::Text(text) => {
                // The Mac rejected a browser (its key did not match), or
                // admitted one.
                if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(text.as_str()) {
                    if map.get("t").and_then(|t| t.as_str()) == Some("ok") {
                        if let Some(c) = map.get("c").and_then(|c| c.as_u64()) {
                            if let Some(link) = host.browsers.lock().get_mut(&(c as u32)) {
                                link.admitted = true;
                                link.admit.notify_one();
                                host.trust(&link.addr);
                                state.note_mac(&id, true);
                            }
                        }
                    }
                    if map.get("t").and_then(|t| t.as_str()) == Some("close") {
                        if let Some(c) = map.get("c").and_then(|c| c.as_u64()) {
                            if let Some(tx) = host.browsers.lock().remove(&(c as u32)) {
                                let code = map.get("code").and_then(|v| v.as_u64()).unwrap_or(4403);
                                let _ = tx.close.send(Some(code));
                            }
                        }
                    }
                }
            }
            ws::Message::Close(_) => break "close-frame",
            _ => {}
        }
    };
    info!(
        host = short(&id),
        reason = ended,
        seconds = connected_at.elapsed().as_secs(),
        "host connection ended"
    );

    writer.abort();
    let left = state.unregister(&id, host.generation);
    host.browsers.lock().clear();
    if left > 0 {
        info!(
            "host {} gone, an earlier registration takes over",
            short(&id)
        );
    } else {
        info!("host {} gone", short(&id));
    }
}

/// Shown while the Mac's Kaku is not connected. It looks like the terminal
/// page's own notice and opens the terminal by itself once the Mac is back;
/// the link's key is still in the address bar, so the reload keeps it.
const OFFLINE_PAGE: &str = r#"<!doctype html><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>Kaku</title>
<style>
html,body{height:100%;margin:0;background:#15141a;color:#d4d4d8}
body{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;padding:24px;box-sizing:border-box;text-align:center;font:14px -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased}
.title{font-size:15px}.detail{font-size:13px;color:#8b8b93}
</style>
<div class="title">Kaku on the Mac is offline</div>
<div class="detail">Open Kaku on the Mac, or run kaku doctor there.</div>
<script>
setInterval(function () {
  fetch(location.pathname, { method: "HEAD", cache: "no-store" })
    .then(function (r) { if (r.ok) location.reload(); })
    .catch(function () {});
}, 3000);
</script>"#;

fn asset_response(name: &str) -> Response {
    let Some((content_type, body)) = page_file(name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            // The page comes fresh so an update reaches it at once; xterm.js
            // and the fonts are ~1 MB that rarely change.
            (
                header::CACHE_CONTROL,
                if name == "index.html" {
                    "no-cache"
                } else {
                    "max-age=3600"
                },
            ),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        body,
    )
        .into_response()
}

async fn page_redirect(Path(id): Path<String>) -> Response {
    // A decoded `%2F` would otherwise make `//host/` and leave the site.
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Keep the trailing slash so the page's relative paths resolve under it;
    // the browser carries the fragment (the key) over the redirect.
    Redirect::permanent(&format!("/t/{id}/")).into_response()
}

async fn short_page_redirect(Path(id): Path<String>) -> Response {
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    Redirect::permanent(&format!("/{id}/")).into_response()
}

async fn page(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    // The page lives on its own origin: links from older Kakus land there,
    // and the browser carries the fragment (the pairing secret) over.
    if let Some(origin) = &state.page_origin {
        return Redirect::permanent(&format!("{origin}/{id}/")).into_response();
    }
    // The page answers 404 while the Mac is away: that is how an open page
    // tells an offline Mac from a network problem.
    if state.host(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            OFFLINE_PAGE,
        )
            .into_response();
    }
    asset_response("index.html")
}

async fn page_asset(
    Path((id, name)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    if !valid_id(&id) || name == "index.html" || state.page_origin.is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    asset_response(&name)
}

/// Whether this Mac is connected: 200 or 404, readable by the page's origin,
/// so a page served elsewhere can tell an offline Mac from no network.
async fn host_status(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    let status = if valid_id(&id) && state.host(&id).is_some() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    let mut response = status.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    if let Some(origin) = state.page_origin.as_deref().and_then(|o| o.parse().ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response
}

/// Whether a browser socket comes from the page: with a page origin set, its
/// `Origin` must be exactly that, so another site cannot drive a browser's
/// paired keys through the relay. Browsers always send `Origin` on a
/// WebSocket handshake.
fn origin_allowed(page_origin: Option<&str>, headers: &HeaderMap) -> bool {
    let Some(expected) = page_origin else {
        return true;
    };
    headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|origin| origin == expected)
}

async fn browser_ws(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: ws::WebSocketUpgrade,
) -> Response {
    let addr = client_addr(&headers);
    if !state.limits.lock().allow_connect(&addr, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !origin_allowed(state.page_origin.as_deref(), &headers) {
        // Pages opened before the page moved to its own origin still run;
        // 4426 tells them to reload, where a refused handshake reads as no
        // network. The socket closes at once, carrying nothing.
        return upgrade
            .max_frame_size(1024)
            .max_message_size(1024)
            .on_upgrade(|mut socket| async move {
                let _ = tokio::time::timeout(WRITE_TIMEOUT, socket.send(browser_close(4426))).await;
            });
    }
    let Some(host) = state.host(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Before the upgrade, so sockets past the budget never reach
    // `make_room` and push out a browser that is still signing in.
    if !host.allow_join(&addr, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    upgrade
        .max_frame_size(MAX_BROWSER_MESSAGE)
        .max_message_size(MAX_BROWSER_MESSAGE)
        .on_upgrade(move |socket| async move {
            info!(
                "browser joined host {} via edge {} from {}",
                short(&id),
                edge(&headers),
                device(&headers)
            );
            serve_browser(socket, host, addr).await
        })
}

fn browser_close(code: u64) -> ws::Message {
    let (code, reason) = match code {
        4401 => (4401, "pairing link expired or used"),
        4403 => (4403, "not a paired browser"),
        4408 => (4408, "authentication timed out"),
        4409 => (4409, "pairing not confirmed on the Mac"),
        4426 => (4426, "page is out of date"),
        4429 => (4429, "pairing link already in use"),
        _ => (4413, "connection is busy"),
    };
    ws::Message::Close(Some(ws::CloseFrame {
        code,
        reason: reason.into(),
    }))
}

/// What an unadmitted browser has passed on to the Mac
/// (`PENDING_MESSAGES`, `PENDING_BYTES`).
#[derive(Default)]
struct Pending {
    messages: u32,
    bytes: usize,
}

impl Pending {
    /// Count a message of `len` bytes if it fits what is left.
    fn admits(&mut self, len: usize) -> bool {
        if self.messages >= PENDING_MESSAGES || self.bytes + len > PENDING_BYTES {
            return false;
        }
        self.messages += 1;
        self.bytes += len;
        true
    }
}

/// Pass one browser message to its Mac, at no more than `HOST_RATE`: the
/// wait is time this browser is not read. False when the Mac's queue is full,
/// which makes this browser the one flooding it, so it is let go rather than
/// the Mac.
async fn forward(host: &Host, c: u32, payload: axum::body::Bytes) -> bool {
    let wait = host
        .upstream
        .lock()
        .take(payload.len() as f64, Instant::now());
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(KIND_DATA);
    frame.extend_from_slice(&c.to_be_bytes());
    frame.extend_from_slice(&payload);
    host.send(ws::Message::Binary(frame.into()))
}

async fn serve_browser(socket: ws::WebSocket, host: Arc<Host>, addr: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut outgoing) = mpsc::channel::<ws::Message>(QUEUE);
    let queued = Arc::new(Queued::new(BROWSER_QUEUE_BYTES, &host.queued.global));
    let (close, mut closing) = watch::channel(None);
    let admit = Arc::new(tokio::sync::Notify::new());
    let c = host.next_id.fetch_add(1, Ordering::Relaxed);
    // Reserve atomically, after the upgrade: simultaneous handshakes would
    // all have seen space in a check made before it.
    let accepted = {
        let mut browsers = host.browsers.lock();
        let from_addr = browsers.values().filter(|b| b.addr == addr).count();
        if from_addr >= MAX_BROWSERS_PER_ADDR || !make_room(&mut browsers) {
            false
        } else {
            browsers.insert(
                c,
                BrowserLink {
                    data: tx,
                    queued: queued.clone(),
                    close,
                    addr,
                    admitted: false,
                    admit: admit.clone(),
                    sent: 0,
                },
            );
            true
        }
    };
    if !accepted {
        let _ = tokio::time::timeout(WRITE_TIMEOUT, sink.send(browser_close(4413))).await;
        return;
    }
    let open = serde_json::json!({ "t": "open", "c": c }).to_string();
    if !host.send(ws::Message::Text(open.into())) {
        host.browsers.lock().remove(&c);
        return;
    }
    let mut writer = tokio::spawn(async move {
        // Leaving the host's list (rejected key, host replaced) ends the
        // socket with its reason, even in the middle of a stalled write; it
        // never waits for an old screen queue to drain.
        let close_with_reason = |closing: &watch::Receiver<Option<u64>>| {
            closing
                .borrow()
                .map(browser_close)
                .unwrap_or(ws::Message::Close(None))
        };
        loop {
            let msg = tokio::select! {
                biased;
                _ = closing.changed() => {
                    let _ = tokio::time::timeout(WRITE_TIMEOUT, sink.send(close_with_reason(&closing))).await;
                    return;
                }
                msg = outgoing.recv() => match msg { Some(msg) => msg, None => return },
            };
            queued.release(message_len(&msg));
            tokio::select! {
                biased;
                _ = closing.changed() => {
                    let _ = tokio::time::timeout(WRITE_TIMEOUT, sink.send(close_with_reason(&closing))).await;
                    return;
                }
                result = tokio::time::timeout(WRITE_TIMEOUT, sink.send(msg)) => {
                    if !matches!(result, Ok(Ok(()))) { return; }
                }
            }
        }
    });
    let joined = tokio::time::Instant::now();
    // Before admission: what was passed on, and a message read past
    // `PENDING_BYTES`, held until the Mac admits this browser.
    let mut pending = Pending::default();
    let mut held: Option<axum::body::Bytes> = None;
    loop {
        // None once the host let it go: the writer is closing it with its
        // reason, and late input is dropped.
        let admitted = host.browsers.lock().get(&c).map(|b| b.admitted);
        if admitted != Some(false) {
            if let Some(payload) = held.take() {
                if admitted.is_some() && !forward(&host, c, payload).await {
                    break;
                }
            }
        }
        let waiting = admitted == Some(false);
        let reading = !waiting || (held.is_none() && pending.messages < PENDING_MESSAGES);
        tokio::select! {
            _ = &mut writer => break,
            _ = admit.notified(), if !reading => {}
            // Unadmitted, the deadline counts from joining whatever it sends;
            // silence counts only once it is a session.
            _ = tokio::time::sleep_until(joined + PENDING_LIMIT), if waiting => break,
            _ = tokio::time::sleep(BROWSER_SILENCE), if !waiting => break,
            msg = stream.next(), if reading => {
                let Some(Ok(msg)) = msg else { break };
                match msg {
                    ws::Message::Binary(payload) => {
                        let pass = {
                            let mut browsers = host.browsers.lock();
                            match browsers.get_mut(&c) {
                                // The writer owns the close reason; do not abort it for late input.
                                None => continue,
                                Some(link) if link.admitted => true,
                                Some(link) => {
                                    let pass = pending.admits(payload.len());
                                    if pass {
                                        link.sent += 1;
                                    }
                                    pass
                                }
                            }
                        };
                        if !pass {
                            held = Some(payload);
                            continue;
                        }
                        if !forward(&host, c, payload).await { break; }
                    }
                    ws::Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    host.browsers.lock().remove(&c);
    writer.abort();
    let close = serde_json::json!({ "t": "close", "c": c }).to_string();
    let _ = host.send(ws::Message::Text(close.into()));
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
        .route("/{id}", get(short_page_redirect))
        .route("/{id}/", get(page))
        .route("/{id}/ws", get(browser_ws))
        .route("/{id}/status", get(host_status))
        .route("/{id}/assets/{name}", get(page_asset))
        .with_state(state)
}

/// Accept connections and serve `router` on each over HTTP/1.1 with
/// upgrades (the WebSockets). `axum::serve` gives hyper no timer, which
/// leaves its header read timeout off, so a client could hold a connection
/// by trickling its headers in forever.
async fn serve(listener: tokio::net::TcpListener, router: Router, header_timeout: Duration) {
    loop {
        let tcp = match listener.accept().await {
            Ok((tcp, _)) => tcp,
            Err(e) => {
                // Out of file descriptors and the like: let some close.
                if !matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionRefused
                ) {
                    tracing::warn!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                continue;
            }
        };
        // Keystrokes and echoes are tiny frames; Nagle would hold them back.
        if let Err(e) = tcp.set_nodelay(true) {
            tracing::warn!("set_nodelay failed: {e}");
        }
        let service = hyper_util::service::TowerToHyperService::new(router.clone());
        tokio::spawn(async move {
            let mut http = hyper::server::conn::http1::Builder::new();
            http.timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(header_timeout);
            let _ = http
                .serve_connection(hyper_util::rt::TokioIo::new(tcp), service)
                .with_upgrades()
                .await;
        });
    }
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
    let access = AccessCode::from_env().map(Arc::new);
    if access.is_some() {
        info!("Macs need the access code to register");
    }
    let page_origin = std::env::var("KAKU_RELAY_PAGE_ORIGIN")
        .ok()
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty())
        .map(Arc::from);
    if let Some(origin) = &page_origin {
        info!("the page is served from {origin}");
    }
    // Counting is never a reason to stop serving: without a usable file the
    // relay runs as it would with none.
    let usage =
        std::env::var_os("KAKU_RELAY_USAGE_FILE").and_then(|path| {
            match usage::Usage::open(path.into(), &usage::today()) {
                Ok(usage) => Some(Arc::new(Mutex::new(usage))),
                Err(error) => {
                    warn!("usage counts off: {error}");
                    None
                }
            }
        });
    let state = AppState {
        access,
        page_origin,
        usage,
        ..AppState::default()
    };
    let counted = state.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(USAGE_EVERY);
        loop {
            every.tick().await;
            let (macs, browsers, admitted) = counted.usage();
            match counted.note_online(&usage::today()) {
                Some(usage::Summary { today, ever }) => info!(
                    "in use: {macs} Macs, {browsers} browsers ({admitted} paired); \
                     today {} Macs ({} used), ever {} Macs ({} used)",
                    today.macs, today.used, ever.macs, ever.used
                ),
                None => info!("in use: {macs} Macs, {browsers} browsers ({admitted} paired)"),
            }
            counted.save_usage().await;
        }
    });
    let stopping = state.clone();
    tokio::select! {
        _ = serve(listener, app(state), HEADER_READ_TIMEOUT) => {}
        signal = stop_signal() => {
            // Dropping `serve` drops the listener, so no new socket lands on
            // this process while it goes.
            let told = stopping.announce_restart();
            info!("{signal}: told {told} Macs the relay is restarting");
            let deadline = tokio::time::Instant::now() + SHUTDOWN_GRACE;
            while !stopping.hosts.lock().is_empty() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            stopping.save_usage().await;
        }
    }
}

/// SIGINT (Fly's default stop signal) or SIGTERM (Docker's).
async fn stop_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut int), Ok(mut term)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = int.recv() => "SIGINT",
        _ = term.recv() => "SIGTERM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_host_registration_cannot_exceed_the_address_budget() {
        let state = AppState::default();
        std::thread::scope(|scope| {
            let barrier = Arc::new(std::sync::Barrier::new(64));
            for n in 0..64 {
                let state = state.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let mut host = test_host(&state);
                    Arc::get_mut(&mut host).unwrap().addr = "192.0.2.1".into();
                    barrier.wait();
                    let _ = state.register(&format!("host-{n}"), host);
                });
            }
        });
        assert_eq!(state.hosts.lock().len(), MAX_HOSTS_PER_ADDR);
        let id = state.hosts.lock().keys().next().unwrap().clone();
        let mut replacement = test_host(&state);
        Arc::get_mut(&mut replacement).unwrap().addr = "192.0.2.1".into();
        let displaced = state.register(&id, replacement.clone()).unwrap().unwrap();
        assert_eq!(state.hosts.lock().len(), MAX_HOSTS_PER_ADDR);
        assert_eq!(state.unregister(&id, replacement.generation), 1);
        assert_eq!(state.host(&id).unwrap().generation, displaced.generation);
        assert_eq!(state.unregister(&id, displaced.generation), 0);
        let mut new_host = test_host(&state);
        Arc::get_mut(&mut new_host).unwrap().addr = "192.0.2.1".into();
        assert!(state
            .register("after-disconnect", new_host)
            .unwrap()
            .is_none());
        assert_eq!(state.hosts.lock().len(), MAX_HOSTS_PER_ADDR);
    }

    #[test]
    fn global_host_budget_still_allows_replacement_and_standby() {
        let state = AppState::default();
        let first = test_host(&state);
        for n in 0..MAX_HOSTS {
            state
                .hosts
                .lock()
                .insert(format!("host-{n}"), vec![first.clone()]);
        }
        let _ = state.register("extra", test_host(&state));
        assert_eq!(state.hosts.lock().len(), MAX_HOSTS);
        let second = test_host(&state);
        let _ = state.register("host-0", second.clone());
        assert_eq!(state.host("host-0").unwrap().generation, second.generation);
        assert_eq!(state.unregister("host-0", second.generation), 1);
        assert_eq!(state.host("host-0").unwrap().generation, first.generation);
    }

    #[test]
    fn rejection_code_reaches_the_browser_without_arbitrary_reason_text() {
        let ws::Message::Close(Some(frame)) = browser_close(4403) else {
            panic!("close frame")
        };
        assert_eq!(frame.code, 4403);
        assert_eq!(frame.reason.as_str(), "not a paired browser");
        // The page tells an expired pairing link from a removed browser by
        // its code; a code the relay does not pass reads as busy and retries.
        for code in [4401u16, 4403, 4408, 4409, 4426, 4429] {
            let ws::Message::Close(Some(frame)) = browser_close(code.into()) else {
                panic!("close frame")
            };
            assert_eq!(frame.code, code);
        }
        let ws::Message::Close(Some(frame)) = browser_close(u64::MAX) else {
            panic!("close frame")
        };
        assert_eq!(frame.code, 4413);
    }

    #[test]
    fn the_page_is_the_relay_own_and_shows_only_for_a_connected_mac() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let state = AppState::default();
        let id = "0123456789abcdef0123456789abcdef".to_string();
        let page_for = |state: AppState| rt.block_on(page(Path(id.clone()), State(state)));
        // Away: the offline notice, as a 404 the open page can recognize.
        assert_eq!(page_for(state.clone()).status(), StatusCode::NOT_FOUND);
        state.register(&id, test_host(&state)).unwrap();
        let shown = page_for(state.clone());
        assert_eq!(shown.status(), StatusCode::OK);
        assert_eq!(shown.headers()[header::CACHE_CONTROL], "no-cache");
        // Files come from the relay's own copy, with or without the Mac.
        let asset = |name: &str| {
            rt.block_on(page_asset(
                Path((id.clone(), name.to_string())),
                State(state.clone()),
            ))
        };
        assert_eq!(asset("xterm.js").status(), StatusCode::OK);
        assert_eq!(
            asset("font-regular.woff2").headers()[header::CONTENT_TYPE],
            "font/woff2"
        );
        assert_eq!(asset("../Cargo.toml").status(), StatusCode::NOT_FOUND);
        assert_eq!(asset("index.html").status(), StatusCode::NOT_FOUND);
        assert!(page_file("index.html")
            .unwrap()
            .1
            .starts_with(b"<!doctype html>"));
    }

    #[test]
    fn with_a_page_origin_links_move_there_and_only_its_browsers_join() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let state = AppState {
            page_origin: Some(Arc::from("https://t.kaku.fun")),
            ..AppState::default()
        };
        let id = "0123456789abcdef0123456789abcdef".to_string();
        let shown = rt.block_on(page(Path(id.clone()), State(state.clone())));
        assert_eq!(shown.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            shown.headers()[header::LOCATION],
            "https://t.kaku.fun/0123456789abcdef0123456789abcdef/"
        );
        let asset = rt.block_on(page_asset(
            Path((id.clone(), "xterm.js".into())),
            State(state.clone()),
        ));
        assert_eq!(asset.status(), StatusCode::NOT_FOUND);

        let from = |origin: Option<&str>| {
            let mut headers = HeaderMap::new();
            if let Some(origin) = origin {
                headers.insert(header::ORIGIN, origin.parse().unwrap());
            }
            origin_allowed(state.page_origin.as_deref(), &headers)
        };
        assert!(from(Some("https://t.kaku.fun")));
        assert!(!from(Some("https://evil.example")));
        assert!(!from(Some("https://r.kaku.fun")));
        assert!(!from(None));
        // Without a page origin (local runs) any browser joins, as before.
        assert!(origin_allowed(None, &HeaderMap::new()));

        // The page asks whether the Mac is here, across origins.
        let status =
            |state: &AppState| rt.block_on(host_status(Path(id.clone()), State(state.clone())));
        let away = status(&state);
        assert_eq!(away.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            away.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://t.kaku.fun"
        );
        state.register(&id, test_host(&state)).unwrap();
        assert_eq!(status(&state).status(), StatusCode::OK);
    }

    fn test_host(state: &AppState) -> Arc<Host> {
        let (to_host, rx) = mpsc::channel(QUEUE);
        // Keep the receiver alive so sends are judged by the budget alone.
        std::mem::forget(rx);
        Arc::new(Host::new(
            state.generation.fetch_add(1, Ordering::Relaxed),
            to_host,
            "-".into(),
            &state.queued_bytes,
        ))
    }

    #[test]
    fn a_third_registration_pushes_out_the_oldest() {
        let state = AppState::default();
        let hosts: Vec<_> = (0..3).map(|_| test_host(&state)).collect();
        for h in &hosts {
            state.register("id", h.clone()).unwrap();
        }
        let kept: Vec<u64> = state.hosts.lock()["id"]
            .iter()
            .map(|h| h.generation)
            .collect();
        assert_eq!(kept, vec![hosts[1].generation, hosts[2].generation]);
        // The pushed-out one was told to end.
        let notified = hosts[0].evicted.notified();
        tokio::pin!(notified);
        assert!(futures_util::FutureExt::now_or_never(notified.as_mut()).is_some());
    }

    #[test]
    fn a_host_queue_is_bounded_by_bytes() {
        let state = AppState::default();
        let host = test_host(&state);
        let big = || ws::Message::Binary(vec![0u8; 3 << 20].into());
        assert!(host.send(big()));
        assert!(host.send(big()));
        // A third 3 MB message would pass 8 MB queued.
        assert!(!host.send(big()));
        assert_eq!(host.queued.load(), 6 << 20);
        assert!(host.send(ws::Message::Text("small".into())));
    }

    fn test_browser(global: &Arc<AtomicUsize>) -> BrowserLink {
        let (data, rx) = mpsc::channel(QUEUE);
        std::mem::forget(rx);
        BrowserLink {
            data,
            queued: Arc::new(Queued::new(BROWSER_QUEUE_BYTES, global)),
            close: watch::channel(None).0,
            addr: String::new(),
            admitted: false,
            admit: Default::default(),
            sent: 0,
        }
    }

    #[test]
    fn usage_counts_macs_once_and_paired_browsers_apart() {
        let state = AppState::default();
        // A second Kaku under the same identity is still one Mac.
        for _ in 0..2 {
            state.register("a", test_host(&state)).unwrap();
        }
        state.register("b", test_host(&state)).unwrap();
        let a = state.host("a").unwrap();
        let mut paired = test_browser(&state.queued_bytes);
        paired.admitted = true;
        a.browsers.lock().insert(1, paired);
        a.browsers
            .lock()
            .insert(2, test_browser(&state.queued_bytes));
        assert_eq!(state.usage(), (2, 2, 1));
    }

    #[test]
    fn a_mac_connected_across_midnight_counts_for_the_new_day() {
        let dir = std::env::temp_dir().join(format!("kaku-relay-online-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState {
            usage: Some(Arc::new(Mutex::new(
                usage::Usage::open(dir.join("usage.json"), "2026-11-09").unwrap(),
            ))),
            ..AppState::default()
        };
        state.register("a", test_host(&state)).unwrap();
        state.register("b", test_host(&state)).unwrap();
        let mut paired = test_browser(&state.queued_bytes);
        paired.admitted = true;
        state.host("a").unwrap().browsers.lock().insert(1, paired);
        state
            .host("b")
            .unwrap()
            .browsers
            .lock()
            .insert(1, test_browser(&state.queued_bytes));

        // Neither Mac registers again; only the round counts them.
        for day in ["2026-11-09", "2026-11-10"] {
            let seen = state.note_online(day).unwrap();
            let both = usage::Counts { macs: 2, used: 1 };
            assert_eq!(seen.today, both, "{day}");
            assert_eq!(seen.ever, both, "{day}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_browser_queue_is_bounded_by_bytes() {
        let state = AppState::default();
        let browser = test_browser(&state.queued_bytes);
        // Screens from the Mac can be 4 MB each; a page that stopped reading
        // must not hold more than its budget however few messages that is.
        let screen = || ws::Message::Binary(vec![0u8; 3 << 20].into());
        assert!(browser.send(screen()));
        assert!(browser.send(screen()));
        assert!(!browser.send(screen()));
        assert_eq!(browser.queued.load(), 6 << 20);
        assert_eq!(state.queued_bytes.load(Ordering::Relaxed), 6 << 20);
    }

    #[test]
    fn the_relay_wide_budget_bounds_every_queue_together() {
        let state = AppState::default();
        let browsers: Vec<_> = (0..20).map(|_| test_browser(&state.queued_bytes)).collect();
        let chunk = || ws::Message::Binary(vec![0u8; 6 << 20].into());
        // Each stays within its own 8 MB, but together they stop at the
        // relay-wide budget.
        let accepted = browsers.iter().filter(|b| b.send(chunk())).count();
        assert_eq!(accepted, GLOBAL_QUEUE_BYTES / (6 << 20));
        assert!(state.queued_bytes.load(Ordering::Relaxed) <= GLOBAL_QUEUE_BYTES);
    }

    #[test]
    fn a_closed_socket_gives_its_unsent_bytes_back() {
        let state = AppState::default();
        let browser = test_browser(&state.queued_bytes);
        let host = test_host(&state);
        assert!(browser.send(ws::Message::Binary(vec![0u8; 1 << 20].into())));
        assert!(host.send(ws::Message::Binary(vec![0u8; 1 << 20].into())));
        assert_eq!(state.queued_bytes.load(Ordering::Relaxed), 2 << 20);
        // Sockets that end with messages still queued must not leak budget,
        // or the relay slowly stops accepting anything.
        drop(browser);
        drop(host);
        assert_eq!(state.queued_bytes.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn an_earlier_host_takes_over_when_the_newer_one_leaves() {
        let state = AppState::default();
        let first = test_host(&state);
        let second = test_host(&state);
        assert!(state.register("id", first.clone()).unwrap().is_none());
        let displaced = state
            .register("id", second.clone())
            .unwrap()
            .expect("first displaced");
        assert_eq!(displaced.generation, first.generation);
        assert_eq!(state.host("id").unwrap().generation, second.generation);

        // The newer Kaku quits: the first, still connected, serves again.
        assert_eq!(state.unregister("id", second.generation), 1);
        assert_eq!(state.host("id").unwrap().generation, first.generation);

        assert_eq!(state.unregister("id", first.generation), 0);
        assert!(state.host("id").is_none());
    }

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

    /// One IPv6 client holds a whole /64 and can use a new source address
    /// per request, which made every per-address budget per-request.
    #[test]
    fn ipv6_clients_are_held_to_budgets_per_slash_64() {
        assert_eq!(
            source_of("2001:db8:1:2:aaaa::1"),
            source_of("2001:db8:1:2:bbbb:cccc:dddd:9")
        );
        assert_ne!(source_of("2001:db8:1:2::1"), source_of("2001:db8:1:3::1"));
        assert_eq!(source_of("::ffff:1.2.3.4"), "1.2.3.4");
        assert_eq!(source_of("1.2.3.4"), "1.2.3.4");
        assert_eq!(source_of("-"), "-");
        let mut limits = AddrLimits::default();
        let start = Instant::now();
        for i in 0..CONNECTS_PER_MINUTE {
            let addr = source_of(&format!("2001:db8:1:2::{i:x}"));
            assert!(limits.allow_connect(&addr, start));
        }
        assert!(!limits.allow_connect(&source_of("2001:db8:1:2:ffff::1"), start));
        assert!(limits.allow_connect(&source_of("2001:db8:1:3::1"), start));
    }

    #[test]
    fn the_access_code_matches_whole_and_trimmed() {
        let code = AccessCode::new("open sesame");
        assert!(code.admits("open sesame"));
        assert!(code.admits(" open sesame\n"));
        assert!(!code.admits("open sesam"));
        assert!(!code.admits("open sesame2"));
        assert!(!code.admits(""));
    }

    #[test]
    fn an_address_is_held_to_its_budgets_and_freed_after_the_window() {
        let mut limits = AddrLimits::default();
        let start = Instant::now();
        for _ in 0..CONNECTS_PER_MINUTE {
            assert!(limits.allow_connect("1.2.3.4", start));
        }
        assert!(!limits.allow_connect("1.2.3.4", start));
        // Another address and local runs are not affected.
        assert!(limits.allow_connect("5.6.7.8", start));
        for _ in 0..1000 {
            assert!(limits.allow_connect("-", start));
        }
        assert!(limits.allow_connect("1.2.3.4", start + Duration::from_secs(61)));

        for _ in 0..ACCESS_FAILURES - 1 {
            limits.note_access_failure("1.2.3.4", start);
        }
        assert!(!limits.access_locked("1.2.3.4", start));
        limits.note_access_failure("1.2.3.4", start);
        assert!(limits.access_locked("1.2.3.4", start));
        assert!(!limits.access_locked("5.6.7.8", start));
        assert!(!limits.access_locked("1.2.3.4", start + ACCESS_WINDOW));
    }

    #[test]
    fn sockets_the_mac_never_admitted_cannot_hold_every_slot() {
        let state = AppState::default();
        let host = test_host(&state);
        let link = |admitted: bool| {
            let (data, rx) = mpsc::channel(QUEUE);
            std::mem::forget(rx);
            let (close, closing) = watch::channel(None);
            (
                BrowserLink {
                    data,
                    queued: Arc::new(Queued::new(BROWSER_QUEUE_BYTES, &host.queued.global)),
                    close,
                    addr: "-".into(),
                    admitted,
                    admit: Default::default(),
                    sent: 0,
                },
                closing,
            )
        };
        let mut browsers = HashMap::new();
        let (owner, _) = link(true);
        browsers.insert(1, owner);
        let mut squatters = Vec::new();
        for c in 2..=MAX_BROWSERS_PER_HOST as u32 {
            let (squatter, closing) = link(false);
            browsers.insert(c, squatter);
            squatters.push(closing);
        }
        // Full: the next browser pushes out the oldest unadmitted socket,
        // never the admitted one.
        assert!(make_room(&mut browsers));
        assert!(browsers.contains_key(&1) && !browsers.contains_key(&2));
        assert_eq!(*squatters[0].borrow(), Some(4413));
        // Every slot admitted: no room.
        for b in browsers.values_mut() {
            b.admitted = true;
        }
        let (extra, _) = link(true);
        browsers.insert(99, extra);
        assert!(!make_room(&mut browsers));
    }

    #[test]
    fn page_addresses_redirect_only_for_a_real_id() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // `/%2Fevil.example` reaches the handler decoded.
        let evil = rt.block_on(short_page_redirect(Path("/evil.example".into())));
        assert_eq!(evil.status(), StatusCode::NOT_FOUND);
        let evil = rt.block_on(page_redirect(Path("/evil.example".into())));
        assert_eq!(evil.status(), StatusCode::NOT_FOUND);
        let id = "0123456789abcdef0123456789abcdef".to_string();
        let ok = rt.block_on(short_page_redirect(Path(id.clone())));
        assert_eq!(ok.headers()[header::LOCATION], format!("/{id}/"));
    }

    #[test]
    fn ids_must_be_128_bit_hex() {
        assert!(!valid_id("short"));
        assert!(!valid_id(&"g".repeat(32)));
        assert!(valid_id(&"0f".repeat(16)));
    }

    #[test]
    fn a_token_bucket_paces_into_debt_and_refills_to_its_burst() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(100.0, 100.0, 1000.0, start);
        assert_eq!(bucket.take(100.0, start), Duration::ZERO);
        // In debt: wait until it is paid back at the rate.
        assert_eq!(bucket.take(50.0, start), Duration::from_millis(500));
        // Refills, but never past the burst.
        let later = start + Duration::from_secs(100);
        assert!(bucket.try_take(1000.0, later));
        assert!(!bucket.try_take(1.0, later));
    }

    /// A browser that came the furthest through the handshake is a real one
    /// (the Mac closes a socket whose hello or proof is wrong), perhaps
    /// waiting for its pairing to be confirmed; sockets from someone who only
    /// knows the id must not push it out however fast they come.
    #[test]
    fn a_churning_socket_cannot_push_out_a_browser_that_has_proved_itself() {
        let state = AppState::default();
        let link = |sent: u32| BrowserLink {
            sent,
            ..test_browser(&state.queued_bytes)
        };
        let mut browsers = HashMap::new();
        browsers.insert(1, link(2));
        for c in 2..=MAX_BROWSERS_PER_HOST as u32 {
            browsers.insert(c, link(c % 2));
        }
        for c in 100..200 {
            assert!(make_room(&mut browsers));
            assert!(browsers.contains_key(&1), "pushed out by socket {c}");
            browsers.insert(c, link(1));
        }
        // When every slot is at the same stage, the oldest goes first.
        browsers.get_mut(&1).unwrap().sent = 1;
        assert!(make_room(&mut browsers));
        assert!(!browsers.contains_key(&1));
    }

    #[test]
    fn new_browser_sockets_per_host_are_limited_but_known_addresses_join() {
        let state = AppState::default();
        let host = test_host(&state);
        let now = Instant::now();
        for _ in 0..JOIN_BURST as u32 {
            assert!(host.allow_join("198.51.100.7", now));
        }
        assert!(!host.allow_join("198.51.100.8", now));
        // An address one of its browsers was admitted from always joins.
        host.trust("203.0.113.9");
        assert!(host.allow_join("203.0.113.9", now));
        assert!(host.allow_join("-", now));
        // Then one every 1 / JOIN_RATE seconds.
        let later = now + Duration::from_secs_f64(1.0 / JOIN_RATE);
        assert!(host.allow_join("198.51.100.8", later));
        assert!(!host.allow_join("198.51.100.8", later));
    }

    #[test]
    fn wrong_code_entries_are_swept_by_connects_alone() {
        let mut limits = AddrLimits::default();
        let start = Instant::now();
        for n in 0..10_001u32 {
            let addr = format!("10.{}.{}.{}", n >> 16, (n >> 8) & 255, n & 255);
            limits.note_access_failure(&addr, start);
        }
        let later = start + ACCESS_WINDOW + Duration::from_secs(1);
        // No wrong code comes in again, only connects.
        assert!(limits.allow_connect("192.0.2.1", later));
        assert!(limits.failures.map.is_empty());
    }

    // ── Over real sockets ─────────────────────────────────────────────────────

    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as T;
    type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn start_relay(state: AppState, header_timeout: Duration) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, app(state), header_timeout));
        addr
    }

    /// A Mac registered under a fresh id and a browser joined to it: both
    /// sockets and the browser's number.
    async fn mac_and_browser(state: &AppState, relay: std::net::SocketAddr) -> (Ws, Ws, u32) {
        let secret = "k".repeat(64);
        let digest = Sha256::digest(secret.as_bytes());
        let id: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        let mut request = format!("ws://{relay}/host/{id}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("x-kaku-secret", secret.parse().unwrap());
        let (mut mac, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        while state.host(&id).is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let (browser, _) = tokio_tungstenite::connect_async(format!("ws://{relay}/{id}/ws"))
            .await
            .unwrap();
        let Some(T::Text(open)) = next(&mut mac, Duration::from_secs(5)).await else {
            panic!("the Mac hears of the browser")
        };
        let open: serde_json::Value = serde_json::from_str(open.as_str()).unwrap();
        (mac, browser, open["c"].as_u64().unwrap() as u32)
    }

    async fn next(ws: &mut Ws, wait: Duration) -> Option<T> {
        loop {
            match tokio::time::timeout(wait, ws.next()).await {
                Ok(Some(Ok(T::Ping(_) | T::Pong(_)))) => continue,
                Ok(Some(Ok(msg))) => return Some(msg),
                _ => return None,
            }
        }
    }

    /// Data messages that reach the Mac until it hears nothing for `wait`.
    async fn data_at_mac(mac: &mut Ws, wait: Duration) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        while let Some(msg) = next(mac, wait).await {
            if let T::Binary(bytes) = msg {
                assert_eq!(bytes[0], KIND_DATA);
                got.push(bytes[5..].to_vec());
            }
        }
        got
    }

    async fn admit(mac: &mut Ws, c: u32) {
        let ok = serde_json::json!({ "t": "ok", "c": c }).to_string();
        mac.send(T::Text(ok.into())).await.unwrap();
    }

    /// Anyone who knows a Mac's id can open its page's socket; until the Mac
    /// admits it, only what the handshake needs reaches the Mac, and the rest
    /// waits, in order, rather than being lost.
    #[tokio::test]
    async fn an_unadmitted_browser_passes_on_only_its_handshake_budget() {
        let state = AppState::default();
        let relay = start_relay(state.clone(), HEADER_READ_TIMEOUT).await;
        let (mut mac, mut browser, c) = mac_and_browser(&state, relay).await;
        let total = PENDING_MESSAGES as usize + 4;
        for n in 0..total {
            browser
                .send(T::Binary(vec![n as u8; 100].into()))
                .await
                .unwrap();
        }
        let before = data_at_mac(&mut mac, Duration::from_millis(300)).await;
        assert_eq!(before.len(), PENDING_MESSAGES as usize);
        admit(&mut mac, c).await;
        let after = data_at_mac(&mut mac, Duration::from_millis(300)).await;
        let all: Vec<u8> = before.iter().chain(&after).map(|m| m[0]).collect();
        assert_eq!(all, (0..total as u8).collect::<Vec<_>>());
    }

    /// A stopping relay closes each Mac's link with 1012, which is what
    /// tells the Mac to retry every second rather than back off.
    #[tokio::test]
    async fn a_restart_closes_every_mac_with_1012() {
        let state = AppState::default();
        let relay = start_relay(state.clone(), HEADER_READ_TIMEOUT).await;
        let (mut mac, _browser, _) = mac_and_browser(&state, relay).await;
        assert_eq!(state.announce_restart(), 1);
        let Some(T::Close(Some(frame))) = next(&mut mac, Duration::from_secs(5)).await else {
            panic!("the Mac gets a close frame")
        };
        assert_eq!(u16::from(frame.code), 1012);
    }

    #[tokio::test]
    async fn a_message_past_the_pending_bytes_waits_for_admission() {
        let state = AppState::default();
        let relay = start_relay(state.clone(), HEADER_READ_TIMEOUT).await;
        let (mut mac, mut browser, c) = mac_and_browser(&state, relay).await;
        browser
            .send(T::Binary(vec![7u8; PENDING_BYTES + 1].into()))
            .await
            .unwrap();
        assert!(data_at_mac(&mut mac, Duration::from_millis(300))
            .await
            .is_empty());
        admit(&mut mac, c).await;
        let after = data_at_mac(&mut mac, Duration::from_millis(300)).await;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].len(), PENDING_BYTES + 1);
    }

    /// A Mac and its own browser cannot use the relay as an unmetered pipe:
    /// past what a link starts with, each direction moves at `HOST_RATE`, by
    /// reading the sender more slowly rather than dropping it.
    #[tokio::test]
    async fn a_host_link_is_paced_from_the_mac() {
        let state = AppState::default();
        let relay = start_relay(state.clone(), HEADER_READ_TIMEOUT).await;
        let (mut mac, mut browser, c) = mac_and_browser(&state, relay).await;
        admit(&mut mac, c).await;
        // HOST_RATE bytes past the start: about a second.
        let total = (HOST_RATE_START + HOST_RATE) as usize;
        let chunk = 1 << 20;
        let started = tokio::time::Instant::now();
        for _ in 0..total.div_ceil(chunk) {
            let mut frame = vec![KIND_DATA];
            frame.extend_from_slice(&c.to_be_bytes());
            frame.resize(5 + chunk, 0);
            mac.send(T::Binary(frame.into())).await.unwrap();
        }
        for _ in 0..total.div_ceil(chunk) {
            let got = next(&mut browser, Duration::from_secs(10)).await;
            assert!(matches!(got, Some(T::Binary(_))));
        }
        let down = started.elapsed();
        assert!(
            down >= Duration::from_millis(800),
            "Mac to browser: {down:?}"
        );
    }

    #[tokio::test]
    async fn a_host_link_is_paced_from_its_browsers() {
        let state = AppState::default();
        let relay = start_relay(state.clone(), HEADER_READ_TIMEOUT).await;
        let (mut mac, mut browser, c) = mac_and_browser(&state, relay).await;
        admit(&mut mac, c).await;
        let total = (HOST_RATE_START + HOST_RATE) as usize;
        let piece = 500_000;
        let started = tokio::time::Instant::now();
        for _ in 0..total.div_ceil(piece) {
            browser
                .send(T::Binary(vec![0u8; piece].into()))
                .await
                .unwrap();
        }
        for _ in 0..total.div_ceil(piece) {
            let got = next(&mut mac, Duration::from_secs(10)).await;
            assert!(matches!(got, Some(T::Binary(_))));
        }
        let up = started.elapsed();
        assert!(up >= Duration::from_millis(800), "browser to Mac: {up:?}");
    }

    #[tokio::test]
    async fn a_request_whose_headers_never_finish_is_cut_off() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let relay = start_relay(AppState::default(), Duration::from_millis(300)).await;
        let mut tcp = tokio::net::TcpStream::connect(relay).await.unwrap();
        tcp.write_all(b"GET /health HTTP/1.1\r\nHost: relay\r\n")
            .await
            .unwrap();
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(3), tcp.read_to_end(&mut rest)).await;
        assert!(closed.is_ok(), "the connection stays open");
        // A whole request is still answered.
        let mut tcp = tokio::net::TcpStream::connect(relay).await.unwrap();
        tcp.write_all(b"GET /health HTTP/1.1\r\nHost: relay\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut answer = String::new();
        tcp.read_to_string(&mut answer).await.unwrap();
        assert!(
            answer.starts_with("HTTP/1.1 200") && answer.ends_with("ok"),
            "{answer}"
        );
    }
}
