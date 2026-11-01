use anyhow::anyhow;
use config::proxy::{apply_to_command, detect_system_proxy};
use config::{configuration, wezterm_version};
use serde::*;
use std::cmp::Ordering as CmpOrdering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wezterm_toast_notification::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Release {
    pub url: String,
    pub body: String,
    pub html_url: String,
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub size: usize,
    pub url: String,
    pub browser_download_url: String,
}

/// Metadata written alongside a staged (pre-downloaded) update.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StagedUpdateInfo {
    pub tag: String,
    pub body: String,
    pub staged_at: u64,
    pub verified: bool,
    /// Path to the extracted Kaku.app inside the staged_update directory.
    pub app_path: String,
}

const STAGED_DIR_NAME: &str = "staged_update";
const STAGED_LOCK_NAME: &str = "staged_update.lock";
const STAGED_META_NAME: &str = "metadata.json";
const UPDATE_ZIP_NAME: &str = "kaku_for_update.zip";
const UPDATE_SHA_NAME: &str = "kaku_for_update.zip.sha256";
const LATEST_ZIP_URL: &str =
    "https://github.com/tw93/Kaku/releases/latest/download/kaku_for_update.zip";
const LATEST_SHA_URL: &str =
    "https://github.com/tw93/Kaku/releases/latest/download/kaku_for_update.zip.sha256";
/// Staged updates older than this are considered expired.
const STAGED_MAX_AGE_SECS: u64 = 7 * 24 * 3600;

fn staged_dir() -> PathBuf {
    config::DATA_DIR.join(STAGED_DIR_NAME)
}

fn staged_meta_path() -> PathBuf {
    staged_dir().join(STAGED_META_NAME)
}

/// Returns info about a staged update if one is present, verified, and not expired.
pub fn staged_update_available() -> Option<StagedUpdateInfo> {
    let meta_path = staged_meta_path();
    let content = std::fs::read_to_string(&meta_path).ok()?;
    let info: StagedUpdateInfo = match serde_json::from_str(&content) {
        Ok(info) => info,
        Err(e) => {
            log::warn!("staged update metadata is corrupted: {}", e);
            return None;
        }
    };
    if !info.verified {
        return None;
    }
    // Check the extracted app still exists.
    let app = PathBuf::from(&info.app_path);
    if !app.exists() {
        return None;
    }
    // A staged update whose version is not newer than what we're running is
    // stale: the update was already applied but the post-update marker
    // cleanup never ran. Drop it so the menu stops offering a pointless
    // "Restart to Update", and so update_checker's startup cleanup removes
    // the directory.
    if !is_newer(&info.tag, wezterm_version()) {
        log::info!(
            "staged update {} is not newer than current {}, removing",
            info.tag,
            wezterm_version()
        );
        cleanup_staged_update();
        return None;
    }
    // Check expiry.
    let now = now_unix_secs();
    if now.saturating_sub(info.staged_at) > STAGED_MAX_AGE_SECS {
        log::info!("staged update {} expired, removing", info.tag);
        cleanup_staged_update();
        return None;
    }
    Some(info)
}

