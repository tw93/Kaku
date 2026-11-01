use anyhow::{anyhow, bail, Context};
use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
pub struct UpdateCommand {
    /// Apply the update without an interactive prompt. Required to update from a
    /// non-interactive session (pipe, cron, CI); interactive runs still prompt.
    #[arg(long)]
    pub yes: bool,
}

impl UpdateCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        imp::run(self.yes)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use config::proxy::{apply_to_command, detect_system_proxy};
    use serde::Deserialize;
    use std::cmp::Ordering;
    use std::fs;
    use std::io::{self, IsTerminal, Read, Write};
    use std::os::unix::process::CommandExt;
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
    /// Official Homebrew/homebrew-cask token.
    const OFFICIAL_BREW_CASK: &str = "kaku";
    /// Personal tap for installs from before the official cask existed.
    const TAP_BREW_CASK: &str = "tw93/tap/kakuku";

    fn brew_casks_in_priority_order() -> [&'static str; 2] {
        [OFFICIAL_BREW_CASK, TAP_BREW_CASK]
    }

    fn brew_upgrade_fallback(cask_name: &str) -> &'static str {
        if cask_name == OFFICIAL_BREW_CASK {
            TAP_BREW_CASK
        } else {
            OFFICIAL_BREW_CASK
        }
    }

    #[derive(Debug, Deserialize)]
    struct GitHubRelease {
        tag_name: String,
        #[serde(default)]
        body: String,
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

    pub fn run(assume_yes: bool) -> anyhow::Result<()> {
        match resolve_update_provider()? {
            UpdateProvider::Brew(info) => {
                println!("Detected Homebrew-managed installation. Using brew upgrade...");
                return run_brew_upgrade(&info, assume_yes);
            }
            UpdateProvider::Direct => {}
        }

        let current_version = config::wezterm_version().to_string();
        let current_version_display = format_version_for_display(&current_version);
        println!("Current version: {}", current_version_display);
        println!("Checking latest release...");

        // Detect the macOS system proxy once and reuse for all curl calls.
        // This ensures updates work even when launched from the menu bar or a
        // notification, where the process env has no proxy vars.
        let proxy = detect_system_proxy();

        let release = match curl_get_text(RELEASE_API_URL, &current_version, &proxy)
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
        } else if let Some(tag_name) = resolve_latest_tag_from_redirect(&current_version, &proxy)? {
            if !is_newer_version(&tag_name, &current_version) {
                println!(
                    "Already up to date. Current={} Latest={}",
                    current_version_display,
                    format_version_for_display(&tag_name)
                );
                return Ok(());
            }
        }

        // Show release notes so the user knows what changed.
        if let Some(release) = &release {
            let body = release.body.trim();
            if !body.is_empty() {
                println!();
                println!(
                    "Release notes ({}):",
                    format_version_for_display(&release.tag_name)
                );
                // Strip markdown image links and clean up for terminal display.
                for line in body.lines() {
                    let trimmed = line.trim();
                    // Skip markdown image links outright.
                    if trimmed.starts_with("![") {
                        continue;
                    }
                    // Render any inline HTML (the release header's
                    // <div>/<img>/<h1>/<p><em>) as plain text.
                    let cleaned = strip_html_tags(line);
                    // Drop lines that were pure HTML: had content, empty now.
                    if cleaned.trim().is_empty() && !trimmed.is_empty() {
                        continue;
                    }
                    println!("  {}", cleaned);
                }
                println!();
            }
        }

        // Keep the checksum source consistent with the package source. A pinned
        // release ZIP must be verified against the *same* release's checksum,
        // never a floating `latest` one: mixing a pinned artifact with a
        // floating hash can falsely abort a good update or verify the wrong
        // build. Only fall back to the latest pair when the ZIP itself does.
        let (zip_url, sha_url) = match release.as_ref() {
            Some(rel) => {
                let zip = find_asset(&rel.assets, UPDATE_ZIP_NAME)
                    .map(|a| a.browser_download_url.as_str());
                let sha = find_asset(&rel.assets, UPDATE_SHA_NAME)
                    .map(|a| a.browser_download_url.as_str());
                match (zip, sha) {
                    (Some(zip), Some(sha)) => (zip, sha),
                    (Some(_), None) => bail!(
                        "release `{}` is missing checksum asset `{}`; refusing to install an unverified build",
                        rel.tag_name,
                        UPDATE_SHA_NAME
                    ),
                    _ => (LATEST_ZIP_URL, LATEST_SHA_URL),
                }
            }
            None => (LATEST_ZIP_URL, LATEST_SHA_URL),
        };

        let update_root = config::DATA_DIR.join("updates");
        config::create_user_owned_dirs(&update_root).context("create updates directory")?;

        // Clean up old update directories (keep only last 2)
        cleanup_old_updates(&update_root);

        let tag = release
            .as_ref()
            .map(|r| sanitize_tag(&r.tag_name))
            .unwrap_or_else(|| "latest".to_string());
        let now = now_unix_seconds();
        let work_dir = update_root.join(format!("{}-{}", tag, now));
        config::create_user_owned_dirs(&work_dir).context("create update work directory")?;

        let zip_path = work_dir.join(UPDATE_ZIP_NAME);
        println!("Downloading {} ...", UPDATE_ZIP_NAME);
        // Flush stdout before curl progress bar to avoid garbled output
        let _ = io::stdout().flush();
        curl_download_to_file(zip_url, &zip_path, &current_version, &proxy)
            .context("failed to download update package")?;

        println!("Verifying package checksum...");
        let checksum_text = curl_get_text(sha_url, &current_version, &proxy).context(
            "failed to fetch checksum; aborting to avoid installing an unverified build",
        )?;
        verify_sha256(&zip_path, &checksum_text).context("checksum verification failed")?;

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

        verify_app_signature(&new_app_path)
            .context("downloaded update failed code-signature verification; refusing to install")?;

        let target_app = resolve_target_app_path().context("resolve installed Kaku.app path")?;
        ensure_can_write_target(&target_app)?;

        let helper_script = update_root.join(format!("apply-update-{}.sh", now));
        write_helper_script(&helper_script).context("write update helper script")?;

        let update_label = release
            .as_ref()
            .map(|r| r.tag_name.as_str())
            .unwrap_or("latest");
        if !confirm_apply_update(update_label, assume_yes)? {
            println!("Update cancelled.");
            let _ = fs::remove_dir_all(&work_dir);
            return Ok(());
        }

        spawn_update_helper(&helper_script, &target_app, &new_app_path, &work_dir)
            .context("spawn update helper")?;

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

        let exe = std::env::current_exe().context("resolve current executable path")?;
        let mut should_probe_brew = path_is_or_points_to_caskroom(&exe);

        if let Some(target) = std::env::var_os("KAKU_UPDATE_TARGET_APP") {
            let target = PathBuf::from(target);
            should_probe_brew |= path_is_or_points_to_caskroom(&target);
        }

        if should_probe_brew {
            if let Some(brew_info) = resolve_brew_info()? {
                return Ok(UpdateProvider::Brew(brew_info));
            }

            if find_brew_binary().is_none() {
                bail!(
                    "Kaku appears to be Homebrew-managed but `brew` was not found in PATH or standard locations"
                );
            }
        }

        Ok(UpdateProvider::Direct)
    }

    fn resolve_brew_info() -> anyhow::Result<Option<BrewInfo>> {
        let Some(brew_bin) = find_brew_binary() else {
            return Ok(None);
        };

        for cask_name in brew_casks_in_priority_order() {
            if is_brew_cask_installed(&brew_bin, cask_name)? {
                return Ok(Some(BrewInfo {
                    brew_bin,
                    cask_name: cask_name.to_string(),
                }));
            }
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

    fn path_is_or_points_to_caskroom(path: &Path) -> bool {
        path_contains_caskroom(path)
            || fs::canonicalize(path)
                .map(|resolved| path_contains_caskroom(&resolved))
                .unwrap_or(false)
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

    fn relaunch_after_upgrade() -> bool {
        // Give the system a moment to settle before relaunching.
        std::thread::sleep(std::time::Duration::from_secs(2));
        match Command::new("/usr/bin/open")
            .arg("-a")
            .arg("Kaku")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    fn run_brew_upgrade(info: &BrewInfo, assume_yes: bool) -> anyhow::Result<()> {
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

        // Same confirm gate as the direct ZIP path: brew upgrade replaces the
        // app bundle and relaunches Kaku, which drops unsaved terminal work.
        let label = format!("brew cask {}", info.cask_name);
        if !confirm_apply_update(&label, assume_yes)? {
            println!("Update cancelled.");
            return Ok(());
        }

        let primary = Command::new(&info.brew_bin)
            .arg("upgrade")
            .arg("--cask")
            .arg(&info.cask_name)
            .status()
            .with_context(|| format!("failed to run brew upgrade for {}", info.cask_name))?;
        if primary.success() {
            println!("Upgrade completed. Relaunching Kaku...");
            if !relaunch_after_upgrade() {
                println!("Upgrade completed. Please launch Kaku manually.");
            }
            return Ok(());
        }

        let fallback_name = brew_upgrade_fallback(&info.cask_name);

        let fallback = Command::new(&info.brew_bin)
            .arg("upgrade")
            .arg("--cask")
            .arg(fallback_name)
            .status()
            .with_context(|| {
                format!("failed to run brew upgrade fallback for {}", fallback_name)
            })?;
        if fallback.success() {
            println!("Upgrade completed. Relaunching Kaku...");
            if !relaunch_after_upgrade() {
                println!("Upgrade completed. Please launch Kaku manually.");
            }
            return Ok(());
        }

        bail!(
            "brew update failed (tried `brew upgrade --cask {}` and `brew upgrade --cask {}`)",
            info.cask_name,
            fallback_name
        )
    }

    fn resolve_latest_tag_from_redirect(
        current_version: &str,
        proxy: &Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.arg("--fail")
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
            .arg(RELEASE_LATEST_URL);
        apply_to_command(&mut cmd, proxy);
        let output = run_output(&mut cmd, "resolve latest release tag via redirect")?;
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

    fn cleanup_old_updates(update_root: &Path) {
        let Ok(entries) = fs::read_dir(update_root) else {
            return;
        };

        let mut dirs: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let modified = e.metadata().ok()?.modified().ok()?;
                Some((e.path(), modified))
            })
            .collect();

        // Sort by modification time, newest first
        dirs.sort_by_key(|dir| std::cmp::Reverse(dir.1));

        // Remove all but the 2 most recent
        for (path, _) in dirs.into_iter().skip(2) {
            let _ = fs::remove_dir_all(&path);
        }
    }

    fn curl_get_text(
        url: &str,
        current_version: &str,
        proxy: &Option<String>,
    ) -> anyhow::Result<String> {
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
            .arg(format!("kaku/{}", current_version))
            .arg(url);
        apply_to_command(&mut cmd, proxy);
        let output = run_output(&mut cmd, "request update metadata")?;
        String::from_utf8(output).context("curl returned non-utf8 response")
    }

    fn curl_download_to_file(
        url: &str,
        output_path: &Path,
        current_version: &str,
        proxy: &Option<String>,
    ) -> anyhow::Result<()> {
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.arg("--fail")
            .arg("--location")
            .arg("--progress-bar")
            .arg("--retry")
            .arg("3")
            .arg("--connect-timeout")
            .arg("20")
            .arg("--user-agent")
            .arg(format!("kaku/{}", current_version))
            .arg("--output")
            .arg(output_path)
            .arg(url);
        apply_to_command(&mut cmd, proxy);
        run_status(&mut cmd, "download update package")
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

    /// Validates that the path points to a valid Kaku.app bundle.
    /// Uses Path component comparison (not string suffix) to handle
    /// trailing separators correctly (e.g., /Applications/Kaku.app/).
    fn validate_app_bundle_path(path: &Path) -> anyhow::Result<()> {
        // Use ends_with("Kaku.app") which compares path components, not strings.
        // This correctly handles paths like /Applications/Kaku.app/ (with trailing slash).
        if !path.ends_with("Kaku.app") {
            bail!(
                "invalid app bundle path: must end with Kaku.app: {}",
                path.display()
            );
        }
        Ok(())
    }

    fn write_helper_script(script_path: &Path) -> anyhow::Result<()> {
        let script = include_str!("../../scripts/update_helper.sh");
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

    fn verify_app_signature(app_path: &Path) -> anyhow::Result<()> {
        // The SHA-256 check only proves the bytes match a hash served from the
        // same origin. codesign + Gatekeeper prove the bundle is intact and
        // signed by a trusted, notarized Developer ID before we replace the
        // installed app. Release builds staple the notarization ticket, so
        // `spctl --assess` succeeds offline (see scripts/notarize.sh).
        run_status(
            Command::new("/usr/bin/codesign")
                .arg("--verify")
                .arg("--deep")
                .arg("--strict")
                .arg(app_path),
            "verify update code signature",
        )?;
        run_status(
            Command::new("/usr/sbin/spctl")
                .arg("--assess")
                .arg("--type")
                .arg("execute")
                .arg(app_path),
            "assess update with Gatekeeper",
        )?;
        Ok(())
    }

    fn confirm_apply_update(update_label: &str, assume_yes: bool) -> anyhow::Result<bool> {
        // Set only after the GUI overlay already confirmed the restart. Menu
        // "Check for Updates" does not set this, so the CLI still prompts.
        if std::env::var_os("KAKU_UPDATE_AUTO_CONFIRM").is_some() {
            println!("Auto-confirming update (KAKU_UPDATE_AUTO_CONFIRM is set).");
            return Ok(true);
        }

        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            // No TTY to prompt on. Replacing the running app is destructive, so
            // require explicit opt-in rather than silently proceeding in a
            // pipe / cron / CI context.
            if assume_yes {
                return Ok(true);
            }
            bail!(
                "Refusing to apply a binary-replacing update in a non-interactive session.\n\
                 Re-run `kaku update --yes` (or set KAKU_UPDATE_AUTO_CONFIRM=1) to proceed."
            );
        }

        println!();
        println!(
            "Ready to apply update {}.",
            format_version_for_display(update_label)
        );
        println!("Kaku will quit and relaunch; running tasks will stop.");
        print!("Press Enter to continue, any other key to cancel. ");
        io::stdout()
            .flush()
            .context("flush stdout for update confirmation")?;

        // Read single key without waiting for Enter
        let result = read_single_key();
        println!();

        Ok(result.map(|c| c == '\n' || c == '\r').unwrap_or(false))
    }

    fn read_single_key() -> anyhow::Result<char> {
        use std::os::unix::io::AsRawFd;

        let stdin_fd = io::stdin().as_raw_fd();
        let mut termios = termios::Termios::from_fd(stdin_fd)?;
        let original = termios;

        // Disable canonical mode and echo
        termios.c_lflag &= !(termios::ICANON | termios::ECHO);
        termios.c_cc[termios::VMIN] = 1;
        termios.c_cc[termios::VTIME] = 0;
        termios::tcsetattr(stdin_fd, termios::TCSANOW, &termios)?;

        let mut buf = [0u8; 1];
        let result = io::stdin().read_exact(&mut buf);

        // Restore original terminal settings
        termios::tcsetattr(stdin_fd, termios::TCSANOW, &original)?;

        result?;
        Ok(buf[0] as char)
    }

    fn spawn_update_helper(
        script: &Path,
        target_app: &Path,
        new_app: &Path,
        work_dir: &Path,
    ) -> anyhow::Result<()> {
        // Validate app bundle paths before spawning the helper
        validate_app_bundle_path(target_app).context("validate target app bundle path")?;
        validate_app_bundle_path(new_app).context("validate new app bundle path")?;

        Command::new("/usr/bin/nohup")
            .arg("/bin/bash")
            .arg(script)
            .arg(target_app)
            .arg(new_app)
            .arg(work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Own process group, so the helper survives Kaku exiting even when
            // its caller belongs to a launchd job's group (#591), until it
            // hands itself to its own launchd job (scripts/update_helper.sh).
            .process_group(0)
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

    /// Strip inline HTML tags so release notes print as plain text. The GitHub
    /// release body mirrors RELEASE_NOTES.md, whose header wraps the logo and
    /// title in <div>/<img>/<h1>/<p><em>; printed verbatim those tags leak into
    /// `kaku update` output.
    fn strip_html_tags(line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut in_tag = false;
        for ch in line.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(ch),
                _ => {}
            }
        }
        out
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

    #[cfg(test)]
    mod tests {
        use super::{
            brew_casks_in_priority_order, brew_upgrade_fallback, is_newer_version,
            spawn_update_helper, strip_html_tags, OFFICIAL_BREW_CASK, TAP_BREW_CASK,
        };
        use std::time::{Duration, Instant};

        // The helper outlives the process that spawns it; when that process is
        // a launchd job, launchd kills its process group on exit (#591).
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

            let deadline = Instant::now() + Duration::from_secs(5);
            while !out.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let text = std::fs::read_to_string(&out).expect("helper wrote its pid and pgid");
            let ids: Vec<&str> = text.split_whitespace().collect();
            assert_eq!(ids.len(), 2, "unexpected probe output: {text:?}");
            assert_eq!(ids[0], ids[1], "helper must lead its own process group");
        }

        #[test]
        fn semver_numeric_comparison() {
            assert!(is_newer_version("0.1.10", "0.1.9"));
            assert!(!is_newer_version("0.2.0", "0.11.0"));
            assert!(!is_newer_version("0.1.1", "0.1.1"));
            assert!(is_newer_version("v0.1.2", "0.1.1"));
        }

        #[test]
        fn release_notes_render_as_plain_text() {
            // Pure-HTML header lines collapse to empty (caller drops them).
            assert_eq!(strip_html_tags("<div align=\"center\">"), "");
            assert_eq!(strip_html_tags("</div>"), "");
            // Tagged content keeps only the text.
            assert_eq!(strip_html_tags("<h1>Kaku V0.12.3</h1>"), "Kaku V0.12.3");
            assert_eq!(
                strip_html_tags("<p><em>A fast terminal.</em></p>"),
                "A fast terminal."
            );
            // Plain markdown lines pass through untouched.
            assert_eq!(
                strip_html_tags("1. **System Proxy**: works"),
                "1. **System Proxy**: works"
            );
        }

        #[test]
        fn official_homebrew_cask_is_preferred() {
            assert_eq!(
                brew_casks_in_priority_order(),
                [OFFICIAL_BREW_CASK, TAP_BREW_CASK]
            );
            assert_eq!(brew_upgrade_fallback(OFFICIAL_BREW_CASK), TAP_BREW_CASK);
            assert_eq!(brew_upgrade_fallback(TAP_BREW_CASK), OFFICIAL_BREW_CASK);
        }

        /// Guard the multi-entry update matrix: every provider that replaces
        /// the running app must call `confirm_apply_update` before the
        /// destructive step. Catches the brew sibling of the menu-confirm fix.
        #[test]
        fn brew_and_direct_paths_confirm_before_replace() {
            let src = include_str!("update.rs");

            assert!(
                src.contains("run_brew_upgrade(&info, assume_yes)"),
                "brew provider must thread assume_yes into run_brew_upgrade"
            );

            let brew_fn = src
                .split("fn run_brew_upgrade")
                .nth(1)
                .expect("run_brew_upgrade present");
            let brew_body = brew_fn
                .split("fn resolve_latest_tag_from_redirect")
                .next()
                .expect("brew function body");
            let confirm_at = brew_body
                .find("confirm_apply_update")
                .expect("run_brew_upgrade must confirm before upgrading");
            let upgrade_at = brew_body
                .find(".arg(\"upgrade\")")
                .expect("run_brew_upgrade must invoke brew upgrade");
            assert!(
                confirm_at < upgrade_at,
                "confirm_apply_update must run before brew upgrade"
            );

            let direct_confirm = src
                .find("if !confirm_apply_update(update_label, assume_yes)?")
                .expect("direct path must confirm before install");
            let direct_spawn = src
                .find("spawn_update_helper(&helper_script, &target_app, &new_app_path, &work_dir)")
                .expect("direct path must spawn helper");
            assert!(
                direct_confirm < direct_spawn,
                "direct path must confirm before spawning the replace helper"
            );

            assert!(
                src.contains("Kaku will quit and relaunch; running tasks will stop."),
                "CLI confirm copy must warn about quit and lost tasks"
            );
        }
    }
}
