use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use super::{App, Mode};
use crate::tui_core::theme::{accent, bg, muted, panel, primary, text_fg};

/// Approximate terminal display width for a string. ASCII characters
/// count as 1 column, anything outside the ASCII printable range counts
/// as 2 (good enough for the CJK strings this TUI handles; emojis and
/// half-width kana are out of scope for now). Right-pads `s` with
/// spaces so the resulting string takes exactly `cols` terminal columns,
/// or returns the input unchanged when it is already wider.
fn pad_display_width(s: &str, cols: usize) -> String {
    let used: usize = s
        .chars()
        .map(|c| if (c as u32) <= 0x7E { 1 } else { 2 })
        .sum();
    if used >= cols {
        s.to_string()
    } else {
        let mut out = String::with_capacity(s.len() + cols - used);
        out.push_str(s);
        for _ in 0..(cols - used) {
            out.push(' ');
        }
        out
    }
}

/// Translate a footer / header label that ships in English. Looks up
/// `config_tui.<slug>` (e.g. `"Save & Exit"` -> `config_tui.save_exit`)
/// and falls back to the English source when the bundle does not carry
/// a matching entry, so partial translations degrade gracefully.
fn translate_label(en: &str) -> String {
    let mut slug = String::with_capacity(en.len());
    let mut last_underscore = true;
    for ch in en.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_underscore = false;
        } else if !last_underscore {
            slug.push('_');
            last_underscore = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        return en.to_string();
    }
    let key = format!("config_tui.{slug}");
    let translated = rust_i18n::t!(&key);
    if translated.as_ref() == key.as_str() {
        en.to_string()
    } else {
        translated.into_owned()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MainLayoutMode {
    HeaderOnly,
    HeaderAndFooter,
    Expanded,
    Compact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FooterAction {
    key: &'static str,
    long_label: &'static str,
    short_label: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FooterLabelStyle {
    Long,
    Short,
}

const NORMAL_FOOTER_ACTIONS: [FooterAction; 5] = [
    FooterAction {
        key: "↑↓",
        long_label: "Navigate",
        short_label: "Move",
    },
    FooterAction {
        key: "Enter",
        long_label: "Edit",
        short_label: "Edit",
    },
    FooterAction {
        key: "Esc",
        long_label: "Save & Exit",
        short_label: "Save",
    },
    FooterAction {
        key: "Q",
        long_label: "Discard",
        short_label: "Discard",
    },
    FooterAction {
        key: "E",
        long_label: "Open File",
        short_label: "Open",
    },
];

const SELECTING_FOOTER_ACTIONS: [FooterAction; 3] = [
    FooterAction {
        key: "↑↓",
        long_label: "Navigate",
        short_label: "Move",
    },
    FooterAction {
        key: "Enter",
        long_label: "Apply",
        short_label: "Apply",
    },
    FooterAction {
        key: "Esc",
        long_label: "Save & Exit",
        short_label: "Save",
    },
];

const EDITING_FOOTER_ACTIONS: [FooterAction; 2] = [
    FooterAction {
        key: "Enter",
        long_label: "Apply",
        short_label: "Apply",
    },
    FooterAction {
        key: "Esc",
        long_label: "Cancel",
        short_label: "Cancel",
    },
];

fn footer_copy(mode: Mode) -> &'static [FooterAction] {
    match mode {
        Mode::Normal => &NORMAL_FOOTER_ACTIONS,
        Mode::Selecting => &SELECTING_FOOTER_ACTIONS,
        Mode::Editing => &EDITING_FOOTER_ACTIONS,
    }
}

pub(super) fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let full = frame.area();
    if full.width < 2 || full.height < 2 {
        return;
    }

    // Use the full frame so background and popup layers stay visually consistent
    // with the active theme across the whole terminal width.
    let area = full;

    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::default().bg(bg())), area);

    let content_rows = rendered_field_row_count(app);
    match resolve_main_layout(area.height, content_rows) {
        MainLayoutMode::HeaderOnly => {
            let chunks = Layout::vertical([Constraint::Length(2)]).split(area);
            render_header(frame, chunks[0]);
        }
        MainLayoutMode::HeaderAndFooter => {
            let chunks =
                Layout::vertical([Constraint::Length(2), Constraint::Length(1)]).split(area);
            render_header(frame, chunks[0]);
            render_footer(frame, chunks[1], app.mode);
        }
        MainLayoutMode::Expanded => {
            let chunks = Layout::vertical([
                Constraint::Length(2),            // header
                Constraint::Length(content_rows), // content
                Constraint::Fill(1),              // flexible gap
                Constraint::Length(1),            // spacer above footer
                Constraint::Length(1),            // footer (stick to bottom)
            ])
            .split(area);

            render_header(frame, chunks[0]);
            render_fields(frame, chunks[1], app);
            render_footer(frame, chunks[4], app.mode);
        }
        MainLayoutMode::Compact => {
            let chunks = Layout::vertical([
                Constraint::Length(2), // header
                Constraint::Fill(1),   // content
                Constraint::Length(1), // spacer above footer
                Constraint::Length(1), // footer (stick to bottom)
            ])
            .split(area);

            render_header(frame, chunks[0]);
            render_fields(frame, chunks[1], app);
            render_footer(frame, chunks[3], app.mode);
        }
    }

    if app.mode == Mode::Selecting {
        render_selector(frame, area, app);
    } else if app.mode == Mode::Editing {
        render_editor(frame, area, app);
    }
}

