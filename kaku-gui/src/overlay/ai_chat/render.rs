use crate::ai_conversations;
use mux::termwiztermtab::TermWizTerminal;
use termwiz::cell::{unicode_column_width, AttributeChange, CellAttributes};
use termwiz::color::{ColorAttribute, SrgbaTuple};
use termwiz::surface::{Change, CursorShape, CursorVisibility, Position};
use termwiz::terminal::Terminal;

use super::layout::{byte_pos_at_visual_col, layout_input, pad_to_visual_width, truncate};
use super::state::App;
use super::strings;
use super::types::*;

/// Map a tool name to a human-readable verb. `in_progress` selects the
/// present participle; false selects the simple past.
fn tool_verb(name: &str, in_progress: bool) -> &str {
    match (name, in_progress) {
        ("fs_read", true) => "reading",
        ("fs_read", false) => "read",
        ("fs_write", true) => "writing",
        ("fs_write", false) => "wrote",
        ("fs_patch", true) => "patching",
        ("fs_patch", false) => "patched",
        ("fs_delete", true) => "deleting",
        ("fs_delete", false) => "deleted",
        ("fs_list", true) => "listing",
        ("fs_list", false) => "listed",
        ("grep_search", true) => "searching",
        ("grep_search", false) => "searched",
        ("shell_exec", true) => "running",
        ("shell_exec", false) => "ran",
        ("web_fetch", true) => "fetching",
        ("web_fetch", false) => "fetched",
        ("web_search", true) => "searching",
        ("web_search", false) => "searched",
        (name, _) => name,
    }
}

/// Format the tool-call suffix appended to an AI header row.
/// Groups consecutive same-name tools. Uses verb tenses and result previews.
/// The `spinner_char` is substituted for the in-progress icon on incomplete tools.
pub(super) fn format_tool_suffix(tools: &[ToolRef], spinner_char: &str) -> String {
    if tools.is_empty() {
        return String::new();
    }

    // Group consecutive tools by name.
    let mut groups: Vec<(&str, Vec<&ToolRef>)> = Vec::new();
    for t in tools {
        if let Some(last) = groups.last_mut() {
            if last.0 == t.name.as_str() {
                last.1.push(t);
                continue;
            }
        }
        groups.push((t.name.as_str(), vec![t]));
    }

    let mut s = String::new();
    for (name, group) in &groups {
        let any_failed = group.iter().any(|t| t.complete && t.failed);
        let any_pending = group.iter().any(|t| !t.complete);
        let icon = if any_pending {
            spinner_char
        } else if any_failed {
            "✗"
        } else {
            "✓"
        };
        let in_progress = any_pending;
        let verb = tool_verb(name, in_progress);

        if group.len() == 1 {
            let t = group[0];
            if t.args.is_empty() {
                s.push_str(&format!("  {} {}", icon, verb));
            } else {
                s.push_str(&format!("  {} {} {}", icon, verb, t.args));
            }
            // Append result preview for completed non-failed tools.
            if t.complete && !t.failed && !t.result.is_empty() {
                s.push_str(&format!(" -> {}", t.result));
            }
        } else {
            // Multiple same-name tools: fold into count summary.
            let count = group.len();
            // Pick a plural noun from the last part of the verb for readability.
            let noun = match *name {
                "fs_read" => "files",
                "fs_write" => "files",
                "fs_patch" => "files",
                "fs_delete" => "files",
                "fs_list" => "dirs",
                "grep_search" => "patterns",
                "shell_exec" => "commands",
                "web_fetch" | "web_search" => "requests",
                _ => "calls",
            };
            s.push_str(&format!("  {} {} {} {}", icon, verb, count, noun));
        }
    }
    s
}

