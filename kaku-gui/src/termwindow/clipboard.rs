use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use config::keyassignment::{ClipboardCopyDestination, ClipboardPasteSource};
use mux::pane::Pane;
use smol::Timer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_toast_notification::persistent_toast_notification;
use window::{Clipboard, ClipboardData, WindowOps};

const AI_NOTICE_DEDUP_WINDOW: Duration = Duration::from_secs(2);
const AI_NOTICE_CACHE_RETENTION: Duration = Duration::from_secs(30);

lazy_static::lazy_static! {
    static ref AI_NOTICE_TIMESTAMPS: Mutex<HashMap<String, Instant>> = Mutex::new(HashMap::new());
}

fn should_emit_ai_notice(kind: &str, message: &str) -> bool {
    let key = format!("{kind}:{message}");
    let now = Instant::now();
    let mut guard = match AI_NOTICE_TIMESTAMPS.lock() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("AI notice dedup mutex poisoned, allowing duplicate: {}", e);
            return true;
        }
    };

    if let Some(last_seen) = guard.get(&key) {
        if now.duration_since(*last_seen) < AI_NOTICE_DEDUP_WINDOW {
            return false;
        }
    }

    guard.insert(key, now);
    guard.retain(|_, ts| now.duration_since(*ts) <= AI_NOTICE_CACHE_RETENTION);
    true
}

impl TermWindow {
    pub fn copy_to_clipboard_if_present(
        &self,
        clipboard: ClipboardCopyDestination,
        text: String,
    ) -> bool {
        let Some(text) = clipboard_text_if_present(text) else {
            return false;
        };
        self.copy_to_clipboard(clipboard, text);
        true
    }

    pub fn copy_to_clipboard(&self, clipboard: ClipboardCopyDestination, text: String) {
        let clipboard = match clipboard {
            ClipboardCopyDestination::Clipboard => [Some(Clipboard::Clipboard), None],
            ClipboardCopyDestination::PrimarySelection => [Some(Clipboard::PrimarySelection), None],
            ClipboardCopyDestination::ClipboardAndPrimarySelection => [
                Some(Clipboard::Clipboard),
                Some(Clipboard::PrimarySelection),
            ],
        };
        for &c in &clipboard {
            if let Some(c) = c {
                self.window.as_ref().unwrap().set_clipboard(c, text.clone());
            }
        }
    }

    fn show_toast_internal(&mut self, message: String, lifetime: Duration) {
        let now = Instant::now();
        let fade_after = lifetime.saturating_sub(Duration::from_millis(500));
        self.toast = Some((now, message, lifetime));
        if let Some(window) = self.window.clone() {
            let win = window.clone();
            // Trigger fade-out during the last 500ms.
            let fade_win = win.clone();
            promise::spawn::spawn(async move {
                Timer::after(fade_after).await;
                fade_win.invalidate();
            })
            .detach();
            // Clear when lifetime expires.
            promise::spawn::spawn(async move {
                Timer::after(lifetime).await;
                window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    if let Some((toast_time, _, _)) = &tw.toast {
                        if *toast_time == now {
                            tw.toast = None;
                        }
                    }
                    win.invalidate();
                })));
            })
            .detach();
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Show toast notification with a message (disappears after 2.5 seconds).
    /// Rapid consecutive calls are safe: each toast stores its creation `Instant`,
    /// so only the matching toast is cleared, and newer toasts naturally supersede older ones.
    pub fn show_toast(&mut self, message: String) {
        self.show_toast_internal(message, Duration::from_millis(2500));
    }

    /// Show toast notification with a custom lifetime in milliseconds.
    pub fn show_toast_for(&mut self, message: String, lifetime_ms: u64) {
        let clamped = lifetime_ms.clamp(800, 15000);
        self.show_toast_internal(message, Duration::from_millis(clamped));
    }

    /// Progress hints should stay local to the terminal surface and auto-dismiss.
    pub fn show_ai_progress_toast(&mut self, message: String, lifetime_ms: u64) {
        let normalized = message.trim().to_string();
        if normalized.is_empty() {
            return;
        }
        if !self.window_state.can_paint() {
            return;
        }
        if !should_emit_ai_notice("progress", &normalized) {
            return;
        }
        let clamped = lifetime_ms.clamp(1200, 8000);
        self.show_toast_internal(normalized, Duration::from_millis(clamped));
    }

    /// Result notices prefer in-window toast when the window is focused;
    /// fallback to system notification when in background/hidden.
    pub fn show_ai_result_notice(&mut self, message: String, lifetime_ms: u64) {
        let normalized = message.trim().to_string();
        if normalized.is_empty() {
            return;
        }
        if !should_emit_ai_notice("result", &normalized) {
            return;
        }

        let show_in_window = self.focused.is_some() && self.window_state.can_paint();
        if show_in_window {
            self.show_toast_for(normalized, lifetime_ms);
            return;
        }
        persistent_toast_notification("AI", &normalized);
    }

    /// Show "Copied" toast notification
    pub fn show_copy_toast(&mut self) {
        self.show_toast("Copied".to_string());
    }

    /// Explain once per window when auto-copy is intentionally disabled.
    pub fn show_copy_on_select_disabled_hint(&mut self) {
        if self.selection_copy_disabled_hint_shown {
            return;
        }
        self.selection_copy_disabled_hint_shown = true;
        self.show_toast_for("Auto copy disabled. Use Cmd+C to copy.".to_string(), 2200);
    }

    pub fn paste_from_clipboard(&mut self, pane: &Arc<dyn Pane>, clipboard: ClipboardPasteSource) {
        let targets = self.terminal_input_targets(pane);
        let pane_ids: Vec<_> = targets.iter().map(|pane| pane.pane_id()).collect();
        log::trace!(
            "paste_from_clipboard in panes {:?} {:?}",
            pane_ids,
            clipboard
        );
        let window = self.window.as_ref().unwrap().clone();
        let clipboard = match clipboard {
            ClipboardPasteSource::Clipboard => Clipboard::Clipboard,
            ClipboardPasteSource::PrimarySelection => Clipboard::PrimarySelection,
        };
        let quote_dropped_files = self.config.quote_dropped_files;
        let future = window.get_clipboard_data(clipboard);
        promise::spawn::spawn(async move {
            match future.await {
                Ok(data) => {
                    window.notify(TermWindowNotif::Apply(Box::new(move |_myself| {
                        if let window::ClipboardData::Image(_) = &data {
                            // Clipboard holds an image, not text.  Instead of
                            // pasting the temp-file path (which confuses TUI
                            // apps), forward a Ctrl+V byte so the TUI app can
                            // read the system clipboard image itself, using the same
                            // path that a real Ctrl+V keypress takes.
                            for pane in &targets {
                                if let Err(err) = pane.writer().write_all(b"\x16") {
                                    log::warn!(
                                        "failed to send ctrl-v for image paste to pane {}: {err:#}",
                                        pane.pane_id()
                                    );
                                }
                            }
                            return;
                        }
                        let clip = match data_to_paste_string(data, quote_dropped_files) {
                            Some(clip) => clip,
                            None => return,
                        };

                        for pane in &targets {
                            if let Err(err) = pane.send_paste(&clip) {
                                log::warn!(
                                    "failed to paste clipboard content into pane {}: {err:#}",
                                    pane.pane_id()
                                );
                            }
                        }
                    })));
                }
                Err(err) => {
                    log::warn!("failed to read clipboard for panes {:?}: {err:#}", pane_ids);
                }
            }
        })
        .detach();
        self.maybe_scroll_to_bottom_for_input(&pane);
    }
}