/// Remove the staged_update directory entirely.
///
/// Acquires the staging lock before removing so this call cannot race with a
/// concurrent `download_and_stage_update`. If another process holds the lock
/// the cleanup is skipped silently: that process owns the directory and will
/// clean it up itself.
pub fn cleanup_staged_update() {
    let dir = staged_dir();
    if dir.exists() {
        let _lock = match StagedUpdateLock::try_acquire() {
            Ok(lock) => lock,
            Err(_) => return,
        };
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => log::info!("cleaned up staged update directory"),
            Err(e) => log::warn!("failed to clean staged update directory: {}", e),
        }
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// RAII guard around an exclusive `flock` on the staging lock file.
///
/// Two Kaku processes started at once would otherwise race on
/// `staged_update/`: one tears it down with `remove_dir_all` while the other
/// is mid-extract. Holding `flock(LOCK_EX | LOCK_NB)` for the duration of
/// `download_and_stage_update` serializes them. The kernel automatically
/// releases the lock when the file handle drops or the process exits, so the
/// lock survives crashes without a stale-PID cleanup dance.
struct StagedUpdateLock {
    _file: std::fs::File,
}

impl StagedUpdateLock {
    fn try_acquire() -> anyhow::Result<Self> {
        let dir = config::DATA_DIR.clone();
        config::create_user_owned_dirs(&dir)
            .map_err(|e| anyhow!("failed to create data dir for staging lock: {}", e))?;
        let path = dir.join(STAGED_LOCK_NAME);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| anyhow!("failed to open staging lock {}: {}", path.display(), e))?;
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        // SAFETY: fd is owned by `file` which outlives this call.
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if matches!(err.raw_os_error(), Some(libc::EWOULDBLOCK)) {
                anyhow::bail!("another Kaku process is already staging an update");
            }
            return Err(anyhow!(
                "failed to acquire staging lock {}: {}",
                path.display(),
                err
            ));
        }
        Ok(Self { _file: file })
    }
}

