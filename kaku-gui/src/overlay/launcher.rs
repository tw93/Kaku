//! The launcher is a menu that presents a list of activities that can
//! be launched, such as spawning a new tab in various domains or attaching
//! ssh/tls domains.
//! The launcher is implemented here as an overlay, but could potentially
//! be rendered as a popup/context menu if the system supports it; at the
//! time of writing our window layer doesn't provide an API for context
//! menus.
use crate::commands::{derive_command_from_key_assignment, is_internal_emit_event_name};
use crate::inputmap::InputMap;
use crate::overlay::quickselect;
use crate::overlay::selector::{matcher_pattern, matcher_score};
use config::configuration;
use config::keyassignment::KeyAssignment::SetPaneEncoding;
use config::keyassignment::{
    KeyAssignment, LauncherActionArgs, PaneEncoding, SpawnCommand, SpawnTabDomain,
};
use mux::domain::{DomainId, DomainState};
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use rayon::prelude::*;
use std::collections::BTreeMap;
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;

pub use config::keyassignment::LauncherFlags;

#[derive(Clone)]
struct Entry {
    pub label: String,
    pub action: LauncherAction,
}

pub struct LauncherTabEntry {
    pub title: String,
    pub pane_id: PaneId,
    pub tab_id: TabId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LauncherAction {
    Assignment(KeyAssignment),
    ActivatePane { pane_id: PaneId, tab_id: TabId },
    CloseNavigatorTab(TabId),
}

impl From<KeyAssignment> for LauncherAction {
    fn from(action: KeyAssignment) -> Self {
        Self::Assignment(action)
    }
}

#[derive(Debug)]
pub struct LauncherDomainEntry {
    pub domain_id: DomainId,
    pub name: String,
    pub state: DomainState,
    pub label: String,
}

pub struct LauncherArgs {
    flags: LauncherFlags,
    domains: Vec<LauncherDomainEntry>,
    tabs: Vec<LauncherTabEntry>,
    domain_id_of_current_tab: DomainId,
    title: String,
    active_workspace: String,
    workspaces: Vec<String>,
    help_text: String,
    fuzzy_help_text: String,
    alphabet: String,
}

impl LauncherArgs {
    /// Must be called on the Mux thread!
    pub async fn new(
        title: &str,
        flags: LauncherFlags,
        domain_id_of_current_tab: DomainId,
        help_text: &str,
        fuzzy_help_text: &str,
        alphabet: &str,
        tabs: Vec<LauncherTabEntry>,
    ) -> Self {
        let mux = Mux::get();

        let active_workspace = mux.active_workspace();

        let workspaces = if flags.contains(LauncherFlags::WORKSPACES) {
            mux.iter_workspaces()
        } else {
            vec![]
        };

        let domains = if flags.contains(LauncherFlags::DOMAINS) {
            let mut domains = mux.iter_domains();
            domains.sort_by(|a, b| {
                let a_state = a.state();
                let b_state = b.state();
                if a_state != b_state {
                    use std::cmp::Ordering;
                    return if a_state == DomainState::Attached {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                a.domain_id().cmp(&b.domain_id())
            });
            domains.retain(|dom| dom.spawnable());
            let mut d = vec![];
            for dom in domains.into_iter() {
                let name = dom.domain_name();
                let label = dom.domain_label().await;
                let label = if name == label || label.is_empty() {
                    format!("domain `{}`", name)
                } else {
                    format!("domain `{}` - {}", name, label)
                };
                d.push(LauncherDomainEntry {
                    domain_id: dom.domain_id(),
                    name: name.to_string(),
                    state: dom.state(),
                    label,
                });
            }
            d
        } else {
            vec![]
        };

        Self {
            flags,
            domains,
            tabs,
            domain_id_of_current_tab,
            title: title.to_string(),
            workspaces,
            active_workspace,
            help_text: help_text.to_string(),
            fuzzy_help_text: fuzzy_help_text.to_string(),
            alphabet: alphabet.to_string(),
        }
    }
}

const ROW_OVERHEAD: usize = 3;

struct ParentLauncherState {
    entries: Vec<Entry>,
    active_idx: usize,
    top_row: usize,
    help_text: String,
    filter_term: String,
    filtering: bool,
}

struct LauncherState {
    active_idx: usize,
    max_items: usize,
    top_row: usize,
    entries: Vec<Entry>,
    filter_term: String,
    filtered_entries: Vec<Entry>,
    filtering: bool,
    help_text: String,
    fuzzy_help_text: String,
    labels: Vec<String>,
    alphabet: String,
    selection: String,
    always_fuzzy: bool,
    allow_tab_close: bool,
    parent_state: Option<ParentLauncherState>,
}

impl LauncherState {
    fn update_filter(&mut self) {
        if self.filter_term.is_empty() {
            self.filtered_entries = self.entries.clone();
            return;
        }

        self.filtered_entries.clear();

        let pattern = matcher_pattern(&self.filter_term);

        struct MatchResult {
            row_idx: usize,
            score: u32,
        }

        let mut scores: Vec<MatchResult> = self
            .entries
            .par_iter()
            .enumerate()
            .filter_map(|(row_idx, entry)| {
                let score = matcher_score(&pattern, &entry.label)?;
                Some(MatchResult { row_idx, score })
            })
            .collect();

        scores.sort_by(|a, b| a.score.cmp(&b.score).reverse());

        for result in scores {
            self.filtered_entries
                .push(self.entries[result.row_idx].clone());
        }

        self.active_idx = 0;
        self.top_row = 0;
    }

    fn build_entries(&mut self, args: LauncherArgs) {
        let config = configuration();

        // Pull in the user defined entries from the launch_menu
        // section of the configuration.
        if args.flags.contains(LauncherFlags::LAUNCH_MENU_ITEMS) {
            for item in &config.launch_menu {
                self.entries.push(Entry {
                    label: match item.label.as_ref() {
                        Some(label) => label.to_string(),
                        None => match item.args.as_ref() {
                            Some(args) => args.join(" "),
                            None => "(default shell)".to_string(),
                        },
                    },
                    action: KeyAssignment::SpawnCommandInNewTab(item.clone()).into(),
                });
            }
        }

        for domain in &args.domains {
            let entry = if domain.state == DomainState::Attached {
                Entry {
                    label: format!("New Tab ({})", domain.label),
                    action: KeyAssignment::SpawnCommandInNewTab(SpawnCommand {
                        domain: SpawnTabDomain::DomainName(domain.name.to_string()),
                        ..SpawnCommand::default()
                    })
                    .into(),
                }
            } else {
                Entry {
                    label: format!("Attach {}", domain.label),
                    action: KeyAssignment::AttachDomain(domain.name.to_string()).into(),
                }
            };

            // Preselect the entry that corresponds to the active tab
            // at the time that the launcher was set up, so that pressing
            // Enter immediately afterwards spawns a tab in the same domain.
            if domain.domain_id == args.domain_id_of_current_tab {
                self.active_idx = self.entries.len();
            }
            self.entries.push(entry);
        }

        if args.flags.contains(LauncherFlags::WORKSPACES) {
            for ws in &args.workspaces {
                if *ws != args.active_workspace {
                    self.entries.push(Entry {
                        label: format!("Switch to workspace: `{}`", ws),
                        action: KeyAssignment::SwitchToWorkspace {
                            name: Some(ws.clone()),
                            spawn: None,
                        }
                        .into(),
                    });
                }
            }
            self.entries.push(Entry {
                label: format!(
                    "Create new Workspace (current is `{}`)",
                    args.active_workspace
                ),
                action: KeyAssignment::SwitchToWorkspace {
                    name: None,
                    spawn: None,
                }
                .into(),
            });
        }

        for tab in &args.tabs {
            self.entries.push(Entry {
                label: tab.title.clone(),
                action: launcher_tab_action(tab),
            });
        }

        if args.flags.contains(LauncherFlags::PANE_ENCODINGS) {
            for encoding in PaneEncoding::ordered_list() {
                self.entries.push(Entry {
                    label: format!("Set pane encoding to {encoding}"),
                    action: SetPaneEncoding(encoding).into(),
                });
            }
        }

        if args.flags.contains(LauncherFlags::COMMANDS) {
            let commands = crate::commands::CommandDef::expanded_commands(&config);
            for cmd in commands {
                if matches!(
                    &cmd.action,
                    KeyAssignment::ActivateTabRelative(_)
                        | KeyAssignment::ActivateTab(_)
                        | KeyAssignment::SendString(_)
                        | KeyAssignment::SendStringIfNotAltScreen(_)
                        | KeyAssignment::SendKey(_)
                        | KeyAssignment::Nop
                        | KeyAssignment::Multiple(_)
                        | KeyAssignment::SetPaneEncoding(_)
                ) {
                    // Filter out some noisy, repetitive entries
                    continue;
                }
                self.entries.push(Entry {
                    label: cmd.brief.to_string(),
                    action: cmd.action.into(),
                });
            }
        }

        // Grab interesting key assignments and show those as a kind of command palette
        if args.flags.contains(LauncherFlags::KEY_ASSIGNMENTS) {
            let input_map = InputMap::new(&config);
            let mut key_entries: Vec<Entry> = vec![];
            // Give a consistent order to the entries
            let keys: BTreeMap<_, _> = input_map.keys.default.into_iter().collect();
            for ((keycode, mods), entry) in keys {
                if matches!(
                    &entry.action,
                    KeyAssignment::ActivateTabRelative(_)
                        | KeyAssignment::ActivateTab(_)
                        | KeyAssignment::SendString(_)
                        | KeyAssignment::SendStringIfNotAltScreen(_)
                        | KeyAssignment::SendKey(_)
                        | KeyAssignment::Nop
                        | KeyAssignment::Multiple(_)
                ) {
                    // Filter out some noisy, repetitive entries
                    continue;
                }
                if matches!(
                    &entry.action,
                    KeyAssignment::EmitEvent(name) if is_internal_emit_event_name(name)
                ) {
                    // Hide generated callback event IDs from the user-facing palette.
                    continue;
                }
                if key_entries
                    .iter()
                    .find(|ent| {
                        matches!(
                            &ent.action,
                            LauncherAction::Assignment(action) if action == &entry.action
                        )
                    })
                    .is_some()
                {
                    // Avoid duplicate entries
                    continue;
                }

                let label = match derive_command_from_key_assignment(&entry.action) {
                    Some(cmd) => cmd.brief.to_string(),
                    None => format!(
                        "{:?} ({} {})",
                        entry.action,
                        mods.to_string(),
                        keycode.to_string().escape_debug()
                    ),
                };
                key_entries.push(Entry {
                    label,
                    action: entry.action.into(),
                });
            }
            key_entries.sort_by(|a, b| a.label.cmp(&b.label));
            self.entries.append(&mut key_entries);
        }

        // Keep the encoding submenu entry at the very end so it doesn't steal
        // the first numeric shortcut from tabs or primary launcher commands.
        if !args.flags.contains(LauncherFlags::PANE_ENCODINGS) {
            self.entries.push(Entry {
                label: "Pane Encoding".to_string(),
                action: KeyAssignment::ShowLauncherArgs(LauncherActionArgs {
                    flags: LauncherFlags::PANE_ENCODINGS,
                    title: Some("Pane Encoding".to_string()),
                    help_text: None,
                    fuzzy_help_text: None,
                    alphabet: None,
                })
                .into(),
            });
        }
    }

    fn render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(6);
        let max_items = size.rows.saturating_sub(ROW_OVERHEAD);
        let desired_label_count = self.filtered_entries.len().min(max_items + 1);
        if max_items != self.max_items || self.labels.len() != desired_label_count {
            self.labels = quickselect::compute_labels_for_alphabet_with_preserved_case(
                &self.alphabet,
                desired_label_count,
            );
            self.max_items = max_items;
        }

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            Change::Text(format!(
                "{}\r\n",
                truncate_right(&self.help_text, max_width)
            )),
            Change::AllAttributes(CellAttributes::default()),
        ];

        let labels = &self.labels;
        let max_label_len = labels.iter().map(|s| s.len()).max().unwrap_or(0);
        let mut labels_iter = labels.into_iter();

        let config = configuration();
        let colors = &config.resolved_palette;
        let launcher_label_fg = colors.launcher_label_fg;
        let launcher_label_bg = colors.launcher_label_bg;
        let white = config::RgbaColor::from((0xff, 0xff, 0xff));
        let black = config::RgbaColor::from((0x00, 0x00, 0x00));
        let is_light_bg = |bg: config::RgbaColor| 0.299 * bg.0 + 0.587 * bg.1 + 0.114 * bg.2 > 0.7;
        let selected_bg_color = colors
            .selection_bg
            .filter(|bg| !is_light_bg(*bg))
            .or_else(|| colors.ansi.as_ref().map(|ansi| ansi[5]))
            .or_else(|| colors.brights.as_ref().map(|brights| brights[5]))
            .or(colors.selection_bg);
        let selected_bg_attr =
            selected_bg_color.map(|bg| ColorAttribute::from(config::ColorSpec::Color(bg)));
        let selected_fg_attr = selected_bg_color
            .map(|bg| if is_light_bg(bg) { black } else { white })
            .or(colors.foreground)
            .map(|fg| ColorAttribute::from(config::ColorSpec::Color(fg)));

        for (row_num, (entry_idx, entry)) in self
            .filtered_entries
            .iter()
            .enumerate()
            .skip(self.top_row)
            .enumerate()
        {
            if row_num > max_items {
                break;
            }

            let mut attr = CellAttributes::blank();

            let mut used_reverse_for_selection = false;
            if entry_idx == self.active_idx {
                if let Some(selected_bg_attr) = selected_bg_attr {
                    changes.push(AttributeChange::Background(selected_bg_attr).into());
                    attr.set_background(selected_bg_attr);

                    if let Some(selected_fg_attr) = selected_fg_attr {
                        changes.push(AttributeChange::Foreground(selected_fg_attr).into());
                        attr.set_foreground(selected_fg_attr);
                    }
                } else {
                    changes.push(AttributeChange::Reverse(true).into());
                    attr.set_reverse(true);
                    used_reverse_for_selection = true;
                }
            }

            // from above we know that row_num <= max_items
            // show labels as long as we have more labels left
            // and we are not filtering
            if !self.filtering {
                if let Some(label) = labels_iter.next() {
                    if let Some(launcher_label_bg) = launcher_label_bg {
                        changes.push(AttributeChange::Background(launcher_label_bg.into()).into());
                    }
                    if let Some(launcher_label_fg) = launcher_label_fg {
                        changes.push(AttributeChange::Foreground(launcher_label_fg.into()).into());
                    }
                    changes.push(Change::Text(format!(" {label:>max_label_len$}. ")));
                    if launcher_label_bg.is_some() {
                        changes.push(AttributeChange::Background(ColorAttribute::Default).into());
                    }
                    if launcher_label_fg.is_some() {
                        changes.push(AttributeChange::Foreground(ColorAttribute::Default).into());
                    }
                } else {
                    changes.push(Change::Text(" ".repeat(max_label_len + 3)));
                }
            } else if !self.always_fuzzy {
                changes.push(Change::Text(" ".repeat(max_label_len + 3)));
            } else {
                changes.push(Change::Text("    ".to_string()));
            }

            let mut line = crate::tabbar::parse_status_text(&entry.label, attr.clone());
            if line.len() > max_width {
                line.resize(max_width, termwiz::surface::SEQ_ZERO);
            }
            changes.append(&mut line.changes(&attr));
            changes.push(Change::Text(" ".to_string()));

            if used_reverse_for_selection {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(Change::Text("\r\n".to_string()));
        }

        if self.filtering || !self.filter_term.is_empty() {
            changes.append(&mut vec![
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::ClearToEndOfLine(ColorAttribute::Default),
                Change::Text(truncate_right(
                    &format!("{}{}", &self.fuzzy_help_text, self.filter_term),
                    max_width,
                )),
            ]);
        }

        term.render(&changes)
    }

    fn launch(&mut self, active_idx: usize) -> Option<LauncherAction> {
        let action = match self.filtered_entries.get(active_idx) {
            Some(entry) => entry.action.clone(),
            None => return None,
        };
        if let LauncherAction::Assignment(KeyAssignment::ShowLauncherArgs(ref args)) = action {
            if args.flags.contains(LauncherFlags::PANE_ENCODINGS) {
                self.enter_encoding_submenu();
                return None;
            }
        }
        Some(action)
    }

    fn close_tab_action(&self) -> Option<LauncherAction> {
        backspace_tab_close_action(
            self.allow_tab_close,
            self.filtering,
            self.filtered_entries
                .get(self.active_idx)
                .map(|entry| &entry.action),
        )
    }

    fn move_up(&mut self) {
        self.active_idx = self.active_idx.saturating_sub(1);
        if self.active_idx < self.top_row {
            self.top_row = self.active_idx;
        }
    }

    fn move_down(&mut self) {
        self.active_idx = (self.active_idx + 1).min(self.filtered_entries.len() - 1);
        if self.active_idx > self.top_row + self.max_items {
            self.top_row = self.active_idx.saturating_sub(self.max_items);
        }
    }

    fn enter_encoding_submenu(&mut self) {
        let parent = ParentLauncherState {
            entries: std::mem::take(&mut self.entries),
            active_idx: self.active_idx,
            top_row: self.top_row,
            help_text: self.help_text.clone(),
            filter_term: std::mem::take(&mut self.filter_term),
            filtering: self.filtering,
        };
        self.parent_state = Some(parent);
        self.entries.clear();
        for encoding in PaneEncoding::ordered_list() {
            self.entries.push(Entry {
                label: format!("Set pane encoding to {encoding}"),
                action: SetPaneEncoding(encoding).into(),
            });
        }
        self.help_text =
            "Select encoding  |  Enter = set  |  Esc = back  |  / = filter".to_string();
        self.active_idx = 0;
        self.top_row = 0;
        self.filtering = false;
        self.selection.clear();
        self.update_filter();
    }

    fn exit_submenu(&mut self) -> bool {
        if let Some(parent) = self.parent_state.take() {
            self.entries = parent.entries;
            self.active_idx = parent.active_idx;
            self.top_row = parent.top_row;
            self.help_text = parent.help_text;
            self.filter_term = parent.filter_term;
            self.filtering = parent.filtering;
            self.selection.clear();
            self.update_filter();
            true
        } else {
            false
        }
    }

    fn run_loop(&mut self, term: &mut TermWizTerminal) -> anyhow::Result<Option<LauncherAction>> {
        while let Ok(Some(event)) = term.poll_input(None) {
            match event {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    modifiers: Modifiers::NONE,
                }) if !self.filtering && self.alphabet.contains(c) => {
                    self.selection.push(c);
                    if let Some(pos) = self.labels.iter().position(|x| *x == self.selection) {
                        // since the number of labels is always <= self.max_items
                        // by construction, we have pos as usize <= self.max_items
                        // for free
                        self.active_idx = self.top_row + pos as usize;
                        if let Some(action) = self.launch(self.active_idx) {
                            return Ok(Some(action));
                        }
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('j'),
                    ..
                }) if !self.filtering => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('k'),
                    ..
                }) if !self.filtering => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('P' | 'K'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('N' | 'J'),
                    modifiers: Modifiers::CTRL,
                }) => {
                    self.move_down();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('/'),
                    ..
                }) if !self.filtering => {
                    self.filtering = true;
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Backspace,
                    ..
                }) => {
                    if let Some(action) = self.close_tab_action() {
                        return Ok(Some(action));
                    }
                    if !self.filtering {
                        self.selection.pop();
                    } else {
                        if self.filter_term.pop().is_none() && !self.always_fuzzy {
                            self.filtering = false;
                        }
                        self.update_filter();
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('G') | KeyCode::Char('['),
                    modifiers: Modifiers::CTRL,
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                }) => {
                    let left_submenu = self.exit_submenu();
                    if !left_submenu {
                        break;
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    ..
                }) if self.filtering => {
                    self.filter_term.push(c);
                    self.update_filter();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::UpArrow,
                    ..
                }) => {
                    self.move_up();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::DownArrow,
                    ..
                }) => {
                    self.move_down();
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) if mouse_buttons.contains(MouseButtons::VERT_WHEEL) => {
                    if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                        self.top_row = self.top_row.saturating_sub(1);
                    } else {
                        self.top_row += 1;
                        self.top_row = self.top_row.min(
                            self.filtered_entries
                                .len()
                                .saturating_sub(self.max_items)
                                .saturating_sub(1),
                        );
                    }
                    if y > 0 && y as usize <= self.filtered_entries.len() {
                        self.active_idx = self.top_row + y as usize - 1;
                    }
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) => {
                    let on_entry_row = y > 0 && y as usize <= self.filtered_entries.len();
                    if on_entry_row {
                        self.active_idx = self.top_row + y as usize - 1;

                        if mouse_buttons == MouseButtons::LEFT {
                            if let Some(action) = self.launch(self.active_idx) {
                                return Ok(Some(action));
                            }
                        }
                    } else if y as usize > self.filtered_entries.len()
                        && mouse_buttons != MouseButtons::NONE
                    {
                        // A press below the list closes it like Esc, so
                        // pointer-only setups are not stuck in the navigator.
                        break;
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Enter,
                    ..
                }) => {
                    if let Some(action) = self.launch(self.active_idx) {
                        return Ok(Some(action));
                    }
                }
                _ => {}
            }
            self.render(term)?;
        }

        Ok(None)
    }
}

