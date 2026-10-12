use crate::termwindow::TermWindowNotif;
use crate::{frontend, TermWindow};
use anyhow::{anyhow, Context};
use config::GuiPosition;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, PaneNode, SerdeUrl, SplitDirectionAndSize, Tab, TabId};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use parking_lot::Mutex;
use promise::spawn::spawn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wezterm_term::TerminalSize;
use wezterm_toast_notification::persistent_toast_notification;
use window::WindowOps;

const SNAPSHOT_VERSION: u32 = 4;

/// Bincode-format version for per-pane scrollback sidecars. Bumped
/// independently of `SNAPSHOT_VERSION` so content changes do not force the
/// envelope to invalidate older snapshots.
const CONTENT_SCHEMA_VERSION: u32 = 1;

/// Sentinel constant for the `Line`/`Cell` serde layout from wezterm-surface.
/// Bump manually when pulling an upstream wezterm change that touches the
/// serialized `Line` shape; old sidecars are then silently skipped instead
/// of producing garbage cells.
const WEZTERM_SURFACE_FINGERPRINT: &str = "wezterm-surface-2026-05";

/// Per-pane scrollback cap. 1500 lines is enough to cover a normal `git
/// log -n 500`, a `cargo build` log, or a verbose CI dump while keeping the
/// bincode sidecars under a few MB each.
const PANE_CONTENT_CAP_LINES: usize = 1500;

/// Hard byte ceiling for a single pane's serialized scrollback sidecar.
/// Image attachments are stripped before encoding (see `write_pane_sidecar`),
/// so this only guards unusually wide or attribute-heavy text. When the
/// encoded sidecar exceeds this ceiling we skip persisting that pane's
/// content (structural snapshot is unaffected) rather than write a giant
/// file. 4 MiB leaves generous headroom for the text cap above.
const PANE_CONTENT_MAX_BYTES: usize = 4 * 1024 * 1024;

// Envelope written on app quit: continue-where-you-left-off.
#[derive(Debug, Serialize, Deserialize)]
struct SavedSession {
    version: u32,
    /// Relative directory name under `session_content/` that holds this
    /// snapshot's per-pane scrollback sidecars. Empty string when the
    /// snapshot has no content sidecars (e.g. every pane was trivial).
    #[serde(default)]
    content_dir: String,
    windows: Vec<SavedWindowSnapshot>,
}

