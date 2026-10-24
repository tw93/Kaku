//! Kaku Assistant configuration management.
//!
//! This module handles the configuration file for Kaku's built-in AI assistant,
//! including default values, file paths, and ensuring required configuration keys exist.
//!
//! The configuration is stored in `assistant.toml` in the user's Kaku config directory.

use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

/// Default AI model to use when none is specified.
/// Default model for command analysis suggestions (the inline `#` query and
/// shell-error fixer). Picked for low cost and low latency.
pub const DEFAULT_MODEL: &str = "gpt-5.4-mini";

/// Default deep model for the AI chat overlay (Cmd+L). Stronger than the
/// simple model because the overlay does multi-turn reasoning + tool calls.
pub const DEFAULT_CHAT_MODEL: &str = "gpt-5.5";

/// Default API base URL for the AI service.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

// Provider detection (`detect_provider_with_auth`) and the preset table live
// in `kaku-gui/src/ai_client.rs` next to the only consumer. There used to be
// a `#[allow(dead_code)]` second copy here that was never wired into the kaku
// binary; it just bit-rotted out of sync with the GUI one. Adding a new
// provider preset is now a one-file edit in `ai_client.rs`.

/// Returns the path to the assistant.toml configuration file.
///
/// The file is located in the same directory as the user's Kaku config,
/// typically `~/.config/kaku/assistant.toml` on macOS/Linux.
///
/// # Errors
/// Returns an error if the user config path cannot be determined or has no parent directory.
pub fn assistant_toml_path() -> anyhow::Result<PathBuf> {
    let user_config_path = config::user_config_path();
    let config_dir = user_config_path
        .parent()
        .ok_or_else(|| anyhow!("invalid user config path: {}", user_config_path.display()))?;
    Ok(config_dir.join("assistant.toml"))
}

