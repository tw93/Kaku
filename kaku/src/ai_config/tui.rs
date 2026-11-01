//! Interactive TUI for `kaku ai`.

use crate::assistant_config;
use crate::utils::{is_jsonc_path, open_path_in_editor, parse_json_or_jsonc, write_atomic};
use anyhow::Context;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

mod providers;
mod ui;

use providers::assistant::*;
use providers::codex::*;
use providers::*;

const FOLLOW_CODEX_MODEL: &str = "Follow Codex";
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    KakuAssistant,
    ClaudeCode,
    Codex,
    Kimi,
    Antigravity,
    Gemini,
    Copilot,
    FactoryDroid,
    OpenClaw,
}

impl Tool {
    fn label(&self) -> &'static str {
        match self {
            Tool::KakuAssistant => "Kaku Assistant",
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Kimi => "Kimi Code",
            Tool::Antigravity => "Antigravity",
            Tool::Gemini => "Gemini CLI",
            Tool::Copilot => "Copilot CLI",
            Tool::FactoryDroid => "Factory Droid",
            Tool::OpenClaw => "OpenClaw",
        }
    }

    fn config_path(&self) -> PathBuf {
        let home = config::HOME_DIR.clone();
        match self {
            Tool::KakuAssistant => assistant_config::assistant_toml_path().unwrap_or_else(|_| {
                config::HOME_DIR
                    .join(".config")
                    .join("kaku")
                    .join("assistant.toml")
            }),
            Tool::ClaudeCode => home.join(".claude").join("settings.json"),
            Tool::Codex => codex_home_dir().join("config.toml"),
            Tool::Kimi => home.join(".kimi").join("config.toml"),
            Tool::Antigravity => home
                .join("Library")
                .join("Application Support")
                .join("Antigravity")
                .join("User")
                .join("settings.json"),
            Tool::Gemini => home.join(".gemini").join("settings.json"),
            Tool::Copilot => home.join(".copilot").join("config.json"),
            Tool::FactoryDroid => home.join(".factory").join("settings.json"),
            Tool::OpenClaw => {
                let new_path = home.join(".openclaw").join("openclaw.json");
                if new_path.exists() {
                    return new_path;
                }
                let legacy = home.join(".clawdbot").join("clawdbot.json");
                if legacy.exists() {
                    return legacy;
                }
                new_path
            }
        }
    }
}

const ALL_TOOLS: [Tool; 9] = [
    Tool::KakuAssistant,
    Tool::ClaudeCode,
    Tool::Codex,
    Tool::Kimi,
    Tool::Antigravity,
    Tool::Gemini,
    Tool::Copilot,
    Tool::FactoryDroid,
    Tool::OpenClaw,
];

const FACTORY_DROID_DEFAULT_MODEL: &str = "opus";
const FACTORY_DROID_DEFAULT_REASONING: &str = "off";
const FACTORY_DROID_DEFAULT_AUTONOMY: &str = "normal";
const FACTORY_DROID_REASONING_OPTIONS: [&str; 5] = ["off", "none", "low", "medium", "high"];
const FACTORY_DROID_AUTONOMY_OPTIONS: [&str; 5] =
    ["normal", "spec", "auto-low", "auto-medium", "auto-high"];
const ASSISTANT_MODELS_CACHE_TTL: Duration = Duration::from_secs(1800);
const ASSISTANT_MODEL_FALLBACKS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
    "gpt-5.1",
    "gpt-5",
    "o3",
    "o4-mini",
    "claude-sonnet-4-6",
    "claude-opus-4-6",
];
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const UI_STATUS_TTL: Duration = Duration::from_secs(3);
const UI_ERROR_TTL: Duration = Duration::from_secs(5);

static UI_ERRORS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn ui_errors() -> &'static Mutex<Vec<String>> {
    UI_ERRORS.get_or_init(|| Mutex::new(Vec::new()))
}

fn push_ui_error(message: impl Into<String>) {
    let message = message.into();
    let mut guard = match ui_errors().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if guard.last().is_some_and(|last| last == &message) {
        return;
    }
    if guard.len() >= 8 {
        guard.remove(0);
    }
    guard.push(message);
}

fn pop_ui_error() -> Option<String> {
    let mut guard = ui_errors().lock().ok()?;
    if guard.is_empty() {
        None
    } else {
        Some(guard.remove(0))
    }
}

#[derive(Clone)]
struct FieldEntry {
    key: String,
    value: String,
    options: Vec<String>,
    editable: bool,
}

impl Default for FieldEntry {
    fn default() -> Self {
        Self {
            key: String::new(),
            value: String::new(),
            options: Vec::new(),
            editable: true,
        }
    }
}

struct ToolState {
    tool: Tool,
    installed: bool,
    fields: Vec<FieldEntry>,
    summary: Option<String>,
}

impl ToolState {
    fn load_without_remote_usage(tool: Tool) -> Self {
        Self::load_with_usage(tool, false)
    }

    fn load_with_usage(tool: Tool, eager_remote_usage: bool) -> Self {
        let path = if tool == Tool::KakuAssistant {
            match assistant_config::ensure_assistant_toml_exists() {
                Ok(path) => path,
                Err(err) => {
                    return ToolState {
                        tool,
                        installed: true,
                        fields: vec![FieldEntry {
                            key: "error".into(),
                            value: err.to_string(),
                            options: vec![],
                            editable: false,
                        }],
                        summary: Some("Setup failed".into()),
                    };
                }
            }
        } else {
            tool.config_path()
        };

        let extra_exists = match tool {
            Tool::Codex => codex_home_dir().join("auth.json").exists(),
            Tool::Kimi => config::HOME_DIR
                .join(".kimi")
                .join("credentials")
                .join("kimi-code.json")
                .exists(),
            _ => false,
        };

        if tool == Tool::Antigravity && !antigravity_app_bundle_path().exists() {
            return ToolState {
                tool,
                installed: false,
                fields: Vec::new(),
                summary: None,
            };
        }

        if tool != Tool::KakuAssistant && !path.exists() && !extra_exists {
            return ToolState {
                tool,
                installed: false,
                fields: Vec::new(),
                summary: None,
            };
        }

        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound && extra_exists => String::new(),
            Err(e) => {
                log::warn!("failed to read config for {}: {}", tool.label(), e);
                return ToolState {
                    tool,
                    installed: false,
                    fields: vec![FieldEntry {
                        key: "error".into(),
                        value: format!("failed to read config: {}", e),
                        options: vec![],
                        ..Default::default()
                    }],
                    summary: Some("Config unreadable".into()),
                };
            }
        };

        let (fields, usage_summary) = match tool {
            Tool::KakuAssistant => (extract_kaku_assistant_fields(&raw), None),
            Tool::ClaudeCode => {
                let parsed = parse_json_or_jsonc_with_debug(&raw, tool.label());
                (
                    extract_claude_code_fields(&parsed),
                    eager_remote_usage
                        .then(|| load_claude_usage_snapshot().and_then(|snapshot| snapshot.summary))
                        .flatten(),
                )
            }
            Tool::Codex => (
                extract_codex_fields(&raw),
                eager_remote_usage
                    .then(|| load_codex_usage_snapshot().and_then(|snapshot| snapshot.summary))
                    .flatten(),
            ),
            Tool::Kimi => (
                extract_kimi_fields(&raw),
                eager_remote_usage
                    .then(|| load_kimi_usage_snapshot().and_then(|snapshot| snapshot.summary))
                    .flatten(),
            ),
            Tool::Antigravity => {
                let snapshot = if eager_remote_usage {
                    load_antigravity_usage_snapshot()
                } else {
                    Some(load_cached_antigravity_usage_snapshot())
                };
                (
                    extract_antigravity_fields(snapshot.as_ref()),
                    snapshot.and_then(|snapshot| snapshot.summary),
                )
            }
            Tool::Gemini => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                (
                    extract_gemini_fields(&parsed),
                    gemini_quota_summary(&parsed),
                )
            }
            Tool::Copilot => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                (
                    extract_copilot_fields(&parsed),
                    eager_remote_usage
                        .then(|| {
                            load_copilot_usage_snapshot().and_then(|snapshot| snapshot.summary)
                        })
                        .flatten(),
                )
            }
            Tool::FactoryDroid => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                (extract_factory_droid_fields(&parsed), None)
            }
            Tool::OpenClaw => {
                let parsed = parse_json_or_jsonc_with_debug(&raw, tool.label());
                (extract_openclaw_fields(&parsed), None)
            }
        };

        let summary = if !eager_remote_usage && supports_remote_usage(tool) {
            Some("Loading usage...".into())
        } else {
            summarize_tool_fields(tool, true, &fields, usage_summary.as_deref())
        };

        ToolState {
            tool,
            installed: true,
            fields,
            summary,
        }
    }
}

fn field_value<'a>(fields: &'a [FieldEntry], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|field| field.key == key)
        .map(|field| field.value.as_str())
}

fn compact_summary_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }

    let value = value
        .strip_prefix("✓ ")
        .or_else(|| value.strip_prefix("✗ "))
        .unwrap_or(value)
        .trim();
    let value = value.strip_suffix(" (default)").unwrap_or(value).trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn summarize_tool_fields(
    tool: Tool,
    installed: bool,
    fields: &[FieldEntry],
    usage_summary: Option<&str>,
) -> Option<String> {
    if !installed {
        return None;
    }

    if tool == Tool::KakuAssistant {
        let model = field_value(fields, "Deep Model")
            .or_else(|| field_value(fields, "Chat Model"))
            .or_else(|| field_value(fields, "Simple Model"))
            .or_else(|| field_value(fields, "Model"))
            .and_then(compact_summary_value)?;
        // codex is "ready" when the CLI is logged in (no API key needed);
        // api_key auth needs a key present.
        let ready = if field_value(fields, "Auth Type") == Some("codex") {
            codex_connection_is_configured()
        } else {
            field_value(fields, "API Key").is_some_and(|value| value != "-")
        };
        if ready {
            return Some(format!("Ready · {model}"));
        }
        return Some(format!("Setup required · {model}"));
    }

    if matches!(
        tool,
        Tool::Codex | Tool::ClaudeCode | Tool::Kimi | Tool::Copilot
    ) {
        return usage_summary
            .map(str::to_string)
            .or_else(|| Some("Quota unavailable".into()));
    }

    if tool == Tool::Antigravity {
        return usage_summary.map(str::to_string).or_else(|| {
            field_value(fields, "Model")
                .map(|_| "Open Antigravity to sync quota".to_string())
                .or_else(|| Some("Open Antigravity to load quota".into()))
        });
    }

    if tool == Tool::Gemini {
        if let Some(summary) = usage_summary {
            return Some(summary.to_string());
        }
    }

    let mut parts = Vec::new();
    if let Some(account) = field_value(fields, "Auth").and_then(compact_summary_value) {
        parts.push(account);
    }

    let model = field_value(fields, "Model")
        .or_else(|| field_value(fields, "Primary Model"))
        .and_then(compact_summary_value);
    if let Some(model) = model {
        parts.push(model);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn parse_json_with_debug(raw: &str, tool_label: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|e| {
        log::warn!("failed to parse {} config json: {}", tool_label, e);
        push_ui_error(format!("{tool_label} config is invalid JSON"));
        serde_json::Value::Null
    })
}

fn parse_json_or_jsonc_with_debug(raw: &str, tool_label: &str) -> serde_json::Value {
    parse_json_or_jsonc(raw).unwrap_or_else(|e| {
        log::warn!("failed to parse {} config json/jsonc: {}", tool_label, e);
        push_ui_error(format!("{tool_label} config is malformed"));
        serde_json::Value::Null
    })
}

fn read_text_with_debug(path: &Path, context: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Some(raw),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            log::debug!("{context}: {} not found", path.display());
            None
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidData
            ) =>
        {
            log::warn!("{context}: failed to read {}: {}", path.display(), e);
            push_ui_error(format!(
                "{context}: cannot read {}. Check file permission or encoding.",
                path.display()
            ));
            None
        }
        Err(e) => {
            log::debug!("{context}: failed to read {}: {}", path.display(), e);
            None
        }
    }
}

fn parse_json_value_with_debug(raw: &str, context: &str) -> Option<serde_json::Value> {
    serde_json::from_str(raw)
        .map_err(|e| {
            log::warn!("{context}: failed to parse json: {}", e);
            push_ui_error(format!("{context}: invalid JSON format"));
        })
        .ok()
}

fn read_json_file_with_debug(path: &Path, context: &str) -> Option<serde_json::Value> {
    let raw = read_text_with_debug(path, context)?;
    parse_json_value_with_debug(&raw, context)
}

fn decode_jwt_payload_with_debug(token: &str, context: &str) -> Option<serde_json::Value> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        log::debug!("{context}: invalid jwt format");
        push_ui_error(format!("{context}: invalid token format"));
        return None;
    }

    let mut payload = parts[1].to_string();
    while payload.len() % 4 != 0 {
        payload.push('=');
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(&payload)
        .map_err(|e| {
            log::debug!("{context}: failed to decode jwt payload: {}", e);
            push_ui_error(format!("{context}: token payload decode failed"));
        })
        .ok()?;
    serde_json::from_slice(&decoded)
        .map_err(|e| {
            log::debug!("{context}: failed to parse jwt payload json: {}", e);
            push_ui_error(format!("{context}: token payload JSON is invalid"));
        })
        .ok()
}