/// Build the attribute cell for an inline span within an AI text line,
/// honoring the enclosing block style.
fn inline_cell(style: InlineStyle, block: BlockStyle, pal: &ChatPalette) -> CellAttributes {
    // Heading markers and body text use contrast-aware palette helpers.
    // Diff lines use semantic colors independent of the palette.
    if let BlockStyle::DiffAdd | BlockStyle::DiffRemove | BlockStyle::DiffHunk = block {
        let fg = match block {
            BlockStyle::DiffAdd => {
                ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(0.349, 0.733, 0.451, 1.0))
            }
            BlockStyle::DiffRemove => {
                ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(0.875, 0.408, 0.408, 1.0))
            }
            _ => ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(0.561, 0.631, 0.749, 1.0)),
        };
        return pal.make_attrs(fg, pal.bg_attr());
    }
    let base = match block {
        BlockStyle::Heading(_) => pal.heading_text_cell(),
        BlockStyle::Quote => pal.input_cell(), // dim fg for block-quoted text
        BlockStyle::Hr => pal.border_dim_cell(),
        BlockStyle::Code => pal.input_cell(),
        _ => pal.ai_text_cell(),
    };
    match style {
        InlineStyle::HeadingMarker => pal.heading_marker_cell(),
        InlineStyle::Plain => base,
        InlineStyle::Bold => {
            let mut a = base;
            a.apply_change(&AttributeChange::Intensity(termwiz::cell::Intensity::Bold));
            a
        }
        InlineStyle::Italic => {
            let mut a = base;
            a.apply_change(&AttributeChange::Italic(true));
            a
        }
        InlineStyle::Code => pal.input_cell(),
        InlineStyle::Highlighted(r, g, b) => {
            let mut a = base;
            let fg = ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                1.0,
            ));
            a.apply_change(&AttributeChange::Foreground(fg));
            a
        }
    }
}

/// Build the styled run sequence for a DisplayLine (the content between the
/// border glyphs). Each run is `(attr, text)`. Includes the left indent and
/// any block-level decoration prefixes (quote bar, list bullet is already
/// baked into the first span by `emit_assistant_markdown`).
fn build_line_runs(
    line: &DisplayLine,
    pal: &ChatPalette,
    spinner_char: &str,
    spinner_char_tool: &str,
    content_width: usize,
) -> Vec<(CellAttributes, String)> {
    let mut runs: Vec<(CellAttributes, String)> = Vec::new();
    match line {
        DisplayLine::Header {
            role: Role::User, ..
        } => {
            runs.push((pal.user_header_cell(), strings::header_user()));
        }
        DisplayLine::Header {
            role: Role::Assistant,
            tools,
        } => {
            runs.push((pal.ai_header_cell(), strings::header_assistant()));
            if !tools.is_empty() {
                // Render tool status in a dimmer tone so the "AI" header still pops.
                let suffix = format_tool_suffix(tools, spinner_char_tool);
                let avail = content_width.saturating_sub(4); // 4 = "  AI"
                let suffix = if unicode_column_width(&suffix, None) > avail {
                    let last_suffix = format_tool_suffix(
                        std::slice::from_ref(tools.last().unwrap()),
                        spinner_char_tool,
                    );
                    if unicode_column_width(&last_suffix, None) <= avail {
                        last_suffix
                    } else {
                        // Even the last tool overflows; hard-truncate the suffix.
                        let chars: Vec<char> = last_suffix.chars().collect();
                        chars[..avail.min(chars.len())].iter().collect()
                    }
                } else {
                    suffix
                };
                runs.push((pal.input_cell(), suffix));
            }
        }
        DisplayLine::AttachmentSummary { labels } => {
            runs.push((pal.input_cell(), "  Attached: ".to_string()));
            runs.push((pal.ai_header_cell(), labels.join(" ")));
        }
        DisplayLine::Text {
            segments,
            role: Role::User,
            ..
        } => {
            runs.push((pal.user_text_cell(), "  ".to_string()));
            for seg in segments {
                runs.push((pal.user_text_cell(), seg.text.clone()));
            }
        }
        DisplayLine::Text {
            segments,
            role: Role::Assistant,
            block,
        } => {
            let indent = match block {
                BlockStyle::Quote => {
                    // "  │ " = 2 cols leading + quote bar + space
                    runs.push((pal.plain_cell(), "  ".to_string()));
                    runs.push((pal.border_dim_cell(), "│ ".to_string()));
                    String::new()
                }
                BlockStyle::ListContinuation => "    ".to_string(),
                _ => "  ".to_string(),
            };
            if !indent.is_empty() {
                // Use the line's base attr for the indent so backgrounds match.
                let indent_attr = inline_cell(InlineStyle::Plain, *block, pal);
                runs.push((indent_attr, indent));
            }
            for seg in segments {
                let attr = inline_cell(seg.style, *block, pal);
                runs.push((attr, seg.text.clone()));
            }
        }
        DisplayLine::LoadingDot => {
            runs.push((
                pal.ai_header_cell(),
                format!("  {}  Thinking...", spinner_char),
            ));
        }
        DisplayLine::ThinkingHeader {
            char_count,
            expanded,
        } => {
            let arrow = if *expanded { "▼" } else { "▶" };
            let label = format!("  {} Thinking ({} chars) · ^O toggle", arrow, char_count);
            runs.push((pal.input_cell(), label));
        }
        DisplayLine::Blank => {}
    }
    runs
}

