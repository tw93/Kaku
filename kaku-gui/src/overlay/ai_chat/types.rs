use crate::ai_conversations;
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::{ColorAttribute, SrgbaTuple};

/// Colors sampled from Kaku's active theme, captured on the GUI thread and
/// passed into the overlay thread so rendering adapts to the user's palette.
#[derive(Clone)]
pub struct ChatPalette {
    pub bg: SrgbaTuple,
    pub fg: SrgbaTuple,
    pub accent: SrgbaTuple,
    pub border: SrgbaTuple,
    pub user_header: SrgbaTuple,
    pub user_text: SrgbaTuple,
    pub ai_text: SrgbaTuple,
    pub selection_fg: SrgbaTuple,
    pub selection_bg: SrgbaTuple,
}

fn palette_luminance(color: SrgbaTuple) -> f32 {
    0.2126 * color.0 + 0.7152 * color.1 + 0.0722 * color.2
}

fn palette_has_separation(bg: SrgbaTuple, fg: SrgbaTuple) -> bool {
    (palette_luminance(bg) - palette_luminance(fg)).abs() >= 0.15
}

fn palette_blend(a: SrgbaTuple, b: SrgbaTuple, amount: f32) -> SrgbaTuple {
    let amount = amount.clamp(0.0, 1.0);
    SrgbaTuple(
        a.0 + (b.0 - a.0) * amount,
        a.1 + (b.1 - a.1) * amount,
        a.2 + (b.2 - a.2) * amount,
        1.0,
    )
}

impl ChatPalette {
    /// True when the chat background reads as a light color, using ITU-R BT.709
    /// relative luminance. Used to pick light-mode-appropriate accents (e.g.
    /// the syntect theme for fenced code blocks).
    pub(crate) fn is_light(&self) -> bool {
        palette_luminance(self.bg) > 0.5
    }

    /// ATX heading `#` markers: muted against the chat background but still
    /// readable on both light and dark themes.
    fn heading_marker_attr(&self) -> ColorAttribute {
        let anchor = if self.is_light() {
            palette_blend(self.bg, self.fg, 0.42)
        } else {
            palette_blend(self.bg, self.fg, 0.5)
        };
        let candidate = if palette_has_separation(self.bg, self.border) {
            self.border
        } else {
            anchor
        };
        let fg = if palette_has_separation(self.bg, candidate) {
            candidate
        } else {
            anchor
        };
        ColorAttribute::TrueColorWithDefaultFallback(fg)
    }

    /// Heading body text: prefer the accent when it separates from the
    /// background, otherwise fall back to bold foreground.
    fn heading_text_attr(&self) -> ColorAttribute {
        let fg = if palette_has_separation(self.bg, self.accent) {
            self.accent
        } else {
            self.fg
        };
        ColorAttribute::TrueColorWithDefaultFallback(fg)
    }

    pub(super) fn bg_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.bg)
    }
    fn accent_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.accent)
    }
    fn border_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.border)
    }
    fn user_header_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.user_header)
    }
    fn user_text_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.user_text)
    }
    fn ai_text_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.ai_text)
    }
    fn fg_attr(&self) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(self.fg)
    }

    pub(super) fn make_attrs(&self, fg: ColorAttribute, bg: ColorAttribute) -> CellAttributes {
        let mut a = CellAttributes::default();
        a.set_foreground(fg);
        a.set_background(bg);
        a
    }
    fn make_attrs_bold(&self, fg: ColorAttribute, bg: ColorAttribute) -> CellAttributes {
        let mut a = self.make_attrs(fg, bg);
        a.apply_change(&AttributeChange::Intensity(termwiz::cell::Intensity::Bold));
        a
    }

    pub fn border_dim_cell(&self) -> CellAttributes {
        self.make_attrs(self.border_attr(), self.bg_attr())
    }
    pub fn plain_cell(&self) -> CellAttributes {
        self.make_attrs(self.fg_attr(), self.bg_attr())
    }
    pub fn user_header_cell(&self) -> CellAttributes {
        self.make_attrs_bold(self.user_header_attr(), self.bg_attr())
    }
    pub fn user_text_cell(&self) -> CellAttributes {
        self.make_attrs(self.user_text_attr(), self.bg_attr())
    }
    pub fn ai_header_cell(&self) -> CellAttributes {
        self.make_attrs_bold(self.accent_attr(), self.bg_attr())
    }
    pub(crate) fn heading_marker_cell(&self) -> CellAttributes {
        self.make_attrs(self.heading_marker_attr(), self.bg_attr())
    }
    pub(crate) fn heading_text_cell(&self) -> CellAttributes {
        self.make_attrs_bold(self.heading_text_attr(), self.bg_attr())
    }
    pub fn ai_text_cell(&self) -> CellAttributes {
        self.make_attrs(self.ai_text_attr(), self.bg_attr())
    }
    pub fn input_cell(&self) -> CellAttributes {
        self.make_attrs(self.fg_attr(), self.bg_attr())
    }
    /// Cursor highlight used in pickers (e.g., resume list, model dropdown).
    /// Uses the accent color as background so it adapts to both dark and light themes.
    pub fn picker_cursor_cell(&self) -> CellAttributes {
        self.make_attrs(self.bg_attr(), self.accent_attr())
    }
}