fn json_str(val: &serde_json::Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn json_nested_or_top_str(
    val: &serde_json::Value,
    parent_key: &str,
    nested_key: &str,
    legacy_top_key: &str,
) -> String {
    val.get(parent_key)
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get(nested_key))
        .and_then(|v| v.as_str())
        .or_else(|| val.get(legacy_top_key).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

fn string_options(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_string()).collect()
}

fn mask_key(val: &str) -> String {
    if val.is_empty() {
        return "-".into();
    }
    let chars: Vec<char> = val.chars().collect();
    if chars.len() <= 12 {
        return "****".into();
    }
    // Show first 12 chars and last 4 chars (operate on chars, not bytes,
    // so a pasted multibyte key cannot panic on a byte-boundary slice).
    let head: String = chars[..12].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{}...{}", head, tail)
}

fn normalize_custom_header(value: &str) -> Option<String> {
    let raw = value.trim();
    if raw.is_empty() {
        return None;
    }
    let (name, header_value) = raw.split_once(':')?;
    let name = name.trim();
    let header_value = header_value.trim();
    if name.is_empty() || header_value.is_empty() {
        return None;
    }
    if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("content-type") {
        return None;
    }
    Some(format!("{name}: {header_value}"))
}

fn normalize_custom_headers(values: Vec<String>) -> Vec<String> {
    let mut dedup = HashSet::new();
    values
        .into_iter()
        .filter_map(|item| normalize_custom_header(&item))
        .filter(|header| {
            let key = header
                .split_once(':')
                .map(|(name, _)| name)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            dedup.insert(key)
        })
        .collect()
}

fn parse_custom_headers_toml(value: Option<&toml::Value>) -> Vec<String> {
    match value {
        Some(toml::Value::Array(items)) => normalize_custom_headers(
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect(),
        ),
        Some(toml::Value::String(raw)) => normalize_custom_headers(
            raw.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect(),
        ),
        _ => vec![],
    }
}

fn normalize_exit_codes(values: Vec<i32>) -> Vec<i32> {
    let mut dedup = HashSet::new();
    let mut out: Vec<i32> = values
        .into_iter()
        .filter(|code| *code >= 0)
        .filter(|code| dedup.insert(*code))
        .collect();
    out.sort_unstable();
    out
}

fn parse_exit_codes_toml(value: Option<&toml::Value>) -> Vec<i32> {
    match value {
        Some(toml::Value::Array(items)) => normalize_exit_codes(
            items
                .iter()
                .filter_map(|v| v.as_integer())
                .filter_map(|v| i32::try_from(v).ok())
                .collect(),
        ),
        Some(toml::Value::String(raw)) => normalize_exit_codes(
            raw.split(',')
                .filter_map(|part| part.trim().parse::<i32>().ok())
                .collect(),
        ),
        _ => vec![],
    }
}

/// Configuration for the Kaku built-in AI assistant.
///
/// This struct holds the configuration for Kaku's AI-powered command analysis
/// feature. It ensures that model and base_url always have valid values
/// by falling back to defaults when empty strings are provided.
#[derive(Debug, Clone)]
struct KakuAssistantConfig {
    /// Whether the AI assistant is enabled
    enabled: bool,
    /// API key for the AI service (may be empty if not configured)
    api_key: String,
    /// Model identifier (never empty, falls back to default)
    model: String,
    /// Base URL for the API endpoint (never empty, falls back to default)
    base_url: String,
    /// Auth mechanism: "api_key", "copilot", or "codex". Stored as opaque
    /// round-trip value; TUI does not branch on this.
    auth_type: String,
    /// API transport for API-key/custom endpoints.
    api_mode: String,
    /// Whether Responses requests include the provider-hosted web_search tool.
    native_web_search: bool,
    /// Optional extra request headers as `Name: Value`
    custom_headers: Vec<String>,
    /// Deep chat model. Empty means reuse Simple Model.
    chat_model: String,
    /// Optional curated chat-overlay model list. Round-tripped through the TUI
    /// to avoid losing user choices made via direct edits or the GUI.
    chat_model_choices: Vec<String>,
    /// Exit codes that should not trigger automatic command-fix suggestions.
    auto_fix_ignored_exit_codes: Vec<i32>,
    /// Web search backend: "none" (disabled), "brave", "pipellm", or "tavily".
    web_search_provider: String,
    /// API key for the web search provider (empty = not configured).
    web_search_api_key: String,
}

impl KakuAssistantConfig {
    /// Creates a new configuration with the given values.
    ///
    /// # Arguments
    /// * `enabled` - Whether the assistant is enabled
    /// * `api_key` - API key (empty string if not set)
    /// * `model` - Model identifier (empty strings will be replaced with default)
    /// * `base_url` - Base URL (empty strings will be replaced with default)
    fn new(
        enabled: bool,
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let model = model.into();
        let base_url = base_url.into();
        let resolved_base_url = if base_url.trim().is_empty() {
            assistant_config::DEFAULT_BASE_URL.to_string()
        } else {
            base_url
        };
        Self {
            enabled,
            api_key: api_key.into(),
            model: if model.trim().is_empty() {
                assistant_config::DEFAULT_MODEL.to_string()
            } else {
                model
            },
            base_url: resolved_base_url,
            auth_type: "api_key".to_string(),
            api_mode: "chat_completions".to_string(),
            native_web_search: false,
            custom_headers: vec![],
            chat_model: String::new(),
            chat_model_choices: Vec::new(),
            auto_fix_ignored_exit_codes: Vec::new(),
            web_search_provider: "none".to_string(),
            web_search_api_key: String::new(),
        }
    }

    fn with_custom_headers(mut self, custom_headers: Vec<String>) -> Self {
        self.custom_headers = normalize_custom_headers(custom_headers);
        self
    }

    /// Returns whether the assistant is enabled.
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the API key (may be empty).
    fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the model identifier (never empty).
    fn model(&self) -> &str {
        &self.model
    }

    /// Returns the effective deep model, falling back to Simple Model.
    fn deep_model(&self) -> &str {
        if self.chat_model.trim().is_empty() {
            self.model()
        } else {
            &self.chat_model
        }
    }

    /// Returns the base URL (never empty).
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn auth_type(&self) -> &str {
        &self.auth_type
    }

    fn api_mode(&self) -> &str {
        &self.api_mode
    }

    fn native_web_search(&self) -> bool {
        self.native_web_search
    }

    fn web_search_provider(&self) -> &str {
        &self.web_search_provider
    }

    fn web_search_api_key(&self) -> &str {
        &self.web_search_api_key
    }

    fn with_chat_model(mut self, chat_model: impl Into<String>) -> Self {
        self.chat_model = chat_model.into();
        self
    }

    fn with_web_search(mut self, provider: impl Into<String>, api_key: impl Into<String>) -> Self {
        self.web_search_provider = provider.into();
        self.web_search_api_key = api_key.into();
        self
    }

    fn custom_headers(&self) -> &[String] {
        &self.custom_headers
    }

    fn chat_model(&self) -> &str {
        &self.chat_model
    }

    fn chat_model_choices(&self) -> &[String] {
        &self.chat_model_choices
    }

    fn auto_fix_ignored_exit_codes(&self) -> &[i32] {
        &self.auto_fix_ignored_exit_codes
    }

    /// Carry over the chat-overlay model choices from another config.
    ///
    /// The TUI edits `chat_model`, but not the curated `chat_model_choices`;
    /// without this passthrough, every save would silently drop direct edits
    /// or GUI-curated choices.
    fn with_chat_model_passthrough(mut self, src: &KakuAssistantConfig) -> Self {
        if self.chat_model.is_empty() {
            self.chat_model = src.chat_model.clone();
        }
        self.chat_model_choices = src.chat_model_choices.clone();
        self
    }

    /// Carry over only `chat_model_choices` from another config, leaving
    /// `chat_model` untouched. Use this in save arms that set `chat_model`
    /// explicitly, including clearing it to empty.
    fn with_chat_model_choices_passthrough(mut self, src: &KakuAssistantConfig) -> Self {
        self.chat_model_choices = src.chat_model_choices.clone();
        self
    }

    fn with_auto_fix_ignored_exit_codes(mut self, exit_codes: Vec<i32>) -> Self {
        self.auto_fix_ignored_exit_codes = normalize_exit_codes(exit_codes);
        self
    }
}

impl Default for KakuAssistantConfig {
    fn default() -> Self {
        Self::new(true, String::new(), String::new(), String::new())
    }
}

// Delegated to kaku-ai-utils crate to avoid cross-binary drift.

fn parse_assistant_models_cache(path: &Path, base_url: &str) -> Option<Vec<String>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    if v.get("base_url").and_then(|s| s.as_str())? != base_url {
        return None;
    }
    let models: Vec<String> = v
        .get("models")
        .and_then(|a| a.as_array())?
        .iter()
        .filter_map(|s| s.as_str().map(String::from))
        .collect();
    if models.is_empty() {
        None
    } else {
        Some(models)
    }
}

fn load_assistant_models_cache(path: &Path, base_url: &str) -> Option<Vec<String>> {
    let elapsed = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())?;
    if elapsed >= ASSISTANT_MODELS_CACHE_TTL {
        return None;
    }
    parse_assistant_models_cache(path, base_url)
}

fn load_assistant_models_stale_cache(path: &Path, base_url: &str) -> Vec<String> {
    parse_assistant_models_cache(path, base_url).unwrap_or_default()
}

fn save_assistant_models_cache(path: &Path, base_url: &str, models: &[String]) {
    let value = serde_json::json!({ "base_url": base_url, "models": models });
    write_json_cache(path, &value);
}

/// Fetch available chat models from the assistant API's `/models` endpoint.
///
/// Uses a 3-second curl timeout and writes successful results to disk. Callers
/// should prefer `assistant_model_options_for_config` during TUI startup so the
/// remote fetch happens in the background instead of blocking first paint.
///
/// The API key is passed via stdin (curl --config -) to avoid exposing it in
/// process arguments visible to ps/audit logs.
fn fetch_kaku_assistant_models(api_key: &str, base_url: &str) -> Vec<String> {
    let api_key = api_key.trim();
    let base_url = base_url.trim().trim_end_matches('/');
    if api_key.is_empty() || base_url.is_empty() {
        return Vec::new();
    }

    let cache_path = assistant_models_cache_path();

    let url = format!("{}/models", base_url);
    // Pass Authorization header via --config - (stdin) to avoid argv exposure.
    let authorization = format!("Bearer {api_key}");
    let curl_config = curl_config_header("Authorization", &authorization);
    let mut child = match std::process::Command::new("/usr/bin/curl")
        .args(["-sS", "--fail", "--max-time", "3", "--config", "-", &url])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::debug!("assistant models fetch: curl spawn failed: {}", e);
            return load_assistant_models_stale_cache(&cache_path, base_url);
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(curl_config.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            log::debug!("assistant models fetch: curl wait failed: {}", e);
            return load_assistant_models_stale_cache(&cache_path, base_url);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "assistant models fetch: curl failed ({}): {}",
            output.status,
            stderr.trim()
        );
        return load_assistant_models_stale_cache(&cache_path, base_url);
    }

    let raw = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("assistant models fetch: non-utf8 response: {}", e);
            return load_assistant_models_stale_cache(&cache_path, base_url);
        }
    };

    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            log::debug!("assistant models fetch: JSON parse failed: {}", e);
            return load_assistant_models_stale_cache(&cache_path, base_url);
        }
    };

    let mut models: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|s| s.as_str()).map(String::from))
                .filter(|id| kaku_ai_utils::is_chat_model_id(id))
                .collect()
        })
        .unwrap_or_default();

    models.sort();
    models.dedup();

    if !models.is_empty() {
        save_assistant_models_cache(&cache_path, base_url, &models);
    }
    models
}

fn normalize_assistant_model_options(
    mut models: Vec<String>,
    simple_model: &str,
    deep_model: &str,
) -> Vec<String> {
    models.retain(|m| !m.trim().is_empty());
    models.sort();
    models.dedup();

    let mut ordered = Vec::new();

    for preferred in [simple_model.trim(), deep_model.trim()] {
        if preferred.is_empty() || ordered.iter().any(|m| m == preferred) {
            continue;
        }
        ordered.push(preferred.to_string());
    }

    for model in models {
        if !ordered.iter().any(|m| m == &model) {
            ordered.push(model);
        }
    }

    ordered
}

fn assistant_model_options_for_config(cfg: &KakuAssistantConfig) -> Vec<String> {
    let cache_path = assistant_models_cache_path();
    let base_url = cfg.base_url().trim().trim_end_matches('/');
    let mut models = load_assistant_models_cache(&cache_path, base_url)
        .unwrap_or_else(|| load_assistant_models_stale_cache(&cache_path, base_url));

    if models.is_empty() {
        models = ASSISTANT_MODEL_FALLBACKS
            .iter()
            .map(|m| (*m).to_string())
            .collect();
    }

    normalize_assistant_model_options(models, cfg.model(), cfg.deep_model())
}

fn assistant_model_options_for_config_remote(cfg: &KakuAssistantConfig) -> Vec<String> {
    let mut models = fetch_kaku_assistant_models(cfg.api_key(), cfg.base_url());

    if models.is_empty() {
        models = ASSISTANT_MODEL_FALLBACKS
            .iter()
            .map(|m| (*m).to_string())
            .collect();
    }

    normalize_assistant_model_options(models, cfg.model(), cfg.deep_model())
}

fn render_toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

/// Get Gemini account email from google_accounts.json
fn get_gemini_account() -> Option<String> {
    let path = config::HOME_DIR
        .join(".gemini")
        .join("google_accounts.json");

    let parsed = read_json_file_with_debug(&path, "gemini account")?;

    // Extract "active" field
    parsed.get("active")?.as_str().map(|s| s.to_string())
}

/// Get GitHub Copilot username from gh CLI
fn get_copilot_account() -> Option<String> {
    let output = match std::process::Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("gh account probe failed to launch: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "gh account probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("gh account probe returned non-utf8 stdout: {}", e))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_claude_auth_status_json() -> Option<serde_json::Value> {
    let output = match std::process::Command::new("claude")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("claude auth status probe failed to launch: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "claude auth status probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    let json_str = String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("claude auth status probe returned non-utf8 stdout: {}", e))
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| log::debug!("failed to parse claude auth status json: {}", e))
        .ok()?;
    Some(parsed)
}

fn parse_claude_auth_status(parsed: &serde_json::Value) -> Option<String> {
    let logged_in = parsed.get("loggedIn").and_then(|value| value.as_bool());
    if matches!(logged_in, Some(false)) {
        return Some("✗ not signed in".into());
    }

    let account = parsed
        .get("email")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let auth_method = parsed
        .get("authMethod")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("oauth");
    // `subscriptionType` is e.g. "max" / "pro"; render as " · Max" / " · Pro"
    // suffix when present.
    let plan = parsed
        .get("subscriptionType")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());

    if logged_in == Some(true) || account.is_some() {
        Some(format_auth_status(account, auth_method, plan))
    } else {
        None
    }
}

fn read_claude_auth_status() -> Option<String> {
    read_claude_auth_status_json()
        .as_ref()
        .and_then(parse_claude_auth_status)
        .or_else(|| {
            read_claude_oauth_access_token().map(|_| format_auth_status(None, "oauth", None))
        })
}

fn kimi_credentials_path() -> PathBuf {
    config::HOME_DIR
        .join(".kimi")
        .join("credentials")
        .join("kimi-code.json")
}

fn read_kimi_auth_status_from_path(path: &Path) -> String {
    let Some(auth) = read_json_file_with_debug(path, "kimi credentials") else {
        return "✗ not signed in".into();
    };

    let has_token = auth
        .get("access_token")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty());
    let expires_at = auth.get("expires_at").and_then(|value| value.as_f64());
    let has_refresh_token = auth
        .get("refresh_token")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);

    if has_refresh_token || (has_token && expires_at.is_some_and(|expires_at| expires_at > now)) {
        "✓ oauth".into()
    } else {
        "✗ login required".into()
    }
}

fn kimi_config_model_options(parsed: &toml::Value) -> Vec<String> {
    parsed
        .get("models")
        .and_then(|value| value.as_table())
        .map(|table| table.keys().cloned().collect::<Vec<_>>())
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| vec!["kimi-code/kimi-for-coding".into()])
}

fn read_kimi_auth_status() -> String {
    read_kimi_auth_status_from_path(&kimi_credentials_path())
}

/// Format auth status, with account fallback to auth method.
///
/// `plan` is optional and surfaced as a trailing `· Max` / `· Pro` suffix
/// when the provider exposes a subscription tier (Claude
/// `subscriptionType`, Codex JWT `chatgpt_plan_type`). Other providers pass
/// `None` and render unchanged.
fn format_auth_status(
    account: Option<String>,
    fallback_method: &str,
    plan: Option<&str>,
) -> String {
    let mut out = match account {
        Some(acc) if !acc.is_empty() => format!("✓ {}", acc),
        _ => format!("✓ {}", fallback_method),
    };
    if let Some(plan) = plan {
        let label = title_case_first(plan.trim());
        if !label.is_empty() {
            out.push_str(" · ");
            out.push_str(&label);
        }
    }
    out
}

/// Capitalize the first character; rest unchanged. Used to render
/// provider-supplied plan strings like `max` / `pro` as `Max` / `Pro`.
fn title_case_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Fetch models.dev data, cached to ~/.cache/kaku/models.json.
/// No TTL - use `r` key in TUI to force refresh.
fn load_models_dev_json() -> Option<serde_json::Value> {
    let cache_dir = config::HOME_DIR.join(".cache").join("kaku");
    let cache_path = cache_dir.join("models.json");

    // Use cache if exists
    if let Some(raw) = read_text_with_debug(&cache_path, "models.dev cache") {
        if let Some(v) = parse_json_value_with_debug(&raw, "models.dev cache") {
            return Some(v);
        }
    }

    // Fetch from API via curl (macOS built-in)
    fetch_models_dev_json()
}

/// Force fetch from models.dev and update cache.
fn fetch_models_dev_json() -> Option<serde_json::Value> {
    let cache_dir = config::HOME_DIR.join(".cache").join("kaku");
    let cache_path = cache_dir.join("models.json");

    let output = match std::process::Command::new("curl")
        .args(["-sS", "--max-time", "10", "https://models.dev/api.json"])
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("models.dev fetch failed to launch curl: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "models.dev fetch failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("models.dev fetch returned non-utf8 stdout: {}", e))
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| log::debug!("failed to parse models.dev json: {}", e))
        .ok()?;
    if let Err(e) = config::create_user_owned_dirs(&cache_dir) {
        log::debug!("Failed to create cache directory: {}", e);
    }
    if let Err(e) = std::fs::write(&cache_path, &raw) {
        log::debug!("Failed to write models cache: {}", e);
    }
    Some(v)
}