// Envelope written when the user closes a single window: undo-close-window.
#[derive(Debug, Serialize, Deserialize)]
struct SavedClosedWindow {
    version: u32,
    /// See `SavedSession::content_dir`.
    #[serde(default)]
    content_dir: String,
    window: SavedWindowSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindowSnapshot {
    active_tab_idx: usize,
    window_title: String,
    #[serde(default)]
    is_focused: bool,
    tabs: Vec<SavedTabSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedTabSnapshot {
    title: String,
    pane_tree: SavedPaneNode,
}

#[derive(Debug, Serialize, Deserialize)]
enum SavedPaneNode {
    Empty,
    Split {
        left: Box<SavedPaneNode>,
        right: Box<SavedPaneNode>,
        node: SplitDirectionAndSize,
    },
    Leaf(SavedPaneEntry),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SavedPaneEntry {
    window_id: MuxWindowId,
    tab_id: TabId,
    pane_id: PaneId,
    title: String,
    size: TerminalSize,
    working_dir: Option<SerdeUrl>,
    domain_name: String,
    is_active_pane: bool,
    is_zoomed_pane: bool,
    workspace: String,
    /// When present, points at a bincode sidecar in this snapshot's
    /// content directory holding the pane's scrollback at save time.
    /// Absent for trivial panes or when scrollback capture failed.
    #[serde(default)]
    content_ref: Option<PaneContentRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PaneContentRef {
    /// Filename under the snapshot's content directory; conventionally
    /// `pane_<pane_id>.bin`.
    filename: String,
    /// Mirrors `CONTENT_SCHEMA_VERSION` at save time.
    schema: u32,
    /// Terminal size at save time. Used to decide whether reflow is
    /// needed when restoring into a different geometry.
    saved_cols: usize,
    saved_rows: usize,
    /// Convenience: how many lines the sidecar contains. Lets the GC and
    /// telemetry skip opening the file.
    line_count: usize,
}

/// Bincode payload written to each pane sidecar.
#[derive(Serialize, Deserialize)]
struct PaneContentPayload {
    schema: u32,
    fingerprint: String,
    cols: usize,
    rows: usize,
    lines: Vec<wezterm_term::Line>,
}

impl SavedPaneNode {
    fn from_live(
        node: PaneNode,
        mux: &Mux,
        content_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<Self> {
        match node {
            PaneNode::Empty => Ok(Self::Empty),
            PaneNode::Split { left, right, node } => Ok(Self::collapse_split(
                Self::from_live(*left, mux, content_dir)?,
                Self::from_live(*right, mux, content_dir)?,
                node,
            )),
            PaneNode::Leaf(entry) => {
                let pane_id = entry.pane_id;
                // A pane can sit in the tab tree while already gone from the
                // mux (just closed and not pruned yet, or a detached domain).
                // Failing the whole tree there would silently drop every other
                // tab in this window from the snapshot, so drop just this leaf.
                match SavedPaneEntry::from_live(entry, mux, content_dir) {
                    Ok(entry) => Ok(Self::Leaf(entry)),
                    Err(err) => {
                        log::warn!("session snapshot skips pane {pane_id}: {err:#}");
                        Ok(Self::Empty)
                    }
                }
            }
        }
    }

    /// Rebuild a split from its saved halves, collapsing onto whichever half
    /// survived when the other could not be captured.
    fn collapse_split(left: Self, right: Self, node: SplitDirectionAndSize) -> Self {
        match (left, right) {
            (Self::Empty, right) => right,
            (left, Self::Empty) => left,
            (left, right) => Self::Split {
                left: Box::new(left),
                right: Box::new(right),
                node,
            },
        }
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    fn root_size(&self) -> Option<TerminalSize> {
        match self {
            Self::Empty => None,
            Self::Split { node, .. } => Some(node.size()),
            Self::Leaf(entry) => Some(entry.size),
        }
    }

    fn into_pane_node(self) -> PaneNode {
        match self {
            Self::Empty => PaneNode::Empty,
            Self::Split { left, right, node } => PaneNode::Split {
                left: Box::new(left.into_pane_node()),
                right: Box::new(right.into_pane_node()),
                node,
            },
            Self::Leaf(entry) => PaneNode::Leaf(entry.into_pane_entry()),
        }
    }
}

impl SavedPaneEntry {
    fn from_live(
        entry: PaneEntry,
        mux: &Mux,
        content_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<Self> {
        let pane = mux
            .get_pane(entry.pane_id)
            .ok_or_else(|| anyhow!("pane {} not found while building snapshot", entry.pane_id))?;
        let domain = mux.get_domain(pane.domain_id()).ok_or_else(|| {
            anyhow!(
                "domain {} not found while building snapshot for pane {}",
                pane.domain_id(),
                entry.pane_id
            )
        })?;

        // Best-effort: capture and write the scrollback sidecar. Any
        // failure (capture returns None, write fails) drops back to
        // `content_ref: None` so the structural snapshot still saves.
        let content_ref = content_dir.and_then(|dir| {
            let (lines, cols, rows) = capture_pane_content(&pane, PANE_CONTENT_CAP_LINES)?;
            write_pane_sidecar(dir, entry.pane_id, lines, cols, rows)
        });

        Ok(Self {
            window_id: entry.window_id,
            tab_id: entry.tab_id,
            pane_id: entry.pane_id,
            title: entry.title,
            size: entry.size,
            working_dir: entry.working_dir,
            domain_name: domain.domain_name().to_string(),
            is_active_pane: entry.is_active_pane,
            is_zoomed_pane: entry.is_zoomed_pane,
            workspace: entry.workspace,
            content_ref,
        })
    }

    fn into_pane_entry(self) -> PaneEntry {
        PaneEntry {
            window_id: self.window_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            title: self.title,
            size: self.size,
            working_dir: self.working_dir,
            is_active_pane: self.is_active_pane,
            is_zoomed_pane: self.is_zoomed_pane,
            workspace: self.workspace,
            cursor_pos: Default::default(),
            physical_top: 0,
            top_row: 0,
            left_col: 0,
            tty_name: None,
        }
    }
}

fn config_dir_file(name: &str) -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(name)
}

fn session_file() -> PathBuf {
    config_dir_file("last_session.json")
}

fn closed_window_file() -> PathBuf {
    config_dir_file("last_closed_window.json")
}

fn content_root_dir() -> PathBuf {
    config_dir_file("session_content")
}

/// Build a per-snapshot content directory name. We avoid pulling in `uuid`
/// here because the existing atomic-write helper already uses a
/// pid-plus-nanos pattern for the same uniqueness purpose.
fn new_content_dir_name(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{pid}-{nanos}", pid = std::process::id())
}

/// Capture up to `cap` of the pane's most recent lines (scrollback +
/// viewport, ordered oldest first). Returns the lines plus the saved
/// `(cols, rows)` geometry. Returns `None` for trivial empty captures so
/// the caller can skip the sidecar write entirely.
fn capture_pane_content(
    pane: &Arc<dyn Pane>,
    cap: usize,
) -> Option<(Vec<wezterm_term::Line>, usize, usize)> {
    if cap == 0 {
        return None;
    }
    if pane.is_alt_screen_active() {
        return None;
    }
    let dims = pane.get_dimensions();
    let end: wezterm_term::StableRowIndex = dims.physical_top + dims.viewport_rows as isize;
    let start = (end - cap as isize).max(dims.scrollback_top);
    if start >= end {
        return None;
    }
    let (_top, lines) = pane.get_lines(start..end);
    if lines.is_empty() {
        return None;
    }
    Some((lines, dims.cols, dims.viewport_rows))
}

/// Atomic write for bincode payloads, mirroring `write_json_atomic`.
///
/// `max_bytes` is an optional ceiling on the encoded size. When the
/// serialized payload exceeds it the write is refused with an error before
/// touching disk, so the caller can degrade gracefully instead of
/// persisting an oversized file.
fn write_bincode_atomic<T: Serialize>(
    file_name: &std::path::Path,
    value: &T,
    max_bytes: Option<usize>,
) -> anyhow::Result<()> {
    if let Some(parent) = file_name.parent() {
        config::create_user_owned_dirs(parent)
            .with_context(|| format!("create content dir {}", parent.display()))?;
    }
    let encoded = bincode::serialize(value).context("encode pane content")?;
    if let Some(max) = max_bytes {
        if encoded.len() > max {
            return Err(anyhow!(
                "encoded payload is {} bytes, exceeding the {} byte ceiling",
                encoded.len(),
                max
            ));
        }
    }
    let tmp = file_name.with_file_name(format!(
        "{}.{}.{}.tmp",
        file_name.file_stem().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &encoded).with_context(|| format!("write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &file_name) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("rename {} -> {}", tmp.display(), file_name.display()));
    }
    Ok(())
}

/// Write a captured pane's content to its sidecar. On success returns the
/// `PaneContentRef` that should be embedded in the `SavedPaneEntry`. On
/// failure logs and returns `None`; the structural snapshot is unaffected.
fn write_pane_sidecar(
    content_dir: &std::path::Path,
    pane_id: PaneId,
    mut lines: Vec<wezterm_term::Line>,
    cols: usize,
    rows: usize,
) -> Option<PaneContentRef> {
    // Every cell an image covers holds its own Arc to the same ImageData, and
    // serde writes the whole blob once per cell, so one inline image can encode
    // to gigabytes on the GUI thread (#559). Keep the text, drop the images.
    for line in &mut lines {
        for cell in line.cells_mut_for_attr_changes_only() {
            if cell.attrs().images().is_some() {
                cell.attrs_mut().clear_images();
            }
        }
    }
    let filename = format!("pane_{pane_id}.bin");
    let path = content_dir.join(&filename);
    let line_count = lines.len();
    let payload = PaneContentPayload {
        schema: CONTENT_SCHEMA_VERSION,
        fingerprint: WEZTERM_SURFACE_FINGERPRINT.to_string(),
        cols,
        rows,
        lines,
    };
    match write_bincode_atomic(&path, &payload, Some(PANE_CONTENT_MAX_BYTES)) {
        Ok(()) => Some(PaneContentRef {
            filename,
            schema: CONTENT_SCHEMA_VERSION,
            saved_cols: cols,
            saved_rows: rows,
            line_count,
        }),
        Err(e) => {
            log::warn!(
                "skip scrollback sidecar for pane {pane_id}: {e:#} ({})",
                path.display()
            );
            None
        }
    }
}

/// Load a pane sidecar referenced by `content_ref`. Returns `None` (with a
/// warning) on any failure so the restore path can degrade to an empty
/// scrollback.
fn load_pane_payload(
    content_dir: &std::path::Path,
    content_ref: &PaneContentRef,
) -> Option<PaneContentPayload> {
    let path = content_dir.join(&content_ref.filename);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            log::debug!("scrollback sidecar missing at {}: {e:#}", path.display());
            return None;
        }
    };
    let payload: PaneContentPayload = match bincode::deserialize(&bytes) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "scrollback sidecar at {} failed to parse: {e:#}",
                path.display()
            );
            return None;
        }
    };
    if payload.schema != CONTENT_SCHEMA_VERSION {
        log::warn!(
            "scrollback sidecar at {} has schema {} (expected {}); skipping",
            path.display(),
            payload.schema,
            CONTENT_SCHEMA_VERSION,
        );
        return None;
    }
    if payload.fingerprint != WEZTERM_SURFACE_FINGERPRINT {
        log::warn!(
            "scrollback sidecar at {} has fingerprint {:?} (expected {:?}); \
             upstream wezterm-surface layout changed, skipping",
            path.display(),
            payload.fingerprint,
            WEZTERM_SURFACE_FINGERPRINT,
        );
        return None;
    }
    Some(payload)
}

/// Reflow saved lines to fit a new column width via `Line::wrap`. Trims
/// from the oldest end so the resulting block does not exceed
/// `PANE_CONTENT_CAP_LINES`.
fn reflow_lines(lines: Vec<wezterm_term::Line>, target_cols: usize) -> Vec<wezterm_term::Line> {
    if target_cols == 0 {
        return Vec::new();
    }
    let mut out: Vec<wezterm_term::Line> = Vec::with_capacity(lines.len());
    for line in lines {
        for sub in line.wrap(target_cols, 0) {
            out.push(sub);
        }
    }
    if out.len() > PANE_CONTENT_CAP_LINES {
        let drop = out.len() - PANE_CONTENT_CAP_LINES;
        out.drain(0..drop);
    }
    out
}

/// Remove any orphaned content directories under `session_content/` that
/// are not referenced by either the current session or last-closed-window
/// envelope. Best-effort; failures are logged at debug level so a flaky
/// filesystem does not derail the save path.
fn gc_content_dirs(keep: &[&str]) {
    let root = content_root_dir();
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) => {
            log::debug!("content GC skipped, cannot list {}: {e:#}", root.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if keep.iter().any(|k| k == &name.as_str()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                log::debug!("content GC could not remove {}: {e:#}", path.display());
            }
        }
    }
}

fn collect_leaf_entries(node: &SavedPaneNode, out: &mut Vec<SavedPaneEntry>) {
    match node {
        SavedPaneNode::Empty => {}
        SavedPaneNode::Split { left, right, .. } => {
            collect_leaf_entries(left, out);
            collect_leaf_entries(right, out);
        }
        SavedPaneNode::Leaf(entry) => out.push(entry.clone()),
    }
}

/// Convert a saved `file://` working-directory URL into a spawnable path.
///
/// Returns `None` for non-file schemes and for hosts that do not identify
/// this machine, so a foreign (e.g. ssh) cwd never leaks into a local spawn.
fn cwd_from_working_dir(working_dir: Option<&SerdeUrl>) -> Option<String> {
    let local_hostname = crate::local_hostname::current();
    cwd_from_working_dir_with_hostname(working_dir, local_hostname.as_deref())
}

fn cwd_from_working_dir_with_hostname(
    working_dir: Option<&SerdeUrl>,
    local_hostname: Option<&str>,
) -> Option<String> {
    let url = working_dir?;
    if url.url.scheme() != "file" {
        return None;
    }
    let mut url = url.url.clone();
    if let Some(host) = url.host_str() {
        // `to_file_path` refuses any host other than empty or `localhost`,
        // but OSC 7 reporters conventionally use the real local hostname.
        // Treat this machine's exact hostname as local; any other host keeps the
        // strict behavior so a remote cwd cannot leak into a local spawn.
        if crate::local_hostname::is_local_file_host(Some(host), local_hostname) {
            // `Url::set_host(None)` updates the serialized file URL but leaves
            // its internal host value intact in url 2.5.7, so `to_file_path`
            // still rejects it. Rewrite a proven-local host to the one value
            // that `to_file_path` explicitly accepts.
            url.set_host(Some("localhost")).ok()?;
        }
    }
    url.to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn is_ssh_domain_name(name: &str) -> bool {
    name.starts_with("SSH:") || name.starts_with("SSHMUX:")
}

/// Working directory to hand to `spawn_pane` on restore.
///
/// An ssh-domain pane's OSC 7 cwd is `file://<remote-host>/path`;
/// `to_file_path` refuses remote-host URLs, so those panes used to restore
/// into the remote home directory. The path component is meaningful on the
/// remote side, so pass it through for ssh domains. Local domains accept
/// empty/`localhost` hosts plus this machine's exact hostname (see
/// `cwd_from_working_dir`); any other host still fails conversion, so a
/// leftover remote path cannot leak into a local spawn.
fn cwd_for_restore(domain_name: &str, working_dir: Option<&SerdeUrl>) -> Option<String> {
    if is_ssh_domain_name(domain_name) {
        let url = working_dir?;
        if url.url.scheme() != "file" {
            return None;
        }
        let path = url.url.path();
        return (!path.is_empty() && path != "/").then(|| path.to_string());
    }
    cwd_from_working_dir(working_dir)
}

fn focused_window_id() -> Option<MuxWindowId> {
    match frontend::try_front_end() {
        Some(fe) => fe.focused_ordinary_mux_window_id(),
        None => Mux::get()
            .iter_windows()
            .into_iter()
            .filter(|id| !is_window_logically_closed(*id))
            .max(),
    }
}

// ---------- Pristine state + logically-closed window tracking ----------

// Counts active RestoringGuards so concurrent / nested restores compose
// correctly. mark_dirty is a no-op when depth > 0, and only the last guard's
// drop clears MUX_DIRTY.
static RESTORING_DEPTH: AtomicUsize = AtomicUsize::new(0);
static MUX_DIRTY: AtomicBool = AtomicBool::new(false);
/// Set once the on-disk session snapshot has been restored into this
/// process. From that point the live mux is strictly more authoritative
/// than the file, so exit-time saves must not preserve the old file in
/// favor of a "trivial" live session: trivial then means the user closed
/// the restored tabs on purpose.
static SNAPSHOT_CONSUMED: AtomicBool = AtomicBool::new(false);
/// Set when startup looked for a snapshot and found none. Together with
/// SNAPSHOT_CONSUMED it marks the process that owns the session file, the
/// only one allowed to refresh it while running.
static STARTUP_FOUND_NO_SNAPSHOT: AtomicBool = AtomicBool::new(false);
/// Bumped on every window, tab or pane change so the periodic save only
/// rewrites the snapshot after something actually changed.
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Release builds abort on panic and a force quit skips the exit path, so the
/// snapshot is refreshed while running instead of only at a clean exit.
const PERIODIC_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);
/// Set when startup restored only part of the saved session. The original
/// snapshot and its scrollback sidecars remain the only complete copy, so
/// this process must not replace or delete them on exit.
static SNAPSHOT_RESTORE_INCOMPLETE: AtomicBool = AtomicBool::new(false);

fn logically_closed() -> &'static Mutex<HashSet<MuxWindowId>> {
    static SET: std::sync::OnceLock<Mutex<HashSet<MuxWindowId>>> = std::sync::OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn mark_dirty() {
    if RESTORING_DEPTH.load(Ordering::Acquire) > 0 {
        return;
    }
    SESSION_GENERATION.fetch_add(1, Ordering::AcqRel);
    MUX_DIRTY.store(true, Ordering::Release);
}

fn is_dirty() -> bool {
    MUX_DIRTY.load(Ordering::Acquire)
}

pub fn mark_window_logically_closed(window_id: MuxWindowId) {
    logically_closed().lock().insert(window_id);
    SESSION_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub fn forget_logically_closed(window_id: MuxWindowId) {
    if logically_closed().lock().remove(&window_id) {
        SESSION_GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) fn is_window_logically_closed(window_id: MuxWindowId) -> bool {
    logically_closed().lock().contains(&window_id)
}

fn should_remove_empty_session_file(snapshot_consumed: bool, snapshot_errors: bool) -> bool {
    snapshot_consumed && !snapshot_errors
}

fn record_startup_restore_outcome(outcome: SessionRestoreOutcome) {
    let complete = outcome.is_complete();
    SNAPSHOT_CONSUMED.store(complete, Ordering::Release);
    SNAPSHOT_RESTORE_INCOMPLETE.store(!complete, Ordering::Release);
    if !complete {
        // Windows that fail to restore (ssh host down, missing domain) were
        // previously dropped with only a log line; the user just saw fewer
        // windows come back. Surface it, and say the snapshot survives.
        let missing = outcome
            .expected_windows
            .saturating_sub(outcome.restored_windows);
        persistent_toast_notification(
            "Session Restore",
            &format!(
                "{missing} of {} window{} could not be restored. The saved session is kept and will be retried on the next launch.",
                outcome.expected_windows,
                if outcome.expected_windows == 1 { "" } else { "s" },
            ),
        );
    }
}

fn should_preserve_existing_session_snapshot() -> bool {
    SNAPSHOT_RESTORE_INCOMPLETE.load(Ordering::Acquire)
}

/// A secondary instance (one command, `--always-new-process`) never ran the
/// startup restore, so it must not race the owner by rewriting the file.
fn owns_session_file(snapshot_consumed: bool, startup_found_no_snapshot: bool) -> bool {
    snapshot_consumed || startup_found_no_snapshot
}

fn periodic_save_due(owner: bool, current_generation: u64, saved_generation: u64) -> bool {
    owner && current_generation != saved_generation
}

/// Refresh the session snapshot while running, at most once per interval and
/// only after a window, tab or pane change. The exit save stays the final word.
pub fn start_periodic_session_snapshot() {
    spawn(async move {
        let mut saved_generation = SESSION_GENERATION.load(Ordering::Acquire);
        loop {
            smol::Timer::after(PERIODIC_SNAPSHOT_INTERVAL).await;
            if !config::configuration().restore_previous_session {
                continue;
            }
            let owner = owns_session_file(
                SNAPSHOT_CONSUMED.load(Ordering::Acquire),
                STARTUP_FOUND_NO_SNAPSHOT.load(Ordering::Acquire),
            );
            let generation = SESSION_GENERATION.load(Ordering::Acquire);
            if RESTORING_DEPTH.load(Ordering::Acquire) > 0
                || !periodic_save_due(owner, generation, saved_generation)
            {
                continue;
            }
            let started = std::time::Instant::now();
            match save_session_snapshot() {
                Ok(()) => {
                    saved_generation = generation;
                    log::debug!("periodic session snapshot saved in {:?}", started.elapsed());
                }
                Err(err) => log::warn!("periodic session snapshot failed: {err:#}"),
            }
        }
    })
    .detach();
}

struct RestoringGuard;

impl RestoringGuard {
    fn new() -> Self {
        RESTORING_DEPTH.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for RestoringGuard {
    fn drop(&mut self) {
        // Only the outermost guard clears MUX_DIRTY: nested / concurrent
        // restores share the gate, and clearing on every drop would let one
        // restore steal the pristine bit from another that is still running.
        let prev = RESTORING_DEPTH.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            MUX_DIRTY.store(false, Ordering::Release);
        }
    }
}

// ---------- Triviality / emptiness ----------

/// Decide triviality from tab count alone. `Some(true)` means trivial,
/// `Some(false)` means definitely not, `None` means inspect the single tab's
/// pane count and scrollback to decide.
fn tab_count_triviality(tab_count: usize) -> Option<bool> {
    match tab_count {
        // Zero tabs: a pristine startup window from
        // applicationOpenUntitledFile whose first tab is still being spawned
        // asynchronously. Treat as trivial so the auto-restore lookup reuses
        // it instead of leaving a phantom window beside the restored ones.
        0 => Some(true),
        1 => None,
        _ => Some(false),
    }
}

fn is_window_trivial(window_id: MuxWindowId) -> bool {
    let mux = Mux::get();
    let Some(window) = mux.get_window(window_id) else {
        return true;
    };
    if let Some(verdict) = tab_count_triviality(window.len()) {
        return verdict;
    }
    let Some(tab) = window.get_by_idx(0) else {
        return true;
    };
    let panes = tab.iter_panes_ignoring_zoom();
    if panes.len() != 1 {
        return false;
    }
    let dims = panes[0].pane.get_dimensions();
    // RenderableDimensions::scrollback_rows is the total line count *including*
    // the viewport, so `<= viewport_rows` means no history has scrolled off —
    // i.e. the shell has emitted only its prompt.
    dims.scrollback_rows <= dims.viewport_rows
}

fn is_window_empty(window_id: MuxWindowId) -> bool {
    // For the menu-restore "replace current empty window" check we use the
    // same definition as triviality: 1 tab + 1 pane + no scrollback.
    is_window_trivial(window_id)
}

// ---------- Snapshot building ----------

/// Map the live active-tab index onto the subset of tabs that made it into the
/// snapshot. `retained` holds the original indices, ascending. When the active
/// tab itself was dropped, fall back to the nearest retained tab before it.
fn retained_active_idx(original_active: usize, retained: &[usize]) -> usize {
    retained
        .iter()
        .take_while(|&&idx| idx <= original_active)
        .count()
        .saturating_sub(1)
}

fn build_snapshot_for_window(
    window_id: MuxWindowId,
    content_dir: Option<&std::path::Path>,
) -> anyhow::Result<SavedWindowSnapshot> {
    let mux = Mux::get();
    let window = mux
        .get_window(window_id)
        .ok_or_else(|| anyhow!("window {window_id} not found"))?;

    let mut tabs = Vec::new();
    let mut retained = Vec::new();
    for (idx, tab) in window.iter().enumerate() {
        let pane_tree = SavedPaneNode::from_live(tab.codec_pane_tree(), &mux, content_dir)?;
        if pane_tree.is_empty() {
            // Every pane in this tab was unresolvable; restoring it would
            // produce a tab with nothing in it.
            log::warn!(
                "session snapshot skips tab {} of window {window_id}: no restorable panes",
                tab.tab_id()
            );
            continue;
        }
        tabs.push(SavedTabSnapshot {
            title: tab.get_title(),
            pane_tree,
        });
        retained.push(idx);
    }

    if tabs.is_empty() {
        return Err(anyhow!("window {window_id} has no restorable tabs"));
    }

    Ok(SavedWindowSnapshot {
        active_tab_idx: retained_active_idx(window.get_active_idx(), &retained),
        window_title: window.get_title().to_string(),
        is_focused: focused_window_id() == Some(window_id),
        tabs,
    })
}

// ---------- Atomic write helpers ----------

fn write_json_atomic<T: Serialize>(file_name: &std::path::Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = file_name.parent() {
        config::create_user_owned_dirs(parent)
            .with_context(|| format!("create snapshot dir {}", parent.display()))?;
    }

    let encoded = serde_json::to_string_pretty(value).context("encode snapshot")?;
    // Atomic write: a crash mid-write would otherwise leave a truncated JSON
    // file that fails to parse on the next launch. Write to a sibling temp
    // file and rename on top, which is atomic on POSIX.
    let tmp = file_name.with_file_name(format!(
        "{}.{}.{}.tmp",
        file_name.file_stem().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, format!("{encoded}\n"))
        .with_context(|| format!("write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &file_name) {
        // Don't leave the sibling temp file behind to accumulate in the
        // long-lived config dir when the rename half of the swap fails.
        let _ = std::fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("rename {} -> {}", tmp.display(), file_name.display()));
    }
    Ok(())
}

// ---------- Save entry points ----------

fn is_hotkey_window(window_id: MuxWindowId) -> bool {
    frontend::try_front_end().is_some_and(|fe| fe.is_hotkey_window(window_id))
}

pub fn save_closed_window_snapshot(window_id: MuxWindowId) -> anyhow::Result<()> {
    if is_hotkey_window(window_id) {
        return Ok(());
    }
    if is_window_trivial(window_id) {
        return Ok(());
    }
    let content_dir_name = new_content_dir_name("closed");
    let content_dir = content_root_dir().join(&content_dir_name);
    if let Err(e) = config::create_user_owned_dirs(&content_dir) {
        log::debug!(
            "scrollback dir {} could not be created: {e:#}",
            content_dir.display()
        );
    }
    let snapshot = match build_snapshot_for_window(window_id, Some(&content_dir)) {
        Ok(s) => s,
        Err(e) => {
            // The envelope will never reference this dir; drop its sidecars.
            let _ = std::fs::remove_dir_all(&content_dir);
            return Err(e);
        }
    };
    let envelope = SavedClosedWindow {
        version: SNAPSHOT_VERSION,
        content_dir: content_dir_name,
        window: snapshot,
    };
    let result = write_json_atomic(&closed_window_file(), &envelope);
    match &result {
        Ok(()) => gc_kept_content_dirs(),
        // Envelope was never persisted, so gc (which keeps only referenced
        // dirs) would not have caught this one until some later successful
        // save. Remove the now-unreferenced sidecars now.
        Err(_) => {
            let _ = std::fs::remove_dir_all(&content_dir);
        }
    }
    result
}

pub fn save_session_snapshot() -> anyhow::Result<()> {
    if !is_dirty() {
        return Ok(());
    }
    if should_preserve_existing_session_snapshot() {
        log::warn!("preserving session snapshot after an incomplete startup restore");
        return Ok(());
    }

    let mux = Mux::get();
    let mut window_ids = mux.iter_windows();
    window_ids.sort();

    let content_dir_name = new_content_dir_name("session");
    let content_dir = content_root_dir().join(&content_dir_name);
    if let Err(e) = config::create_user_owned_dirs(&content_dir) {
        log::debug!(
            "scrollback dir {} could not be created: {e:#}",
            content_dir.display()
        );
    }

    let mut windows = Vec::new();
    let mut snapshot_errors = false;
    for id in window_ids {
        if is_window_logically_closed(id) || is_hotkey_window(id) {
            continue;
        }
        match build_snapshot_for_window(id, Some(&content_dir)) {
            Ok(snap) => windows.push(snap),
            Err(err) => {
                snapshot_errors = true;
                log::debug!("skip window {id} for session snapshot: {err:#}");
            }
        }
    }

    // Don't overwrite a useful saved session with a session that is entirely
    // trivial (e.g. a single fresh shell prompt the user opened and closed).
    if windows.is_empty() {
        let _ = std::fs::remove_dir_all(&content_dir);
        // No window survived because the user closed every one of them
        // (logically closed and hidden, or already gone from the mux).
        // Keeping the previous snapshot would resurrect those tabs with
        // stale content on the next launch, so drop it. Only a process that
        // actually consumed the snapshot may do this: a secondary instance
        // that never restored (e.g. spawned to run one command) must not
        // destroy the main session, and a capture error is not intentional
        // closure either.
        if should_remove_empty_session_file(
            SNAPSHOT_CONSUMED.load(Ordering::Acquire),
            snapshot_errors,
        ) {
            let _ = std::fs::remove_file(session_file());
            gc_kept_content_dirs();
        }
        return Ok(());
    }
    // Only guard the old file while it has not been consumed: once this
    // process restored it, a trivial live session reflects tabs the user
    // closed after the restore and must replace the snapshot, not lose to it.
    if windows.len() == 1 && !SNAPSHOT_CONSUMED.load(Ordering::Acquire) {
        let w = &windows[0];
        let mut leaves = Vec::new();
        if let Some(t) = w.tabs.first() {
            collect_leaf_entries(&t.pane_tree, &mut leaves);
        }
        let has_content = leaves.iter().any(|l| l.content_ref.is_some());
        if w.tabs.len() <= 1 && leaves.len() <= 1 && !has_content {
            let _ = std::fs::remove_dir_all(&content_dir);
            return Ok(());
        }
    }

    let session = SavedSession {
        version: SNAPSHOT_VERSION,
        content_dir: content_dir_name,
        windows,
    };
    let result = write_json_atomic(&session_file(), &session);
    match &result {
        Ok(()) => gc_kept_content_dirs(),
        // Envelope was never persisted; drop the orphaned sidecar dir instead
        // of waiting for a later successful save to gc it.
        Err(_) => {
            let _ = std::fs::remove_dir_all(&content_dir);
        }
    }
    result
}

/// Sweep `session_content/` so only the directories referenced by the
/// current `last_session.json` and `last_closed_window.json` envelopes
/// survive. Called after each successful envelope write.
fn gc_kept_content_dirs() {
    let mut keep_strings = Vec::new();
    if let Ok(Some(session)) = load_session_from_path(&session_file()) {
        if !session.content_dir.is_empty() {
            keep_strings.push(session.content_dir);
        }
    }
    if let Ok(Some(closed)) = load_closed_window_from_path(&closed_window_file()) {
        if !closed.content_dir.is_empty() {
            keep_strings.push(closed.content_dir);
        }
    }
    let keep_refs: Vec<&str> = keep_strings.iter().map(|s| s.as_str()).collect();
    gc_content_dirs(&keep_refs);
}

// ---------- Load entry points ----------

fn load_session_from_path(file_name: &std::path::Path) -> anyhow::Result<Option<SavedSession>> {
    let contents = match std::fs::read_to_string(file_name) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!("read {}", file_name.display())));
        }
    };

    let session: SavedSession = match serde_json::from_str(&contents) {
        Ok(s) => s,
        Err(err) => {
            log::warn!(
                "ignoring corrupt session snapshot at {}: {err}",
                file_name.display()
            );
            return Ok(None);
        }
    };

    if session.version != SNAPSHOT_VERSION {
        log::warn!(
            "ignoring session snapshot at {} with unsupported version {} (expected {})",
            file_name.display(),
            session.version,
            SNAPSHOT_VERSION
        );
        return Ok(None);
    }

    if session.windows.is_empty() {
        return Ok(None);
    }

    Ok(Some(session))
}

fn load_session() -> anyhow::Result<Option<SavedSession>> {
    load_session_from_path(&session_file())
}

fn load_closed_window_from_path(
    file_name: &std::path::Path,
) -> anyhow::Result<Option<SavedClosedWindow>> {
    let contents = match std::fs::read_to_string(file_name) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!("read {}", file_name.display())));
        }
    };

    let closed: SavedClosedWindow = match serde_json::from_str(&contents) {
        Ok(c) => c,
        Err(err) => {
            log::warn!(
                "ignoring corrupt closed-window snapshot at {}: {err}",
                file_name.display()
            );
            return Ok(None);
        }
    };

    if closed.version != SNAPSHOT_VERSION {
        log::warn!(
            "ignoring closed-window snapshot at {} with unsupported version {} (expected {})",
            file_name.display(),
            closed.version,
            SNAPSHOT_VERSION
        );
        return Ok(None);
    }

    Ok(Some(closed))
}