fn curl_get_release_json(url: &str, proxy: &Option<String>) -> anyhow::Result<Release> {
    use std::process::Command;

    let mut cmd = Command::new("/usr/bin/curl");
    cmd.arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--connect-timeout")
        .arg("15")
        .arg("--user-agent")
        .arg(format!("kaku/{}", wezterm_version()))
        .arg(url);
    apply_to_command(&mut cmd, proxy);

    let out = cmd.output().map_err(|e| anyhow!("curl failed: {}", e))?;
    if !out.status.success() {
        anyhow::bail!(
            "curl request failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    serde_json::from_slice(&out.stdout).map_err(|e| anyhow!("failed to parse release JSON: {}", e))
}

pub fn get_latest_release_info() -> anyhow::Result<Release> {
    let proxy = detect_system_proxy();
    curl_get_release_json(
        "https://api.github.com/repos/tw93/Kaku/releases/latest",
        &proxy,
    )
    .or_else(|_| get_latest_tag_via_redirect(&proxy))
}

fn get_latest_tag_via_redirect(proxy: &Option<String>) -> anyhow::Result<Release> {
    use std::process::Command;

    let mut cmd = Command::new("/usr/bin/curl");
    cmd.arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--connect-timeout")
        .arg("10")
        .arg("--write-out")
        .arg("%{url_effective}")
        .arg("--output")
        .arg("/dev/null")
        .arg("https://github.com/tw93/Kaku/releases/latest");
    apply_to_command(&mut cmd, proxy);

    let output = cmd.output().map_err(|e| anyhow!("curl failed: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("curl returned non-zero status");
    }

    let effective_url = String::from_utf8_lossy(&output.stdout);
    let tag = effective_url
        .trim()
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("failed to extract tag from URL"))?;

    Ok(Release {
        url: String::new(),
        body: String::new(),
        html_url: "https://github.com/tw93/Kaku/releases/latest".to_string(),
        tag_name: tag.to_string(),
        assets: vec![],
    })
}

#[allow(unused)]
pub fn get_nightly_release_info() -> anyhow::Result<Release> {
    let proxy = detect_system_proxy();
    curl_get_release_json(
        "https://api.github.com/repos/tw93/Kaku/releases/tags/nightly",
        &proxy,
    )
}

fn is_newer(latest: &str, current: &str) -> bool {
    let latest = latest.trim_start_matches('v');
    let current = current.trim_start_matches('v');

    // If latest is a WezTerm-style date version (e.g. 20240203-...) and current is SemVer (e.g. 0.1.0),
    // treat the date version as older/different system.
    if latest.starts_with("20") && latest.contains('-') && !current.starts_with("20") {
        return false;
    }

    match compare_versions(latest, current) {
        Some(CmpOrdering::Greater) => true,
        Some(_) => false,
        None => latest != current,
    }
}

fn compare_versions(left: &str, right: &str) -> Option<CmpOrdering> {
    let left = parse_version_numbers(left)?;
    let right = parse_version_numbers(right)?;
    let max_len = left.len().max(right.len());
    for idx in 0..max_len {
        let l = left.get(idx).copied().unwrap_or(0);
        let r = right.get(idx).copied().unwrap_or(0);
        match l.cmp(&r) {
            CmpOrdering::Equal => {}
            non_eq => return Some(non_eq),
        }
    }
    Some(CmpOrdering::Equal)
}

fn parse_version_numbers(version: &str) -> Option<Vec<u64>> {
    let cleaned = version.trim().trim_start_matches(['v', 'V']);
    let mut out = Vec::new();
    for part in cleaned.split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        let value = digits.parse::<u64>().ok()?;
        out.push(value);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn format_version_for_display(version: &str) -> String {
    version.trim().trim_start_matches(['v', 'V']).to_string()
}

// ---------------------------------------------------------------------------
// Background download helpers
// ---------------------------------------------------------------------------

fn find_asset<'a>(assets: &'a [Asset], name: &str) -> Option<&'a Asset> {
    assets.iter().find(|a| a.name.eq_ignore_ascii_case(name))
}

fn curl_download_to_file(
    url: &str,
    output_path: &Path,
    proxy: &Option<String>,
) -> anyhow::Result<()> {
    use std::process::Command;
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--retry")
        .arg("3")
        .arg("--connect-timeout")
        .arg("20")
        .arg("--user-agent")
        .arg(format!("kaku/{}", wezterm_version()))
        .arg("--output")
        .arg(output_path)
        .arg(url);
    apply_to_command(&mut cmd, proxy);

    let status = cmd
        .status()
        .map_err(|e| anyhow!("curl download failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("curl download returned non-zero status");
    }
    Ok(())
}

fn curl_get_text(url: &str, proxy: &Option<String>) -> anyhow::Result<String> {
    use std::process::Command;
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--retry")
        .arg("3")
        .arg("--connect-timeout")
        .arg("15")
        .arg("--user-agent")
        .arg(format!("kaku/{}", wezterm_version()))
        .arg(url);
    apply_to_command(&mut cmd, proxy);

    let out = cmd
        .output()
        .map_err(|e| anyhow!("curl get text failed: {}", e))?;
    if !out.status.success() {
        anyhow::bail!(
            "curl get text failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8(out.stdout).map_err(|e| anyhow!("non-utf8 response: {}", e))
}

fn verify_sha256(zip_path: &Path, checksum_text: &str) -> anyhow::Result<()> {
    use std::process::Command;

    let expected = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("checksum file is empty"))?
        .trim()
        .to_ascii_lowercase();

    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("checksum file has invalid sha256: {}", expected);
    }

    let output = Command::new("/usr/bin/shasum")
        .arg("-a")
        .arg("256")
        .arg(zip_path)
        .output()
        .map_err(|e| anyhow!("shasum failed: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("shasum returned non-zero status");
    }

    let actual_line = String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("shasum output is not valid UTF-8"))?;
    let actual = actual_line
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("failed to parse shasum output"))?
        .trim()
        .to_ascii_lowercase();

    if actual != expected {
        anyhow::bail!("sha256 mismatch (expected {}, got {})", expected, actual);
    }
    Ok(())
}

fn find_kaku_app(extracted_dir: &Path) -> Option<PathBuf> {
    let direct = extracted_dir.join("Kaku.app");
    if direct.exists() {
        return Some(direct);
    }

    let entries = std::fs::read_dir(extracted_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("Kaku.app"))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

fn read_app_version(app_path: &Path) -> anyhow::Result<String> {
    use std::process::Command;
    let plist = app_path.join("Contents/Info.plist");
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :CFBundleShortVersionString")
        .arg(&plist)
        .output()
        .map_err(|e| anyhow!("PlistBuddy failed: {}", e))?;
    if !output.status.success() {
        anyhow::bail!("PlistBuddy returned non-zero status");
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("version is not valid UTF-8"))?
        .trim()
        .to_string();
    if version.is_empty() {
        anyhow::bail!("downloaded app version is empty");
    }
    Ok(version)
}

/// Download, verify, extract, and stage an update in the background.
/// Returns the StagedUpdateInfo on success.
fn download_and_stage_update(
    release: &Release,
    proxy: &Option<String>,
) -> anyhow::Result<StagedUpdateInfo> {
    // Hold an exclusive lock for the whole staging operation so two Kaku
    // processes cannot trample each other's `staged_update/` directory.
    let _lock = StagedUpdateLock::try_acquire()?;

    let dir = staged_dir();

    // Clean any previous staged update.
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| anyhow!("failed to remove old staged update: {}", e))?;
    }
    config::create_user_owned_dirs(&dir)
        .map_err(|e| anyhow!("failed to create staged update dir: {}", e))?;

    let zip_url = find_asset(&release.assets, UPDATE_ZIP_NAME)
        .map(|a| a.browser_download_url.as_str())
        .unwrap_or(LATEST_ZIP_URL);

    let sha_url = find_asset(&release.assets, UPDATE_SHA_NAME)
        .map(|a| a.browser_download_url.as_str())
        .unwrap_or(LATEST_SHA_URL);

    // 1. Download zip
    let zip_path = dir.join(UPDATE_ZIP_NAME);
    log::info!("staged update: downloading {} ...", UPDATE_ZIP_NAME);
    curl_download_to_file(zip_url, &zip_path, proxy)?;

    // 2. Verify SHA256
    log::info!("staged update: verifying checksum...");
    let checksum_text = curl_get_text(sha_url, proxy)?;
    verify_sha256(&zip_path, &checksum_text)?;

    // 3. Extract
    let extracted_dir = dir.join("extracted");
    config::create_user_owned_dirs(&extracted_dir)
        .map_err(|e| anyhow!("failed to create extraction dir: {}", e))?;

    let ditto_status = std::process::Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(&zip_path)
        .arg(&extracted_dir)
        .status()
        .map_err(|e| anyhow!("ditto failed: {}", e))?;
    if !ditto_status.success() {
        anyhow::bail!("ditto extraction failed");
    }

    // 4. Find and verify the extracted app
    let new_app_path = find_kaku_app(&extracted_dir)
        .ok_or_else(|| anyhow!("update package does not contain Kaku.app"))?;

    let new_version = read_app_version(&new_app_path)?;
    let current = wezterm_version();
    if !is_newer(&new_version, current) {
        anyhow::bail!(
            "staged app version {} is not newer than current {}",
            new_version,
            current
        );
    }

    // 5. Remove the zip to save disk space
    let _ = std::fs::remove_file(&zip_path);

    // 6. Write metadata
    let info = StagedUpdateInfo {
        tag: release.tag_name.clone(),
        body: release.body.clone(),
        staged_at: now_unix_secs(),
        verified: true,
        app_path: new_app_path.to_string_lossy().into_owned(),
    };
    let meta_json = serde_json::to_string_pretty(&info)
        .map_err(|e| anyhow!("failed to serialize metadata: {}", e))?;
    std::fs::write(staged_meta_path(), meta_json)
        .map_err(|e| anyhow!("failed to write metadata: {}", e))?;

    log::info!(
        "staged update: {} ready at {}",
        info.tag,
        new_app_path.display()
    );
    Ok(info)
}

