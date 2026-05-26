//! HTTP fetch and web-request helpers.

use anyhow::{Context, Result};
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

/// Shared HTTP client for all web tool calls (connection pool, keep-alive).
pub(super) fn web_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|e| {
                log::warn!("web client build failed ({e}), falling back to default");
                reqwest::blocking::Client::new()
            })
    })
}

/// Maximum bytes to buffer from any single HTTP fetch response.
const MAX_FETCH_BYTES: usize = 512 * 1024;

/// Read at most `MAX_FETCH_BYTES` from a reqwest blocking Response.
pub(super) fn read_response_capped(resp: reqwest::blocking::Response) -> Result<String> {
    let mut buf = Vec::with_capacity(MAX_FETCH_BYTES.min(64 * 1024));
    resp.take(MAX_FETCH_BYTES as u64)
        .read_to_end(&mut buf)
        .context("read HTTP response body")?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read at most 4 KiB from an error response for diagnostic messages.
pub(super) fn read_error_body(resp: reqwest::blocking::Response) -> String {
    let mut buf = Vec::with_capacity(4096);
    let _ = resp.take(4096).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Fetch a URL as Markdown. Primary: defuddle.md. Fallback: r.jina.ai.
pub(super) fn fetch_markdown_default(url: &str) -> Result<String> {
    let client = web_client();
    if let Ok(resp) = client.get(format!("https://defuddle.md/{}", url)).send() {
        if resp.status().is_success() {
            if let Ok(body) = read_response_capped(resp) {
                if !body.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
    }
    let resp = client
        .get(format!("https://r.jina.ai/{}", url))
        .send()
        .context("both defuddle.md and r.jina.ai unreachable")?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "fetch failed: defuddle.md and r.jina.ai both returned non-2xx (last: {})",
            resp.status()
        );
    }
    read_response_capped(resp).context("read fetch response body")
}

pub(super) fn exec_http_request(
    method: &str,
    url: &str,
    headers: Option<&serde_json::Map<String, serde_json::Value>>,
    body: Option<&str>,
    query_params: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<String> {
    let client = web_client();
    let mut req = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        _ => anyhow::bail!("unsupported HTTP method: {}", method),
    };

    if let Some(params) = query_params {
        let pairs: Vec<(&str, &str)> = params
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
            .collect();
        req = req.query(&pairs);
    }

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    if let Some(b) = body {
        if serde_json::from_str::<serde_json::Value>(b).is_ok() {
            req = req
                .header("Content-Type", "application/json")
                .body(b.to_string());
        } else {
            req = req.body(b.to_string());
        }
    }

    let resp = req
        .send()
        .with_context(|| format!("http_request {} {} failed", method, url))?;

    let status = resp.status();
    let resp_headers: Vec<String> = resp
        .headers()
        .iter()
        .filter(|(k, _)| {
            let name = k.as_str().to_ascii_lowercase();
            matches!(
                name.as_str(),
                "content-type" | "content-length" | "x-request-id" | "x-ratelimit-remaining"
            )
        })
        .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or("?")))
        .collect();
    let body_text = read_response_capped(resp).context("read http_request response body")?;

    let mut out = format!("HTTP {}\n", status.as_u16());
    if !resp_headers.is_empty() {
        out.push_str(&resp_headers.join("\n"));
        out.push('\n');
    }
    out.push('\n');
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body_text) {
        out.push_str(&serde_json::to_string_pretty(&json).unwrap_or(body_text));
    } else {
        out.push_str(&body_text);
    }
    Ok(out)
}