fn clipboard_text_if_present(text: String) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn data_to_paste_string(
    data: ClipboardData,
    quote_dropped_files: config::DroppedFileQuoting,
) -> Option<String> {
    match data {
        ClipboardData::Text(text) => Some(text),
        ClipboardData::Image(_) => None,
        ClipboardData::Files(paths) => {
            if paths.is_empty() {
                return None;
            }
            Some(format_dropped_paths(paths, quote_dropped_files))
        }
    }
}

fn format_dropped_paths(
    paths: Vec<PathBuf>,
    quote_dropped_files: config::DroppedFileQuoting,
) -> String {
    paths
        .iter()
        .map(|path| quote_path_for_clipboard_paste(path, quote_dropped_files))
        .collect::<Vec<_>>()
        .join(" ")
        + " " // Trailing space so the shell treats this as ready-to-append arguments.
}

#[cfg(test)]
mod tests {
    use super::clipboard_text_if_present;

    #[test]
    fn clipboard_text_if_present_skips_empty_strings() {
        assert_eq!(clipboard_text_if_present(String::new()), None);
    }

    #[test]
    fn clipboard_text_if_present_keeps_non_empty_strings() {
        assert_eq!(
            clipboard_text_if_present("selected text".to_string()),
            Some("selected text".to_string())
        );
    }
}

fn quote_path_for_clipboard_paste(
    path: &PathBuf,
    quote_dropped_files: config::DroppedFileQuoting,
) -> String {
    let path = path.to_string_lossy();
    match quote_dropped_files {
        config::DroppedFileQuoting::None => path.into_owned(),
        // Clipboard file paste used to be POSIX-quoted before image support was added.
        // Keep that safety baseline for default SpacesOnly mode.
        config::DroppedFileQuoting::SpacesOnly | config::DroppedFileQuoting::Posix => {
            let path_str = path.to_string();
            match shlex::try_quote(&path_str) {
                Ok(quoted) => quoted.into_owned(),
                Err(e) => {
                    log::warn!(
                        "Failed to quote path {:?} for clipboard paste: {}. Using as-is.",
                        path_str,
                        e
                    );
                    path_str
                }
            }
        }
        config::DroppedFileQuoting::Windows | config::DroppedFileQuoting::WindowsAlwaysQuoted => {
            quote_dropped_files.escape(path.as_ref())
        }
    }
}