/// Emit a single content row: pad to `inner_w`, apply selection overlay across
/// the styled runs, truncate anything that overflows `inner_w`.
fn emit_styled_line(
    changes: &mut Vec<Change>,
    runs: &[(CellAttributes, String)],
    inner_w: usize,
    sel_range: Option<(usize, usize)>,
    pal: &ChatPalette,
) {
    // Compute total content width, append a plain padding run.
    let content_w: usize = runs
        .iter()
        .map(|(_, t)| unicode_column_width(t.as_str(), None))
        .sum();
    let pad_w = inner_w.saturating_sub(content_w);

    // Build pieces with absolute column ranges.
    struct Piece {
        attr: CellAttributes,
        text: String,
        start: usize,
        end: usize,
    }
    let mut pieces: Vec<Piece> = Vec::with_capacity(runs.len() + 1);
    let mut col = 0usize;
    for (attr, text) in runs {
        if text.is_empty() {
            continue;
        }
        let w = unicode_column_width(text.as_str(), None);
        pieces.push(Piece {
            attr: attr.clone(),
            text: text.clone(),
            start: col,
            end: col + w,
        });
        col += w;
    }
    if pad_w > 0 {
        pieces.push(Piece {
            attr: pal.plain_cell(),
            text: " ".repeat(pad_w),
            start: col,
            end: col + pad_w,
        });
    }

    // Truncate pieces that cross `inner_w`.
    let final_pieces: Vec<Piece> = pieces
        .into_iter()
        .filter_map(|p| {
            if p.start >= inner_w {
                return None;
            }
            if p.end <= inner_w {
                return Some(p);
            }
            let keep_cols = inner_w - p.start;
            let byte = byte_pos_at_visual_col(&p.text, keep_cols);
            Some(Piece {
                attr: p.attr,
                text: p.text[..byte].to_string(),
                start: p.start,
                end: p.start + keep_cols,
            })
        })
        .collect();

    for p in final_pieces {
        match sel_range {
            Some((sc, ec)) if sc < p.end && ec > p.start => {
                let mid_s = sc.max(p.start);
                let mid_e = ec.min(p.end);
                let b1 = byte_pos_at_visual_col(&p.text, mid_s - p.start);
                let b2 = byte_pos_at_visual_col(&p.text, mid_e - p.start);
                if b1 > 0 {
                    changes.push(Change::AllAttributes(p.attr.clone()));
                    changes.push(Change::Text(p.text[..b1].to_string()));
                }
                if b2 > b1 {
                    let mut sel_attr = p.attr.clone();
                    if pal.selection_fg.3 != 0.0 {
                        sel_attr.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(
                            pal.selection_fg,
                        ));
                    }
                    sel_attr.set_background(ColorAttribute::TrueColorWithDefaultFallback(
                        pal.selection_bg,
                    ));
                    changes.push(Change::AllAttributes(sel_attr));
                    changes.push(Change::Text(p.text[b1..b2].to_string()));
                }
                if b2 < p.text.len() {
                    changes.push(Change::AllAttributes(p.attr.clone()));
                    changes.push(Change::Text(p.text[b2..].to_string()));
                }
            }
            _ => {
                changes.push(Change::AllAttributes(p.attr.clone()));
                changes.push(Change::Text(p.text.clone()));
            }
        }
    }
}

pub(super) fn render(term: &mut TermWizTerminal, app: &App) -> termwiz::Result<()> {
    match &app.mode {
        AppMode::Chat => render_chat(term, app),
        AppMode::ResumePicker { items, cursor } => render_picker(term, app, items, *cursor),
    }
}

/// Emit a bordered separator row containing the given styled runs.
/// Used for the inline slash-command and attachment pickers.
fn push_picker_row(
    changes: &mut Vec<Change>,
    row: usize,
    inner_w: usize,
    pal: &ChatPalette,
    runs: Vec<(CellAttributes, String)>,
) {
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text("│".to_string()));
    emit_styled_line(changes, &runs, inner_w, None, pal);
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text("│".to_string()));
}

