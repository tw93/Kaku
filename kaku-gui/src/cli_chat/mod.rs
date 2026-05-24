//! Standalone CLI renderer for the `k` command.
//!
//! Interactive mode uses an alternate-screen TUI via termwiz UnixTerminal.
//! One-shot mode streams tokens directly to stdout (pipe-friendly).

use crate::ai_chat_engine::{Engine, StreamMsg};
use crate::ai_conversations;
use std::io::Write;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::Duration;
use termwiz::caps::Capabilities;
use termwiz::cell::CellAttributes;
use termwiz::color::{AnsiColor, ColorAttribute};
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

// ── Entry point ───────────────────────────────────────────────────────────────

pub struct CliArgs {
    /// If Some, run a single one-shot query then exit.
    pub prompt: Option<String>,
    /// Force a new conversation even when a cwd mapping exists.
    pub new: bool,
    /// List recent conversations and optionally resume by ID.
    pub resume: Option<Option<String>>,
}

/// Main entry point for the `k` binary.
pub fn run(args: CliArgs) -> anyhow::Result<()> {
    // When running inside a Kaku pane with no extra args, trigger the Cmd+L
    // overlay directly via OSC 1337 SetUserVar and exit. This gives the exact
    // same experience as pressing Cmd+L from the keyboard.
    let inside_kaku = std::env::var_os("KAKU_UNIX_SOCKET").is_some();
    if inside_kaku && args.prompt.is_none() && !args.new && args.resume.is_none() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;
        use std::io::Write;
        // The receiving terminal suppresses SetUserVar alerts when the decoded
        // value matches the previous one, so each invocation needs a unique
        // payload to re-trigger the overlay after the user closed it.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let encoded = STANDARD.encode(nonce.to_string());
        print!("\x1b]1337;SetUserVar=kaku_open_ai_chat={}\x07", encoded);
        let _ = std::io::stdout().flush();
        return Ok(());
    }

    // Load assistant config.
    let cfg = crate::ai_client::AssistantConfig::load()?;
    let model = cfg.chat_model.clone();
    let client = crate::ai_client::AiClient::new(cfg);

    // Handle --resume: list or switch.
    if let Some(resume_arg) = args.resume {
        let convs = ai_conversations::load_index();
        if convs.is_empty() {
            eprintln!("No conversations found.");
            return Ok(());
        }
        if let Some(id) = resume_arg {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut engine = Engine::with_conv_id(cwd.clone(), client, model, &id)?;
            let _ = ai_conversations::write_cwd_index(&cwd, &id);
            let result = run_repl(&mut engine);
            crate::ai_tools::cleanup_spill_files();
            return result;
        } else {
            print_recent_conversations();
            return Ok(());
        }
    }

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Resolve or create a conversation for this cwd.
    let conv_id = if args.new {
        ai_conversations::start_new_active()?
    } else {
        ai_conversations::resolve_or_create_conv_for_cwd(&cwd)?
    };

    let mut engine = Engine::with_conv_id(cwd.clone(), client, model, &conv_id)?;
    // Keep the cwd index up to date in case this is a newly created conv.
    let _ = ai_conversations::write_cwd_index(&cwd, &engine.active_id);

    let result = if let Some(prompt) = args.prompt {
        // One-shot mode: stream tokens to stdout (pipe-friendly).
        run_one_shot(&mut engine, prompt)
    } else {
        // Interactive TUI.
        run_repl(&mut engine)
    };

    crate::ai_tools::cleanup_spill_files();
    result
}

// ── One-shot (pipe-friendly) ──────────────────────────────────────────────────