// ---------------------------------------------------------------------------
// Update checker loop
// ---------------------------------------------------------------------------

fn update_checker() {
    log::info!("update_checker thread started");

    let initial_interval = Duration::from_secs(3);
    let force_ui = std::env::var_os("KAKU_ALWAYS_SHOW_UPDATE_UI").is_some();

    let update_file_name = config::DATA_DIR.join("check_update");

    // On startup, clean orphaned or invalid staged updates.
    // Acquire the staging lock first so we don't race with another instance
    // that is mid-download. If the lock is held, skip cleanup silently.
    if staged_update_available().is_none() {
        let dir = staged_dir();
        if dir.exists() {
            match StagedUpdateLock::try_acquire() {
                Ok(_lock) => {
                    log::info!("removing orphaned or invalid staged update directory");
                    let _ = std::fs::remove_dir_all(&dir);
                }
                Err(_) => {
                    log::info!("skipping startup cleanup: another process holds the staging lock");
                }
            }
        }
    }

    // Check if we already know about a newer version from the cached file.
    // If so, show notification immediately without waiting.
    // Respect check_for_updates so disabled users don't get startup notifications.
    if configuration().check_for_updates {
        if let Ok(content) = std::fs::read_to_string(&update_file_name) {
            if let Ok(cached_release) = serde_json::from_str::<Release>(&content) {
                let current = wezterm_version();
                if is_newer(&cached_release.tag_name, current) {
                    log::info!(
                        "update_checker: cached release {} is newer than current {}, showing notification",
                        cached_release.tag_name,
                        current
                    );
                    std::thread::sleep(initial_interval);
                    let my_sock =
                        config::RUNTIME_DIR.join(format!("gui-sock-{}", unsafe { libc::getpid() }));
                    let socks = wezterm_client::discovery::discover_gui_socks();
                    if force_ui || socks.is_empty() || socks.first() == Some(&my_sock) {
                        show_update_notification(&cached_release.tag_name);
                    }
                }
            }
        }
    }

    // Compute how long we should sleep for;
    // if we've never checked, give it a few seconds after the first
    // launch, otherwise compute the interval based on the time of
    // the last check.
    let update_interval = Duration::from_secs(configuration().check_for_updates_interval_seconds);

    let delay = update_file_name
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map_err(|_| ())
        .and_then(|systime| {
            let elapsed = systime.elapsed().unwrap_or(Duration::new(0, 0));
            update_interval.checked_sub(elapsed).ok_or(())
        })
        .unwrap_or(initial_interval);

    log::info!(
        "update_checker: sleeping for {:?}",
        if force_ui { initial_interval } else { delay }
    );
    std::thread::sleep(if force_ui { initial_interval } else { delay });
    log::info!("update_checker: woke up, starting check loop");

    let my_sock = config::RUNTIME_DIR.join(format!("gui-sock-{}", unsafe { libc::getpid() }));

    loop {
        // Figure out which other wezterm-guis are running.
        // We have a little "consensus protocol" to decide which
        // of us will show the toast notification or show the update
        // window: the one of us that sorts first in the list will
        // own doing that, so that if there are a dozen gui processes
        // running, we don't spam the user with a lot of notifications.
        let socks = wezterm_client::discovery::discover_gui_socks();

        log::info!(
            "update_checker: check_for_updates={}",
            configuration().check_for_updates
        );
        if configuration().check_for_updates {
            log::info!("update_checker: fetching release info...");
            match get_latest_release_info() {
                Ok(latest) => {
                    log::info!("update_checker: got release {}", latest.tag_name);
                    let current = wezterm_version();
                    if is_newer(&latest.tag_name, current) || force_ui {
                        log::info!(
                            "latest release {} is newer than current build {}",
                            latest.tag_name,
                            current
                        );

                        // If we already have this version staged, just show the notification.
                        let already_staged = staged_update_available()
                            .map(|s| s.tag == latest.tag_name)
                            .unwrap_or(false);

                        if !already_staged {
                            // Download and stage the update in the background.
                            let proxy = detect_system_proxy();
                            match download_and_stage_update(&latest, &proxy) {
                                Ok(info) => {
                                    log::info!("update_checker: staged update {} ready", info.tag);
                                }
                                Err(e) => {
                                    log::warn!("update_checker: failed to stage update: {}", e);
                                    // Only clean up if we actually owned the staging dir.
                                    // On EWOULDBLOCK another process holds the lock and its
                                    // directory must not be touched.
                                    if !e.to_string().contains("already staging") {
                                        cleanup_staged_update();
                                    }
                                    // Fall through to show notification anyway
                                    // (clicking will use the old terminal-tab flow).
                                }
                            }
                        }

                        log::info!("update_checker: socks={:?}, my_sock={:?}", socks, my_sock);
                        if force_ui || socks.is_empty() || socks[0] == my_sock {
                            log::info!("update_checker: showing notification");
                            show_update_notification(&latest.tag_name);
                        } else {
                            log::info!(
                                "update_checker: skipping notification (not primary instance)"
                            );
                        }
                    }

                    if let Some(parent) = update_file_name.parent() {
                        config::create_user_owned_dirs(parent).ok();
                    }

                    // Record the time of this check
                    if let Ok(f) = std::fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&update_file_name)
                    {
                        serde_json::to_writer_pretty(f, &latest).ok();
                    }
                }
                Err(e) => {
                    log::warn!("update_checker: failed to get release info: {}", e);
                }
            }
        }

        std::thread::sleep(Duration::from_secs(
            configuration().check_for_updates_interval_seconds,
        ));
    }
}

