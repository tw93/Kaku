//! Kaku Assistant configuration management.
//!
//! This module handles the configuration file for Kaku's built-in AI assistant,
//! including default values, file paths, and ensuring required configuration keys exist.
//!
//! The configuration is stored in `assistant.toml` in the user's Kaku config directory.

use crate::utils::write_atomic;
use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

/// Default AI model to use when none is specified.
/// Default model for command analysis suggestions.
pub const DEFAULT_MODEL: &str = "DeepSeek-V3.2";

/// Default API base URL for the AI service.
pub const DEFAULT_BASE_URL: &str = "https://api.vivgrid.com/v1";

/// A provider preset with its API base URL and available models.
pub struct ProviderPreset {
    /// Display name for the provider.
    pub name: &'static str,
    /// Base URL for the provider's OpenAI-compatible API.
    pub base_url: &'static str,
    /// Available model identifiers for this provider (empty = free-text).
    pub models: &'static [&'static str],
}

/// Built-in provider presets for the Kaku Assistant.
/// To add a new provider: add an entry here. The TUI and provider detection
/// derive everything (base_url, model list, auto-fill on selection) from this
/// table via provider_preset() and detect_provider(). No other changes needed.
pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "VivGrid",
        base_url: "https://api.vivgrid.com/v1",
        models: &["DeepSeek-V3.2"],
    },
    ProviderPreset {
        name: "MiniMax",
        base_url: "https://api.minimax.io/v1",
        models: &[
            "MiniMax-M2.7",
            "MiniMax-M2.7-highspeed",
            "MiniMax-M2.5",
            "MiniMax-M2.5-highspeed",
        ],
    },
    ProviderPreset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        models: &[],
    },
    ProviderPreset {
        name: "Custom",
        base_url: "",
        models: &[],
    },
];

/// Returns the provider preset matching the given name, if any.
pub fn provider_preset(name: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|p| p.name == name)
}

/// Returns all provider preset names as a `Vec<String>`.
pub fn provider_names() -> Vec<String> {
    PROVIDER_PRESETS
        .iter()
        .map(|p| p.name.to_string())
        .collect()
}

/// Detects the provider name from a base URL.
///
/// Returns the matching preset name if the base URL matches a known provider,
/// or `"Custom"` otherwise.
pub fn detect_provider(base_url: &str) -> &'static str {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    for preset in PROVIDER_PRESETS {
        if !preset.base_url.is_empty()
            && normalized == preset.base_url.trim_end_matches('/').to_ascii_lowercase()
        {
            return preset.name;
        }
    }
    "Custom"
}

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
        std::fs::write(&path, default_assistant_toml_template())
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

/// Reads whether Kaku Assistant is enabled.
///
/// Missing or malformed values fall back to `true` so the default template
/// remains the effective behavior until the user explicitly turns it off.
pub fn read_enabled() -> anyhow::Result<bool> {
    let path = ensure_assistant_toml_exists()?;
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;

    Ok(raw
        .parse::<toml::Value>()
        .ok()
        .and_then(|parsed| parsed.get("enabled").and_then(|value| value.as_bool()))
        .unwrap_or(true))
}

