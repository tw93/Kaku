// Shared library target for kaku-gui: exposes non-GUI modules to the `k` CLI binary.
// GUI-only modules (overlay, termwindow, renderstate, etc.) are not included here.
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_closure)]

// Register the i18n bundle for the lib half of `kaku-gui`. This is what
// the `k` CLI binary (bin/k.rs) and `cli_chat` rely on. The main binary
// registers its own copy in `main.rs`; rust-i18n requires per-crate
// registration but shares a process-wide locale via `set_locale`.
//
// rust-i18n resolves the path relative to the crate's Cargo.toml, so all
// three call sites (kaku/src/main.rs, kaku-gui/src/main.rs, this file)
// use the same `../locales` even though they live at different depths.
rust_i18n::i18n!("../locales", fallback = "en");

pub mod ai_chat_engine;
pub mod ai_client;
pub mod ai_conversations;
pub mod ai_tools;
pub mod cli_chat;
pub mod soul;

mod ai_auth;
pub mod thread_util;
