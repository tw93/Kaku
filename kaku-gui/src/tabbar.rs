use crate::termwindow::{PaneInformation, TabInformation, UIItem, UIItemType};
use config::{ConfigHandle, TabBarColors};
use finl_unicode::grapheme_clusters::Graphemes;
use mlua::FromLua;
use mux::pane::CachePolicy;
use mux::Mux;
use std::path::Path;
use termwiz::cell::{unicode_column_width, Cell, CellAttributes};
use termwiz::color::{AnsiColor, ColorSpec};
use termwiz::escape::csi::Sgr;
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, CSI};
use termwiz::surface::SEQ_ZERO;
use termwiz_funcs::{format_as_escapes, FormatColor, FormatItem};
use wezterm_term::{Line, Progress};
use window::{IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct TabBarState {
    line: Line,
    items: Vec<TabEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarItem {
    None,
    LeftStatus,
    RightStatus,
    Tab { tab_idx: usize, active: bool },
    NewTabButton,
    WindowButton(IntegratedTitleButton),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabEntry {
    pub item: TabBarItem,
    pub title: Line,
    pub progress: Progress,
    x: usize,
    width: usize,
}

#[derive(Clone, Debug)]
struct TitleText {
    items: Vec<FormatItem>,
}

#[derive(Clone, Debug)]
struct BatchTabTitles {
    callback_present: bool,
    titles: Vec<Option<TitleText>>,
}

impl BatchTabTitles {
    fn without_callback(count: usize) -> Self {
        Self {
            callback_present: false,
            titles: vec![None; count],
        }
    }
}

fn parse_format_tab_title_result<'lua>(
    v: mlua::Value<'lua>,
    lua: &'lua mlua::Lua,
) -> mlua::Result<Option<TitleText>> {
    match &v {
        mlua::Value::Nil => Ok(None),
        mlua::Value::Table(_) => {
            let items = <Vec<FormatItem>>::from_lua(v, lua)?;
            // Validate table payload from Lua early so downstream
            // format_as_escapes(...).expect() stays infallible.
            let _ = format_as_escapes(items.clone()).map_err(mlua::Error::external)?;
            Ok(Some(TitleText { items }))
        }
        _ => {
            let s = String::from_lua(v, lua)?;
            Ok(Some(TitleText {
                items: vec![FormatItem::Text(s)],
            }))
        }
    }
}

fn has_format_tab_title_callback(lua: &mlua::Lua) -> mlua::Result<bool> {
    let tbl: mlua::Value = lua.named_registry_value("wezterm-event-format-tab-title")?;
    Ok(matches!(tbl, mlua::Value::Table(_)))
}

fn call_format_tab_titles_batch_with_lua(
    lua: &mlua::Lua,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> mlua::Result<BatchTabTitles> {
    let n = tab_info.len();
    if !has_format_tab_title_callback(lua)? {
        return Ok(BatchTabTitles::without_callback(n));
    }

    // Serialize shared data once for all tabs.
    let tabs = lua.create_sequence_from(tab_info.iter().cloned())?;
    let panes = lua.create_sequence_from(pane_info.iter().cloned())?;
    let lua_config = luahelper::to_lua(lua, (**config).clone())?;

    let mut results = Vec::with_capacity(n);
    for tab in tab_info {
        // SSH tabs skip Lua; caller will use build_default_title fallback.
        if let Some(pane) = &tab.active_pane {
            if tab.tab_title.is_empty() && ssh_destination_for_pane(pane).is_some() {
                results.push(None);
                continue;
            }
        }

        let result = config::lua::emit_sync_callback(
            lua,
            (
                "format-tab-title".to_string(),
                (
                    tab.clone(),
                    tabs.clone(),
                    panes.clone(),
                    lua_config.clone(),
                    false,
                    tab_max_width,
                ),
            ),
        )
        .and_then(|v| parse_format_tab_title_result(v, lua));
        match result {
            Ok(title) => results.push(title),
            Err(err) => {
                log::warn!("format-tab-title: {}", err);
                results.push(None);
            }
        }
    }

    Ok(BatchTabTitles {
        callback_present: true,
        titles: results,
    })
}

/// Calls format-tab-title for all tabs in a single Lua scope, serializing
/// Config, tabs, and panes sequences only once instead of once per tab.
/// Returns None for SSH tabs (which skip Lua) or when no callback is registered.
fn call_format_tab_titles_batch(
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> BatchTabTitles {
    let n = tab_info.len();
    match config::run_immediate_with_lua_config(|lua| {
        let Some(lua) = lua else {
            return Ok(BatchTabTitles::without_callback(n));
        };
        Ok(call_format_tab_titles_batch_with_lua(
            &lua,
            tab_info,
            pane_info,
            config,
            tab_max_width,
        )?)
    }) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("format-tab-title (batch): {}", err);
            BatchTabTitles::without_callback(n)
        }
    }
}

/// Calls format-tab-title for a single tab with hover=true.
/// Only invoked when the mouse is actually over a non-active tab and a
/// format-tab-title callback is registered.
fn call_format_tab_title_hover_with_lua(
    lua: &mlua::Lua,
    tab: &TabInformation,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> mlua::Result<Option<TitleText>> {
    let tabs = lua.create_sequence_from(tab_info.iter().cloned())?;
    let panes = lua.create_sequence_from(pane_info.iter().cloned())?;
    let v = config::lua::emit_sync_callback(
        lua,
        (
            "format-tab-title".to_string(),
            (
                tab.clone(),
                tabs,
                panes,
                (**config).clone(),
                true,
                tab_max_width,
            ),
        ),
    )?;
    parse_format_tab_title_result(v, lua)
}

fn call_format_tab_title_hover(
    tab: &TabInformation,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> Option<TitleText> {
    match config::run_immediate_with_lua_config(|lua| {
        let Some(lua) = lua else {
            return Ok(None);
        };
        Ok(call_format_tab_title_hover_with_lua(
            &lua,
            tab,
            tab_info,
            pane_info,
            config,
            tab_max_width,
        )?)
    }) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("format-tab-title (hover): {}", err);
            None
        }
    }
}

/// pct is a percentage in the range 0-100.
/// We want to map it to one of the nerdfonts:
///
/// * `md-checkbox_blank_circle_outline` (0xf0130) for an empty circle
/// * `md_circle_slice_1..=7` (0xf0a9e ..= 0xf0aa4) for a partly filled
///   circle
/// * `md_circle_slice_8` (0xf0aa5) for a filled circle
///
/// We use an empty circle for values close to 0%, a filled circle for values
/// close to 100%, and a partly filled circle for the rest (roughly evenly
/// distributed).
fn pct_to_glyph(pct: u8) -> char {
    match pct {
        0..=5 => '\u{f0130}',    // empty circle
        6..=18 => '\u{f0a9e}',   // centered at 12 (slightly smaller than 12.5)
        19..=31 => '\u{f0a9f}',  // centered at 25
        32..=43 => '\u{f0aa0}',  // centered at 37.5
        44..=56 => '\u{f0aa1}',  // half-filled circle, centered at 50
        57..=68 => '\u{f0aa2}',  // centered at 62.5
        69..=81 => '\u{f0aa3}',  // centered at 75
        82..=94 => '\u{f0aa4}',  // centered at 88 (slightly larger than 87.5)
        95..=100 => '\u{f0aa5}', // filled circle
        // Any other value is mapped to a filled circle.
        _ => '\u{f0aa5}',
    }
}

fn compute_tab_title_from_precomputed(
    tab: &TabInformation,
    config: &ConfigHandle,
    precomputed: Option<TitleText>,
) -> TitleText {
    if let Some(pane) = &tab.active_pane {
        if tab.tab_title.is_empty() {
            if let Some(ssh_host) = ssh_destination_for_pane(pane) {
                return build_default_title(tab, config, &ssh_host, false, true);
            }
        }
    }
    match precomputed {
        Some(title) => title,
        None => {
            if let Some(pane) = &tab.active_pane {
                let title = if !tab.tab_title.is_empty() {
                    tab.tab_title.clone()
                } else if let Some(path_title) = pane_cwd_title(pane) {
                    path_title
                } else if let Some(ssh_host) = ssh_destination_for_pane(pane) {
                    ssh_host
                } else {
                    pane.title.clone()
                };
                build_default_title(tab, config, &title, true, false)
            } else {
                TitleText {
                    items: vec![FormatItem::Text(" no pane ".to_string())],
                }
            }
        }
    }
}

fn pane_cwd_title(pane: &PaneInformation) -> Option<String> {
    let mux = Mux::try_get()?;
    let real_pane = mux.get_pane(pane.pane_id)?;
    let cwd = real_pane.get_current_working_dir(CachePolicy::AllowStale)?;
    let path = cwd.path().trim_end_matches('/');
    if path.is_empty() {
        return None;
    }

    let path = Path::new(path);
    let current = path
        .file_name()
        .and_then(|name| name.to_str())
        .or_else(|| path.to_str())?;

    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if parent.is_empty() {
        Some(current.to_string())
    } else {
        Some(format!("{parent}/{current}"))
    }
}

pub fn compute_tab_plain_title(tab: &TabInformation) -> String {
    if !tab.tab_title.is_empty() {
        return tab.tab_title.clone();
    }

    if let Some(pane) = &tab.active_pane {
        if let Some(title) = pane_cwd_title(pane) {
            return title;
        }
        if let Some(ssh_host) = ssh_destination_for_pane(pane) {
            return ssh_host;
        }
        return pane.title.clone();
    }

    "no pane".to_string()
}

fn build_default_title(
    tab: &TabInformation,
    config: &ConfigHandle,
    title: &str,
    with_tab_index: bool,
    with_edge_padding: bool,
) -> TitleText {
    let mut items = vec![];
    let mut len = 0;
    let mut title = title.to_string();

    let classic_spacing = if config.use_fancy_tab_bar { "" } else { " " };
    if with_tab_index && config.show_tab_index_in_tab_bar {
        let index = format!(
            "{classic_spacing}{}: ",
            tab.tab_index
                + if config.tab_and_split_indices_are_zero_based {
                    0
                } else {
                    1
                }
        );
        len += unicode_column_width(&index, None);
        items.push(FormatItem::Text(index));
        title = format!("{}{classic_spacing}", title);
    }

    if let Some(pane) = &tab.active_pane {
        match pane.progress {
            Progress::None => {}
            Progress::Percentage(pct) | Progress::Error(pct) => {
                if !config.use_fancy_tab_bar {
                    let graphic = format!("{}", pct_to_glyph(pct));
                    len += unicode_column_width(&graphic, None);
                    let color = if matches!(pane.progress, Progress::Percentage(_)) {
                        FormatItem::Foreground(FormatColor::AnsiColor(AnsiColor::Green))
                    } else {
                        FormatItem::Foreground(FormatColor::AnsiColor(AnsiColor::Red))
                    };
                    items.push(color);
                    items.push(FormatItem::Text(graphic));
                    items.push(FormatItem::Foreground(FormatColor::Default));
                }
            }
            Progress::Indeterminate => {
                // TODO: Decide what to do here to indicate this
            }
        }
    }

    if with_edge_padding {
        title = format!(" {} ", title);
    } else if !config.use_fancy_tab_bar {
        while len + unicode_column_width(&title, None) < 5 {
            title.push(' ');
        }
    }

    items.push(FormatItem::Text(title));

    TitleText { items }
}

/// Detect the SSH destination for a pane, used to show the remote host in tab titles.
///
/// Fallback chain (first match wins):
///   1. `WEZTERM_PROG` user var → parse SSH command
///   2. Domain name prefix (`SSH:` / `SSHMUX:`)
///   3. Foreground process named `ssh` → parse its argv
///   4. CWD host component (e.g. from `file://host/…`)
fn ssh_destination_for_pane(pane: &PaneInformation) -> Option<String> {
    if let Some(command) = pane.user_vars.get("WEZTERM_PROG") {
        if let Some(host) = ssh_target_from_command(command) {
            return Some(host);
        }
    }

    let mux = Mux::try_get()?;
    let real_pane = mux.get_pane(pane.pane_id)?;

    if let Some(domain) = mux.get_domain(real_pane.domain_id()) {
        let name = domain.domain_name();
        if let Some(host) = name
            .strip_prefix("SSH:")
            .or_else(|| name.strip_prefix("SSHMUX:"))
        {
            return Some(host.to_string());
        }
    }

    let fg = real_pane.get_foreground_process_name(CachePolicy::AllowStale)?;
    if command_basename(&fg) != "ssh" {
        return None;
    }

    if let Some(info) = real_pane.get_foreground_process_info(CachePolicy::AllowStale) {
        if let Some(host) = ssh_target_from_tokens(&info.argv) {
            return Some(host);
        }
    }

    real_pane
        .get_current_working_dir(CachePolicy::AllowStale)
        .and_then(|cwd| cwd.host_str().map(ToString::to_string))
}

fn ssh_target_from_command(command: &str) -> Option<String> {
    let tokens = shlex::split(command).unwrap_or_else(|| {
        command
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    });

    ssh_target_from_tokens(&tokens)
}

fn ssh_target_from_tokens(tokens: &[String]) -> Option<String> {
    if tokens.is_empty() || command_basename(&tokens[0]) != "ssh" {
        return None;
    }

    let mut expect_value = false;
    for token in tokens.iter().skip(1) {
        if expect_value {
            expect_value = false;
            continue;
        }
        if token == "--" {
            return None;
        }
        if token.starts_with('-') {
            expect_value = ssh_option_needs_value(token);
            continue;
        }
        return normalize_ssh_target(token);
    }
    None
}

fn ssh_option_needs_value(token: &str) -> bool {
    if token.len() != 2 || !token.starts_with('-') {
        return false;
    }
    matches!(
        token.chars().nth(1),
        Some(
            'B' | 'b'
                | 'c'
                | 'D'
                | 'E'
                | 'e'
                | 'F'
                | 'I'
                | 'i'
                | 'J'
                | 'L'
                | 'l'
                | 'm'
                | 'O'
                | 'o'
                | 'p'
                | 'Q'
                | 'R'
                | 'S'
                | 'W'
                | 'w'
        )
    )
}

fn command_basename(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
}

fn normalize_ssh_target(target: &str) -> Option<String> {
    let mut host = target.trim();
    if host.is_empty() {
        return None;
    }

    if let Some(rest) = host.rsplit_once('@').map(|(_, rhs)| rhs) {
        host = rest;
    }

    if let Some(without_open) = host.strip_prefix('[') {
        if let Some(end) = without_open.find(']') {
            return Some(without_open[..end].to_string());
        }
    }

    if host.matches(':').count() == 1 {
        if let Some((h, port)) = host.rsplit_once(':') {
            if !h.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                host = h;
            }
        }
    }

    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn is_tab_hover(mouse_x: Option<usize>, x: usize, tab_title_len: usize) -> bool {
    mouse_x
        .map(|mouse_x| mouse_x >= x && mouse_x < x + tab_title_len)
        .unwrap_or(false)
}

impl TabBarState {
    pub fn default() -> Self {
        Self {
            line: Line::with_width(1, SEQ_ZERO),
            items: vec![TabEntry {
                item: TabBarItem::None,
                title: Line::from_text(" ", &CellAttributes::blank(), 1, None),
                progress: Progress::None,
                x: 1,
                width: 1,
            }],
        }
    }

    pub fn line(&self) -> &Line {
        &self.line
    }

    pub fn items(&self) -> &[TabEntry] {
        &self.items
    }

    fn integrated_title_buttons(
        mouse_x: Option<usize>,
        x: &mut usize,
        config: &ConfigHandle,
        items: &mut Vec<TabEntry>,
        line: &mut Line,
        colors: &TabBarColors,
    ) {
        let default_cell = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab().as_cell_attributes()
        };

        let default_cell_hover = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab_hover().as_cell_attributes()
        };

        let window_hide =
            parse_status_text(&config.tab_bar_style.window_hide, default_cell.clone());
        let window_hide_hover = parse_status_text(
            &config.tab_bar_style.window_hide_hover,
            default_cell_hover.clone(),
        );

        let window_maximize =
            parse_status_text(&config.tab_bar_style.window_maximize, default_cell.clone());
        let window_maximize_hover = parse_status_text(
            &config.tab_bar_style.window_maximize_hover,
            default_cell_hover.clone(),
        );

        let window_close =
            parse_status_text(&config.tab_bar_style.window_close, default_cell.clone());
        let window_close_hover = parse_status_text(
            &config.tab_bar_style.window_close_hover,
            default_cell_hover.clone(),
        );

        for button in &config.integrated_title_buttons {
            use IntegratedTitleButton as Button;
            let title = match button {
                Button::Hide => {
                    let hover = is_tab_hover(mouse_x, *x, window_hide_hover.len());

                    if hover {
                        &window_hide_hover
                    } else {
                        &window_hide
                    }
                }
                Button::Maximize => {
                    let hover = is_tab_hover(mouse_x, *x, window_maximize_hover.len());

                    if hover {
                        &window_maximize_hover
                    } else {
                        &window_maximize
                    }
                }
                Button::Close => {
                    let hover = is_tab_hover(mouse_x, *x, window_close_hover.len());

                    if hover {
                        &window_close_hover
                    } else {
                        &window_close
                    }
                }
            };

            line.append_line(title.to_owned(), SEQ_ZERO);

            let width = title.len();
            items.push(TabEntry {
                item: TabBarItem::WindowButton(*button),
                title: title.to_owned(),
                progress: Progress::None,
                x: *x,
                width,
            });

            *x += width;
        }
    }

    /// Build a new tab bar from the current state
    /// mouse_x is some if the mouse is on the same row as the tab bar.
    /// title_width is the total number of cell columns in the window.
    /// window allows access to the tabs associated with the window.
    pub fn new(
        title_width: usize,
        mouse_x: Option<usize>,
        tab_info: &[TabInformation],
        pane_info: &[PaneInformation],
        is_fullscreen: bool,
        colors: Option<&TabBarColors>,
        config: &ConfigHandle,
        left_status: &str,
        right_status: &str,
    ) -> Self {
        let colors = colors.cloned().unwrap_or_else(TabBarColors::default);

        let active_cell_attrs = colors.active_tab().as_cell_attributes();
        let inactive_hover_attrs = colors.inactive_tab_hover().as_cell_attributes();
        let inactive_cell_attrs = colors.inactive_tab().as_cell_attributes();
        let new_tab_hover_attrs = colors.new_tab_hover().as_cell_attributes();
        let new_tab_attrs = colors.new_tab().as_cell_attributes();

        let new_tab = parse_status_text(
            &config.tab_bar_style.new_tab,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_attrs.clone()
            },
        );
        let new_tab_hover = parse_status_text(
            &config.tab_bar_style.new_tab_hover,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_hover_attrs.clone()
            },
        );

        let use_integrated_title_buttons = config
            .window_decorations
            .contains(window::WindowDecorations::INTEGRATED_BUTTONS);

        // We ultimately want to produce a line looking like this:
        // ` | tab1-title x | tab2-title x |  +      . - X `
        // Where the `+` sign will spawn a new tab (or show a context
        // menu with tab creation options) and the other three chars
        // are symbols representing minimize, maximize and close.

        let mut active_tab_no = 0;
        if config.show_tabs_in_tab_bar {
            for tab in tab_info {
                if tab.is_active {
                    active_tab_no = tab.tab_index;
                }
            }
        }
        let number_of_tabs = if config.show_tabs_in_tab_bar {
            tab_info.len()
        } else {
            0
        };

        // Tab titles are rendered contiguously; only reserve width for controls
        // that are actually shown.
        let controls_width = if config.show_new_tab_button_in_tab_bar {
            new_tab.len()
        } else {
            0
        };
        let available_cells = title_width.saturating_sub(controls_width);
        let tab_width_max = if number_of_tabs == 0 {
            config.tab_max_width.max(1)
        } else if config.use_fancy_tab_bar {
            usize::MAX
        } else {
            let per_tab = (available_cells / number_of_tabs).max(1);
            per_tab.min(config.tab_max_width.max(1))
        };
        let tab_title_max_width_for_callback = if tab_width_max == usize::MAX {
            config.tab_max_width.max(1)
        } else {
            tab_width_max
        };

        let mut line = Line::with_width(0, SEQ_ZERO);

        let mut x = 0;
        let mut items = vec![];

        let black_cell = Cell::blank_with_attrs(
            CellAttributes::default()
                .set_background(ColorSpec::TrueColor(*colors.background()))
                .clone(),
        );

        if use_integrated_title_buttons
            && config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            && !config.use_fancy_tab_bar
            && !config.tab_bar_at_bottom
            && !is_fullscreen
        {
            for _ in 0..10_usize {
                line.insert_cell(0, black_cell.clone(), title_width, SEQ_ZERO);
                x += 1;
            }
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Left
        {
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        let left_status_line = parse_status_text(left_status, black_cell.attrs().clone());
        if left_status_line.len() > 0 {
            items.push(TabEntry {
                item: TabBarItem::LeftStatus,
                title: left_status_line.clone(),
                progress: Progress::None,
                x,
                width: left_status_line.len(),
            });
            x += left_status_line.len();
            line.append_line(left_status_line, SEQ_ZERO);
        }

        // Pre-compute all tab titles in a single Lua scope to avoid serializing
        // Config, tabs, and panes sequences once per tab.
        let precomputed_titles = if number_of_tabs > 0 {
            call_format_tab_titles_batch(
                tab_info,
                pane_info,
                config,
                tab_title_max_width_for_callback,
            )
        } else {
            BatchTabTitles::without_callback(0)
        };

        for tab_idx in 0..number_of_tabs {
            let active = tab_idx == active_tab_no;
            let mut hover = false;

            let precomputed = precomputed_titles
                .titles
                .get(tab_idx)
                .and_then(|t| t.clone());

            let mut tab_title =
                compute_tab_title_from_precomputed(&tab_info[tab_idx], config, precomputed.clone());
            let mut cell_attrs = if active {
                &active_cell_attrs
            } else {
                &inactive_cell_attrs
            };

            let tab_start_idx = x;

            let mut esc =
                format_as_escapes(tab_title.items.clone()).expect("already parsed ok above");
            let mut tab_line = parse_status_text(
                &esc,
                if config.use_fancy_tab_bar {
                    CellAttributes::default()
                } else {
                    cell_attrs.clone()
                },
            );
            if tab_line.len() > tab_width_max {
                tab_line.resize(tab_width_max, SEQ_ZERO);
            }
            let mut width = tab_line.len();
            if !active {
                hover = is_tab_hover(mouse_x, x, width);
            }
            if hover {
                // The normal callback may return nil to opt into the default
                // title while still customizing the hover state.
                // SSH tabs skip Lua entirely: compute_tab_title_from_precomputed
                // returns the SSH default title regardless of hover_precomputed.
                let is_ssh_tab = tab_info[tab_idx]
                    .active_pane
                    .as_ref()
                    .map(|p| {
                        tab_info[tab_idx].tab_title.is_empty()
                            && ssh_destination_for_pane(p).is_some()
                    })
                    .unwrap_or(false);
                let hover_precomputed = if precomputed_titles.callback_present && !is_ssh_tab {
                    call_format_tab_title_hover(
                        &tab_info[tab_idx],
                        tab_info,
                        pane_info,
                        config,
                        tab_title_max_width_for_callback,
                    )
                } else {
                    None
                };
                tab_title = compute_tab_title_from_precomputed(
                    &tab_info[tab_idx],
                    config,
                    hover_precomputed,
                );
                cell_attrs = &inactive_hover_attrs;
                esc = format_as_escapes(tab_title.items.clone()).expect("already parsed ok above");
                tab_line = parse_status_text(
                    &esc,
                    if config.use_fancy_tab_bar {
                        CellAttributes::default()
                    } else {
                        cell_attrs.clone()
                    },
                );
                if tab_line.len() > tab_width_max {
                    tab_line.resize(tab_width_max, SEQ_ZERO);
                }
                width = tab_line.len();
            }
            let title = tab_line.clone();

            items.push(TabEntry {
                item: TabBarItem::Tab { tab_idx, active },
                title,
                progress: tab_info[tab_idx]
                    .active_pane
                    .as_ref()
                    .map_or(Progress::None, |p| p.progress.clone()),
                x: tab_start_idx,
                width,
            });

            line.append_line(tab_line, SEQ_ZERO);
            x += width;
        }

        // New tab button
        if config.show_new_tab_button_in_tab_bar {
            let hover = is_tab_hover(mouse_x, x, new_tab_hover.len());

            let new_tab_button = if hover { &new_tab_hover } else { &new_tab };

            let button_start = x;
            let width = new_tab_button.len();

            line.append_line(new_tab_button.clone(), SEQ_ZERO);

            items.push(TabEntry {
                item: TabBarItem::NewTabButton,
                title: new_tab_button.clone(),
                progress: Progress::None,
                x: button_start,
                width,
            });

            x += width;
        }

        // Reserve place for integrated title buttons
        let title_width = if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            let window_hide =
                parse_status_text(&config.tab_bar_style.window_hide, CellAttributes::default());
            let window_hide_hover = parse_status_text(
                &config.tab_bar_style.window_hide_hover,
                CellAttributes::default(),
            );

            let window_maximize = parse_status_text(
                &config.tab_bar_style.window_maximize,
                CellAttributes::default(),
            );
            let window_maximize_hover = parse_status_text(
                &config.tab_bar_style.window_maximize_hover,
                CellAttributes::default(),
            );
            let window_close = parse_status_text(
                &config.tab_bar_style.window_close,
                CellAttributes::default(),
            );
            let window_close_hover = parse_status_text(
                &config.tab_bar_style.window_close_hover,
                CellAttributes::default(),
            );

            let hide_len = window_hide.len().max(window_hide_hover.len());
            let maximize_len = window_maximize.len().max(window_maximize_hover.len());
            let close_len = window_close.len().max(window_close_hover.len());

            let mut width_to_reserve = 0;
            for button in &config.integrated_title_buttons {
                use IntegratedTitleButton as Button;
                let button_len = match button {
                    Button::Hide => hide_len,
                    Button::Maximize => maximize_len,
                    Button::Close => close_len,
                };
                width_to_reserve += button_len;
            }

            title_width.saturating_sub(width_to_reserve)
        } else {
            title_width
        };

        let status_space_available = title_width.saturating_sub(x);

        let mut right_status_line = parse_status_text(right_status, black_cell.attrs().clone());
        items.push(TabEntry {
            item: TabBarItem::RightStatus,
            title: right_status_line.clone(),
            progress: Progress::None,
            x,
            width: status_space_available,
        });

        if right_status_line.len() > status_space_available {
            let excess = right_status_line.len() - status_space_available;
            right_status_line = right_status_line.split_off(excess, SEQ_ZERO);
        }

        line.append_line(right_status_line, SEQ_ZERO);
        while line.len() < title_width {
            line.insert_cell(x, black_cell.clone(), title_width, SEQ_ZERO);
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            x = title_width;
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        Self { line, items }
    }

    pub fn compute_ui_items(&self, y: usize, cell_height: usize, cell_width: usize) -> Vec<UIItem> {
        let mut items = vec![];

        for entry in self.items.iter() {
            items.push(UIItem {
                x: entry.x * cell_width,
                width: entry.width * cell_width,
                y,
                height: cell_height,
                item_type: UIItemType::TabBar(entry.item),
            });
        }

        items
    }
}