fn load_closed_window() -> anyhow::Result<Option<SavedClosedWindow>> {
    load_closed_window_from_path(&closed_window_file())
}

fn delete_closed_window_file() {
    let path = closed_window_file();
    if let Err(err) = std::fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::debug!("could not remove {}: {err:#}", path.display());
        }
    }
}

// ---------- Restore ----------

async fn spawn_panes_for_tab(
    root: &SavedPaneNode,
    content_dir: Option<&std::path::Path>,
) -> anyhow::Result<std::collections::HashMap<PaneId, Arc<dyn Pane>>> {
    let mux = Mux::get();
    let encoding = config::configuration().default_encoding;
    let mut entries = Vec::new();
    collect_leaf_entries(root, &mut entries);

    let mut panes = std::collections::HashMap::new();
    for entry in entries {
        let domain = mux
            .get_domain_by_name(&entry.domain_name)
            .ok_or_else(|| anyhow!("snapshot domain `{}` is not available", entry.domain_name))?;
        let pane = domain
            .spawn_pane(
                &mux,
                entry.size,
                None,
                cwd_for_restore(&entry.domain_name, entry.working_dir.as_ref()),
                encoding,
            )
            .await
            .with_context(|| {
                format!(
                    "spawn pane for snapshot pane {} in domain `{}`",
                    entry.pane_id, entry.domain_name
                )
            })?;

        // Best-effort: surface the previous session's scrollback above the
        // freshly spawned shell's prompt. Failure paths (sidecar missing,
        // schema mismatch, alt screen) all degrade silently to an empty
        // scrollback so the structural restore is never blocked.
        if let (Some(dir), Some(content_ref)) = (content_dir, entry.content_ref.as_ref()) {
            if let Some(payload) = load_pane_payload(dir, content_ref) {
                let lines = if payload.cols != entry.size.cols {
                    reflow_lines(payload.lines, entry.size.cols)
                } else {
                    payload.lines
                };
                if let Err(e) = pane.inject_scrollback(lines) {
                    log::warn!("inject scrollback for pane {}: {e:#}", entry.pane_id);
                }
            }
        }

        panes.insert(entry.pane_id, pane);
    }

    Ok(panes)
}

