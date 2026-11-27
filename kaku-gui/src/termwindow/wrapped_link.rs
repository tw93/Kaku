//! Rejoin a link that a TUI hard-wrapped across rows.
//!
//! Claude Code and Codex wrap long tokens themselves and emit a real newline
//! plus an indent, so the terminal never sets the wrap attribute and each
//! fragment is scanned on its own (#547). Joining rows by shape alone was
//! reverted twice because unrelated output can fabricate a target, so this
//! module only proposes candidates. The caller keeps a file path only when
//! it exists on disk, and a URL only on the alternate screen across rows
//! whose content fills the full width, which is where Claude Code breaks a
//! token that does not fit; a word wrap always ends short of it.

use std::ops::Range;
use std::sync::Arc;
use termwiz::hyperlink::{Hyperlink, Rule};
use termwiz::surface::Line;
use wezterm_term::StableRowIndex;

/// Rows above and below the hovered row that a rejoined link may span.
pub const MAX_SPAN: usize = 4;

/// The rows within `MAX_SPAN` of `stable_row`, and where `stable_row` sits
/// in them, with the stable index of the first row. The range starts no higher than `scrollback_top`: for a start
/// above it `get_lines` returns rows from the bottom of the buffer instead,
/// so a link on one of the first rows after `clear` or in a new window was
/// never rejoined.
pub fn rows_around(
    stable_row: StableRowIndex,
    scrollback_top: StableRowIndex,
    get_lines: impl FnOnce(Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>),
) -> Option<(StableRowIndex, usize, Vec<Line>)> {
    let span = MAX_SPAN as StableRowIndex;
    let start = (stable_row - span).max(scrollback_top);
    let (top, lines) = get_lines(start..stable_row + span + 1);
    let hover_row = stable_row - top;
    (hover_row >= 0 && (hover_row as usize) < lines.len()).then_some((
        top,
        hover_row as usize,
        lines,
    ))
}

/// Columns a TUI may leave free at the right edge when it wraps there.
/// Claude Code stops about four columns short; a wide glyph adds one.
const EDGE_SLACK: usize = 6;

pub struct LinkRow<'a> {
    pub text: &'a str,
    /// The terminal wrap attribute: the next row continues this one verbatim.
    pub soft_wrapped: bool,
    /// The row's content runs to the right edge, as it does when a TUI
    /// breaks a token that does not fit.
    pub reaches_edge: bool,
    /// The row's content ends in the last column.
    pub fills_width: bool,
}

impl<'a> LinkRow<'a> {
    pub fn new(line: &Line, text: &'a str, cols: usize) -> Self {
        let content_end = line
            .visible_cells()
            .filter(|cell| !cell.str().trim().is_empty())
            .map(|cell| cell.cell_index() + cell.width())
            .last()
            .unwrap_or(0);
        Self {
            text,
            soft_wrapped: line.last_cell_was_wrapped(),
            reaches_edge: content_end + EDGE_SLACK >= cols,
            fills_width: content_end >= cols,
        }
    }
}

/// A link found by joining rows, with the byte range of each row's text
/// that it covers.
pub struct Rejoined {
    pub link: Arc<Hyperlink>,
    pub segments: Vec<(usize, Range<usize>)>,
}

impl Rejoined {
    pub fn is_file(&self) -> bool {
        self.link.uri().starts_with("file://")
    }
}

/// Byte offset in `line.as_str()` of the cell that covers `column`.
pub fn byte_offset_of_column(line: &Line, column: usize) -> Option<usize> {
    let mut offset = 0;
    for cell in line.visible_cells() {
        let start = cell.cell_index();
        if (start..start + cell.width().max(1)).contains(&column) {
            return Some(offset);
        }
        offset += cell.str().len();
    }
    None
}

/// Cell indices whose text starts inside `bytes` of `line.as_str()`.
pub fn cells_in_byte_range(line: &Line, bytes: &Range<usize>) -> Vec<usize> {
    let mut offset = 0;
    let mut cells = vec![];
    for cell in line.visible_cells() {
        if bytes.contains(&offset) {
            cells.push(cell.cell_index());
        }
        offset += cell.str().len();
    }
    cells
}

/// Put `link` on the cells of `line` whose text starts inside `bytes`,
/// leaving an explicit OSC 8 link in place.
pub fn apply_link_to_line(line: &mut Line, bytes: &Range<usize>, link: &Arc<Hyperlink>) {
    let targets = cells_in_byte_range(line, bytes);
    let cells = line.cells_mut_for_attr_changes_only();
    for idx in targets {
        if let Some(cell) = cells.get_mut(idx) {
            let attrs = cell.attrs_mut();
            if attrs.hyperlink().map_or(true, |link| link.is_implicit()) {
                attrs.set_hyperlink(Some(Arc::clone(link)));
            }
        }
    }
}