pub fn parse_status_text(text: &str, default_cell: CellAttributes) -> Line {
    let mut pen = default_cell.clone();
    let mut cells = vec![];
    let mut ignoring = false;
    let mut print_buffer = String::new();

    fn flush_print(buf: &mut String, cells: &mut Vec<Cell>, pen: &CellAttributes) {
        for g in Graphemes::new(buf.as_str()) {
            let cell = Cell::new_grapheme(g, pen.clone(), None);
            let width = cell.width();
            cells.push(cell);
            for _ in 1..width {
                // Line/Screen expect double wide graphemes to be followed by a blank in
                // the next column position, otherwise we'll render incorrectly
                cells.push(Cell::blank_with_attrs(pen.clone()));
            }
        }
        buf.clear();
    }

    let mut parser = Parser::new();
    parser.parse(text.as_bytes(), |action| {
        if ignoring {
            return;
        }
        match action {
            Action::Print(c) => print_buffer.push(c),
            Action::PrintString(s) => print_buffer.push_str(&s),
            Action::Control(c) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                match c {
                    ControlCode::CarriageReturn | ControlCode::LineFeed => {
                        ignoring = true;
                    }
                    _ => {}
                }
            }
            Action::CSI(csi) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                match csi {
                    CSI::Sgr(sgr) => match sgr {
                        Sgr::Reset => pen = default_cell.clone(),
                        Sgr::Intensity(i) => {
                            pen.set_intensity(i);
                        }
                        Sgr::Underline(u) => {
                            pen.set_underline(u);
                        }
                        Sgr::Overline(o) => {
                            pen.set_overline(o);
                        }
                        Sgr::VerticalAlign(o) => {
                            pen.set_vertical_align(o);
                        }
                        Sgr::Blink(b) => {
                            pen.set_blink(b);
                        }
                        Sgr::Italic(i) => {
                            pen.set_italic(i);
                        }
                        Sgr::Inverse(inverse) => {
                            pen.set_reverse(inverse);
                        }
                        Sgr::Invisible(invis) => {
                            pen.set_invisible(invis);
                        }
                        Sgr::StrikeThrough(strike) => {
                            pen.set_strikethrough(strike);
                        }
                        Sgr::Foreground(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_foreground(default_cell.foreground());
                            } else {
                                pen.set_foreground(col);
                            }
                        }
                        Sgr::Background(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_background(default_cell.background());
                            } else {
                                pen.set_background(col);
                            }
                        }
                        Sgr::UnderlineColor(col) => {
                            pen.set_underline_color(col);
                        }
                        Sgr::Font(_) => {}
                    },
                    _ => {}
                }
            }
            Action::OperatingSystemCommand(_)
            | Action::DeviceControl(_)
            | Action::Esc(_)
            | Action::KittyImage(_)
            | Action::XtGetTcap(_)
            | Action::Sixel(_) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
            }
        }
    });
    flush_print(&mut print_buffer, &mut cells, &pen);
    Line::from_cells(cells, SEQ_ZERO)
}

