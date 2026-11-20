//! End-to-end link corpus: what a Cmd+click opens for text as it appears on
//! screen. Each case runs the same steps as a hover in the GUI: the rule
//! scan over each logical line (`Pane::apply_hyperlinks`), the hover-time
//! repair (`wrapped_link::hover_link`), the cell update, then reading the
//! hovered cell's link and resolving it the way a click does
//! (`TermWindow::resolve_file_uri`). Files live in a temp dir, so the
//! existence checks are real.
//!
//! Add a case here when a link shape is reported unclickable.

use super::super::wrapped_link::{self, LinkRow, PathKind};
use super::super::TermWindow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use termwiz::cell::CellAttributes;
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::Line;

const FILES: &[&str] = &[
    "proj/src/main.rs",
    "proj/README.md",
    "proj/kaku/src/main.rs",
    "proj/app.py",
    "proj/main.go",
    "proj/index.js",
    "proj/src/index.ts",
    "proj/main_test.go",
    "proj/main.c",
    "proj/node_modules/@scope/pkg/index.js",
    "proj/文档/说明.md",
    "proj/docs/a b.md",
    "proj/very/long/directory/structure/for/wrapping/final-sheet.png",
    "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
    "home/Movies/吉隆坡旅行/day1.mp4",
    "home/Downloads/shot (1).png",
    "home/Downloads/shot(2).png",
    "home/.config/kaku/kaku.lua",
    "home/Downloads/Archive (1)/x.txt",
    "home/Downloads/Project(1)/x.txt",
    "home/Desktop/x.txt",
    "home/Downloads/My Folder/x.txt",
];

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for file in FILES {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"").unwrap();
        }
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn expand(&self, text: &str) -> String {
        text.replace("{T}", &self.root().display().to_string())
    }

    fn resolve(&self, uri: &str) -> Option<super::super::FileLinkTarget> {
        TermWindow::resolve_file_uri(uri, Some(self.root().join("home")), || {
            Some(self.root().join("proj"))
        })
    }

    fn kind(&self, uri: &str) -> PathKind {
        match self.resolve(uri) {
            Some(t) if t.path.is_dir() => PathKind::Dir,
            Some(t) if t.path.exists() => PathKind::File,
            _ => PathKind::Missing,
        }
    }
}

