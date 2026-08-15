//! AI client for Kaku's built-in chat overlay.
//!
//! Reads API config from `~/.config/kaku/assistant.toml` and provides
//! synchronous streaming clients for OpenAI-compatible Chat Completions and
//! Responses APIs.
//! Supports function/tool calling for agentic workflows.
//!
//! Runs on a plain OS thread (inside overlay), so blocking I/O is fine.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::ai_auth;
use crate::codex_connection::{self, CodexConnection, CodexCredential};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};

const DEFAULT_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_MODELS_BODY_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_SSE_LINE_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_STREAM_BYTES: usize = 16 * 1024 * 1024;
// Guards against runaway/looping streams, not against legitimate length:
// reasoning-heavy providers (DeepSeek/GLM) emit one small delta per SSE
// event, so a single long response can pass 16K events. Memory stays bounded
// by MAX_RESPONSE_STREAM_BYTES either way.
const MAX_RESPONSE_STREAM_EVENTS: usize = 65_536;
const MAX_RESPONSE_TOOL_CALLS: usize = 32;
const MAX_RESPONSE_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_OUTPUT_ITEMS: usize = 128;
const MAX_RESPONSE_CITATIONS: usize = 256;
const MAX_RESPONSE_CITATION_TITLE_CHARS: usize = 512;
const MAX_RESPONSE_CITATION_URL_CHARS: usize = 2_048;
const MAX_ERROR_BODY_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiMode {
    ChatCompletions,
    Responses,
}

impl ApiMode {
    fn from_config(value: Option<&str>) -> Self {
        match value.unwrap_or("chat_completions") {
            "chat_completions" => Self::ChatCompletions,
            "responses" => Self::Responses,
            other => {
                // Same tolerant policy as the `kaku ai` TUI, which coerces
                // unknown values to chat_completions: a typo in assistant.toml
                // must degrade to the default, not disable AI entirely.
                log::warn!("unknown api_mode `{other}` in assistant.toml; using chat_completions");
                Self::ChatCompletions
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }
}

/// Configuration loaded from `assistant.toml`.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AssistantConfig {
    pub api_key: String,
    /// Deep chat model. Falls back to the Simple Model from assistant.toml when omitted.
    pub chat_model: String,
    /// Optional user-curated model list for the chat overlay. When set, the chat
    /// overlay cycles only through these via Shift+Tab and skips the auto-fetch step.
    pub chat_model_choices: Vec<String>,
    pub base_url: String,
    /// Optional extra headers for enterprise proxies / API gateways.
    pub custom_headers: Vec<(String, String)>,
    /// Provider name derived from base_url and auth_type (e.g. "OpenAI", "Copilot").
    pub provider: String,
    /// API wire format for API-key/custom endpoints. Chat Completions remains
    /// the compatibility default; Codex following mode resolves its own Responses provider.
    pub api_mode: ApiMode,
    /// Auth mechanism: "api_key" (default), "copilot", or "codex".
    /// Legacy "gemini_key" values are recognized only to surface a friendly
    /// error at load time; the Gemini provider was removed in V0.10.0.
    pub auth_type: String,
    /// When false, the `tools` field is omitted from chat requests.
    /// Set `chat_tools_enabled = false` in assistant.toml for providers that do not
    /// support function calling (e.g. some Kimi or local-model variants).
    pub chat_tools_enabled: bool,
    /// Enable the provider-hosted Responses `web_search` tool. This does not
    /// require `web_search_provider` or `web_search_api_key`.
    pub native_web_search: bool,
    /// Web search provider: "brave", "pipellm", or "tavily". None = disabled.
    pub web_search_provider: Option<String>,
    /// API key for web_search_provider. None = search tool not registered.
    pub web_search_api_key: Option<String>,
    /// Hidden escape hatch: path to a custom fetch script (not in TUI or template).
    /// Script receives the URL as $1 and must print Markdown to stdout.
    pub web_fetch_script: Option<String>,
    /// Simple Model for quick command generation and lightweight chat. When it
    /// differs from chat_model, the overlay offers it via Shift+Tab.
    pub fast_model: Option<String>,
    /// Optional dedicated model for background memory curation. Falls back to
    /// `chat_model` when unset. Point at a cheaper/faster model to reduce cost.
    pub memory_curator_model: Option<String>,
}

impl AssistantConfig {
    /// Whether the assistant configuration file exists at the active config path.
    pub fn file_exists() -> bool {
        assistant_toml_path()
            .map(|path| path.is_file())
            .unwrap_or(false)
    }

    pub fn load() -> Result<Self> {
        let path = assistant_toml_path()?;
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let parsed: toml::Value = raw.parse().context("Invalid assistant.toml")?;

        let auth_type = parsed
            .get("auth_type")
            .and_then(|v| v.as_str())
            .unwrap_or("api_key")
            .to_string();

        let api_mode = ApiMode::from_config(parsed.get("api_mode").and_then(|v| v.as_str()));

        // The Gemini provider was removed in V0.10.0. Surface a clear migration
        // path instead of letting the OpenAI-compatible code path silently
        // mangle Gemini requests.
        if auth_type == "gemini_key" {
            anyhow::bail!(
                "Gemini provider was removed in V0.10.0. Open `kaku ai` and \
                 switch to a different provider (OpenAI, Copilot, Codex, or a \
                 custom OpenAI-compatible endpoint), then update {}.",
                path.display()
            );
        }

        let api_key = parsed
            .get("api_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let model = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_MODEL)
            .to_string();

        let legacy_fast_model = parsed
            .get("fast_model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        let mut simple_model = legacy_fast_model.clone().unwrap_or_else(|| model.clone());

        // If an old config had both model and fast_model but no chat_model,
        // preserve model as the deep slot and fold fast_model into Simple Model.
        let mut chat_model = parsed
            .get("chat_model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if legacy_fast_model.is_some() {
                    model.clone()
                } else {
                    simple_model.clone()
                }
            });

        if auth_type == "codex"
            && (simple_model == codex_connection::FOLLOW_CODEX_MODEL
                || chat_model == codex_connection::FOLLOW_CODEX_MODEL)
        {
            if let Some(codex_model) = codex_connection::load_configured_codex_model()
                .context("resolve configured Follow Codex model")?
            {
                if simple_model == codex_connection::FOLLOW_CODEX_MODEL {
                    simple_model = codex_model.clone();
                }
                if chat_model == codex_connection::FOLLOW_CODEX_MODEL {
                    chat_model = codex_model;
                }
            }
        }

        let chat_model_choices = parsed
            .get("chat_model_choices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();

        let custom_headers = parse_custom_headers(parsed.get("custom_headers"))?;

        let provider = detect_provider_with_auth(&base_url, &auth_type).to_string();

        let chat_tools_enabled = parsed
            .get("chat_tools_enabled")
            .and_then(|v| v.as_bool())
            // OpenAI-compatible tool calling is supported by all providers we
            // ship presets for; per-provider opt-out is still possible by
            // setting `chat_tools_enabled = false` in assistant.toml.
            .unwrap_or(true);

        let native_web_search = parsed
            .get("native_web_search")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let web_search_provider = parsed
            .get("web_search_provider")
            .and_then(|v| v.as_str())
            .filter(|s| matches!(*s, "brave" | "pipellm" | "tavily"))
            .map(String::from);

        let web_search_api_key = parsed
            .get("web_search_api_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let web_fetch_script = parsed
            .get("web_fetch_script")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| expand_tilde(s));

        let fast_model = (simple_model != chat_model).then_some(simple_model);

        let memory_curator_model = parsed
            .get("memory_curator_model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        Ok(Self {
            api_key,
            chat_model,
            chat_model_choices,
            base_url,
            custom_headers,
            provider,
            api_mode,
            auth_type,
            chat_tools_enabled,
            native_web_search,
            web_search_provider,
            web_search_api_key,
            web_fetch_script,
            fast_model,
            memory_curator_model,
        })
    }

    /// Returns true when the third-party web_search function is configured and
    /// native Responses web search is not taking its place.
    pub fn web_search_ready(&self) -> bool {
        !self.native_web_search_ready()
            && self.web_search_provider.is_some()
            && self.web_search_api_key.is_some()
    }

    pub fn native_web_search_ready(&self) -> bool {
        self.api_mode == ApiMode::Responses && self.native_web_search && self.chat_tools_enabled
    }

    /// Codex authentication always uses the Responses transport regardless of
    /// the compatibility value stored in `api_mode`.
    pub fn effective_api_mode(&self) -> ApiMode {
        if self.auth_type == "codex" {
            ApiMode::Responses
        } else {
            self.api_mode
        }
    }
}

fn parse_custom_headers(value: Option<&toml::Value>) -> Result<Vec<(String, String)>> {
    let raw_headers: Vec<String> = match value {
        Some(toml::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
        Some(toml::Value::String(raw)) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
        Some(_) | None => Vec::new(),
    };

    let mut headers = Vec::new();
    for raw in raw_headers {
        let (name, value) = raw
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid custom_headers entry `{raw}`"))?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            anyhow::bail!("invalid custom_headers entry `{raw}`");
        }
        if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("content-type") {
            anyhow::bail!("custom_headers cannot override `{name}`");
        }
        HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid custom header name `{name}`"))?;
        HeaderValue::from_str(value)
            .with_context(|| format!("invalid custom header value for `{name}`"))?;
        headers.push((name.to_string(), value.to_string()));
    }
    Ok(headers)
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn assistant_toml_path() -> Result<PathBuf> {
    let user_config_path = config::user_config_path();
    let config_dir = user_config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid user config path"))?;
    Ok(config_dir.join("assistant.toml"))
}

// ─── Message types ────────────────────────────────────────────────────────────

/// A single message in API format. Stored as a raw JSON value so it can represent
/// any role (system, user, assistant, tool) including tool_calls and tool results.
#[derive(Clone)]
pub struct ApiMessage(pub serde_json::Value);

impl ApiMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "system", "content": content.into() }))
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "user", "content": content.into() }))
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "assistant", "content": content.into() }))
    }
    pub fn assistant_with_reasoning(
        content: impl Into<String>,
        reasoning_content: impl AsRef<str>,
    ) -> Self {
        let mut msg = serde_json::json!({ "role": "assistant", "content": content.into() });
        let reasoning = reasoning_content.as_ref();
        if !reasoning.is_empty() {
            msg["reasoning_content"] = serde_json::Value::String(reasoning.to_string());
        }
        Self(msg)
    }
    /// Assistant turn that requested tool calls (content is null per the OpenAI spec).
    pub fn assistant_tool_calls(tool_calls: serde_json::Value) -> Self {
        Self(serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": tool_calls
        }))
    }
    /// Tool result message returned after executing a function call.
    /// Includes the tool name so non-OpenAI providers (for example Gemini)
    /// can map responses back to the corresponding function declaration.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id.into(),
            "name": name.into(),
            "content": content.into()
        }))
    }

    /// A raw output item returned by the Responses API. Reasoning models can
    /// require encrypted reasoning items to be replayed unchanged before tool
    /// outputs on the next step, so reducing these to chat-completion messages
    /// loses protocol state.
    pub fn responses_output_item(item: serde_json::Value) -> Self {
        Self(serde_json::json!({ "kaku_responses_output_item": item }))
    }

    /// Approximate serialized byte size of this message. Used for history-budget
    /// accounting in the agent loop; does not need to be exact.
    pub fn byte_len(&self) -> usize {
        serde_json::to_vec(&self.0).map(|v| v.len()).unwrap_or(0)
    }
}

pub fn should_roundtrip_reasoning_content(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.contains("deepseek")
        || model.contains("kimi")
        || model.contains("mimo")
        || model.contains("glm")
}

// ─── Tool calling ─────────────────────────────────────────────────────────────

/// A fully assembled tool call returned by the model after streaming is complete.
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Complete JSON-encoded arguments string, e.g. `{"path": "~/Downloads"}`.
    pub arguments: String,
}

/// Result of one model step. `response_items` is empty for Chat Completions;
/// Responses callers must replay these raw items before function-call outputs.
pub struct ChatStepResult {
    pub tool_calls: Vec<ToolCall>,
    pub response_items: Vec<serde_json::Value>,
}

impl ChatStepResult {
    fn empty() -> Self {
        Self {
            tool_calls: Vec::new(),
            response_items: Vec::new(),
        }
    }

    fn chat_completions(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls,
            response_items: Vec::new(),
        }
    }
}

// ─── Client ───────────────────────────────────────────────────────────────────

/// Synchronous AI client for use inside overlay threads.
/// Clone is cheap: reqwest::blocking::Client is Arc-backed internally.
#[derive(Clone)]
pub struct AiClient {
    config: AssistantConfig,
    client: reqwest::blocking::Client,
    codex_discovered_model: Arc<OnceLock<String>>,
    max_request_attempts: u32,
}

/// Build a blocking reqwest client that respects the user's system proxy.
///
/// Reqwest already honors standard proxy env vars; this helper additionally
/// falls back to `scutil --proxy` on macOS so launches from the menu bar or
/// Finder, which inherit launchd's empty environment, still go through the
/// user's configured proxy. Without this fallback such launches silently
/// bypass the proxy, the same hazard already fixed in the curl-based
/// update path.
///
/// `timeout` controls the per-request ceiling; AI chat needs minutes for
/// long streaming completions while web tools should fail fast.
pub(crate) fn build_client_with_proxy(timeout: std::time::Duration) -> reqwest::blocking::Client {
    build_client_with_proxy_redirects(timeout, true)
}

fn build_client_with_proxy_redirects(
    timeout: std::time::Duration,
    follow_redirects: bool,
) -> reqwest::blocking::Client {
    build_client_with_proxy_options(timeout, None, follow_redirects)
}