/// Links that cover byte `hover_byte` of `rows[hover_row]` once the row is
/// joined with its hard-wrapped neighbours, longest join first. Only links
/// that cross a row boundary are returned, since the hovered fragment on
/// its own is what failed. With `require_edge`, a hard break counts only
/// when the row above it reaches the right edge.
pub fn rejoined_links(
    rows: &[LinkRow],
    hover_row: usize,
    hover_byte: usize,
    rules: &[Rule],
    require_edge: bool,
) -> Vec<Rejoined> {
    if hover_row >= rows.len() {
        return vec![];
    }
    let joins = |upper: usize| -> bool {
        let (above, below) = (&rows[upper], &rows[upper + 1]);
        above.soft_wrapped
            || (!above.text.trim().is_empty()
                && !below.text.trim().is_empty()
                && (!require_edge || above.reaches_edge))
    };
    let mut first = hover_row;
    while first > 0 && hover_row - first < MAX_SPAN && joins(first - 1) {
        first -= 1;
    }
    let mut last = hover_row;
    while last + 1 < rows.len() && last - hover_row < MAX_SPAN && joins(last) {
        last += 1;
    }

    let mut spans = vec![];
    for top in first..=hover_row {
        for bottom in hover_row..=last {
            if top != bottom {
                spans.push((top, bottom));
            }
        }
    }
    spans.sort_by_key(|&(top, bottom)| std::cmp::Reverse(bottom - top));

    let mut found: Vec<Rejoined> = vec![];
    for (top, bottom) in spans {
        // (row, start in joined, start in row text, length) for each piece
        let mut pieces = vec![];
        let mut joined = String::new();
        let mut hovered = None;
        for idx in top..=bottom {
            let mut piece = rows[idx].text;
            let mut lead = 0;
            if idx > top && !rows[idx - 1].soft_wrapped {
                let trimmed = piece.trim_start();
                lead = piece.len() - trimmed.len();
                piece = trimmed;
            }
            if idx < bottom && !rows[idx].soft_wrapped {
                piece = piece.trim_end();
            }
            let start = joined.len();
            if idx == hover_row {
                if hover_byte < lead || hover_byte - lead >= piece.len() {
                    break;
                }
                hovered = Some((start + hover_byte - lead, start..start + piece.len()));
            }
            pieces.push((idx, start, lead, piece.len()));
            joined.push_str(piece);
        }
        let Some((offset, hovered_piece)) = hovered else {
            continue;
        };
        for m in Rule::match_hyperlinks(&joined, rules) {
            let crosses = m.range.start < hovered_piece.start || m.range.end > hovered_piece.end;
            if !crosses
                || !m.range.contains(&offset)
                || found.iter().any(|r| r.link.uri() == m.link.uri())
            {
                continue;
            }
            let segments = pieces
                .iter()
                .filter_map(|&(row, start, lead, len)| {
                    let from = m.range.start.max(start);
                    let to = m.range.end.min(start + len);
                    (from < to).then(|| (row, lead + from - start..lead + to - start))
                })
                .collect();
            found.push(Rejoined {
                link: m.link,
                segments,
            });
        }
    }
    found
}

/// Whether every hard break that `rejoined` crosses has the link running
/// into the last column. A URL has no existence check, so a TUI breaking a
/// token that does not fit is the only accepted shape.
pub fn breaks_at_full_width(rows: &[LinkRow], rejoined: &Rejoined) -> bool {
    let crossed = rejoined.segments.len().saturating_sub(1);
    rejoined.segments.iter().take(crossed).all(|(row, bytes)| {
        let row = &rows[*row];
        row.soft_wrapped || (row.fills_width && bytes.end == row.text.trim_end().len())
    })
}

/// What a `file://` candidate points at on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Missing,
    File,
    Dir,
}

