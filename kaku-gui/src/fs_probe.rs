//! File checks for link hover, which runs on the GUI thread.
//!
//! Hovering text looks up every candidate path it might link (up to 64 per
//! word). On a local disk a `stat` is cheap, but on an SMB, NFS or WebDAV
//! mount a slow or vanished server can hold it for seconds and freeze the
//! window. Paths on a network mount are therefore checked on a helper thread
//! with a short wait, and the answer is cached, so hover never blocks on the
//! network. Clicking a link still checks the path directly.

use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Missing,
    File,
    Dir,
}

/// How long hover waits for a network check before treating the path as
/// missing; a later hover reads the cached answer.
const NETWORK_WAIT: Duration = Duration::from_millis(30);
/// How long a network check's answer is reused.
const RESULT_TTL: Duration = Duration::from_secs(10);
/// How often the mount table is read again.
const MOUNTS_TTL: Duration = Duration::from_secs(30);

/// What a path is, without blocking the caller on a network mount.
pub fn kind(path: &Path) -> Kind {
    static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
    let mut guard = PROBE.lock().unwrap();
    let probe = guard.get_or_insert_with(Probe::default);
    let mounts = probe.network_mounts().to_vec();
    match mount_for(path, &mounts) {
        None => {
            drop(guard);
            stat_kind(path)
        }
        Some(mount) => probe.network_kind(path, mount, stat_kind, NETWORK_WAIT),
    }
}

/// Whether `path` exists, under the same rules as `kind`.
pub fn exists(path: &Path) -> bool {
    kind(path) != Kind::Missing
}

fn stat_kind(path: &Path) -> Kind {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Kind::Dir,
        Ok(_) => Kind::File,
        Err(_) => Kind::Missing,
    }
}

/// The network mount `path` lies on, the longest matching mount point.
fn mount_for(path: &Path, mounts: &[PathBuf]) -> Option<PathBuf> {
    mounts
        .iter()
        .filter(|mount| mount.as_os_str() != "/" && path.starts_with(mount))
        .max_by_key(|mount| mount.as_os_str().len())
        .cloned()
}

#[derive(Default)]
struct Probe {
    mounts: Option<(Instant, Vec<PathBuf>)>,
    results: HashMap<PathBuf, (Instant, Kind)>,
    /// Mounts with a check still running; a hung server gets one stuck
    /// thread, not one per hovered word.
    pending: HashSet<PathBuf>,
}

/// Shared between `Probe` and the helper threads it starts.
static RESULTS: Mutex<Vec<(PathBuf, PathBuf, Kind)>> = Mutex::new(Vec::new());

impl Probe {
    fn network_mounts(&mut self) -> &[PathBuf] {
        let stale = self
            .mounts
            .as_ref()
            .map_or(true, |(read_at, _)| read_at.elapsed() > MOUNTS_TTL);
        if stale {
            self.mounts = Some((Instant::now(), read_network_mounts()));
        }
        &self.mounts.as_ref().unwrap().1
    }

    fn collect_finished(&mut self) {
        for (path, mount, kind) in RESULTS.lock().unwrap().drain(..) {
            self.pending.remove(&mount);
            self.results.insert(path, (Instant::now(), kind));
        }
    }

    fn network_kind(
        &mut self,
        path: &Path,
        mount: PathBuf,
        check: fn(&Path) -> Kind,
        wait: Duration,
    ) -> Kind {
        self.collect_finished();
        if let Some((at, kind)) = self.results.get(path) {
            if at.elapsed() < RESULT_TTL {
                return *kind;
            }
        }
        if self.results.len() > 4096 {
            self.results.retain(|_, (at, _)| at.elapsed() < RESULT_TTL);
        }
        if !self.pending.insert(mount.clone()) {
            return Kind::Missing;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let owned = path.to_path_buf();
        std::thread::spawn(move || {
            let kind = check(&owned);
            RESULTS.lock().unwrap().push((owned, mount, kind));
            tx.send(kind).ok();
        });
        match rx.recv_timeout(wait) {
            Ok(kind) => {
                self.collect_finished();
                kind
            }
            Err(_) => Kind::Missing,
        }
    }
}

/// Mount points of filesystems that are not local. `MNT_NOWAIT` returns
/// the kernel's cached table without asking any server.
fn read_network_mounts() -> Vec<PathBuf> {
    let mut mounts = vec![];
    unsafe {
        let mut entries: *mut libc::statfs = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut entries, libc::MNT_NOWAIT);
        if count <= 0 || entries.is_null() {
            return mounts;
        }
        for entry in std::slice::from_raw_parts(entries, count as usize) {
            if entry.f_flags & libc::MNT_LOCAL as u32 != 0 {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(entry.f_mntonname.as_ptr());
            mounts.push(PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes())));
        }
    }
    mounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_network_mount_wins_and_root_never_matches() {
        let mounts = vec![
            PathBuf::from("/"),
            PathBuf::from("/Volumes/share"),
            PathBuf::from("/Volumes/share/nested"),
        ];
        assert_eq!(mount_for(Path::new("/Users/me/a.txt"), &mounts), None);
        assert_eq!(
            mount_for(Path::new("/Volumes/share/a.txt"), &mounts),
            Some(PathBuf::from("/Volumes/share"))
        );
        assert_eq!(
            mount_for(Path::new("/Volumes/share/nested/b"), &mounts),
            Some(PathBuf::from("/Volumes/share/nested"))
        );
        // A sibling whose name only starts the same is not on the mount.
        assert_eq!(mount_for(Path::new("/Volumes/shared/c"), &mounts), None);
    }

    #[test]
    fn local_paths_are_checked_directly() {
        // Reads the real mount table on the way.
        assert_eq!(kind(Path::new("/")), Kind::Dir);
        assert_eq!(kind(Path::new("/etc/hosts")), Kind::File);
        assert_eq!(kind(Path::new("/no/such/kaku/path")), Kind::Missing);
    }

    #[test]
    fn a_hung_network_check_does_not_hold_hover() {
        fn slow(_: &Path) -> Kind {
            std::thread::sleep(Duration::from_millis(300));
            Kind::File
        }
        let mount = PathBuf::from("/Volumes/hung-probe-test");
        let path = mount.join("notes.md");
        let mut probe = Probe::default();

        let started = Instant::now();
        assert_eq!(
            probe.network_kind(&path, mount.clone(), slow, Duration::from_millis(30)),
            Kind::Missing
        );
        // While that check runs, more words on the same mount start nothing.
        assert_eq!(
            probe.network_kind(&mount.join("other.md"), mount.clone(), slow, Duration::ZERO),
            Kind::Missing
        );
        assert!(started.elapsed() < Duration::from_millis(200));

        // Once it finishes, hover reads the answer from the cache.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            probe.network_kind(&path, mount, slow, Duration::ZERO),
            Kind::File
        );
    }
}
