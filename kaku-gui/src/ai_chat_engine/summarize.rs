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

/// System and environment messages are always preserved. The environment turn
/// carries cwd/date/platform context and must not be folded into a summary.
const PREFIX_KEEP: usize = 2;

/// How many recent messages to keep verbatim. Anything older folds into the
/// summary, unless doing so would cut through the active tool-call sequence.
const KEEP_TAIL: usize = 6;

const SUMMARIZE_PROMPT: &str = include_str!("../../../assets/prompts/summarize.txt");

fn role_of(msg: &ApiMessage) -> &str {
    msg.0.get("role").and_then(|v| v.as_str()).unwrap_or("user")
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

fn has_tool_calls(msg: &ApiMessage) -> bool {
    msg.0.get("tool_calls").and_then(|v| v.as_array()).is_some()
}

fn is_tool_result(msg: &ApiMessage) -> bool {
    role_of(msg) == "tool"
}

fn first_tool_interaction(messages: &[ApiMessage]) -> Option<usize> {
    messages
        .iter()
        .position(|msg| has_tool_calls(msg) || is_tool_result(msg))
}

fn active_tool_sequence_start(messages: &[ApiMessage], first_tool: usize) -> usize {
    messages[PREFIX_KEEP..first_tool]
        .iter()
        .rposition(|msg| role_of(msg) == "user")
        .map(|idx| PREFIX_KEEP + idx)
        .unwrap_or(first_tool)
}

fn summary_split_index(messages: &[ApiMessage]) -> Option<usize> {
    if messages.len() <= PREFIX_KEEP + KEEP_TAIL {
        return None;
    }

    let mut split = messages.len() - KEEP_TAIL;
    if let Some(first_tool) = first_tool_interaction(&messages[PREFIX_KEEP..]) {
        let first_tool = PREFIX_KEEP + first_tool;
        split = split.min(active_tool_sequence_start(messages, first_tool));
    }

    while split > PREFIX_KEEP && is_tool_result(&messages[split]) {
        split -= 1;
    }

    (split > PREFIX_KEEP).then_some(split)
}

/// Replace older ordinary history with a single summary user message. Returns
/// true if substitution happened.
///
/// `model` is the model to use for the summarization call. Caller should pass
/// `fast_model` when available, falling back to `chat_model`, to keep this
/// step cheap and quick.
pub(crate) fn summarize_in_place(
    client: &AiClient,
    model: &str,
    messages: &mut Vec<ApiMessage>,
) -> bool {
    let Some(split) = summary_split_index(messages) else {
        return false;
    };
    let older = &messages[PREFIX_KEEP..split];
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
    messages.truncate(PREFIX_KEEP);
    messages.push(summary_msg);
    messages.extend(tail);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_calls() -> ApiMessage {
        ApiMessage::assistant_tool_calls(serde_json::json!([
            {
                "id": "call_1",
                "type": "function",
                "function": { "name": "fs_read", "arguments": "{}" }
            },
            {
                "id": "call_2",
                "type": "function",
                "function": { "name": "grep_search", "arguments": "{}" }
            }
        ]))
    }

    #[test]
    fn summary_split_preserves_system_and_environment_prefix() {
        let mut messages = vec![
            ApiMessage::system("system"),
            ApiMessage::user("environment"),
        ];
        for idx in 0..10 {
            messages.push(ApiMessage::user(format!("user {idx}")));
        }

        assert_eq!(summary_split_index(&messages), Some(6));
    }

    #[test]
    fn summary_split_does_not_cut_into_active_tool_sequence() {
        let messages = vec![
            ApiMessage::system("system"),
            ApiMessage::user("environment"),
            ApiMessage::user("old user"),
            ApiMessage::assistant("old assistant"),
            ApiMessage::user("current user"),
            tool_calls(),
            ApiMessage::tool_result("call_1", "fs_read", "content"),
            ApiMessage::tool_result("call_2", "grep_search", "matches"),
            ApiMessage::assistant("after tools"),
            ApiMessage::user("follow up"),
            ApiMessage::assistant("answer"),
            ApiMessage::user("tail"),
        ];

        assert_eq!(summary_split_index(&messages), Some(4));
    }

    #[test]
    fn summary_split_returns_none_when_only_tool_sequence_would_be_summarized() {
        let messages = vec![
            ApiMessage::system("system"),
            ApiMessage::user("environment"),
            tool_calls(),
            ApiMessage::tool_result("call_1", "fs_read", "content"),
            ApiMessage::tool_result("call_2", "grep_search", "matches"),
            ApiMessage::assistant("after tools"),
            ApiMessage::user("follow up"),
            ApiMessage::assistant("answer"),
            ApiMessage::user("tail"),
        ];

        assert_eq!(summary_split_index(&messages), None);
    }
}