/// The link to put under the pointer after the rule scan, when the scan
/// alone got it wrong: a path with spaces in it, or a path or URL that a
/// TUI hard-wrapped across rows (#547). `hovered` is the link the scan left
/// on the hovered cell. `path_kind` resolves a `file://` URI the way a click
/// would; nothing is proposed that does not exist on disk, and a URL is
/// rejoined only on the alternate screen across rows that fill the width.
pub fn hover_link(
    rows: &[LinkRow],
    hover_row: usize,
    hover_byte: usize,
    hovered: Option<&Arc<Hyperlink>>,
    rules: &[Rule],
    alt_screen: bool,
    path_kind: &dyn Fn(&str) -> PathKind,
) -> Option<Rejoined> {
    let text = rows.get(hover_row)?.text;
    let (before, after) = (text.get(..hover_byte)?, text.get(hover_byte..)?);

    // Existence checks are remembered per URI and drawn from two budgets:
    // the path-candidate search, which can propose many, and the steps
    // that propose a handful (diff prefix, bare name, rejoin), so a long
    // line spent on the first cannot blind the others. Past its budget a
    // check reports None.
    let candidate_budget = std::cell::Cell::new(MAX_CANDIDATE_CHECKS);
    let other_budget = std::cell::Cell::new(MAX_PATH_CHECKS - MAX_CANDIDATE_CHECKS);
    let checked = std::cell::RefCell::new(Vec::<(String, PathKind)>::new());
    let check = |uri: &str, budget: &std::cell::Cell<usize>| -> Option<PathKind> {
        if let Some((_, kind)) = checked.borrow().iter().find(|(u, _)| u == uri) {
            return Some(*kind);
        }
        if budget.get() == 0 {
            return None;
        }
        budget.set(budget.get() - 1);
        let kind = path_kind(uri);
        checked.borrow_mut().push((uri.to_string(), kind));
        Some(kind)
    };
    let path_kind = &|uri: &str| check(uri, &other_budget).unwrap_or(PathKind::Missing);
    if let Some(link) = hovered {
        if !link.is_implicit() {
            return None;
        }
        // Linked on an earlier hover and the row has not changed since.
        if link.uri().starts_with("file://") && link.uri().contains(char::is_whitespace) {
            return None;
        }
    }

    if let Some(link) = hovered {
        if let Some(found) = strip_diff_prefix(text, hover_byte, link, rules, path_kind) {
            return Some(Rejoined {
                segments: vec![(hover_row, found.0)],
                link: found.1,
            });
        }
    }

    let hovered_uri = hovered.map(|link| link.uri());
    let hovered_kind = hovered_uri
        .filter(|uri| uri.starts_with("file://"))
        .map(path_kind);

    // The hovered row and its terminal-wrap neighbours first; then, if that
    // finds nothing, across one hard break each way for a TUI word wrap.
    'passes: for hard_breaks in [false, true] {
        let joined = JoinedRows::around(rows, hover_row, hover_byte, hard_breaks);
        if hard_breaks && !joined.crosses_hard_break {
            break;
        }
        let Some(hover) = joined.hover else {
            continue;
        };
        // Names that end on the hovered row first: a long line of prose
        // after a path should not spend the budget before the path does.
        let hover_piece = joined
            .pieces
            .iter()
            .find(|piece| piece.0 == hover_row)
            .map(|&(_, start, _, len)| start..start + len);
        let mut candidates = spaced_path_candidates(&joined.text, hover);
        candidates.sort_by_key(|c| {
            let on_hover_row = hover_piece
                .as_ref()
                .is_some_and(|piece| c.range.end <= piece.end);
            (!on_hover_row, std::cmp::Reverse(c.range.len()))
        });
        for candidate in candidates {
            let path = candidate.path.as_str();
            let uri = format!("file://{path}");
            let only_prefix = path.trim_end_matches('/').is_empty()
                || ["~", ".", ".."].contains(&path.trim_end_matches('/'));
            // A rule link that exists keeps priority over its own prefix,
            // and an existing file (`app.py:12`) yields only to a longer
            // name with a space in it.
            let hovered_exists = hovered_kind.is_some_and(|k| k != PathKind::Missing);
            if only_prefix
                || (hovered_exists && hovered_uri.is_some_and(|h| h.starts_with(&uri)))
                || (hovered_kind == Some(PathKind::File) && !path.contains(char::is_whitespace))
            {
                continue;
            }
            // Most candidates share a parent folder; a missing one rules
            // them all out with one check.
            if let Some(slash) = path
                .rfind('/')
                .filter(|&i| i > 0 && !["~", ".", ".."].contains(&&path[..i]))
            {
                match check(&format!("file://{}", &path[..slash]), &candidate_budget) {
                    None => break 'passes,
                    Some(PathKind::Dir) => {}
                    Some(_) => continue,
                }
            }
            let Some(kind) = check(&uri, &candidate_budget) else {
                break 'passes;
            };
            if kind != PathKind::Missing {
                return Some(Rejoined {
                    link: Arc::new(Hyperlink::new_implicit(uri)),
                    segments: joined.segments(&candidate.range),
                });
            }
        }
    }

    if hovered.is_none() {
        if let Some((range, uri)) = bare_file_name(text, hover_byte) {
            if path_kind(&uri) == PathKind::File {
                return Some(Rejoined {
                    link: Arc::new(Hyperlink::new_implicit(uri)),
                    segments: vec![(hover_row, range)],
                });
            }
        }
    }

    // Only the first or last token of a row can continue on another row.
    let token_start = before
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(0, |(idx, c)| idx + c.len_utf8());
    let token_end = after
        .find(char::is_whitespace)
        .map_or(text.len(), |idx| hover_byte + idx);
    if token_end == hover_byte
        || (!before[..token_start].trim().is_empty() && !text[token_end..].trim().is_empty())
    {
        return None;
    }
    let require_edge = match hovered {
        Some(link) if link.uri().starts_with("file://") => match path_kind(link.uri()) {
            // The break fell after a `/` of a longer path.
            PathKind::Dir => true,
            PathKind::File => return None,
            PathKind::Missing => false,
        },
        _ => false,
    };
    rejoined_links(rows, hover_row, hover_byte, rules, require_edge)
        .into_iter()
        .find(|rejoined| {
            if rejoined.is_file() {
                path_kind(rejoined.link.uri()) != PathKind::Missing
            } else {
                alt_screen && breaks_at_full_width(rows, rejoined)
            }
        })
}