async fn get_existing_terminal_size() -> Option<TerminalSize> {
    let window = frontend::try_front_end()?
        .gui_windows()
        .into_iter()
        .next()
        .map(|w| w.window.clone())?;
    let (tx, rx) = smol::channel::bounded::<TerminalSize>(1);
    window.notify(TermWindowNotif::Apply(Box::new(
        move |tw: &mut TermWindow| {
            let _ = tx.try_send(tw.get_terminal_size());
        },
    )));
    rx.recv().await.ok()
}

async fn build_tabs_into_window(
    window_id: MuxWindowId,
    tabs: Vec<SavedTabSnapshot>,
    actual_size: Option<TerminalSize>,
    content_dir: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let mux = Mux::get();
    for saved_tab in tabs {
        let size = saved_tab.pane_tree.root_size().unwrap_or_default();
        let tab = Arc::new(Tab::new(&size));
        let panes = spawn_panes_for_tab(&saved_tab.pane_tree, content_dir).await?;
        let pane_tree = saved_tab.pane_tree.into_pane_node();

        tab.try_sync_with_pane_tree(size, pane_tree, |entry| {
            panes
                .get(&entry.pane_id)
                .cloned()
                .ok_or_else(|| anyhow!("missing restored pane {}", entry.pane_id))
        })?;

        if !saved_tab.title.is_empty() {
            tab.set_title(&saved_tab.title);
        }

        mux.add_tab_no_panes(&tab);
        mux.add_tab_to_window(&tab, window_id)?;

        if let Some(s) = actual_size {
            tab.resize(s);
        }
    }
    Ok(())
}