fn build_client_with_proxy_options(
    timeout: std::time::Duration,
    read_timeout: Option<std::time::Duration>,
    follow_redirects: bool,
) -> reqwest::blocking::Client {
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(timeout);
    if let Some(read_timeout) = read_timeout {
        builder = builder.timeout(read_timeout);
    }
    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }

    if let Some(proxy_url) = config::proxy::detect_system_proxy() {
        match reqwest::Proxy::all(&proxy_url) {
            Ok(proxy) => {
                // Bypass the proxy for loopback/LAN/CGNAT ranges so a
                // self-hosted model server stays reachable. Critical because
                // reqwest is built without the `socks` feature, so forcing a
                // SOCKS proxy onto an internal `base_url` fails the request
                // outright ("error sending request for url").
                let proxy = proxy.no_proxy(build_no_proxy());
                log::info!(
                    "HTTP client using system proxy: {} (private-range bypass enabled)",
                    proxy_url
                );
                builder = builder.proxy(proxy);
            }
            Err(e) => log::warn!(
                "Failed to apply detected system proxy {}: {}; continuing without proxy",
                proxy_url,
                e
            ),
        }
    }

    builder.build().unwrap_or_else(|e| {
        log::warn!("Failed to build HTTP client: {e}; falling back to default client");
        let mut fallback = reqwest::blocking::Client::builder();
        if let Some(read_timeout) = read_timeout {
            fallback = fallback.timeout(read_timeout);
        }
        let fallback = if follow_redirects {
            fallback
        } else {
            fallback.redirect(reqwest::redirect::Policy::none())
        };
        fallback
            .build()
            .expect("default reqwest client configuration must be valid")
    })
}

/// Hosts and ranges that must bypass any system proxy and connect directly.
///
/// A user with a global SOCKS/HTTP proxy still needs to reach a self-hosted
/// model server on loopback, their LAN, or a CGNAT/Tailscale address. The list
/// combines hard-coded private/loopback ranges, the `NO_PROXY` environment
/// variable, and the macOS `scutil` ExceptionsList.
fn build_no_proxy() -> Option<reqwest::NoProxy> {
    reqwest::NoProxy::from_string(&build_no_proxy_list().join(","))
}

fn build_no_proxy_list() -> Vec<String> {
    let mut entries: Vec<String> = [
        "localhost",
        "127.0.0.0/8",
        "::1",
        "169.254.0.0/16",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "100.64.0.0/10",
        ".local",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    for var in ["NO_PROXY", "no_proxy"] {
        if let Ok(v) = std::env::var(var) {
            entries.extend(
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            );
        }
    }

    entries.extend(config::proxy::system_proxy_exceptions());

    entries
}

/// Process-level HTTP client shared across all overlay sessions.
///
/// TLS stack is initialized once; subsequent `AiClient::new` calls are free.
fn shared_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_client_with_proxy(std::time::Duration::from_secs(600)))
}

impl AiClient {
    pub fn new(config: AssistantConfig) -> Self {
        Self {
            config,
            client: shared_http_client().clone(),
            codex_discovered_model: Arc::new(OnceLock::new()),
            max_request_attempts: 3,
        }
    }

    /// Build a provider-aware one-shot client with a caller-specific timeout.
    ///
    /// Inline shell requests already have a UI-level timeout. Keep them to one
    /// normal API transport attempt so retry layers cannot multiply that budget.
    pub fn new_with_timeout(config: AssistantConfig, timeout: std::time::Duration) -> Self {
        Self {
            config,
            client: build_client_with_proxy(timeout),
            codex_discovered_model: Arc::new(OnceLock::new()),
            max_request_attempts: 1,
        }
    }

    /// Whether this client will include tools in chat requests.
    pub fn tools_enabled(&self) -> bool {
        self.config.chat_tools_enabled
    }

    /// Returns a reference to the loaded assistant configuration.
    pub fn config(&self) -> &AssistantConfig {
        &self.config
    }

    /// Single-shot (non-streaming) completion for short tasks like title generation.
    ///
    /// Internally uses `chat_step` with an empty tools list and accumulates all tokens
    /// into a String. The returned text is trimmed of leading/trailing whitespace.
    pub fn complete_once(&self, model: &str, messages: &[ApiMessage]) -> Result<String> {
        let cancelled = AtomicBool::new(false);
        let mut text = String::new();
        self.chat_step(
            model,
            messages,
            &[],
            false,
            &cancelled,
            &mut |tok| {
                text.push_str(tok);
            },
            &mut |_| {},
        )?;
        Ok(text.trim().to_string())
    }

    /// Fetch available chat models from `{base_url}/models`.
    /// Filters out non-chat models (embeddings, TTS, image, etc.).
    pub fn list_models(&self) -> Result<Vec<String>> {
        if self.config.auth_type == "codex" {
            let connection = codex_connection::load_codex_connection()
                .context("resolve user Codex connection for model discovery")?;
            return self.list_codex_models(&connection);
        }
        let url = format!("{}/models", self.config.base_url);
        let req = self.client.get(&url);
        let req = self.apply_auth_headers(req)?;
        let resp = req.send().context("GET /models failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = read_error_response_preview(resp, MAX_ERROR_BODY_BYTES);
            anyhow::bail!("models API {}: {}", status, body);
        }
        parse_models_response(resp, "models API", true)
    }

    fn list_codex_models(&self, connection: &CodexConnection) -> Result<Vec<String>> {
        let endpoint = connection.models_endpoint();
        let mut credential = connection.credential.clone();
        let mut provider_headers = HeaderMap::new();
        for (name, value) in &connection.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid Codex provider header name `{name}`"))?;
            let value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid Codex provider header value for `{name}`"))?;
            provider_headers.insert(name, value);
        }

        let build = |credential: &CodexCredential| {
            let mut req = self
                .client
                .get(&endpoint)
                .query(&connection.query_params)
                .header("Accept", "application/json")
                .header("User-Agent", "codex_cli_rs")
                .headers(provider_headers.clone());
            match credential {
                CodexCredential::ChatGpt(auth) => {
                    req = req
                        .header("Authorization", format!("Bearer {}", auth.access_token))
                        .header("OpenAI-Beta", "responses=experimental")
                        .header("originator", "codex_cli_rs");
                    if let Some(account_id) = auth.account_id.as_deref() {
                        req = req.header("chatgpt-account-id", account_id);
                    }
                }
                CodexCredential::Bearer(token) => {
                    req = req.header("Authorization", format!("Bearer {token}"));
                }
                CodexCredential::None => {}
            }
            req
        };