fn render_vertical_picker(
    changes: &mut Vec<Change>,
    start_row: usize,
    inner_w: usize,
    pal: &ChatPalette,
    options: &[(&str, &str)],
    selected: usize,
) {
    let len = options.len();
    let visible_h = len.min(MAX_PICKER_ROWS);
    let scroll_offset = selected
        .saturating_sub(visible_h.saturating_sub(1))
        .min(len.saturating_sub(visible_h));
    for i in 0..visible_h {
        let opt_idx = scroll_offset + i;
        let (label, desc) = options[opt_idx];
        let attr = if opt_idx == selected {
            pal.picker_cursor_cell()
        } else {
            pal.ai_text_cell()
        };
        // Pre-pad to inner_w so the selected row's highlight stripe (drawn as
        // the run's background) spans the entire content width, not just the
        // label. Critical for visibility on light themes where the bg vs
        // accent contrast is subtle.
        let row_text = pad_to_visual_width(&format!(" {}  {}", label, desc), inner_w);
        let runs = vec![(attr, row_text)];
        push_picker_row(changes, start_row + i, inner_w, pal, runs);
    }
}

fn render_chat(term: &mut TermWizTerminal, app: &App) -> termwiz::Result<()> {
    let cols = app.cols;
    let rows = app.rows;
    let inner_w = cols.saturating_sub(2); // inside left and right borders
    let pal = &app.context.colors;

    let mut changes: Vec<Change> = Vec::with_capacity(rows * 4);

    // Begin atomic frame: hold all terminal actions until sync-end so the GPU
    // render thread never sees a half-drawn frame. Cursor is hidden here so it
    // does not flash at (0,0) during ClearScreen, then restored at the end.
    changes.push(Change::Text("\x1b[?2026h".to_string()));
    changes.push(Change::CursorVisibility(CursorVisibility::Hidden));

    // 1. Clear screen using the active theme's background color.
    changes.push(Change::AllAttributes(pal.plain_cell()));
    changes.push(Change::ClearScreen(pal.bg_attr()));

    // 2. Top border.
    let model_display = if let Some((ref flash_msg, _)) = app.model_status_flash {
        flash_msg.clone()
    } else {
        let suffix = match &app.model_fetch {
            ModelFetch::Loading => format!(" · {} loading…", app.spinner_char()),
            ModelFetch::Failed(_) => " · (list failed)".to_string(),
            ModelFetch::Loaded if app.available_models.len() > 2 => {
                format!(" ({}/{})", app.model_index + 1, app.available_models.len())
            }
            _ => String::new(),
        };
        format!("{}{}", app.current_model(), suffix)
    };
    let has_switch = app.available_models.len() > 1;
    let title = if has_switch {
        format!(" Kaku AI • {} · ⇧⇥ switch · ESC exit ", model_display)
    } else {
        format!(" Kaku AI • {} · ESC exit ", model_display)
    };
    let title_width = unicode_column_width(&title, None);
    let border_fill = inner_w.saturating_sub(title_width);
    let top_line = format!("╭─{}{}─╮", title, "─".repeat(border_fill.saturating_sub(2)));
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(0),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(truncate(&top_line, cols)));

    // 3. Message area.
    let input_visible_h = app.input_visible_rows();
    let slash_options = app.slash_picker_options();
    let attach_options = app.attachment_picker_options();
    let picker_h = {
        let opt_len = if !slash_options.is_empty() {
            slash_options.len()
        } else if !attach_options.is_empty() {
            attach_options.len()
        } else {
            0
        };
        let max_h = rows.saturating_sub(input_visible_h + 3);
        opt_len.min(MAX_PICKER_ROWS).min(max_h)
    };
    let msg_area_h = rows.saturating_sub(3 + input_visible_h + picker_h);
    let all_lines = app.display_lines();
    let total = all_lines.len();

    // Determine the slice to show, accounting for scroll.
    let visible_start = if total <= msg_area_h {
        0
    } else {
        (total - msg_area_h).saturating_sub(app.scroll_offset)
    };
    let visible = &all_lines[visible_start..total.min(visible_start + msg_area_h)];

    for (i, line) in visible.iter().enumerate() {
        let row = i + 1; // row 0 is top border
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(row),
        });
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));

        let runs = build_line_runs(
            line,
            pal,
            app.spinner_char(),
            app.spinner_char_tool(),
            inner_w,
        );
        let line_idx = visible_start + i;

        // Determine the selection column range for this line (content columns, 0-based).
        // Terminal col 1 is the first content col (col 0 is the left border │).
        let sel_range: Option<(usize, usize)> = app.selection.and_then(|(r0, c0, r1, c1)| {
            let (sel_r0, sel_c0, sel_r1, sel_c1) = if r0 < r1 || (r0 == r1 && c0 <= c1) {
                (r0, c0, r1, c1)
            } else {
                (r1, c1, r0, c0)
            };
            if line_idx >= sel_r0 && line_idx <= sel_r1 {
                // c values are terminal x; content starts at terminal col 1.
                let sc = if line_idx == sel_r0 {
                    sel_c0.saturating_sub(1)
                } else {
                    0
                };
                let ec = if line_idx == sel_r1 {
                    sel_c1.saturating_sub(1)
                } else {
                    inner_w
                };
                Some((sc, ec))
            } else {
                None
            }
        });

        emit_styled_line(&mut changes, &runs, inner_w, sel_range, pal);

        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
    }

    // Fill remaining rows in message area with empty lines.
    for i in visible.len()..msg_area_h {
        let row = i + 1;
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(row),
        });
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
        changes.push(Change::AllAttributes(pal.plain_cell()));
        changes.push(Change::Text(pad_to_visual_width("", inner_w)));
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
    }

    // 4. Vertical picker (above separator) + separator row.
    let sep_row = rows.saturating_sub(2 + input_visible_h);
    if picker_h > 0 {
        let picker_start = sep_row.saturating_sub(picker_h);
        let selected = app.attachment_picker_index;
        if !slash_options.is_empty() {
            let opts: Vec<(&str, &str)> = slash_options.iter().map(|(l, d)| (*l, *d)).collect();
            let clamped = selected.min(opts.len().saturating_sub(1));
            render_vertical_picker(&mut changes, picker_start, inner_w, pal, &opts, clamped);
        } else {
            let opts: Vec<(&str, &str)> = attach_options
                .iter()
                .map(|o| (o.label, o.description))
                .collect();
            let clamped = selected.min(opts.len().saturating_sub(1));
            render_vertical_picker(&mut changes, picker_start, inner_w, pal, &opts, clamped);
        }
    }
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(sep_row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(format!(
        "├{}┤",
        "─".repeat(inner_w.saturating_sub(0))
    )));

    // 5. Input area (one or more rows). Approval banner, when present,
    // always takes a single row.
    let input_top = rows.saturating_sub(1 + input_visible_h);

    // Compute cursor state now; apply AFTER bottom border so it's the final position.
    let cursor_state: Option<(usize, usize)> = if let Some((summary, _)) = &app.pending_approval {
        // Approval banner uses the AI accent color + a live spinner so it visually
        // separates from the regular `> ` input row and pulls the user's eye.
        // Keys are placed first so they remain visible when summary is truncated.
        let approval_text = format!(
            "  {} Enter allow · ESC deny   {}",
            app.spinner_char(),
            summary
        );
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(input_top),
        });
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
        changes.push(Change::AllAttributes(pal.ai_header_cell()));
        changes.push(Change::Text(truncate(
            &pad_to_visual_width(&approval_text, inner_w),
            inner_w,
        )));
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
        None // hidden
    } else {
        // Show a pulsing spinner instead of `>` while streaming so the user
        // sees the response is still in progress.
        let prompt = app.current_input_prompt();
        let layout = layout_input(&prompt, &app.input, app.input_cursor, inner_w);
        let total_rows = layout.rows.len();
        // Anchor the cursor at the bottom of the visible window when the
        // wrapped input overflows the cap, so typing always stays in view.
        let scroll = if total_rows > input_visible_h {
            layout
                .cursor_row
                .saturating_sub(input_visible_h.saturating_sub(1))
                .min(total_rows - input_visible_h)
        } else {
            0
        };

        for i in 0..input_visible_h {
            let abs_row = input_top + i;
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(abs_row),
            });
            changes.push(Change::AllAttributes(pal.border_dim_cell()));
            changes.push(Change::Text("│".to_string()));
            changes.push(Change::AllAttributes(pal.input_cell()));
            let row_text = layout
                .rows
                .get(scroll + i)
                .map(String::as_str)
                .unwrap_or("");
            changes.push(Change::Text(pad_to_visual_width(row_text, inner_w)));
            changes.push(Change::AllAttributes(pal.border_dim_cell()));
            changes.push(Change::Text("│".to_string()));
        }

        // Hide the input cursor during streaming until the user deliberately
        // clicks the input row. Keeps visual focus on the AI response and
        // avoids a blinking cursor next to the spinner that reads as noise.
        if app.is_streaming && !app.input_clicked_this_stream {
            None
        } else if layout.cursor_row >= scroll && layout.cursor_row < scroll + input_visible_h {
            // +1 for the left border column; cap at the last content column so
            // the caret never lands on top of the right border glyph if the
            // phantom-row guard ever misses.
            let col = (1 + layout.cursor_col).min(cols.saturating_sub(2));
            let row = input_top + (layout.cursor_row - scroll);
            Some((col, row))
        } else {
            None
        }
    };

    // 6. Bottom border.
    let bot_row = rows.saturating_sub(1);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(bot_row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(format!(
        "╰{}╯",
        "─".repeat(inner_w.saturating_sub(0))
    )));

    // Restore cursor to input position AFTER drawing all decorations, so the
    // terminal's physical cursor lands on the input row, not the bottom border.
    match cursor_state {
        Some((cx, cy)) => {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(cx),
                y: Position::Absolute(cy),
            });
            changes.push(Change::CursorShape(CursorShape::SteadyBlock));
            changes.push(Change::CursorVisibility(CursorVisibility::Visible));
        }
        None => {
            changes.push(Change::CursorVisibility(CursorVisibility::Hidden));
        }
    }

    // End atomic frame: flush all buffered terminal actions at once.
    changes.push(Change::Text("\x1b[?2026l".to_string()));

    term.render(&changes)
}