/// Terminal context captured from the active pane before entering chat mode.
pub struct TerminalContext {
    pub cwd: String,
    pub visible_lines: Vec<String>,
    pub tab_snapshot: String,
    pub selected_text: String,
    pub colors: ChatPalette,

    pub panel_cols: usize,
    pub panel_rows: usize,

    pub last_exit_code: Option<i32>,

    /// Output lines from the last command (from OSC 133 C to D), if available.
    /// Only populated when last_exit_code.is_some() && last_exit_code != 0.
    /// Capped at 50 lines to avoid context overflow.
    pub last_command_output: Option<Vec<String>>,
}

// ─── Message model ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug)]
pub(crate) struct MessageAttachment {
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) payload: String,
}

impl MessageAttachment {
    pub(crate) fn new(kind: &str, label: &str, payload: String) -> Self {
        Self {
            kind: kind.to_string(),
            label: label.to_string(),
            payload,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Message {
    pub(crate) role: Role,
    pub(crate) content: String,
    pub(crate) reasoning_content: String,
    /// False while the assistant is still streaming.
    pub(crate) complete: bool,
    /// True for UI-only messages (e.g. welcome text) that are not sent to the API.
    pub(crate) is_context: bool,
    /// When Some, this message is a tool-call event line, not a text turn.
    pub(crate) tool_name: Option<String>,
    /// Short preview of the tool's arguments (first 40 chars).
    pub(crate) tool_args: Option<String>,
    /// True when the tool execution returned an error.
    pub(crate) tool_failed: bool,
    pub(crate) attachments: Vec<MessageAttachment>,
}

impl Message {
    pub(crate) fn text(
        role: Role,
        content: impl Into<String>,
        complete: bool,
        is_context: bool,
    ) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning_content: String::new(),
            complete,
            is_context,
            tool_name: None,
            tool_args: None,
            tool_failed: false,
            attachments: Vec::new(),
        }
    }
    pub(crate) fn user_text(
        content: impl Into<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            reasoning_content: String::new(),
            complete: true,
            is_context: false,
            tool_name: None,
            tool_args: None,
            tool_failed: false,
            attachments,
        }
    }
    pub(crate) fn tool_event(name: impl Into<String>, args_preview: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: String::new(),
            reasoning_content: String::new(),
            complete: false,
            is_context: false,
            tool_name: Some(name.into()),
            tool_args: Some(args_preview.into()),
            tool_failed: false,
            attachments: Vec::new(),
        }
    }
    pub(crate) fn is_tool(&self) -> bool {
        self.tool_name.is_some()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AttachmentOption {
    pub(crate) kind: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) const ATTACHMENT_CWD: AttachmentOption = AttachmentOption {
    kind: "cwd",
    label: "@cwd",
    description: "folder summary",
};
pub(crate) const ATTACHMENT_TAB: AttachmentOption = AttachmentOption {
    kind: "tab",
    label: "@tab",
    description: "terminal snapshot",
};
pub(crate) const ATTACHMENT_SELECTION: AttachmentOption = AttachmentOption {
    kind: "selection",
    label: "@selection",
    description: "selected text",
};

// `StreamMsg` is consumed only by `state.rs`, which imports it directly from
// `crate::ai_chat_engine`. No overlay-local re-export needed.

// ─── Model selection ─────────────────────────────────────────────────────────

pub(crate) enum ModelFetch {
    /// Fetch in progress (background thread running).
    Loading,
    /// Fetch succeeded; `available_models` is fully populated.
    Loaded,
    /// Fetch failed with the given error message.
    Failed(String),
}

// ─── App state ───────────────────────────────────────────────────────────────

/// Maximum number of messages kept in the in-memory display list. When this is
/// exceeded the oldest messages are dropped so long sessions do not accumulate
/// unbounded RAM. Only chat messages count; tool events are included.
pub(crate) const MAX_DISPLAY_MESSAGES: usize = 300;

/// How many characters of an approval summary we send into a system
/// notification (the Cmd+L overlay is in the foreground; this banner is
/// surfaced only when the window is unfocused, so it should fit on one line).
pub(crate) const APPROVAL_NOTIFICATION_CHARS: usize = 100;

/// Hard cap on how much of an attached file we inline into the prompt before
/// truncating with a `[truncated]` marker.
pub(crate) const FILE_PREVIEW_CHARS: usize = 1200;

/// Maximum number of (input, cursor) snapshots retained for Cmd+Z on the
/// input line. A single-line prompt rarely needs more; once the cap is hit
/// the oldest snapshot is dropped to keep memory bounded.
pub(crate) const INPUT_UNDO_MAX: usize = 32;

#[derive(Clone)]
pub(crate) struct InputSnapshot {
    pub(crate) input: String,
    pub(crate) cursor: usize,
}

pub(crate) const SPINNER_FRAMES: &[&str] = &["✦", "✶", "✺", "✵", "✸", "✹", "✺"];
pub(crate) const SPINNER_FRAMES_INPUT: &[&str] = &["◉", "◎"];
pub(crate) const SPINNER_FRAMES_TOOL: &[&str] = &["●", "∙"];
pub(crate) const SPINNER_INTERVAL_MS: u128 = 50;

/// Cap on how many wrapped rows the input box can occupy before it starts to
/// scroll internally. Keeps the message area from collapsing when a user
/// pastes or types a long prompt while still showing enough context to edit.
pub(crate) const MAX_INPUT_VISIBLE_ROWS: usize = 5;
pub(crate) const MAX_PICKER_ROWS: usize = 6;

/// UI mode: normal chat or conversation picker.
pub(crate) enum AppMode {
    Chat,
    ResumePicker {
        items: Vec<ai_conversations::ConversationMeta>,
        cursor: usize,
    },
}

/// A single tool-call reference embedded in an AI header row.
#[derive(Clone)]
pub(crate) struct ToolRef {
    pub(crate) name: String,
    pub(crate) args: String,
    pub(crate) result: String,
    pub(crate) complete: bool,
    pub(crate) failed: bool,
}

/// Inline text style produced by the lightweight markdown tokenizer.
///
/// Only the four styles we can cleanly render in a narrow TUI: bold, italic,
/// monospace code, plain. Strikethrough is collapsed to plain (content kept,
/// markers dropped). Links keep the visible label and drop the URL.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InlineStyle {
    Plain,
    Bold,
    Italic,
    Code,
    /// ATX heading `#` prefix rendered before the heading body text.
    HeadingMarker,
    Highlighted(u8, u8, u8),
}