        let cancelled = AtomicBool::new(false);
        let response = self.send_codex_request_with_retry(
            &mut credential,
            &self.client,
            build,
            "Codex model discovery",
            &cancelled,
            connection.request_max_attempts,
        )?;
        let models = parse_models_response(response, "Codex model discovery", false)?;
        if models.is_empty() {
            anyhow::bail!("Codex model discovery returned no chat models");
        }
        Ok(models)
    }

    fn resolve_codex_request_model(
        &self,
        connection: &CodexConnection,
        requested_model: &str,
    ) -> Result<String> {
        if requested_model != codex_connection::FOLLOW_CODEX_MODEL {
            return Ok(requested_model.to_string());
        }
        if let Some(configured) = connection
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
        {
            return Ok(configured.to_string());
        }
        if let Some(cached) = self.codex_discovered_model.get() {
            return Ok(cached.clone());
        }
        let discovered = self
            .list_codex_models(connection)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Codex provider returned no models"))?;
        let _ = self.codex_discovered_model.set(discovered.clone());
        Ok(discovered)
    }

    fn send_codex_request_with_retry<F>(
        &self,
        credential: &mut CodexCredential,
        http_client: &reqwest::blocking::Client,
        build: F,
        provider_label: &str,
        cancelled: &AtomicBool,
        max_attempts: u32,
    ) -> Result<reqwest::blocking::Response>
    where
        F: Fn(&CodexCredential) -> reqwest::blocking::RequestBuilder,
    {
        let max_attempts = max_attempts.max(1);
        let mut last_err = String::new();
        let mut refreshed_chatgpt = false;
        let mut attempt = 0;
        while attempt < max_attempts {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
                if cancelled.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelled during retry backoff");
                }
            }
            let response = match build(credential).send() {
                Ok(response) => response,
                Err(error) => {
                    last_err = error.without_url().to_string();
                    log::warn!(
                        "{} HTTP attempt {}: {}",
                        provider_label,
                        attempt + 1,
                        last_err
                    );
                    attempt += 1;
                    continue;
                }
            };
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                && matches!(credential, CodexCredential::ChatGpt(_))
                && !refreshed_chatgpt
            {
                log::debug!("{provider_label} ChatGPT token rejected; refreshing");
                *credential = CodexCredential::ChatGpt(ai_auth::refresh_codex_auth(http_client)?);
                refreshed_chatgpt = true;
                continue;
            }
            if status.is_success() {
                return Ok(response);
            }
            let code = status.as_u16();
            let body = read_error_response_preview(response, MAX_ERROR_BODY_BYTES);
            if code == 429 || code >= 500 {
                let preview: String = body.chars().take(200).collect();
                last_err = format!("{provider_label} error {code}: {preview}");
                log::warn!(
                    "{} HTTP attempt {} retryable: {}",
                    provider_label,
                    attempt + 1,
                    last_err
                );
                attempt += 1;
                continue;
            }
            anyhow::bail!("{provider_label} error {code}: {body}");
        }
        Err(anyhow::anyhow!(
            "{} request failed after {} attempts: {}",
            provider_label,
            max_attempts,
            last_err
        ))
    }

    /// Build provider-specific auth headers for the HTTP request builder.
    fn apply_auth_headers(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        let req = match self.config.auth_type.as_str() {
            "copilot" => {
                let token = ai_auth::get_copilot_token(&self.client)?;
                req.header("Authorization", format!("Bearer {token}"))
                    .header("Copilot-Integration-Id", "vscode-chat")
                    .header("Editor-Version", "vscode/1.110.1")
                    .header("Editor-Plugin-Version", "copilot-chat/0.38.2")
                    .header("Openai-Organization", "github-copilot")
                    .header("Openai-Intent", "conversation-panel")
            }
            "codex" => {
                anyhow::bail!(
                    "Codex following mode must use the resolved Codex Responses connection"
                )
            }
            _ => {
                if self.config.api_key.trim().is_empty() {
                    req
                } else {
                    req.header("Authorization", format!("Bearer {}", self.config.api_key))
                }
            }
        };
        self.apply_custom_headers(req)
    }

    fn apply_custom_headers(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        let mut headers = HeaderMap::new();
        for (name, value) in &self.config.custom_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid custom header name `{name}`"))?;
            let header_value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid custom header value for `{name}`"))?;
            headers.insert(header_name, header_value);
        }
        Ok(req.headers(headers))
    }

    /// Single chat step with optional tool support.
    ///
    /// Streams text tokens via `on_token`. If the model responds by requesting
    /// tool calls instead of (or before) text, returns those calls for the
    /// caller to execute and loop. Returns an empty vec when the step is text-only.
    ///
    /// The caller must set `cancelled` to `true` to abort mid-stream.
    #[allow(clippy::too_many_arguments)]
    pub fn chat_step(
        &self,
        model: &str,
        messages: &[ApiMessage],
        tools: &[serde_json::Value],
        allow_native_web_search: bool,
        cancelled: &AtomicBool,
        on_token: &mut dyn FnMut(&str),
        on_reasoning: &mut dyn FnMut(&str),
    ) -> Result<ChatStepResult> {
        // Codex following mode resolves the user's Codex Responses provider
        // rather than using assistant.toml's Chat Completions connection.
        if self.config.auth_type == "codex" {
            return self.chat_step_codex(model, messages, tools, cancelled, on_token, on_reasoning);
        }
        if self.config.effective_api_mode() == ApiMode::Responses {
            return self.chat_step_responses(
                model,
                messages,
                tools,
                allow_native_web_search,
                cancelled,
                on_token,
                on_reasoning,
            );
        }

        let url = format!("{}/chat/completions", self.config.base_url);

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
            "stream": true,
        });
        if !tools.is_empty() && self.config.chat_tools_enabled {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }

        let req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Accept-Encoding", "identity")
            .json(&body);
        let req = self.apply_auth_headers(req)?;

        let response = send_with_retry(req, "API", cancelled, self.max_request_attempts)?;

        let mut reader = BufReader::new(response);
        // Accumulate tool call fragments by index; each index is one pending call.
        // BTreeMap keeps indices sorted so we process them in order.
        let mut tc_buf: BTreeMap<usize, ToolCallBuf> = BTreeMap::new();
        let mut finish_reason = String::new();
        let mut think_filter = InlineThinkFilter::new();
        let mut stream_bytes = 0usize;
        let mut stream_events = 0usize;

        loop {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            let Some(mut line_bytes) = read_sse_line_capped(&mut reader, "API")? else {
                break;
            };
            add_stream_bytes(&mut stream_bytes, line_bytes.len(), "API")?;
            while matches!(line_bytes.last(), Some(b'\n' | b'\r')) {
                line_bytes.pop();
            }
            let line = std::str::from_utf8(&line_bytes).context("API SSE line was not UTF-8")?;
            let Some(data) = sse_data_payload(line) else {
                continue;
            };
            add_stream_event(&mut stream_events, "API")?;
            if data.trim() == "[DONE]" {
                break;
            }
            let chunk = match serde_json::from_str::<serde_json::Value>(data) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("Failed to parse SSE chunk: {e}");
                    continue;
                }
            };

            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };

            // Capture finish_reason when present.
            if let Some(fr) = choice["finish_reason"].as_str() {
                if !fr.is_empty() && fr != "null" {
                    finish_reason = fr.to_string();
                }
            }

            let delta = &choice["delta"];

            // Reasoning delta (DeepSeek et al. via dedicated field).
            if let Some(reasoning) = reasoning_delta_text(choice, delta) {
                if !reasoning.is_empty() {
                    on_reasoning(reasoning);
                }
            }
            // Text delta: filter inline <think> tags (Zhipu glm-5-turbo et al.
            // embed reasoning inside content rather than a dedicated field).
            if let Some(content) = delta["content"].as_str() {
                for seg in think_filter.feed(content) {
                    match seg {
                        ThinkSegment::Token(t) => on_token(&t),
                        ThinkSegment::Reasoning(r) => on_reasoning(&r),
                    }
                }
            }

            // Tool call deltas: accumulate arguments by index.
            if let Some(tc_arr) = delta["tool_calls"].as_array() {
                for tc in tc_arr {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    if !tc_buf.contains_key(&idx) && tc_buf.len() >= MAX_RESPONSE_TOOL_CALLS {
                        anyhow::bail!("API returned too many function calls");
                    }
                    let entry = tc_buf.entry(idx).or_default();
                    if let Some(id) = tc["id"].as_str() {
                        entry.id = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        entry.name = name.to_string();
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        append_tool_arguments(&mut entry.arguments, args, "API")?;
                    }
                }
            }
        }

        for seg in think_filter.flush() {
            match seg {
                ThinkSegment::Token(t) => on_token(&t),
                ThinkSegment::Reasoning(r) => on_reasoning(&r),
            }
        }

        // Build ToolCall results. Some proxies (e.g. vivgrid) never set
        // finish_reason to "tool_calls" even when streaming tool call deltas,
        // so fall back to any accumulated tc_buf entries with a valid name.
        if finish_reason == "tool_calls" || !tc_buf.is_empty() {
            let calls = tc_buf
                .into_values()
                .filter(|b| !b.name.is_empty())
                .map(|b| ToolCall {
                    id: b.id,
                    name: b.name,
                    arguments: b.arguments,
                })
                .collect::<Vec<_>>();
            if calls.is_empty() {
                Ok(ChatStepResult::empty())
            } else {
                Ok(ChatStepResult::chat_completions(calls))
            }
        } else {
            Ok(ChatStepResult::empty())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn chat_step_responses(
        &self,
        model: &str,
        messages: &[ApiMessage],
        tools: &[serde_json::Value],
        allow_native_web_search: bool,
        cancelled: &AtomicBool,
        on_token: &mut dyn FnMut(&str),
        on_reasoning: &mut dyn FnMut(&str),
    ) -> Result<ChatStepResult> {
        use serde_json::{json, Value};

        let url = format!("{}/responses", self.config.base_url);
        let (instructions, input) = translate_responses_messages(messages);
        let responses_tools = translate_responses_tools(
            tools,
            self.config.chat_tools_enabled,
            allow_native_web_search && self.config.native_web_search_ready(),
        );

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
            "store": false,
        });
        if !instructions.is_empty() {
            body["instructions"] = Value::String(instructions);
        }
        if !responses_tools.is_empty() {
            body["tools"] = Value::Array(responses_tools);
            body["tool_choice"] = Value::String("auto".to_string());
        }
        if supports_encrypted_reasoning_include(&self.config.base_url) {
            body["include"] = json!(["reasoning.encrypted_content"]);
        }
        let req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream, application/json")
            .header("Cache-Control", "no-cache")
            .header("Accept-Encoding", "identity")
            .json(&body);
        let req = self.apply_auth_headers(req)?;
        let response = send_with_retry(req, "Responses API", cancelled, self.max_request_attempts)?;

        parse_responses_http(response, cancelled, on_token, on_reasoning, "Responses API")
    }

    /// Codex-following chat step over the user-selected Responses provider.
    ///
    /// Translates chat-format messages and tools into the Responses request
    /// shape, streams text/reasoning, and assembles streamed `function_call`
    /// items back into `ToolCall`s for the agent loop to execute.
    fn chat_step_codex(
        &self,
        model: &str,
        messages: &[ApiMessage],
        tools: &[serde_json::Value],
        cancelled: &AtomicBool,
        on_token: &mut dyn FnMut(&str),
        on_reasoning: &mut dyn FnMut(&str),
    ) -> Result<ChatStepResult> {
        let connection =
            codex_connection::load_codex_connection().context("resolve user Codex connection")?;
        let resolved_model = self.resolve_codex_request_model(&connection, model)?;
        self.chat_step_codex_connection(
            &connection,
            &resolved_model,
            messages,
            tools,
            cancelled,
            on_token,
            on_reasoning,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn chat_step_codex_connection(
        &self,
        connection: &CodexConnection,
        model: &str,
        messages: &[ApiMessage],
        tools: &[serde_json::Value],
        cancelled: &AtomicBool,
        on_token: &mut dyn FnMut(&str),
        on_reasoning: &mut dyn FnMut(&str),
    ) -> Result<ChatStepResult> {
        use serde_json::{json, Value};

        let mut credential = connection.credential.clone();

        let (instructions, input) = translate_responses_messages(messages);
        let responses_tools =
            translate_responses_tools(tools, self.config.chat_tools_enabled, false);

        let mut reasoning = serde_json::Map::new();
        reasoning.insert(
            "effort".to_string(),
            Value::String(
                connection
                    .reasoning_effort
                    .clone()
                    .unwrap_or_else(|| "medium".to_string()),
            ),
        );
        reasoning.insert(
            "summary".to_string(),
            Value::String(
                connection
                    .reasoning_summary
                    .clone()
                    .unwrap_or_else(|| "auto".to_string()),
            ),
        );
        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
            "store": false,
            "reasoning": Value::Object(reasoning),
        });
        if !instructions.is_empty() {
            body["instructions"] = Value::String(instructions);
        }
        if !responses_tools.is_empty() {
            body["tools"] = Value::Array(responses_tools);
            body["tool_choice"] = Value::String("auto".to_string());
        }
        // Always ask for encrypted reasoning payloads, not only when tools
        // are enabled: reasoning models emit reasoning items regardless, the
        // stubs are persisted for replay, and under `store: false` a replayed
        // reasoning item without its encrypted content is rejected.
        body["include"] = json!(["reasoning.encrypted_content"]);

        let endpoint = connection.endpoint();
        let provider_client = connection.stream_idle_timeout_ms.map(|timeout_ms| {
            build_client_with_proxy_options(
                std::time::Duration::from_secs(600),
                Some(std::time::Duration::from_millis(timeout_ms.max(1))),
                true,
            )
        });
        let http_client = provider_client.as_ref().unwrap_or(&self.client);
        let mut provider_headers = HeaderMap::new();
        for (name, value) in &connection.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid Codex provider header name `{name}`"))?;
            let value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid Codex provider header value for `{name}`"))?;
            provider_headers.insert(name, value);
        }
        let build = |credential: &CodexCredential| -> reqwest::blocking::RequestBuilder {
            let mut req = http_client
                .post(&endpoint)
                .query(&connection.query_params)
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream, application/json")
                .header("Cache-Control", "no-cache")
                .header("Accept-Encoding", "identity")
                .header("User-Agent", "codex_cli_rs")
                .headers(provider_headers.clone())
                .json(&body);
            match credential {
                CodexCredential::ChatGpt(auth) => {
                    req = req
                        .header("Authorization", format!("Bearer {}", auth.access_token))
                        .header("OpenAI-Beta", "responses=experimental")
                        .header("originator", "codex_cli_rs");
                    if let Some(account_id) = auth.account_id.as_deref() {
                        req = req.header("chatgpt-account-id", account_id);
                    }
                }
                CodexCredential::Bearer(token) => {
                    req = req.header("Authorization", format!("Bearer {token}"));
                }
                CodexCredential::None => {}
            }
            req
        };

        for stream_attempt in 0..=connection.stream_max_retries {
            let response = self.send_codex_request_with_retry(
                &mut credential,
                http_client,
                build,
                "Codex provider",
                cancelled,
                connection.request_max_attempts,
            )?;

            let emitted = std::cell::Cell::new(false);
            let parsed = parse_responses_http(
                response,
                cancelled,
                &mut |token| {
                    emitted.set(true);
                    on_token(token);
                },
                &mut |reasoning| {
                    emitted.set(true);
                    on_reasoning(reasoning);
                },
                "Codex provider",
            );
            match parsed {
                Ok(result) => return Ok(result),
                Err(err) if !emitted.get() && stream_attempt < connection.stream_max_retries => {
                    log::warn!(
                        "Codex provider stream attempt {} failed before output: {}; retrying",
                        stream_attempt + 1,
                        err
                    );
                }
                Err(err) => return Err(err),
            }
        }
        unreachable!("Codex stream loop always returns")
    }
}

fn translate_responses_messages(messages: &[ApiMessage]) -> (String, Vec<serde_json::Value>) {
    use serde_json::json;

    let mut instructions = String::new();
    let mut input = Vec::new();
    for ApiMessage(message) in messages {
        if let Some(item) = message.get("kaku_responses_output_item") {
            // A reasoning stub without its encrypted payload cannot be
            // replayed under `store: false` (providers reject it). Skip the
            // stub, keep the rest of the transcript.
            if item["type"].as_str() == Some("reasoning")
                && item["encrypted_content"].as_str().is_none()
            {
                continue;
            }
            input.push(item.clone());
            continue;
        }
        let role = message["role"].as_str().unwrap_or("user");

        if role == "tool" {
            input.push(json!({
                "type": "function_call_output",
                "call_id": message["tool_call_id"].as_str().unwrap_or(""),
                "output": message["content"].as_str().unwrap_or(""),
            }));
            continue;
        }

        if role == "assistant" {
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                for call in tool_calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call["id"].as_str().unwrap_or(""),
                        "name": call["function"]["name"].as_str().unwrap_or(""),
                        "arguments": call["function"]["arguments"].as_str().unwrap_or("{}"),
                    }));
                }
                continue;
            }
        }

        let content = message["content"].as_str().unwrap_or("");
        if content.is_empty() {
            continue;
        }
        if role == "system" {
            if !instructions.is_empty() {
                instructions.push_str("\n\n");
            }
            instructions.push_str(content);
            continue;
        }
        let content_type = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        input.push(json!({
            "type": "message",
            "role": role,
            "content": [{ "type": content_type, "text": content }],
        }));
    }

    // Responses rejects empty input. One-shot helpers sometimes supply only a
    // system message, so promote that text to input instead of sending an
    // instructions-only request.
    if input.is_empty() && !instructions.is_empty() {
        input.push(json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": std::mem::take(&mut instructions) }],
        }));
    }

    (instructions, input)
}

fn translate_responses_tools(
    tools: &[serde_json::Value],
    tools_enabled: bool,
    native_web_search: bool,
) -> Vec<serde_json::Value> {
    use serde_json::{json, Value};

    if !tools_enabled {
        return Vec::new();
    }

    let mut translated = tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function")?;
            let mut translated = json!({
                "type": "function",
                "name": function.get("name")?,
                "description": function.get("description").cloned().unwrap_or(Value::Null),
                "parameters": function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            });
            if let Some(strict) = function.get("strict").and_then(Value::as_bool) {
                translated["strict"] = Value::Bool(strict);
            }
            Some(translated)
        })
        .collect::<Vec<_>>();

    if native_web_search {
        translated.push(json!({ "type": "web_search" }));
    }
    translated
}

fn parse_responses_http(
    response: reqwest::blocking::Response,
    cancelled: &AtomicBool,
    on_token: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
    provider_label: &str,
) -> Result<ChatStepResult> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    if content_type
        .as_deref()
        .is_some_and(content_type_is_event_stream)
    {
        return parse_responses_sse(
            BufReader::new(response),
            cancelled,
            on_token,
            on_reasoning,
            provider_label,
        );
    }

    let bytes = read_response_body_capped(response, provider_label)?;
    let looks_json = content_type.as_deref().is_some_and(content_type_is_json)
        || bytes
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| matches!(byte, b'{' | b'['));
    if looks_json {
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)
            .with_context(|| format!("parse {provider_label} JSON response"))?;
        return parse_responses_value(&value, on_token, on_reasoning, provider_label);
    }

    parse_responses_sse(
        std::io::Cursor::new(bytes),
        cancelled,
        on_token,
        on_reasoning,
        provider_label,
    )
}

fn content_type_is_json(value: &str) -> bool {
    let media_type = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
}

fn content_type_is_event_stream(value: &str) -> bool {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("text/event-stream")
}

fn read_response_body_capped(
    response: reqwest::blocking::Response,
    provider_label: &str,
) -> Result<Vec<u8>> {
    read_body_capped(response, MAX_RESPONSE_BODY_BYTES, provider_label)
}

fn read_body_capped(reader: impl Read, max_bytes: usize, provider_label: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {provider_label} response body"))?;
    if bytes.len() > max_bytes {
        anyhow::bail!("{provider_label} response exceeded {} bytes", max_bytes);
    }
    Ok(bytes)
}

fn read_error_response_preview(response: reqwest::blocking::Response, max_chars: usize) -> String {
    let mut bytes = Vec::new();
    let _ = response
        .take(MAX_ERROR_BODY_BYTES as u64)
        .read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes)
        .chars()
        .take(max_chars)
        .collect()
}

fn read_sse_line_capped<R: BufRead>(
    reader: &mut R,
    provider_label: &str,
) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .with_context(|| format!("read {provider_label} SSE line"))?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        let new_len = line
            .len()
            .checked_add(take)
            .ok_or_else(|| anyhow::anyhow!("{provider_label} SSE line length overflowed"))?;
        if new_len > MAX_RESPONSE_SSE_LINE_BYTES {
            anyhow::bail!(
                "{provider_label} SSE line exceeded {} bytes",
                MAX_RESPONSE_SSE_LINE_BYTES
            );
        }
        let found_newline = available.get(take.saturating_sub(1)) == Some(&b'\n');
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if found_newline {
            return Ok(Some(line));
        }
    }
}

fn add_stream_bytes(total: &mut usize, amount: usize, provider_label: &str) -> Result<()> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| anyhow::anyhow!("{provider_label} stream size overflowed"))?;
    if *total > MAX_RESPONSE_STREAM_BYTES {
        anyhow::bail!(
            "{provider_label} stream exceeded {} bytes",
            MAX_RESPONSE_STREAM_BYTES
        );
    }
    Ok(())
}