/// Show the appropriate update notification depending on whether a staged
/// update is ready or not.
fn show_update_notification(tag: &str) {
    let version = format_version_for_display(tag);
    if staged_update_available().is_some() {
        persistent_toast_notification_with_click_to_open_url(
            "Update Ready",
            &format!("{} is ready. Click to restart and update.", version),
            "kaku://update",
        );
    } else {
        persistent_toast_notification_with_click_to_open_url(
            "Update Available",
            &format!("{} is available. Click to update.", version),
            "kaku://update",
        );
    }
}

pub fn start_update_checker() {
    static CHECKER_STARTED: AtomicBool = AtomicBool::new(false);
    if let Ok(false) =
        CHECKER_STARTED.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
    {
        // Initialize the notification system up front so macOS shows the
        // permission dialog on first launch, rather than lazily when a
        // notification fires. This runs just after the first paint (see
        // paint_impl) so the init cost stays off the first-frame path.
        wezterm_toast_notification::macos_initialize();

        // Register callback so a notification click asks for confirmation
        // before doing anything destructive. Applying an update closes every
        // window and stops running tasks, so a stray click must not trigger it
        // silently. Menu "Restart to Update" and staged "Check for Updates"
        // share the same confirm path. The confirmed action picks the staged
        // fast-path or the terminal-tab flow inside apply_update_now().
        wezterm_toast_notification::set_update_click_callback(|| {
            crate::frontend::confirm_and_apply_update();
        });

        // Check if we just completed an update and show notification
        check_update_completed();

        std::thread::Builder::new()
            .name("update_checker".into())
            .spawn(update_checker)
            .expect("failed to spawn update checker thread");
    }
}