/// Read model IDs from models.dev for a given provider.
/// Returns latest models sorted by release_date (newest first), deduped to
/// only keep the short alias (e.g. "claude-sonnet-4-5") and skip dated
/// variants (e.g. "claude-sonnet-4-5-20250929").
fn read_models_dev(provider_id: &str) -> Vec<String> {
    let parsed = match load_models_dev_json() {
        Some(v) => v,
        None => return Vec::new(),
    };
    let models = match parsed
        .get(provider_id)
        .and_then(|p| p.get("models"))
        .and_then(|m| m.as_object())
    {
        Some(m) => m,
        None => return Vec::new(),
    };

    // Collect (id, release_date) pairs, skip embedding/tts/image-only models
    let mut items: Vec<(&str, &str)> = models
        .iter()
        .filter_map(|(id, m)| {
            // Skip non-chat models
            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.contains("Embedding")
                || name.contains("TTS")
                || name.contains("Image")
                || name.contains("Live")
            {
                return None;
            }
            // Skip dated variants (e.g. "claude-opus-4-5-20251101", "gemini-2.5-flash-preview-06-17")
            if id.chars().rev().take(8).all(|c| c.is_ascii_digit()) {
                return None;
            }
            // Skip dated preview variants with "xx-xx" suffix (e.g. "09-2025", "06-17")
            // Require both segments ≥ 2 chars to avoid filtering version numbers like "4-5"
            if let Some(last_dash) = id.rfind('-') {
                let suffix = &id[last_dash + 1..];
                if let Some(second_dash) = id[..last_dash].rfind('-') {
                    let prev = &id[second_dash + 1..last_dash];
                    if prev.len() >= 2
                        && prev.len() <= 4
                        && suffix.len() >= 2
                        && suffix.len() <= 4
                        && prev.chars().all(|c| c.is_ascii_digit())
                        && suffix.chars().all(|c| c.is_ascii_digit())
                    {
                        return None;
                    }
                }
            }
            // Skip "-latest" aliases (e.g. "gemini-flash-latest")
            if id.ends_with("-latest") {
                return None;
            }
            let rd = m.get("release_date").and_then(|v| v.as_str()).unwrap_or("");
            Some((id.as_str(), rd))
        })
        .collect();

    items.sort_by(|a, b| b.1.cmp(a.1));
    items
        .into_iter()
        .take(4)
        .map(|(id, _)| id.to_string())
        .collect()
}

fn extract_claude_code_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let model_options = read_models_dev("anthropic");

    let display_value = if model.is_empty() {
        // Show the latest model name as default hint
        model_options
            .first()
            .cloned()
            .unwrap_or_else(|| "default".into())
    } else {
        model
    };

    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: display_value,
        options: model_options,
        ..Default::default()
    }];

    if let Some(auth_status) = read_claude_auth_status() {
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: auth_status,
            options: vec![],
            editable: false,
        });
    }

    // Show env-based provider config if present (e.g. OpenRouter, Pipe AI, Kimi)
    if let Some(env) = val.get("env").and_then(|e| e.as_object()) {
        let base_url = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let auth_token = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !base_url.is_empty() {
            fields.push(FieldEntry {
                key: "Base URL".into(),
                value: base_url,
                options: vec![],
                ..Default::default()
            });
        }
        if !auth_token.is_empty() {
            fields.push(FieldEntry {
                key: "API Key".into(),
                value: mask_key(&auth_token),
                options: vec![],
                ..Default::default()
            });
        }
    }

    fields
}

fn extract_kimi_fields(raw: &str) -> Vec<FieldEntry> {
    let parsed = raw.parse::<toml::Value>().ok();
    let model_options = parsed
        .as_ref()
        .map(kimi_config_model_options)
        .unwrap_or_else(|| vec!["kimi-code/kimi-for-coding".into()]);
    let model = parsed
        .as_ref()
        .and_then(|parsed| parsed.get("default_model"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let display_value = if model.is_empty() {
        model_options
            .first()
            .cloned()
            .unwrap_or_else(|| "kimi-code/kimi-for-coding".into())
    } else {
        model
    };

    vec![
        FieldEntry {
            key: "Model".into(),
            value: display_value,
            options: model_options,
            ..Default::default()
        },
        FieldEntry {
            key: "Auth".into(),
            value: read_kimi_auth_status(),
            options: vec![],
            editable: false,
        },
    ]
}

fn extract_antigravity_fields(snapshot: Option<&AntigravityUsageSnapshot>) -> Vec<FieldEntry> {
    let auth = read_antigravity_auth_status();
    let account = auth.as_ref().and_then(|status| {
        status
            .get("email")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .or_else(|| {
                status
                    .get("name")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            })
    });

    let mut fields = Vec::new();

    if let Some(snapshot) = snapshot {
        if let Some(label) = &snapshot.selected_model_label {
            fields.push(FieldEntry {
                key: "Model".into(),
                value: label.clone(),
                options: vec![],
                editable: false,
            });
        }
    }

    if auth.is_some() {
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, "oauth", None),
            options: vec![],
            editable: false,
        });
    }

    fields
}

fn extract_gemini_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let mut fields = Vec::new();

    let model = val
        .get("model")
        .and_then(|m| {
            m.get("name")
                .and_then(|n| n.as_str())
                .or_else(|| m.as_str())
        })
        .unwrap_or("")
        .to_string();
    let model_options = read_models_dev("google");

    let display_value = if model.is_empty() {
        model_options
            .first()
            .cloned()
            .unwrap_or_else(|| "default".into())
    } else {
        model
    };

    fields.push(FieldEntry {
        key: "Model".into(),
        value: display_value,
        options: model_options,
        ..Default::default()
    });

    let auth_type = val
        .get("security")
        .and_then(|s| s.get("auth"))
        .and_then(|a| a.get("selectedType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !auth_type.is_empty() {
        let account = get_gemini_account();
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, auth_type, None),
            options: vec![],
            editable: false,
        });
    }

    fields
}

/// Read model choices from `copilot --help` output, fallback to models.dev.
fn read_copilot_model_options() -> Vec<String> {
    let output = match std::process::Command::new("copilot").arg("--help").output() {
        Ok(output) => output,
        Err(e) => {
            log::debug!("copilot --help probe failed to launch: {}", e);
            return read_models_dev("anthropic");
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "copilot --help probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return read_models_dev("anthropic");
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Find "--model" first, then parse the choices after it
    if let Some(model_pos) = text.find("--model") {
        let after_model = &text[model_pos..];
        if let Some(choices_pos) = after_model.find("choices:") {
            let rest = &after_model[choices_pos + "choices:".len()..];
            if let Some(end) = rest.find(')') {
                let choices_str = &rest[..end];
                let models: Vec<String> = choices_str
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !models.is_empty() {
                    return models;
                }
            }
        }
    }

    log::debug!("copilot --help output did not expose model choices");
    read_models_dev("anthropic")
}

fn extract_copilot_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let model_options = read_copilot_model_options();

    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: if model.is_empty() {
            "default".into()
        } else {
            model
        },
        options: model_options,
        ..Default::default()
    }];

    // Copilot authenticates via GitHub OAuth; session files indicate auth
    let session_dir = config::HOME_DIR.join(".copilot").join("session-state");
    if session_dir.exists() {
        let account = get_copilot_account();
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, "github", None),
            options: vec![],
            editable: false,
        });
    }

    fields
}