fn run_one_shot(engine: &mut Engine, prompt: String) -> anyhow::Result<()> {
    let rx = engine.submit(prompt);
    let mut assistant_buf = String::new();
    let mut reasoning_buf = String::new();
    let stdout = std::io::stdout();

    for msg in rx {
        match msg {
            StreamMsg::AssistantStart => {}
            StreamMsg::Token(t) => {
                assistant_buf.push_str(&t);
                let mut out = stdout.lock();
                let _ = out.write_all(t.as_bytes());
                let _ = out.flush();
            }
            StreamMsg::Reasoning(t) => {
                reasoning_buf.push_str(&t);
            }
            StreamMsg::ToolStart { name, args_preview } => {
                eprintln!("  -> {} {}", name, args_preview);
            }
            StreamMsg::ToolDone { result_preview } => {
                eprintln!("     <- {}", result_preview);
            }
            StreamMsg::ToolFailed { error } => {
                eprintln!("     [error] {}", error);
            }
            StreamMsg::ApprovalRequired { summary, reply_tx } => {
                eprint!("Allow: {}? [y/N] ", summary);
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                let approved = std::io::stdin()
                    .read_line(&mut line)
                    .map(|_| line.trim().to_lowercase() == "y")
                    .unwrap_or(false);
                let _ = reply_tx.send(approved);
            }
            StreamMsg::Done => {
                println!();
                break;
            }
            StreamMsg::Err(e) => {
                eprintln!("\n[error] {}", e);
                break;
            }
        }
    }

    if !assistant_buf.is_empty() {
        engine.record_assistant_with_reasoning(assistant_buf, reasoning_buf);
        engine.spawn_post_round_tasks();
    }

    Ok(())
}

// ── TUI REPL (alternate screen) ───────────────────────────────────────────────

#[derive(Clone, Copy)]
struct ThemeColors {
    header_bg: ColorAttribute,
    header_fg: ColorAttribute,
    user_label: ColorAttribute,
    ai_label: ColorAttribute,
    system_text: ColorAttribute,
    separator: ColorAttribute,
}

fn detect_theme() -> ThemeColors {
    let is_dark = detect_dark_mode();
    if is_dark {
        ThemeColors {
            header_bg: ColorAttribute::PaletteIndex(8), // bright black / dark gray
            header_fg: ColorAttribute::PaletteIndex(AnsiColor::White as u8),
            user_label: ColorAttribute::PaletteIndex(AnsiColor::Lime as u8),
            ai_label: ColorAttribute::PaletteIndex(AnsiColor::Aqua as u8),
            system_text: ColorAttribute::PaletteIndex(AnsiColor::Grey as u8),
            separator: ColorAttribute::PaletteIndex(8),
        }
    } else {
        ThemeColors {
            header_bg: ColorAttribute::PaletteIndex(AnsiColor::White as u8),
            header_fg: ColorAttribute::PaletteIndex(AnsiColor::Black as u8),
            user_label: ColorAttribute::PaletteIndex(AnsiColor::Green as u8),
            ai_label: ColorAttribute::PaletteIndex(AnsiColor::Teal as u8),
            system_text: ColorAttribute::PaletteIndex(AnsiColor::Grey as u8),
            separator: ColorAttribute::PaletteIndex(AnsiColor::Grey as u8),
        }
    }
}

fn detect_dark_mode() -> bool {
    // COLORFGBG is "fg;bg" where bg < 7 typically means dark background.
    if let Ok(val) = std::env::var("COLORFGBG") {
        if let Some(bg) = val.rsplit(';').next().and_then(|s| s.parse::<u8>().ok()) {
            return bg < 7;
        }
    }
    // Most modern terminals default to dark; assume dark when unknown.
    true
}

#[derive(Clone)]
enum KMsg {
    User(String),
    Ai(String),
    System(String),
}

struct Tui {
    messages: Vec<KMsg>,
    input: String,
    input_cursor: usize,
    /// Streaming AI response buffer (appended as tokens arrive).
    streaming_buf: String,
    is_streaming: bool,
    stream_rx: Option<Receiver<StreamMsg>>,
    /// Pending assistant buffer for record_assistant after Done.
    pending_assistant: String,
    pending_reasoning: String,
    /// Pending approval request waiting for user y/N.
    pending_approval: Option<(String, SyncSender<bool>)>,
    scroll_offset: usize,
    cols: usize,
    rows: usize,
    theme: ThemeColors,
}