fn check_update_completed() {
    let marker_file = config::DATA_DIR.join("update_completed");
    if !marker_file.exists() {
        return;
    }

    // Check if marker file is recent (within last 5 minutes)
    // This prevents showing stale notifications from old failed updates
    let is_recent = marker_file
        .metadata()
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().map(|e| e.as_secs() < 300).unwrap_or(false))
        .unwrap_or(false);

    if is_recent {
        if let Ok(version) = std::fs::read_to_string(&marker_file) {
            let version = version.trim();
            if !version.is_empty() {
                log::info!("update_completed: showing notification for {}", version);
                wezterm_toast_notification::persistent_toast_notification(
                    "Updated",
                    &format!("Successfully updated to {}.", version),
                );
            }
        }
    } else {
        log::info!("update_completed: skipping stale marker file");
    }

    // Always remove the marker file, and also clean up the staged_update dir
    // since the update succeeded.
    let _ = std::fs::remove_file(&marker_file);
    cleanup_staged_update();
}

// ---------------------------------------------------------------------------
// Helper script for restart-to-update (duplicated from kaku/src/update.rs
// because kaku-gui does not depend on the kaku crate).
// ---------------------------------------------------------------------------

/// Resolve the installed Kaku.app path, matching the logic in the CLI crate.
pub fn resolve_target_app_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("KAKU_UPDATE_TARGET_APP") {
        let app = PathBuf::from(path);
        if app.ends_with("Kaku.app") {
            return Ok(app);
        }
        anyhow::bail!("KAKU_UPDATE_TARGET_APP must point to Kaku.app");
    }

    let exe = std::env::current_exe().map_err(|e| anyhow!("resolve current executable: {}", e))?;
    for ancestor in exe.ancestors() {
        if ancestor
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("Kaku.app"))
            .unwrap_or(false)
        {
            return Ok(ancestor.to_path_buf());
        }
    }

    let default_app = PathBuf::from("/Applications/Kaku.app");
    if default_app.exists() {
        return Ok(default_app);
    }

    anyhow::bail!("cannot locate installed Kaku.app")
}

