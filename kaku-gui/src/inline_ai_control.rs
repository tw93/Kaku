//! Authenticates shell-to-GUI control messages used by inline AI features.
//!
//! Terminal output can emit OSC 1337 user variables, so privileged messages
//! carry a local capability before they are allowed to reach Lua or GUI actions.

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CAPABILITY_FILE_NAME: &str = "ai_inline_capability";
const CAPABILITY_BYTES: usize = 32;
const CAPABILITY_ENCODED_LEN: usize = 43;
const PRIVILEGED_USER_VARS: &[&str] = &[
    "kaku_ai_query",
    "kaku_last_cmd",
    "kaku_last_exit_code",
    "kaku_open_ai_chat",
    "kaku_user_typing",
    "kaku_copy_command",
];

static ACTIVE_CAPABILITY: OnceLock<String> = OnceLock::new();

fn capability_file_path() -> PathBuf {
    config::HOME_DIR
        .join(".config")
        .join("kaku")
        .join(CAPABILITY_FILE_NAME)
}

fn is_valid_capability(value: &str) -> bool {
    value.len() == CAPABILITY_ENCODED_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn generate_capability() -> Result<String> {
    let mut bytes = [0u8; CAPABILITY_BYTES];
    getrandom::fill(&mut bytes).context("generate inline AI capability")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn create_capability_file(path: &Path, capability: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("inline AI capability path has no parent"))?;
    config::create_user_owned_dirs(parent).context("create inline AI state directory")?;

    let suffix = &capability[..8];
    let temp_path = path.with_extension(format!("{}.{}.tmp", std::process::id(), suffix));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("create {}", temp_path.display()))?;
        file.write_all(capability.as_bytes())
            .with_context(|| format!("write {}", temp_path.display()))?;
        // Trailing newline matters: zsh's `read` reports EOF as failure when
        // the last line is unterminated, which made the shell integration
        // treat the capability as unreadable (#511). The Rust reader trims.
        file.write_all(b"\n")
            .with_context(|| format!("write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {}", temp_path.display()))?;
        match std::fs::hard_link(&temp_path, path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(err) => Err(err).with_context(|| format!("create {}", path.display())),
        }
    })();

    let _ = std::fs::remove_file(&temp_path);
    result
}

/// Loads the install-local capability, creating it without replacing a value
/// another Kaku process may already be using.
pub fn initialize_capability() -> Result<()> {
    if ACTIVE_CAPABILITY.get().is_some() {
        return Ok(());
    }

    let path = capability_file_path();
    if !path
        .try_exists()
        .with_context(|| format!("inspect {}", path.display()))?
    {
        create_capability_file(&path, &generate_capability()?)?;
    }
    let capability = read_capability_file(&path)?;
    ACTIVE_CAPABILITY
        .set(capability)
        .map_err(|_| anyhow::anyhow!("inline AI capability was initialized twice"))
}

fn read_capability_file(path: &Path) -> Result<String> {
    let metadata =
        std::fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!("inline AI capability path is not a regular file");
    }
    if metadata.len() > 128 {
        anyhow::bail!("inline AI capability file is too large");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            anyhow::bail!("inline AI capability file permissions are too broad");
        }
    }
    let capability =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let capability = capability.trim();
    if !is_valid_capability(capability) {
        anyhow::bail!("inline AI capability file is invalid");
    }
    Ok(capability.to_string())
}

fn is_privileged_user_var(name: &str) -> bool {
    PRIVILEGED_USER_VARS.contains(&name)
}

fn decode_control_value_with_capability<'a>(
    name: &str,
    value: &'a str,
    capability: Option<&str>,
) -> Option<&'a str> {
    if !is_privileged_user_var(name) {
        return Some(value);
    }
    let capability = capability?;
    value.strip_prefix(capability)?.strip_prefix(':')
}

/// Validates and strips the capability from a privileged user variable.
pub fn decode_control_value<'a>(name: &str, value: &'a str) -> Option<&'a str> {
    decode_control_value_with_capability(name, value, ACTIVE_CAPABILITY.get().map(String::as_str))
}

/// Adds the local capability before a trusted helper emits a control variable.
pub fn encode_control_value(name: &str, value: &str) -> Result<String> {
    if !is_privileged_user_var(name) {
        return Ok(value.to_string());
    }
    let capability = read_capability_file(&capability_file_path())?;
    Ok(format!("{capability}:{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_user_vars_require_the_active_capability() {
        let capability = URL_SAFE_NO_PAD.encode([7u8; CAPABILITY_BYTES]);
        let value = format!("{capability}:[mode:auto] list files");

        assert_eq!(
            decode_control_value_with_capability(
                "kaku_ai_query",
                &value,
                Some(capability.as_str())
            ),
            Some("[mode:auto] list files")
        );
        assert_eq!(
            decode_control_value_with_capability("kaku_ai_query", &value, Some("wrong")),
            None
        );
        assert_eq!(
            decode_control_value_with_capability("kaku_ai_query", &value, None),
            None
        );
        assert_eq!(
            decode_control_value_with_capability("unrelated", "plain", None),
            Some("plain")
        );
    }

    #[test]
    fn capability_file_round_trips_with_private_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CAPABILITY_FILE_NAME);
        let capability = URL_SAFE_NO_PAD.encode([9u8; CAPABILITY_BYTES]);

        create_capability_file(&path, &capability).unwrap();
        assert_eq!(read_capability_file(&path).unwrap(), capability);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn capability_creation_does_not_replace_an_existing_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CAPABILITY_FILE_NAME);
        let first = URL_SAFE_NO_PAD.encode([1u8; CAPABILITY_BYTES]);
        let second = URL_SAFE_NO_PAD.encode([2u8; CAPABILITY_BYTES]);

        create_capability_file(&path, &first).unwrap();
        create_capability_file(&path, &second).unwrap();

        assert_eq!(read_capability_file(&path).unwrap(), first);
    }
}