/// Where the hover cache key starts for `hover_byte`: the run of ASCII
/// word characters around it, split at path delimiters, or the character
/// itself in CJK text. What `hover_link` finds depends on this much of the
/// position (`Read(README.md)`, `~/Movies/吉隆坡旅行里`), so moving within it
/// can reuse the last result.
pub fn probe_start(text: &str, hover_byte: usize) -> usize {
    let at = text[hover_byte..].chars().next();
    if at.map_or(true, |c| {
        !c.is_ascii() || c.is_ascii_whitespace() || (c.is_ascii_punctuation() && !"_-".contains(c))
    }) {
        return hover_byte;
    }
    text[..hover_byte]
        .char_indices()
        .rfind(|&(_, c)| !c.is_ascii() || opens_path(c) || closes_path(c) || c == ')' || c == '/')
        .map_or(0, |(idx, c)| idx + c.len_utf8())
}

/// A file name with no `/` under the pointer, as `ls` and `go test`
/// print them (`README.md`, `main_test.go:12:`), with up to two `:N`
/// location suffixes. Only a dotted name qualifies, so plain words never
/// cost an existence check; the caller links it only when the file exists.
fn bare_file_name(text: &str, hover_byte: usize) -> Option<(Range<usize>, String)> {
    let ends_token = |c: char| opens_path(c) || closes_path(c) || c == ')';
    let start = text[..hover_byte]
        .char_indices()
        .rfind(|&(_, c)| ends_token(c))
        .map_or(0, |(idx, c)| idx + c.len_utf8());
    let end = text[hover_byte..]
        .char_indices()
        .find(|&(_, c)| ends_token(c))
        .map_or(text.len(), |(idx, _)| hover_byte + idx);
    let token = text[start..end].trim_end_matches(|c: char| ".,;:!?".contains(c));
    let end = start + token.len();
    if end <= hover_byte {
        return None;
    }
    let mut name = token;
    for _ in 0..2 {
        match name.rsplit_once(':') {
            Some((head, num)) if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) => {
                name = head;
            }
            _ => break,
        }
    }
    let (stem, ext) = name.rsplit_once('.')?;
    let valid = !stem.is_empty()
        && !ext.is_empty()
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "_.@+-".contains(c));
    valid.then(|| (start..end, format!("file://{token}")))
}

/// `git diff` prints `a/src/main.rs` and `b/src/main.rs`. When the hovered
/// path does not exist but the one without that prefix does, link that.
fn strip_diff_prefix(
    text: &str,
    hover_byte: usize,
    link: &Arc<Hyperlink>,
    rules: &[Rule],
    path_kind: &dyn Fn(&str) -> PathKind,
) -> Option<(Range<usize>, Arc<Hyperlink>)> {
    let rest = link
        .uri()
        .strip_prefix("file://a/")
        .or_else(|| link.uri().strip_prefix("file://b/"))?;
    let stripped = format!("file://{rest}");
    if path_kind(link.uri()) != PathKind::Missing || path_kind(&stripped) == PathKind::Missing {
        return None;
    }
    let m = Rule::match_hyperlinks(text, rules)
        .into_iter()
        .find(|m| m.link.uri() == link.uri() && m.range.contains(&hover_byte))?;
    Some((m.range, Arc::new(Hyperlink::new_implicit(stripped))))
}

/// The hovered row joined with the rows it continues into: verbatim across
/// a terminal wrap, and with one space across a hard break between two
/// non-empty rows, which is where a TUI word-wraps a path that has a space
/// in it. Only existence checks decide what the joined text links.
struct JoinedRows {
    text: String,
    /// Whether a hard break was joined.
    crosses_hard_break: bool,
    /// (row, start in `text`, start in the row's text, length)
    pieces: Vec<(usize, usize, usize, usize)>,
    /// `hover_byte` mapped into `text`; None when it sits in trimmed indent.
    hover: Option<usize>,
}

