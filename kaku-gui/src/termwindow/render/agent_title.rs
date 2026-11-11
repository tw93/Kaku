//! The title an AI agent CLI (Claude Code, Codex, Kimi) gives its session,
//! drawn small at the top right of the window with the agent's mark: in the
//! empty strip an integrated-buttons window keeps above the content, or in
//! the top padding when fullscreen. Not drawn under a top tab bar.
//!
//! Agents set this title over OSC 0/2 ("✳ Fix the padding bug"). A tab is too
//! narrow for it and would change width with every update, and the top right
//! is empty, so the title lives there and leaves the tab alone.

use crate::customglyph::{BlockAlpha, Poly, PolyStyle};
use crate::termwindow::box_model::*;
use crate::termwindow::render::agent_marks::{CLAUDE_MARK, CODEX_MARK, KIMI_MARK, PI_MARK};
use crate::termwindow::render::borders::{
    integrated_buttons_top_inset, INTEGRATED_BUTTONS_TOP_CLEARANCE_POINTS,
};
use crate::termwindow::{DimensionContext, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::{WindowOps, WindowState};
use config::Dimension;
use std::rc::Rc;
use wezterm_font::LoadedFont;
use window::color::LinearRgba;

/// The title may take at most this share of the window width.
const MAX_WIDTH_FRACTION: f32 = 0.5;
/// How many rows up from the screen's bottom to look for the agent's own title.
const TOPIC_SCAN_ROWS: wezterm_term::StableRowIndex = 24;
/// Mark and text alpha: present but quieter than content.
const TEXT_ALPHA: f32 = 0.55;
/// Mark size relative to the caption line height.
const MARK_SCALE: f32 = 0.75;
/// How far the mark sits below the line box's middle, to meet the text.
const MARK_DROP_POINTS: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentTitle {
    /// "claude", "codex" or "kimi", from `foreground_agent_cli`.
    pub program: &'static str,
    pub topic: String,
    /// The agent already shows `topic` on screen, so only the mark is drawn.
    pub topic_on_screen: bool,
}

fn agent_label(program: &str) -> &'static str {
    match program {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "kimi" => "Kimi",
        "pi" => "pi",
        _ => "Agent",
    }
}

fn agent_mark(program: &str) -> Option<&'static [Poly]> {
    const fn mark(path: &'static [crate::customglyph::PolyCommand]) -> [Poly; 1] {
        [Poly {
            path,
            intensity: BlockAlpha::Full,
            style: PolyStyle::FillEvenOdd,
        }]
    }
    const CLAUDE: [Poly; 1] = mark(CLAUDE_MARK);
    const CODEX: [Poly; 1] = mark(CODEX_MARK);
    const KIMI: [Poly; 1] = mark(KIMI_MARK);
    const PI: [Poly; 1] = mark(PI_MARK);
    match program {
        "claude" => Some(&CLAUDE),
        "codex" => Some(&CODEX),
        "kimi" => Some(&KIMI),
        "pi" => Some(&PI),
        _ => None,
    }
}

/// The topic in `program`'s pane title. Leading spinner and status glyphs
/// (Claude's ◐ ◑ ✳, Codex's braille spinner) are dropped, so the text stays
/// still while the agent works, and so is a leading agent name such as
/// Codex's `app-name` title item, since the mark already names the agent.
/// Falls back to the agent's name when the title has no topic.
pub(crate) fn agent_topic(program: &str, pane_title: &str) -> String {
    let label = agent_label(program);
    let mut rest = pane_title.trim();
    // pi names its sessions "π - <project>". π is alphabetic, so the glyph
    // rule below keeps it; drop it here, the same way a spinner is dropped.
    if program == "pi" {
        rest = rest.strip_prefix('π').unwrap_or(rest).trim_start();
    }
    loop {
        if let Some(prefix) = rest.get(..label.len()) {
            let suffix = &rest[label.len()..];
            let ends_name = suffix
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || matches!(c, ':' | '|' | '·'));
            if prefix.eq_ignore_ascii_case(label) {
                if !ends_name {
                    break;
                }
                rest = suffix.trim_start();
                continue;
            }
        }
        let Some(token) = rest.split_whitespace().next() else {
            break;
        };
        if token.eq_ignore_ascii_case(program) || !token.chars().any(char::is_alphanumeric) {
            rest = rest[token.len()..].trim_start();
        } else {
            break;
        }
    }
    if rest.is_empty() || rest == crate::tabbar::DEFAULT_PANE_TITLE {
        label.to_string()
    } else {
        rest.to_string()
    }
}

