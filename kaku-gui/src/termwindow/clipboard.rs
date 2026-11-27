use crate::ssh_image_paste::{SshTarget, LATE_FALLBACK};
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use config::keyassignment::{ClipboardCopyDestination, ClipboardPasteSource};
use mux::pane::{CachePolicy, Pane};
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

fn is_ai_progress_toast(message: &str) -> bool {
    matches!(
        message,
        "Kaku Assistant analyzing command" | "Kaku generating command"
    )
}

impl TermWindow {
    pub fn copy_to_clipboard(&self, clipboard: ClipboardCopyDestination, text: String) {
        let text = if self.config.copy_strip_leading_whitespace {
            strip_common_leading_whitespace(&text)
        } else {
            text
        };
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

    pub fn clear_ai_progress_toast(&mut self) {
        let is_progress = self
            .toast
            .as_ref()
            .map(|(_, message, _)| is_ai_progress_toast(message))
            .unwrap_or(false);
        if !is_progress {
            return;
        }

        self.toast = None;
        self.toast_shaped_width = None;
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
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
        let target_pane = pane.clone();
        let pane_id = target_pane.pane_id();
        log::trace!("paste_from_clipboard in pane {pane_id} {clipboard:?}");
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
                    window.notify(TermWindowNotif::Apply(Box::new(move |myself| {
                        if let window::ClipboardData::Image(image) = &data {
                            // ssh has no clipboard on the far side for Ctrl+V
                            // to read, so copy the image there instead (#634).
                            if let Some(ssh) = target_pane
                                .get_foreground_process_info(CachePolicy::FetchImmediate)
                                .and_then(|p| {
                                    SshTarget::from_process(&p.executable, &p.argv, p.pid, &p.cwd)
                                })
                            {
                                myself.paste_image_over_ssh(&target_pane, ssh, image.clone());
                                return;
                            }
                            // Clipboard holds an image, not text.  Instead of
                            // pasting the temp-file path (which confuses TUI
                            // apps), forward a Ctrl+V byte so the TUI app can
                            // read the system clipboard image itself, using the same
                            // path that a real Ctrl+V keypress takes.
                            let result = target_pane.writer().write_all(b"\x16");
                            if let Err(err) = myself.finish_terminal_input(&target_pane, result) {
                                log::warn!(
                                    "failed to send ctrl-v for image paste to pane {pane_id}: {err:#}"
                                );
                            }
                            return;
                        }
                        let clip = match data_to_paste_string(data, quote_dropped_files) {
                            Some(clip) => clip,
                            None => return,
                        };

                        let result = target_pane.send_paste(&clip);
                        if let Err(err) = myself.finish_terminal_input(&target_pane, result) {
                            log::warn!(
                                "failed to paste clipboard content into pane {pane_id}: {err:#}"
                            );
                        }
                    })));
                }
                Err(err) => {
                    log::warn!("failed to read clipboard for pane {pane_id}: {err:#}");
                }
            }
        })
        .detach();
        self.maybe_scroll_to_bottom_for_input(&pane);
    }

    /// Uploads `image` to the host the pane's `ssh` is connected to, then
    /// pastes its remote path. If the upload fails, a toast says why and the
    /// pane gets the Ctrl+V a local image paste sends, unless the failure
    /// came so late that the Ctrl+V would land in whatever the user is doing
    /// by then.
    fn paste_image_over_ssh(&mut self, pane: &Arc<dyn Pane>, ssh: SshTarget, image: PathBuf) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let pane = pane.clone();
        std::thread::spawn(move || {
            // ssh_config can start another program first (`RemoteCommand
            // tmux attach`, a further ssh); the image would not land where
            // the TUI reads it, so keep the plain Ctrl+V.
            let outcome = if ssh.configured_remote_command() {
                SshPasteOutcome::CtrlV
            } else {
                let started = Instant::now();
                let notice_window = window.clone();
                let host = ssh.host.clone();
                let result = ssh.upload(&image, move || {
                    notice_window.notify(TermWindowNotif::Apply(Box::new(move |myself| {
                        myself.show_toast_for(format!("{SSH_UPLOAD_TOAST} {host}…"), 15_000);
                    })));
                });
                match result {
                    Ok(remote_path) => SshPasteOutcome::Path(remote_path),
                    Err(err) => SshPasteOutcome::Failed {
                        err,
                        late: started.elapsed() > LATE_FALLBACK,
                    },
                }
            };
            window.notify(TermWindowNotif::Apply(Box::new(move |myself| {
                let pane_id = pane.pane_id();
                let uploading = myself.toast.as_ref().map_or(false, |(_, message, _)| {
                    message.starts_with(SSH_UPLOAD_TOAST)
                });
                if uploading {
                    myself.toast = None;
                    if let Some(window) = myself.window.as_ref() {
                        window.invalidate();
                    }
                }
                // The upload can take seconds; if that ssh has exited or another
                // program took over the pane, typing there is wrong.
                let same_session = pane
                    .get_foreground_process_info(CachePolicy::FetchImmediate)
                    .map_or(false, |p| p.pid == ssh.pid);
                if !same_session {
                    log::warn!(
                        "image paste to {} finished after its ssh session left pane {pane_id}",
                        ssh.host
                    );
                    return;
                }
                let written: anyhow::Result<()> = match outcome {
                    SshPasteOutcome::Path(remote_path) => pane.send_paste(&remote_path),
                    SshPasteOutcome::CtrlV => pane.writer().write_all(b"\x16").map_err(Into::into),
                    SshPasteOutcome::Failed { err, late } => {
                        log::warn!("image paste over ssh to {} failed: {err:#}", ssh.host);
                        myself
                            .show_toast(format!("Couldn't copy the image to {}: {err}", ssh.host));
                        if late {
                            Ok(())
                        } else {
                            pane.writer().write_all(b"\x16").map_err(Into::into)
                        }
                    }
                };
                if let Err(err) = myself.finish_terminal_input(&pane, written) {
                    log::warn!("failed to paste image into pane {pane_id}: {err:#}");
                }
            })));
        });
    }
}

/// The start of the toast shown while a slow image upload runs; the result
/// clears it.
const SSH_UPLOAD_TOAST: &str = "Copying the image to";

enum SshPasteOutcome {
    /// The image is on the remote host at this path.
    Path(String),
    /// Send the Ctrl+V a local image paste sends.
    CtrlV,
    Failed {
        err: anyhow::Error,
        /// The failure took longer than `LATE_FALLBACK`.
        late: bool,
    },
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

fn strip_common_leading_whitespace(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim_start().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    if min_indent == 0 {
        return text.to_string();
    }
    lines
        .iter()
        .map(|line| {
            if line.trim_start().is_empty() {
                *line
            } else {
                &line[min_indent..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::is_ai_progress_toast;

    #[test]
    fn only_ai_progress_toasts_are_clearable() {
        assert!(is_ai_progress_toast("Kaku Assistant analyzing command"));
        assert!(is_ai_progress_toast("Kaku generating command"));
        assert!(!is_ai_progress_toast("Kaku Assistant is not configured"));
        assert!(!is_ai_progress_toast("Copied"));
    }
}