fn render_picker(
    term: &mut TermWizTerminal,
    app: &App,
    items: &[ai_conversations::ConversationMeta],
    cursor: usize,
) -> termwiz::Result<()> {
    let cols = app.cols;
    let rows = app.rows;
    let inner_w = cols.saturating_sub(2);
    let pal = &app.context.colors;

    let mut changes: Vec<Change> = Vec::with_capacity(rows * 4);

    // Begin atomic frame (same rationale as render_chat).
    changes.push(Change::Text("\x1b[?2026h".to_string()));
    changes.push(Change::CursorVisibility(CursorVisibility::Hidden));

    changes.push(Change::AllAttributes(pal.plain_cell()));
    changes.push(Change::ClearScreen(pal.bg_attr()));

    // Top border
    let title = format!(" Resume Conversation · {} saved · ESC cancel ", items.len());
    let title_width = unicode_column_width(&title, None);
    let border_fill = inner_w.saturating_sub(title_width);
    let top_line = format!("╭─{}{}─╮", title, "─".repeat(border_fill.saturating_sub(2)));
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(0),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(truncate(&top_line, cols)));

    // List area
    let msg_area_h = app.msg_area_height();
    for i in 0..msg_area_h {
        let row = i + 1;
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(row),
        });
        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));

        if let Some(meta) = items.get(i) {
            let ts = chrono::DateTime::from_timestamp(meta.updated_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let summary = if meta.summary.trim_matches('…').is_empty()
                || meta.summary == "…"
                || meta.summary.is_empty()
            {
                "(no summary yet)".to_string()
            } else {
                meta.summary.chars().take(30).collect::<String>()
            };
            let line_text = format!(" {} {} ({} msgs)", ts, summary, meta.message_count);
            let padded = pad_to_visual_width(&line_text, inner_w);
            if i == cursor {
                changes.push(Change::AllAttributes(pal.picker_cursor_cell()));
            } else {
                changes.push(Change::AllAttributes(pal.plain_cell()));
            }
            changes.push(Change::Text(truncate(&padded, inner_w)));
        } else {
            changes.push(Change::AllAttributes(pal.plain_cell()));
            changes.push(Change::Text(pad_to_visual_width("", inner_w)));
        }

        changes.push(Change::AllAttributes(pal.border_dim_cell()));
        changes.push(Change::Text("│".to_string()));
    }

    // Separator
    let sep_row = rows.saturating_sub(3);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(sep_row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(format!("├{}┤", "─".repeat(inner_w))));

    // Hint row
    let input_row = rows.saturating_sub(2);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(input_row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text("│".to_string()));
    let hint = format!("  ↑↓ select · Enter load · Esc cancel");
    changes.push(Change::AllAttributes(pal.input_cell()));
    changes.push(Change::Text(pad_to_visual_width(&hint, inner_w)));
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text("│".to_string()));
    // cursor is already hidden at frame start

    // Bottom border
    let bot_row = rows.saturating_sub(1);
    changes.push(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(bot_row),
    });
    changes.push(Change::AllAttributes(pal.border_dim_cell()));
    changes.push(Change::Text(format!("╰{}╯", "─".repeat(inner_w))));

    // End atomic frame.
    changes.push(Change::Text("\x1b[?2026l".to_string()));

    term.render(&changes)
}