/// Read a URL and return clean extracted text.
/// Uses provider-native readers where available, falls back to generic fetchers.
pub(super) fn exec_read_url(url: &str, provider: &str, api_key: &str) -> Result<String> {
    match provider {
        "pipellm" => {
            let domains = ["https://api.pipellm.ai", "https://api.pipellm.com"];
            let mut last_err = String::new();
            for base in &domains {
                let resp = match web_client()
                    .get(format!("{}/v1/websearch/reader", base))
                    .query(&[("url", url)])
                    .bearer_auth(api_key)
                    .send()
                {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = e.to_string();
                        continue;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = read_error_body(resp);
                    last_err = format!(
                        "{} from {}: {}",
                        status,
                        base,
                        body.chars().take(300).collect::<String>()
                    );
                    continue;
                }
                let json: serde_json::Value =
                    resp.json().context("parse pipellm reader response")?;
                let text = json["content"]
                    .as_str()
                    .or_else(|| json["text"].as_str())
                    .or_else(|| json.as_str())
                    .unwrap_or("")
                    .to_string();
                if !text.trim().is_empty() {
                    return Ok(text);
                }
                return Ok("Page returned empty content.".into());
            }
            log::warn!(
                "pipellm reader failed ({}), falling back to generic fetch",
                last_err
            );
            fetch_markdown_default(url)
        }
        "tavily" => {
            let resp = web_client()
                .post("https://api.tavily.com/extract")
                .bearer_auth(api_key)
                .json(&serde_json::json!({ "urls": [url] }))
                .send()
                .context("tavily extract request failed")?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = read_error_body(resp);
                log::warn!(
                    "tavily extract returned {} ({}), falling back to generic fetch",
                    status,
                    body.trim().chars().take(200).collect::<String>()
                );
                return fetch_markdown_default(url);
            }
            let json: serde_json::Value = resp.json().context("parse tavily extract response")?;
            let content = json["results"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|r| r["raw_content"].as_str().or_else(|| r["content"].as_str()))
                .unwrap_or("")
                .to_string();
            if content.trim().is_empty() {
                return fetch_markdown_default(url);
            }
            Ok(content)
        }
        _ => fetch_markdown_default(url),
    }
}

/// Content above this raw-bytes threshold is passed through a cheap
/// summarizer before being returned to the main agent. Picked so a typical
/// docs page (under 4 KB clean markdown) skips the second LLM hop, while
/// long blog posts and reference pages get compressed.
const SUMMARIZE_FETCH_THRESHOLD: usize = 4_000;

const WEBFETCH_SUMMARIZE_PROMPT: &str =
    include_str!("../../../assets/prompts/webfetch_summarize.txt");

pub(super) fn should_return_raw_fetch(detail: &str, raw_requested: bool) -> bool {
    raw_requested || detail == "full"
}

/// Compress a verbose web_fetch result so the main agent context stays cheap.
///
/// - `raw_passthrough`: return fetched content verbatim. Used when the caller
///   needs exact source text for quoting, debugging, or a full-detail read.
/// - Below `SUMMARIZE_FETCH_THRESHOLD` bytes: passthrough.
/// - Otherwise: build a small `AiClient` from the active config, call
///   `complete_once` with the webfetch-summarizer prompt, return the
///   summary. On any error, fall back to the raw content so the agent loop
///   never breaks just because the summarizer was misconfigured.
///
/// Uses `fast_model` when present (this is a low-stakes compression step
/// and should not bill against the deep model).
pub(super) fn maybe_summarize_fetched(
    url: &str,
    content: String,
    config: &crate::ai_client::AssistantConfig,
    raw_passthrough: bool,
) -> String {
    if raw_passthrough {
        return content;
    }
    if content.len() < SUMMARIZE_FETCH_THRESHOLD {
        return content;
    }
    let model = config
        .fast_model
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&config.chat_model)
        .to_string();
    if model.is_empty() {
        return content;
    }
    let prompt = crate::ai_chat_engine::strip_prompt_metadata(WEBFETCH_SUMMARIZE_PROMPT)
        .replace("${URL}", url)
        .replace("${WEB_CONTENT}", &content);
    let client = crate::ai_client::AiClient::new(config.clone());
    match client.complete_once(&model, &[crate::ai_client::ApiMessage::system(prompt)]) {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => {
            log::warn!("maybe_summarize_fetched: empty summary, returning raw");
            content
        }
        Err(e) => {
            log::warn!("maybe_summarize_fetched: model call failed: {e}; returning raw");
            content
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_fetch_policy_respects_full_detail_and_explicit_raw() {
        assert!(should_return_raw_fetch("full", false));
        assert!(should_return_raw_fetch("default", true));
        assert!(!should_return_raw_fetch("default", false));
        assert!(!should_return_raw_fetch("brief", false));
    }
}