async fn restore_window(
    snapshot: SavedWindowSnapshot,
    current_window_id: Option<MuxWindowId>,
    content_dir: Option<&std::path::Path>,
) -> anyhow::Result<MuxWindowId> {
    let mux = Mux::get();
    let SavedWindowSnapshot {
        active_tab_idx,
        window_title,
        is_focused: _,
        tabs,
    } = snapshot;
    let actual_size = get_existing_terminal_size().await;

    // The caller decides whether this window is reusable: startup's
    // `restore_session` passes the `preexisting_empty` pristine window;
    // `restore_previous_window_from_menu` only passes the current window if
    // it has pre-checked emptiness. Either way, force-replace here. The old
    // inner `is_window_empty` re-check was racy: shell banners or plugin
    // status could push scrollback past the viewport between the lookup and
    // this point, causing the function to fall through and create a phantom
    // new window beside the original.
    //
    // We still drop the stale logically-closed marker so the next save
    // captures consistent state.
    if let Some(window_id) = current_window_id {
        forget_logically_closed(window_id);
        if mux.get_window(window_id).is_some() {
            let existing_tab_ids: Vec<TabId> = match mux.get_window(window_id) {
                Some(window) => window.iter().map(|t| t.tab_id()).collect(),
                None => Vec::new(),
            };

            // Spawn new tabs first to avoid a tab-less window flash.
            build_tabs_into_window(window_id, tabs, actual_size, content_dir).await?;

            // Now drop the originally-empty tabs.
            for old in existing_tab_ids {
                mux.remove_tab(old);
            }

            if let Some(mut window) = mux.get_window_mut(window_id) {
                if !window_title.is_empty() {
                    window.set_title(&window_title);
                }
                if window.len() > 0 {
                    let max_idx = window.len() - 1;
                    window.set_active_without_saving(active_tab_idx.min(max_idx));
                }
            }

            return Ok(window_id);
        }
    }

    // Otherwise create a new mux window.
    let workspace = mux.active_workspace();
    let builder = mux.new_empty_window(Some(workspace), None::<GuiPosition>);
    let new_window_id = *builder;

    let result = async {
        build_tabs_into_window(new_window_id, tabs, actual_size, content_dir).await?;

        if let Some(mut window) = mux.get_window_mut(new_window_id) {
            if !window_title.is_empty() {
                window.set_title(&window_title);
            }
            if window.len() > 0 {
                let max_idx = window.len() - 1;
                window.set_active_without_saving(active_tab_idx.min(max_idx));
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match result {
        Ok(()) => {
            drop(builder);
            Ok(new_window_id)
        }
        Err(err) => {
            builder.cancel();
            Err(err)
        }
    }
}

async fn restore_session(
    session: SavedSession,
    current_window_id: Option<MuxWindowId>,
) -> anyhow::Result<SessionRestoreOutcome> {
    let _guard = RestoringGuard::new();

    let SavedSession {
        version: _,
        content_dir: content_dir_name,
        windows,
    } = session;
    if windows.is_empty() {
        return Ok(SessionRestoreOutcome::default());
    }
    let expected_windows = windows.len();

    let content_dir_path = if content_dir_name.is_empty() {
        None
    } else {
        Some(content_root_dir().join(&content_dir_name))
    };
    let content_dir = content_dir_path.as_deref();

    let focused_idx = windows.iter().position(|w| w.is_focused).unwrap_or(0);

    let mut restored_windows = 0;
    let mut focused_window_id = None;
    for (idx, window_snap) in windows.into_iter().enumerate() {
        let target = if idx == 0 { current_window_id } else { None };
        match restore_window(window_snap, target, content_dir).await {
            Ok(id) => {
                restored_windows += 1;
                if idx == focused_idx {
                    focused_window_id = Some(id);
                }
            }
            Err(err) => log::warn!("failed to restore one window from session: {err:#}"),
        }
    }

    // Best-effort focus on the previously-focused window. The GUI TermWindow
    // for a freshly-created mux window is spawned asynchronously, so the
    // lookup may miss; that is acceptable — focus then stays on whichever
    // window the platform picked.
    if let Some(target_id) = focused_window_id {
        if let Some(fe) = frontend::try_front_end() {
            if let Some(gui) = fe.gui_window_for_mux_window(target_id) {
                gui.window.focus();
            }
        }
    }

    Ok(SessionRestoreOutcome {
        expected_windows,
        restored_windows,
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SessionRestoreOutcome {
    expected_windows: usize,
    restored_windows: usize,
}

impl SessionRestoreOutcome {
    fn is_complete(self) -> bool {
        self.restored_windows == self.expected_windows
    }

    fn restored_any(self) -> bool {
        self.restored_windows > 0
    }
}

pub fn restore_previous_window_from_menu(current_window_id: Option<MuxWindowId>) {
    spawn(async move {
        let result = async {
            match load_closed_window()? {
                Some(closed) => {
                    let _guard = RestoringGuard::new();
                    // The menu path must preserve the user's current work.
                    // Only force-reuse `current_window_id` when it is empty;
                    // otherwise pass `None` so `restore_window` opens a
                    // fresh window beside it. `restore_window` no longer
                    // does this check internally (it would race with
                    // shell-startup output and miscompute emptiness).
                    let reuse_target = current_window_id.filter(|window_id| {
                        !is_hotkey_window(*window_id) && is_window_empty(*window_id)
                    });
                    let content_dir_path = if closed.content_dir.is_empty() {
                        None
                    } else {
                        Some(content_root_dir().join(&closed.content_dir))
                    };
                    restore_window(closed.window, reuse_target, content_dir_path.as_deref())
                        .await?;
                    drop(_guard);
                    // Consume the snapshot only on success: if restore failed
                    // (e.g. domain unavailable), keep the file so the user can
                    // retry after fixing the underlying issue.
                    delete_closed_window_file();
                    Ok::<bool, anyhow::Error>(true)
                }
                None => Ok(false),
            }
        }
        .await;

        match result {
            Ok(true) => {}
            Ok(false) => {
                persistent_toast_notification(
                    "Restore Previous Window",
                    "No previously-closed window is available to restore.",
                );
            }
            Err(err) => {
                log::warn!("failed to restore previous window: {err:#}");
                persistent_toast_notification("Restore Previous Window", &format!("{err:#}"));
            }
        }
    })
    .detach();
}

pub async fn try_restore_on_startup() -> anyhow::Result<bool> {
    match load_session()? {
        Some(session) => {
            // macOS's applicationOpenUntitledFile can dispatch a SpawnWindow
            // before this runs, leaving a pristine empty mux window in place.
            // Reuse it as the target for the first restored window so the user
            // does not end up with one phantom empty window plus the restored
            // ones.
            let preexisting_empty = Mux::get()
                .iter_windows()
                .into_iter()
                .find(|id| !is_hotkey_window(*id) && is_window_empty(*id));
            let outcome = restore_session(session, preexisting_empty).await?;
            record_startup_restore_outcome(outcome);
            if !outcome.is_complete() {
                log::warn!(
                    "restored {} of {} session windows; preserving the original snapshot",
                    outcome.restored_windows,
                    outcome.expected_windows
                );
            }
            Ok(outcome.restored_any())
        }
        None => {
            STARTUP_FOUND_NO_SNAPSHOT.store(true, Ordering::Release);
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_session_snapshot_uses_the_ordinary_focus_resolver() {
        let source = include_str!("session_restore.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let focus = source
            .split("fn focused_window_id()")
            .nth(1)
            .unwrap()
            .split("// ---------- Pristine state")
            .next()
            .unwrap();
        assert!(focus.contains("Some(fe) => fe.focused_ordinary_mux_window_id()"));
        assert!(focus.contains("!is_window_logically_closed(*id)"));
        assert!(!focus.contains("fe.focused_mux_window_id()"));
        assert!(source.contains("is_focused: focused_window_id() == Some(window_id)"));
    }

    #[test]
    fn only_the_session_owner_refreshes_the_snapshot_while_running() {
        // Restored this snapshot, or started with none: this process owns it.
        assert!(owns_session_file(true, false));
        assert!(owns_session_file(false, true));
        // Secondary instance, or a partial restore that must keep the file.
        assert!(!owns_session_file(false, false));

        assert!(periodic_save_due(true, 3, 2));
        assert!(!periodic_save_due(true, 3, 3));
        assert!(!periodic_save_due(false, 3, 2));
    }

    fn serde_url(s: &str) -> SerdeUrl {
        SerdeUrl {
            url: url::Url::parse(s).unwrap(),
        }
    }

    fn leaf(pane_id: usize) -> SavedPaneNode {
        SavedPaneNode::Leaf(SavedPaneEntry {
            window_id: 0,
            tab_id: TabId::new(0),
            pane_id: PaneId::new(pane_id),
            title: String::new(),
            size: TerminalSize::default(),
            working_dir: None,
            domain_name: "local".to_string(),
            is_active_pane: true,
            is_zoomed_pane: false,
            workspace: "default".to_string(),
            content_ref: None,
        })
    }

    fn split_node() -> SplitDirectionAndSize {
        SplitDirectionAndSize {
            direction: mux::tab::SplitDirection::Horizontal,
            first: TerminalSize::default(),
            second: TerminalSize::default(),
        }
    }

    /// One unresolvable pane must cost only that pane, not the split it lived
    /// in and not the rest of the window.
    #[test]
    fn split_collapses_onto_the_surviving_pane() {
        let survivor = |node: SavedPaneNode| match node {
            SavedPaneNode::Leaf(entry) => Some(entry.pane_id),
            _ => None,
        };

        assert_eq!(
            survivor(SavedPaneNode::collapse_split(
                SavedPaneNode::Empty,
                leaf(7),
                split_node()
            )),
            Some(PaneId::new(7))
        );

        assert_eq!(
            survivor(SavedPaneNode::collapse_split(
                leaf(7),
                SavedPaneNode::Empty,
                split_node()
            )),
            Some(PaneId::new(7))
        );

        assert!(SavedPaneNode::collapse_split(
            SavedPaneNode::Empty,
            SavedPaneNode::Empty,
            split_node()
        )
        .is_empty());

        assert!(matches!(
            SavedPaneNode::collapse_split(leaf(7), leaf(8), split_node()),
            SavedPaneNode::Split { .. }
        ));
    }

    #[test]
    fn active_tab_idx_follows_the_retained_tabs() {
        // Nothing dropped: the index is unchanged.
        assert_eq!(retained_active_idx(2, &[0, 1, 2, 3]), 2);
        // A tab before the active one was dropped: shift down with it.
        assert_eq!(retained_active_idx(2, &[0, 2, 3]), 1);
        // Only tabs after the active one were dropped.
        assert_eq!(retained_active_idx(1, &[0, 1]), 1);
        // The active tab itself was dropped: land on the nearest one before it.
        assert_eq!(retained_active_idx(2, &[0, 1, 3]), 1);
        // The active tab was the first one and it was dropped.
        assert_eq!(retained_active_idx(0, &[1, 2]), 0);
    }

    #[test]
    fn restore_cwd_local_domain_requires_local_url() {
        assert_eq!(
            cwd_for_restore("local", Some(&serde_url("file:///Users/a/src"))).as_deref(),
            Some("/Users/a/src")
        );
        assert_eq!(
            cwd_for_restore("local", Some(&serde_url("file://localhost/Users/a/src"))).as_deref(),
            Some("/Users/a/src")
        );
        // A remote-host cwd must not leak into a local spawn.
        assert_eq!(
            cwd_for_restore("local", Some(&serde_url("file://server/home/u"))),
            None
        );
    }

    #[test]
    fn restore_cwd_local_domain_accepts_exact_local_hostname() {
        // URL parsing lowercases the host; the comparison must not care.
        assert_eq!(
            cwd_from_working_dir_with_hostname(
                Some(&serde_url("file://MAC.LOCAL/Users/a/src")),
                Some("mac.local"),
            )
            .as_deref(),
            Some("/Users/a/src")
        );
        // A trailing dot (FQDN form) still identifies this machine.
        assert_eq!(
            cwd_from_working_dir_with_hostname(
                Some(&serde_url("file://mac.local./Users/a/src")),
                Some("mac.local"),
            )
            .as_deref(),
            Some("/Users/a/src")
        );
        // A short remote host with the same first label is not local proof.
        assert_eq!(
            cwd_from_working_dir_with_hostname(
                Some(&serde_url("file://mac/Users/a/src")),
                Some("mac.local"),
            ),
            None
        );
    }

    #[test]
    fn restore_cwd_ssh_domain_keeps_remote_path() {
        assert_eq!(
            cwd_for_restore("SSH:server", Some(&serde_url("file://server/home/u/proj"))).as_deref(),
            Some("/home/u/proj")
        );
        assert_eq!(
            cwd_for_restore("SSHMUX:server", Some(&serde_url("file://server/"))),
            None
        );
        assert_eq!(cwd_for_restore("SSH:server", None), None);
    }

    fn sample_window(title: &str) -> SavedWindowSnapshot {
        SavedWindowSnapshot {
            active_tab_idx: 0,
            window_title: title.to_string(),
            is_focused: false,
            tabs: vec![SavedTabSnapshot {
                title: "Test Tab".to_string(),
                pane_tree: SavedPaneNode::Empty,
            }],
        }
    }

    fn sample_session(version: u32) -> SavedSession {
        SavedSession {
            version,
            content_dir: String::new(),
            windows: vec![sample_window("Test Window")],
        }
    }

    fn sample_closed(version: u32) -> SavedClosedWindow {
        SavedClosedWindow {
            version,
            content_dir: String::new(),
            window: sample_window("Test Window"),
        }
    }

    #[test]
    fn image_cells_do_not_blow_up_the_scrollback_sidecar() {
        use termwiz::cell::CellAttributes;
        use termwiz::image::{ImageCell, ImageData};
        use termwiz::surface::{TextureCoordinate, SEQ_ZERO};

        // One image stretched over a row: every cell shares the same
        // Arc<ImageData>, but serde writes the full blob once per cell (#559).
        let image = Arc::new(ImageData::with_raw_data(vec![0u8; 128 * 1024]));
        let origin = ordered_float::NotNan::new(0.0f32).unwrap();
        let mut line = wezterm_term::Line::from_text(
            &"x".repeat(100),
            &CellAttributes::default(),
            SEQ_ZERO,
            None,
        );
        for cell in line.cells_mut_for_attr_changes_only() {
            cell.attrs_mut().attach_image(ImageCell::new(
                TextureCoordinate::new(origin, origin),
                TextureCoordinate::new(origin, origin),
                Arc::clone(&image),
            ));
        }

        let dir = tempfile::tempdir().unwrap();
        let content_ref = write_pane_sidecar(dir.path(), 7.into(), vec![line], 100, 1)
            .expect("image-heavy pane still gets a sidecar");
        let size = std::fs::metadata(dir.path().join(&content_ref.filename))
            .unwrap()
            .len();
        assert!(size < 64 * 1024, "sidecar is {} bytes", size);

        let payload = load_pane_payload(dir.path(), &content_ref).expect("payload");
        let restored = &payload.lines[0];
        assert_eq!(restored.as_str().trim_end(), "x".repeat(100));
        assert!(restored
            .visible_cells()
            .all(|c| c.attrs().images().is_none()));
    }

    #[test]
    fn session_round_trips_via_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        write_json_atomic(&path, &sample_session(SNAPSHOT_VERSION)).unwrap();

        let loaded = load_session_from_path(&path).unwrap().expect("session");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.windows.len(), 1);
        assert_eq!(loaded.windows[0].window_title, "Test Window");
    }

    #[test]
    fn closed_window_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_closed_window.json");
        write_json_atomic(&path, &sample_closed(SNAPSHOT_VERSION)).unwrap();

        let loaded = load_closed_window_from_path(&path)
            .unwrap()
            .expect("closed window");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.window.window_title, "Test Window");
    }

    #[test]
    fn corrupt_session_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    #[test]
    fn unsupported_session_version_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(
            &path,
            serde_json::to_string(&sample_session(SNAPSHOT_VERSION + 1)).unwrap(),
        )
        .unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    #[test]
    fn v2_snapshot_is_ignored() {
        // Pre-v3 single-window envelope: must be silently ignored on upgrade.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(
            &path,
            r#"{"version":2,"active_tab_idx":0,"window_title":"x","tabs":[]}"#,
        )
        .unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    // Shared by every test that touches MUX_DIRTY / RESTORING_DEPTH so they
    // serialize against cargo's parallel test runner.
    static DIRTY_TEST_GATE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[test]
    fn pristine_state_machine() {
        let _g = DIRTY_TEST_GATE.lock();

        MUX_DIRTY.store(false, Ordering::Release);
        RESTORING_DEPTH.store(0, Ordering::Release);
        assert!(!is_dirty());

        // mark_dirty outside a restore should flip the bit.
        mark_dirty();
        assert!(is_dirty());

        // Inside RestoringGuard, mark_dirty is a no-op (the previously-set
        // bit remains), and dropping the outermost guard forces dirty back
        // to false.
        {
            let _guard = RestoringGuard::new();
            mark_dirty();
            assert!(MUX_DIRTY.load(Ordering::Acquire));
        }
        assert!(!is_dirty());
    }

    #[test]
    fn nested_restoring_guards_compose() {
        let _g = DIRTY_TEST_GATE.lock();

        MUX_DIRTY.store(false, Ordering::Release);
        RESTORING_DEPTH.store(0, Ordering::Release);

        let outer = RestoringGuard::new();
        {
            let _inner = RestoringGuard::new();
            // Both guards are active: depth == 2.
            assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 2);
        }
        // Inner dropped; depth == 1 and MUX_DIRTY still untouched.
        assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 1);
        // Even if something marks dirty here, it must be ignored — depth > 0.
        mark_dirty();
        assert!(!is_dirty());
        drop(outer);
        // Outer dropped; depth == 0 and dirty cleared by the outer drop.
        assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 0);
        assert!(!is_dirty());
    }

    #[test]
    fn logically_closed_set_round_trips() {
        // Use a high id unlikely to collide with concurrent test windows.
        let id: MuxWindowId = 999_999;
        forget_logically_closed(id);
        assert!(!is_window_logically_closed(id));
        mark_window_logically_closed(id);
        assert!(is_window_logically_closed(id));
        forget_logically_closed(id);
        assert!(!is_window_logically_closed(id));
    }

    #[test]
    fn consumed_snapshot_is_removed_only_after_intentional_full_close() {
        assert!(should_remove_empty_session_file(true, false));
        assert!(!should_remove_empty_session_file(false, false));
        assert!(!should_remove_empty_session_file(true, true));
    }

    #[test]
    fn hotkey_sessions_are_neither_saved_nor_reused_for_ordinary_restore() {
        let source = include_str!("session_restore.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let closed = source
            .split("pub fn save_closed_window_snapshot(")
            .nth(1)
            .unwrap()
            .split("pub fn save_session_snapshot()")
            .next()
            .unwrap();
        assert!(
            closed.find("if is_hotkey_window(window_id)").unwrap()
                < closed.find("build_snapshot_for_window").unwrap()
        );
        let session = source
            .split("pub fn save_session_snapshot()")
            .nth(1)
            .unwrap()
            .split("async fn")
            .next()
            .unwrap();
        assert!(
            session.find("is_hotkey_window(id)").unwrap()
                < session.find("match build_snapshot_for_window").unwrap()
        );
        assert_eq!(source.matches("!is_hotkey_window(*").count(), 2);
    }

    #[test]
    fn session_snapshot_is_consumed_only_after_every_window_restores() {
        SNAPSHOT_CONSUMED.store(false, Ordering::Release);
        SNAPSHOT_RESTORE_INCOMPLETE.store(false, Ordering::Release);

        let complete = SessionRestoreOutcome {
            expected_windows: 2,
            restored_windows: 2,
        };
        assert!(complete.is_complete());
        assert!(complete.restored_any());
        record_startup_restore_outcome(complete);
        assert!(SNAPSHOT_CONSUMED.load(Ordering::Acquire));
        assert!(!should_preserve_existing_session_snapshot());

        let partial = SessionRestoreOutcome {
            expected_windows: 2,
            restored_windows: 1,
        };
        assert!(!partial.is_complete());
        assert!(partial.restored_any());
        record_startup_restore_outcome(partial);
        assert!(!SNAPSHOT_CONSUMED.load(Ordering::Acquire));
        assert!(should_preserve_existing_session_snapshot());

        let failed = SessionRestoreOutcome {
            expected_windows: 2,
            restored_windows: 0,
        };
        assert!(!failed.is_complete());
        assert!(!failed.restored_any());
        record_startup_restore_outcome(failed);
        assert!(!SNAPSHOT_CONSUMED.load(Ordering::Acquire));
        assert!(should_preserve_existing_session_snapshot());

        let empty = SessionRestoreOutcome::default();
        assert!(empty.is_complete());
        assert!(!empty.restored_any());
        record_startup_restore_outcome(empty);
        assert!(SNAPSHOT_CONSUMED.load(Ordering::Acquire));
        assert!(!should_preserve_existing_session_snapshot());

        SNAPSHOT_CONSUMED.store(false, Ordering::Release);
    }

    #[test]
    fn tab_count_triviality_treats_pristine_zero_as_trivial() {
        // Pristine startup window with no tabs yet: must be reusable so the
        // auto-restore lookup does not leave a phantom window beside the
        // restored ones.
        assert_eq!(tab_count_triviality(0), Some(true));
        // Single-tab path is undecided here; the caller must inspect panes.
        assert_eq!(tab_count_triviality(1), None);
        // Multi-tab is never trivial.
        assert_eq!(tab_count_triviality(2), Some(false));
        assert_eq!(tab_count_triviality(99), Some(false));
    }

    // Bug B (`restore_window` force-reuses when `current_window_id` is Some)
    // and Bug C (menu caller pre-checks `is_window_empty`) both depend on a
    // live `Mux` singleton plus a freshly spawned async window. There is no
    // mux test harness in this crate yet, so they are covered by the manual
    // verification step in /Users/tw93/.claude/plans/quiet-weaving-valiant.md:
    // cold-launch with a saved one-window session must produce exactly one
    // window, and the menu "restore previous window" path inside a non-empty
    // window must open a new window beside the user's current work.

    // ---- Part 2: content restoration ----

    fn make_line(text: &str) -> wezterm_term::Line {
        wezterm_term::Line::from_text(text, &wezterm_term::CellAttributes::default(), 0, None)
    }

    fn sample_payload(cols: usize, rows: usize, line_count: usize) -> PaneContentPayload {
        let lines = (0..line_count)
            .map(|i| make_line(&format!("line {i}")))
            .collect();
        PaneContentPayload {
            schema: CONTENT_SCHEMA_VERSION,
            fingerprint: WEZTERM_SURFACE_FINGERPRINT.to_string(),
            cols,
            rows,
            lines,
        }
    }

    #[test]
    fn pane_content_round_trips_via_bincode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pane_42.bin");
        let payload = sample_payload(80, 24, 5);
        write_bincode_atomic(&path, &payload, None).unwrap();

        let content_ref = PaneContentRef {
            filename: "pane_42.bin".into(),
            schema: CONTENT_SCHEMA_VERSION,
            saved_cols: 80,
            saved_rows: 24,
            line_count: 5,
        };
        let loaded = load_pane_payload(dir.path(), &content_ref).expect("payload present");
        assert_eq!(loaded.cols, 80);
        assert_eq!(loaded.rows, 24);
        assert_eq!(loaded.lines.len(), 5);
        assert_eq!(loaded.lines[0].as_str(), "line 0");
        assert_eq!(loaded.lines[4].as_str(), "line 4");
    }

    #[test]
    fn pane_content_skipped_when_sidecar_missing() {
        let dir = tempfile::tempdir().unwrap();
        let content_ref = PaneContentRef {
            filename: "pane_999.bin".into(),
            schema: CONTENT_SCHEMA_VERSION,
            saved_cols: 80,
            saved_rows: 24,
            line_count: 0,
        };
        assert!(load_pane_payload(dir.path(), &content_ref).is_none());
    }

    #[test]
    fn pane_content_skipped_when_schema_mismatched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pane_7.bin");
        let mut payload = sample_payload(80, 24, 2);
        payload.schema = u32::MAX;
        write_bincode_atomic(&path, &payload, None).unwrap();

        let content_ref = PaneContentRef {
            filename: "pane_7.bin".into(),
            schema: u32::MAX,
            saved_cols: 80,
            saved_rows: 24,
            line_count: 2,
        };
        assert!(load_pane_payload(dir.path(), &content_ref).is_none());
    }

    #[test]
    fn pane_content_skipped_when_fingerprint_mismatched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pane_8.bin");
        let mut payload = sample_payload(80, 24, 2);
        payload.fingerprint = "wezterm-surface-from-the-future".to_string();
        write_bincode_atomic(&path, &payload, None).unwrap();

        let content_ref = PaneContentRef {
            filename: "pane_8.bin".into(),
            schema: CONTENT_SCHEMA_VERSION,
            saved_cols: 80,
            saved_rows: 24,
            line_count: 2,
        };
        assert!(load_pane_payload(dir.path(), &content_ref).is_none());
    }

    #[test]
    fn write_bincode_atomic_refuses_oversized_payload() {
        // An image-heavy pane can serialize far past the line cap. A tiny
        // byte ceiling stands in for that here: the write must be refused
        // and no file should be left behind.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pane_huge.bin");
        let payload = sample_payload(80, 24, 200);
        let err = write_bincode_atomic(&path, &payload, Some(16))
            .expect_err("payload over the ceiling must be refused");
        assert!(
            err.to_string().contains("exceeding"),
            "unexpected error: {:#}",
            err
        );
        assert!(!path.exists(), "no sidecar file should be written");
    }

    #[test]
    fn write_bincode_atomic_allows_payload_within_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pane_ok.bin");
        let payload = sample_payload(80, 24, 3);
        write_bincode_atomic(&path, &payload, Some(PANE_CONTENT_MAX_BYTES))
            .expect("small payload must write within the ceiling");
        assert!(path.exists());
    }

    #[test]
    fn reflow_caps_total_lines_after_wrapping() {
        // Start with 1200 short lines at 80 cols, target 10 cols. Each line
        // is unwrapped (no internal wrap state) so wrap to 10 cols leaves it
        // a single sub-line (`Line::wrap` only splits at the column boundary
        // when the line is actually wider). What this test guarantees is the
        // pure invariant: `reflow_lines` never returns more than the cap.
        let lines: Vec<wezterm_term::Line> =
            (0..2000).map(|i| make_line(&format!("L{i}"))).collect();
        let out = reflow_lines(lines, 10);
        assert!(out.len() <= PANE_CONTENT_CAP_LINES);
    }

    #[test]
    fn reflow_preserves_recent_lines_when_capping() {
        // The newest lines must survive when the cap drops the oldest.
        let lines: Vec<wezterm_term::Line> = (0..PANE_CONTENT_CAP_LINES + 200)
            .map(|i| make_line(&format!("L{i}")))
            .collect();
        let out = reflow_lines(lines, 80);
        assert_eq!(out.len(), PANE_CONTENT_CAP_LINES);
        let last = out.last().unwrap();
        let total = PANE_CONTENT_CAP_LINES + 200;
        assert_eq!(last.as_str(), format!("L{}", total - 1));
    }
}