impl JoinedRows {
    /// With `hard_breaks`, at most one hard break is crossed above and one
    /// below the hovered row.
    fn around(rows: &[LinkRow], hover_row: usize, hover_byte: usize, hard_breaks: bool) -> Self {
        let hard = |upper: usize| {
            !rows[upper].soft_wrapped
                && !rows[upper].text.trim().is_empty()
                && !rows[upper + 1].text.trim().is_empty()
        };
        let (mut up_hard, mut down_hard) = (hard_breaks, hard_breaks);
        let mut crosses_hard_break = false;
        let mut first = hover_row;
        while first > 0 && hover_row - first < MAX_SPAN {
            if rows[first - 1].soft_wrapped {
                first -= 1;
            } else if up_hard && hard(first - 1) {
                up_hard = false;
                crosses_hard_break = true;
                first -= 1;
            } else {
                break;
            }
        }
        let mut last = hover_row;
        while last + 1 < rows.len() && last - hover_row < MAX_SPAN {
            if rows[last].soft_wrapped {
                last += 1;
            } else if down_hard && hard(last) {
                down_hard = false;
                crosses_hard_break = true;
                last += 1;
            } else {
                break;
            }
        }
        let mut joined = Self {
            text: String::new(),
            crosses_hard_break,
            pieces: vec![],
            hover: None,
        };
        for idx in first..=last {
            let mut piece = rows[idx].text;
            let mut lead = 0;
            if idx > first && !rows[idx - 1].soft_wrapped {
                let trimmed = piece.trim_start();
                lead = piece.len() - trimmed.len();
                piece = trimmed;
                joined.text.push(' ');
            }
            if idx < last && !rows[idx].soft_wrapped {
                piece = piece.trim_end();
            }
            let start = joined.text.len();
            if idx == hover_row && hover_byte >= lead && hover_byte - lead < piece.len() {
                joined.hover = Some(start + hover_byte - lead);
            }
            joined.pieces.push((idx, start, lead, piece.len()));
            joined.text.push_str(piece);
        }
        joined
    }

    /// The per-row byte ranges that `range` of the joined text covers.
    fn segments(&self, range: &Range<usize>) -> Vec<(usize, Range<usize>)> {
        self.pieces
            .iter()
            .filter_map(|&(row, start, lead, len)| {
                let from = range.start.max(start);
                let to = range.end.min(start + len);
                (from < to).then(|| (row, lead + from - start..lead + to - start))
            })
            .collect()
    }
}

/// A path proposed by `spaced_path_candidates`.
pub struct SpacedPath {
    /// Byte range of the path in the row text.
    pub range: Range<usize>,
    /// The path with `\ ` escapes resolved.
    pub path: String,
}

/// Starts of absolute, home and dot-relative paths. Bare relative names are
/// left out: with spaces allowed any word could start one.
const SPACED_PATH_PREFIXES: [&str; 4] = ["/", "~/", "./", "../"];

/// Nearest path starts left of the hover that are tried.
const MAX_SPACED_STARTS: usize = 4;

/// Existence checks one hover may cost, parents included. A hover runs on
/// the GUI thread and only when the pointer enters a new word.
const MAX_PATH_CHECKS: usize = 64;

/// The share of `MAX_PATH_CHECKS` the path-candidate search may use.
const MAX_CANDIDATE_CHECKS: usize = 48;

/// Ends tried inside one run of non-ASCII text (`吉隆坡旅行里面…`); a file
/// name rarely has more CJK characters in a row than this.
const MAX_CJK_RUN_ENDS: usize = 24;

/// Characters that may sit right before a path, as in the file-path rule.
fn opens_path(c: char) -> bool {
    c.is_whitespace() || "([<`'\"：，。、；！？（【「『《“‘".contains(c)
}

/// Characters that end a path even when spaces are allowed inside it:
/// quotes, brackets other than `(` `)` (Finder names copies `a (1).png`)
/// and full-width CJK punctuation.
fn closes_path(c: char) -> bool {
    c == '\t' || "[]{}<>`'\"：，。、；！？（）【】「」『』《》“”‘’".contains(c)
}

/// Paths that cover `hover_byte` of `text`, longest first, including the
/// shapes the file-path rule cannot take: spaces inside, and a CJK name
/// with a sentence running on after it. Joining words by shape would turn
/// prose into paths, so these are only proposals: the caller keeps the
/// first one that exists on disk.
pub fn spaced_path_candidates(text: &str, hover_byte: usize) -> Vec<SpacedPath> {
    if hover_byte >= text.len() || !text.is_char_boundary(hover_byte) {
        return vec![];
    }
    let hover_end = hover_byte + text[hover_byte..].chars().next().map_or(0, char::len_utf8);
    let mut starts: Vec<usize> = text[..hover_end]
        .char_indices()
        .filter(|&(idx, _)| {
            SPACED_PATH_PREFIXES
                .iter()
                .any(|p| text[idx..].starts_with(p))
                && text[..idx].chars().next_back().map_or(true, opens_path)
        })
        .map(|(idx, _)| idx)
        .collect();
    starts.reverse();
    starts.truncate(MAX_SPACED_STARTS);
    starts.sort_unstable();

    let mut found: Vec<SpacedPath> = vec![];
    let mut seen = std::collections::HashSet::new();
    for start in starts {
        let limit = text[start..]
            .char_indices()
            .find(|&(_, c)| closes_path(c))
            .map_or(text.len(), |(idx, _)| start + idx);
        if hover_byte >= limit {
            continue;
        }
        // A path may end at a space, before `)`, or where ASCII meets CJK
        // (`~/Movies/吉隆坡旅行里`), and inside a CJK run, since a sentence
        // runs straight on from a name with no space.
        let mut ends = vec![];
        let mut run = text[start..hover_byte]
            .chars()
            .rev()
            .take_while(|c| !c.is_ascii())
            .count();
        let mut prev_ascii = run == 0;
        for (idx, c) in text[hover_byte..limit].char_indices() {
            let boundary = c.is_whitespace() || c == ')' || c.is_ascii() != prev_ascii;
            if idx > 0 && (boundary || (!c.is_ascii() && run <= MAX_CJK_RUN_ENDS)) {
                ends.push(hover_byte + idx);
            }
            run = if c.is_ascii() { 0 } else { run + 1 };
            prev_ascii = c.is_ascii();
        }
        ends.push(limit);
        for end in ends {
            let raw = text[start..end].trim_end_matches(|c: char| ".,;:!?".contains(c));
            let end = start + raw.len();
            if end <= hover_byte || raw.ends_with(char::is_whitespace) || !seen.insert((start, end))
            {
                continue;
            }
            found.push(SpacedPath {
                range: start..end,
                path: raw.replace("\\ ", " "),
            });
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.range.len()));
    found
}

