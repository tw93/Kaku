use anyhow::{anyhow, bail, Context};
use clap::Parser;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Parser, Clone, Default)]
pub struct InitCommand {
    /// Refresh shell integration without interactive prompts
    #[arg(long)]
    pub update_only: bool,
}

impl InitCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        imp::run(self.update_only)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::bail;

    pub fn run(_update_only: bool) -> anyhow::Result<()> {
        bail!("`kaku init` is currently supported on macOS only")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::shell::{detect_shell_kind, ShellKind};
    use std::os::unix::fs::PermissionsExt;

    pub fn run(update_only: bool) -> anyhow::Result<()> {
        ensure_user_config().context("ensure user config exists")?;

        install_kaku_wrapper().context("install kaku wrapper")?;
        install_k_wrapper().context("install k wrapper")?;

        let shell = detect_shell_kind();
        let script_name = match shell {
            ShellKind::Fish => "setup_fish.sh",
            _ => "setup_zsh.sh",
        };
        let script = resolve_setup_script(script_name)
            .ok_or_else(|| anyhow!("failed to locate {} for Kaku initialization", script_name))?;

        let mut cmd = Command::new("/bin/bash");
        cmd.arg(&script).env("KAKU_INIT_INTERNAL", "1");
        if update_only {
            cmd.arg("--update-only");
        }
        let status = cmd
            .status()
            .with_context(|| format!("run {}", script.display()))?;

        if status.success() {
            return Ok(());
        }

        bail!("kaku init failed with status {}", status);
    }

    fn install_kaku_wrapper() -> anyhow::Result<()> {
        let wrapper_path = wrapper_path();
        let wrapper_dir = wrapper_path
            .parent()
            .ok_or_else(|| anyhow!("invalid wrapper path"))?;
        config::create_user_owned_dirs(wrapper_dir).context("create wrapper directory")?;

        if fs::symlink_metadata(&wrapper_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            fs::remove_file(&wrapper_path).with_context(|| {
                format!("remove legacy symlink wrapper {}", wrapper_path.display())
            })?;
        }

        let preferred_bin = resolve_preferred_kaku_bin()
            .unwrap_or_else(|| PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku"));
        let preferred_bin = escape_for_double_quotes(&preferred_bin.display().to_string());

        let script = format!(
            r#"#!/bin/bash
set -euo pipefail

if [[ -n "${{KAKU_BIN:-}}" && -x "${{KAKU_BIN}}" ]]; then
	exec "${{KAKU_BIN}}" "$@"
fi

for candidate in \
	"{preferred_bin}" \
	"/Applications/Kaku.app/Contents/MacOS/kaku" \
	"$HOME/Applications/Kaku.app/Contents/MacOS/kaku"; do
	if [[ -n "$candidate" && -x "$candidate" ]]; then
		exec "$candidate" "$@"
	fi
done

echo "kaku: Kaku.app not found. Expected /Applications/Kaku.app." >&2
exit 127
"#
        );

        let mut file = fs::File::create(&wrapper_path)
            .with_context(|| format!("create wrapper {}", wrapper_path.display()))?;
        file.write_all(script.as_bytes())
            .with_context(|| format!("write wrapper {}", wrapper_path.display()))?;
        fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod wrapper {}", wrapper_path.display()))?;
        Ok(())
    }

    fn install_k_wrapper() -> anyhow::Result<()> {
        let k_path = k_wrapper_path();
        let k_dir = k_path
            .parent()
            .ok_or_else(|| anyhow!("invalid k wrapper path"))?;
        config::create_user_owned_dirs(k_dir).context("create k wrapper directory")?;

        // If something else already owns this path and it is not our wrapper, skip.
        if k_path.exists() {
            let content = fs::read_to_string(&k_path).unwrap_or_default();
            if !content.contains("Kaku") && !content.contains("kaku") {
                eprintln!(
                    "{}",
                    rust_i18n::t!(
                        "init.k_wrapper_conflict",
                        path = k_path.display().to_string()
                    )
                );
                return Ok(());
            }
        }
        if fs::symlink_metadata(&k_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            fs::remove_file(&k_path)
                .with_context(|| format!("remove legacy symlink k wrapper {}", k_path.display()))?;
        }

        let preferred_k_bin = resolve_preferred_k_bin()
            .unwrap_or_else(|| PathBuf::from("/Applications/Kaku.app/Contents/MacOS/k"));
        let preferred_k_bin = escape_for_double_quotes(&preferred_k_bin.display().to_string());

        let script = format!(
            r#"#!/bin/bash
set -euo pipefail

for candidate in \
	"{preferred_k_bin}" \
	"/Applications/Kaku.app/Contents/MacOS/k" \
	"$HOME/Applications/Kaku.app/Contents/MacOS/k"; do
	if [[ -n "$candidate" && -x "$candidate" ]]; then
		exec "$candidate" "$@"
	fi
done

echo "k: Kaku.app not found. Run 'kaku init' after installing Kaku." >&2
exit 127
"#
        );

        let mut file = fs::File::create(&k_path)
            .with_context(|| format!("create k wrapper {}", k_path.display()))?;
        file.write_all(script.as_bytes())
            .with_context(|| format!("write k wrapper {}", k_path.display()))?;
        fs::set_permissions(&k_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod k wrapper {}", k_path.display()))?;
        Ok(())
    }

    fn k_wrapper_path() -> PathBuf {
        let dir = match detect_shell_kind() {
            ShellKind::Fish => "fish",
            _ => "zsh",
        };
        config::HOME_DIR
            .join(".config")
            .join("kaku")
            .join(dir)
            .join("bin")
            .join("k")
    }

    fn resolve_preferred_k_bin() -> Option<PathBuf> {
        if let Ok(exe) = std::env::current_exe() {
            // current_exe is the `kaku` binary; `k` sits alongside it.
            let k_candidate = exe.with_file_name("k");
            if is_executable_file(&k_candidate) {
                return Some(k_candidate);
            }
        }
        for candidate in [
            PathBuf::from("/Applications/Kaku.app/Contents/MacOS/k"),
            config::HOME_DIR
                .join("Applications")
                .join("Kaku.app")
                .join("Contents")
                .join("MacOS")
                .join("k"),
        ] {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn wrapper_path() -> PathBuf {
        let dir = match detect_shell_kind() {
            ShellKind::Fish => "fish",
            _ => "zsh",
        };
        config::HOME_DIR
            .join(".config")
            .join("kaku")
            .join(dir)
            .join("bin")
            .join("kaku")
    }

    fn resolve_preferred_kaku_bin() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("KAKU_BIN") {
            let path = PathBuf::from(path);
            if is_executable_file(&path) {
                return Some(path);
            }
        }

        if let Ok(exe) = std::env::current_exe() {
            if exe
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("kaku"))
                .unwrap_or(false)
                && is_executable_file(&exe)
            {
                return Some(exe);
            }
        }

        for candidate in [
            PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku"),
            config::HOME_DIR
                .join("Applications")
                .join("Kaku.app")
                .join("Contents")
                .join("MacOS")
                .join("kaku"),
        ] {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }

        None
    }

    fn is_executable_file(path: &Path) -> bool {
        fs::metadata(path)
            .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
            .unwrap_or(false)
    }

    fn escape_for_double_quotes(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    }

    fn resolve_setup_script(script_name: &str) -> Option<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(
                cwd.join("assets")
                    .join("shell-integration")
                    .join(script_name),
            );
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(contents_dir) = exe.parent().and_then(|p| p.parent()) {
                candidates.push(contents_dir.join("Resources").join(script_name));
            }
        }

        candidates.push(PathBuf::from(format!(
            "/Applications/Kaku.app/Contents/Resources/{}",
            script_name
        )));
        candidates.push(
            config::HOME_DIR
                .join("Applications")
                .join("Kaku.app")
                .join("Contents")
                .join("Resources")
                .join(script_name),
        );

        candidates.into_iter().find(|p| p.exists())
    }

    fn ensure_user_config() -> anyhow::Result<()> {
        config::ensure_user_config_exists().context("ensure user config exists")?;
        Ok(())
    }
}