/// Whether `row` draws `topic` set into a horizontal rule, the way Claude Code
/// puts a named session on its input box border ("──── Fix the bug ─"). The
/// rule on both sides keeps a chat line that merely mentions the topic, or a
/// longer name containing it, from hiding the text.
fn rule_carries_topic(row: &str, topic: &str) -> bool {
    let is_rule = |c: Option<char>| matches!(c, Some('\u{2500}'..='\u{257f}'));
    !topic.is_empty()
        && row.match_indices(topic).any(|(at, _)| {
            is_rule(row[..at].trim_end().chars().next_back())
                && is_rule(row[at + topic.len()..].trim_start().chars().next())
        })
}

/// Whether the live bottom of `pane` already shows `topic`. A taller input box
/// can push its border past the scanned rows; the text then simply stays.
pub(crate) fn pane_shows_topic(pane: &dyn mux::pane::Pane, topic: &str) -> bool {
    let dims = pane.get_dimensions();
    let end = dims.physical_top + dims.viewport_rows as wezterm_term::StableRowIndex;
    let start = (end - TOPIC_SCAN_ROWS).max(dims.physical_top);
    let (_, lines) = pane.get_lines(start..end);
    lines
        .iter()
        .any(|line| rule_carries_topic(&line.as_str(), topic))
}

/// The longest prefix of `text` that fits `max_width` with an ellipsis,
/// given a measure of rendered width. `text` itself when it already fits.
fn fit_with_ellipsis(
    text: &str,
    max_width: f32,
    mut width: impl FnMut(&str) -> anyhow::Result<f32>,
) -> anyhow::Result<String> {
    if width(text)? <= max_width {
        return Ok(text.to_string());
    }
    let chars: Vec<char> = text.chars().collect();
    let truncated = |n: usize| chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&truncated(mid))? <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Ok(if lo == 0 {
        String::new()
    } else {
        truncated(lo)
    })
}

impl crate::TermWindow {
    /// The agent title for the active pane, or None when no agent runs in it.
    pub(crate) fn active_agent_title(&self) -> Option<AgentTitle> {
        if !self.config.show_agent_title {
            return None;
        }
        let pane = self.get_active_pane_no_overlay()?;
        let program = crate::tabbar::foreground_agent_cli(&*pane)?;
        let topic = agent_topic(program, &pane.get_title());
        Some(AgentTitle {
            program,
            topic_on_screen: pane_shows_topic(&*pane, &topic),
            topic,
        })
    }

    fn shaped_text_width(&self, font: &Rc<LoadedFont>, text: &str) -> anyhow::Result<f32> {
        let shape_window = self.window.as_ref().unwrap().clone();
        let shaped = font.shape(
            text,
            move || shape_window.notify(TermWindowNotif::InvalidateShapeCache),
            |_| {},
            None,
            wezterm_bidi::Direction::LeftToRight,
            None,
            None,
        )?;
        Ok(shaped.iter().map(|g| g.x_advance.get() as f32).sum())
    }