enum Expect {
    Url(&'static str),
    /// Path relative to the fixture root, line, column.
    File(&'static str, Option<usize>, Option<usize>),
    Nothing,
}

struct Row {
    text: &'static str,
    /// The terminal wrap attribute: the next row continues this one.
    soft_wrapped: bool,
    /// An OSC 8 link the program put on the whole row.
    osc8: Option<&'static str>,
}

fn row(text: &'static str) -> Row {
    Row {
        text,
        soft_wrapped: false,
        osc8: None,
    }
}

fn soft(text: &'static str) -> Row {
    Row {
        soft_wrapped: true,
        ..row(text)
    }
}

fn osc8(text: &'static str, uri: &'static str) -> Row {
    Row {
        osc8: Some(uri),
        ..row(text)
    }
}

struct Case {
    name: &'static str,
    rows: Vec<Row>,
    /// Terminal width; defaults to the widest row.
    cols: Option<usize>,
    alt_screen: bool,
    /// Row index and a substring of it to hover (its first occurrence).
    hover: (usize, &'static str),
    expect: Expect,
}

fn case(name: &'static str, rows: Vec<Row>, hover: (usize, &'static str), expect: Expect) -> Case {
    Case {
        name,
        rows,
        cols: None,
        alt_screen: false,
        hover,
        expect,
    }
}

/// One row, hover `hover` in it.
fn one(name: &'static str, text: &'static str, hover: &'static str, expect: Expect) -> Case {
    case(name, vec![row(text)], (0, hover), expect)
}

fn column_of_byte(line: &Line, byte: usize) -> Option<usize> {
    let mut offset = 0;
    for cell in line.visible_cells() {
        if offset == byte {
            return Some(cell.cell_index());
        }
        offset += cell.str().len();
    }
    None
}

/// The URI a click on the hovered cell would open, after the GUI's hover steps.
fn clicked_uri(fx: &Fixture, case: &Case) -> Result<Option<String>, String> {
    let rules = config::default_hyperlink_rules();
    let mut lines: Vec<Line> = case
        .rows
        .iter()
        .map(|r| {
            let mut line = Line::from_text(&fx.expand(r.text), &CellAttributes::default(), 1, None);
            if r.soft_wrapped {
                // The terminal marks the last visible cell, which for a
                // row ending in a wide glyph is not the last stored cell
                // that `set_last_cell_was_wrapped` writes.
                if let Some(idx) = line.visible_cells().last().map(|c| c.cell_index()) {
                    line.cells_mut()[idx].attrs_mut().set_wrapped(true);
                }
            }
            if let Some(uri) = r.osc8 {
                let link = Arc::new(Hyperlink::new(fx.expand(uri)));
                for cell in line.cells_mut() {
                    cell.attrs_mut().set_hyperlink(Some(Arc::clone(&link)));
                }
            }
            line
        })
        .collect();
    let cols = case
        .cols
        .unwrap_or_else(|| lines.iter().map(|l| l.len()).max().unwrap_or(0));

    // Pane::apply_hyperlinks scans each logical line once.
    let mut start = 0;
    while start < lines.len() {
        let mut end = start;
        while end + 1 < lines.len() && lines[end].last_cell_was_wrapped() {
            end += 1;
        }
        let mut group: Vec<&mut Line> = lines[start..=end].iter_mut().collect();
        Line::apply_hyperlink_rules(&rules, &mut group);
        start = end + 1;
    }

    let (hover_row, needle) = case.hover;
    let texts: Vec<String> = lines.iter().map(|l| l.as_str().into_owned()).collect();
    let hover_byte = texts[hover_row]
        .find(&fx.expand(needle))
        .ok_or_else(|| format!("hover text {needle:?} not in row {hover_row}"))?;
    let column = column_of_byte(&lines[hover_row], hover_byte)
        .ok_or_else(|| "hover byte is not a cell start".to_string())?;
    let hovered = lines[hover_row]
        .get_cell(column)
        .and_then(|cell| cell.attrs().hyperlink().cloned());

    let rows: Vec<LinkRow> = lines
        .iter()
        .zip(&texts)
        .map(|(line, text)| LinkRow::new(line, text, cols))
        .collect();
    let repaired = wrapped_link::hover_link(
        &rows,
        hover_row,
        hover_byte,
        hovered.as_ref(),
        &rules,
        case.alt_screen,
        &|uri| fx.kind(uri),
    );
    drop(rows);
    if let Some(rejoined) = repaired {
        for (row, bytes) in &rejoined.segments {
            wrapped_link::apply_link_to_line(&mut lines[*row], bytes, &rejoined.link);
        }
    }
    Ok(lines[hover_row]
        .get_cell(column)
        .and_then(|cell| cell.attrs().hyperlink().map(|l| l.uri().to_string())))
}

fn check(fx: &Fixture, case: &Case) -> Result<(), String> {
    let uri = clicked_uri(fx, case)?;
    match (&case.expect, uri) {
        (Expect::Nothing, None) => Ok(()),
        (Expect::Nothing, Some(uri)) => Err(format!("expected no link, got {uri}")),
        (_, None) => Err("no link under the pointer".to_string()),
        (Expect::Url(want), Some(uri)) => {
            if uri == *want {
                Ok(())
            } else {
                Err(format!("opens {uri}, want {want}"))
            }
        }
        (Expect::File(rel, line, col), Some(uri)) => {
            let target = fx
                .resolve(&uri)
                .ok_or_else(|| format!("{uri} does not resolve"))?;
            let want = fx.root().join(rel);
            let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => a == b,
            };
            if same(&target.path, &want) && target.line == *line && target.col == *col {
                Ok(())
            } else {
                Err(format!(
                    "opens {} line {:?} col {:?} (from {uri}), want {} line {line:?} col {col:?}",
                    target.path.display(),
                    target.line,
                    target.col,
                    PathBuf::from(rel).display()
                ))
            }
        }
    }
}

fn corpus() -> Vec<Case> {
    use Expect::*;
    vec![
        // ---- URLs ----
        one(
            "plain url",
            "see https://github.com/tw93/Kaku for more",
            "github",
            Url("https://github.com/tw93/Kaku"),
        ),
        one(
            "url sentence period",
            "Docs: https://kaku.fun/docs.",
            "kaku",
            Url("https://kaku.fun/docs"),
        ),
        one(
            "url query fragment",
            "open https://example.com/a?b=1&c=2#frag now",
            "example",
            Url("https://example.com/a?b=1&c=2#frag"),
        ),
        one(
            "localhost port",
            "Local:   http://localhost:3000/",
            "localhost",
            Url("http://localhost:3000/"),
        ),
        one(
            "ipv4 port",
            "GET http://127.0.0.1:8080/health 200",
            "127",
            Url("http://127.0.0.1:8080/health"),
        ),
        one(
            "ipv6",
            "listening on http://[::1]:8080/",
            "8080",
            Url("http://[::1]:8080/"),
        ),
        one(
            "wikipedia parens",
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "wiki",
            Url("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
        ),
        one(
            "markdown link",
            "[Kaku](https://github.com/tw93/Kaku)",
            "github",
            Url("https://github.com/tw93/Kaku"),
        ),
        one(
            "angle brackets",
            "<https://kaku.fun>",
            "kaku",
            Url("https://kaku.fun"),
        ),
        one(
            "in parentheses",
            "(see https://kaku.fun/docs)",
            "docs",
            Url("https://kaku.fun/docs"),
        ),
        one(
            "double quoted",
            r#"url = "https://kaku.fun/a""#,
            "kaku",
            Url("https://kaku.fun/a"),
        ),
        one(
            "json array first",
            r#"["https://a.example.com/x","https://b.example.com/y"]"#,
            "a.example",
            Url("https://a.example.com/x"),
        ),
        one(
            "json array second",
            r#"["https://a.example.com/x","https://b.example.com/y"]"#,
            "b.example",
            Url("https://b.example.com/y"),
        ),
        one(
            "cjk before and after",
            "访问https://kaku.fun即可",
            "kaku",
            Url("https://kaku.fun"),
        ),
        one(
            "cjk punctuation around",
            "地址：https://kaku.fun/docs，谢谢",
            "docs",
            Url("https://kaku.fun/docs"),
        ),
        one(
            "full-width parens",
            "（https://kaku.fun）",
            "kaku",
            Url("https://kaku.fun"),
        ),
        one(
            "trailing colon",
            "see https://kaku.fun:",
            "kaku",
            Url("https://kaku.fun"),
        ),
        one(
            "issue comment anchor",
            "https://github.com/tw93/Kaku/pull/628#issuecomment-123",
            "pull",
            Url("https://github.com/tw93/Kaku/pull/628#issuecomment-123"),
        ),
        one(
            "percent encoded",
            "https://zh.wikipedia.org/wiki/%E5%90%89",
            "wiki",
            Url("https://zh.wikipedia.org/wiki/%E5%90%89"),
        ),
        one(
            "ssh scheme",
            "ssh://git@github.com/tw93/Kaku.git",
            "github",
            Url("ssh://git@github.com/tw93/Kaku.git"),
        ),
        one(
            "git scp remote",
            "origin  git@github.com:tw93/Kaku.git (fetch)",
            "tw93",
            Url("https://github.com/tw93/Kaku"),
        ),
        one(
            "email",
            "contact tw93@example.com today",
            "example",
            Url("mailto:tw93@example.com"),
        ),
        one(
            "bare domain path",
            "github.com/tw93/Kaku is the repo",
            "github",
            Url("https://github.com/tw93/Kaku"),
        ),
        one(
            "www domain",
            "visit www.kaku.fun now",
            "kaku",
            Url("https://www.kaku.fun"),
        ),
        one(
            "bare domain after cjk",
            "官网kaku.fun",
            "kaku",
            Url("https://kaku.fun"),
        ),
        one(
            "file scheme",
            "file://{T}/proj/README.md",
            "README",
            File("proj/README.md", None, None),
        ),
        case(
            "osc8 explicit",
            vec![osc8("click here", "https://kaku.fun/osc8")],
            (0, "here"),
            Url("https://kaku.fun/osc8"),
        ),
        // ---- paths ----
        one(
            "absolute",
            "{T}/proj/src/main.rs",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "absolute line",
            "{T}/proj/src/main.rs:12",
            "main",
            File("proj/src/main.rs", Some(12), None),
        ),
        one(
            "absolute line col",
            "{T}/proj/src/main.rs:12:5",
            "main",
            File("proj/src/main.rs", Some(12), Some(5)),
        ),
        one(
            "rustc arrow",
            "  --> src/main.rs:12:5",
            "main",
            File("proj/src/main.rs", Some(12), Some(5)),
        ),
        one(
            "gcc style",
            "src/main.rs:12:5: error: expected `;`",
            "main",
            File("proj/src/main.rs", Some(12), Some(5)),
        ),
        one(
            "grep -n",
            "src/main.rs:12:fn main() {",
            "main",
            File("proj/src/main.rs", Some(12), None),
        ),
        one(
            "python traceback",
            r#"  File "{T}/proj/app.py", line 12, in <module>"#,
            "app",
            File("proj/app.py", Some(12), None),
        ),
        one(
            "tsc paren location",
            "src/index.ts(12,5): error TS2322",
            "index",
            File("proj/src/index.ts", Some(12), Some(5)),
        ),
        one(
            "rust panic",
            "thread 'main' panicked at src/main.rs:12:5:",
            "main.rs",
            File("proj/src/main.rs", Some(12), Some(5)),
        ),
        one(
            "go test bare file",
            "    main_test.go:12: got 1, want 2",
            "main_test",
            File("proj/main_test.go", Some(12), None),
        ),
        one(
            "ls bare file",
            "Cargo.lock  README.md  src",
            "README",
            File("proj/README.md", None, None),
        ),
        one(
            "bare word not a file",
            "e.g. version 1.2.3 works",
            "1.2",
            Nothing,
        ),
        one(
            "cjk quoted url",
            "“https://kaku.fun/a”",
            "kaku",
            Url("https://kaku.fun/a"),
        ),
        one(
            "cjk quoted path",
            "打开“{T}/proj/README.md”看看",
            "README",
            File("proj/README.md", None, None),
        ),
        one(
            "finder copy no space",
            "~/Downloads/shot(2).png",
            "shot",
            File("home/Downloads/shot(2).png", None, None),
        ),
        case(
            "osc8 file",
            vec![osc8("main.rs", "file://{T}/proj/src/main.rs")],
            (0, "main"),
            File("proj/src/main.rs", None, None),
        ),
        case(
            "osc8 not overridden",
            vec![osc8(
                "~/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                "https://kaku.fun/osc8",
            )],
            (0, "旅行"),
            Url("https://kaku.fun/osc8"),
        ),
        one(
            "go panic",
            "\t{T}/proj/main.go:12 +0x1d",
            "main.go",
            File("proj/main.go", Some(12), None),
        ),
        one(
            "js stack",
            "    at foo ({T}/proj/index.js:12:5)",
            "index",
            File("proj/index.js", Some(12), Some(5)),
        ),
        one(
            "home",
            "config at ~/.config/kaku/kaku.lua",
            "kaku.lua",
            File("home/.config/kaku/kaku.lua", None, None),
        ),
        one(
            "dot relative",
            "./src/main.rs",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "dot dot relative",
            "../proj/README.md",
            "README",
            File("proj/README.md", None, None),
        ),
        one(
            "bare relative",
            "edit kaku/src/main.rs",
            "main",
            File("proj/kaku/src/main.rs", None, None),
        ),
        one(
            "scoped package",
            "node_modules/@scope/pkg/index.js",
            "index",
            File("proj/node_modules/@scope/pkg/index.js", None, None),
        ),
        one(
            "cjk file name",
            "看 {T}/proj/文档/说明.md 这个",
            "说明",
            File("proj/文档/说明.md", None, None),
        ),
        one(
            "cjk dir then prose",
            "视频在 ~/Movies/吉隆坡旅行里",
            "旅行",
            File("home/Movies/吉隆坡旅行", None, None),
        ),
        one(
            "backticks",
            "open `src/main.rs` first",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "markdown link to path",
            "[readme](./README.md)",
            "README",
            File("proj/README.md", None, None),
        ),
        one(
            "claude tool header",
            "⏺ Update(src/main.rs)",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "after full-width colon",
            "截图：{T}/proj/src/main.rs",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "git diff a prefix",
            "--- a/src/main.rs",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        one(
            "git diff b prefix",
            "+++ b/src/main.rs",
            "main",
            File("proj/src/main.rs", None, None),
        ),
        // ---- paths with spaces ----
        one(
            "spaced path from start",
            "~/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4，全片",
            "Movies",
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        one(
            "spaced path middle",
            "~/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4，全片",
            "旅行",
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        one(
            "spaced path tail",
            "~/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4，全片",
            "11.mp4",
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        one(
            "escaped spaces",
            r"open ~/Movies/2026-11\ 吉隆坡旅行/吉隆坡\ 2026-11.mp4",
            "Movies",
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        one(
            "single quoted spaced",
            "'{T}/proj/docs/a b.md'",
            "docs",
            File("proj/docs/a b.md", None, None),
        ),
        one(
            "finder copy name",
            "saved ~/Downloads/shot (1).png",
            "shot",
            File("home/Downloads/shot (1).png", None, None),
        ),
        one(
            "prose not joined",
            "see /tmp/kaku-none and more",
            "and",
            Nothing,
        ),
        // ---- soft wrap (terminal wrap attribute) ----
        case(
            "soft wrapped url",
            vec![
                soft("go to https://github.com/tw9"),
                row("3/Kaku/issues now"),
            ],
            (1, "issues"),
            Url("https://github.com/tw93/Kaku/issues"),
        ),
        case(
            "soft wrapped path",
            vec![
                soft("open {T}/proj/very/long/dire"),
                row("ctory/structure/for/wrapping/final-sheet.png ok"),
            ],
            (1, "final"),
            File(
                "proj/very/long/directory/structure/for/wrapping/final-sheet.png",
                None,
                None,
            ),
        ),
        case(
            "soft wrapped spaced path",
            vec![
                soft("~/Movies/2026-11 吉隆坡"),
                row("旅行/吉隆坡 2026-11.mp4"),
            ],
            (1, "11.mp4"),
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        // ---- hard wrap by a TUI (#547) ----
        case(
            "claude wrapped path head",
            vec![
                row("  截图：{T}/proj/very/long/dire"),
                row("  ctory/structure/for/wrapping/final-sheet.png，看看"),
            ],
            (0, "very"),
            File(
                "proj/very/long/directory/structure/for/wrapping/final-sheet.png",
                None,
                None,
            ),
        ),
        case(
            "claude wrapped path tail",
            vec![
                row("  截图：{T}/proj/very/long/dire"),
                row("  ctory/structure/for/wrapping/final-sheet.png，看看"),
            ],
            (1, "final"),
            File(
                "proj/very/long/directory/structure/for/wrapping/final-sheet.png",
                None,
                None,
            ),
        ),
        Case {
            name: "claude wrapped url on alt screen",
            rows: vec![
                row("  see https://github.com/tw93/Kaku/iss"),
                row("  ues/547 for details"),
            ],
            cols: Some(38),
            alt_screen: true,
            hover: (1, "547"),
            expect: Url("https://github.com/tw93/Kaku/issues/547"),
        },
        case(
            "word wrapped spaced path",
            vec![
                row("  文件在 ~/Movies/2026-11"),
                row("  吉隆坡旅行/吉隆坡 2026-11.mp4"),
            ],
            (1, "11.mp4"),
            File(
                "home/Movies/2026-11 吉隆坡旅行/吉隆坡 2026-11.mp4",
                None,
                None,
            ),
        ),
        // ---- review round 1 ----
        case("spaced path above long prose", vec![row("see {T}/proj/docs/a b.md"), row("  and then a lot of unrelated words follow here for a while"), row("  more unrelated words keep coming on this row as well ok"), row("  still more words that are not part of any path at all"), row("  the last row of prose words before the prompt returns")], (0, "docs"), File("proj/docs/a b.md", None, None)),
        one("cjk dir before a long sentence", "视频在 ~/Movies/吉隆坡旅行里面有很多我拍的素材还有一些朋友发给我的照片和视频都放在一起了以后慢慢整理吧先这样放着不动以后再说吧真的太多了", "旅行", File("home/Movies/吉隆坡旅行", None, None)),
        one("folder named like a copy", "unzipped to ~/Downloads/Archive (1)", "Archive", File("home/Downloads/Archive (1)", None, None)),
        one("folder with paren suffix", "~/Downloads/Project(1)", "Project", File("home/Downloads/Project(1)", None, None)),
        one("lone slash", "Continue? [Y / n]", "/", Nothing),
        one("scheme after hyphen", "build-https://kaku.fun/docs", "kaku", Url("https://kaku.fun/docs")),
        one("scheme after dot", "1.https://kaku.fun/docs", "kaku", Url("https://kaku.fun/docs")),
        one("prefix of a missing rule link", "文件在 ~/Desktop里面有个report.pdf", "Desktop", File("home/Desktop", None, None)),
        one("git remote after cjk", "仓库git@github.com:tw93/Kaku.git", "github", Url("https://github.com/tw93/Kaku")),
        // ---- review round 2 ----
        Case {
            name: "spaced path at the start of a long soft-wrapped line",
            rows: vec![
                soft("see {T}/proj/docs/a b.md w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4 w5 w6 w7"),
                soft("w8 w9 w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3"),
                soft("w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4 w5 w6 w7 w8 w9"),
                row("w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w0 w1 w2 w3 w4"),
            ],
            cols: None,
            alt_screen: false,
            hover: (0, "docs"),
            expect: File("proj/docs/a b.md", None, None),
        },
        one("git remote before cjk", "仓库git@github.com:tw93/Kaku.git即可", "github", Url("https://github.com/tw93/Kaku")),
        one("email before cjk", "联系tw93@example.com即可", "example", Url("mailto:tw93@example.com")),
        one("spaced folder with trailing slash", "open ~/Downloads/My Folder/ now", "Folder", File("home/Downloads/My Folder", None, None)),
        // ---- review round 3 ----
        Case {
            name: "bare file late in a long compile line",
            rows: vec![
                soft("/usr/bin/clang -c main.c -W0 -W1 -W2 -W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2"),
                soft("-W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2 -W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2"),
                soft("-W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2 -W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2"),
                row("-W3 -W4 -W5 -W6 -W7 -W8 -W9 -W0 -W1 -W2 -W3 -W4 -W5 -W6 -W7 -W8 -W9 -o main.o"),
            ],
            cols: None,
            alt_screen: false,
            hover: (0, "main.c"),
            expect: File("proj/main.c", None, None),
        },
        case(
            "unrelated rows stay apart",
            vec![row("  build /tmp/kaku-none-a"), row("  done in 3s")],
            (1, "done"),
            Nothing,
        ),
    ]
}

#[test]
fn every_link_shape_opens_what_it_shows() {
    let fx = Fixture::new();
    let failures: Vec<String> = corpus()
        .iter()
        .filter_map(|case| {
            check(&fx, case)
                .err()
                .map(|e| format!("{}: {e}", case.name))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} link cases fail:\n{}",
        failures.len(),
        corpus().len(),
        failures.join("\n")
    );
}
