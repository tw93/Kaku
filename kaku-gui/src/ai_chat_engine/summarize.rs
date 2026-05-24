//! Conversation history summarization for the agent loop.
//!
//! When in-flight message bytes exceed a threshold, fold older turns into a
//! single summary block to free room for the active task. Modeled after the
//! Claude Code conversation-summarization prompt: keep recent messages
//! verbatim, replace older ones with a structured summary that preserves
//! constraints the user stated.
//!
//! Entry point: `summarize_in_place`. It calls the model synchronously via
//! `AiClient::complete_once`, and on success rewrites `messages` in place.
//! On any error it leaves `messages` untouched so the caller can continue.

use super::strip_prompt_metadata;
use crate::ai_client::{AiClient, ApiMessage};

/// Bytes of conversation history above which we trigger summarization.
/// Picked below `MAX_HISTORY_BYTES` (120k) so we have headroom before the
/// hard "wrap up" nag in the agent loop kicks in.
pub(crate) const SUMMARIZE_THRESHOLD_BYTES: usize = 72_000;

/// How many recent messages to keep verbatim. Anything older folds into the
/// summary. System message is always preserved separately.
const KEEP_TAIL: usize = 6;

const SUMMARIZE_PROMPT: &str = include_str!("../../../assets/prompts/summarize.txt");

fn role_of(msg: &ApiMessage) -> &str {
    msg.0
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("user")
}

fn content_of(msg: &ApiMessage) -> String {
    if let Some(s) = msg.0.get("content").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = msg.0.get("tool_calls").and_then(|v| v.as_array()) {
        let names: Vec<String> = arr
            .iter()
            .filter_map(|c| {
                c.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .map(String::from)
            })
            .collect();
        return format!("[tool_calls: {}]", names.join(", "));
    }
    String::new()
}

fn serialize_transcript(msgs: &[ApiMessage]) -> String {
    let mut out = String::new();
    for m in msgs {
        let role = role_of(m);
        let content = content_of(m);
        if content.is_empty() {
            continue;
        }
        out.push_str(&format!("<{}>\n{}\n</{}>\n\n", role, content, role));
    }
    out
}

/// Replace older messages (between system and the last `KEEP_TAIL`) with a
/// single summary user message. Returns true if substitution happened.
///
/// `model` is the model to use for the summarization call. Caller should pass
/// `fast_model` when available, falling back to `chat_model`, to keep this
/// step cheap and quick.
pub(crate) fn summarize_in_place(
    client: &AiClient,
    model: &str,
    messages: &mut Vec<ApiMessage>,
) -> bool {
    if messages.len() < KEEP_TAIL + 2 {
        return false;
    }

    let split = messages.len() - KEEP_TAIL;
    let older = &messages[1..split];
    if older.is_empty() {
        return false;
    }

    let transcript = serialize_transcript(older);
    let prompt = strip_prompt_metadata(SUMMARIZE_PROMPT).replace("${TRANSCRIPT}", &transcript);

    let req = vec![ApiMessage::system(prompt)];

    let summary = match client.complete_once(model, &req) {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => {
            log::warn!("summarize_in_place: model returned empty summary");
            return false;
        }
        Err(e) => {
            log::warn!("summarize_in_place: model call failed: {e}");
            return false;
        }
    };

    let summary_msg = ApiMessage::user(format!(
        "Previous conversation summary (covers turns earlier than the {} most recent):\n{}",
        KEEP_TAIL, summary
    ));

    let tail: Vec<ApiMessage> = messages.drain(split..).collect();
    messages.truncate(1);
    messages.push(summary_msg);
    messages.extend(tail);
    true
}