fn resolve_main_layout(area_height: u16, content_rows: u16) -> MainLayoutMode {
    let remaining_height = area_height.saturating_sub(2);
    if remaining_height == 0 {
        MainLayoutMode::HeaderOnly
    } else if remaining_height == 1 {
        MainLayoutMode::HeaderAndFooter
    } else if content_rows + 2 <= remaining_height {
        MainLayoutMode::Expanded
    } else {
        MainLayoutMode::Compact
    }
}

fn rendered_field_row_count(app: &App) -> u16 {
    let mut rows = app.fields.len() as u16;
    let mut sections = 0u16;
    let mut last_section: Option<&str> = None;

    for field in &app.fields {
        if last_section != Some(field.section) {
            sections += 1;
            if last_section.is_some() {
                rows += 1;
            }
            last_section = Some(field.section);
        }
    }

    rows + sections
}

fn render_header(frame: &mut ratatui::Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            "  Kaku",
            Style::default().fg(primary()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(muted())),
        Span::styled(translate_label("Settings"), Style::default().fg(text_fg())),
    ]);
    frame.render_widget(Paragraph::new(vec![line, Line::from("")]), area);
}

fn render_fields(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let area = area.inner(Margin::new(0, 0));
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_flat: Option<usize> = None;
    let mut flat = 0usize;
    let key_width = 24usize;
    let mut current_section: Option<&str> = None;

    for (idx, field) in app.fields.iter().enumerate() {
        if current_section != Some(field.section) {
            if current_section.is_some() {
                items.push(ListItem::new(Line::from("")));
                flat += 1;
            }

            items.push(ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    translate_label(field.section),
                    Style::default().fg(muted()).add_modifier(Modifier::BOLD),
                ),
            ])));
            flat += 1;
            current_section = Some(field.section);
        }

        let is_selected = idx == app.selected;
        if is_selected {
            selected_flat = Some(flat);
        }

        let display_value_raw = app.display_value(field);
        // Translate both the key label and the displayed enum value so a
        // user-set locale flows through every cell of the table; padding
        // is computed on display columns (CJK ~2 cols/char) so the value
        // column stays aligned regardless of language.
        let display_value = translate_label(display_value_raw);
        let key_label = translate_label(field.key);
        let has_options = field.has_options();

        let key_style = if is_selected {
            Style::default().fg(primary()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(text_fg())
        };

        let value_style = if is_selected {
            Style::default().fg(primary()).add_modifier(Modifier::BOLD)
        } else if field.value.is_empty() {
            Style::default().fg(muted())
        } else {
            Style::default().fg(accent())
        };

        let marker = if is_selected { "› " } else { "  " };
        let suffix = if has_options && field.options.len() > 2 {
            " ▾"
        } else {
            ""
        };

        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                marker,
                Style::default()
                    .fg(if is_selected { primary() } else { muted() })
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(pad_display_width(&key_label, key_width), key_style),
            Span::styled(format!("{}{}", display_value, suffix), value_style),
        ]);

        items.push(ListItem::new(line));
        flat += 1;
    }

    let mut state = ListState::default();
    state.select(selected_flat);

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, mode: Mode) {
    let actions = footer_copy(mode);
    let label_style = if area.width >= 52 {
        FooterLabelStyle::Long
    } else {
        FooterLabelStyle::Short
    };

    frame.render_widget(
        Paragraph::new(build_footer_line(actions, label_style, area.width)),
        area,
    );
}