impl Tui {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            messages: vec![KMsg::System(
                "k -- /new  /resume [id]  /clear  /status  /memory  /exit".into(),
            )],
            input: String::new(),
            input_cursor: 0,
            streaming_buf: String::new(),
            is_streaming: false,
            stream_rx: None,
            pending_assistant: String::new(),
            pending_reasoning: String::new(),
            pending_approval: None,
            scroll_offset: 0,
            cols,
            rows,
            theme: detect_theme(),
        }
    }

    /// Wrap text at `width`, returning a list of display lines.
    fn wrap(text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![text.to_string()];
        }
        let mut lines = Vec::new();
        for raw_line in text.split('\n') {
            if raw_line.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut cur = String::new();
            let mut cur_width = 0usize;
            for word in raw_line.split_inclusive(' ') {
                let wlen = word.chars().count();
                if cur_width + wlen > width && !cur.is_empty() {
                    lines.push(cur.trim_end().to_string());
                    cur = String::new();
                    cur_width = 0;
                }
                cur.push_str(word);
                cur_width += wlen;
            }
            if !cur.is_empty() {
                lines.push(cur.trim_end().to_string());
            }
        }
        lines
    }

    /// All display lines (message renders + streaming partial).
    fn all_display_lines(&self) -> Vec<(ColorAttribute, String)> {
        let inner_w = self.cols.saturating_sub(2);
        let mut out: Vec<(ColorAttribute, String)> = Vec::new();
        let theme = &self.theme;

        for msg in &self.messages {
            match msg {
                KMsg::System(s) => {
                    out.push((theme.system_text, format!("  {}", s)));
                }
                KMsg::User(s) => {
                    out.push((theme.user_label, "  You".to_string()));
                    for line in Self::wrap(s, inner_w.saturating_sub(2)) {
                        out.push((ColorAttribute::Default, format!("  {}", line)));
                    }
                    out.push((ColorAttribute::Default, String::new()));
                }
                KMsg::Ai(s) => {
                    out.push((theme.ai_label, "  AI".to_string()));
                    for line in Self::wrap(s, inner_w.saturating_sub(2)) {
                        out.push((ColorAttribute::Default, format!("  {}", line)));
                    }
                    out.push((ColorAttribute::Default, String::new()));
                }
            }
        }

        // Streaming partial.
        if self.is_streaming || !self.streaming_buf.is_empty() {
            out.push((theme.ai_label, "  AI".to_string()));
            if self.streaming_buf.is_empty() && !self.pending_reasoning.is_empty() {
                out.push((theme.system_text, "  Thinking...".to_string()));
            } else {
                for line in Self::wrap(&self.streaming_buf, inner_w.saturating_sub(2)) {
                    out.push((ColorAttribute::Default, format!("  {}", line)));
                }
            }
        }

        out
    }

    /// Drain one batch of stream messages; returns true if anything changed.
    fn drain_stream(&mut self) -> bool {
        let rx = match self.stream_rx.take() {
            Some(r) => r,
            None => return false,
        };

        let mut changed = false;

        loop {
            match rx.try_recv() {
                Ok(StreamMsg::Token(t)) => {
                    self.streaming_buf.push_str(&t);
                    self.pending_assistant.push_str(&t);
                    changed = true;
                }
                Ok(StreamMsg::Reasoning(t)) => {
                    let should_show_status =
                        self.streaming_buf.is_empty() && self.pending_reasoning.is_empty();
                    self.pending_reasoning.push_str(&t);
                    if should_show_status {
                        changed = true;
                    }
                }
                Ok(StreamMsg::AssistantStart) => {}
                Ok(StreamMsg::ToolStart { name, args_preview }) => {
                    self.streaming_buf
                        .push_str(&format!("\n[tool: {} {}]\n", name, args_preview));
                    changed = true;
                }
                Ok(StreamMsg::ToolDone { result_preview }) => {
                    self.streaming_buf
                        .push_str(&format!("[done: {}]\n", result_preview));
                    changed = true;
                }
                Ok(StreamMsg::ToolFailed { error }) => {
                    self.streaming_buf
                        .push_str(&format!("[error: {}]\n", error));
                    changed = true;
                }
                Ok(StreamMsg::ApprovalRequired { summary, reply_tx }) => {
                    self.streaming_buf
                        .push_str(&format!("[approve: {}? y/N] ", summary));
                    self.pending_approval = Some((summary, reply_tx));
                    changed = true;
                    self.stream_rx = Some(rx);
                    return changed;
                }
                Ok(StreamMsg::Done) => {
                    let ai_text = std::mem::take(&mut self.streaming_buf);
                    if !ai_text.is_empty() {
                        self.messages.push(KMsg::Ai(ai_text));
                    }
                    self.is_streaming = false;
                    changed = true;
                    // Don't put rx back; stream is finished.
                    return changed;
                }
                Ok(StreamMsg::Err(e)) => {
                    let ai_text = std::mem::take(&mut self.streaming_buf);
                    if !ai_text.is_empty() {
                        self.messages.push(KMsg::Ai(ai_text));
                    }
                    self.messages.push(KMsg::System(format!("[error] {}", e)));
                    self.is_streaming = false;
                    changed = true;
                    return changed;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.stream_rx = Some(rx);
                    return changed;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.is_streaming = false;
                    if !self.streaming_buf.is_empty() {
                        let ai_text = std::mem::take(&mut self.streaming_buf);
                        self.messages.push(KMsg::Ai(ai_text));
                    }
                    changed = true;
                    return changed;
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        let display_rows = self.rows.saturating_sub(3); // header + input
        let total = self.all_display_lines().len();
        if total > display_rows {
            self.scroll_offset = total - display_rows;
        } else {
            self.scroll_offset = 0;
        }
    }
}

#[allow(clippy::result_large_err)]
fn render_tui(term: &mut dyn Terminal, tui: &Tui) -> termwiz::Result<()> {
    let cols = tui.cols;
    let rows = tui.rows;

    // Synchronize output: hide cursor, clear screen.
    let mut changes: Vec<Change> = vec![
        Change::Text("\x1b[?2026h".to_string()),
        Change::CursorVisibility(CursorVisibility::Hidden),
        Change::AllAttributes(CellAttributes::default()),
        Change::ClearScreen(ColorAttribute::Default),
    ];

    // Header row.
    let theme = &tui.theme;
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(0),
    });
    changes.push(Change::AllAttributes({
        let mut a = CellAttributes::default();
        a.set_background(theme.header_bg);
        a.set_foreground(theme.header_fg);
        a
    }));
    let header = format!(" k  AI Chat {}", " ".repeat(cols.saturating_sub(10)));
    changes.push(Change::Text(truncate_to_chars(&header, cols)));

    // Message area: rows 1 .. rows-2.
    let display_rows = rows.saturating_sub(3);
    let all_lines = tui.all_display_lines();
    let start = tui.scroll_offset.min(all_lines.len().saturating_sub(1));
    let visible = &all_lines[start..];

    for (i, (color, text)) in visible.iter().take(display_rows).enumerate() {
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(i + 1),
        });
        let mut attr = CellAttributes::default();
        attr.set_foreground(*color);
        changes.push(Change::AllAttributes(attr));
        let padded = format!("{:<width$}", text, width = cols);
        changes.push(Change::Text(truncate_to_chars(&padded, cols)));
    }

    // Separator row.
    let sep_row = rows.saturating_sub(2);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(sep_row),
    });
    changes.push(Change::AllAttributes({
        let mut a = CellAttributes::default();
        a.set_foreground(theme.separator);
        a
    }));
    changes.push(Change::Text("─".repeat(cols)));

    // Input row.
    let input_row = rows.saturating_sub(1);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(input_row),
    });
    changes.push(Change::AllAttributes(CellAttributes::default()));
    let prompt_prefix = "> ";
    let display_input = format!("{}{}", prompt_prefix, tui.input);
    let padded_input = format!("{:<width$}", display_input, width = cols);
    changes.push(Change::Text(truncate_to_chars(&padded_input, cols)));

    // Place cursor in input row.
    let cursor_x = (prompt_prefix.len() + tui.input_cursor).min(cols.saturating_sub(1));
    changes.push(Change::CursorPosition {
        x: Position::Absolute(cursor_x),
        y: Position::Absolute(input_row),
    });
    changes.push(Change::CursorVisibility(CursorVisibility::Visible));
    changes.push(Change::Text("\x1b[?2026l".to_string()));

    term.render(&changes)?;
    Ok(())
}