fn add_stream_event(total: &mut usize, provider_label: &str) -> Result<()> {
    *total += 1;
    if *total > MAX_RESPONSE_STREAM_EVENTS {
        anyhow::bail!(
            "{provider_label} stream exceeded {} events",
            MAX_RESPONSE_STREAM_EVENTS
        );
    }
    Ok(())
}

fn parse_responses_sse<R: BufRead>(
    mut reader: R,
    cancelled: &AtomicBool,
    on_token: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
    provider_label: &str,
) -> Result<ChatStepResult> {
    let mut calls: Vec<(String, ToolCallBuf)> = Vec::new();
    let mut citations = Vec::new();
    let mut saw_text_delta = false;
    let mut saw_reasoning_delta = false;
    let mut streamed_text = String::new();
    let mut completed_output_text = String::new();
    let mut response_items = Vec::new();
    let mut indexless_item_positions = std::collections::HashMap::new();
    let mut call_alias_positions = std::collections::HashMap::new();
    let mut call_output_positions = std::collections::HashMap::new();
    let mut completed = false;
    let mut stream_bytes = 0usize;
    let mut stream_events = 0usize;

    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(ChatStepResult::empty());
        }
        let Some(mut line_bytes) = read_sse_line_capped(&mut reader, provider_label)? else {
            break;
        };
        add_stream_bytes(&mut stream_bytes, line_bytes.len(), provider_label)?;
        while matches!(line_bytes.last(), Some(b'\n' | b'\r')) {
            line_bytes.pop();
        }
        let line = std::str::from_utf8(&line_bytes)
            .with_context(|| format!("{provider_label} SSE line was not UTF-8"))?;
        let Some(data) = sse_data_payload(line) else {
            continue;
        };
        add_stream_event(&mut stream_events, provider_label)?;
        if data.trim() == "[DONE]" {
            break;
        }
        let event = serde_json::from_str::<serde_json::Value>(data)
            .with_context(|| format!("parse {provider_label} SSE event"))?;

        match event["type"].as_str() {
            Some("response.output_text.delta") => {
                if let Some(delta) = event["delta"].as_str() {
                    saw_text_delta = true;
                    streamed_text.push_str(delta);
                    on_token(delta);
                }
            }
            Some("response.reasoning_summary_text.delta")
            | Some("response.reasoning_text.delta") => {
                if let Some(delta) = event["delta"].as_str() {
                    saw_reasoning_delta = true;
                    on_reasoning(delta);
                }
            }
            Some("response.output_text.annotation.added") => {
                collect_response_annotation(&event["annotation"], &mut citations);
            }
            Some("response.output_item.added") | Some("response.output_item.done") => {
                let item = &event["item"];
                upsert_response_item(
                    &mut response_items,
                    item,
                    event["output_index"].as_u64(),
                    &mut indexless_item_positions,
                )?;
                if item["type"] == "function_call" {
                    upsert_response_call(
                        &mut calls,
                        item,
                        event["output_index"].as_u64(),
                        &mut call_alias_positions,
                        &mut call_output_positions,
                    )?;
                } else if item["type"] == "message" {
                    collect_response_citations(item, &mut citations);
                }
            }
            Some("response.function_call_arguments.delta") => {
                let item_id = event["item_id"].as_str().unwrap_or("");
                let updated_arguments = if let Some(buffer) = upsert_stream_call(
                    &mut calls,
                    item_id,
                    event["output_index"].as_u64(),
                    &mut call_alias_positions,
                    &mut call_output_positions,
                )? {
                    if let Some(delta) = event["delta"].as_str() {
                        append_tool_arguments(&mut buffer.arguments, delta, provider_label)?;
                    }
                    Some(buffer.arguments.clone())
                } else {
                    None
                };
                if let Some(arguments) = updated_arguments {
                    update_response_item_arguments(&mut response_items, item_id, &arguments);
                }
            }
            Some("response.function_call_arguments.done") => {
                let item_id = event["item_id"].as_str().unwrap_or("");
                let updated_arguments = if let Some(buffer) = upsert_stream_call(
                    &mut calls,
                    item_id,
                    event["output_index"].as_u64(),
                    &mut call_alias_positions,
                    &mut call_output_positions,
                )? {
                    if let Some(arguments) = event["arguments"].as_str() {
                        set_tool_arguments(&mut buffer.arguments, arguments, provider_label)?;
                    }
                    Some(buffer.arguments.clone())
                } else {
                    None
                };
                if let Some(arguments) = updated_arguments {
                    update_response_item_arguments(&mut response_items, item_id, &arguments);
                }
            }
            Some("response.completed") => {
                let completed_response = &event["response"];
                validate_completed_response(completed_response, provider_label)?;
                completed_output_text = completed_response["output_text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if let Some(output) = completed_response["output"].as_array() {
                    validate_response_item_count(output.len(), provider_label)?;
                    if !output.is_empty() {
                        response_items = output.clone();
                        let completed_calls = response_calls(completed_response, provider_label)?;
                        if !completed_calls.is_empty() {
                            calls = completed_calls;
                        }
                    }
                }
                emit_response_output(
                    completed_response,
                    !saw_text_delta,
                    !saw_reasoning_delta,
                    on_token,
                    on_reasoning,
                );
                collect_response_citations(completed_response, &mut citations);
                completed = true;
                break;
            }
            Some("response.failed") | Some("response.incomplete") => {
                let message = response_error_message(&event)
                    .or_else(|| response_error_message(&event["response"]))
                    .unwrap_or("unknown error");
                anyhow::bail!("{provider_label} failed: {message}");
            }
            Some("error") => {
                let message = response_error_message(&event).unwrap_or("unknown error");
                anyhow::bail!("{provider_label} failed: {message}");
            }
            _ => {}
        }
    }

    if cancelled.load(Ordering::Relaxed) {
        return Ok(ChatStepResult::empty());
    }
    if !completed {
        anyhow::bail!("{provider_label} stream ended before response.completed");
    }
    validate_response_call_buffers(&calls, provider_label)?;
    sync_response_call_items(&mut response_items, &calls, provider_label)?;
    let citations_text = format_response_citations(&citations);
    if !citations_text.is_empty() {
        on_token(&citations_text);
    }
    if !response_items.iter().any(response_message_has_text) {
        let mut final_text = if streamed_text.is_empty() {
            completed_output_text
        } else {
            streamed_text
        };
        final_text.push_str(&citations_text);
        if !final_text.is_empty() {
            upsert_synthesized_response_message(&mut response_items, final_text, provider_label)?;
        }
    }
    validate_response_item_count(response_items.len(), provider_label)?;
    Ok(ChatStepResult {
        tool_calls: tool_calls_from_buffers(calls),
        response_items,
    })
}

fn parse_responses_value(
    response: &serde_json::Value,
    on_token: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
    provider_label: &str,
) -> Result<ChatStepResult> {
    validate_completed_response(response, provider_label)?;

    let mut citations = Vec::new();
    let mut output = response["output"].as_array().cloned().unwrap_or_default();
    validate_response_item_count(output.len(), provider_label)?;
    emit_response_output(response, true, true, on_token, on_reasoning);
    collect_response_citations(response, &mut citations);

    let citations_text = format_response_citations(&citations);
    if !citations_text.is_empty() {
        on_token(&citations_text);
    }
    if !output.iter().any(response_message_has_text) {
        let mut text = response["output_text"].as_str().unwrap_or("").to_string();
        text.push_str(&citations_text);
        if !text.is_empty() {
            upsert_synthesized_response_message(&mut output, text, provider_label)?;
        }
    }
    Ok(ChatStepResult {
        tool_calls: tool_calls_from_buffers(response_calls(response, provider_label)?),
        response_items: output,
    })
}

fn validate_completed_response(response: &serde_json::Value, provider_label: &str) -> Result<()> {
    match response["status"].as_str() {
        Some("completed") => Ok(()),
        Some("failed" | "incomplete") => {
            let message = response_error_message(response).unwrap_or("unknown error");
            anyhow::bail!("{provider_label} failed: {message}")
        }
        Some(status) => anyhow::bail!("{provider_label} returned unexpected status `{status}`"),
        None => anyhow::bail!("{provider_label} response omitted completion status"),
    }
}