/// Ensures the assistant.toml configuration file exists, creating it with defaults if necessary.
///
/// This function:
/// 1. Creates the config directory if it doesn't exist
/// 2. Writes a default configuration file if none exists
/// 3. Ensures required keys (model, base_url) are present, adding them if missing
///
/// # Returns
/// * `Ok(PathBuf)` - The path to the configuration file
///
/// # Errors
/// Returns an error if the config directory cannot be created or the file cannot be written.
pub fn ensure_assistant_toml_exists() -> anyhow::Result<PathBuf> {
    let path = assistant_toml_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid assistant.toml path: {}", path.display()))?;
    config::create_user_owned_dirs(parent).context("create config directory")?;

    if !path.exists() {
        crate::utils::write_atomic(&path, default_assistant_toml_template().as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
    }

    ensure_required_keys(&path)?;

    // Best-effort cleanup for deprecated config files
    let ai_toml = parent.join("ai.toml");
    if ai_toml.exists() {
        if let Err(e) = std::fs::remove_file(&ai_toml) {
            log::debug!("Failed to remove deprecated ai.toml: {}", e);
        }
    }
    let auto_toml = parent.join("auto.toml");
    if auto_toml.exists() {
        if let Err(e) = std::fs::remove_file(&auto_toml) {
            log::debug!("Failed to remove deprecated auto.toml: {}", e);
        }
    }

    Ok(path)
}

/// Returns the default assistant.toml configuration template.
///
/// This template includes documentation comments explaining each configuration option
/// and uses the default model and base URL constants.
///
/// The template has `enabled = true` but the API key is commented out,
/// requiring the user to explicitly configure their API key.
pub fn default_assistant_toml_template() -> String {
    format!(
        "# Kaku Assistant configuration\n\
#\n\
# enabled: true enables command analysis suggestions; false disables requests.\n\
# api_key: provider API key, example: \"sk-xxxx\".\n\
# model: Simple Model for quick command generation and lightweight chat.\n\
# chat_model: Deep Model for Cmd+L, k, and tool-using chat. Omit to reuse `model`.\n\
# chat_model_choices: optional curated list for the chat overlay. When set,\n\
#                     Kaku skips auto-fetching from /models and cycles only\n\
#                     through these entries.\n\
#                     example: [\"gpt-5.4\", \"gpt-5.4-mini\", \"claude-sonnet-4-6\"]\n\
# auto_fix_ignored_exit_codes: optional exit codes that should not trigger\n\
#                              automatic command-fix suggestions.\n\
#                              example: [2]\n\
# base_url: OpenAI-compatible API root URL.\n\
# api_mode: \"chat_completions\" (default) or \"responses\".\n\
# native_web_search: add the provider-hosted web_search tool in responses mode.\n\
# custom_headers: optional extra HTTP headers for enterprise proxies or API gateways.\n\
#                 format: [\"Header-Name: value\", \"Another-Header: value\"]\n\
#                 note: Authorization and Content-Type are reserved and cannot be overridden.\n\
\n\
enabled = true\n\
# api_key = \"<your_api_key>\"\n\
model = \"{DEFAULT_MODEL}\"\n\
chat_model = \"{DEFAULT_CHAT_MODEL}\"\n\
# auto_fix_ignored_exit_codes = []\n\
base_url = \"{DEFAULT_BASE_URL}\"\n\
# api_mode = \"responses\"\n\
# native_web_search = true\n\
# custom_headers = [\"X-Customer-ID: your-customer-id\"]\n\
# web_search_provider: optional web search backend for the chat agent.\n\
#   \"none\" (default) disables the third-party web_search tool.\n\
#   \"brave\" | \"pipellm\" | \"tavily\" enables both. Requires web_search_api_key.\n\
#   Responses native_web_search does not require either third-party setting.\n\
#   Configure via `kaku ai` instead of editing this file directly.\n\
#   Capabilities used per provider:\n\
#     brave:   web/news search, extra_snippets, freshness filter\n\
#     pipellm: simple-search, news-search, deep RAG search, page reader\n\
#     tavily:  search with direct AI answer, advanced depth, topic, page extract\n\
# web_search_api_key: API key for the chosen provider.\n\
# web_fetch_script: optional path to a local shell script invoked as\n\
#   `bash <script> <url>` when the agent fetches a web page.\n\
#   SECURITY: only set this to a script you personally wrote and trust.\n\
#   Never copy a web_fetch_script path from an untrusted source.\n"
    )
}

/// Ensures that required configuration keys exist in the assistant.toml file.
///
/// If the `model` or `base_url` keys are missing, they are added with their default values.
/// This ensures backward compatibility when new required fields are added.
///
/// # Arguments
/// * `path` - Path to the assistant.toml file
///
/// # Errors
/// Returns an error if the file cannot be read or written.
fn ensure_required_keys(path: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let (updated, changed) = ensure_required_keys_in_content(&raw);

    if changed {
        crate::utils::write_atomic(path, updated.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn ensure_required_keys_in_content(raw: &str) -> (String, bool) {
    let mut insert_lines = Vec::new();
    if !top_level_toml_has_key(raw, "model") {
        insert_lines.push(format!("model = \"{DEFAULT_MODEL}\""));
    }
    if !top_level_toml_has_key(raw, "base_url") {
        insert_lines.push(format!("base_url = \"{DEFAULT_BASE_URL}\""));
    }

    if insert_lines.is_empty() {
        return (raw.to_string(), false);
    }

    let insert_block = format!("{}\n", insert_lines.join("\n"));
    let insert_at = first_table_header_offset(raw).unwrap_or(raw.len());
    let (before, after) = raw.split_at(insert_at);
    let mut updated = String::with_capacity(raw.len() + insert_block.len() + 2);

    let before_trimmed = before.trim_end_matches(['\r', '\n']);
    updated.push_str(before_trimmed);
    if !before_trimmed.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&insert_block);
    if !after.is_empty() {
        updated.push_str(after.trim_start_matches(['\r', '\n']));
    }

    (updated, true)
}

fn first_table_header_offset(content: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let head = line.split('#').next().unwrap_or("").trim_start();
        if head.starts_with('[') {
            return Some(offset);
        }
        offset += line.len();
    }

    let trailing = &content[offset..];
    let head = trailing.split('#').next().unwrap_or("").trim_start();
    if head.starts_with('[') {
        return Some(offset);
    }
    None
}

/// Checks if a TOML top-level key exists in the given content.
///
/// This only scans lines before the first table header. Keys inside `[section]`
/// tables do not count as top-level keys.
///
/// # Arguments
/// * `content` - The TOML file content to search
/// * `key` - The key name to look for
///
/// # Returns
/// `true` if the key is found, `false` otherwise
fn top_level_toml_has_key(content: &str, key: &str) -> bool {
    for line in content.lines() {
        let head = line.split('#').next().unwrap_or("").trim();
        if head.is_empty() {
            continue;
        }
        if head.starts_with('[') {
            break;
        }
        if let Some((name, _)) = head.split_once('=') {
            if name.trim() == key {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn required_keys_update_preserves_symlink_permissions_and_user_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("dotfiles.toml");
        let link = dir.path().join("assistant.toml");
        let original = "# My settings\napi_key = \"user-key\"\nmodel = \"custom-model\"\n\n[provider]\nname = \"custom\"\n";
        std::fs::write(&target, original).expect("seed target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o400))
            .expect("protect target");
        symlink("dotfiles.toml", &link).expect("symlink");

        ensure_required_keys(&link).expect("insert missing base_url");

        assert_eq!(
            std::fs::read_link(&link).expect("read link"),
            Path::new("dotfiles.toml")
        );
        let saved = std::fs::read_to_string(&target).expect("read target");
        assert_eq!(
            saved,
            format!(
                "# My settings\napi_key = \"user-key\"\nmodel = \"custom-model\"\nbase_url = \"{DEFAULT_BASE_URL}\"\n[provider]\nname = \"custom\"\n"
            )
        );
        assert_eq!(
            std::fs::metadata(&target)
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
        ensure_required_keys(&link).expect("already complete config needs no write");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target again"),
            saved
        );
    }

    #[test]
    fn required_keys_write_failure_preserves_symlink_and_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target_dir = dir.path().join("dotfiles");
        std::fs::create_dir(&target_dir).expect("target dir");
        let target = target_dir.join("assistant.toml");
        let original = "# My settings\napi_key = \"user-key\"\n";
        std::fs::write(&target, original).expect("seed target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            .expect("target permissions");
        let link = dir.path().join("assistant.toml");
        symlink("dotfiles/assistant.toml", &link).expect("symlink");

        // The target remains writable, but a sibling temporary file cannot be
        // created. A direct write would overwrite the user's existing config.
        std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o500))
            .expect("protect target directory");
        let result = ensure_required_keys(&link);
        std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore target directory");

        assert!(result.is_err(), "write in protected directory succeeded");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            original
        );
        assert_eq!(
            std::fs::read_link(&link).expect("read link"),
            Path::new("dotfiles/assistant.toml")
        );
        assert_eq!(
            std::fs::metadata(&target)
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    // detect_provider* tests live in `kaku-gui/src/ai_client.rs` next to the
    // single canonical implementation. The previously duplicated tests here
    // were exercising dead code in this binary.

    #[test]
    fn top_level_key_check_ignores_table_keys() {
        let content = r#"
enabled = true

[provider]
model = "nested"
"#;
        assert!(!top_level_toml_has_key(content, "model"));
        assert!(top_level_toml_has_key(content, "enabled"));
    }

    #[test]
    fn inserts_missing_required_keys_before_first_table() {
        let content = r#"# header
enabled = true

[provider]
api_key = "x"
"#;
        let (updated, changed) = ensure_required_keys_in_content(content);
        assert!(changed);
        let model_pos = updated.find("model = ").expect("model inserted");
        let base_pos = updated.find("base_url = ").expect("base_url inserted");
        let table_pos = updated.find("[provider]").expect("table header");
        assert!(model_pos < table_pos);
        assert!(base_pos < table_pos);
        assert!(updated.contains("enabled = true"));
    }

    #[test]
    fn preserves_existing_top_level_required_keys() {
        let content = format!(
            "enabled = true\nmodel = \"{}\"\nbase_url = \"{}\"\n[provider]\nname = \"x\"\n",
            DEFAULT_MODEL, DEFAULT_BASE_URL
        );
        let (updated, changed) = ensure_required_keys_in_content(&content);
        assert!(!changed);
        assert_eq!(updated, content);
    }

    #[test]
    fn default_template_includes_custom_headers_hint() {
        let template = default_assistant_toml_template();
        assert!(template.contains("custom_headers"));
    }

    #[test]
    fn default_template_pins_simple_and_deep_model() {
        // Lock in the shipped defaults so new contributors do not silently
        // change them. Touching either constant must consciously update this
        // assertion.
        let template = default_assistant_toml_template();
        assert!(
            template.contains(&format!("model = \"{}\"", DEFAULT_MODEL)),
            "template must set simple model = {}",
            DEFAULT_MODEL
        );
        assert!(
            template.contains(&format!("chat_model = \"{}\"", DEFAULT_CHAT_MODEL)),
            "template must set deep model = {}",
            DEFAULT_CHAT_MODEL
        );
        assert!(
            !template
                .lines()
                .any(|line| line.starts_with("fast_model =")),
            "new default configs must not expose fast_model"
        );
    }
}