#[cfg(test)]
#[path = "link_corpus.rs"]
mod link_corpus;

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str =
        "/private/tmp/claude-501/-Users-tw93-www-yado/737fb7ad/scratchpad/shots/final-sheet.png";

    fn rules() -> Vec<Rule> {
        config::default_hyperlink_rules()
    }

    fn row(text: &str) -> LinkRow<'_> {
        LinkRow {
            text,
            soft_wrapped: false,
            reaches_edge: true,
            fills_width: false,
        }
    }

    fn uris(found: Vec<Rejoined>) -> Vec<String> {
        found.iter().map(|r| r.link.uri().to_string()).collect()
    }

    #[test]
    fn claude_code_wrapped_path_rejoins_from_either_fragment() {
        // The shape Claude Code printed: a CJK sentence, a path broken
        // mid-word, and the continuation indented by two columns with the
        // sentence running straight on after it.
        let (head, tail) = PATH.split_at(PATH.find("-www-ya").unwrap() + "-www-ya".len());
        let first = format!("  截图拼在一张图里：{head}");
        let second = format!("  {tail}，五张分别是照片删除");
        let rows = [row(&first), row(&second)];
        let expected = vec![format!("file://{PATH}")];

        let in_head = first.find("/private").unwrap() + 3;
        assert_eq!(
            uris(rejoined_links(&rows, 0, in_head, &rules(), false)),
            expected
        );
        let in_tail = second.find("do/").unwrap() + 1;
        let found = rejoined_links(&rows, 1, in_tail, &rules(), false);
        assert_eq!(
            uris(rejoined_links(&rows, 1, in_tail, &rules(), false)),
            expected
        );
        assert_eq!(
            found[0].segments,
            vec![
                (0, first.find("/private").unwrap()..first.len()),
                (1, 2..2 + tail.len()),
            ],
            "segments cover each fragment without the indent or the trailing sentence"
        );
    }

    #[test]
    fn rows_near_the_top_of_the_scrollback_are_the_rows_around_the_hover() {
        // A fresh 22-row screen: rows above the hovered one do not exist,
        // and asking for them used to bring back the bottom of the buffer.
        let mut term = wezterm_term::Terminal::new(
            wezterm_term::TerminalSize {
                rows: 22,
                cols: 40,
                pixel_width: 320,
                pixel_height: 352,
                dpi: 0,
            },
            Arc::new(config::TermConfig::new()),
            "test",
            "test",
            Box::new(std::io::sink()),
        );
        term.advance_bytes("  see /tmp/kaku-wr\r\n  ap/sheet.png here\r\n");
        for hovered in 0..2 {
            let (_, hover_row, lines) = rows_around(hovered, 0, |range| {
                mux::renderable::terminal_get_lines(&mut term, range)
            })
            .expect("the hovered row is in range");
            assert_eq!(hover_row, hovered as usize);
            assert_eq!(lines[0].as_str().trim_end(), "  see /tmp/kaku-wr");
            assert_eq!(lines[1].as_str().trim_end(), "  ap/sheet.png here");
        }
    }

    #[test]
    fn a_tail_without_a_slash_still_rejoins() {
        // `sheet.png` alone is not a link, so hovering it must still find
        // the path that ends on the row above.
        let rows = [row("  see /tmp/kaku-wrap/"), row("  sheet.png here")];
        assert_eq!(
            uris(rejoined_links(&rows, 1, 3, &rules(), false)),
            vec!["file:///tmp/kaku-wrap/sheet.png".to_string()]
        );
    }

    #[test]
    fn path_over_three_rows_rejoins_from_the_middle() {
        let rows = [
            row("  /private/tmp/claude-501/-Users"),
            row("  -tw93-www-yado/737fb7ad/scratchpad"),
            row("  /shots/final-sheet.png"),
        ];
        assert_eq!(
            uris(rejoined_links(&rows, 1, 6, &rules(), false))[0],
            format!("file://{PATH}")
        );
    }

    #[test]
    fn blank_row_ends_the_join() {
        let rows = [
            row("  /private/tmp/claude-501/-Users-tw93-www-ya"),
            row(""),
            row("  do/x.png"),
        ];
        assert!(rejoined_links(&rows, 0, 5, &rules(), false).is_empty());
    }

    #[test]
    fn edge_requirement_rejects_a_short_row() {
        // `cd /Users/tw93/www/` followed by `ls` output: the first row is a
        // complete directory that stops well short of the edge.
        let rows = [
            LinkRow {
                reaches_edge: false,
                ..row("cd /Users/tw93/www/")
            },
            row("kaku  mole  pake"),
        ];
        assert!(rejoined_links(&rows, 0, 5, &rules(), true).is_empty());
        assert_eq!(
            uris(rejoined_links(&rows, 0, 5, &rules(), false)),
            vec!["file:///Users/tw93/www/kaku".to_string()],
            "without the edge requirement the caller's existence check decides"
        );
    }

    #[test]
    fn hovers_in_the_indent_or_outside_a_crossing_link_find_nothing() {
        let rows = [row("  see /tmp/a.png here"), row("  and more")];
        assert!(rejoined_links(&rows, 1, 0, &rules(), false).is_empty());
        assert!(rejoined_links(&rows, 0, 8, &rules(), false).is_empty());
    }

    #[test]
    fn soft_wrapped_rows_keep_their_leading_spaces() {
        let rows = [
            LinkRow {
                soft_wrapped: true,
                ..row("/tmp/a b")
            },
            row(" /c.png"),
        ];
        assert!(rejoined_links(&rows, 0, 2, &rules(), false).is_empty());
    }

    fn urls(rows: &[LinkRow], hover_row: usize, hover_byte: usize) -> Vec<Rejoined> {
        rejoined_links(rows, hover_row, hover_byte, &rules(), true)
            .into_iter()
            .filter(|r| !r.is_file() && breaks_at_full_width(rows, r))
            .collect()
    }

    #[test]
    fn url_broken_at_full_width_rejoins() {
        // Claude Code 2.1.310 at 60 columns, replayed through a terminal
        // emulator: the URL fills the first row to column 60 and continues
        // on the next one. Without the fix the second row's fragment was
        // detected as the relative path `termwindow/render/...`.
        let rows = [
            LinkRow {
                fills_width: true,
                ..row("⏺ Bare: https://github.com/tw93/Kaku/blob/main/kaku-gui/src/")
            },
            row("  termwindow/render/screen_line.rs#L120-L180"),
        ];
        let expected = "https://github.com/tw93/Kaku/blob/main/kaku-gui/src/termwindow/render/screen_line.rs#L120-L180";
        assert_eq!(urls(&rows, 1, 5)[0].link.uri(), expected);
        assert_eq!(urls(&rows, 0, 20)[0].link.uri(), expected);
    }

    #[test]
    fn complete_url_before_a_word_wrap_is_not_glued_to_the_next_word() {
        // A URL that fit, then a word that did not: the row ends short of
        // the last column, so the next word must not join it.
        let rows = [
            row("  see https://example.com/docs/page"),
            row("  carefully please"),
        ];
        assert!(
            !rejoined_links(&rows, 0, 10, &rules(), true).is_empty(),
            "the shape alone would glue the next word"
        );
        assert!(urls(&rows, 0, 10).is_empty());
    }

    #[test]
    fn url_ending_mid_row_above_a_full_row_is_not_joined() {
        // Only the row the link leaves from counts: a full row below does
        // not make the break above it a token break.
        let rows = [
            row("  see https://example.com/docs/page"),
            LinkRow {
                fills_width: true,
                ..row("  next")
            },
        ];
        assert!(urls(&rows, 0, 10).is_empty());
    }

    fn spaced(text: &str, hover: &str) -> Vec<String> {
        spaced_path_candidates(text, text.find(hover).unwrap())
            .into_iter()
            .map(|c| c.path)
            .collect()
    }

    #[test]
    fn spaced_path_is_proposed_from_any_word_and_stops_at_cjk_punctuation() {
        let text = "/Users/tw93/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4，全片";
        let full = "/Users/tw93/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4";
        for hover in ["Users", "旅行", "2026-11.mp4"] {
            let found = spaced(text, hover);
            assert_eq!(
                found.first().map(String::as_str),
                Some(full),
                "hover {hover}"
            );
        }
        assert!(spaced(text, "旅行")
            .contains(&"/Users/tw93/Movies/2026-11 吉隆坡旅行/吉隆坡".to_string()));
        assert!(spaced(text, "全片").is_empty());
    }

    #[test]
    fn spaced_path_candidates_shrink_at_spaces_and_trailing_punctuation() {
        let text = "saved to /tmp/kaku dir/a b.png. Done";
        assert_eq!(
            spaced(text, "kaku"),
            vec![
                "/tmp/kaku dir/a b.png. Done".to_string(),
                "/tmp/kaku dir/a b.png".to_string(),
                "/tmp/kaku dir/a".to_string(),
                "/tmp/kaku".to_string(),
            ]
        );
        // The word before the path is not a start, so it never joins in.
        assert!(spaced(text, "saved").is_empty());
    }

    #[test]
    fn spaced_path_resolves_escapes_and_keeps_finder_copy_names() {
        assert_eq!(
            spaced("open ~/Movies/2026-11\\ 吉隆坡旅行/a.mp4", "Movies")
                .first()
                .cloned(),
            Some("~/Movies/2026-11 吉隆坡旅行/a.mp4".to_string())
        );
        let text = "(/tmp/shot (1).png)";
        assert!(spaced(text, "shot").contains(&"/tmp/shot (1).png".to_string()));
        // Quotes bound the path on both sides.
        assert_eq!(
            spaced("'/tmp/a b.txt' and more", "a b").first().cloned(),
            Some("/tmp/a b.txt".to_string())
        );
    }

    #[test]
    fn candidates_shrink_inside_a_cjk_run() {
        // hover_link skips the candidate equal to what the rule linked.
        assert_eq!(
            spaced("see /tmp/a.png", "a.png"),
            vec!["/tmp/a.png".to_string()]
        );
        assert_eq!(
            spaced("视频在 ~/Movies/吉隆坡旅行里", "旅行"),
            vec![
                "~/Movies/吉隆坡旅行里".to_string(),
                "~/Movies/吉隆坡旅行".to_string(),
                "~/Movies/吉隆坡旅".to_string(),
            ]
        );
    }

    #[test]
    fn first_existing_spaced_candidate_is_the_whole_file() {
        // The caller's rule: keep the longest candidate that exists. Here
        // `2026-11` alone does not exist, the folder is `2026-11 吉隆坡旅行`.
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("2026-11 吉隆坡旅行");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("吉隆坡 2026-11.mp4"), b"").unwrap();
        let full = folder.join("吉隆坡 2026-11.mp4");
        let text = format!("{}，全片", full.display());
        for hover in ["2026-11 ", "旅行", "mp4"] {
            let found = spaced_path_candidates(&text, text.find(hover).unwrap())
                .into_iter()
                .find(|c| std::path::Path::new(&c.path).exists())
                .map(|c| c.path);
            assert_eq!(
                found.as_deref(),
                Some(full.to_str().unwrap()),
                "hover {hover}"
            );
        }
    }

    #[test]
    fn probe_start_changes_where_the_hover_result_can_change() {
        let text = "⏺ Read(README.md) 视频在吉隆坡旅行里";
        let at = |needle: &str| probe_start(text, text.find(needle).unwrap());
        assert_ne!(
            at("Read"),
            at("README"),
            "a new word inside the parentheses"
        );
        assert_eq!(at("README"), at("EADME"), "same word, same result");
        assert_ne!(at("旅"), at("行"), "each CJK character can end a name");
    }

    #[test]
    fn one_hover_checks_the_disk_a_bounded_number_of_times() {
        let calls = std::cell::Cell::new(0);
        let line = "/tmp words ".repeat(30);
        let texts = [line.clone(), line.clone(), line];
        let rows: Vec<LinkRow> = texts.iter().map(|t| row(t)).collect();
        // `/tmp` has no parent to prune by and nothing exists, so every
        // candidate across both passes would be checked without a budget.
        let found = hover_link(&rows, 1, 2, None, &rules(), false, &|_| {
            calls.set(calls.get() + 1);
            PathKind::Missing
        });
        assert!(found.is_none());
        assert!(calls.get() <= MAX_PATH_CHECKS, "{} checks", calls.get());
    }

    #[test]
    fn probe_start_is_the_pointer_itself_on_delimiters() {
        let text = "⏺ Read(README.md) x";
        let at = |needle: &str| probe_start(text, text.find(needle).unwrap());
        assert_ne!(at(")"), at("README"), "entering from the right at `)`");
        let ls = "Cargo.lock  README.md  src";
        let at = |byte: usize| probe_start(ls, byte);
        let space_after = ls.find("README.md").unwrap() + "README.md".len();
        assert_ne!(at(space_after), at(ls.find("README").unwrap()));
    }

    #[test]
    fn probe_start_is_the_pointer_itself_on_trailing_punctuation() {
        let go = "    main_test.go:12: got 1";
        let colon = go.find("12:").unwrap() + 2;
        assert_ne!(probe_start(go, colon), probe_start(go, colon - 1));
        let dot = "see README.md.";
        assert_ne!(
            probe_start(dot, dot.len() - 1),
            probe_start(dot, dot.len() - 2)
        );
    }
}