fn emit_response_output(
    response: &serde_json::Value,
    emit_text: bool,
    emit_reasoning: bool,
    on_token: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
) {
    let mut emitted_text = false;
    if let Some(output) = response["output"].as_array() {
        for item in output {
            match item["type"].as_str() {
                Some("message") if emit_text => {
                    if let Some(content) = item["content"].as_array() {
                        for part in content {
                            if let Some(text) =
                                part["text"].as_str().or_else(|| part["refusal"].as_str())
                            {
                                emitted_text = true;
                                on_token(text);
                            }
                        }
                    }
                }
                Some("reasoning") if emit_reasoning => {
                    if let Some(summary) = item["summary"].as_array() {
                        for part in summary {
                            if let Some(text) = part["text"].as_str() {
                                on_reasoning(text);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if emit_text && !emitted_text {
        if let Some(text) = response["output_text"].as_str() {
            on_token(text);
        }
    }
}

fn response_calls(
    response: &serde_json::Value,
    provider_label: &str,
) -> Result<Vec<(String, ToolCallBuf)>> {
    let mut calls = Vec::new();
    let mut aliases = std::collections::HashMap::new();
    let mut output_positions = std::collections::HashMap::new();
    if let Some(output) = response["output"].as_array() {
        for (output_index, item) in output.iter().enumerate() {
            if item["type"] == "function_call" {
                upsert_response_call(
                    &mut calls,
                    item,
                    Some(output_index as u64),
                    &mut aliases,
                    &mut output_positions,
                )?;
            }
        }
    }
    validate_response_call_buffers(&calls, provider_label)?;
    Ok(calls)
}

fn upsert_response_call(
    calls: &mut Vec<(String, ToolCallBuf)>,
    item: &serde_json::Value,
    output_index: Option<u64>,
    aliases: &mut std::collections::HashMap<String, usize>,
    output_positions: &mut std::collections::HashMap<u64, usize>,
) -> Result<()> {
    let item_id = item["id"].as_str().filter(|value| !value.is_empty());
    let call_id = item["call_id"].as_str().filter(|value| !value.is_empty());
    let mut aliases_for_item = Vec::with_capacity(2);
    if let Some(item_id) = item_id {
        aliases_for_item.push(item_id);
    }
    if let Some(call_id) = call_id {
        aliases_for_item.push(call_id);
    }
    if let Some(buffer) = upsert_call_identity(
        calls,
        &aliases_for_item,
        output_index,
        aliases,
        output_positions,
    )? {
        if let Some(call_id) = item["call_id"].as_str().filter(|value| !value.is_empty()) {
            buffer.id = call_id.to_string();
        }
        if let Some(name) = item["name"].as_str().filter(|value| !value.is_empty()) {
            buffer.name = name.to_string();
        }
        if let Some(arguments) = item["arguments"].as_str().filter(|value| !value.is_empty()) {
            set_tool_arguments(&mut buffer.arguments, arguments, "Responses API")?;
        }
    }
    Ok(())
}

fn upsert_stream_call<'a>(
    calls: &'a mut Vec<(String, ToolCallBuf)>,
    item_id: &str,
    output_index: Option<u64>,
    aliases: &mut std::collections::HashMap<String, usize>,
    output_positions: &mut std::collections::HashMap<u64, usize>,
) -> Result<Option<&'a mut ToolCallBuf>> {
    let identities = (!item_id.is_empty())
        .then_some(item_id)
        .into_iter()
        .collect::<Vec<_>>();
    upsert_call_identity(calls, &identities, output_index, aliases, output_positions)
}

fn upsert_call_identity<'a>(
    calls: &'a mut Vec<(String, ToolCallBuf)>,
    identities: &[&str],
    output_index: Option<u64>,
    aliases: &mut std::collections::HashMap<String, usize>,
    output_positions: &mut std::collections::HashMap<u64, usize>,
) -> Result<Option<&'a mut ToolCallBuf>> {
    let position = identities
        .iter()
        .find_map(|identity| aliases.get(*identity).copied())
        .or_else(|| output_index.and_then(|index| output_positions.get(&index).copied()));

    let position = match position {
        Some(position) => position,
        None => {
            if identities.is_empty() && output_index.is_none() {
                return Ok(None);
            }
            if calls.len() >= MAX_RESPONSE_TOOL_CALLS {
                anyhow::bail!("Responses API returned too many function calls");
            }
            let storage_id = identities
                .first()
                .map(|identity| (*identity).to_string())
                .unwrap_or_else(|| format!("output_index:{}", output_index.unwrap_or_default()));
            calls.push((storage_id, ToolCallBuf::default()));
            calls.len() - 1
        }
    };

    for identity in identities {
        aliases.insert((*identity).to_string(), position);
    }
    if let Some(index) = output_index {
        output_positions.insert(index, position);
    }
    Ok(Some(&mut calls[position].1))
}

fn upsert_response_item(
    items: &mut Vec<serde_json::Value>,
    item: &serde_json::Value,
    output_index: Option<u64>,
    indexless_positions: &mut std::collections::HashMap<u64, usize>,
) -> Result<()> {
    let id = item["id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| item["call_id"].as_str().filter(|value| !value.is_empty()));
    if let Some(id) = id {
        if let Some(position) = items.iter().position(|existing| {
            existing["id"].as_str() == Some(id) || existing["call_id"].as_str() == Some(id)
        }) {
            items[position] = item.clone();
            if let Some(index) = output_index {
                indexless_positions.insert(index, position);
            }
            return Ok(());
        }
    }
    // Custom `/responses` endpoints may omit item ids on either the `.added`
    // or the `.done` event. `output_index` is stable across both, so track it
    // for every stored item; whichever identity the later event carries, it
    // replaces instead of duplicating (positions are stable: parsing only
    // appends or replaces in place).
    if let Some(index) = output_index {
        if let Some(&position) = indexless_positions.get(&index) {
            items[position] = item.clone();
            return Ok(());
        }
    }
    validate_response_item_count(items.len() + 1, "Responses API")?;
    if let Some(index) = output_index {
        indexless_positions.insert(index, items.len());
    }
    items.push(item.clone());
    Ok(())
}

fn update_response_item_arguments(items: &mut [serde_json::Value], item_id: &str, arguments: &str) {
    if item_id.is_empty() {
        return;
    }
    if let Some(item) = items.iter_mut().find(|item| {
        item["id"].as_str() == Some(item_id) || item["call_id"].as_str() == Some(item_id)
    }) {
        item["arguments"] = serde_json::Value::String(arguments.to_string());
    }
}

fn sync_response_call_items(
    items: &mut Vec<serde_json::Value>,
    calls: &[(String, ToolCallBuf)],
    provider_label: &str,
) -> Result<()> {
    for (item_id, call) in calls {
        let existing = items.iter_mut().find(|item| {
            (!item_id.is_empty() && item["id"].as_str() == Some(item_id.as_str()))
                || (!call.id.is_empty() && item["call_id"].as_str() == Some(call.id.as_str()))
        });
        if let Some(item) = existing {
            item["arguments"] = serde_json::Value::String(call.arguments.clone());
            continue;
        }

        validate_response_item_count(items.len() + 1, provider_label)?;
        let mut item = serde_json::json!({
            "type": "function_call",
            "call_id": call.id,
            "name": call.name,
            "arguments": call.arguments,
        });
        if !item_id.is_empty() {
            item["id"] = serde_json::Value::String(item_id.clone());
        }
        items.push(item);
    }
    Ok(())
}

fn validate_response_item_count(count: usize, provider_label: &str) -> Result<()> {
    if count > MAX_RESPONSE_OUTPUT_ITEMS {
        anyhow::bail!(
            "{provider_label} returned more than {} output items",
            MAX_RESPONSE_OUTPUT_ITEMS
        );
    }
    Ok(())
}

fn append_tool_arguments(target: &mut String, delta: &str, provider_label: &str) -> Result<()> {
    let new_len = target
        .len()
        .checked_add(delta.len())
        .ok_or_else(|| anyhow::anyhow!("{provider_label} tool arguments overflowed"))?;
    if new_len > MAX_RESPONSE_TOOL_ARGUMENT_BYTES {
        anyhow::bail!(
            "{provider_label} tool arguments exceeded {} bytes",
            MAX_RESPONSE_TOOL_ARGUMENT_BYTES
        );
    }
    target.push_str(delta);
    Ok(())
}

fn set_tool_arguments(target: &mut String, arguments: &str, provider_label: &str) -> Result<()> {
    if arguments.len() > MAX_RESPONSE_TOOL_ARGUMENT_BYTES {
        anyhow::bail!(
            "{provider_label} tool arguments exceeded {} bytes",
            MAX_RESPONSE_TOOL_ARGUMENT_BYTES
        );
    }
    target.clear();
    target.push_str(arguments);
    Ok(())
}

fn validate_response_call_buffers(
    calls: &[(String, ToolCallBuf)],
    provider_label: &str,
) -> Result<()> {
    if calls.len() > MAX_RESPONSE_TOOL_CALLS {
        anyhow::bail!(
            "{provider_label} returned more than {} function calls",
            MAX_RESPONSE_TOOL_CALLS
        );
    }
    for (_, call) in calls {
        if call.arguments.len() > MAX_RESPONSE_TOOL_ARGUMENT_BYTES {
            anyhow::bail!(
                "{provider_label} tool arguments exceeded {} bytes",
                MAX_RESPONSE_TOOL_ARGUMENT_BYTES
            );
        }
    }
    Ok(())
}

fn tool_calls_from_buffers(calls: Vec<(String, ToolCallBuf)>) -> Vec<ToolCall> {
    calls
        .into_iter()
        .map(|(_, buffer)| buffer)
        .filter(|buffer| !buffer.name.is_empty())
        .map(|buffer| ToolCall {
            id: buffer.id,
            name: buffer.name,
            arguments: buffer.arguments,
        })
        .collect()
}

fn response_error_message(value: &serde_json::Value) -> Option<&str> {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["message"].as_str())
        .or_else(|| value["incomplete_details"]["reason"].as_str())
}

fn collect_response_citations(value: &serde_json::Value, citations: &mut Vec<(String, String)>) {
    if let Some(content) = value["content"].as_array() {
        for part in content {
            if let Some(annotations) = part["annotations"].as_array() {
                for annotation in annotations {
                    collect_response_annotation(annotation, citations);
                }
            }
        }
    }
    if let Some(output) = value["output"].as_array() {
        for item in output {
            collect_response_citations(item, citations);
        }
    }
}

fn collect_response_annotation(
    annotation: &serde_json::Value,
    citations: &mut Vec<(String, String)>,
) {
    if annotation["type"] != "url_citation" {
        return;
    }
    if citations.len() >= MAX_RESPONSE_CITATIONS {
        return;
    }
    let Some(url) = annotation["url"].as_str().filter(|url| !url.is_empty()) else {
        return;
    };
    // Truncate before deduplicating so stored (truncated) URLs compare
    // against the same shape.
    let url = url
        .chars()
        .take(MAX_RESPONSE_CITATION_URL_CHARS)
        .collect::<String>();
    if citations.iter().any(|(_, existing)| existing == &url) {
        return;
    }
    let title = annotation["title"]
        .as_str()
        .filter(|title| !title.is_empty())
        .unwrap_or(&url)
        .replace(['\n', '\r'], " ")
        .chars()
        .take(MAX_RESPONSE_CITATION_TITLE_CHARS)
        .collect();
    citations.push((title, url));
}

fn format_response_citations(citations: &[(String, String)]) -> String {
    if citations.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nSources:\n");
    for (title, url) in citations {
        // Kaku's compact Markdown renderer intentionally drops link targets,
        // so keep the URL visible for copying and terminal link detection.
        out.push_str(&format!("- {title}: {url}\n"));
    }
    out
}

fn synthesized_response_message(text: String) -> serde_json::Value {
    serde_json::json!({
        "type": "message",
        "role": "assistant",
        "status": "completed",
        "content": [{
            "type": "output_text",
            "text": text,
            "annotations": [],
        }],
    })
}

fn response_message_has_text(item: &serde_json::Value) -> bool {
    item["type"] == "message"
        && item["content"].as_array().is_some_and(|content| {
            content.iter().any(|part| {
                part["text"]
                    .as_str()
                    .or_else(|| part["refusal"].as_str())
                    .is_some_and(|text| !text.is_empty())
            })
        })
}

fn upsert_synthesized_response_message(
    items: &mut Vec<serde_json::Value>,
    text: String,
    provider_label: &str,
) -> Result<()> {
    let message = synthesized_response_message(text);
    if let Some(existing) = items.iter_mut().find(|item| item["type"] == "message") {
        *existing = message;
    } else {
        validate_response_item_count(items.len() + 1, provider_label)?;
        items.push(message);
    }
    Ok(())
}

/// Send a request up to `max_attempts` times with exponential backoff on transient
/// failures (network errors, HTTP 429, HTTP 5xx). Non-retryable HTTP errors
/// (4xx other than 429) bail immediately so misconfiguration surfaces fast.
///
/// `provider_label` is folded into log lines and the final error message so a
/// user reading logs can tell which transport failed.
fn send_with_retry(
    req: reqwest::blocking::RequestBuilder,
    provider_label: &str,
    cancelled: &AtomicBool,
    max_attempts: u32,
) -> Result<reqwest::blocking::Response> {
    let mut last_err = String::new();
    let max_attempts = max_attempts.max(1);
    for attempt in 0..max_attempts {
        if attempt > 0 {
            let backoff = std::time::Duration::from_secs(1 << attempt);
            std::thread::sleep(backoff);
            if cancelled.load(Ordering::Relaxed) {
                anyhow::bail!("cancelled during retry backoff");
            }
        }
        let r = match req.try_clone().context("clone request")?.send() {
            Ok(r) => r,
            Err(e) => {
                // Provider URLs can contain secret query parameters. Keep the
                // transport error while removing the URL before logs/UI see it.
                last_err = e.without_url().to_string();
                log::warn!(
                    "{} HTTP attempt {}: {}",
                    provider_label,
                    attempt + 1,
                    last_err
                );
                continue;
            }
        };
        let status = r.status();
        if status.is_success() {
            return Ok(r);
        }
        let code = status.as_u16();
        let body = read_error_response_preview(r, MAX_ERROR_BODY_BYTES);
        if code == 429 || code >= 500 {
            let preview: String = body.chars().take(200).collect();
            last_err = format!("{} error {}: {}", provider_label, code, preview);
            log::warn!(
                "{} HTTP attempt {} retryable: {}",
                provider_label,
                attempt + 1,
                last_err
            );
            continue;
        }
        anyhow::bail!("{} error {}: {}", provider_label, code, body);
    }
    Err(anyhow::anyhow!(
        "{} request failed after {} attempts: {}",
        provider_label,
        max_attempts,
        last_err
    ))
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn parse_models_response(
    response: reqwest::blocking::Response,
    provider_label: &str,
    sort_models: bool,
) -> Result<Vec<String>> {
    let body = read_body_capped(response, MAX_MODELS_BODY_BYTES, provider_label)?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .with_context(|| format!("parse {provider_label} response"))?;
    parse_models_value(&value, provider_label, sort_models)
}

fn parse_models_value(
    value: &serde_json::Value,
    provider_label: &str,
    sort_models: bool,
) -> Result<Vec<String>> {
    let entries = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!("{provider_label} response has no `data` or `models` array")
        })?;
    let mut models: Vec<String> = entries
        .iter()
        .filter(|entry| {
            entry
                .get("visibility")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|visibility| visibility == "list")
        })
        .filter_map(|entry| {
            entry
                .as_str()
                .or_else(|| entry.get("id").and_then(serde_json::Value::as_str))
                .or_else(|| entry.get("slug").and_then(serde_json::Value::as_str))
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(String::from)
        })
        .filter(|model| kaku_ai_utils::is_chat_model_id(model))
        .collect();
    if sort_models {
        models.sort();
        models.dedup();
    } else {
        let mut seen = HashSet::new();
        models.retain(|model| seen.insert(model.clone()));
    }
    models.truncate(30);
    Ok(models)
}

/// Buffer for accumulating streamed tool call fragments.
#[derive(Default)]
struct ToolCallBuf {
    id: String,
    name: String,
    arguments: String,
}

fn reasoning_delta_text<'a>(
    choice: &'a serde_json::Value,
    delta: &'a serde_json::Value,
) -> Option<&'a str> {
    delta["reasoning_content"]
        .as_str()
        .or_else(|| delta["reasoning"].as_str())
        .or_else(|| delta["reasoning"]["content"].as_str())
        .or_else(|| delta["thinking"].as_str())
        .or_else(|| delta["thinking"]["content"].as_str())
        .or_else(|| choice["reasoning_content"].as_str())
        .or_else(|| choice["reasoning"].as_str())
        .or_else(|| choice["thinking"].as_str())
        .or_else(|| choice["thinking"]["content"].as_str())
        .or_else(|| choice["message"]["reasoning_content"].as_str())
        .or_else(|| choice["message"]["reasoning"].as_str())
}

fn sse_data_payload(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim_start)
}

// ─── Inline <think> / <thinking> tag filter ─────────────────────────────────

const THINK_TAG_NAMES: &[&str] = &["thinking", "think"];

enum ThinkSegment {
    Token(String),
    Reasoning(String),
}

struct InlineThinkFilter {
    inside_think: bool,
    tag_name: &'static str,
    pending: String,
}

impl InlineThinkFilter {
    fn new() -> Self {
        Self {
            inside_think: false,
            tag_name: "",
            pending: String::new(),
        }
    }

    fn find_open_tag(s: &str) -> Option<(usize, usize, &'static str)> {
        for (pos, _) in s.match_indices('<') {
            if let Some((end, name)) = parse_think_tag_at(s, pos, false, None) {
                return Some((pos, end, name));
            }
        }
        None
    }

    fn find_close_tag(s: &str, tag_name: &str) -> Option<(usize, usize)> {
        for (pos, _) in s.match_indices('<') {
            if let Some((end, _)) = parse_think_tag_at(s, pos, true, Some(tag_name)) {
                return Some((pos, end));
            }
        }
        None
    }

    fn safe_emit_len(pending: &str, closing: bool) -> usize {
        partial_think_tag_start(pending, closing).unwrap_or(pending.len())
    }

    fn feed(&mut self, chunk: &str) -> Vec<ThinkSegment> {
        self.pending.push_str(chunk);
        let mut out = Vec::new();
        loop {
            if self.inside_think {
                if let Some((pos, end)) = Self::find_close_tag(&self.pending, self.tag_name) {
                    let reasoning = &self.pending[..pos];
                    if !reasoning.is_empty() {
                        out.push(ThinkSegment::Reasoning(reasoning.to_string()));
                    }
                    self.pending = self.pending[end..].to_string();
                    self.inside_think = false;
                } else {
                    let safe = Self::safe_emit_len(&self.pending, true);
                    if safe > 0 {
                        out.push(ThinkSegment::Reasoning(self.pending[..safe].to_string()));
                        self.pending = self.pending[safe..].to_string();
                    }
                    break;
                }
            } else if let Some((pos, end, name)) = Self::find_open_tag(&self.pending) {
                let text = &self.pending[..pos];
                if !text.is_empty() {
                    out.push(ThinkSegment::Token(text.to_string()));
                }
                self.pending = self.pending[end..].to_string();
                self.tag_name = name;
                self.inside_think = true;
            } else {
                let safe = Self::safe_emit_len(&self.pending, false);
                if safe > 0 {
                    out.push(ThinkSegment::Token(self.pending[..safe].to_string()));
                    self.pending = self.pending[safe..].to_string();
                }
                break;
            }
        }
        out
    }

    fn flush(&mut self) -> Vec<ThinkSegment> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            if self.inside_think {
                out.push(ThinkSegment::Reasoning(text));
            } else {
                out.push(ThinkSegment::Token(text));
            }
        }
        out
    }
}