fn backspace_tab_close_action(
    allow_tab_close: bool,
    filtering: bool,
    action: Option<&LauncherAction>,
) -> Option<LauncherAction> {
    if !allow_tab_close || filtering {
        return None;
    }

    match action {
        Some(LauncherAction::ActivatePane { tab_id, .. }) => {
            Some(LauncherAction::CloseNavigatorTab(*tab_id))
        }
        _ => None,
    }
}

fn launcher_tab_action(tab: &LauncherTabEntry) -> LauncherAction {
    LauncherAction::ActivatePane {
        pane_id: tab.pane_id,
        tab_id: tab.tab_id,
    }
}

pub fn launcher(
    args: LauncherArgs,
    mut term: TermWizTerminal,
    initial_choice_idx: usize,
    allow_tab_close: bool,
) -> anyhow::Result<Option<LauncherAction>> {
    let filtering = args.flags.contains(LauncherFlags::FUZZY);
    let mut state = LauncherState {
        active_idx: initial_choice_idx,
        max_items: 0,
        top_row: 0,
        entries: vec![],
        filter_term: String::new(),
        filtered_entries: vec![],
        filtering,
        help_text: args.help_text.clone(),
        fuzzy_help_text: args.fuzzy_help_text.clone(),
        labels: vec![],
        selection: String::new(),
        alphabet: args.alphabet.clone(),
        always_fuzzy: filtering,
        allow_tab_close,
        parent_state: None,
    };

    term.set_raw_mode()?;
    term.render(&[Change::Title(args.title.to_string())])?;
    state.build_entries(args);
    state.update_filter();
    state.render(&mut term)?;
    state.run_loop(&mut term)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_action(tab_id: TabId) -> LauncherAction {
        LauncherAction::ActivatePane {
            pane_id: 42.into(),
            tab_id,
        }
    }

    #[test]
    fn pointer_only_users_can_dismiss_list_overlays() {
        // Touch over remote desktop has no easy Esc: a press away from the
        // list closes the tab navigator and the command palette.
        let launcher = include_str!("launcher.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(launcher.contains(
            "} else if y as usize > self.filtered_entries.len()\n                        && mouse_buttons != MouseButtons::NONE\n                    {"
        ));
        let palette = include_str!("../termwindow/palette.rs");
        assert!(palette.contains("} else if !self.point_in_panel(abs_x, abs_y) {"));
        let dismiss = palette
            .split("} else if !self.point_in_panel(abs_x, abs_y) {")
            .nth(1)
            .unwrap();
        assert!(dismiss
            .split('}')
            .next()
            .unwrap()
            .contains("term_window.cancel_modal();"));
    }

    #[test]
    fn pane_entries_keep_the_stable_pane_id() {
        let entry = LauncherTabEntry {
            title: "  |- src/main.rs".to_string(),
            pane_id: 42.into(),
            tab_id: TabId::new(7),
        };

        assert_eq!(
            launcher_tab_action(&entry),
            LauncherAction::ActivatePane {
                pane_id: 42.into(),
                tab_id: TabId::new(7),
            }
        );
    }

    #[test]
    fn backspace_closes_the_selected_tab_in_the_navigator() {
        let action = pane_action(TabId::new(7));

        assert_eq!(
            backspace_tab_close_action(true, false, Some(&action)),
            Some(LauncherAction::CloseNavigatorTab(TabId::new(7)))
        );
    }

    #[test]
    fn backspace_does_not_close_a_tab_while_filtering() {
        let action = pane_action(TabId::new(7));

        assert_eq!(backspace_tab_close_action(true, true, Some(&action)), None);
    }

    #[test]
    fn backspace_does_not_close_a_tab_in_a_custom_launcher() {
        let action = pane_action(TabId::new(7));

        assert_eq!(
            backspace_tab_close_action(false, false, Some(&action)),
            None
        );
    }
}