fn extract_factory_droid_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let session_defaults = val
        .get("sessionDefaultSettings")
        .and_then(|v| v.as_object());
    let model = json_nested_or_top_str(val, "sessionDefaultSettings", "model", "model");
    let reasoning = json_nested_or_top_str(
        val,
        "sessionDefaultSettings",
        "reasoningEffort",
        "reasoningEffort",
    );
    let autonomy = session_defaults
        .and_then(|s| s.get("autonomyMode").or_else(|| s.get("autonomyLevel")))
        .and_then(|v| v.as_str())
        .or_else(|| {
            val.get("autonomyMode")
                .or_else(|| val.get("autonomyLevel"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();

    vec![
        FieldEntry {
            key: "Model".into(),
            value: if model.is_empty() {
                FACTORY_DROID_DEFAULT_MODEL.into()
            } else {
                model
            },
            options: vec![],
            editable: true,
        },
        FieldEntry {
            key: "Reasoning Effort".into(),
            value: if reasoning.is_empty() {
                FACTORY_DROID_DEFAULT_REASONING.into()
            } else {
                reasoning
            },
            options: string_options(&FACTORY_DROID_REASONING_OPTIONS),
            editable: true,
        },
        FieldEntry {
            key: "Autonomy Level".into(),
            value: if autonomy.is_empty() {
                FACTORY_DROID_DEFAULT_AUTONOMY.into()
            } else {
                autonomy
            },
            options: string_options(&FACTORY_DROID_AUTONOMY_OPTIONS),
            editable: true,
        },
    ]
}

fn factory_droid_session_key(field_key: &str) -> Option<&'static str> {
    match field_key {
        "Model" => Some("model"),
        "Reasoning Effort" => Some("reasoningEffort"),
        // Normalize writes to `autonomyMode` while still accepting legacy `autonomyLevel`.
        "Autonomy Level" => Some("autonomyMode"),
        _ => None,
    }
}

fn save_factory_droid_field(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    field_key: &str,
    new_val: &str,
) -> anyhow::Result<()> {
    let Some(target_key) = factory_droid_session_key(field_key) else {
        return Ok(());
    };

    let session_defaults = obj
        .entry("sessionDefaultSettings")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("sessionDefaultSettings is not an object")?;

    if new_val == "-" || new_val.is_empty() {
        session_defaults.remove(target_key);
    } else {
        session_defaults.insert(
            target_key.to_string(),
            serde_json::Value::String(new_val.to_string()),
        );
    }

    Ok(())
}

fn save_gemini_model(obj: &mut serde_json::Map<String, serde_json::Value>, new_val: &str) {
    let clear = new_val == "-" || new_val.is_empty();
    if let Some(model) = obj.get_mut("model").and_then(|m| m.as_object_mut()) {
        // Only touch `name` so other model settings (e.g. maxSessionTurns) survive.
        if clear {
            model.remove("name");
            if model.is_empty() {
                obj.remove("model");
            }
        } else {
            model.insert(
                "name".to_string(),
                serde_json::Value::String(new_val.to_string()),
            );
        }
        return;
    }

    if clear {
        obj.remove("model");
    } else {
        let keep_string_shape = obj.get("model").is_some_and(|m| m.is_string());
        if keep_string_shape {
            obj.insert(
                "model".to_string(),
                serde_json::Value::String(new_val.to_string()),
            );
        } else {
            obj.insert("model".to_string(), serde_json::json!({"name": new_val}));
        }
    }
}

fn extract_openclaw_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let agents = val.get("agents").unwrap_or(&serde_json::Value::Null);
    let defaults = agents.get("defaults").unwrap_or(&serde_json::Value::Null);
    let model = defaults.get("model").unwrap_or(&serde_json::Value::Null);

    let primary = model
        .get("primary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Collect all model IDs from all providers for the Primary Model selector
    let mut all_model_ids: Vec<String> = Vec::new();

    let mut provider_fields: Vec<FieldEntry> = Vec::new();

    // Dynamically enumerate all providers
    if let Some(providers) = val
        .get("models")
        .and_then(|m| m.get("providers"))
        .and_then(|p| p.as_object())
    {
        for (name, prov) in providers {
            let url = prov
                .get("baseUrl")
                .or_else(|| prov.get("baseURL"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key = prov
                .get("apiKey")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Collect models from this provider
            let model_ids: Vec<String> = prov
                .get("models")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                        .map(|id| format!("{}/{}", name, id))
                        .collect()
                })
                .unwrap_or_default();

            // Also check agents.defaults.models for any registered models with this provider
            if let Some(agent_models) = defaults.get("models").and_then(|m| m.as_object()) {
                for model_key in agent_models.keys() {
                    if model_key.starts_with(&format!("{}/", name))
                        && !all_model_ids.contains(model_key)
                    {
                        all_model_ids.push(model_key.clone());
                    }
                }
            }

            for mid in &model_ids {
                if !all_model_ids.contains(mid) {
                    all_model_ids.push(mid.clone());
                }
            }

            let models_display = if model_ids.is_empty() {
                // Show from agent defaults if provider models array is empty
                all_model_ids
                    .iter()
                    .filter(|m| m.starts_with(&format!("{}/", name)))
                    .map(|m| m.split('/').last().unwrap_or(m).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                model_ids
                    .iter()
                    .map(|m| m.split('/').last().unwrap_or(m).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            // Provider header
            let api_type = prov
                .get("api")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            provider_fields.push(FieldEntry {
                key: name.clone(),
                value: if api_type.is_empty() {
                    "provider".into()
                } else {
                    api_type
                },
                options: vec![],
                editable: false,
            });

            provider_fields.push(FieldEntry {
                key: format!("{} ▸ Base URL", name),
                value: if url.is_empty() { "-".into() } else { url },
                options: vec![],
                ..Default::default()
            });
            provider_fields.push(FieldEntry {
                key: format!("{} ▸ API Key", name),
                value: mask_key(&key),
                options: vec![],
                ..Default::default()
            });
            if !models_display.is_empty() {
                provider_fields.push(FieldEntry {
                    key: format!("{} ▸ Models", name),
                    value: models_display,
                    options: vec![],
                    editable: false,
                });
            }
        }
    }

    // Fallback: if no providers defined, collect models from agents.defaults.models
    if all_model_ids.is_empty() {
        if let Some(agent_models) = defaults.get("models").and_then(|m| m.as_object()) {
            for model_key in agent_models.keys() {
                all_model_ids.push(model_key.clone());
            }
        }
    }

    let mut fields = vec![FieldEntry {
        key: "Primary Model".into(),
        value: if primary.is_empty() {
            "-".into()
        } else {
            primary
        },
        options: all_model_ids,
        ..Default::default()
    }];

    fields.extend(provider_fields);

    let plugins = val
        .get("plugins")
        .and_then(|p| p.get("entries"))
        .and_then(|e| e.as_object())
        .map(|obj| {
            obj.keys()
                .filter(|k| {
                    obj.get(*k)
                        .and_then(|v| v.get("enabled"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    fields.push(FieldEntry {
        key: "Plugins".into(),
        value: if plugins.is_empty() {
            "-".into()
        } else {
            plugins
        },
        options: vec![],
        editable: false,
    });

    fields
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    ToolList,
    Editor,
}

#[derive(Clone, PartialEq, Eq)]
enum AppMode {
    Browsing,
    Editing {
        field_idx: usize,
        buffer: String,
        cursor: usize,
    },
    Selecting {
        field_idx: usize,
        options: Vec<String>,
        filtered: Vec<usize>,
        selected: usize,
        filter: String,
    },
}

struct App {
    tools: Vec<ToolState>,
    tool_index: usize,
    field_index: usize,
    assistant_collapsed: bool,
    usage_update_rx: Option<Receiver<UsageSummaryUpdate>>,
    usage_update_tx: Option<Sender<UsageSummaryUpdate>>,
    usage_update_generation: Arc<AtomicUsize>,
    focus: Focus,
    mode: AppMode,
    status_msg: Option<String>,
    status_expire: Option<Instant>,
    last_error: Option<String>,
    error_expire: Option<Instant>,
    should_quit: bool,
}

fn status_value_for_display(field_key: &str, new_val: &str) -> String {
    if field_key.contains("API Key") {
        return if new_val.trim().is_empty() {
            "-".into()
        } else {
            mask_key(new_val.trim())
        };
    }
    new_val.to_string()
}

fn field_accepts_custom_select_value(tool: Tool, field_key: &str) -> bool {
    tool == Tool::KakuAssistant
        && matches!(
            field_key,
            "Simple Model" | "Deep Model" | "Model" | "Chat Model"
        )
}

impl App {
    fn new() -> Self {
        let tools = Self::load_tools();
        let first = tools.iter().position(|t| !t.fields.is_empty()).unwrap_or(0);
        let mut app = App {
            tools,
            tool_index: first,
            field_index: 0,
            assistant_collapsed: false,
            usage_update_rx: None,
            usage_update_tx: None,
            usage_update_generation: Arc::new(AtomicUsize::new(0)),
            focus: Focus::ToolList,
            mode: AppMode::Browsing,
            status_msg: None,
            status_expire: None,
            last_error: None,
            error_expire: None,
            should_quit: false,
        };
        app.restart_usage_loading();
        let _ = app.sync_transient_errors();
        app
    }

    fn load_tools() -> Vec<ToolState> {
        ALL_TOOLS
            .iter()
            .map(|t| ToolState::load_without_remote_usage(*t))
            .filter(|t| t.tool == Tool::KakuAssistant || t.installed)
            .collect()
    }

    fn tool_row_count(&self, tool_index: usize) -> usize {
        let Some(tool) = self.tools.get(tool_index) else {
            return 0;
        };
        if tool.tool == Tool::KakuAssistant {
            return 1 + if self.assistant_collapsed {
                0
            } else {
                tool.fields.len()
            };
        }
        tool.fields.len()
    }

    fn current_tool(&self) -> Option<&ToolState> {
        self.tools.get(self.tool_index)
    }

    fn current_tool_mut(&mut self) -> Option<&mut ToolState> {
        self.tools.get_mut(self.tool_index)
    }

    fn tool_is_collapsed(&self, tool_index: usize) -> bool {
        self.tools
            .get(tool_index)
            .is_some_and(|tool| tool.tool == Tool::KakuAssistant && self.assistant_collapsed)
    }

    fn selected_field_index(&self) -> Option<usize> {
        let tool = self.current_tool()?;
        if tool.fields.is_empty() {
            return None;
        }

        if tool.tool == Tool::KakuAssistant {
            self.field_index.checked_sub(1)
        } else {
            Some(self.field_index)
        }
    }

    fn display_index_for_field(&self, tool: Tool, field_index: usize) -> usize {
        if tool == Tool::KakuAssistant {
            field_index + 1
        } else {
            field_index
        }
    }

    fn toggle_kaku_assistant_collapsed(&mut self) {
        if !self
            .tools
            .iter()
            .any(|tool| tool.tool == Tool::KakuAssistant && !tool.fields.is_empty())
        {
            return;
        }

        self.assistant_collapsed = !self.assistant_collapsed;
        if self
            .current_tool()
            .is_some_and(|tool| tool.tool == Tool::KakuAssistant)
        {
            self.field_index = 0;
        }
        self.set_status(if self.assistant_collapsed {
            "Kaku Assistant hidden"
        } else {
            "Kaku Assistant expanded"
        });
    }

    fn total_rows(&self) -> usize {
        (0..self.tools.len())
            .map(|idx| self.tool_row_count(idx))
            .sum()
    }

    fn rendered_tool_row_count(&self) -> usize {
        self.tools
            .iter()
            .map(|tool| {
                1 + if tool.tool == Tool::KakuAssistant && self.assistant_collapsed {
                    1
                } else {
                    tool.fields.len() + 1
                }
            })
            .sum()
    }

    fn flatten_index(&self) -> usize {
        let mut idx = 0;
        for (ti, _) in self.tools.iter().enumerate() {
            let count = self.tool_row_count(ti);
            if ti == self.tool_index {
                return idx + self.field_index.min(count.saturating_sub(1));
            }
            idx += count;
        }
        idx
    }

    fn is_editing(&self) -> bool {
        matches!(self.mode, AppMode::Editing { .. })
    }

    fn is_selecting(&self) -> bool {
        matches!(self.mode, AppMode::Selecting { .. })
    }

    fn editing_view(&self) -> Option<(usize, &str, usize)> {
        match &self.mode {
            AppMode::Editing {
                field_idx,
                buffer,
                cursor,
            } => Some((*field_idx, buffer.as_str(), *cursor)),
            _ => None,
        }
    }

    fn editing_mut(&mut self) -> Option<(&mut String, &mut usize)> {
        match &mut self.mode {
            AppMode::Editing { buffer, cursor, .. } => Some((buffer, cursor)),
            _ => None,
        }
    }

    fn selecting_view(&self) -> Option<(usize, Vec<String>, usize, String)> {
        match &self.mode {
            AppMode::Selecting {
                field_idx,
                options,
                filtered,
                selected,
                filter,
            } => Some((
                *field_idx,
                filtered
                    .iter()
                    .filter_map(|idx| options.get(*idx).cloned())
                    .collect(),
                *selected,
                filter.clone(),
            )),
            _ => None,
        }
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.error_expire = Some(Instant::now() + UI_ERROR_TTL);
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status_msg = Some(message.into());
        self.status_expire = Some(Instant::now() + UI_STATUS_TTL);
    }

    fn open_antigravity_app(&mut self) {
        #[cfg(test)]
        {
            return;
        }

        #[cfg(not(test))]
        match std::process::Command::new("open")
            .args(["-a", "Antigravity"])
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => self.set_error(format!(
                "Failed to open Antigravity (exit status: {})",
                status
            )),
            Err(err) => self.set_error(format!("Failed to open Antigravity: {}", err)),
        }
    }

    fn sync_transient_errors(&mut self) -> bool {
        let mut changed = self.drain_usage_updates();

        while let Some(error) = pop_ui_error() {
            self.set_error(error);
            changed = true;
        }

        let now = Instant::now();
        if self.status_expire.is_some_and(|t| now >= t) {
            self.status_msg = None;
            self.status_expire = None;
            changed = true;
        }

        if self.error_expire.is_some_and(|t| now >= t) {
            self.last_error = None;
            self.error_expire = None;
            changed = true;
        }

        changed
    }

    fn restart_usage_loading(&mut self) {
        self.usage_update_rx = None;
        self.usage_update_tx = None;

        let (tx, rx) = mpsc::channel();
        let mut spawned = false;
        let generation = self.usage_update_generation.fetch_add(1, Ordering::Relaxed) + 1;

        for tool in self
            .tools
            .iter()
            .filter(|tool| tool.installed && supports_remote_usage(tool.tool))
        {
            spawned = true;
            let tx = tx.clone();
            let tool_kind = tool.tool;
            let active_generation = Arc::clone(&self.usage_update_generation);
            std::thread::spawn(move || {
                if active_generation.load(Ordering::Relaxed) != generation {
                    return;
                }
                let update = load_usage_update(tool_kind);
                if active_generation.load(Ordering::Relaxed) != generation {
                    return;
                }
                let _ = tx.send(update);
            });
        }

        if spawned {
            self.usage_update_rx = Some(rx);
            self.usage_update_tx = Some(tx);
        } else {
            self.usage_update_rx = None;
            self.usage_update_tx = None;
        }
    }

    fn schedule_usage_reload(&self, tool: Tool) {
        if !supports_remote_usage(tool) {
            return;
        }
        if !self
            .tools
            .iter()
            .any(|entry| entry.tool == tool && entry.installed)
        {
            return;
        }
        let Some(tx) = self.usage_update_tx.clone() else {
            return;
        };
        let generation = self.usage_update_generation.load(Ordering::Relaxed);
        let active_generation = Arc::clone(&self.usage_update_generation);
        std::thread::spawn(move || {
            if active_generation.load(Ordering::Relaxed) != generation {
                return;
            }
            let update = load_usage_update(tool);
            if active_generation.load(Ordering::Relaxed) != generation {
                return;
            }
            let _ = tx.send(update);
        });
    }

    fn drain_usage_updates(&mut self) -> bool {
        let Some(rx) = &self.usage_update_rx else {
            return false;
        };

        let mut updates = Vec::new();
        while let Ok(update) = rx.try_recv() {
            updates.push(update);
        }

        let changed = !updates.is_empty();
        for update in updates {
            if let Some(index) = self.tools.iter().position(|tool| tool.tool == update.tool) {
                if let Some(fields) = update.fields {
                    self.tools[index].fields = fields;
                }
                let installed = self.tools[index].installed;
                let summary = summarize_tool_fields(
                    update.tool,
                    installed,
                    &self.tools[index].fields,
                    update.summary.as_deref(),
                );
                self.tools[index].summary = summary;
            }
        }
        changed
    }

    fn set_from_flat(&mut self, flat: usize) {
        let previous_tool = self.current_tool().map(|tool| tool.tool);
        let mut remaining = flat;
        for (ti, _) in self.tools.iter().enumerate() {
            let count = self.tool_row_count(ti);
            if count == 0 {
                continue;
            }
            if remaining < count {
                self.tool_index = ti;
                self.field_index = remaining;
                if self.current_tool().map(|tool| tool.tool) != previous_tool {
                    if let Some(tool) = self.current_tool().map(|tool| tool.tool) {
                        self.schedule_usage_reload(tool);
                    }
                }
                return;
            }
            remaining -= count;
        }
    }

    fn move_up(&mut self) {
        let flat = self.flatten_index();
        if flat > 0 {
            self.set_from_flat(flat - 1);
        }
    }

    fn move_down(&mut self) {
        let flat = self.flatten_index();
        if flat + 1 < self.total_rows() {
            self.set_from_flat(flat + 1);
        }
    }

    fn move_select_up(&mut self) {
        if let AppMode::Selecting { selected, .. } = &mut self.mode {
            if *selected > 0 {
                *selected -= 1;
            }
        }
    }

    fn move_select_down(&mut self) {
        if let AppMode::Selecting {
            selected, filtered, ..
        } = &mut self.mode
        {
            if *selected + 1 < filtered.len() {
                *selected += 1;
            }
        }
    }

    fn rebuild_select_filter(&mut self) {
        if let AppMode::Selecting {
            options,
            filtered,
            selected,
            filter,
            ..
        } = &mut self.mode
        {
            let query = filter.trim().to_lowercase();
            filtered.clear();
            if query.is_empty() {
                filtered.extend(0..options.len());
            } else {
                filtered.extend(
                    options
                        .iter()
                        .enumerate()
                        .filter(|(_, option)| option.to_lowercase().contains(&query))
                        .map(|(idx, _)| idx),
                );
            }
            if filtered.is_empty() {
                *selected = 0;
            } else {
                *selected = (*selected).min(filtered.len() - 1);
            }
        }
    }

    fn push_select_filter(&mut self, c: char) {
        if let AppMode::Selecting { filter, .. } = &mut self.mode {
            filter.push(c);
        }
        self.rebuild_select_filter();
    }

    fn pop_select_filter(&mut self) {
        if let AppMode::Selecting { filter, .. } = &mut self.mode {
            filter.pop();
        }
        self.rebuild_select_filter();
    }

    fn clear_select_filter(&mut self) {
        if let AppMode::Selecting { filter, .. } = &mut self.mode {
            filter.clear();
        }
        self.rebuild_select_filter();
    }

    fn start_edit(&mut self) {
        if self
            .current_tool()
            .is_some_and(|tool| tool.tool == Tool::KakuAssistant && self.field_index == 0)
        {
            self.toggle_kaku_assistant_collapsed();
            return;
        }

        let Some(tool) = self.current_tool() else {
            return;
        };
        if !tool.installed || tool.fields.is_empty() {
            return;
        }
        let Some(selected_field_idx) = self.selected_field_index() else {
            return;
        };
        if selected_field_idx >= tool.fields.len() {
            return;
        }
        let field = &tool.fields[selected_field_idx];

        if tool.tool == Tool::Antigravity && field.key == "Model" {
            self.set_status("Change the model in Antigravity settings.");
            self.open_antigravity_app();
            return;
        }

        // Show OAuth re-authentication command for non-editable auth fields
        if !field.editable {
            if field.key == "Auth" || (field.value.starts_with('✓') && !field.key.contains(" ▸ "))
            {
                if let Some(auth_cmd) = reauth_command(tool.tool) {
                    self.open_in_terminal(auth_cmd);
                } else if tool.tool == Tool::OpenClaw {
                    self.set_status("OpenClaw uses API keys, check config file");
                }
            }
            return;
        }

        if !field.options.is_empty() {
            let options = field.options.clone();
            let filtered = (0..options.len()).collect();
            self.mode = AppMode::Selecting {
                field_idx: selected_field_idx,
                options,
                filtered,
                selected: field
                    .options
                    .iter()
                    .position(|o| *o == field.value)
                    .unwrap_or(0),
                filter: String::new(),
            };
            self.focus = Focus::Editor;
            return;
        }

        let edit_buf = if field.value == "-" {
            // Empty placeholder
            String::new()
        } else if tool.tool == Tool::KakuAssistant && field.key == "API Key" {
            get_kaku_assistant_api_key().unwrap_or_else(String::new)
        } else {
            field.value.clone()
        };
        let edit_cursor = edit_buf.len(); // Start cursor at end (always a valid byte boundary)
        self.mode = AppMode::Editing {
            field_idx: selected_field_idx,
            buffer: edit_buf,
            cursor: edit_cursor,
        };
        self.focus = Focus::Editor;
    }

    fn confirm_select(&mut self) {
        let AppMode::Selecting {
            field_idx,
            options,
            filtered,
            selected,
            filter,
        } = std::mem::replace(&mut self.mode, AppMode::Browsing)
        else {
            return;
        };
        self.focus = Focus::ToolList;

        let Some((tool_kind, field_key, old_val)) = self.current_tool().and_then(|tool| {
            tool.fields
                .get(field_idx)
                .map(|field| (tool.tool, field.key.clone(), field.value.clone()))
        }) else {
            return;
        };
        self.field_index = self.display_index_for_field(tool_kind, field_idx);
        let new_val = match filtered.get(selected).and_then(|idx| options.get(*idx)) {
            Some(option) => option.clone(),
            None if field_accepts_custom_select_value(tool_kind, &field_key)
                && !filter.trim().is_empty() =>
            {
                filter.trim().to_string()
            }
            None => return,
        };

        if new_val == old_val {
            return;
        }

        if let Some(tool) = self.current_tool_mut() {
            tool.fields[field_idx].value = new_val.clone();
        }
        let status_val = status_value_for_display(&field_key, &new_val);
        match save_field(tool_kind, &field_key, &new_val) {
            Ok(()) => self.set_status(format!("Saved {} as {}", field_key, status_val)),
            Err(e) => {
                self.set_status(format!("Save failed: {}", e));
                self.set_error(format!("Save failed: {}", e));
            }
        }
        // Selecting Web Search adds/removes the Search Key row, so prefer
        // landing on it when present so the user can immediately fill in the
        // key. For all other fields focus stays on the row just edited.
        let focus_key = if field_key == "Web Search" && new_val != "none" {
            "Search Key"
        } else {
            field_key.as_str()
        };
        self.reload_current_tool_focusing(Some(focus_key));
    }

    fn cancel_select(&mut self) {
        self.mode = AppMode::Browsing;
        self.focus = Focus::ToolList;
    }

    fn confirm_edit(&mut self) {
        let AppMode::Editing {
            field_idx,
            buffer,
            cursor: _,
        } = std::mem::replace(&mut self.mode, AppMode::Browsing)
        else {
            return;
        };
        self.focus = Focus::ToolList;

        let Some((tool_kind, field_key, old_val)) = self.current_tool().and_then(|tool| {
            tool.fields
                .get(field_idx)
                .map(|field| (tool.tool, field.key.clone(), field.value.clone()))
        }) else {
            return;
        };
        self.field_index = if tool_kind == Tool::KakuAssistant {
            field_idx + 1
        } else {
            field_idx
        };
        let new_val = buffer.trim().to_string();

        // Empty input on API Key fields means cancel, not clear
        if new_val.is_empty() && field_key.contains("API Key") {
            return;
        }

        if new_val == old_val || (new_val.is_empty() && old_val == "-") {
            return;
        }

        if let Some(tool) = self.current_tool_mut() {
            tool.fields[field_idx].value = new_val.clone();
        }

        let status_val = status_value_for_display(&field_key, &new_val);
        match save_field(tool_kind, &field_key, &new_val) {
            Ok(()) => self.set_status(format!("Saved {} as {}", field_key, status_val)),
            Err(e) => {
                self.set_status(format!("Save failed: {}", e));
                self.set_error(format!("Save failed: {}", e));
            }
        }
        // Keep the cursor on the row just edited after the disk reload, so
        // the user does not lose their place.
        self.reload_current_tool_focusing(Some(field_key.as_str()));
    }

    /// Reload the active tool's fields from disk. When `focus_key` is set,
    /// move the selection cursor to the field with that key in the refreshed
    /// list. This is what makes "select Web Search → brave" leave the cursor
    /// on the freshly-revealed Search Key row instead of jumping back to the
    /// tool header at the top.
    ///
    /// `focus_key` falls back gracefully: if the named key no longer exists
    /// after reload (e.g. user picked "none" so Search Key disappeared), we
    /// keep the cursor on the *editor* row that triggered the reload by
    /// using its old position as a hint and clamping into bounds.
    fn reload_current_tool_focusing(&mut self, focus_key: Option<&str>) {
        let Some(tool_type) = self.current_tool().map(|tool| tool.tool) else {
            return;
        };
        let prior_field_idx = self.selected_field_index();
        let refreshed = ToolState::load_without_remote_usage(tool_type);
        if let Some(tool) = self.current_tool_mut() {
            *tool = refreshed;
        }
        if tool_type == Tool::KakuAssistant {
            self.assistant_collapsed = false;
        }
        let Some(tool) = self.current_tool() else {
            return;
        };
        let new_field_idx = focus_key
            .and_then(|key| tool.fields.iter().position(|f| f.key == key))
            .or_else(|| prior_field_idx.and_then(|p| (p < tool.fields.len()).then_some(p)));
        self.field_index = match new_field_idx {
            Some(idx) => self.display_index_for_field(tool_type, idx),
            None if tool_type == Tool::KakuAssistant => 0,
            None => 0,
        };
        self.schedule_usage_reload(tool_type);
    }

    fn cancel_edit(&mut self) {
        self.mode = AppMode::Browsing;
        self.focus = Focus::ToolList;
    }

    fn config_path_to_open(&self) -> Option<PathBuf> {
        let tool = self.current_tool()?;
        let path = tool.tool.config_path();
        path.exists().then_some(path)
    }

    fn refresh_models(&mut self) {
        let codex_usage_cache = codex_usage_cache_path();
        if let Err(e) = std::fs::remove_file(&codex_usage_cache) {
            log::trace!("Could not remove codex usage cache: {}", e);
        }
        let claude_usage_cache = claude_usage_cache_path();
        if let Err(e) = std::fs::remove_file(&claude_usage_cache) {
            log::trace!("Could not remove claude usage cache: {}", e);
        }
        let copilot_usage_cache = copilot_usage_cache_path();
        if let Err(e) = std::fs::remove_file(&copilot_usage_cache) {
            log::trace!("Could not remove copilot usage cache: {}", e);
        }
        let kimi_usage_cache = kimi_usage_cache_path();
        if let Err(e) = std::fs::remove_file(&kimi_usage_cache) {
            log::trace!("Could not remove kimi usage cache: {}", e);
        }
        let antigravity_usage_cache = antigravity_usage_cache_path();
        if let Err(e) = std::fs::remove_file(&antigravity_usage_cache) {
            log::trace!("Could not remove antigravity usage cache: {}", e);
        }

        let models_refreshed = fetch_models_dev_json().is_some();
        let assistant_cache = assistant_models_cache_path();
        if let Err(e) = std::fs::remove_file(&assistant_cache) {
            log::trace!("Could not remove assistant models cache: {}", e);
        }
        self.tools = Self::load_tools();
        self.tool_index = self
            .tools
            .iter()
            .position(|tool| !tool.fields.is_empty())
            .unwrap_or(0);
        self.field_index = 0;
        self.restart_usage_loading();
        if models_refreshed {
            self.set_status("AI settings refreshed");
        } else {
            self.set_status("Usage refreshed");
            self.set_error("Models refresh failed. Kept local model cache.");
        }
        let _ = self.sync_transient_errors();
    }

    /// Open a shell command in a new Kaku tab (preferred) or fall back to Terminal.app.
    fn open_in_terminal(&mut self, cmd: &str) {
        // Prefer a new tab in the current Kaku window.
        // kaku cli spawn reads WEZTERM_PANE from the environment to target the right window.
        // Append `exec $SHELL` so the pane stays alive after the command finishes.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let shell_cmd = format!("{}; exec \"{}\"", cmd, shell);
        let kaku_status = std::process::Command::new("kaku")
            .args(["cli", "spawn", "--", &shell, "-c", &shell_cmd])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if kaku_status.as_ref().is_ok_and(|status| status.success()) {
            self.set_status("Opening in new Kaku tab...");
            return;
        }

        // Fallback: open in macOS Terminal.app via osascript.
        let script = format!("tell application \"Terminal\" to do script \"{}\"", cmd);
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
        {
            Ok(_) => self.set_status("Opening in new terminal window..."),
            Err(_) => self.set_status(format!("Failed to open terminal. Run '{}' manually", cmd)),
        }
    }
}

/// The command opened in a terminal to sign in again from an auth field. Each
/// must be a real subcommand: an unknown word such as `codex auth` is taken as
/// a prompt and starts a session instead of a login.
fn reauth_command(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::KakuAssistant | Tool::OpenClaw => None,
        // Gemini CLI has no auth subcommand; it signs in when launched.
        Tool::Gemini => Some("gemini"),
        Tool::Codex => Some("codex login"),
        Tool::Kimi => Some("kimi login"),
        Tool::Antigravity => Some("open -a Antigravity"),
        Tool::Copilot => Some("gh auth login"),
        Tool::FactoryDroid => Some("droid"),
        Tool::ClaudeCode => Some("claude auth login"),
    }
}

fn save_field(tool: Tool, field_key: &str, new_val: &str) -> anyhow::Result<()> {
    if tool == Tool::KakuAssistant {
        return save_kaku_assistant_field(field_key, new_val);
    }

    // Codex uses TOML; delegate immediately before any JSON parsing attempt.
    if tool == Tool::Codex {
        return save_codex_field(field_key, new_val);
    }
    if tool == Tool::Kimi {
        return save_kimi_field(field_key, new_val);
    }

    let path = tool.config_path();
    if !path.exists() {
        anyhow::bail!("config file not found: {}", path.display());
    }

    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut parsed: serde_json::Value =
        parse_json_or_jsonc(&raw).with_context(|| format!("parse {}", path.display()))?;

    match tool {
        Tool::KakuAssistant => unreachable!("Kaku Assistant is handled before JSON parsing"),
        Tool::Antigravity => return Ok(()),
        Tool::Gemini => {
            if field_key == "Model" {
                if let Some(obj) = parsed.as_object_mut() {
                    save_gemini_model(obj, new_val);
                }
            } else {
                return Ok(());
            }
        }
        Tool::Copilot => {
            if field_key == "Model" {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "-" || new_val.is_empty() {
                        obj.remove("model");
                    } else {
                        obj.insert(
                            "model".to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::FactoryDroid => {
            let obj = parsed.as_object_mut().context("root is not object")?;
            save_factory_droid_field(obj, field_key, new_val)?;
        }
        Tool::ClaudeCode => {
            let env_key = match field_key {
                "Base URL" => Some("ANTHROPIC_BASE_URL"),
                "API Key" => Some("ANTHROPIC_AUTH_TOKEN"),
                _ => None,
            };
            let top_key = match field_key {
                "Model" => Some("model"),
                _ => None,
            };

            if let Some(ek) = env_key {
                let obj = parsed.as_object_mut().context("root is not object")?;
                let env = obj.entry("env").or_insert_with(|| serde_json::json!({}));
                if let Some(env_obj) = env.as_object_mut() {
                    if new_val == "-" || new_val.is_empty() {
                        env_obj.remove(ek);
                    } else {
                        env_obj.insert(
                            ek.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else if let Some(tk) = top_key {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "-" || new_val.is_empty() {
                        obj.remove(tk);
                    } else {
                        obj.insert(
                            tk.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::OpenClaw => {
            // Parse "provider_name ▸ Base URL" or "provider_name ▸ API Key"
            if let Some(sep_pos) = field_key.find(" ▸ ") {
                let provider_name = &field_key[..sep_pos];
                let sub_field = &field_key[sep_pos + " ▸ ".len()..];

                if sub_field == "Models" {
                    // Rename model key in agents.defaults.models
                    // old_display: "claude-opus-4-5-20251101" → full key: "provider/claude-opus-4-5-20251101"
                    // new_val:     "claude-opus-4-6"          → full key: "provider/claude-opus-4-6"
                    let defaults = parsed
                        .as_object_mut()
                        .context("root not object")?
                        .entry("agents")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("agents not object")?
                        .entry("defaults")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("defaults not object")?;

                    // Collect old→new key mappings first
                    let prefix = format!("{}/", provider_name);
                    let mut renames: Vec<(String, String)> = Vec::new();

                    if let Some(models_obj) =
                        defaults.get_mut("models").and_then(|m| m.as_object_mut())
                    {
                        let old_keys: Vec<String> = models_obj
                            .keys()
                            .filter(|k| k.starts_with(&prefix))
                            .cloned()
                            .collect();

                        for old_key in old_keys {
                            if let Some(val) = models_obj.remove(&old_key) {
                                let new_full = format!("{}/{}", provider_name, new_val);
                                models_obj.insert(new_full.clone(), val);
                                renames.push((old_key, new_full));
                            }
                        }
                    }

                    // Sync model.primary if it pointed to an old key
                    if let Some(model_obj) =
                        defaults.get_mut("model").and_then(|m| m.as_object_mut())
                    {
                        for (old_key, new_full) in &renames {
                            if model_obj.get("primary").and_then(|v| v.as_str()) == Some(old_key) {
                                model_obj.insert(
                                    "primary".to_string(),
                                    serde_json::Value::String(new_full.clone()),
                                );
                            }
                        }
                    }
                } else {
                    let json_field = match sub_field {
                        "Base URL" => "baseUrl",
                        "API Key" => "apiKey",
                        _ => return Ok(()),
                    };

                    let providers = parsed
                        .as_object_mut()
                        .context("root not object")?
                        .entry("models")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("models not object")?
                        .entry("providers")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("providers not object")?;

                    let prov = providers
                        .entry(provider_name)
                        .or_insert_with(|| serde_json::json!({}));

                    if let Some(obj) = prov.as_object_mut() {
                        if new_val == "-" || new_val.is_empty() {
                            obj.remove(json_field);
                        } else {
                            obj.insert(
                                json_field.to_string(),
                                serde_json::Value::String(new_val.to_string()),
                            );
                        }
                    }
                }
            } else if field_key == "Primary Model" {
                let model = parsed
                    .as_object_mut()
                    .context("root not object")?
                    .entry("agents")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("agents not object")?
                    .entry("defaults")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("defaults not object")?
                    .entry("model")
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(obj) = model.as_object_mut() {
                    if new_val == "-" || new_val.is_empty() {
                        obj.remove("primary");
                    } else {
                        obj.insert(
                            "primary".to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::Codex | Tool::Kimi => {
            unreachable!("TOML-backed tools are handled before JSON parsing")
        }
    }

    let output = serde_json::to_string_pretty(&parsed).context("serialize config")?;
    if is_jsonc_path(&path) {
        log::info!(
            "Note: {} comments will be removed when Kaku rewrites this file.",
            path.display()
        );
    }
    write_atomic(&path, output.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn save_kimi_field(field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let path = Tool::Kimi.config_path();
    save_kimi_field_at(&path, field_key, new_val)
}

fn save_kimi_field_at(path: &Path, field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let toml_key = match field_key {
        "Model" => "default_model",
        _ => return Ok(()),
    };

    let raw = if path.exists() {
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = raw.lines().map(|line| line.to_string()).collect();
    let target = format!("{toml_key} = ");
    let new_line = format!("{toml_key} = \"{new_val}\"");

    let mut found = false;
    let mut in_top_level = true;
    for line in &mut lines {
        let trimmed = line.trim_start();
        // Entering a table section: [section] or [[array-of-tables]]
        if trimmed.starts_with('[') {
            in_top_level = false;
        }
        if in_top_level && trimmed.starts_with(&target) {
            if new_val == "-" || new_val.is_empty() {
                *line = String::new();
            } else {
                *line = new_line.clone();
            }
            found = true;
            break;
        }
    }

    if !found && !new_val.is_empty() && new_val != "-" {
        let insert_pos = lines
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .unwrap_or(lines.len());
        lines.insert(insert_pos, new_line);
    }

    let output: Vec<&str> = lines.iter().map(|line| line.as_str()).collect();
    let result = output.join("\n");
    write_atomic(path, result.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn run(
    config_file: Option<std::ffi::OsString>,
    config_override: Vec<(String, String)>,
    skip_config: bool,
) -> anyhow::Result<()> {
    struct TerminalGuard {
        raw_mode: bool,
        bracketed_paste: bool,
        alternate_screen: bool,
    }

    impl TerminalGuard {
        fn new() -> Self {
            Self {
                raw_mode: false,
                bracketed_paste: false,
                alternate_screen: false,
            }
        }
    }

    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            if self.raw_mode {
                let _ = disable_raw_mode();
            }

            let mut stdout = io::stdout();
            if self.bracketed_paste {
                let _ = stdout.execute(DisableBracketedPaste);
            }
            if self.alternate_screen {
                let _ = stdout.execute(LeaveAlternateScreen);
            }
        }
    }

    let mut guard = TerminalGuard::new();
    // Load config synchronously *before* entering the alternate screen so
    // the user does not stare at a blank black buffer during cold load.
    // On warm cache this is <50 ms; on cold cache it can be ~1 s but the
    // user still sees their existing terminal until the TUI is ready to
    // paint, which is less jarring than a flash of "Loading..." that gets
    // overwritten 100 ms later.
    if let Err(e) = config::common_init(config_file.as_ref(), &config_override, skip_config) {
        log::error!("config init failed: {:#}", e);
    }
    let mut app = App::new();

    enable_raw_mode().context("enable raw mode")?;
    guard.raw_mode = true;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnableBracketedPaste).context("enable bracketed paste")?;
    guard.bracketed_paste = true;
    stdout
        .execute(EnterAlternateScreen)
        .context("enter alternate screen")?;
    guard.alternate_screen = true;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let result = run_loop(&mut terminal, &mut app);

    terminal.show_cursor().context("show cursor")?;

    result
}

fn is_confirm_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') | KeyCode::Char(' ')
    )
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut needs_redraw = true;
    loop {
        if app.sync_transient_errors() {
            needs_redraw = true;
        }

        if needs_redraw {
            terminal.draw(|frame| ui::ui(frame, app))?;
            needs_redraw = false;
        }

        if !event::poll(EVENT_POLL_INTERVAL).context("poll event")? {
            continue;
        }

        needs_redraw = true;
        match event::read().context("read event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.status_msg = None;
                app.status_expire = None;
                if app.is_selecting() {
                    match key.code {
                        code if is_confirm_key(code) => app.confirm_select(),
                        KeyCode::Esc => app.cancel_select(),
                        KeyCode::Up => app.move_select_up(),
                        KeyCode::Down => app.move_select_down(),
                        KeyCode::Backspace => {
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                || key.modifiers.contains(KeyModifiers::SUPER)
                            {
                                app.clear_select_filter();
                            } else {
                                app.pop_select_filter();
                            }
                        }
                        KeyCode::Char(c)
                            if !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::SUPER) =>
                        {
                            app.push_select_filter(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.is_editing() {
                    match key.code {
                        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                            app.confirm_edit()
                        }
                        KeyCode::Esc => app.cancel_edit(),
                        KeyCode::Left => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor > 0 {
                                    *edit_cursor = prev_char_boundary(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Right => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor < edit_buf.len() {
                                    *edit_cursor = next_char_boundary(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Home => {
                            if let Some((_, edit_cursor)) = app.editing_mut() {
                                *edit_cursor = 0;
                            }
                        }
                        KeyCode::End => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                *edit_cursor = edit_buf.len();
                            }
                        }
                        KeyCode::Backspace => {
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                || key.modifiers.contains(KeyModifiers::SUPER)
                            {
                                // Cmd+Backspace (macOS) or Ctrl+Backspace - clear all
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_buf.clear();
                                    *edit_cursor = 0;
                                }
                            } else if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor > 0 {
                                    edit_backspace(edit_buf, edit_cursor);
                                }
                            }
                        }
                        KeyCode::Delete => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor < edit_buf.len() {
                                    edit_delete(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            // Handle Ctrl+U (clear line) - macOS Cmd+Backspace may also send this
                            if (key.modifiers.contains(KeyModifiers::CONTROL)
                                || key.modifiers.contains(KeyModifiers::SUPER))
                                && c == 'u'
                            {
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_buf.clear();
                                    *edit_cursor = 0;
                                }
                            }
                            // Ignore other control characters
                            else if !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::SUPER)
                            {
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_insert_char(edit_buf, edit_cursor, c);
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true
                    }
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    code if is_confirm_key(code) => {
                        app.start_edit();
                    }
                    KeyCode::Char('o') => {
                        if let Some(path) = app.config_path_to_open() {
                            match with_terminal_suspended(terminal, || open_path_in_editor(&path)) {
                                Ok(()) => app.set_status("Opened config file"),
                                Err(err) => {
                                    log::debug!("Failed to open config file: {}", err);
                                    app.set_status(format!("Failed to open: {}", err));
                                }
                            }
                        }
                    }
                    KeyCode::Char('r') => app.refresh_models(),
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => app.move_up(),
                MouseEventKind::ScrollDown => app.move_down(),
                _ => {}
            },
            Event::Paste(text) => {
                if !app.is_editing() || text.is_empty() {
                    continue;
                }

                let ends_with_newline = text.ends_with('\n') || text.ends_with('\r');

                // Clipboard paste may include a trailing newline from terminal copy.
                // Strip line breaks so paste doesn't break single-line inputs.
                let cleaned: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();
                if !cleaned.is_empty() {
                    if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                        for c in cleaned.chars() {
                            edit_insert_char(edit_buf, edit_cursor, c);
                        }
                    }
                }

                // If the pasted text (often from voice typing tools) ends with a newline, auto-submit
                if ends_with_newline {
                    app.confirm_edit();
                }
            }
            _ => {}
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn with_terminal_suspended<F>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    func: F,
) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    disable_raw_mode().context("disable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, DisableBracketedPaste).context("disable bracketed paste")?;
    stdout
        .execute(LeaveAlternateScreen)
        .context("leave alternate screen")?;

    let action_result = func();

    let restore_result = (|| -> anyhow::Result<()> {
        enable_raw_mode().context("enable raw mode")?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnableBracketedPaste).context("enable bracketed paste")?;
        stdout
            .execute(EnterAlternateScreen)
            .context("enter alternate screen")?;
        terminal.clear().context("clear terminal")?;
        Ok(())
    })();

    action_result.and(restore_result)
}

fn prev_char_boundary(buf: &str, cursor: usize) -> usize {
    let cursor = cursor.min(buf.len());
    if cursor == 0 {
        return 0;
    }
    buf[..cursor]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_char_boundary(buf: &str, cursor: usize) -> usize {
    let cursor = cursor.min(buf.len());
    if cursor >= buf.len() {
        return buf.len();
    }
    cursor + buf[cursor..].chars().next().map_or(0, |c| c.len_utf8())
}

fn edit_backspace(buf: &mut String, cursor: &mut usize) {
    if *cursor == 0 || buf.is_empty() {
        return;
    }
    let prev = prev_char_boundary(buf, *cursor);
    buf.remove(prev);
    *cursor = prev;
}

fn edit_delete(buf: &mut String, cursor: usize) {
    if cursor >= buf.len() || buf.is_empty() {
        return;
    }
    buf.remove(cursor);
}

fn edit_insert_char(buf: &mut String, cursor: &mut usize, c: char) {
    let at = (*cursor).min(buf.len());
    buf.insert(at, c);
    *cursor = at + c.len_utf8();
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use tempfile::tempdir;

    #[test]
    fn reauth_commands_use_real_login_subcommands() {
        // `codex auth login` and `gemini auth login` are not subcommands; both
        // CLIs read the words as a prompt and start a session instead.
        assert_eq!(reauth_command(Tool::Codex), Some("codex login"));
        assert_eq!(reauth_command(Tool::Gemini), Some("gemini"));
        assert_eq!(reauth_command(Tool::ClaudeCode), Some("claude auth login"));
        assert_eq!(reauth_command(Tool::KakuAssistant), None);
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return out;
            }
        }
    }

    fn encode_len_delimited(field_number: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = encode_varint((field_number << 3) | 2);
        out.extend(encode_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn encode_int32_state_value(value: i32) -> String {
        let mut out = encode_varint(2 << 3);
        out.extend(encode_varint(value as u64));
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    fn encode_antigravity_unified_state(entries: &[(&str, &str)]) -> String {
        let mut outer = Vec::new();
        for (key, value) in entries {
            let nested_value = encode_len_delimited(1, value.as_bytes());
            let mut entry = encode_len_delimited(1, key.as_bytes());
            entry.extend(encode_len_delimited(2, &nested_value));
            outer.extend(encode_len_delimited(1, &entry));
        }
        base64::engine::general_purpose::STANDARD.encode(outer)
    }

    #[test]
    fn curl_config_header_escapes_quoted_value() {
        assert_eq!(
            curl_config_header("Authorization", "Bearer a\\b\"c\n\r\tend"),
            "header = \"Authorization: Bearer a\\\\b\\\"c\\n\\r\\tend\"\n"
        );
    }

    #[test]
    fn antigravity_lsp_curl_args_do_not_include_csrf_token() {
        let token = "secret-csrf-token";
        let args = antigravity_lsp_curl_args("https://127.0.0.1:56503/status", "{}");

        assert!(!args.iter().any(|arg| arg.contains(token)));
        assert!(!args.iter().any(|arg| arg.contains("x-codeium-csrf-token")));

        let curl_config = curl_config_header("x-codeium-csrf-token", token);
        assert!(curl_config.contains(token));
        assert!(curl_config.contains("x-codeium-csrf-token"));
    }

    #[test]
    fn unicode_edit_helpers_keep_cursor_on_char_boundaries() {
        let mut buf = "你a好".to_string();
        let mut cursor = buf.len();

        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你a".len());
        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, 0);

        cursor = next_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        cursor = next_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你a".len());

        edit_insert_char(&mut buf, &mut cursor, '界');
        assert_eq!(buf, "你a界好");
        assert_eq!(cursor, "你a界".len());

        edit_backspace(&mut buf, &mut cursor);
        assert_eq!(buf, "你a好");
        assert_eq!(cursor, "你a".len());

        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        edit_delete(&mut buf, cursor);
        assert_eq!(buf, "你好");
        assert_eq!(cursor, "你".len());
    }

    #[test]
    fn factory_droid_extract_prefers_session_defaults() {
        let parsed = serde_json::json!({
            "model": "legacy-model",
            "reasoningEffort": "low",
            "autonomyMode": "spec",
            "sessionDefaultSettings": {
                "model": "fd-model",
                "reasoningEffort": "medium",
                "autonomyLevel": "auto-low"
            }
        });

        let fields = extract_factory_droid_fields(&parsed);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].key, "Model");
        assert_eq!(fields[0].value, "fd-model");
        assert!(fields[0].options.is_empty());

        assert_eq!(fields[1].key, "Reasoning Effort");
        assert_eq!(fields[1].value, "medium");
        assert_eq!(
            fields[1].options,
            string_options(&FACTORY_DROID_REASONING_OPTIONS)
        );

        assert_eq!(fields[2].key, "Autonomy Level");
        assert_eq!(fields[2].value, "auto-low");
        assert_eq!(
            fields[2].options,
            string_options(&FACTORY_DROID_AUTONOMY_OPTIONS)
        );
    }

    #[test]
    fn factory_droid_extract_falls_back_to_top_level_and_defaults() {
        let parsed = serde_json::json!({
            "model": "legacy-model",
            "reasoningEffort": "high",
            "autonomyLevel": "spec"
        });

        let fields = extract_factory_droid_fields(&parsed);
        assert_eq!(fields[0].value, "legacy-model");
        assert_eq!(fields[1].value, "high");
        assert_eq!(fields[2].value, "spec");

        let empty_fields = extract_factory_droid_fields(&serde_json::json!({}));
        assert_eq!(empty_fields[0].value, FACTORY_DROID_DEFAULT_MODEL);
        assert_eq!(empty_fields[1].value, FACTORY_DROID_DEFAULT_REASONING);
        assert_eq!(empty_fields[2].value, FACTORY_DROID_DEFAULT_AUTONOMY);
    }

    #[test]
    fn factory_droid_save_writes_session_defaults_and_normalizes_autonomy_key() {
        let mut obj = serde_json::json!({
            "autonomyLevel": "spec"
        })
        .as_object()
        .cloned()
        .expect("object");

        save_factory_droid_field(&mut obj, "Autonomy Level", "auto-high").expect("save autonomy");
        save_factory_droid_field(&mut obj, "Model", "gpt-5.1-codex").expect("save model");
        save_factory_droid_field(&mut obj, "Reasoning Effort", "").expect("clear reasoning effort");

        let session_defaults = obj
            .get("sessionDefaultSettings")
            .and_then(|v| v.as_object())
            .expect("sessionDefaultSettings object");

        assert_eq!(
            session_defaults
                .get("autonomyMode")
                .and_then(|v| v.as_str()),
            Some("auto-high")
        );
        assert_eq!(session_defaults.get("autonomyLevel"), None);
        assert_eq!(
            session_defaults.get("model").and_then(|v| v.as_str()),
            Some("gpt-5.1-codex")
        );
        assert_eq!(session_defaults.get("reasoningEffort"), None);
    }

    #[test]
    fn kaku_assistant_fields_include_enabled_toggle() {
        let fields = extract_kaku_assistant_fields("enabled = true\n");
        assert!(fields.iter().any(|field| field.key == "Enabled"));
        let fields = extract_kaku_assistant_fields("enabled = false\n");
        assert!(fields.iter().any(|field| field.key == "Enabled"));
    }

    #[test]
    fn kaku_assistant_model_fields_accept_custom_select_values() {
        assert!(field_accepts_custom_select_value(
            Tool::KakuAssistant,
            "Simple Model"
        ));
        assert!(field_accepts_custom_select_value(
            Tool::KakuAssistant,
            "Deep Model"
        ));
        assert!(!field_accepts_custom_select_value(
            Tool::KakuAssistant,
            "Web Search"
        ));
        assert!(!field_accepts_custom_select_value(Tool::Codex, "Model"));
    }

    #[test]
    fn summarize_kaku_assistant_requires_api_key() {
        let fields = extract_kaku_assistant_fields("enabled = false\n");
        assert_eq!(
            summarize_tool_fields(Tool::KakuAssistant, true, &fields, None),
            Some(format!(
                "Setup required · {}",
                assistant_config::DEFAULT_MODEL
            ))
        );
    }

    #[test]
    fn summarize_kaku_assistant_prefers_status_and_model() {
        let fields = extract_kaku_assistant_fields(
            "enabled = true\napi_key = \"sk-test\"\nmodel = \"gpt-5-mini\"\n",
        );
        assert_eq!(
            summarize_tool_fields(Tool::KakuAssistant, true, &fields, None),
            Some("Ready · gpt-5-mini".into())
        );
    }

    #[test]
    fn summarize_non_usage_tool_uses_auth_and_model() {
        let fields = vec![
            FieldEntry {
                key: "Model".into(),
                value: "gpt-5".into(),
                options: vec![],
                editable: true,
            },
            FieldEntry {
                key: "Auth".into(),
                value: "✓ user@example.com".into(),
                options: vec![],
                editable: false,
            },
        ];

        assert_eq!(
            summarize_tool_fields(Tool::FactoryDroid, true, &fields, None),
            Some("user@example.com · gpt-5".into())
        );
    }

    #[test]
    fn summarize_codex_prefers_usage_only() {
        assert_eq!(
            summarize_tool_fields(
                Tool::Codex,
                true,
                &[FieldEntry {
                    key: "Auth".into(),
                    value: "✓ user@example.com".into(),
                    options: vec![],
                    editable: false,
                }],
                Some("5h remain 75% · reset 4h0m  |  7d remain 93% · reset 6d0h"),
            ),
            Some("5h remain 75% · reset 4h0m  |  7d remain 93% · reset 6d0h".into())
        );
    }

    #[test]
    fn summarize_claude_prefers_usage_only() {
        assert_eq!(
            summarize_tool_fields(
                Tool::ClaudeCode,
                true,
                &[],
                Some("5h remain 94% · reset 2h0m")
            ),
            Some("5h remain 94% · reset 2h0m".into())
        );
    }

    #[test]
    fn summarize_kimi_prefers_usage_only() {
        assert_eq!(
            summarize_tool_fields(
                Tool::Kimi,
                true,
                &[],
                Some("5h remain 94% · reset 14m  |  7d remain 67% · reset 4d11h")
            ),
            Some("5h remain 94% · reset 14m  |  7d remain 67% · reset 4d11h".into())
        );
    }

    #[test]
    fn summarize_antigravity_uses_usage_summary() {
        let fields = vec![
            FieldEntry {
                key: "Plan".into(),
                value: "Google AI Pro".into(),
                options: vec![],
                editable: false,
            },
            FieldEntry {
                key: "Auth".into(),
                value: "✓ user@example.com".into(),
                options: vec![],
                editable: false,
            },
        ];

        assert_eq!(
            summarize_tool_fields(
                Tool::Antigravity,
                true,
                &fields,
                Some("remain 60% · reset 3d4h"),
            ),
            Some("remain 60% · reset 3d4h".into())
        );
    }

    #[test]
    fn summarize_antigravity_falls_back_to_sync_message_when_model_present() {
        let fields = vec![FieldEntry {
            key: "Model".into(),
            value: "Gemini 3.1 Pro".into(),
            options: vec![],
            editable: false,
        }];

        assert_eq!(
            summarize_tool_fields(Tool::Antigravity, true, &fields, None),
            Some("Open Antigravity to sync quota".into())
        );
    }

    #[test]
    fn antigravity_unified_state_decodes_int32_credit_values() {
        let available = encode_int32_state_value(128);
        let minimum = encode_int32_state_value(4);
        let raw = encode_antigravity_unified_state(&[
            ("availableCreditsSentinelKey", &available),
            ("minimumCreditAmountForUsageKey", &minimum),
        ]);

        let entries = parse_antigravity_unified_state(&raw).expect("entries");
        let parsed_available = entries
            .iter()
            .find(|(key, _)| key == "availableCreditsSentinelKey")
            .and_then(|(_, value)| decode_antigravity_int32_value(value));
        let parsed_minimum = entries
            .iter()
            .find(|(key, _)| key == "minimumCreditAmountForUsageKey")
            .and_then(|(_, value)| decode_antigravity_int32_value(value));

        assert_eq!(parsed_available, Some(128));
        assert_eq!(parsed_minimum, Some(4));
    }

    #[test]
    fn antigravity_model_label_from_sentinel_value_maps_known_models() {
        assert_eq!(
            antigravity_model_label_from_sentinel_value(37),
            Some("Gemini 3.1 Pro")
        );
        assert_eq!(
            antigravity_model_label_from_sentinel_value(35),
            Some("Claude Sonnet 4.6")
        );
    }

    #[test]
    fn antigravity_model_id_from_sentinel_maps_placeholder_suffix() {
        let model_ids = vec![
            "MODEL_PLACEHOLDER_M37".to_string(),
            "MODEL_PLACEHOLDER_M35".to_string(),
            "MODEL_PLACEHOLDER_M18".to_string(),
        ];
        assert_eq!(
            antigravity_model_id_from_sentinel(1035, &model_ids),
            Some("MODEL_PLACEHOLDER_M35".into())
        );
    }

    #[test]
    fn antigravity_process_info_parses_pid_csrf_and_extension_port() {
        let line = "34643 /Applications/Antigravity.app/Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm --enable_lsp --csrf_token abc --extension_server_port 56502 --extension_server_csrf_token def --random_port --app_data_dir antigravity";
        let parsed = parse_antigravity_process_info_line(line).expect("process info");
        assert_eq!(parsed.pid, 34643);
        assert_eq!(parsed.csrf_token, "abc");
        assert_eq!(parsed.extension_server_port, Some(56502));
    }

    #[test]
    fn antigravity_process_info_rejects_non_app_server_processes() {
        let line = "34643 /tmp/language_server_macos_arm --enable_lsp --csrf_token abc --extension_server_port 56502";
        assert!(parse_antigravity_process_info_line(line).is_none());
    }

    #[test]
    fn antigravity_unified_state_returns_none_for_malformed_payload() {
        let raw = base64::engine::general_purpose::STANDARD.encode([0x0a, 0x05, 0x01]);
        assert!(parse_antigravity_unified_state(&raw).is_none());
    }

    #[test]
    fn antigravity_usage_snapshot_parses_live_quota_windows() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "clientModelConfigs": [
                            {
                                "label": "Claude Sonnet 4.6 (Thinking)",
                                "modelOrAlias": {
                                    "model": "claude-sonnet-thinking"
                                },
                                "quotaInfo": {
                                    "remainingFraction": 0.6,
                                    "resetTime": "2100-03-12T07:44:33Z"
                                }
                            },
                            {
                                "label": "Gemini 3 Flash",
                                "quotaInfo": {
                                    "remainingFraction": 1.0
                                }
                            }
                        ]
                    }
                }
            },
            "command_model_configs": {
                "configs": [
                    {
                        "modelId": "claude-sonnet-thinking",
                        "modelDisplayName": "Claude Sonnet 4.6 (Thinking)"
                    }
                ]
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(
            snapshot.selected_model_label,
            Some("Claude Sonnet 4.6".into())
        );
        assert!(snapshot
            .summary
            .as_deref()
            .is_some_and(|summary| summary.starts_with("remain 60%")));
    }

    #[test]
    fn antigravity_usage_snapshot_ignores_unrelated_nested_quota_entries() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M18"
                            }
                        },
                        "clientModelConfigs": [
                            {
                                "label": "Gemini 3 Flash",
                                "modelOrAlias": {
                                    "model": "MODEL_PLACEHOLDER_M18"
                                },
                                "quotaInfo": {
                                    "remainingFraction": 1.0
                                }
                            }
                        ]
                    },
                    "someOtherSection": {
                        "history": [
                            {
                                "label": "Gemini 3 Flash",
                                "modelOrAlias": {
                                    "model": "MODEL_PLACEHOLDER_M18"
                                },
                                "quotaInfo": {
                                    "remainingFraction": 0.1
                                }
                            }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3 Flash".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_falls_back_to_first_model_when_unselected() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "clientModelConfigs": [
                            { "label": "Claude Opus 4.6 (Thinking)", "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Claude Sonnet 4.6 (Thinking)", "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Gemini 3 Flash", "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Gemini 3.1 Pro (High)", "quotaInfo": { "remainingFraction": 1.0 } }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(
            snapshot.selected_model_label,
            Some("Claude Opus 4.6".into())
        );
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_falls_back_to_first_non_selected_model() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "clientModelConfigs": [
                            { "label": "Claude Sonnet 4.6 (Thinking)", "quotaInfo": { "remainingFraction": 0.6 } },
                            { "label": "Gemini 3 Flash", "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "GPT-OSS 120B (Medium)", "quotaInfo": { "remainingFraction": 0.6 } }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(
            snapshot.selected_model_label,
            Some("Claude Sonnet 4.6".into())
        );
        assert_eq!(snapshot.summary.as_deref(), Some("remain 60%"));
    }

    #[test]
    fn antigravity_usage_snapshot_prefers_default_override_model_when_available() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M18"
                            }
                        },
                        "clientModelConfigs": [
                            { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" }, "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Claude Sonnet 4.6 (Thinking)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M35" }, "quotaInfo": { "remainingFraction": 0.6 } },
                            { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" }, "quotaInfo": { "remainingFraction": 1.0 } }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3 Flash".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_prefers_live_default_override_over_stale_sentinel() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M37"
                            }
                        },
                        "clientModelConfigs": [
                            { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" }, "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Claude Sonnet 4.6 (Thinking)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M35" }, "quotaInfo": { "remainingFraction": 0.6 } }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3.1 Pro".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_prefers_default_override_over_command_model_list() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M37"
                            }
                        },
                        "clientModelConfigs": [
                            { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" }, "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" }, "quotaInfo": { "remainingFraction": 1.0 } }
                        ]
                    }
                }
            },
            "command_model_configs": {
                "clientModelConfigs": [
                    { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" } }
                ]
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3.1 Pro".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_ignores_ambiguous_command_model_configs() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M37"
                            }
                        },
                        "clientModelConfigs": [
                            { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" }, "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" }, "quotaInfo": { "remainingFraction": 1.0 } }
                        ]
                    }
                }
            },
            "command_model_configs": {
                "clientModelConfigs": [
                    { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" } },
                    { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" } }
                ]
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3.1 Pro".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 100%"));
    }

    #[test]
    fn antigravity_usage_snapshot_prefers_explicit_command_selected_model() {
        let data = serde_json::json!({
            "user_status": {
                "userStatus": {
                    "cascadeModelConfigData": {
                        "defaultOverrideModelConfig": {
                            "modelOrAlias": {
                                "model": "MODEL_PLACEHOLDER_M37"
                            }
                        },
                        "clientModelConfigs": [
                            { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" }, "quotaInfo": { "remainingFraction": 1.0 } },
                            { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" }, "quotaInfo": { "remainingFraction": 0.8 } }
                        ]
                    }
                }
            },
            "command_model_configs": {
                "selectedModelConfig": {
                    "modelOrAlias": {
                        "model": "MODEL_PLACEHOLDER_M18"
                    }
                },
                "clientModelConfigs": [
                    { "label": "Gemini 3 Flash", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M18" } },
                    { "label": "Gemini 3.1 Pro (High)", "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M37" } }
                ]
            }
        });

        let snapshot = parse_antigravity_usage_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.selected_model_label, Some("Gemini 3 Flash".into()));
        assert_eq!(snapshot.summary.as_deref(), Some("remain 80%"));
    }

    #[test]
    fn extract_antigravity_fields_include_model_and_auth_only() {
        let snapshot = AntigravityUsageSnapshot {
            summary: Some("remain 60%".into()),
            selected_model_label: Some("Claude Sonnet 4.6".into()),
        };

        let fields = extract_antigravity_fields(Some(&snapshot));
        assert!(fields
            .iter()
            .any(|field| field.key == "Model" && field.value == "Claude Sonnet 4.6"));
        assert!(fields.iter().all(|field| field.key != "Quota"));
        assert!(fields
            .iter()
            .all(|field| !field.key.starts_with("Quota · ")));
        assert!(fields.iter().all(|field| field.key != "Prompt Credits"));
        assert!(fields.iter().all(|field| field.key != "Flow Credits"));
        assert!(fields.iter().all(|field| field.key != "Plan"));
    }

    #[test]
    fn antigravity_model_click_shows_status_instead_of_editing() {
        let mut app = App {
            tools: vec![ToolState {
                tool: Tool::Antigravity,
                installed: true,
                fields: vec![FieldEntry {
                    key: "Model".into(),
                    value: "Gemini 3.1 Pro".into(),
                    options: vec![],
                    editable: false,
                }],
                summary: Some("remain 100%".into()),
            }],
            tool_index: 0,
            field_index: 0,
            assistant_collapsed: false,
            usage_update_rx: None,
            usage_update_tx: None,
            usage_update_generation: Arc::new(AtomicUsize::new(0)),
            focus: Focus::ToolList,
            mode: AppMode::Browsing,
            status_msg: None,
            status_expire: None,
            last_error: None,
            error_expire: None,
            should_quit: false,
        };

        app.start_edit();

        assert_eq!(
            app.status_msg.as_deref(),
            Some("Change the model in Antigravity settings.")
        );
        assert!(!app.is_editing());
        assert!(!app.is_selecting());
    }

    #[test]
    fn confirm_key_accepts_space_and_enter() {
        assert!(is_confirm_key(KeyCode::Char(' ')));
        assert!(is_confirm_key(KeyCode::Enter));
    }

    #[test]
    fn antigravity_plan_name_extracts_google_ai_tier() {
        let raw = base64::engine::general_purpose::STANDARD
            .encode(b"\0\0Google AI Pro\0ignored model text");
        assert_eq!(
            extract_antigravity_plan_name(&raw),
            Some("Google AI Pro".into())
        );
    }

    #[test]
    fn summarize_gemini_falls_back_to_auth_and_model_when_quota_missing() {
        let fields = vec![
            FieldEntry {
                key: "Model".into(),
                value: "gemini-3.1-flash-lite-preview".into(),
                options: vec![],
                editable: true,
            },
            FieldEntry {
                key: "Auth".into(),
                value: "✓ user@example.com".into(),
                options: vec![],
                editable: false,
            },
        ];

        assert_eq!(
            summarize_tool_fields(Tool::Gemini, true, &fields, None),
            Some("user@example.com · gemini-3.1-flash-lite-preview".into())
        );
    }

    #[test]
    fn claude_usage_snapshot_shows_reauth_required_for_invalid_refresh_state() {
        let parsed = serde_json::json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "details": {
                    "error_code": "token_expired"
                }
            }
        });

        let snapshot = parse_claude_usage_snapshot(&parsed).expect("snapshot");
        assert_eq!(snapshot.summary, Some("Re-auth required".into()));
    }

    #[test]
    fn parse_claude_usage_error_detects_auth_failure() {
        let parsed = serde_json::json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "details": {
                    "error_code": "token_expired"
                }
            }
        });

        assert_eq!(
            parse_claude_usage_error(&parsed).as_deref(),
            Some("Re-auth required")
        );
    }

    #[test]
    fn parse_claude_keychain_account_extracts_account_name() {
        let raw = r#"
keychain: "/Users/alice/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    "acct"<blob>="alice"
    "svce"<blob>="Claude Code-credentials"
"#;

        assert_eq!(parse_claude_keychain_account(raw), Some("alice".into()));
    }

    #[test]
    fn gemini_quota_summary_uses_auth_type() {
        let parsed = serde_json::json!({
            "security": {
                "auth": {
                    "selectedType": "oauth-personal"
                }
            }
        });
        assert_eq!(
            gemini_quota_summary(&parsed),
            Some("Quota 1000/day · 60/min".into())
        );
    }

    #[test]
    fn copilot_usage_snapshot_prefers_remaining_count() {
        let parsed = serde_json::json!({
            "quota_reset_date_utc": "2100-04-01T00:00:00.000Z",
            "quota_snapshots": {
                "premium_interactions": {
                    "remaining": 298,
                    "quota_remaining": 298.34
                }
            }
        });
        let snapshot = parse_copilot_usage_snapshot(&parsed).expect("snapshot");
        let summary = snapshot.summary.expect("summary");
        assert!(summary.starts_with("298 left this month · reset "));
    }

    #[test]
    fn kimi_extract_reads_model_and_auth_field() {
        let fields = extract_kimi_fields(
            r#"
default_model = "kimi-code/kimi-for-coding"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
"#,
        );
        assert_eq!(fields[0].key, "Model");
        assert_eq!(fields[0].value, "kimi-code/kimi-for-coding");
        assert_eq!(fields[1].key, "Auth");
    }

    #[test]
    fn kimi_save_round_trip_for_default_model() {
        let path = std::env::temp_dir().join(format!(
            "kaku-kimi-{}.toml",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("unix epoch")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "default_model = \"kimi-code/kimi-for-coding\"\n[models.\"kimi-code/kimi-for-coding\"]\nprovider = \"managed:kimi-code\"\n",
        )
        .expect("write temp config");

        save_kimi_field_at(&path, "Model", "kimi-code/new-model").expect("save model");
        let updated = std::fs::read_to_string(&path).expect("read temp config");
        assert!(updated.contains("default_model = \"kimi-code/new-model\""));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn kimi_usage_snapshot_formats_current_and_weekly_windows() {
        let parsed = serde_json::json!({
            "usage": {
                "limit": "100",
                "used": "33",
                "remaining": "67",
                "resetTime": "2100-03-12T14:43:16.853067Z"
            },
            "limits": [
                {
                    "window": {
                        "duration": 300,
                        "timeUnit": "TIME_UNIT_MINUTE"
                    },
                    "detail": {
                        "limit": "100",
                        "used": "6",
                        "remaining": "94",
                        "resetTime": "2100-03-08T03:43:16.853067Z"
                    }
                }
            ]
        });

        let snapshot = parse_kimi_usage_snapshot(&parsed).expect("snapshot");
        let summary = snapshot.summary.expect("summary");
        assert!(summary.starts_with("5h remain 94% · reset "));
        assert!(summary.contains("  |  7d remain 67% · reset "));
    }

    #[test]
    fn kimi_auth_status_accepts_refreshable_session() {
        let auth = serde_json::json!({
            "access_token": "",
            "refresh_token": "refresh-token",
            "expires_at": 0.0
        });
        let dir = tempdir().expect("tempdir");
        let path = dir
            .path()
            .join(".kimi")
            .join("credentials")
            .join("kimi-code.json");
        let parent = path.parent().expect("credentials dir");
        std::fs::create_dir_all(parent).expect("create credentials dir");
        std::fs::write(&path, serde_json::to_vec(&auth).expect("serialize auth"))
            .expect("write credentials");

        assert_eq!(read_kimi_auth_status_from_path(&path), "✓ oauth");
    }

    #[test]
    fn claude_auth_status_prefers_email_when_logged_in() {
        let parsed = serde_json::json!({
            "loggedIn": true,
            "authMethod": "claude.ai",
            "email": "user@example.com",
        });

        assert_eq!(
            parse_claude_auth_status(&parsed),
            Some("✓ user@example.com".into())
        );
    }

    #[test]
    fn claude_auth_status_reports_signed_out() {
        let parsed = serde_json::json!({
            "loggedIn": false,
            "authMethod": "claude.ai",
        });

        assert_eq!(
            parse_claude_auth_status(&parsed),
            Some("✗ not signed in".into())
        );
    }

    #[test]
    fn claude_auth_status_appends_subscription_tier() {
        // Real `claude auth status` output shape (Max + Pro + lowercase pro).
        for (tier, want) in [
            ("max", "✓ user@example.com · Max"),
            ("pro", "✓ user@example.com · Pro"),
            ("plus", "✓ user@example.com · Plus"),
        ] {
            let parsed = serde_json::json!({
                "loggedIn": true,
                "authMethod": "claude.ai",
                "email": "user@example.com",
                "subscriptionType": tier,
            });
            assert_eq!(
                parse_claude_auth_status(&parsed).as_deref(),
                Some(want),
                "tier={tier}"
            );
        }
    }

    #[test]
    fn format_auth_status_handles_optional_plan() {
        assert_eq!(
            format_auth_status(Some("foo@bar".to_string()), "oauth", None),
            "✓ foo@bar"
        );
        assert_eq!(
            format_auth_status(Some("foo@bar".to_string()), "oauth", Some("max")),
            "✓ foo@bar · Max"
        );
        // Empty plan string is treated as no plan.
        assert_eq!(
            format_auth_status(Some("foo@bar".to_string()), "oauth", Some("")),
            "✓ foo@bar"
        );
        // Account missing → falls back to method, plan still appended.
        assert_eq!(
            format_auth_status(None, "github", Some("pro")),
            "✓ github · Pro"
        );
    }

    #[test]
    fn kaku_assistant_fields_show_simple_and_deep_models() {
        let fields = extract_kaku_assistant_fields(
            "enabled = true\napi_key = \"sk-test\"\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n",
        );
        // No Provider row.
        assert!(fields.iter().all(|f| f.key != "Provider"));
        // Fixed rows always present.
        for key in &[
            "Enabled",
            "API Mode",
            "Simple Model",
            "Deep Model",
            "Base URL",
            "API Key",
            "Web Search",
        ] {
            assert!(
                fields.iter().any(|f| f.key == *key),
                "missing field: {}",
                key
            );
        }
        let simple_model = fields.iter().find(|f| f.key == "Simple Model").unwrap();
        assert_eq!(simple_model.value, "gpt-5.4-mini");
        let deep_model = fields.iter().find(|f| f.key == "Deep Model").unwrap();
        assert_eq!(deep_model.value, "gpt-5.4-mini");
        assert!(fields.iter().all(|f| f.key != "Fast Model"));
        let api_key = fields.iter().find(|f| f.key == "API Key").unwrap();
        assert_ne!(api_key.value, "-");
    }

    #[test]
    fn kaku_assistant_legacy_fast_model_collapses_into_simple_model() {
        let raw = "enabled = true\napi_key = \"sk-test\"\nmodel = \"gpt-5.4\"\nfast_model = \"gpt-5.4-mini\"\nchat_model = \"gpt-5.5\"\nbase_url = \"https://api.openai.com/v1\"\n";
        let fields = extract_kaku_assistant_fields(raw);

        let simple_model = fields.iter().find(|f| f.key == "Simple Model").unwrap();
        assert_eq!(simple_model.value, "gpt-5.4-mini");
        let deep_model = fields.iter().find(|f| f.key == "Deep Model").unwrap();
        assert_eq!(deep_model.value, "gpt-5.5");

        let cfg = parse_kaku_assistant_config(raw);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        write_kaku_assistant_config(&path, &cfg).expect("write config");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(saved.contains("model = \"gpt-5.4-mini\""));
        assert!(
            !saved.lines().any(|l| {
                let head = l.split('#').next().unwrap_or("").trim_start();
                head.starts_with("fast_model =")
            }),
            "fast_model should be folded into model on save: {}",
            saved
        );
    }

    #[test]
    fn kaku_assistant_legacy_fast_model_preserves_old_model_as_deep_model() {
        let raw = "enabled = true\napi_key = \"sk-test\"\nmodel = \"gpt-5.4\"\nfast_model = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n";
        let fields = extract_kaku_assistant_fields(raw);

        let simple_model = fields.iter().find(|f| f.key == "Simple Model").unwrap();
        assert_eq!(simple_model.value, "gpt-5.4-mini");
        let deep_model = fields.iter().find(|f| f.key == "Deep Model").unwrap();
        assert_eq!(deep_model.value, "gpt-5.4");

        let cfg = parse_kaku_assistant_config(raw);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        write_kaku_assistant_config(&path, &cfg).expect("write config");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(saved.contains("model = \"gpt-5.4-mini\""));
        assert!(saved.contains("chat_model = \"gpt-5.4\""));
        assert!(
            !saved.lines().any(|l| {
                let head = l.split('#').next().unwrap_or("").trim_start();
                head.starts_with("fast_model =")
            }),
            "fast_model should be folded into model on save: {}",
            saved
        );
    }

    #[test]
    fn normalize_assistant_model_options_keeps_simple_and_deep_first() {
        let models = normalize_assistant_model_options(
            vec![
                "gpt-5.4-mini".into(),
                "gpt-5.5".into(),
                "gpt-5.5".into(),
                String::new(),
            ],
            "gpt-5.5",
            "gpt-5.2",
        );
        assert_eq!(models.first().map(String::as_str), Some("gpt-5.5"));
        assert_eq!(models.get(1).map(String::as_str), Some("gpt-5.2"));
        assert!(models.iter().any(|m| m == "gpt-5.4-mini"));
    }

    #[test]
    fn normalize_assistant_model_options_keeps_more_than_fifty_models() {
        let mut remote_models: Vec<String> = (0..75)
            .map(|idx| format!("openrouter/model-{idx:02}"))
            .collect();
        remote_models.push("gpt-5.5".into());

        let models =
            normalize_assistant_model_options(remote_models, "custom/current", "custom/fast");

        assert_eq!(models.first().map(String::as_str), Some("custom/current"));
        assert_eq!(models.get(1).map(String::as_str), Some("custom/fast"));
        assert!(
            models.len() > 50,
            "model selector must not drop providers with large catalogs"
        );
        assert!(models.iter().any(|m| m == "openrouter/model-74"));
    }

    #[test]
    fn kaku_assistant_save_base_url_and_model_round_trip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        // Parse, change base_url and model, write back.
        let raw = std::fs::read_to_string(&path).expect("read");
        let cfg = parse_kaku_assistant_config(&raw);
        let mut updated = KakuAssistantConfig::new(
            cfg.is_enabled(),
            cfg.api_key(),
            "my-model",
            "https://my-proxy.example.com/v1",
        )
        .with_custom_headers(cfg.custom_headers().to_vec());
        updated.auth_type = cfg.auth_type().to_string();
        write_kaku_assistant_config(&path, &updated).expect("write config");

        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(saved.contains("model = \"my-model\""));
        assert!(saved.contains("base_url = \"https://my-proxy.example.com/v1\""));
    }

    /// The GUI reads what `kaku ai` writes; both sides assert this fixture.
    /// The CLI half also re-reads its own rendered file, so a rewrite through
    /// the TUI must not drop anything the GUI depends on.
    #[test]
    fn kaku_assistant_config_matches_shared_gui_contract() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/assistant-config.json"
        )))
        .expect("shared assistant config fixtures");
        for case in cases.as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let parsed = parse_kaku_assistant_config(case["toml"].as_str().unwrap());
            let rewritten = parse_kaku_assistant_config(&render_kaku_assistant_config(&parsed));
            for (stage, cfg) in [("parse", &parsed), ("rewrite", &rewritten)] {
                let ctx = format!("{} ({})", name, stage);
                assert_eq!(
                    cfg.model(),
                    case["simple_model"].as_str().unwrap(),
                    "{}",
                    ctx
                );
                assert_eq!(
                    cfg.deep_model(),
                    case["deep_model"].as_str().unwrap(),
                    "{}",
                    ctx
                );
                assert_eq!(
                    cfg.base_url().trim_end_matches('/'),
                    case["base_url"].as_str().unwrap(),
                    "{}",
                    ctx
                );
                assert_eq!(
                    cfg.api_mode(),
                    case["api_mode"].as_str().unwrap(),
                    "{}",
                    ctx
                );
                assert_eq!(
                    cfg.auth_type(),
                    case["auth_type"].as_str().unwrap(),
                    "{}",
                    ctx
                );
                let choices: Vec<&str> = case["chat_model_choices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(cfg.chat_model_choices(), choices.as_slice(), "{}", ctx);
            }
        }
    }

    #[test]
    fn kaku_assistant_save_auth_type_codex_round_trip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\napi_key = \"sk-keep\"\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        // Switch auth to codex via the TUI save path.
        save_kaku_assistant_field_to_path(&path, "Auth Type", "codex").expect("save auth type");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(
            saved.contains("auth_type = \"codex\""),
            "auth_type must persist: {}",
            saved
        );
        // api_key stays in the file even though codex hides it in the UI.
        assert!(saved.contains("api_key = \"sk-keep\""));
        assert!(saved.contains("model = \"Follow Codex\""));
        assert!(!saved.contains("chat_model ="));

        let fields = extract_kaku_assistant_fields(&saved);
        assert!(fields
            .iter()
            .any(|f| f.key == "Auth Type" && f.value == "codex"));
        assert!(fields
            .iter()
            .any(|f| f.key == "Simple Model" && f.value == FOLLOW_CODEX_MODEL));
        assert!(fields
            .iter()
            .any(|f| f.key == "Deep Model" && f.value == FOLLOW_CODEX_MODEL));
        assert!(
            !fields.iter().any(|f| f.key == "API Key"),
            "API Key field must be hidden under codex auth"
        );
        assert!(
            !fields.iter().any(|f| f.key == "Base URL"),
            "Base URL field must be hidden under codex auth"
        );
        assert!(
            !fields.iter().any(|f| f.key == "API Mode"),
            "API Mode is fixed and must be hidden under codex auth"
        );
        // Auth Type sits right after Enabled.
        assert_eq!(fields.first().map(|f| f.key.as_str()), Some("Enabled"));
        assert_eq!(fields.get(1).map(|f| f.key.as_str()), Some("Auth Type"));

        // Switching back to api_key drops the auth_type line and restores the field.
        save_kaku_assistant_field_to_path(&path, "Auth Type", "api_key").expect("save api_key");
        let saved2 = std::fs::read_to_string(&path).expect("read saved2");
        assert!(!saved2.contains("auth_type ="));
        assert!(saved2.contains("model = \"gpt-5.4-mini\""));
        let fields2 = extract_kaku_assistant_fields(&saved2);
        assert!(fields2.iter().any(|f| f.key == "API Key"));
    }

    #[test]
    fn kaku_assistant_switch_to_codex_preserves_explicit_model_overrides() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"provider-fast\"\nchat_model = \"provider-deep\"\nbase_url = \"https://example.com/v1\"\n",
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Auth Type", "codex").expect("save auth type");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(saved.contains("auth_type = \"codex\""));
        assert!(saved.contains("model = \"provider-fast\""));
        assert!(saved.contains("chat_model = \"provider-deep\""));
    }

    #[test]
    fn kaku_assistant_responses_and_native_search_round_trip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "API Mode", "responses")
            .expect("save responses mode");
        let fields = extract_kaku_assistant_fields(
            &std::fs::read_to_string(&path).expect("read responses config"),
        );
        assert!(fields
            .iter()
            .any(|field| field.key == "API Mode" && field.value == "responses"));
        assert!(fields
            .iter()
            .any(|field| { field.key == "Native Web Search" && field.value == "Off" }));

        save_kaku_assistant_field_to_path(&path, "Native Web Search", "On")
            .expect("enable native search");
        save_kaku_assistant_field_to_path(&path, "Simple Model", "gpt-5.4")
            .expect("round-trip transport fields");
        let saved = std::fs::read_to_string(&path).expect("read saved config");
        assert!(saved.lines().any(|line| line == "api_mode = \"responses\""));
        assert!(saved.lines().any(|line| line == "native_web_search = true"));

        save_kaku_assistant_field_to_path(&path, "API Mode", "chat_completions")
            .expect("restore chat completions");
        let saved = std::fs::read_to_string(&path).expect("read restored config");
        assert!(!saved.lines().any(|line| line == "native_web_search = true"));
    }

    #[test]
    fn kaku_assistant_save_preserves_chat_model_keys() {
        // Regression: saving any field through the TUI must not drop chat_model
        // or chat_model_choices, which the GUI overlay relies on.
        let raw = "enabled = true\nmodel = \"gpt-5.4-mini\"\nchat_model = \"gpt-5.4\"\nchat_model_choices = [\"gpt-5.4\", \"claude-sonnet-4-6\"]\nbase_url = \"https://api.openai.com/v1\"\n";
        let cfg = parse_kaku_assistant_config(raw);
        assert_eq!(cfg.chat_model(), "gpt-5.4");
        assert_eq!(cfg.chat_model_choices(), &["gpt-5.4", "claude-sonnet-4-6"]);

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        write_kaku_assistant_config(&path, &cfg).expect("write config");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(
            saved.contains("chat_model = \"gpt-5.4\""),
            "chat_model must round-trip but was missing: {}",
            saved
        );
        assert!(
            saved.contains("chat_model_choices = [\"gpt-5.4\", \"claude-sonnet-4-6\"]"),
            "chat_model_choices must round-trip but was missing: {}",
            saved
        );

        // Re-parse and confirm full round trip.
        let cfg2 = parse_kaku_assistant_config(&saved);
        assert_eq!(cfg2.chat_model(), "gpt-5.4");
        assert_eq!(cfg2.chat_model_choices(), &["gpt-5.4", "claude-sonnet-4-6"]);
    }

    #[test]
    fn kaku_assistant_tui_save_preserves_runtime_and_unknown_keys() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            concat!(
                "enabled = true\n",
                "model = \"gpt-5.4-mini\"\n",
                "base_url = \"https://api.openai.com/v1\"\n",
                "chat_tools_enabled = false\n",
                "web_fetch_script = \"~/bin/fetch-safe\"\n",
                "memory_curator_model = \"gpt-5.4-nano\"\n",
                "future_provider_option = \"keep-me\"\n",
            ),
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Simple Model", "gpt-5.4").expect("save model");
        let saved = std::fs::read_to_string(&path).expect("read saved config");

        for expected in [
            "chat_tools_enabled = false",
            "web_fetch_script = \"~/bin/fetch-safe\"",
            "memory_curator_model = \"gpt-5.4-nano\"",
            "future_provider_option = \"keep-me\"",
        ] {
            assert!(
                saved.contains(expected),
                "TUI save dropped `{}`:\n{}",
                expected,
                saved
            );
        }
    }

    #[test]
    fn kaku_assistant_tui_save_preserves_comments() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            concat!(
                "# Kaku assistant configuration\n",
                "enabled = true\n",
                "model = \"gpt-5.4-mini\"\n",
                "# Custom option kept across saves\n",
                "future_provider_option = \"keep-me\"\n",
            ),
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Simple Model", "gpt-5.4").expect("save model");
        let saved = std::fs::read_to_string(&path).expect("read saved config");

        assert!(
            saved.contains("# Kaku assistant configuration"),
            "{}",
            saved
        );
        assert!(
            saved.contains("# Custom option kept across saves"),
            "{}",
            saved
        );
        assert!(saved.contains("model = \"gpt-5.4\""), "{}", saved);
    }

    #[test]
    fn kaku_assistant_auto_fix_ignored_exit_codes_parse_and_normalize() {
        let cfg = parse_kaku_assistant_config(
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nauto_fix_ignored_exit_codes = [2, -1, 2, 130]\n",
        );
        assert_eq!(cfg.auto_fix_ignored_exit_codes(), &[2, 130]);

        let cfg = parse_kaku_assistant_config(
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nauto_fix_ignored_exit_codes = \"2,nope,-1,2\"\n",
        );
        assert_eq!(cfg.auto_fix_ignored_exit_codes(), &[2]);

        let cfg = parse_kaku_assistant_config("enabled = true\nmodel = \"gpt-5.4-mini\"\n");
        assert!(cfg.auto_fix_ignored_exit_codes().is_empty());
    }

    #[test]
    fn kaku_assistant_save_preserves_auto_fix_ignored_exit_codes() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nauto_fix_ignored_exit_codes = [2]\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Base URL", "https://proxy.example.com/v1")
            .expect("save base url");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(
            saved.contains("auto_fix_ignored_exit_codes = [2]"),
            "ignored exit codes must round-trip through TUI saves: {}",
            saved
        );
    }

    #[test]
    fn kaku_assistant_deep_model_field_is_editable() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nchat_model = \"gpt-5.5\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Deep Model", "claude-sonnet-4-6")
            .expect("save deep model");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(saved.contains("model = \"gpt-5.4-mini\""));
        assert!(saved.contains("chat_model = \"claude-sonnet-4-6\""));
    }

    #[test]
    fn kaku_assistant_deep_model_clears_when_dash_selected() {
        // Regression: picking "-" in the Deep Model selector must drop the key,
        // not silently restore the previous value via passthrough.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nchat_model = \"gpt-5.5\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .expect("write temp config");

        save_kaku_assistant_field_to_path(&path, "Deep Model", "-")
            .expect("save cleared deep model");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(
            !saved.lines().any(|l| {
                let head = l.split('#').next().unwrap_or("").trim_start();
                head.starts_with("chat_model =")
            }),
            "chat_model must be absent after clearing with '-': {}",
            saved
        );
        assert!(
            saved.contains("model = \"gpt-5.4-mini\""),
            "model must be preserved"
        );
    }

    const MALFORMED_ASSISTANT_TOML: &str =
        "model = \"a\"\nmodel = \"b\"\napi_key = \"sk-x\"\nchat_tools_enabled = false\n";

    const MALFORMED_ASSISTANT_TOML_SAVES: &[(&str, &str)] = &[
        ("Enabled", "Off"),
        ("Simple Model", "gpt-5.4"),
        ("Deep Model", "gpt-5.5"),
        ("Base URL", "https://proxy.example.com/v1"),
        ("API Key", "sk-new"),
    ];

    #[test]
    fn kaku_assistant_save_errors_on_malformed_toml() {
        for (field, value) in MALFORMED_ASSISTANT_TOML_SAVES {
            let dir = tempdir().expect("tempdir");
            let path = dir.path().join("assistant.toml");
            std::fs::write(&path, MALFORMED_ASSISTANT_TOML).expect("write temp config");

            let result = save_kaku_assistant_field_to_path(&path, field, value);
            assert!(
                result.is_err(),
                "saving {} over malformed assistant.toml must fail",
                field
            );
        }
    }

    #[test]
    fn kaku_assistant_save_leaves_malformed_toml_bytes_unchanged() {
        for (field, value) in MALFORMED_ASSISTANT_TOML_SAVES {
            let dir = tempdir().expect("tempdir");
            let path = dir.path().join("assistant.toml");
            std::fs::write(&path, MALFORMED_ASSISTANT_TOML).expect("write temp config");

            let _ = save_kaku_assistant_field_to_path(&path, field, value);
            let after = std::fs::read(&path).expect("read after save");
            assert_eq!(
                String::from_utf8_lossy(&after),
                MALFORMED_ASSISTANT_TOML,
                "saving {} must not rewrite malformed assistant.toml",
                field
            );
        }
    }

    #[test]
    fn kaku_assistant_save_omits_chat_model_when_unset() {
        // When the user has no chat_model, neither key should appear in the
        // file (keeps the default template clean).
        let raw =
            "enabled = true\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\n";
        let cfg = parse_kaku_assistant_config(raw);
        assert!(cfg.chat_model().is_empty());
        assert!(cfg.chat_model_choices().is_empty());

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        write_kaku_assistant_config(&path, &cfg).expect("write config");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        assert!(
            !saved.lines().any(|l| {
                let head = l.split('#').next().unwrap_or("").trim_start();
                head.starts_with("chat_model =") || head.starts_with("chat_model_choices =")
            }),
            "chat_model keys must stay absent when unset: {}",
            saved
        );
    }

    #[test]
    fn kaku_assistant_provider_round_trip_preserves_headers() {
        let raw = "enabled = true\napi_key = \"sk-test\"\nmodel = \"gpt-5.4-mini\"\nbase_url = \"https://api.openai.com/v1\"\ncustom_headers = [\"X-Foo: bar\"]\n";
        let cfg = parse_kaku_assistant_config(raw);
        assert_eq!(cfg.custom_headers(), &["X-Foo: bar"]);

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("assistant.toml");
        write_kaku_assistant_config(&path, &cfg).expect("write config");
        let saved = std::fs::read_to_string(&path).expect("read saved");
        // The AI provider preset (e.g. "OpenAI") must not be persisted because
        // it is derived from base_url at load time, not a stored field.
        // Check only uncommented lines to avoid tripping on commented-out web_search_provider.
        assert!(
            !saved.lines().any(|l| {
                // Strip comments: only look at the text before the first '#'.
                l.split('#').next().unwrap_or("").contains("provider =")
            }),
            "provider preset must not be written to TOML"
        );
        assert!(saved.contains("custom_headers = [\"X-Foo: bar\"]"));
    }

    #[test]
    fn codex_usage_snapshot_prefers_current_window() {
        let parsed = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 62.7,
                    "reset_at": 4_102_444_800i64
                },
                "secondary_window": {
                    "used_percent": 18.0,
                    "reset_at": 4_102_704_000i64
                }
            }
        });

        let snapshot = parse_codex_usage_snapshot(&parsed).expect("snapshot");
        let summary = snapshot.summary.expect("summary");
        assert!(summary.starts_with("5h remain 37.3% · reset "));
        assert!(summary.contains("  |  7d remain 82% · reset "));
    }

    #[test]
    fn gemini_save_model_updates_name_and_keeps_other_model_settings() {
        let mut parsed = serde_json::json!({"model": {"name": "a", "maxSessionTurns": 5}});
        let obj = parsed.as_object_mut().expect("settings object");

        save_gemini_model(obj, "b");

        assert_eq!(
            parsed.pointer("/model/name").and_then(|v| v.as_str()),
            Some("b")
        );
        assert_eq!(
            parsed
                .pointer("/model/maxSessionTurns")
                .and_then(|v| v.as_i64()),
            Some(5)
        );
    }

    #[test]
    fn gemini_clear_model_removes_name_and_keeps_other_model_settings() {
        let mut parsed = serde_json::json!({"model": {"name": "a", "maxSessionTurns": 5}});
        let obj = parsed.as_object_mut().expect("settings object");

        save_gemini_model(obj, "-");

        assert_eq!(parsed.pointer("/model/name"), None);
        assert_eq!(
            parsed
                .pointer("/model/maxSessionTurns")
                .and_then(|v| v.as_i64()),
            Some(5)
        );
    }
}