fn build_footer_line(
    actions: &[FooterAction],
    label_style: FooterLabelStyle,
    width: u16,
) -> Line<'static> {
    let mut spans = vec![Span::styled("  ", Style::default())];
    let mut used_width = 2usize;
    let max_width = width as usize;
    let separator = " | ";

    for (idx, action) in actions.iter().enumerate() {
        let label_raw = match label_style {
            FooterLabelStyle::Long => action.long_label,
            FooterLabelStyle::Short => action.short_label,
        };
        let label = translate_label(label_raw);
        let segment_width = action.key.chars().count() + 1 + label.chars().count();
        let separator_width = if idx == 0 {
            0
        } else {
            separator.chars().count()
        };

        if used_width + separator_width + segment_width > max_width {
            if idx == 0 && label_style == FooterLabelStyle::Long {
                return build_footer_line(actions, FooterLabelStyle::Short, width);
            }
            break;
        }

        if idx > 0 {
            spans.push(Span::styled(separator, Style::default().fg(muted())));
            used_width += separator_width;
        }

        spans.push(Span::styled(action.key, Style::default().fg(primary())));
        spans.push(Span::styled(
            format!(" {}", label),
            Style::default().fg(muted()),
        ));
        used_width += segment_width;
    }

    if spans.len() == 1 {
        let action = actions[0];
        spans.push(Span::styled(action.key, Style::default().fg(primary())));
    }

    Line::from(spans)
}

fn render_selector(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some((field, select_index)) = app.selecting_view() else {
        return;
    };

    let option_count = field.options.len() as u16;
    let max_popup_width = area.width.saturating_sub(4);
    let min_popup_width = 40u16.min(max_popup_width);
    let longest_option_width = field
        .options
        .iter()
        .map(|opt| opt.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let popup_width = std::cmp::max(
        min_popup_width,
        longest_option_width.saturating_add(10).min(max_popup_width),
    );
    let popup_height = (option_count + 2).min(area.height.saturating_sub(4));
    let popup = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        (area.height.saturating_sub(popup_height)) / 2,
        popup_width,
        popup_height,
    );

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" {}: ", translate_label("Select")),
                Style::default().fg(primary()),
            ),
            Span::styled(translate_label(field.key), Style::default().fg(text_fg())),
            Span::styled("  ", Style::default()),
            Span::styled("Enter", Style::default().fg(primary())),
            Span::styled(
                format!(": {}  ", translate_label("Apply")),
                Style::default().fg(muted()),
            ),
            Span::styled("Esc", Style::default().fg(primary())),
            Span::styled(
                format!(": {} ", translate_label("Save & Exit")),
                Style::default().fg(muted()),
            ),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(primary()))
        .style(Style::default().bg(panel()));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = field
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let is_sel = i == select_index;
            let marker = if is_sel { "› " } else { "  " };
            let style = if is_sel {
                Style::default().fg(primary()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(text_fg())
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    marker,
                    Style::default()
                        .fg(if is_sel { primary() } else { muted() })
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(translate_label(opt), style),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(select_index));

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, inner, &mut state);
}