#[derive(Clone, Debug)]
pub(crate) struct InlineSpan {
    pub(crate) text: String,
    pub(crate) style: InlineStyle,
}

/// Block-level classification for a single wrapped display line.
///
/// Borrowed from `termimad`'s composite/block split: the block style controls
/// line-level decoration (indent, bullet, rule), while `InlineStyle` spans
/// inside the line carry character-level emphasis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DiffKind {
    None,
    Add,
    Remove,
    Hunk,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BlockStyle {
    Normal,
    Heading(u8),
    Quote,
    Hr,
    Code,
    DiffAdd,
    DiffRemove,
    DiffHunk,
    /// First wrapped line of a list item (renders the bullet/number); subsequent
    /// wrapped lines of the same item use `ListContinuation` to keep the indent
    /// without re-emitting the marker.
    ListItem,
    ListContinuation,
}

#[derive(Clone)]
pub(crate) enum DisplayLine {
    Header {
        role: Role,
        /// Tool calls attached to this AI header row. Always empty for User headers.
        tools: Vec<ToolRef>,
    },
    AttachmentSummary {
        labels: Vec<String>,
    },
    Text {
        segments: Vec<InlineSpan>,
        role: Role,
        block: BlockStyle,
    },
    /// Standalone "AI is thinking" indicator placed where the assistant's
    /// message will appear. The renderer substitutes the current spinner
    /// frame at draw time so the dot pulses without rebuilding the cache.
    LoadingDot,
    /// Collapsible reasoning header. Shows character count when collapsed,
    /// reasoning content follows as Quote-styled lines when expanded.
    ThinkingHeader {
        char_count: usize,
        expanded: bool,
    },
    Blank,
}

// ─── Markdown block IR ────────────────────────────────────────────────────────
//
// Intentionally minimalist. Inspired by termimad's two-pass design (block pass
// → inline tokenize) and glamour/glow's theme-driven styling, but scoped to
// the subset an LLM typically emits in a chat answer. We do NOT support:
// tables, reference links, footnotes, nested lists, HTML, setext headings.
//
// Streaming is handled by re-running the full parse on every content delta;
// partial (unclosed) emphasis renders as literal until its closer arrives,
// which matches termimad's behavior.

#[derive(Clone, Debug)]
pub(crate) enum MdBlock {
    Blank,
    Paragraph(String),
    Heading {
        level: u8,
        text: String,
    },
    Quote(String),
    ListItem {
        marker: String,
        text: String,
    },
    CodeLine {
        text: String,
        diff: DiffKind,
        lang: String,
    },
    Hr,
}

// ─── Input handling ──────────────────────────────────────────────────────────

pub(crate) enum Action {
    Continue,
    Quit,
}