pub fn write_update_helper_script(script_path: &Path) -> anyhow::Result<()> {
    // This is the same helper script as kaku/src/update.rs::write_helper_script.
    let script = include_str!("../../scripts/update_helper.sh");
    std::fs::write(script_path, script)
        .map_err(|e| anyhow!("failed to write helper script: {}", e))?;
    let status = std::process::Command::new("/bin/chmod")
        .arg("700")
        .arg(script_path)
        .status()
        .map_err(|e| anyhow!("chmod failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("chmod helper script failed");
    }
    Ok(())
}

pub fn spawn_update_helper(
    script: &Path,
    target_app: &Path,
    new_app: &Path,
    work_dir: &Path,
) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    // Validate paths end with Kaku.app.
    if !target_app.ends_with("Kaku.app") {
        anyhow::bail!(
            "target_app must end with Kaku.app: {}",
            target_app.display()
        );
    }
    if !new_app.ends_with("Kaku.app") {
        anyhow::bail!("new_app must end with Kaku.app: {}", new_app.display());
    }

    Command::new("/usr/bin/nohup")
        .arg("/bin/bash")
        .arg(script)
        .arg(target_app)
        .arg(new_app)
        .arg(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Kaku.app runs as a launchd job, and launchd kills what is left of
        // its process group when kaku-gui exits: the helper stops Kaku first,
        // so in Kaku's group it died before replacing the app (#591). The
        // group only covers the moment until the helper hands itself to its
        // own launchd job; see scripts/update_helper.sh.
        .process_group(0)
        .spawn()
        .map_err(|e| anyhow!("failed to spawn update helper: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Kaku.app launched from Finder is a launchd job whose process group
    // launchd kills once kaku-gui exits, so the helper must lead its own group
    // or it dies right after stopping Kaku (#591).
    #[test]
    fn update_helper_leads_its_own_process_group() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let script = tmp.path().join("probe.sh");
        let out = tmp.path().join("pgid");
        std::fs::write(
            &script,
            "printf '%s %s\\n' \"$$\" \"$(ps -o pgid= -p $$ | tr -d ' ')\" > \"$3/pgid.tmp\"\nmv \"$3/pgid.tmp\" \"$3/pgid\"\n",
        )
        .unwrap();
        let app = tmp.path().join("Kaku.app");
        spawn_update_helper(&script, &app, &app, tmp.path()).expect("spawn helper");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !out.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&out).expect("helper wrote its pid and pgid");
        let ids: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "unexpected probe output: {text:?}");
        assert_eq!(ids[0], ids[1], "helper must lead its own process group");
    }

    #[test]
    fn semver_numeric_comparison() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.2.0", "0.11.0"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(is_newer("v0.1.2", "0.1.1"));
    }

    #[test]
    fn version_0_9_is_newer_than_0_8() {
        assert!(is_newer("V0.9.0", "0.8.0"));
        assert!(is_newer("0.9.0", "0.8.0"));
        assert!(!is_newer("0.8.0", "0.9.0"));
    }

    #[test]
    fn staged_metadata_roundtrip() {
        let info = StagedUpdateInfo {
            tag: "V0.9.0".to_string(),
            body: "some release notes".to_string(),
            staged_at: 1700000000,
            verified: true,
            app_path: "/tmp/test/Kaku.app".to_string(),
        };
        let json = serde_json::to_string_pretty(&info).unwrap();
        let parsed: StagedUpdateInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tag, "V0.9.0");
        assert!(parsed.verified);
        assert_eq!(parsed.app_path, "/tmp/test/Kaku.app");
    }

    #[test]
    fn format_version_strips_prefix() {
        assert_eq!(format_version_for_display("V0.9.0"), "0.9.0");
        assert_eq!(format_version_for_display("v1.2.3"), "1.2.3");
        assert_eq!(format_version_for_display("0.8.0"), "0.8.0");
    }

    /// End-to-end test: fetch real release from GitHub, download, verify,
    /// extract, and stage. Requires network access.
    /// Run with: cargo nextest run -p kaku-gui staged_update_e2e -- --ignored
    #[test]
    #[ignore]
    fn staged_update_e2e() {
        use std::fs;

        // Use a temp dir to avoid polluting the real data dir.
        let tmp = tempfile::tempdir().expect("create tempdir");
        let staged = tmp.path().join("staged_update");
        fs::create_dir_all(&staged).unwrap();

        let proxy = config::proxy::detect_system_proxy();

        // Fetch real release info
        let release = get_latest_release_info().expect("fetch release info");
        println!("release tag: {}", release.tag_name);
        println!(
            "release body (first 100 chars): {}",
            &release.body[..release.body.len().min(100)]
        );
        assert!(!release.tag_name.is_empty());

        // Simulate being on 0.8.0
        assert!(
            is_newer(&release.tag_name, "0.8.0"),
            "latest should be newer than 0.8.0"
        );

        // Download zip
        let zip_url = find_asset(&release.assets, UPDATE_ZIP_NAME)
            .map(|a| a.browser_download_url.as_str())
            .unwrap_or(LATEST_ZIP_URL);
        let zip_path = staged.join(UPDATE_ZIP_NAME);
        println!("downloading from: {}", zip_url);
        curl_download_to_file(zip_url, &zip_path, &proxy).expect("download zip");
        assert!(zip_path.exists(), "zip should exist after download");
        let zip_size = fs::metadata(&zip_path).unwrap().len();
        println!("downloaded zip size: {} bytes", zip_size);
        assert!(zip_size > 1_000_000, "zip should be at least 1MB");

        // Verify SHA256
        let sha_url = find_asset(&release.assets, UPDATE_SHA_NAME)
            .map(|a| a.browser_download_url.as_str())
            .unwrap_or(LATEST_SHA_URL);
        let checksum_text = curl_get_text(sha_url, &proxy).expect("fetch checksum");
        println!("checksum: {}", checksum_text.trim());
        verify_sha256(&zip_path, &checksum_text).expect("sha256 verification");
        println!("SHA256 verified OK");

        // Extract
        let extracted = staged.join("extracted");
        fs::create_dir_all(&extracted).unwrap();
        let ditto_status = std::process::Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(&zip_path)
            .arg(&extracted)
            .status()
            .expect("ditto");
        assert!(ditto_status.success(), "ditto extraction should succeed");

        // Find Kaku.app
        let app_path = find_kaku_app(&extracted).expect("should find Kaku.app");
        println!("found app: {}", app_path.display());
        assert!(app_path.exists());

        // Read version from extracted app
        let app_version = read_app_version(&app_path).expect("read app version");
        println!("extracted app version: {}", app_version);
        assert!(
            is_newer(&app_version, "0.8.0"),
            "extracted app should be newer than 0.8.0"
        );

        // Write metadata
        let meta = StagedUpdateInfo {
            tag: release.tag_name.clone(),
            body: release.body.clone(),
            staged_at: now_unix_secs(),
            verified: true,
            app_path: app_path.to_string_lossy().into_owned(),
        };
        let meta_path = staged.join(STAGED_META_NAME);
        let meta_json = serde_json::to_string_pretty(&meta).unwrap();
        fs::write(&meta_path, &meta_json).unwrap();

        // Read it back
        let read_back: StagedUpdateInfo =
            serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(read_back.tag, release.tag_name);
        assert!(read_back.verified);

        println!("\n=== E2E staged update test PASSED ===");
        println!(
            "tag={}, version={}, staged_at={}",
            read_back.tag, app_version, read_back.staged_at
        );
    }
}