fn render_editor(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some((field, edit_buf, edit_cursor)) = app.editing_view() else {
        return;
    };

    let popup_width = ((area.width as f32 * 0.7) as u16).min(area.width.saturating_sub(4));
    let popup_height = 5u16.min(area.height.saturating_sub(4));
    let popup = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        (area.height.saturating_sub(popup_height)) / 2,
        popup_width,
        popup_height,
    );

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" {}: ", translate_label("Edit")),
                Style::default().fg(primary()),
            ),
            Span::styled(translate_label(field.key), Style::default().fg(text_fg())),
            Span::styled("  ", Style::default()),
            Span::styled("Enter", Style::default().fg(primary())),
            Span::styled(
                format!(": {}  ", translate_label("Save")),
                Style::default().fg(muted()),
            ),
            Span::styled("Esc", Style::default().fg(primary())),
            Span::styled(
                format!(": {} ", translate_label("Cancel")),
                Style::default().fg(muted()),
            ),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(primary()))
        .style(Style::default().bg(panel()));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let content_area = inner.inner(Margin::new(1, 0));

    let line = if edit_buf.is_empty() {
        Line::from(Span::styled(" ", Style::default().bg(primary())))
    } else {
        let char_count = edit_buf.chars().count();
        let byte_pos = edit_buf
            .char_indices()
            .nth(edit_cursor)
            .map(|(i, _)| i)
            .unwrap_or(edit_buf.len());
        let before = &edit_buf[..byte_pos];
        let after = &edit_buf[byte_pos..];

        if edit_cursor >= char_count {
            Line::from(vec![
                Span::styled(before, Style::default().fg(text_fg())),
                Span::styled(" ", Style::default().bg(primary())),
            ])
        } else {
            let mut chars = after.chars();
            let current_char = chars.next().unwrap_or(' ');
            let remaining = chars.as_str();

            Line::from(vec![
                Span::styled(before, Style::default().fg(text_fg())),
                Span::styled(
                    current_char.to_string(),
                    Style::default().bg(primary()).fg(bg()),
                ),
                Span::styled(remaining, Style::default().fg(text_fg())),
            ])
        }
    };

    let input = Paragraph::new(vec![line]).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(input, content_area);
}

#[cfg(test)]
mod tests {
    use super::{
        build_footer_line, footer_copy, resolve_main_layout, FooterAction, FooterLabelStyle,
        MainLayoutMode, NORMAL_FOOTER_ACTIONS,
    };
    use crate::config_tui::Mode;

    #[test]
    fn keeps_spacer_in_compact_layout() {
        assert_eq!(resolve_main_layout(8, 8), MainLayoutMode::Compact);
    }

    #[test]
    fn requires_room_for_footer_and_spacer_before_expanding() {
        assert_eq!(resolve_main_layout(8, 4), MainLayoutMode::Expanded);
        assert_eq!(resolve_main_layout(8, 5), MainLayoutMode::Compact);
    }

    #[test]
    fn handles_tiny_terminal_heights() {
        assert_eq!(resolve_main_layout(2, 1), MainLayoutMode::HeaderOnly);
        assert_eq!(resolve_main_layout(3, 1), MainLayoutMode::HeaderAndFooter);
    }

    #[test]
    fn normal_footer_keeps_escape_as_apply_shortcut() {
        assert_eq!(footer_copy(Mode::Normal), &NORMAL_FOOTER_ACTIONS);
    }

    #[test]
    fn normal_footer_shows_open_file_shortcut_last() {
        assert_eq!(
            footer_copy(Mode::Normal).last(),
            Some(&FooterAction {
                key: "E",
                long_label: "Open File",
                short_label: "Open",
            })
        );
    }

    #[test]
    fn selecting_footer_switches_escape_to_save_and_exit() {
        assert_eq!(
            footer_text(Mode::Selecting, 80),
            "  ↑↓ Navigate | Enter Apply | Esc Save & Exit"
        );
    }

    #[test]
    fn editing_footer_uses_compact_labels_on_narrow_widths() {
        assert_eq!(footer_text(Mode::Editing, 24), "  Enter Apply");
    }

    #[test]
    fn normal_footer_matches_ai_style_with_separators() {
        assert_eq!(
            footer_text(Mode::Normal, 90),
            "  ↑↓ Navigate | Enter Edit | Esc Save & Exit | Q Discard | E Open File"
        );
    }

    fn footer_text(mode: Mode, width: u16) -> String {
        let label_style = if width >= 52 {
            FooterLabelStyle::Long
        } else {
            FooterLabelStyle::Short
        };

        build_footer_line(footer_copy(mode), label_style, width)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}