    /// The y the title is vertically centered on, or None where it has no
    /// room: in a window, the line through the traffic-light buttons, which
    /// sit on the strip's bottom edge (16pt down), half over the top padding
    /// (lifted when that padding is too thin to hold it); fullscreen, the
    /// same 16pt below the top edge, so the title keeps its window position
    /// instead of hugging the menu bar (the paint clamp keeps it above the
    /// first row).
    fn agent_title_center_y(&self) -> Option<f32> {
        if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            return None;
        }
        let border_top = self.get_os_border().top.get() as f32;
        if self
            .effective_layout_window_state()
            .contains(WindowState::FULL_SCREEN)
        {
            let (_, padding_top) = self.padding_left_top();
            let clearance =
                INTEGRATED_BUTTONS_TOP_CLEARANCE_POINTS * self.dimensions.dpi as f32 / 72.;
            return (padding_top > 0.).then_some(border_top + clearance);
        }
        let inset = integrated_buttons_top_inset(
            &self.config,
            self.effective_layout_window_state(),
            false,
            self.dimensions.dpi as f32,
        );
        (inset > 0 && border_top > 0.).then_some(border_top)
    }

    pub fn paint_agent_title(&mut self) -> anyhow::Result<()> {
        // Checked here too so turning it off hides the title on the reload
        // repaint, before the next title update clears `agent_title`.
        if !self.config.show_agent_title {
            return Ok(());
        }
        let Some(title) = self.agent_title.clone() else {
            return Ok(());
        };
        let Some(center_y) = self.agent_title_center_y() else {
            return Ok(());
        };

        let font = self.fonts.caption_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dpi = self.dimensions.dpi as f32;
        let window_width = self.dimensions.pixel_width as f32;
        let (right_inset, padding_top) = self.padding_left_top();
        let mark = agent_mark(title.program);
        let mark_size = (metrics.cell_size.height as f32 * MARK_SCALE).round();
        let mark_gap = (metrics.cell_size.width as f32 * 0.6 + 0.5 * dpi / 72.).round();
        // Centering the mark on the line box leaves it above the text, whose
        // ink sits low in the line (measured 1 to 2.5px at 2x); a top margin
        // of twice the shift moves a middle-aligned box down by the shift.
        let mark_drop = (MARK_DROP_POINTS * dpi / 72.).round();
        let mark_width = if mark.is_some() {
            mark_size + mark_gap
        } else {
            0.
        };
        let max_text_width = window_width * MAX_WIDTH_FRACTION - mark_width;

        // A fallback font arriving bumps shape_generation and changes the
        // measured widths, so it invalidates the fit like a config reload.
        let generation = (self.config.generation(), self.shape_generation);
        let fitted = match &self.agent_title_fitted {
            _ if title.topic_on_screen => String::new(),
            Some((source, cached_dpi, cached_generation, cached_max, fitted))
                if *source == title.topic
                    && *cached_dpi == self.dimensions.dpi
                    && *cached_generation == generation
                    && *cached_max == max_text_width as usize =>
            {
                fitted.clone()
            }
            _ => {
                let fitted = fit_with_ellipsis(&title.topic, max_text_width, |t| {
                    self.shaped_text_width(&font, t)
                })?;
                self.agent_title_fitted = Some((
                    title.topic.clone(),
                    self.dimensions.dpi,
                    generation,
                    max_text_width as usize,
                    fitted.clone(),
                ));
                fitted
            }
        };

        let palette = self
            .get_active_pane_no_overlay()
            .map(|pane| pane.palette())
            .unwrap_or_else(|| self.palette().clone());
        let color = palette.foreground.to_linear().mul_alpha(TEXT_ALPHA);
        let colors = ElementColors {
            border: BorderColor::default(),
            bg: LinearRgba::TRANSPARENT.into(),
            text: color.into(),
        };

        let mut children = vec![];
        if let Some(poly) = mark {
            children.push(
                Element::new(
                    &font,
                    ElementContent::Poly {
                        line_width: 1,
                        poly: SizedPoly {
                            poly,
                            width: Dimension::Pixels(mark_size),
                            height: Dimension::Pixels(mark_size),
                        },
                    },
                )
                .colors(colors.clone())
                .vertical_align(VerticalAlign::Middle)
                .margin(BoxDimension {
                    left: Dimension::Pixels(0.),
                    right: Dimension::Pixels(if fitted.is_empty() { 0. } else { mark_gap }),
                    top: Dimension::Pixels(mark_drop * 2.),
                    bottom: Dimension::Pixels(0.),
                }),
            );
        }
        if !fitted.is_empty() {
            children.push(
                Element::new(&font, ElementContent::Text(fitted))
                    .colors(colors.clone())
                    .vertical_align(VerticalAlign::Middle),
            );
        }
        if children.is_empty() {
            return Ok(());
        }
        let element = Element::new(&font, ElementContent::Children(children)).colors(colors);

        let line_height = metrics.cell_size.height as f32;
        let bounds = euclid::rect(0., 0., window_width * MAX_WIDTH_FRACTION, line_height);
        let gl_state = self.render_state.as_ref().unwrap();
        let mut computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi,
                    pixel_max: self.dimensions.pixel_height as f32,
                    pixel_cell: line_height,
                },
                width: DimensionContext {
                    dpi,
                    pixel_max: window_width,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds,
                metrics: &metrics,
                gl_state,
                zindex: 30,
            },
            &element,
        )?;
        let x = window_width - right_inset - computed.bounds.width();
        // With little or no top padding the centered line would cover the
        // first row; lift it so its bottom stays at the row's top edge.
        let first_row_top = self.terminal_first_row_offset() + padding_top;
        let height = computed.bounds.height();
        let y = (center_y - height / 2.).min(first_row_top - height).max(0.);
        computed.translate(euclid::vec2(
            x - computed.bounds.min_x(),
            y - computed.bounds.min_y(),
        ));
        self.render_element(&computed, gl_state, None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_topic_set_into_a_rule_counts_as_shown() {
        // Rows read back from Claude Code's input box border.
        let topic = "Mole 调研 DeepSeek 插件方案";
        let border = "────────── Mole 调研 DeepSeek 插件方案 ─";
        assert!(rule_carries_topic(border, topic));
        // Through a real line, since wide characters span two cells.
        let line = termwiz::surface::Line::from_text(
            border,
            &Default::default(),
            termwiz::surface::SEQ_ZERO,
            None,
        );
        assert!(rule_carries_topic(&line.as_str(), topic));

        // No border text, or another name: the title is still the only copy.
        assert!(!rule_carries_topic("────────────────", topic));
        assert!(!rule_carries_topic(
            "───── Mole 修复 TUN 模式流量重复统计 ─",
            topic
        ));
        // A longer name that contains the topic is a different title.
        assert!(!rule_carries_topic("───── Mole 调研 ─", "Mole"));
        // Chat text mentioning it, with or without a rule on one side.
        assert!(!rule_carries_topic(
            "  我在看 Mole 调研 DeepSeek 插件方案 这个会话",
            topic
        ));
        assert!(!rule_carries_topic(
            "───── Mole 调研 DeepSeek 插件方案",
            topic
        ));
        assert!(!rule_carries_topic(border, ""));
    }

    #[test]
    fn spinner_glyphs_are_dropped_from_the_topic() {
        assert_eq!(
            agent_topic("claude", "✳ Fix the padding bug"),
            "Fix the padding bug"
        );
        assert_eq!(
            agent_topic("claude", "◐ Fix the padding bug"),
            "Fix the padding bug"
        );
        assert_eq!(agent_topic("claude", "✳ 修复窗口间距"), "修复窗口间距");
    }

    #[test]
    fn a_leading_agent_name_is_dropped_since_the_mark_names_it() {
        assert_eq!(
            agent_topic("codex", "⠋ codex | Fix padding | kaku"),
            "Fix padding | kaku"
        );
        assert_eq!(agent_topic("claude", "✳ Claude Code · Fix it"), "Fix it");
    }

    #[test]
    fn agent_name_prefix_keeps_complete_topic_words() {
        for (program, topic) in [
            ("codex", "Codexification refactor"),
            ("kimi", "Kimimaro parser"),
            ("claude", "Claude Codebase cleanup"),
        ] {
            assert_eq!(agent_topic(program, topic), topic);
        }
        assert_eq!(agent_topic("codex", "CODEX | Fix padding"), "Fix padding");
        assert_eq!(
            agent_topic("claude", "Claude Code: Fix padding"),
            "Fix padding"
        );
    }

    #[test]
    fn a_title_without_a_topic_falls_back_to_the_agent_name() {
        assert_eq!(agent_topic("claude", "✳"), "Claude Code");
        assert_eq!(agent_topic("claude", "✳ Claude Code"), "Claude Code");
        assert_eq!(agent_topic("codex", "⠋ codex"), "Codex");
        assert_eq!(agent_topic("kimi", "kaku"), "Kimi");
        assert_eq!(agent_topic("kimi", ""), "Kimi");
    }

    #[test]
    fn pi_titles_lead_with_a_pi_glyph() {
        // pi names its session "π - <project>".
        assert_eq!(agent_topic("pi", "π - wiki"), "wiki");
        assert_eq!(agent_topic("pi", "π"), "pi");
    }

    #[test]
    fn every_agent_has_a_mark() {
        for program in ["claude", "codex", "kimi", "pi"] {
            assert!(agent_mark(program).is_some(), "{}", program);
        }
    }

    #[test]
    fn long_titles_end_in_an_ellipsis_within_the_width() {
        let width = |t: &str| Ok(t.chars().count() as f32);
        assert_eq!(fit_with_ellipsis("short", 10., width).unwrap(), "short");
        assert_eq!(
            fit_with_ellipsis("a long agent title", 8., width).unwrap(),
            "a long…"
        );
        assert_eq!(fit_with_ellipsis("abc", 0.5, width).unwrap(), "");
    }
}