fn run_repl(engine: &mut Engine) -> anyhow::Result<()> {
    use termwiz::terminal::UnixTerminal;

    let caps = Capabilities::new_from_env()?;
    let mut term = UnixTerminal::new(caps)?;
    term.enter_alternate_screen()?;
    term.set_raw_mode()?;

    let size = term.get_screen_size()?;
    let mut tui = Tui::new(size.cols, size.rows);

    // Show existing conversation history.
    for m in &engine.messages {
        if m.role == "user" {
            tui.messages.push(KMsg::User(m.content.clone()));
        } else if m.role == "assistant" {
            tui.messages.push(KMsg::Ai(m.content.clone()));
        }
    }
    tui.scroll_to_bottom();

    let mut needs_redraw = true;

    loop {
        if tui.drain_stream() {
            tui.scroll_to_bottom();
            needs_redraw = true;

            // After streaming done, record assistant response.
            if !tui.is_streaming && !tui.pending_assistant.is_empty() {
                let buf = std::mem::take(&mut tui.pending_assistant);
                let reasoning = std::mem::take(&mut tui.pending_reasoning);
                engine.record_assistant_with_reasoning(buf, reasoning);
                engine.spawn_post_round_tasks();
            }
        }

        if needs_redraw {
            let _ = render_tui(&mut term, &tui);
            needs_redraw = false;
        }

        let timeout = if tui.is_streaming {
            Some(Duration::from_millis(30))
        } else {
            Some(Duration::from_millis(200))
        };

        match term.poll_input(timeout)? {
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char('c'),
                modifiers: Modifiers::CTRL,
            }))
            | Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                modifiers: Modifiers::NONE,
            })) => {
                if let Some((_summary, reply_tx)) = tui.pending_approval.take() {
                    let _ = reply_tx.send(false);
                    tui.streaming_buf.push_str("[denied]\n");
                    needs_redraw = true;
                } else if tui.is_streaming {
                    engine.cancel();
                    tui.stream_rx = None;
                    tui.is_streaming = false;
                    if !tui.streaming_buf.is_empty() {
                        let ai_text = std::mem::take(&mut tui.streaming_buf);
                        tui.messages.push(KMsg::Ai(ai_text));
                    }
                    needs_redraw = true;
                } else {
                    break;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char(ch),
                ..
            })) if tui.pending_approval.is_some() => {
                let (summary, reply_tx) = tui.pending_approval.take().unwrap();
                let approved = ch == 'y' || ch == 'Y';
                let _ = reply_tx.send(approved);
                let label = if approved { "approved" } else { "denied" };
                tui.streaming_buf
                    .push_str(&format!("[{}: {}]\n", label, summary));
                needs_redraw = true;
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Enter,
                modifiers: Modifiers::NONE,
            })) => {
                let line = std::mem::take(&mut tui.input);
                tui.input_cursor = 0;
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }

                // Slash command handling.
                if trimmed.starts_with('/') {
                    let mut parts = trimmed.splitn(2, ' ');
                    let cmd = parts.next().unwrap_or("");
                    let rest = parts.next().unwrap_or("").trim();
                    match cmd {
                        "/exit" | "/quit" => break,
                        "/new" | "/clear" => match engine.start_new() {
                            Ok(_) => {
                                let _ = ai_conversations::write_cwd_index(
                                    &engine.cwd,
                                    &engine.active_id,
                                );
                                let label = if cmd == "/new" {
                                    "new conversation started"
                                } else {
                                    "conversation cleared"
                                };
                                tui.messages.push(KMsg::System(label.into()));
                            }
                            Err(e) => {
                                tui.messages.push(KMsg::System(format!("[error] {}", e)));
                            }
                        },
                        "/resume" => {
                            if rest.is_empty() {
                                let convs = ai_conversations::load_index();
                                if convs.is_empty() {
                                    tui.messages.push(KMsg::System("No conversations.".into()));
                                } else {
                                    for (i, c) in convs.iter().take(10).enumerate() {
                                        let summary = if c.summary.trim().is_empty() {
                                            "(no summary)"
                                        } else {
                                            c.summary.as_str()
                                        };
                                        tui.messages.push(KMsg::System(format!(
                                            "  {}  {}  {}",
                                            i + 1,
                                            c.id,
                                            summary
                                        )));
                                    }
                                    tui.messages
                                        .push(KMsg::System("Use /resume <id> to switch.".into()));
                                }
                            } else {
                                match engine.switch_to(rest) {
                                    Ok(_) => {
                                        tui.messages
                                            .push(KMsg::System(format!("switched to {}", rest)));
                                    }
                                    Err(e) => {
                                        tui.messages.push(KMsg::System(format!("[error] {}", e)));
                                    }
                                }
                            }
                        }
                        "/status" => {
                            let turns = engine.messages.iter().filter(|m| m.role == "user").count();
                            tui.messages.push(KMsg::System(format!(
                                "conv: {}  turns: {}  cwd: {}",
                                engine.active_id, turns, engine.cwd
                            )));
                        }
                        "/memory" => {
                            let path = crate::soul::memory_path();
                            match std::fs::read_to_string(&path) {
                                Ok(contents) if !contents.trim().is_empty() => {
                                    tui.messages.push(KMsg::System(contents));
                                }
                                Ok(_) => {
                                    tui.messages.push(KMsg::System("[no memories yet]".into()));
                                }
                                Err(_) => {
                                    tui.messages.push(KMsg::System(format!(
                                        "[no memory file at {}]",
                                        path.display()
                                    )));
                                }
                            }
                        }
                        other => {
                            tui.messages
                                .push(KMsg::System(format!("[unknown command: {}]", other)));
                        }
                    }
                    tui.scroll_to_bottom();
                    needs_redraw = true;
                    continue;
                }

                // Regular prompt: push user message and start streaming.
                tui.messages.push(KMsg::User(trimmed.clone()));
                tui.is_streaming = true;
                tui.streaming_buf.clear();
                tui.pending_assistant.clear();
                tui.pending_reasoning.clear();
                tui.stream_rx = Some(engine.submit(trimmed));
                tui.scroll_to_bottom();
                needs_redraw = true;
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Backspace,
                ..
            })) => {
                if tui.input_cursor > 0 {
                    tui.input_cursor -= 1;
                    let byte_pos = char_to_byte(&tui.input, tui.input_cursor);
                    tui.input.remove(byte_pos);
                    needs_redraw = true;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::LeftArrow,
                ..
            })) => {
                if tui.input_cursor > 0 {
                    tui.input_cursor -= 1;
                    needs_redraw = true;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::RightArrow,
                ..
            })) => {
                if tui.input_cursor < tui.input.chars().count() {
                    tui.input_cursor += 1;
                    needs_redraw = true;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::UpArrow,
                ..
            })) => {
                if tui.scroll_offset > 0 {
                    tui.scroll_offset -= 1;
                    needs_redraw = true;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::DownArrow,
                ..
            })) => {
                let total = tui.all_display_lines().len();
                let display_rows = tui.rows.saturating_sub(3);
                if tui.scroll_offset + display_rows < total {
                    tui.scroll_offset += 1;
                    needs_redraw = true;
                }
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Home, ..
            })) => {
                tui.scroll_offset = 0;
                needs_redraw = true;
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::End, ..
            })) => {
                tui.scroll_to_bottom();
                needs_redraw = true;
            }
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char(c),
                modifiers,
            })) if modifiers == Modifiers::NONE || modifiers == Modifiers::SHIFT => {
                let byte_pos = char_to_byte(&tui.input, tui.input_cursor);
                tui.input.insert(byte_pos, c);
                tui.input_cursor += 1;
                needs_redraw = true;
            }
            Some(InputEvent::Resized { cols, rows }) => {
                tui.cols = cols;
                tui.rows = rows;
                tui.scroll_to_bottom();
                needs_redraw = true;
            }
            Some(_) | None => {
                if tui.is_streaming {
                    needs_redraw = true;
                }
            }
        }
    }

    let _ = term.set_cooked_mode();
    let _ = term.exit_alternate_screen();

    Ok(())
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn truncate_to_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_recent_conversations() {
    let convs = ai_conversations::load_index();
    if convs.is_empty() {
        eprintln!("No conversations.");
        return;
    }
    eprintln!("Recent conversations:");
    for (i, c) in convs.iter().take(10).enumerate() {
        let summary = if c.summary.trim().is_empty() {
            "(no summary)"
        } else {
            c.summary.as_str()
        };
        eprintln!("  {}  {}  {}", i + 1, c.id, summary);
    }
    eprintln!("Use /resume <id> to switch.");
}