/// Writes the enabled flag while preserving the rest of assistant.toml.
pub fn write_enabled(enabled: bool) -> anyhow::Result<()> {
    let path = ensure_assistant_toml_exists()?;
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| default_assistant_toml_template());
    let updated = set_top_level_bool_key_in_content(&raw, "enabled", enabled);
    write_atomic(&path, updated.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
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
# model: model id, example: \"DeepSeek-V3.2\" or \"MiniMax-M2.7\".\n\
# base_url: chat-completions API root URL.\n\
# custom_headers: optional extra HTTP headers for enterprise proxies or API gateways.\n\
#                 format: [\"Header-Name: value\", \"Another-Header: value\"]\n\
#                 note: Authorization and Content-Type are reserved and cannot be overridden.\n\
\n\
enabled = true\n\
# api_key = \"<your_api_key>\"\n\
model = \"{DEFAULT_MODEL}\"\n\
base_url = \"{DEFAULT_BASE_URL}\"\n\
# custom_headers = [\"X-Customer-ID: your-customer-id\"]\n"
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
        std::fs::write(path, updated.as_bytes())
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

fn set_top_level_bool_key_in_content(content: &str, key: &str, value: bool) -> String {
    let replacement = format!("{key} = {value}");
    let mut updated_lines = Vec::new();
    let mut replaced = false;
    let mut in_top_level = true;

    for line in content.lines() {
        let head = line.split('#').next().unwrap_or("").trim_start();
        if in_top_level && head.starts_with('[') {
            in_top_level = false;
        }

        if in_top_level {
            let raw_head = line.split('#').next().unwrap_or("").trim();
            if let Some((name, _)) = raw_head.split_once('=') {
                if name.trim() == key {
                    updated_lines.push(replacement.clone());
                    replaced = true;
                    continue;
                }
            }
        }

        updated_lines.push(line.to_string());
    }

    if replaced {
        let mut updated = updated_lines.join("\n");
        if content.ends_with('\n') {
            updated.push('\n');
        }
        updated
    } else {
        let insert_block = format!("{replacement}\n");
        let insert_at = first_table_header_offset(content).unwrap_or(content.len());
        let (before, after) = content.split_at(insert_at);
        let mut updated = String::with_capacity(content.len() + insert_block.len() + 2);

        let before_trimmed = before.trim_end_matches(['\r', '\n']);
        updated.push_str(before_trimmed);
        if !before_trimmed.is_empty() {
            updated.push('\n');
        }
        updated.push_str(&insert_block);
        if !after.is_empty() {
            updated.push_str(after.trim_start_matches(['\r', '\n']));
        }
        updated
    }
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

    #[test]
    fn detect_provider_matches_known_urls() {
        assert_eq!(detect_provider("https://api.minimax.io/v1"), "MiniMax");
        assert_eq!(detect_provider("https://api.minimax.io/v1/"), "MiniMax");
        assert_eq!(detect_provider("https://api.vivgrid.com/v1"), "VivGrid");
        assert_eq!(detect_provider("https://api.openai.com/v1"), "OpenAI");
    }

    #[test]
    fn detect_provider_returns_custom_for_unknown_urls() {
        assert_eq!(detect_provider("https://my-proxy.example.com/v1"), "Custom");
        assert_eq!(detect_provider(""), "Custom");
    }

    #[test]
    fn provider_preset_lookup_returns_correct_preset() {
        let preset = provider_preset("MiniMax").expect("MiniMax preset");
        assert_eq!(preset.base_url, "https://api.minimax.io/v1");
        assert!(preset.models.contains(&"MiniMax-M2.7"));
        assert!(preset.models.contains(&"MiniMax-M2.7-highspeed"));
    }

    #[test]
    fn provider_preset_lookup_returns_none_for_unknown() {
        assert!(provider_preset("UnknownProvider").is_none());
    }

    #[test]
    fn provider_names_includes_all_presets() {
        let names = provider_names();
        assert!(names.contains(&"VivGrid".to_string()));
        assert!(names.contains(&"MiniMax".to_string()));
        assert!(names.contains(&"OpenAI".to_string()));
        assert!(names.contains(&"Custom".to_string()));
    }

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
    fn set_enabled_replaces_existing_value() {
        let content = "enabled = true\nmodel = \"x\"\n";
        let updated = set_top_level_bool_key_in_content(content, "enabled", false);
        assert_eq!(updated, "enabled = false\nmodel = \"x\"\n");
    }

    #[test]
    fn set_enabled_inserts_missing_value_before_table() {
        let content = "model = \"x\"\n\n[provider]\nname = \"y\"\n";
        let updated = set_top_level_bool_key_in_content(content, "enabled", true);
        let enabled_pos = updated.find("enabled = true").expect("enabled inserted");
        let table_pos = updated.find("[provider]").expect("table exists");
        assert!(enabled_pos < table_pos);
    }
}
