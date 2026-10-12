//! User-visible strings for the Cmd+L AI overlay.
//!
//! Centralized so a brand / wording change is a single-file edit instead
//! of a `grep` across `mod.rs`. Keep entries narrow (labels, headers,
//! toast titles); long-form templates that interpolate values still live
//! next to their `format!` call sites.

/// Label printed at the top of a user-authored message.
///
/// Matches what `cmd_export` writes as `User:` on disk; the overlay
/// prefers the shorter "You" because horizontal space is tight.
pub(crate) fn header_user() -> String {
    "  You".to_string()
}

/// Label printed at the top of an assistant-authored message.
pub(crate) fn header_assistant() -> String {
    "  AI".to_string()
}

/// Title shown by the system notification when an approval is required
/// and the Kaku window is unfocused.
pub(crate) fn approval_notification_title() -> String {
    "Kaku AI Needs Confirmation".to_string()
}

/// Title shown by the system notification when a chat task finishes
/// while the Kaku window is unfocused.
pub(crate) fn task_complete_notification_title() -> String {
    "Kaku AI Task Complete".to_string()
}

/// Body shown by the task-complete system notification.
pub(crate) fn task_complete_notification_body() -> String {
    "Response ready.".to_string()
}
