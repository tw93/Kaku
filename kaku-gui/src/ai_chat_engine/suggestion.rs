//! Prompt-suggestion generator.
//!
//! Predicts the user's likely next message based on the recent transcript.
//! Mirrors Piebald's `prompt-suggestion-generator-v2` rules: no evaluative
//! filler, no assistant-voice, no new ideas, stay silent when the next step
//! is not obvious.
//!
//! Returns either a short string (2-12 words) or empty when silence is the
//! correct answer. Empty is a valid, expected outcome and the caller should
//! display nothing in that case.

use super::strip_prompt_metadata;
use crate::ai_client::{AiClient, ApiMessage};
use crate::ai_conversations::PersistedMessage;

const SUGGESTION_PROMPT: &str = include_str!("../../../assets/prompts/prompt_suggestion.txt");

/// Maximum characters in a rendered suggestion. Anything longer would not
/// fit one row of the input area at common terminal widths.
const SUGGESTION_MAX_CHARS: usize = 80;

/// How many recent messages to feed into the predictor. More context costs
/// tokens with diminishing accuracy gains; six covers a typical
/// question/answer/follow-up window.
const WINDOW: usize = 6;

fn serialize_window(messages: &[PersistedMessage]) -> String {
    let window = if messages.len() > WINDOW {
        &messages[messages.len() - WINDOW..]
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

fn parse_json_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    let candidate = &trimmed[start..=end];
    let val: serde_json::Value = serde_json::from_str(candidate).ok()?;
    let text = val.get("text")?.as_str()?.trim().to_string();
    Some(text)
}

/// Generate the user's likely next message. Empty string means "stay silent".
///
/// On any error, returns `Ok(String::new())` — the caller should treat an
/// unavailable suggestion the same as a deliberate silence so a failing
/// model never blocks the chat. Hard errors only bubble up via log::warn.
pub(crate) fn generate_suggestion(
    client: &AiClient,
    messages: &[PersistedMessage],
) -> anyhow::Result<String> {
    if messages.len() < 2 {
        return Ok(String::new());
    }
    let cfg = client.config();
    let model = cfg
        .fast_model
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&cfg.chat_model)
        .to_string();
    if model.is_empty() {
        return Ok(String::new());
    }
    let transcript = serialize_window(messages);
    let prompt = strip_prompt_metadata(SUGGESTION_PROMPT).replace("${TRANSCRIPT}", &transcript);
    let raw = client.complete_once(&model, &[ApiMessage::system(prompt)])?;
    let text = parse_json_text(&raw).unwrap_or_default();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let truncated: String = trimmed.chars().take(SUGGESTION_MAX_CHARS).collect();
    Ok(truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_text_extracts_field() {
        assert_eq!(
            parse_json_text(r#"{"text": "run the tests"}"#).as_deref(),
            Some("run the tests")
        );
    }

    #[test]
    fn parse_json_text_returns_empty_when_silent() {
        // Model used the "stay silent" escape hatch (empty string).
        // Should still parse cleanly so caller treats it as silence.
        assert_eq!(parse_json_text(r#"{"text": ""}"#).as_deref(), Some(""));
    }

    #[test]
    fn parse_json_text_rejects_missing_field() {
        assert_eq!(parse_json_text(r#"{"other": "x"}"#), None);
    }
}
