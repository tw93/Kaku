use anyhow::{anyhow, bail, Context};
use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
pub struct UpdateCommand {}

impl UpdateCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        imp::run()
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::bail;

    pub fn run() -> anyhow::Result<()> {
        bail!("`kaku update` is currently supported on macOS only")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use serde::Deserialize;
    use std::cmp::Ordering;
    use std::fs;
    use std::io::Write;
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    const RELEASE_API_URL: &str = "https://api.github.com/repos/tw93/Kaku/releases/latest";
    const LATEST_ZIP_URL: &str =
        "https://github.com/tw93/Kaku/releases/latest/download/kaku_for_update.zip";
    const LATEST_SHA_URL: &str =
        "https://github.com/tw93/Kaku/releases/latest/download/kaku_for_update.zip.sha256";
    const RELEASE_LATEST_URL: &str = "https://github.com/tw93/Kaku/releases/latest";
    const UPDATE_ZIP_NAME: &str = "kaku_for_update.zip";
    const UPDATE_SHA_NAME: &str = "kaku_for_update.zip.sha256";
    const BREW_CASK_NAME: &str = "tw93/tap/kakuterm";

    #[derive(Debug, Deserialize)]
    struct GitHubRelease {
        tag_name: String,
        assets: Vec<GitHubAsset>,
    }

    #[derive(Debug, Deserialize)]
    struct GitHubAsset {
        name: String,
        browser_download_url: String,
    }

    struct BrewInfo {
        brew_bin: PathBuf,
        cask_name: String,
    }

    enum UpdateProvider {
        Direct,
        Brew(BrewInfo),
    }

    pub fn run() -> anyhow::Result<()> {
        match resolve_update_provider()? {
            UpdateProvider::Brew(info) => {
                println!("Detected Homebrew-managed installation. Using brew upgrade...");
                return run_brew_upgrade(&info);
            }
            UpdateProvider::Direct => {}
        }

        let current_version = config::wezterm_version().to_string();
        let current_version_display = format_version_for_display(&current_version);
        println!("Current version: {}", current_version_display);
        println!("Checking latest release...");

        let release = match curl_get_text(RELEASE_API_URL, &current_version)
            .context("request release metadata")
            .and_then(|raw| {
                serde_json::from_str::<GitHubRelease>(&raw).context("parse release metadata")
            }) {
            Ok(release) => Some(release),
            Err(err) => {
                println!(
                    "Release API unavailable ({}). Falling back to latest asset URL.",
                    err
                );
                None
            }
        };

        if let Some(release) = &release {
            if !is_newer_version(&release.tag_name, &current_version) {
                println!(
                    "Already up to date. Current={} Latest={}",
                    current_version_display,
                    format_version_for_display(&release.tag_name)
                );
                return Ok(());
            }
        } else if let Some(tag_name) = resolve_latest_tag_from_redirect(&current_version)? {
            if !is_newer_version(&tag_name, &current_version) {
                println!(
                    "Already up to date. Current={} Latest={}",
                    current_version_display,
                    format_version_for_display(&tag_name)
                );
                return Ok(());
            }
        }

        let zip_url = release
            .as_ref()
            .and_then(|rel| find_asset(&rel.assets, UPDATE_ZIP_NAME))
            .map(|asset| asset.browser_download_url.as_str())
            .unwrap_or(LATEST_ZIP_URL);

        let sha_url = release
            .as_ref()
            .and_then(|rel| find_asset(&rel.assets, UPDATE_SHA_NAME))
            .map(|asset| asset.browser_download_url.as_str())
            .or(Some(LATEST_SHA_URL));

        let update_root = config::DATA_DIR.join("updates");
        config::create_user_owned_dirs(&update_root).context("create updates directory")?;

        let tag = release
            .as_ref()
            .map(|r| sanitize_tag(&r.tag_name))
            .unwrap_or_else(|| "latest".to_string());
        let now = now_unix_seconds();
        let work_dir = update_root.join(format!("{}-{}", tag, now));
        config::create_user_owned_dirs(&work_dir).context("create update work directory")?;

        let zip_path = work_dir.join(UPDATE_ZIP_NAME);
        println!("Downloading {} ...", UPDATE_ZIP_NAME);
        curl_download_to_file(zip_url, &zip_path, &current_version)
            .context("failed to download update package")?;

        if let Some(sha_url) = sha_url {
            match curl_get_text(sha_url, &current_version) {
                Ok(checksum_text) => {
                    println!("Verifying package checksum...");
                    verify_sha256(&zip_path, &checksum_text)
                        .context("checksum verification failed")?;
                }
                Err(err) => {
                    println!(
                        "Checksum unavailable ({}). Continuing without checksum.",
                        err
                    );
                }
            }
        }

        let extracted_dir = work_dir.join("extracted");
        config::create_user_owned_dirs(&extracted_dir).context("create extraction directory")?;

        run_status(
            Command::new("/usr/bin/ditto")
                .arg("-x")
                .arg("-k")
                .arg(&zip_path)
                .arg(&extracted_dir),
            "extract update package",
        )?;

        let new_app_path = find_kaku_app(&extracted_dir).ok_or_else(|| {
            anyhow!(
                "update package `{}` does not contain `Kaku.app`",
                UPDATE_ZIP_NAME
            )
        })?;
        if let Ok(new_version) = read_app_version(&new_app_path) {
            if !is_newer_version(&new_version, &current_version) {
                println!(
                    "Already up to date after download. Current={} Package={}",
                    current_version_display,
                    format_version_for_display(&new_version)
                );
                let _ = fs::remove_dir_all(&work_dir);
                return Ok(());
            }
        }

        let target_app = resolve_target_app_path().context("resolve installed Kaku.app path")?;
        ensure_can_write_target(&target_app)?;

        let helper_script = update_root.join(format!("apply-update-{}.sh", now));
        write_helper_script(&helper_script).context("write update helper script")?;

        spawn_update_helper(&helper_script, &target_app, &new_app_path, &work_dir)
            .context("spawn update helper")?;

        let update_label = release
            .as_ref()
            .map(|r| r.tag_name.as_str())
            .unwrap_or("latest");
        println!(
            "Update to {} has started in background.",
            format_version_for_display(update_label)
        );
        println!("Kaku will quit and relaunch automatically when replacement is complete.");
        Ok(())
    }

    fn resolve_update_provider() -> anyhow::Result<UpdateProvider> {
        if let Some(provider) = std::env::var_os("KAKU_UPDATE_PROVIDER") {
            let provider = provider.to_string_lossy().to_ascii_lowercase();
            return match provider.as_str() {
                "brew" => {
                    let brew_info = resolve_brew_info()?.ok_or_else(|| {
                        anyhow!(
                            "KAKU_UPDATE_PROVIDER=brew but brew-managed Kaku installation was not found"
                        )
                    })?;
                    Ok(UpdateProvider::Brew(brew_info))
                }
                "direct" => Ok(UpdateProvider::Direct),
                other => bail!("invalid KAKU_UPDATE_PROVIDER `{}`", other),
            };
        }

        if let Some(brew_info) = resolve_brew_info()? {
            return Ok(UpdateProvider::Brew(brew_info));
        }

        let exe = std::env::current_exe().context("resolve current executable path")?;
        if let Some(target) = std::env::var_os("KAKU_UPDATE_TARGET_APP") {
            let target = PathBuf::from(target);
            if path_contains_caskroom(&exe) || path_contains_caskroom(&target) {
                if find_brew_binary().is_none() {
                    bail!(
                        "Kaku appears to be Homebrew-managed but `brew` was not found in PATH or standard locations"
                    );
                }
            }
        }

        Ok(UpdateProvider::Direct)
    }

    fn resolve_brew_info() -> anyhow::Result<Option<BrewInfo>> {
        let Some(brew_bin) = find_brew_binary() else {
            return Ok(None);
        };

        if is_brew_cask_installed(&brew_bin, BREW_CASK_NAME)? {
            return Ok(Some(BrewInfo {
                brew_bin,
                cask_name: BREW_CASK_NAME.to_string(),
            }));
        }

        // Old cask name "kaku" conflicts with another software in homebrew/cask.
        // Do not use it; prompt user to migrate instead.
        if is_brew_cask_installed(&brew_bin, "kaku")? {
            println!("WARNING: Detected old Homebrew cask 'kaku' which conflicts with another software.");
            println!("Please migrate to the new cask name manually:");
            println!();
            println!("  brew uninstall --cask kaku");
            println!("  brew install --cask {}", BREW_CASK_NAME);
            println!();
            println!("After migration, run 'kaku update' again.");
            // Return None to fall back to direct update from GitHub
            return Ok(None);
        }

        Ok(None)
    }

    fn find_brew_binary() -> Option<PathBuf> {
        for candidate in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                return Some(path);
            }
        }

        std::env::var_os("PATH").and_then(|path_var| {
            std::env::split_paths(&path_var)
                .map(|dir| dir.join("brew"))
                .find(|candidate| candidate.exists())
        })
    }

    fn path_contains_caskroom(path: &Path) -> bool {
        path.components().any(|c| match c {
            Component::Normal(name) => name == "Caskroom",
            _ => false,
        })
    }

    fn is_brew_cask_installed(brew_bin: &Path, cask_name: &str) -> anyhow::Result<bool> {
        let output = Command::new(brew_bin)
            .arg("list")
            .arg("--cask")
            .arg("--versions")
            .arg(cask_name)
            .output()
            .with_context(|| format!("query brew cask installation for {}", cask_name))?;

        if output.status.success() {
            return Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if stderr.contains("no such cask")
            || stderr.contains("not installed")
            || output.status.code() == Some(1)
        {
            return Ok(false);
        }

        bail!(
            "query brew cask installation for {} failed: {}",
            cask_name,
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }

    fn is_brew_cask_outdated(brew_bin: &Path, cask_name: &str) -> anyhow::Result<bool> {
        let output = run_output(
            Command::new(brew_bin)
                .arg("outdated")
                .arg("--cask")
                .arg("--quiet")
                .arg(cask_name),
            &format!("query brew cask outdated status for {}", cask_name),
        )?;
        Ok(!String::from_utf8_lossy(&output).trim().is_empty())
    }

    fn run_brew_upgrade(info: &BrewInfo) -> anyhow::Result<()> {
        match is_brew_cask_outdated(&info.brew_bin, &info.cask_name) {
            Ok(false) => {
                println!(
                    "Already up to date (brew cask `{}` has no available update).",
                    info.cask_name
                );
                return Ok(());
            }
            Ok(true) => {}
            Err(err) => {
                println!(
                    "Unable to pre-check brew outdated status ({}). Trying upgrade directly.",
                    err
                );
            }
        }

        let primary = Command::new(&info.brew_bin)
            .arg("upgrade")
            .arg("--cask")
            .arg(&info.cask_name)
            .status()
            .with_context(|| format!("failed to run brew upgrade for {}", info.cask_name))?;
        if primary.success() {
            return Ok(());
        }

        let fallback_name = if info.cask_name == BREW_CASK_NAME {
            "kaku"
        } else {
            BREW_CASK_NAME
        };

        let fallback = Command::new(&info.brew_bin)
            .arg("upgrade")
            .arg("--cask")
            .arg(fallback_name)
            .status()
            .with_context(|| {
                format!("failed to run brew upgrade fallback for {}", fallback_name)
            })?;
        if fallback.success() {
            return Ok(());
        }

        bail!(
            "brew update failed (tried `brew upgrade --cask {}` and `brew upgrade --cask {}`)",
            info.cask_name,
            fallback_name
        )
    }

    fn resolve_latest_tag_from_redirect(current_version: &str) -> anyhow::Result<Option<String>> {
        let output = run_output(
            Command::new("/usr/bin/curl")
                .arg("--fail")
                .arg("--location")
                .arg("--silent")
                .arg("--show-error")
                .arg("--retry")
                .arg("2")
                .arg("--connect-timeout")
                .arg("10")
                .arg("--user-agent")
                .arg(format!("kaku/{}", current_version))
                .arg("--write-out")
                .arg("%{url_effective}")
                .arg("--output")
                .arg("/dev/null")
                .arg(RELEASE_LATEST_URL),
            "resolve latest release tag via redirect",
        )?;
        let effective_url = String::from_utf8(output)
            .context("latest redirect url is not valid UTF-8")?
            .trim()
            .to_string();
        if effective_url.is_empty() {
            return Ok(None);
        }

        let tag = effective_url
            .rsplit('/')
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(tag)
    }

    fn find_asset<'a>(assets: &'a [GitHubAsset], name: &str) -> Option<&'a GitHubAsset> {
        assets.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    fn sanitize_tag(tag: &str) -> String {
        tag.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn now_unix_seconds() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn curl_get_text(url: &str, current_version: &str) -> anyhow::Result<String> {
        let output = run_output(
            Command::new("/usr/bin/curl")
                .arg("--fail")
                .arg("--location")
                .arg("--silent")
                .arg("--show-error")
                .arg("--retry")
                .arg("3")
                .arg("--connect-timeout")
                .arg("15")
                .arg("--user-agent")
                .arg(format!("kaku/{}", current_version))
                .arg(url),
            "request update metadata",
        )?;
        String::from_utf8(output).context("curl returned non-utf8 response")
    }

    fn curl_download_to_file(
        url: &str,
        output_path: &Path,
        current_version: &str,
    ) -> anyhow::Result<()> {
        run_status(
            Command::new("/usr/bin/curl")
                .arg("--fail")
                .arg("--location")
                .arg("--silent")
                .arg("--show-error")
                .arg("--retry")
                .arg("3")
                .arg("--connect-timeout")
                .arg("20")
                .arg("--user-agent")
                .arg(format!("kaku/{}", current_version))
                .arg("--output")
                .arg(output_path)
                .arg(url),
            "download update package",
        )
    }

    fn verify_sha256(zip_path: &Path, checksum_text: &str) -> anyhow::Result<()> {
        let expected = checksum_text
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow!("checksum file is empty"))?
            .trim()
            .to_ascii_lowercase();

        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("checksum file has invalid sha256: {}", expected);
        }

        let output = run_output(
            Command::new("/usr/bin/shasum")
                .arg("-a")
                .arg("256")
                .arg(zip_path),
            "compute sha256",
        )?;
        let actual_line =
            String::from_utf8(output).context("`shasum` output was not valid UTF-8")?;
        let actual = actual_line
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow!("failed to parse `shasum` output"))?
            .trim()
            .to_ascii_lowercase();

        if actual != expected {
            bail!("sha256 mismatch (expected {}, got {})", expected, actual);
        }
        Ok(())
    }

    fn find_kaku_app(extracted_dir: &Path) -> Option<PathBuf> {
        let direct = extracted_dir.join("Kaku.app");
        if direct.exists() {
            return Some(direct);
        }

        let entries = fs::read_dir(extracted_dir).ok()?;
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
        let plist = app_path.join("Contents/Info.plist");
        let output = run_output(
            Command::new("/usr/libexec/PlistBuddy")
                .arg("-c")
                .arg("Print :CFBundleShortVersionString")
                .arg(&plist),
            "read downloaded app version",
        )?;
        let version = String::from_utf8(output)
            .context("downloaded app version is not valid UTF-8")?
            .trim()
            .to_string();
        if version.is_empty() {
            bail!("downloaded app version is empty");
        }
        Ok(version)
    }

    fn resolve_target_app_path() -> anyhow::Result<PathBuf> {
        if let Some(path) = std::env::var_os("KAKU_UPDATE_TARGET_APP") {
            let app = PathBuf::from(path);
            if app.ends_with("Kaku.app") {
                return Ok(app);
            }
            bail!("KAKU_UPDATE_TARGET_APP must point to Kaku.app");
        }

        let exe = std::env::current_exe().context("resolve current executable")?;
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

        bail!("cannot locate installed Kaku.app; run this from installed Kaku")
    }

    fn ensure_can_write_target(target_app: &Path) -> anyhow::Result<()> {
        let parent = target_app
            .parent()
            .ok_or_else(|| anyhow!("invalid app path: {}", target_app.display()))?;
        if !parent.exists() {
            bail!(
                "install location does not exist: {}",
                parent.as_os_str().to_string_lossy()
            );
        }

        let test_file = parent.join(format!(".kaku-update-write-test-{}", now_unix_seconds()));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&test_file)
        {
            Ok(mut f) => {
                let _ = f.write_all(b"ok");
                let _ = fs::remove_file(test_file);
                Ok(())
            }
            Err(err) => bail!(
                "no write permission in {} ({})",
                parent.as_os_str().to_string_lossy(),
                err
            ),
        }
    }

    fn write_helper_script(script_path: &Path) -> anyhow::Result<()> {
        let script = r#"#!/bin/bash
set -euo pipefail

TARGET_APP="$1"
NEW_APP="$2"
WORK_DIR="$3"
LOG_FILE="$WORK_DIR/update.log"
BACKUP_APP="${TARGET_APP}.backup.$(date +%s)"
TARGET_GUI="$TARGET_APP/Contents/MacOS/kaku-gui"
TARGET_CLI="$TARGET_APP/Contents/MacOS/kaku"

log() {
  printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >>"$LOG_FILE"
}

rollback() {
  log "restore from backup"
  /bin/rm -rf "$TARGET_APP" || true
  if [[ -d "$BACKUP_APP" ]]; then
    /bin/mv "$BACKUP_APP" "$TARGET_APP" || true
  fi
}

log "start apply update"

for _ in $(seq 1 20); do
  if /usr/bin/pgrep -f "$TARGET_GUI" >/dev/null 2>&1 || /usr/bin/pgrep -f "$TARGET_CLI" >/dev/null 2>&1; then
    /usr/bin/pkill -TERM -f "$TARGET_GUI" >/dev/null 2>&1 || true
    /usr/bin/pkill -TERM -f "$TARGET_CLI" >/dev/null 2>&1 || true
    sleep 1
  else
    break
  fi
done

/usr/bin/pkill -KILL -f "$TARGET_GUI" >/dev/null 2>&1 || true
/usr/bin/pkill -KILL -f "$TARGET_CLI" >/dev/null 2>&1 || true

if [[ -d "$TARGET_APP" ]]; then
  log "backup existing app"
  /bin/mv "$TARGET_APP" "$BACKUP_APP"
fi

log "copy new app"
if ! /usr/bin/ditto "$NEW_APP" "$TARGET_APP"; then
  rollback
  exit 1
fi

/usr/bin/xattr -cr "$TARGET_APP" >/dev/null 2>&1 || true

if [[ -d "$BACKUP_APP" ]]; then
  /bin/rm -rf "$BACKUP_APP" || true
fi

log "refresh shell integration"
"$TARGET_CLI" init --update-only >/dev/null 2>&1 || true

log "relaunch app"
/usr/bin/open "$TARGET_APP" >/dev/null 2>&1 || true

log "done"
/bin/rm -f "$0" >/dev/null 2>&1 || true
/bin/rm -rf "$WORK_DIR" >/dev/null 2>&1 || true
"#;

        fs::write(script_path, script).with_context(|| {
            format!(
                "failed to write helper script to {}",
                script_path.as_os_str().to_string_lossy()
            )
        })?;
        run_status(
            Command::new("/bin/chmod").arg("700").arg(script_path),
            "chmod update helper script",
        )?;
        Ok(())
    }

    fn spawn_update_helper(
        script: &Path,
        target_app: &Path,
        new_app: &Path,
        work_dir: &Path,
    ) -> anyhow::Result<()> {
        Command::new("/usr/bin/nohup")
            .arg("/bin/bash")
            .arg(script)
            .arg(target_app)
            .arg(new_app)
            .arg(work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("launch detached updater helper")?;
        Ok(())
    }

    fn run_output(cmd: &mut Command, context_text: &str) -> anyhow::Result<Vec<u8>> {
        let output = cmd
            .output()
            .with_context(|| format!("failed to {}", context_text))?;
        if output.status.success() {
            return Ok(output.stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{} failed: {}", context_text, stderr.trim());
    }

    fn run_status(cmd: &mut Command, context_text: &str) -> anyhow::Result<()> {
        let status = cmd
            .status()
            .with_context(|| format!("failed to {}", context_text))?;
        if status.success() {
            return Ok(());
        }
        bail!("{} failed with status {}", context_text, status);
    }

    fn is_newer_version(latest: &str, current: &str) -> bool {
        match compare_versions(latest, current) {
            Some(Ordering::Greater) => true,
            Some(_) => false,
            None => latest.trim_start_matches(['v', 'V']) != current.trim_start_matches(['v', 'V']),
        }
    }

    fn format_version_for_display(version: &str) -> String {
        version.trim().trim_start_matches(['v', 'V']).to_string()
    }

    fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
        let left = parse_version_numbers(left)?;
        let right = parse_version_numbers(right)?;
        let max_len = left.len().max(right.len());
        for idx in 0..max_len {
            let l = left.get(idx).copied().unwrap_or(0);
            let r = right.get(idx).copied().unwrap_or(0);
            match l.cmp(&r) {
                Ordering::Equal => {}
                non_eq => return Some(non_eq),
            }
        }
        Some(Ordering::Equal)
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
}
