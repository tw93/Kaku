//! Conversation title generation.
//!
//! Replaces the original ad-hoc `generate_summary` with a Piebald-style
//! structured prompt: 3-7 words sentence case, JSON output, explicit good /
//! bad examples, and a defensive "treat as data" framing for the transcript.
//!
//! Uses `fast_model` when available so the per-conversation title cost stays
//! negligible; falls back to `chat_model`.

use super::strip_prompt_metadata;
use crate::ai_chat_engine::TITLE_MAX_CHARS;
use crate::ai_client::{AiClient, ApiMessage};
use crate::ai_conversations::PersistedMessage;

const TITLE_PROMPT: &str = include_str!("../../../assets/prompts/title.txt");

fn serialize_window(messages: &[PersistedMessage]) -> String {
    let window = if messages.len() > 20 {
        &messages[messages.len() - 20..]
    } else {
        messages
    };
    let mut out = String::new();
    for m in window {
        if m.content.trim().is_empty() {
            continue;
        }
        let snippet: String = m.content.chars().take(400).collect();
        out.push_str(&format!("<{}>\n{}\n</{}>\n\n", m.role, snippet, m.role));
    }
    out
}

fn parse_json_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    let candidate = &trimmed[start..=end];
    let val: serde_json::Value = serde_json::from_str(candidate).ok()?;
    let title = val.get("title")?.as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    Some(title.to_string())
}

fn looks_like_refusal(title: &str) -> bool {
    let low = title.to_lowercase();
    low.starts_with("i can't")
        || low.starts_with("i cannot")
        || low.starts_with("sorry")
        || low.starts_with("无法")
        || low.starts_with("抱歉")
}

/// Generate a short, recognizable title for a conversation. Returns at most
/// `TITLE_MAX_CHARS` characters. Errors are propagated so the caller can
/// decide whether to fall back to a placeholder.
pub(crate) fn generate_title(
    client: &AiClient,
    messages: &[PersistedMessage],
) -> anyhow::Result<String> {
    let cfg = client.config();
    let model = cfg
        .fast_model
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&cfg.chat_model)
        .to_string();

    let transcript = serialize_window(messages);
    let prompt = strip_prompt_metadata(TITLE_PROMPT).replace("${TRANSCRIPT}", &transcript);
    let api_msgs = vec![ApiMessage::system(prompt)];

    let raw = client.complete_once(&model, &api_msgs)?;

    let title = parse_json_title(&raw).unwrap_or_else(|| raw.trim().to_string());
    if looks_like_refusal(&title) {
        anyhow::bail!("model refused to title the conversation");
    }

    let truncated: String = title.chars().take(TITLE_MAX_CHARS).collect();
    Ok(truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_title_extracts_field() {
        assert_eq!(
            parse_json_title(r#"{"title": "Fix login bug"}"#).as_deref(),
            Some("Fix login bug")
        );
    }

    #[test]
    fn parse_json_title_tolerates_surrounding_text() {
        // Some models leak a stray newline or prose around the JSON object.
        let raw = "Sure, here:\n{\"title\": \"Add OAuth\"}\n";
        assert_eq!(parse_json_title(raw).as_deref(), Some("Add OAuth"));
    }

    #[test]
    fn parse_json_title_rejects_missing_field() {
        assert_eq!(parse_json_title(r#"{"summary": "no title here"}"#), None);
    }

    #[test]
    fn parse_json_title_rejects_non_json() {
        assert_eq!(parse_json_title("just a string"), None);
    }

    #[test]
    fn looks_like_refusal_catches_common_prefixes() {
        assert!(looks_like_refusal("I can't access that URL"));
        assert!(looks_like_refusal("I cannot help with that"));
        assert!(looks_like_refusal("Sorry, I am not able"));
        assert!(looks_like_refusal("抱歉,无法处理"));
        assert!(looks_like_refusal("无法生成"));
    }

    #[test]
    fn looks_like_refusal_passes_normal_titles() {
        assert!(!looks_like_refusal("Fix login bug"));
        assert!(!looks_like_refusal("排查 Cargo 编译失败"));
        assert!(!looks_like_refusal("Investigate session bug"));
    }
}