#[cfg(test)]
mod test {
    use super::*;

    fn plain_text(title: &TitleText) -> String {
        title
            .items
            .iter()
            .filter_map(|item| match item {
                FormatItem::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn make_tab(tab_id: usize, tab_index: usize, is_active: bool, title: &str) -> TabInformation {
        TabInformation {
            tab_id: tab_id.into(),
            tab_index,
            is_active,
            is_last_active: false,
            active_pane: None,
            window_id: 0,
            tab_title: title.to_string(),
        }
    }

    #[test]
    fn parse_plain_ssh_target() {
        assert_eq!(
            ssh_target_from_command("ssh root@10.0.0.8").as_deref(),
            Some("10.0.0.8")
        );
    }

    #[test]
    fn parse_ssh_target_with_options() {
        assert_eq!(
            ssh_target_from_command("ssh -p 2222 -i ~/.ssh/id user@build-host").as_deref(),
            Some("build-host")
        );
    }

    #[test]
    fn ignore_non_ssh_command() {
        assert!(ssh_target_from_command("ls -la").is_none());
    }

    #[test]
    fn hover_title_callback_runs_even_when_normal_returns_nil() -> anyhow::Result<()> {
        let lua = mlua::Lua::new();
        let callback = lua.create_function(
            |lua,
             (_tab, _tabs, _panes, _config, hover, _max_width): (
                mlua::Value,
                mlua::Value,
                mlua::Value,
                mlua::Value,
                bool,
                usize,
            )| {
                if hover {
                    Ok(mlua::Value::String(lua.create_string("HOVER")?))
                } else {
                    Ok(mlua::Value::Nil)
                }
            },
        )?;
        config::lua::register_event(&lua, ("format-tab-title".to_string(), callback))?;

        let config = ConfigHandle::default_config();
        let tab_info = vec![make_tab(0, 0, true, "tab-0")];
        let pane_info = vec![];

        let batch =
            call_format_tab_titles_batch_with_lua(&lua, &tab_info, &pane_info, &config, 32)?;
        assert!(batch.callback_present);
        assert_eq!(batch.titles.len(), 1);
        assert!(batch.titles[0].is_none());

        let hover = call_format_tab_title_hover_with_lua(
            &lua,
            &tab_info[0],
            &tab_info,
            &pane_info,
            &config,
            32,
        )?
        .expect("hover callback should produce a title");

        assert_eq!(plain_text(&hover), "HOVER");
        Ok(())
    }
}