fn parse_think_tag_at(
    s: &str,
    start: usize,
    closing: bool,
    expected_name: Option<&str>,
) -> Option<(usize, &'static str)> {
    let bytes = s.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }

    let mut i = start + 1;
    i = skip_ascii_whitespace(bytes, i);
    if closing {
        if bytes.get(i) != Some(&b'/') {
            return None;
        }
        i += 1;
        i = skip_ascii_whitespace(bytes, i);
    } else if bytes.get(i) == Some(&b'/') {
        return None;
    }

    let (name, next) = parse_think_tag_name(bytes, i)?;
    if let Some(expected) = expected_name {
        if name != expected {
            return None;
        }
    }
    i = skip_ascii_whitespace(bytes, next);
    if bytes.get(i) != Some(&b'>') {
        return None;
    }
    Some((i + 1, name))
}

fn parse_think_tag_name(bytes: &[u8], start: usize) -> Option<(&'static str, usize)> {
    for name in THINK_TAG_NAMES {
        let raw = name.as_bytes();
        if bytes.len() < start + raw.len() {
            continue;
        }
        if bytes[start..start + raw.len()].eq_ignore_ascii_case(raw) {
            let next = start + raw.len();
            match bytes.get(next) {
                Some(b'>') | Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0c) => {
                    return Some((name, next));
                }
                _ => {}
            }
        }
    }
    None
}

fn partial_think_tag_start(s: &str, closing: bool) -> Option<usize> {
    let pos = s.rfind('<')?;
    let tail = &s[pos..];
    if tail.contains('>') {
        return None;
    }

    let bytes = tail.as_bytes();
    let mut i = 1;
    i = skip_ascii_whitespace(bytes, i);
    if closing {
        match bytes.get(i) {
            None => return Some(pos),
            Some(b'/') => {
                i += 1;
                i = skip_ascii_whitespace(bytes, i);
            }
            Some(c) if c.is_ascii_whitespace() => return Some(pos),
            _ => return None,
        }
    } else {
        match bytes.get(i) {
            None => return Some(pos),
            Some(b'/') => return None,
            Some(c) if c.is_ascii_whitespace() => return Some(pos),
            _ => {}
        }
    }

    let name = &tail[i..];
    if name.is_empty() || name.as_bytes().iter().all(|b| b.is_ascii_whitespace()) {
        return Some(pos);
    }

    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_whitespace());
    if name.len() != trimmed.len() {
        return THINK_TAG_NAMES
            .iter()
            .any(|tag| trimmed.eq_ignore_ascii_case(tag))
            .then_some(pos);
    }

    THINK_TAG_NAMES
        .iter()
        .any(|tag| {
            tag.as_bytes()
                .starts_with(&trimmed.to_ascii_lowercase().into_bytes())
        })
        .then_some(pos)
}

fn skip_ascii_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes.get(i), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0c)) {
        i += 1;
    }
    i
}

fn supports_encrypted_reasoning_include(base_url: &str) -> bool {
    url::Url::parse(base_url).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case("api.openai.com"))
    })
}

/// Maps (base_url, auth_type) to a display provider name.
///
/// Single source of truth for provider naming. The `kaku` binary used to
/// carry a parallel `#[allow(dead_code)]` table; that copy was removed in
/// V0.10.0 because it never matched the GUI version under maintenance.
fn detect_provider_with_auth(base_url: &str, auth_type: &str) -> &'static str {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    match (normalized.as_str(), auth_type) {
        ("https://api.githubcopilot.com", _) => "Copilot",
        ("https://api.openai.com/v1", "codex") => "Codex",
        ("https://api.orcarouter.ai/v1", _) => "OrcaRouter",
        _ => "Custom",
    }
}

// Delegated to kaku-ai-utils crate to avoid cross-binary drift.

#[cfg(test)]
mod tests {
    use super::{
        add_stream_bytes, add_stream_event, build_no_proxy_list, content_type_is_json,
        detect_provider_with_auth, parse_custom_headers, parse_models_value, parse_responses_sse,
        parse_responses_value, read_body_capped, read_sse_line_capped, reasoning_delta_text,
        should_roundtrip_reasoning_content, sse_data_payload, supports_encrypted_reasoning_include,
        translate_responses_messages, translate_responses_tools, AiClient, ApiMessage, ApiMode,
        AssistantConfig, InlineThinkFilter, ThinkSegment, DEFAULT_BASE_URL, MAX_MODELS_BODY_BYTES,
        MAX_RESPONSE_SSE_LINE_BYTES, MAX_RESPONSE_STREAM_BYTES, MAX_RESPONSE_STREAM_EVENTS,
        MAX_RESPONSE_TOOL_ARGUMENT_BYTES,
    };
    use crate::codex_connection::{CodexConnection, CodexCredential};
    use reqwest::header::{AUTHORIZATION, USER_AGENT};
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    fn collect_segments(segs: Vec<ThinkSegment>) -> (String, String) {
        let mut tokens = String::new();
        let mut reasoning = String::new();
        for seg in segs {
            match seg {
                ThinkSegment::Token(t) => tokens.push_str(&t),
                ThinkSegment::Reasoning(r) => reasoning.push_str(&r),
            }
        }
        (tokens, reasoning)
    }

    #[test]
    fn generic_models_parser_preserves_empty_success_response() {
        let models = parse_models_value(&serde_json::json!({ "data": [] }), "models API", true)
            .expect("empty model list remains a successful response");
        assert!(models.is_empty());
    }

    fn route_mock_sse_lines(lines: &[&str]) -> (String, String) {
        let mut think_filter = InlineThinkFilter::new();
        let mut tokens = String::new();
        let mut reasoning = String::new();

        for line in lines {
            let Some(data) = sse_data_payload(line) else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            // Mirror chat_step()'s production resilience: malformed JSON chunks
            // are skipped rather than panicking. Keeping the two paths in sync
            // means tests exercise the same parse error policy as live traffic.
            let chunk: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };
            let delta = &choice["delta"];

            if let Some(text) = reasoning_delta_text(choice, delta) {
                reasoning.push_str(text);
            }
            if let Some(content) = delta["content"].as_str() {
                let (visible, hidden) = collect_segments(think_filter.feed(content));
                tokens.push_str(&visible);
                reasoning.push_str(&hidden);
            }
        }

