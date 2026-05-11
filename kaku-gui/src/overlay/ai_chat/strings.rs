//! User-visible strings for the Cmd+L AI overlay.
//!
//! Backed by `rust-i18n`. The accessor functions return `String` (not
//! `&'static str`) because the underlying lookup is runtime; callers
//! should clone-on-use rather than try to borrow these for `'static`
//! contexts. Long-form templates that interpolate values still live next
//! to their `format!` call sites — extract them only when they ship as
//! standalone labels.
//!
//! Translation source: `locales/{en,zh-CN}.yml`, scope `ai`.

use rust_i18n::t;

/// Label printed at the top of a user-authored message.
///
/// Matches what `cmd_export` writes as `User:` on disk; the overlay
/// prefers the shorter "You" / "你" because horizontal space is tight.
pub(crate) fn header_user() -> String {
    t!("ai.header.user").into_owned()
}

/// Label printed at the top of an assistant-authored message.
pub(crate) fn header_assistant() -> String {
    t!("ai.header.assistant").into_owned()
}

/// Title shown by the system notification when an approval is required
/// and the Kaku window is unfocused.
pub(crate) fn approval_notification_title() -> String {
    t!("ai.approval.title").into_owned()
}