#[cfg(test)]
mod tests {
    use super::{truncate_to_chars, KMsg, Tui};
    use crate::ai_chat_engine::StreamMsg;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn truncate_to_chars_handles_utf8_boundaries() {
        let s = "k 你好 world";
        assert_eq!(truncate_to_chars(s, 0), "");
        assert_eq!(truncate_to_chars(s, 3), "k 你");
        assert_eq!(truncate_to_chars(s, 5), "k 你好 ");
    }

    #[test]
    fn reasoning_stream_shows_compact_status_without_visible_text() {
        let (tx, rx) = sync_channel(4);
        let mut tui = Tui::new(80, 24);
        tui.is_streaming = true;
        tui.stream_rx = Some(rx);

        tx.send(StreamMsg::Reasoning("hidden thought".to_string()))
            .unwrap();

        assert!(tui.drain_stream());
        assert!(tui.streaming_buf.is_empty());
        assert!(tui.pending_assistant.is_empty());
        assert_eq!(tui.pending_reasoning, "hidden thought");

        let lines = tui.all_display_lines();
        assert!(lines.iter().any(|(_, text)| text == "  Thinking..."));
        assert!(!lines
            .iter()
            .any(|(_, text)| text.contains("hidden thought")));
    }

    #[test]
    fn reasoning_only_completion_does_not_record_empty_ai_message() {
        let (tx, rx) = sync_channel(4);
        let mut tui = Tui::new(80, 24);
        tui.is_streaming = true;
        tui.stream_rx = Some(rx);

        tx.send(StreamMsg::Reasoning("hidden thought".to_string()))
            .unwrap();
        tx.send(StreamMsg::Done).unwrap();

        assert!(tui.drain_stream());
        assert!(!tui.is_streaming);
        assert!(tui.pending_assistant.is_empty());
        assert!(tui.messages.iter().all(|msg| match msg {
            KMsg::Ai(text) => !text.is_empty(),
            _ => true,
        }));
    }
}