        let (visible, hidden) = collect_segments(think_filter.flush());
        tokens.push_str(&visible);
        reasoning.push_str(&hidden);
        (tokens, reasoning)
    }

    #[test]
    fn detects_copilot_and_codex_and_falls_back_to_custom() {
        assert_eq!(
            detect_provider_with_auth("https://api.githubcopilot.com", "copilot"),
            "Copilot"
        );
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1", "codex"),
            "Codex"
        );
        // Same OpenAI URL with the default api_key auth is treated as a generic
        // OpenAI-compatible endpoint, so we surface it as Custom.
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1", "api_key"),
            "Custom"
        );
        // The OrcaRouter gateway is a named OpenAI-compatible endpoint, so it is
        // surfaced as its own provider regardless of auth type.
        assert_eq!(
            detect_provider_with_auth("https://api.orcarouter.ai/v1", "api_key"),
            "OrcaRouter"
        );
        // Unknown / removed providers (Gemini was dropped in V0.10.0) fall
        // through to Custom rather than crashing detection.
        assert_eq!(
            detect_provider_with_auth("https://generativelanguage.googleapis.com", "gemini_key"),
            "Custom"
        );
        assert_eq!(detect_provider_with_auth("", "api_key"), "Custom");
    }

    #[test]
    fn encrypted_reasoning_include_is_only_sent_to_openai() {
        assert!(supports_encrypted_reasoning_include(
            "https://api.openai.com/v1"
        ));
        assert!(!supports_encrypted_reasoning_include(
            "https://responses.example.com/v1"
        ));
        assert!(!supports_encrypted_reasoning_include(
            "https://api.openai.com.evil.example/v1"
        ));
        assert!(!supports_encrypted_reasoning_include(
            "http://api.openai.com/v1"
        ));
    }

    #[test]
    fn trailing_slash_does_not_break_match() {
        assert_eq!(
            detect_provider_with_auth("https://api.githubcopilot.com/", "copilot"),
            "Copilot"
        );
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1/", "codex"),
            "Codex"
        );
        assert_eq!(
            detect_provider_with_auth("https://api.orcarouter.ai/v1/", "api_key"),
            "OrcaRouter"
        );
    }

    #[test]
    fn assistant_with_reasoning_keeps_reasoning_hidden_field() {
        let msg = ApiMessage::assistant_with_reasoning("visible", "hidden thought");
        assert_eq!(msg.0["role"], "assistant");
        assert_eq!(msg.0["content"], "visible");
        assert_eq!(msg.0["reasoning_content"], "hidden thought");

        let without = ApiMessage::assistant_with_reasoning("visible", "");
        assert!(without.0.get("reasoning_content").is_none());
    }

    #[test]
    fn reasoning_delta_text_accepts_common_openai_compatible_shapes() {
        let cases = [
            (
                serde_json::json!({"delta": {"reasoning_content": "a"}}),
                "a",
            ),
            (serde_json::json!({"delta": {"reasoning": "b"}}), "b"),
            (
                serde_json::json!({"delta": {"reasoning": {"content": "c"}}}),
                "c",
            ),
            (serde_json::json!({"delta": {"thinking": "d"}}), "d"),
            (
                serde_json::json!({"delta": {"thinking": {"content": "e"}}}),
                "e",
            ),
            (
                serde_json::json!({"delta": {}, "reasoning_content": "fw"}),
                "fw",
            ),
            (serde_json::json!({"delta": {}, "reasoning": "f"}), "f"),
            (
                serde_json::json!({"delta": {}, "thinking": {"content": "g"}}),
                "g",
            ),
            (
                serde_json::json!({"delta": {}, "message": {"reasoning_content": "h"}}),
                "h",
            ),
        ];

        for (choice, expected) in cases {
            assert_eq!(
                reasoning_delta_text(&choice, &choice["delta"]),
                Some(expected)
            );
        }

        let choice = serde_json::json!({"delta": {"content": "visible"}});
        assert_eq!(reasoning_delta_text(&choice, &choice["delta"]), None);
    }

    #[test]
    fn sse_data_payload_accepts_optional_space_after_colon() {
        assert_eq!(sse_data_payload("data:{\"x\":1}"), Some("{\"x\":1}"));
        assert_eq!(sse_data_payload("data: {\"x\":1}"), Some("{\"x\":1}"));
        assert_eq!(sse_data_payload("event: message"), None);
    }

    #[test]
    fn responses_translation_flattens_functions_and_adds_native_search() {
        let messages = vec![
            ApiMessage::system("Be concise"),
            ApiMessage::user("Search this"),
            ApiMessage::assistant_tool_calls(serde_json::json!([{
                "id": "call_1",
                "type": "function",
                "function": { "name": "pwd", "arguments": "{}" }
            }])),
            ApiMessage::tool_result("call_1", "pwd", "/tmp"),
        ];
        let (instructions, input) = translate_responses_messages(&messages);
        assert_eq!(instructions, "Be concise");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[2]["type"], "function_call_output");

        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "pwd",
                "description": "Print cwd",
                "parameters": { "type": "object", "properties": {} },
                "strict": true
            }
        })];
        let translated = translate_responses_tools(&tools, true, true);
        assert_eq!(translated[0]["name"], "pwd");
        assert_eq!(translated[0]["strict"], true);
        assert_eq!(translated[1], serde_json::json!({ "type": "web_search" }));
        assert!(translate_responses_tools(&tools, false, true).is_empty());
    }

    #[test]
    fn responses_translation_replays_raw_reasoning_items_before_tool_outputs() {
        let reasoning = serde_json::json!({
            "type": "reasoning",
            "id": "rs_1",
            "encrypted_content": "opaque-provider-state",
            "summary": []
        });
        let function_call = serde_json::json!({
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "pwd",
            "arguments": "{}"
        });
        let messages = vec![
            ApiMessage::responses_output_item(reasoning.clone()),
            ApiMessage::responses_output_item(function_call.clone()),
            ApiMessage::tool_result("call_1", "pwd", "/tmp"),
        ];

        let (_, input) = translate_responses_messages(&messages);
        assert_eq!(input[0], reasoning);
        assert_eq!(input[1], function_call);
        assert_eq!(input[2]["type"], "function_call_output");
    }

    #[test]
    fn upsert_response_item_replaces_across_mixed_identity_events() {
        let added = serde_json::json!({ "type": "message", "id": "msg_1", "content": [] });
        let done = serde_json::json!({
            "type": "message",
            "content": [{ "type": "output_text", "text": "hi" }]
        });

        // added carries an id, done does not: output_index must still dedup.
        let mut items = Vec::new();
        let mut positions = std::collections::HashMap::new();
        super::upsert_response_item(&mut items, &added, Some(0), &mut positions).unwrap();
        super::upsert_response_item(&mut items, &done, Some(0), &mut positions).unwrap();
        assert_eq!(items.len(), 1, "mixed-identity events must not duplicate");
        assert_eq!(items[0], done);

        // Reverse order: added without id, done with id.
        let mut items = Vec::new();
        let mut positions = std::collections::HashMap::new();
        super::upsert_response_item(&mut items, &done, Some(0), &mut positions).unwrap();
        super::upsert_response_item(&mut items, &added, Some(0), &mut positions).unwrap();
        assert_eq!(items.len(), 1, "mixed-identity events must not duplicate");
        assert_eq!(items[0], added);
    }

    #[test]
    fn responses_sse_deduplicates_function_calls_across_mixed_identities() {
        let stream = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"fs_write\",\"arguments\":\"{}\"}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"fs_write\",\"arguments\":\"{}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
        );
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_| {},
            "test",
        )
        .unwrap();

        assert_eq!(result.response_items.len(), 1);
        assert_eq!(
            result.tool_calls.len(),
            1,
            "one output item must execute once"
        );
        assert_eq!(result.tool_calls[0].id, "call_1");
    }

    #[test]
    fn responses_sse_uses_output_index_for_late_item_id_deltas() {
        let stream = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"fs_write\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"item_id\":\"fc_1\",\"delta\":\"{}\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
        );
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_| {},
            "test",
        )
        .unwrap();

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].arguments, "{}");
        assert_eq!(result.response_items.len(), 1);
        assert_eq!(result.response_items[0]["arguments"], "{}");
    }

    #[test]
    fn responses_translation_skips_reasoning_stubs_without_encrypted_content() {
        let stub = serde_json::json!({
            "type": "reasoning",
            "id": "rs_1",
            "summary": []
        });
        let message_item = serde_json::json!({
            "type": "message",
            "id": "msg_1",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "hi" }]
        });
        let messages = vec![
            ApiMessage::responses_output_item(stub),
            ApiMessage::responses_output_item(message_item.clone()),
        ];

        let (_, input) = translate_responses_messages(&messages);
        assert_eq!(
            input.len(),
            1,
            "content-less reasoning stub must be dropped"
        );
        assert_eq!(input[0], message_item);
    }

    #[test]
    fn recognizes_vendor_json_content_types() {
        assert!(content_type_is_json("application/json; charset=utf-8"));
        assert!(content_type_is_json("application/problem+json"));
        assert!(content_type_is_json("application/vnd.openai.response+json"));
        assert!(!content_type_is_json("text/event-stream"));
    }

    #[test]
    fn custom_responses_mode_posts_expected_wire_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock responses server");
        let address = listener.local_addr().expect("mock server address");
        let (request_tx, request_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept responses request");
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read request headers");
                assert!(count > 0, "connection closed before request headers");
                request.extend_from_slice(&chunk[..count]);
                if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("content-length header");
            while request.len() < header_end + content_length {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read request body");
                assert!(count > 0, "connection closed before request body");
                request.extend_from_slice(&chunk[..count]);
            }
            request_tx.send(request).expect("capture request");

            let body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write responses stream");
        });

        let config = AssistantConfig {
            api_key: "test-token".to_string(),
            chat_model: "gpt-test".to_string(),
            chat_model_choices: Vec::new(),
            base_url: format!("http://{address}"),
            custom_headers: Vec::new(),
            provider: "Custom".to_string(),
            api_mode: ApiMode::Responses,
            auth_type: "api_key".to_string(),
            chat_tools_enabled: true,
            native_web_search: true,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new_with_timeout(config, std::time::Duration::from_secs(5));
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "pwd",
                "description": "Print cwd",
                "parameters": { "type": "object", "properties": {} }
            }
        })];
        let mut text = String::new();
        let calls = client
            .chat_step(
                "gpt-test",
                &[ApiMessage::user("hello")],
                &tools,
                true,
                &AtomicBool::new(false),
                &mut |token| text.push_str(token),
                &mut |_| {},
            )
            .expect("responses request");
        assert_eq!(text, "ok");
        assert!(calls.tool_calls.is_empty());

        server.join().expect("mock responses server");
        let request = request_rx.recv().expect("captured request");
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("request separator")
            + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
        assert!(headers.starts_with("post /responses http/1.1"));
        assert!(headers.contains("authorization: bearer test-token"));
        let body: serde_json::Value =
            serde_json::from_slice(&request[header_end..]).expect("request JSON");
        assert!(body.get("messages").is_none());
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert!(body["tools"]
            .as_array()
            .expect("responses tools")
            .iter()
            .any(|tool| tool == &serde_json::json!({ "type": "web_search" })));
        assert!(body["tools"]
            .as_array()
            .expect("responses tools")
            .iter()
            .any(|tool| tool["type"] == "function" && tool["name"] == "pwd"));
        assert!(
            body.get("include").is_none(),
            "custom Responses endpoints must not receive OpenAI-only include values"
        );
    }

    #[test]
    fn codex_connection_posts_to_custom_provider_with_headers_and_query() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Codex provider");
        let address = listener.local_addr().expect("mock provider address");
        let (request_tx, request_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept Codex request");
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read Codex headers");
                assert!(count > 0, "connection closed before Codex headers");
                request.extend_from_slice(&chunk[..count]);
                if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("Codex content-length header");
            while request.len() < header_end + content_length {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read Codex body");
                assert!(count > 0, "connection closed before Codex body");
                request.extend_from_slice(&chunk[..count]);
            }
            request_tx.send(request).expect("capture Codex request");

            let body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"followed\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write Codex response");
        });

        let config = AssistantConfig {
            api_key: String::new(),
            chat_model: "gpt-followed".to_string(),
            chat_model_choices: Vec::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            custom_headers: Vec::new(),
            provider: "Codex".to_string(),
            api_mode: ApiMode::Responses,
            auth_type: "codex".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new_with_timeout(config, std::time::Duration::from_secs(5));
        let connection = CodexConnection {
            model: Some("gpt-followed".to_string()),
            reasoning_effort: Some("high".to_string()),
            reasoning_summary: Some("auto".to_string()),
            base_url: format!("http://{address}/v1"),
            headers: vec![("x-codex-provider".to_string(), "followed".to_string())],
            query_params: vec![("tenant".to_string(), "kaku".to_string())],
            credential: CodexCredential::None,
            request_max_attempts: 1,
            stream_max_retries: 0,
            stream_idle_timeout_ms: None,
        };
        let mut text = String::new();
        client
            .chat_step_codex_connection(
                &connection,
                "gpt-followed",
                &[ApiMessage::user("hello")],
                &[],
                &AtomicBool::new(false),
                &mut |token| text.push_str(token),
                &mut |_| {},
            )
            .expect("Codex provider request");
        assert_eq!(text, "followed");

        server.join().expect("mock Codex provider");
        let request = request_rx.recv().expect("captured Codex request");
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("Codex request separator")
            + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/responses?tenant=kaku http/1.1"));
        assert!(headers.contains("x-codex-provider: followed"));
        assert!(!headers.contains("authorization:"));
        let body: serde_json::Value =
            serde_json::from_slice(&request[header_end..]).expect("Codex request JSON");
        assert_eq!(body["model"], "gpt-followed");
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    #[test]
    fn codex_model_discovery_uses_provider_models_endpoint_and_connection_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Codex provider");
        let address = listener.local_addr().expect("mock provider address");
        let (request_tx, request_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept models request");
            let mut request = Vec::new();
            loop {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read models request");
                assert!(count > 0, "connection closed before models headers");
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            request_tx.send(request).expect("capture models request");

            let body = r#"{"data":[{"id":"custom-chat"},{"id":"text-embedding-3-small"},{"id":"custom-fast"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write models response");
        });

        let config = AssistantConfig {
            api_key: String::new(),
            chat_model: "custom-chat".to_string(),
            chat_model_choices: Vec::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            custom_headers: Vec::new(),
            provider: "Codex".to_string(),
            api_mode: ApiMode::Responses,
            auth_type: "codex".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new_with_timeout(config, std::time::Duration::from_secs(5));
        let connection = CodexConnection {
            model: None,
            reasoning_effort: None,
            reasoning_summary: None,
            base_url: format!("http://{address}/v1"),
            headers: vec![("x-codex-provider".to_string(), "followed".to_string())],
            query_params: vec![("tenant".to_string(), "kaku".to_string())],
            credential: CodexCredential::Bearer("models-key".to_string()),
            request_max_attempts: 1,
            stream_max_retries: 0,
            stream_idle_timeout_ms: None,
        };

        let model = client
            .resolve_codex_request_model(&connection, crate::codex_connection::FOLLOW_CODEX_MODEL)
            .expect("discover Codex provider model");
        assert_eq!(model, "custom-chat");

        server.join().expect("mock models provider");
        let request = request_rx.recv().expect("captured models request");
        let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
        assert!(headers.starts_with("get /v1/models?tenant=kaku http/1.1"));
        assert!(headers.contains("x-codex-provider: followed"));
        assert!(headers.contains("authorization: bearer models-key"));
    }

    #[test]
    fn codex_chatgpt_requests_honor_provider_retry_count() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind retry provider");
        let address = listener.local_addr().expect("retry provider address");
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept retry request");
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0_u8; 4096];
                    let count = stream.read(&mut chunk).expect("read retry request");
                    assert!(count > 0, "connection closed before retry headers");
                    request.extend_from_slice(&chunk[..count]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
                assert!(headers.contains("authorization: bearer oauth-test"));
                let status = if attempt == 0 {
                    "500 Internal Server Error"
                } else {
                    "200 OK"
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                )
                .expect("write retry response");
            }
        });

        let config = AssistantConfig {
            api_key: String::new(),
            chat_model: "custom-chat".to_string(),
            chat_model_choices: Vec::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            custom_headers: Vec::new(),
            provider: "Codex".to_string(),
            api_mode: ApiMode::Responses,
            auth_type: "codex".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new_with_timeout(config, std::time::Duration::from_secs(5));
        let endpoint = format!("http://{address}/models");
        let build = |credential: &CodexCredential| {
            let CodexCredential::ChatGpt(auth) = credential else {
                panic!("expected ChatGPT credential");
            };
            client
                .client
                .get(&endpoint)
                .header("Authorization", format!("Bearer {}", auth.access_token))
        };
        let mut credential = CodexCredential::ChatGpt(crate::ai_auth::CodexAuth {
            access_token: "oauth-test".to_string(),
            account_id: None,
        });
        let response = client
            .send_codex_request_with_retry(
                &mut credential,
                &client.client,
                build,
                "retry provider",
                &AtomicBool::new(false),
                2,
            )
            .expect("ChatGPT request retries");
        assert!(response.status().is_success());
        server.join().expect("retry provider server");
    }

    #[test]
    fn codex_stream_idle_timeout_resets_when_data_keeps_arriving() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind streaming provider");
        let address = listener.local_addr().expect("streaming provider address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept streaming request");
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read streaming headers");
                assert!(count > 0, "connection closed before streaming headers");
                request.extend_from_slice(&chunk[..count]);
                if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("streaming content-length header");
            while request.len() < header_end + content_length {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).expect("read streaming body");
                assert!(count > 0, "connection closed before streaming body");
                request.extend_from_slice(&chunk[..count]);
            }

            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            )
            .expect("write streaming headers");
            write!(
                stream,
                "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"still \"}}\n\n"
            )
            .expect("write first stream event");
            std::thread::sleep(std::time::Duration::from_millis(300));
            write!(
                stream,
                "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"alive\"}}\n\n"
            )
            .expect("write second stream event");
            std::thread::sleep(std::time::Duration::from_millis(300));
            write!(
                stream,
                "data: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\",\"output\":[]}}}}\n\n"
            )
            .expect("write completed event");
        });

        let config = AssistantConfig {
            api_key: String::new(),
            chat_model: "custom-chat".to_string(),
            chat_model_choices: Vec::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            custom_headers: Vec::new(),
            provider: "Codex".to_string(),
            api_mode: ApiMode::Responses,
            auth_type: "codex".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new_with_timeout(config, std::time::Duration::from_secs(5));
        let connection = CodexConnection {
            model: Some("custom-chat".to_string()),
            reasoning_effort: None,
            reasoning_summary: None,
            base_url: format!("http://{address}"),
            headers: Vec::new(),
            query_params: Vec::new(),
            credential: CodexCredential::None,
            request_max_attempts: 1,
            stream_max_retries: 0,
            stream_idle_timeout_ms: Some(500),
        };
        let mut text = String::new();
        client
            .chat_step_codex_connection(
                &connection,
                "custom-chat",
                &[ApiMessage::user("hello")],
                &[],
                &AtomicBool::new(false),
                &mut |token| text.push_str(token),
                &mut |_| {},
            )
            .expect("active stream outlives one idle-timeout window");
        assert_eq!(text, "still alive");
        server.join().expect("streaming provider server");
    }

    #[test]
    fn responses_json_parses_text_citations_reasoning_and_function_calls() {
        let response = serde_json::json!({
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": "Checking sources" }]
                },
                {
                    "type": "message",
                    "content": [{
                        "type": "output_text",
                        "text": "Current answer",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.com/source",
                            "title": "Example source"
                        }]
                    }]
                },
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "pwd",
                    "arguments": "{}"
                }
            ]
        });
        let mut text = String::new();
        let mut reasoning = String::new();
        let result = parse_responses_value(
            &response,
            &mut |token| text.push_str(token),
            &mut |token| reasoning.push_str(token),
            "test",
        )
        .unwrap();

        assert_eq!(reasoning, "Checking sources");
        assert!(text.starts_with("Current answer"));
        assert!(text.contains("Example source: https://example.com/source"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].name, "pwd");
        assert_eq!(result.tool_calls[0].arguments, "{}");
        assert_eq!(result.response_items.len(), 3);
    }

    #[test]
    fn responses_sse_parses_streamed_text_citations_and_function_calls() {
        let stream = concat!(
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Thinking\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Answer\"}\n\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"url\":\"https://example.com\",\"title\":\"Example\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"pwd\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{}\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
        );
        let mut text = String::new();
        let mut reasoning = String::new();
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |token| text.push_str(token),
            &mut |token| reasoning.push_str(token),
            "test",
        )
        .unwrap();

        assert_eq!(reasoning, "Thinking");
        assert!(text.starts_with("Answer"));
        assert!(text.contains("Example: https://example.com"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call_1");
        assert_eq!(result.tool_calls[0].arguments, "{}");
        assert_eq!(result.response_items[0]["arguments"], "{}");
        let message = result
            .response_items
            .iter()
            .find(|item| item["type"] == "message")
            .expect("delta-only response should synthesize a replayable message item");
        assert_eq!(message["content"][0]["text"], text);
    }

    #[test]
    fn responses_json_output_text_synthesizes_replayable_message() {
        let response = serde_json::json!({
            "status": "completed",
            "output": [],
            "output_text": "final answer",
        });
        let mut text = String::new();
        let result = parse_responses_value(
            &response,
            &mut |token| text.push_str(token),
            &mut |_| {},
            "test",
        )
        .unwrap();

        assert_eq!(text, "final answer");
        assert_eq!(result.response_items.len(), 1);
        assert_eq!(result.response_items[0]["type"], "message");
        assert_eq!(
            result.response_items[0]["content"][0]["text"],
            "final answer"
        );
    }

    #[test]
    fn models_body_reader_rejects_oversized_success_payload() {
        let oversized = vec![b'x'; MAX_MODELS_BODY_BYTES + 1];
        let error = read_body_capped(Cursor::new(oversized), MAX_MODELS_BODY_BYTES, "models API")
            .expect_err("oversized model lists must be rejected before JSON parsing");
        assert!(error.to_string().contains("exceeded"));
    }

    #[test]
    fn responses_sse_rejects_eof_before_response_completed() {
        let stream = concat!(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"pwd\",\"arguments\":\"{}\"}}\n\n",
            "data: [DONE]\n\n"
        );
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_| {},
            "test",
        );

        assert!(
            result.is_err(),
            "truncated streams must never execute tools"
        );
    }

    #[test]
    fn sse_line_reader_rejects_unterminated_oversized_line() {
        let oversized = vec![b'x'; MAX_RESPONSE_SSE_LINE_BYTES + 1];
        let error = read_sse_line_capped(&mut Cursor::new(oversized), "test")
            .expect_err("oversized line must fail before it is returned");
        assert!(error.to_string().contains("SSE line exceeded"));
    }

    #[test]
    fn stream_budget_rejects_excess_bytes_and_events() {
        let mut bytes = MAX_RESPONSE_STREAM_BYTES;
        assert!(add_stream_bytes(&mut bytes, 1, "test").is_err());
        let mut events = MAX_RESPONSE_STREAM_EVENTS;
        assert!(add_stream_event(&mut events, "test").is_err());
    }

    #[test]
    fn responses_sse_rejects_malformed_data_event() {
        let stream = concat!(
            "data: {not-json}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
        );
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_| {},
            "test",
        );

        assert!(
            result.is_err(),
            "malformed lifecycle events must fail closed"
        );
    }

    #[test]
    fn responses_sse_rejects_oversized_tool_arguments() {
        let oversized = "x".repeat(MAX_RESPONSE_TOOL_ARGUMENT_BYTES + 1);
        let stream = format!(
            "data: {{\"type\":\"response.output_item.added\",\"item\":{{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"pwd\",\"arguments\":\"\"}}}}\n\ndata: {{\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{}\"}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\",\"output\":[]}}}}\n\n",
            oversized
        );
        let result = parse_responses_sse(
            Cursor::new(stream),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_| {},
            "test",
        );

        assert!(result.is_err());
    }

    #[test]
    fn mock_sse_routes_fireworks_reasoning_content_before_visible_content() {
        let (tokens, reasoning) = route_mock_sse_lines(&[
            r#"data: {"choices":[{"delta":{"reasoning_content":"hidden "},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"visible"},"finish_reason":null}]}"#,
            "data: [DONE]",
        ]);

        assert_eq!(reasoning, "hidden ");
        assert_eq!(tokens, "visible");
    }

    #[test]
    fn mock_sse_inline_think_tags_split_across_chunks_do_not_leak() {
        let (tokens, reasoning) = route_mock_sse_lines(&[
            r#"data: {"choices":[{"delta":{"content":"<THI"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"NK >one</ TH"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"INK >visible<think"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"ing>two</thinking>"},"finish_reason":null}]}"#,
            "data: [DONE]",
        ]);

        assert_eq!(reasoning, "onetwo");
        assert_eq!(tokens, "visible");
        assert!(!tokens.to_ascii_lowercase().contains("think"));
    }

    #[test]
    fn reasoning_roundtrip_is_limited_to_reasoning_models() {
        assert!(should_roundtrip_reasoning_content("deepseek-v4-pro"));
        assert!(should_roundtrip_reasoning_content("Kimi-K2.5"));
        assert!(should_roundtrip_reasoning_content("mimo-thinking"));
        assert!(!should_roundtrip_reasoning_content("gpt-5.4"));
        assert!(!should_roundtrip_reasoning_content(
            "gemini-3-flash-preview"
        ));
    }

    #[test]
    fn parses_custom_headers_from_array_and_rejects_bad_entries() {
        let value = toml::Value::Array(vec![
            toml::Value::String("X-Customer-ID: acme".to_string()),
            toml::Value::String("X-Trace: abc:123".to_string()),
        ]);
        let headers = parse_custom_headers(Some(&value)).unwrap();
        assert_eq!(
            headers,
            vec![
                ("X-Customer-ID".to_string(), "acme".to_string()),
                ("X-Trace".to_string(), "abc:123".to_string())
            ]
        );

        let bad = toml::Value::Array(vec![toml::Value::String("missing-colon".to_string())]);
        assert!(parse_custom_headers(Some(&bad)).is_err());

        let reserved =
            toml::Value::Array(vec![toml::Value::String("Authorization: nope".to_string())]);
        assert!(parse_custom_headers(Some(&reserved)).is_err());
    }

    #[test]
    fn custom_headers_replace_existing_user_agent_without_dropping_auth() {
        let config = AssistantConfig {
            api_key: "test-token".to_string(),
            chat_model: "gpt-test".to_string(),
            chat_model_choices: Vec::new(),
            base_url: "https://example.test/v1".to_string(),
            custom_headers: vec![
                ("User-Agent".to_string(), "Kaku-Test".to_string()),
                ("X-Customer-ID".to_string(), "acme".to_string()),
            ],
            provider: "Custom".to_string(),
            api_mode: ApiMode::ChatCompletions,
            auth_type: "api_key".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };
        let client = AiClient::new(config);
        let request = reqwest::blocking::Client::new()
            .post("https://example.test/v1/chat/completions")
            .header(USER_AGENT, "reqwest-default");

        let request = client.apply_auth_headers(request).unwrap().build().unwrap();
        let headers = request.headers();
        let user_agents = headers.get_all(USER_AGENT).iter().collect::<Vec<_>>();

        assert_eq!(user_agents.len(), 1);
        assert_eq!(user_agents[0], "Kaku-Test");
        assert_eq!(
            headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()),
            Some("Bearer test-token")
        );
        assert_eq!(
            headers.get("X-Customer-ID").and_then(|v| v.to_str().ok()),
            Some("acme")
        );
    }

    #[test]
    fn one_shot_client_does_not_multiply_inline_timeout_with_retries() {
        let config = AssistantConfig {
            api_key: "test-token".to_string(),
            chat_model: "gpt-test".to_string(),
            chat_model_choices: Vec::new(),
            base_url: "https://example.test/v1".to_string(),
            custom_headers: Vec::new(),
            provider: "Custom".to_string(),
            api_mode: ApiMode::ChatCompletions,
            auth_type: "api_key".to_string(),
            chat_tools_enabled: true,
            native_web_search: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        };

        let client = AiClient::new_with_timeout(config, std::time::Duration::from_millis(100));
        assert_eq!(client.max_request_attempts, 1);
    }

    #[test]
    fn think_filter_single_block() {
        let mut f = InlineThinkFilter::new();
        let segs = f.feed("<think>reasoning</think>visible");
        let mut tokens = Vec::new();
        let mut reasoning = Vec::new();
        for s in segs {
            match s {
                ThinkSegment::Token(t) => tokens.push(t),
                ThinkSegment::Reasoning(r) => reasoning.push(r),
            }
        }
        assert_eq!(reasoning.join(""), "reasoning");
        assert_eq!(tokens.join(""), "visible");
    }

    #[test]
    fn think_filter_split_across_chunks() {
        let mut f = InlineThinkFilter::new();
        let mut tokens = Vec::new();
        let mut reasoning = Vec::new();
        let collect =
            |segs: Vec<ThinkSegment>, tokens: &mut Vec<String>, reasoning: &mut Vec<String>| {
                for s in segs {
                    match s {
                        ThinkSegment::Token(t) => tokens.push(t),
                        ThinkSegment::Reasoning(r) => reasoning.push(r),
                    }
                }
            };
        collect(f.feed("<thi"), &mut tokens, &mut reasoning);
        collect(f.feed("nk>deep thought</thi"), &mut tokens, &mut reasoning);
        collect(f.feed("nk>hello"), &mut tokens, &mut reasoning);
        collect(f.flush(), &mut tokens, &mut reasoning);
        assert_eq!(reasoning.join(""), "deep thought");
        assert_eq!(tokens.join(""), "hello");
    }

    #[test]
    fn think_filter_no_tags() {
        let mut f = InlineThinkFilter::new();
        let segs = f.feed("plain text");
        assert!(segs.iter().all(|s| matches!(s, ThinkSegment::Token(_))));
        let text: String = segs
            .into_iter()
            .map(|s| match s {
                ThinkSegment::Token(t) => t,
                _ => String::new(),
            })
            .collect();
        assert_eq!(text, "plain text");
    }

    #[test]
    fn think_filter_repeated_tags() {
        let mut f = InlineThinkFilter::new();
        let segs = f.feed("<think>a</think>x<think>b</think>y");
        let mut tokens = String::new();
        let mut reasoning = String::new();
        for s in segs {
            match s {
                ThinkSegment::Token(t) => tokens.push_str(&t),
                ThinkSegment::Reasoning(r) => reasoning.push_str(&r),
            }
        }
        assert_eq!(reasoning, "ab");
        assert_eq!(tokens, "xy");
    }

    #[test]
    fn think_filter_thinking_tags() {
        let mut f = InlineThinkFilter::new();
        let segs = f.feed("<thinking>deep</thinking>answer");
        let mut tokens = String::new();
        let mut reasoning = String::new();
        for s in segs {
            match s {
                ThinkSegment::Token(t) => tokens.push_str(&t),
                ThinkSegment::Reasoning(r) => reasoning.push_str(&r),
            }
        }
        assert_eq!(reasoning, "deep");
        assert_eq!(tokens, "answer");
    }

    #[test]
    fn think_filter_is_case_and_spacing_tolerant() {
        let mut f = InlineThinkFilter::new();
        let (tokens, reasoning) = collect_segments(f.feed("< THINKING >deep</ THINKING >answer"));
        assert_eq!(reasoning, "deep");
        assert_eq!(tokens, "answer");
    }

    #[test]
    fn think_filter_mixed_tag_variants() {
        let mut f = InlineThinkFilter::new();
        let segs = f.feed("<think>a</think>x<thinking>b</thinking>y");
        let mut tokens = String::new();
        let mut reasoning = String::new();
        for s in segs {
            match s {
                ThinkSegment::Token(t) => tokens.push_str(&t),
                ThinkSegment::Reasoning(r) => reasoning.push_str(&r),
            }
        }
        assert_eq!(reasoning, "ab");
        assert_eq!(tokens, "xy");
    }

    #[test]
    fn think_filter_thinking_split_across_chunks() {
        let mut f = InlineThinkFilter::new();
        let mut tokens = Vec::new();
        let mut reasoning = Vec::new();
        let collect =
            |segs: Vec<ThinkSegment>, tokens: &mut Vec<String>, reasoning: &mut Vec<String>| {
                for s in segs {
                    match s {
                        ThinkSegment::Token(t) => tokens.push(t),
                        ThinkSegment::Reasoning(r) => reasoning.push(r),
                    }
                }
            };
        collect(f.feed("<thinki"), &mut tokens, &mut reasoning);
        collect(f.feed("ng>reason</thinki"), &mut tokens, &mut reasoning);
        collect(f.feed("ng>visible"), &mut tokens, &mut reasoning);
        collect(f.flush(), &mut tokens, &mut reasoning);
        assert_eq!(reasoning.join(""), "reason");
        assert_eq!(tokens.join(""), "visible");
    }

    // ─── SSE rough-input rubustness ──────────────────────────────────────
    // Real providers occasionally return malformed SSE: HTML error pages
    // from CDNs, truncated chunks, empty choices arrays, comment frames.
    // The contract is: parse what we can, skip what we can't, never panic.

    #[test]
    fn mock_sse_skips_malformed_json_chunks() {
        let lines = vec![
            "data: {not json}",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
            "data: [DONE]",
        ];
        let (tokens, reasoning) = route_mock_sse_lines(&lines);
        assert_eq!(tokens, "hi");
        assert!(reasoning.is_empty());
    }

    #[test]
    fn mock_sse_skips_chunks_with_empty_choices() {
        // Some providers (Anthropic-compat shims, certain proxies) send
        // keep-alive chunks with empty `choices` arrays. Must not panic on
        // `choices[0]` indexing.
        let lines = vec![
            "data: {\"choices\":[]}",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}",
            "data: [DONE]",
        ];
        let (tokens, _) = route_mock_sse_lines(&lines);
        assert_eq!(tokens, "ok");
    }

    #[test]
    fn mock_sse_ignores_html_error_page() {
        // CDN / reverse-proxy failure modes occasionally return an HTML
        // 502/504 with `data:` prefix injected by middleware. We must walk
        // off the end without crashing or fabricating output.
        let lines = vec![
            "data: <html>",
            "data: <body>502 Bad Gateway</body>",
            "data: </html>",
        ];
        let (tokens, reasoning) = route_mock_sse_lines(&lines);
        assert!(tokens.is_empty());
        assert!(reasoning.is_empty());
    }

    #[test]
    fn mock_sse_handles_interleaved_done_and_data() {
        // [DONE] must terminate the stream even if more data lines follow
        // (some providers leak trailing chunks during connection close).
        let lines = vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}",
            "data: [DONE]",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ignored\"}}]}",
        ];
        let (tokens, _) = route_mock_sse_lines(&lines);
        assert_eq!(tokens, "a");
    }

    #[test]
    fn no_proxy_list_includes_private_and_local_model_hosts() {
        let entries = build_no_proxy_list();
        for expected in [
            "localhost",
            "127.0.0.0/8",
            "::1",
            "169.254.0.0/16",
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "100.64.0.0/10",
            ".local",
        ] {
            assert!(
                entries.iter().any(|entry| entry == expected),
                "missing no-proxy entry {}",
                expected
            );
        }
    }
}
