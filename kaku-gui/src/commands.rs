use crate::inputmap::InputMap;
use config::keyassignment::{ClipboardCopyDestination, ClipboardPasteSource, PaneEncoding, *};
use config::window::WindowLevel;
use config::{ConfigHandle, DeferredKeyCode};
use mux::domain::DomainState;
use mux::Mux;
use ordered_float::NotNan;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::convert::TryFrom;
use window::{KeyCode, Modifiers};
use KeyAssignment::*;

/// Describes an argument/parameter/context that is required
/// in order for the command to have meaning.
/// The intent is for this to be used when filtering the items
/// that should be shown in eg: a context menu.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ArgType {
    /// Operates on the active pane
    ActivePane,
    /// Operates on the active tab
    ActiveTab,
    /// Operates on the active window
    ActiveWindow,
}

/// A helper function used to synthesize key binding permutations.
/// If the input is a character on a US ANSI keyboard layout, returns
/// the the typical character that is produced when holding down
/// the shift key and pressing the original key.
/// This doesn't produce an exhaustive list because there are only
/// a handful of default assignments in the command DEFS below.
fn us_layout_shift(s: &str) -> String {
    match s {
        "1" => "!".to_string(),
        "2" => "@".to_string(),
        "3" => "#".to_string(),
        "4" => "$".to_string(),
        "5" => "%".to_string(),
        "6" => "^".to_string(),
        "7" => "&".to_string(),
        "8" => "*".to_string(),
        "9" => "(".to_string(),
        "0" => ")".to_string(),
        "[" => "{".to_string(),
        "]" => "}".to_string(),
        "=" => "+".to_string(),
        "-" => "_".to_string(),
        "'" => "\"".to_string(),
        s if s.len() == 1 => s.to_ascii_uppercase(),
        s => s.to_string(),
    }
}

/// `CommandDef` defines a command in the UI.
pub struct CommandDef {
    /// Brief description
    pub brief: Cow<'static, str>,
    /// A longer, more detailed, description
    pub doc: Cow<'static, str>,
    /// The key assignments associated with this command.
    pub keys: Vec<(Modifiers, String)>,
    /// The argument types/context in which this command is valid.
    pub args: &'static [ArgType],
    /// Where to place the command in a menubar
    pub menubar: &'static [&'static str],
    pub icon: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ExpandedCommand {
    pub brief: Cow<'static, str>,
    pub doc: Cow<'static, str>,
    pub action: KeyAssignment,
    pub keys: Vec<(Modifiers, KeyCode)>,
    pub menubar: &'static [&'static str],
    pub icon: Option<Cow<'static, str>>,
}

impl std::fmt::Debug for CommandDef {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("CommandDef")
            .field("brief", &self.brief)
            .field("doc", &self.doc)
            .field("keys", &self.keys)
            .field("args", &self.args)
            .finish()
    }
}

impl CommandDef {
    /// Blech. Depending on the OS, a shifted key combination
    /// such as CTRL-SHIFT-L may present as either:
    /// CTRL+SHIFT + mapped lowercase l
    /// CTRL+SHIFT + mapped uppercase l
    /// CTRL       + mapped uppercase l
    ///
    /// This logic synthesizes the different combinations so
    /// that it isn't such a headache to maintain the mapping
    /// and prevents missing cases.
    ///
    /// Note that the mapped form of these things assumes
    /// US layout for some of the special shifted/punctuation cases.
    /// It's not perfect.
    ///
    /// The synthesis here requires that the defaults in
    /// the keymap below use the lowercase form of single characters!
    fn permute_keys(&self, config: &ConfigHandle) -> Vec<(Modifiers, KeyCode)> {
        let mut keys = vec![];

        for (mods, label) in &self.keys {
            let mods = *mods;
            let key = DeferredKeyCode::try_from(label.as_str())
                .unwrap()
                .resolve(config.key_map_preference)
                .clone();

            let ukey = DeferredKeyCode::try_from(us_layout_shift(&label))
                .unwrap()
                .resolve(config.key_map_preference)
                .clone();

            keys.push((mods, key.clone()));

            if mods == Modifiers::SUPER {
                // We want each SUPER/CMD version of the keys to also have
                // CTRL+SHIFT version(s) for environments where SUPER/CMD
                // is reserved for the window manager.
                // This bit synthesizes those.
                keys.push((Modifiers::CTRL | Modifiers::SHIFT, key.clone()));
                if ukey != key {
                    keys.push((Modifiers::CTRL | Modifiers::SHIFT, ukey.clone()));
                    keys.push((Modifiers::CTRL, ukey.clone()));
                }
            } else if mods.contains(Modifiers::SHIFT) && ukey != key {
                keys.push((mods, ukey.clone()));
                keys.push((mods - Modifiers::SHIFT, ukey.clone()));
            }
        }

        keys
    }

    /// Produces the list of default key assignments and actions.
    /// Used by the InputMap.
    pub fn default_key_assignments(
        config: &ConfigHandle,
    ) -> Vec<(Modifiers, KeyCode, KeyAssignment)> {
        let mut result = vec![];
        for cmd in Self::expanded_commands(config) {
            for (mods, code) in cmd.keys {
                result.push((mods, code.clone(), cmd.action.clone()));
            }
        }
        result
    }

    fn expand_action(
        action: KeyAssignment,
        config: &ConfigHandle,
        is_built_in: bool,
    ) -> Option<ExpandedCommand> {
        match derive_command_from_key_assignment(&action) {
            None => {
                if is_built_in {
                    log::warn!(
                        "{action:?} is a default action, but we cannot derive a CommandDef for it"
                    );
                }
                None
            }
            Some(def) => {
                let keys = if is_built_in && config.disable_default_key_bindings {
                    vec![]
                } else {
                    def.permute_keys(config)
                };
                Some(ExpandedCommand {
                    brief: def.brief.into(),
                    doc: def.doc.into(),
                    keys,
                    action,
                    menubar: def.menubar,
                    icon: def.icon.map(Cow::Borrowed),
                })
            }
        }
    }

    /// Produces the complete set of expanded commands.
    pub fn expanded_commands(config: &ConfigHandle) -> Vec<ExpandedCommand> {
        let mut result = vec![];

        for action in compute_default_actions() {
            if let Some(command) = Self::expand_action(action, config, true) {
                result.push(command);
            }
        }

        result
    }

    /// Returns only essential commands for Command Palette (fast, lightweight)
    pub fn actions_for_palette_only(config: &ConfigHandle) -> Vec<ExpandedCommand> {
        fn is_palette_noise_action(action: &KeyAssignment) -> bool {
            matches!(
                action,
                SendString(_) | SendKey(_) | Nop | Multiple(_) | ActivateTab(_)
            )
        }

        // Only include core actions, not dynamic domain/workspace/launch_menu commands
        let core_actions = [
            // Shell menu
            SpawnWindow,
            SpawnTab(SpawnTabDomain::CurrentPaneDomain),
            SplitHorizontal(SpawnCommand::default()),
            SplitVertical(SpawnCommand::default()),
            CloseCurrentTab { confirm: true },
            CloseCurrentPane { confirm: true },
            RestorePreviousWindow,
            // Edit menu
            CopyTo(ClipboardCopyDestination::Clipboard),
            PasteFrom(ClipboardPasteSource::Clipboard),
            Search(Pattern::CurrentSelectionOrEmptyString),
            QuickSelect,
            ClearScrollback(ScrollbackEraseMode::ScrollbackOnly),
            // View menu
            ResetFontSize,
            IncreaseFontSize,
            DecreaseFontSize,
            ResetFontAndWindowSize,
            ScrollToTop,
            ScrollToBottom,
            // Window menu
            ToggleFullScreen,
            CenterWindow,
            MaximizeWindow,
            Hide,
            ToggleAlwaysOnTop,
            ToggleAlwaysOnBottom,
            ActivateWindowRelative(-1),
            ActivateWindowRelative(1),
            ActivateTabRelative(-1),
            ActivateTabRelative(1),
            ActivateLastTab,
            ToggleCurrentTabPanesInputBroadcast,
            ToggleAllPanesInputBroadcast,
            MoveTabRelative(-1),
            MoveTabRelative(1),
            TogglePaneZoomState,
            ShowTabNavigator,
            // Help menu
            ShowDebugOverlay,
            OpenUri("https://github.com/tw93/Kaku".to_string()),
            OpenUri("https://github.com/tw93/Kaku/issues/".to_string()),
        ];

        let mut result = vec![];
        for action in &core_actions {
            if let Some(command) = Self::expand_action(action.clone(), config, true) {
                result.push(command);
            }
        }

        // Add key assignments that aren't already in the list
        let inputmap = InputMap::new(config);
        for ((keycode, mods), entry) in inputmap.keys.default.iter() {
            if is_palette_noise_action(&entry.action) {
                continue;
            }

            if let Some(existing) = result.iter_mut().find(|cmd| cmd.action == entry.action) {
                if !existing.keys.iter().any(|(existing_mods, existing_key)| {
                    *existing_mods == *mods && existing_key == keycode
                }) {
                    existing.keys.push((*mods, keycode.clone()));
                }
                continue;
            }

            if let Some(cmd) = derive_command_from_key_assignment(&entry.action) {
                result.push(ExpandedCommand {
                    brief: cmd.brief.into(),
                    doc: cmd.doc.into(),
                    keys: vec![(*mods, keycode.clone())],
                    action: entry.action.clone(),
                    menubar: cmd.menubar,
                    icon: cmd.icon.map(Cow::Borrowed),
                });
            }
        }

        // Keep shortcut display aligned with the effective keymap, including user overrides.
        for cmd in &mut result {
            let mut merged_keys = vec![];
            let mut push_unique_key = |mods: Modifiers, key: KeyCode| {
                if !merged_keys.iter().any(|(existing_mods, existing_key)| {
                    *existing_mods == mods && *existing_key == key
                }) {
                    merged_keys.push((mods, key));
                }
            };

            for key in &config.keys {
                if key.action != cmd.action {
                    continue;
                }
                push_unique_key(
                    key.key.mods,
                    key.key.key.resolve(config.key_map_preference).clone(),
                );
            }

            for (key, mods) in inputmap.locate_app_wide_key_assignment(&cmd.action) {
                push_unique_key(mods, key);
            }

            merged_keys.sort_by(|(a_mods, a_key), (b_mods, b_key)| {
                fn score_mods(mods: &Modifiers) -> usize {
                    let mut score: usize = mods.bits() as usize;
                    if mods.contains(Modifiers::SUPER) {
                        score += 1000;
                    }
                    score
                }

                score_mods(b_mods)
                    .cmp(&score_mods(a_mods))
                    .then_with(|| a_key.cmp(b_key))
            });
            merged_keys.dedup();
            cmd.keys = merged_keys;
        }

        fn is_explicitly_bound(action: &KeyAssignment, config: &ConfigHandle) -> bool {
            config.keys.iter().any(|key| key.action == *action)
        }

        fn rank_command(cmd: &ExpandedCommand, config: &ConfigHandle) -> (u8, usize) {
            let explicit = is_explicitly_bound(&cmd.action, config);
            let level = if explicit {
                2
            } else if !cmd.keys.is_empty() {
                1
            } else {
                0
            };
            (level, cmd.keys.len())
        }

        fn is_high_value_discovery_action(action: &KeyAssignment) -> bool {
            matches!(
                action,
                SplitHorizontal(_)
                    | SplitVertical(_)
                    | Search(_)
                    | QuickSelect
                    | ShowLauncher
                    | ShowLauncherArgs(_)
                    | ShowTabNavigator
                    | RestorePreviousWindow
                    | ActivateTabRelative(_)
                    | ActivateLastTab
                    | MoveTabRelative(_)
                    | TogglePaneZoomState
                    | AdjustPaneSize(_, _)
                    | ActivatePaneDirection(_)
            ) || matches!(
                action,
                EmitEvent(name)
                    if name == "kaku-ai-chat"
                        || name == "kaku-launch-lazygit"
                        || name == "kaku-launch-yazi"
                        || name == "run-kaku-ai-config"
            )
        }

        fn is_familiar_action(action: &KeyAssignment) -> bool {
            matches!(
                action,
                SpawnWindow
                    | SpawnTab(_)
                    | CopyTo(_)
                    | PasteFrom(_)
                    | CloseCurrentTab { .. }
                    | CloseCurrentPane { .. }
                    | Hide
                    | ToggleFullScreen
                    | IncreaseFontSize
                    | DecreaseFontSize
                    | ResetFontSize
                    | ResetFontAndWindowSize
            )
        }

        fn is_rare_action(action: &KeyAssignment) -> bool {
            matches!(
                action,
                ShowDebugOverlay | OpenUri(_) | ScrollToTop | ScrollToBottom | ToggleAlwaysOnBottom
            )
        }

        fn browse_bucket(cmd: &ExpandedCommand) -> u8 {
            let has_key = !cmd.keys.is_empty();
            if is_rare_action(&cmd.action) {
                return 4;
            }
            if is_familiar_action(&cmd.action) {
                return 0;
            }
            if is_high_value_discovery_action(&cmd.action) {
                return 1;
            }
            if has_key {
                return 2;
            }
            3
        }

        /// Sub-order by menu category so semantically related commands cluster.
        fn semantic_group(cmd: &ExpandedCommand) -> u8 {
            match cmd.menubar.first().copied() {
                Some("Shell") => 0,
                Some("Edit") => 1,
                Some("View") => 2,
                Some("Window") => 3,
                Some("Help") => 4,
                _ => 5,
            }
        }

        fn action_dedupe_identity(action: &KeyAssignment) -> String {
            match action {
                // Treat both "new tab in current/default domain" as the same palette item.
                SpawnTab(SpawnTabDomain::CurrentPaneDomain | SpawnTabDomain::DefaultDomain) => {
                    "spawn_tab_default".to_string()
                }
                // Treat confirm/no-confirm variants as the same command in palette.
                CloseCurrentTab { .. } => "close_current_tab".to_string(),
                CloseCurrentPane { .. } => "close_current_pane".to_string(),
                _ => format!("{action:?}"),
            }
        }

        let mut deduped = vec![];
        let mut by_identity: HashMap<(String, String), usize> = HashMap::new();
        for cmd in result {
            let label = cmd.brief.trim().to_ascii_lowercase();
            let identity = action_dedupe_identity(&cmd.action);
            let key = (label, identity);
            match by_identity.get(&key).copied() {
                None => {
                    by_identity.insert(key, deduped.len());
                    deduped.push(cmd);
                }
                Some(idx) => {
                    if rank_command(&cmd, config) > rank_command(&deduped[idx], config) {
                        deduped[idx] = cmd;
                    }
                }
            }
        }

        // Default browsing order: familiar first, then discovery, then others, rare last.
        deduped.sort_by(|a, b| {
            browse_bucket(a)
                .cmp(&browse_bucket(b))
                .then_with(|| semantic_group(a).cmp(&semantic_group(b)))
                .then_with(|| a.brief.cmp(&b.brief))
        });

        deduped
    }

    pub fn actions_for_palette_and_menubar(config: &ConfigHandle) -> Vec<ExpandedCommand> {
        let mut result = Self::expanded_commands(config);

        // Generate some stuff based on the config
        for cmd in &config.launch_menu {
            let label = match cmd.label.as_ref() {
                Some(label) => label.to_string(),
                None => match cmd.args.as_ref() {
                    Some(args) => args.join(" "),
                    None => "(default shell)".to_string(),
                },
            };
            result.push(ExpandedCommand {
                brief: format!("{label} (New Tab)").into(),
                doc: "".into(),
                keys: vec![],
                action: KeyAssignment::SpawnCommandInNewTab(cmd.clone()),
                menubar: &["Shell"],
                icon: None,
            });
        }

        // Generate some stuff based on the mux state
        if let Some(mux) = Mux::try_get() {
            let mut domains = mux.iter_domains();
            domains.sort_by(|a, b| {
                let a_state = a.state();
                let b_state = b.state();
                if a_state != b_state {
                    return if a_state == DomainState::Attached {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                a.domain_id().cmp(&b.domain_id())
            });
            for dom in &domains {
                let name = dom.domain_name();
                // FIXME: use domain_label here, but needs to be async
                let label = name;

                if dom.spawnable() {
                    if dom.state() == DomainState::Attached {
                        result.push(ExpandedCommand {
                            brief: format!("New Tab (Domain {label})").into(),
                            doc: "".into(),
                            keys: vec![],
                            action: KeyAssignment::SpawnCommandInNewTab(SpawnCommand {
                                domain: SpawnTabDomain::DomainName(name.to_string()),
                                ..SpawnCommand::default()
                            }),
                            menubar: &["Shell"],
                            icon: None,
                        });
                    } else {
                        result.push(ExpandedCommand {
                            brief: format!("Attach Domain {label}").into(),
                            doc: "".into(),
                            keys: vec![],
                            action: KeyAssignment::AttachDomain(name.to_string()),
                            menubar: &["Shell"],
                            icon: None,
                        });
                    }
                }
            }
            for dom in &domains {
                let name = dom.domain_name();
                // FIXME: use domain_label here, but needs to be async
                let label = name;

                if dom.state() == DomainState::Attached {
                    if name == "local" {
                        continue;
                    }
                    result.push(ExpandedCommand {
                        brief: format!("Detach Domain {label}").into(),
                        doc: "".into(),
                        keys: vec![],
                        action: KeyAssignment::DetachDomain(SpawnTabDomain::DomainName(
                            name.to_string(),
                        )),
                        menubar: &["Shell"],
                        icon: None,
                    });
                }
            }

            let active_workspace = mux.active_workspace();
            for workspace in mux.iter_workspaces() {
                if workspace != active_workspace {
                    result.push(ExpandedCommand {
                        brief: format!("Switch to workspace {workspace}").into(),
                        doc: "".into(),
                        keys: vec![],
                        action: KeyAssignment::SwitchToWorkspace {
                            name: Some(workspace.clone()),
                            spawn: None,
                        },
                        menubar: &["Window"],
                        icon: None,
                    });
                }
            }
            result.push(ExpandedCommand {
                brief: "Create new Workspace".into(),
                doc: "".into(),
                keys: vec![],
                action: KeyAssignment::SwitchToWorkspace {
                    name: None,
                    spawn: None,
                },
                menubar: &["Window"],
                icon: None,
            });
        }

        // And sweep to pick up stuff from their key assignments
        let inputmap = InputMap::new(config);
        for ((keycode, mods), entry) in inputmap.keys.default.iter() {
            if result
                .iter()
                .position(|cmd| cmd.action == entry.action)
                .is_some()
            {
                continue;
            }
            if let Some(cmd) = derive_command_from_key_assignment(&entry.action) {
                result.push(ExpandedCommand {
                    brief: cmd.brief.into(),
                    doc: cmd.doc.into(),
                    keys: vec![(*mods, keycode.clone())],
                    action: entry.action.clone(),
                    menubar: cmd.menubar,
                    icon: cmd.icon.map(Cow::Borrowed),
                });
            }
        }
        for table in inputmap.keys.by_name.values() {
            for entry in table.values() {
                if result
                    .iter()
                    .position(|cmd| cmd.action == entry.action)
                    .is_some()
                {
                    continue;
                }
                if let Some(cmd) = derive_command_from_key_assignment(&entry.action) {
                    result.push(ExpandedCommand {
                        brief: cmd.brief.into(),
                        doc: cmd.doc.into(),
                        keys: vec![],
                        action: entry.action.clone(),
                        menubar: cmd.menubar,
                        icon: cmd.icon.map(Cow::Borrowed),
                    });
                }
            }
        }

        result
    }

    #[cfg(not(target_os = "macos"))]
    pub fn recreate_menubar(_config: &ConfigHandle) {}

    /// Redirect AppKit's "Spotlight for Help" injection to an orphan menu
    /// that is never attached to the menubar. On macOS 26, AppKit's
    /// injected NSMenuItem lifetime is mis-handled, PAC-faulting during
    /// key-equivalent routing. Must run synchronously at startup, before
    /// any key event can trigger routeKeyEquivalent.
    #[cfg(target_os = "macos")]
    pub fn install_orphan_help_menu() {
        use std::sync::Once;
        use window::os::macos::menu::*;

        static HELP_SINK_ONCE: Once = Once::new();
        HELP_SINK_ONCE.call_once(|| {
            let help_sink = Menu::new_with_title("");
            help_sink.assign_as_help_menu();
            // NSApp's setHelpMenu: retains (strong property); forgetting the
            // Rust StrongPtr keeps refcount safely high for process lifetime.
            std::mem::forget(help_sink);
        });
    }

    /// Rebuild the macOS menubar from the current config and key bindings.
    /// Uses a mark-sweep approach: tag existing items, update or create new
    /// ones, then remove stale items at the end.
    #[cfg(target_os = "macos")]
    pub fn recreate_menubar(config: &ConfigHandle) {
        use window::os::macos::menu::*;
        use window::{Connection, ConnectionOps};

        Self::install_orphan_help_menu();

        let inputmap = InputMap::new(config);

        let mut candidates_for_removal = vec![];
        #[allow(unexpected_cfgs)] // <https://github.com/SSheldon/rust-objc/issues/125>
        let kaku_perform_key_assignment_sel = sel!(kakuPerformKeyAssignment:);

        /// Mark menu items as candidates for removal
        fn mark_candidates(menu: &Menu, candidates: &mut Vec<MenuItem>, action: SEL) {
            for item in menu.items() {
                if let Some(submenu) = item.get_sub_menu() {
                    mark_candidates(&submenu, candidates, action);
                }
                if item.get_action() == Some(action) {
                    item.set_tag(0);
                    candidates.push(item);
                }
            }
        }

        let main_menu = match Menu::get_main_menu() {
            Some(existing) => {
                mark_candidates(
                    &existing,
                    &mut candidates_for_removal,
                    kaku_perform_key_assignment_sel,
                );

                existing
            }
            None => {
                let menu = Menu::new_with_title("MainMenu");
                menu.assign_as_main_menu();
                menu
            }
        };

        let mut commands = Self::actions_for_palette_and_menubar(config);
        commands.retain(|cmd| !cmd.menubar.is_empty());

        // Prefer to put the menus in this order
        let mut order: Vec<&'static str> = vec!["Kaku", "Shell", "Edit", "View", "Window", "Help"];
        // Add any other menus on the end
        for cmd in &commands {
            if !order.contains(&cmd.menubar[0]) {
                order.push(cmd.menubar[0]);
            }
        }

        fn command_rank_for_menu(title: &str, action: &KeyAssignment) -> usize {
            match title {
                "Kaku" => match action {
                    HideApplication => 80,
                    QuitApplication => 90,
                    _ => 500,
                },
                "Shell" => match action {
                    SpawnWindow => 10,
                    SpawnTab(_) | SpawnCommandInNewTab(_) => 20,
                    EmitEvent(name) if name == "kaku-ai-chat" => 20,
                    EmitEvent(name) if name == "run-kaku-ai-config" => 21,
                    EmitEvent(name) if name == "kaku-launch-lazygit" => 22,
                    EmitEvent(name) if name == "kaku-launch-yazi" => 23,
                    EmitEvent(name) if name == "kaku-open-remote-files" => 24,
                    SplitVertical(_) | SplitHorizontal(_) | SplitPane(_) => 30,
                    CloseCurrentTab { .. } | CloseCurrentPane { .. } => 40,
                    RestorePreviousWindow => 42,
                    ActivateCommandPalette => 25,
                    ShowLauncher | ShowLauncherArgs(_) => 50,
                    AttachDomain(_) => 70,
                    DetachDomain(_) => 80,
                    _ => 500,
                },
                "Edit" => match action {
                    CopyTextTo { .. } | CopyTo(_) => 10,
                    PasteFrom(_) => 20,
                    Search(_) => 30,
                    QuickSelect => 40,
                    ClearScrollback(_) => 50,
                    _ => 500,
                },
                "View" => match action {
                    ResetFontSize => 10,
                    IncreaseFontSize => 20,
                    DecreaseFontSize => 30,
                    ResetFontAndWindowSize => 40,
                    ScrollToTop => 50,
                    ScrollToBottom => 51,
                    _ => 500,
                },
                "Window" => match action {
                    Hide => 10,
                    ToggleFullScreen => 12,
                    CenterWindow => 13,
                    MaximizeWindow => 14,
                    ToggleAlwaysOnTop => 15,
                    ActivateWindowRelative(-1) => 20,
                    ActivateWindowRelative(1) => 21,
                    ActivateWindow(_) => 22,
                    ActivateTabRelative(-1) => 30,
                    ActivateTabRelative(1) => 31,
                    ActivateLastTab => 32,
                    ShowTabNavigator => 33,
                    ToggleCurrentTabPanesInputBroadcast => 34,
                    ToggleAllPanesInputBroadcast => 35,
                    MoveTabRelative(-1) => 40,
                    MoveTabRelative(1) => 41,
                    PaneSelect(PaneSelectArguments {
                        mode: PaneSelectMode::Activate,
                        ..
                    }) => 50,
                    PaneSelect(PaneSelectArguments {
                        mode: PaneSelectMode::MoveToNewTab,
                        ..
                    }) => 51,
                    PaneSelect(PaneSelectArguments {
                        mode: PaneSelectMode::MoveToNewWindow,
                        ..
                    }) => 52,
                    TogglePaneZoomState => 60,
                    SwitchToWorkspace { .. } | SwitchWorkspaceRelative(_) => 70,
                    _ => 500,
                },
                "Help" => match action {
                    OpenUri(uri) if uri == "https://github.com/tw93/Kaku" => 10,
                    OpenUri(uri) if uri == "https://github.com/tw93/Kaku/discussions/" => 20,
                    OpenUri(uri) if uri == "https://github.com/tw93/Kaku/issues/" => 30,
                    ShowDebugOverlay => 90,
                    _ => 500,
                },
                _ => 1000,
            }
        }

        fn separator_group_for_menu(title: &str, rank: usize) -> usize {
            match title {
                "Shell" => match rank {
                    0..=20 => 1,  // New Window, New Tab
                    21..=26 => 2, // AI Config, Lazygit, Yazi, Remote Files, Command Palette
                    27..=35 => 3, // Split
                    36..=45 => 4, // Close
                    46..=55 => 5, // Launcher
                    _ => 6,       // Attach/Detach and others
                },
                "Edit" => match rank {
                    0..=25 => 1,  // Copy, Paste
                    26..=45 => 2, // Search, QuickSelect
                    _ => 3,       // Clear Scrollback
                },
                "View" => match rank {
                    0..=42 => 1,  // Font size group
                    43..=48 => 2, // Command Palette
                    _ => 3,       // Scroll
                },
                "Window" => match rank {
                    0..=15 => 1,  // Hide, FullScreen
                    16..=25 => 2, // Window nav
                    26..=35 => 3, // Tab nav
                    36..=45 => 4, // Move tab
                    46..=55 => 5, // Pane select
                    56..=65 => 6, // Toggle Pane Zoom
                    _ => 7,       // Workspace and others
                },
                _ => 1,
            }
        }

        for &title in &order {
            let mut menu_commands: Vec<&ExpandedCommand> = commands
                .iter()
                .filter(|cmd| cmd.menubar[0] == title)
                .collect();
            menu_commands.sort_by(|a, b| {
                command_rank_for_menu(title, &a.action)
                    .cmp(&command_rank_for_menu(title, &b.action))
                    .then_with(|| a.brief.cmp(&b.brief))
            });

            let mut prev_group: Option<usize> = None;
            for cmd in menu_commands {
                let rank = command_rank_for_menu(title, &cmd.action);
                let group = separator_group_for_menu(title, rank);

                let mut submenu = main_menu.get_or_create_sub_menu(&cmd.menubar[0], |menu| {
                    // Do not call setWindowsMenu: / setHelpMenu: on our
                    // submenus: on macOS 26, AppKit inserts auto-managed
                    // items (Tile/Move & Resize/Bring All to Front; Help
                    // Search field) whose NSMenuItem lifetime AppKit
                    // mis-handles, PAC-faulting during key-equivalent
                    // routing. Kaku owns its own tab/window switcher and
                    // has no help book, so the AppKit-managed children
                    // have no value here.
                    if cmd.menubar[0] == "Kaku" {
                        menu.assign_as_app_menu();

                        let about_item = MenuItem::new_with(
                            &format!("Kaku V{}", config::wezterm_version()),
                            Some(kaku_perform_key_assignment_sel),
                            "",
                        );
                        about_item.set_represented_item(RepresentedItem::KeyAssignment(
                            KeyAssignment::EmitEvent("run-kaku-cli".to_string()),
                        ));
                        menu.add_item(&about_item);

                        menu.add_item(&MenuItem::new_separator());

                        #[allow(unexpected_cfgs)]
                        // <https://github.com/SSheldon/rust-objc/issues/125>
                        let show_settings_window_sel = sel!(showSettingsWindow:);
                        let app_delegate = unsafe {
                            let app = cocoa::appkit::NSApp();
                            let delegate: cocoa::base::id = msg_send![app, delegate];
                            delegate
                        };
                        let settings_item =
                            MenuItem::new_with("Settings...", Some(show_settings_window_sel), ",");
                        settings_item
                            .set_key_equiv_modifier_mask(NSEventModifierFlags::NSCommandKeyMask);
                        if !app_delegate.is_null() {
                            settings_item.set_target(app_delegate);
                        }
                        menu.add_item(&settings_item);

                        let check_update = MenuItem::new_with(
                            "Check for Updates...",
                            Some(kaku_perform_key_assignment_sel),
                            "",
                        );
                        check_update.set_represented_item(RepresentedItem::KeyAssignment(
                            KeyAssignment::EmitEvent("run-kaku-update".to_string()),
                        ));
                        menu.add_item(&check_update);

                        // Show "Restart to Update" when a staged update is ready.
                        if let Some(info) = crate::update::staged_update_available() {
                            let version = info.tag.trim_start_matches(['v', 'V']);
                            let restart_item = MenuItem::new_with(
                                &format!("Restart to Update ({})", version),
                                Some(kaku_perform_key_assignment_sel),
                                "",
                            );
                            restart_item.set_represented_item(RepresentedItem::KeyAssignment(
                                KeyAssignment::EmitEvent("restart-to-update".to_string()),
                            ));
                            menu.add_item(&restart_item);
                        }

                        let set_default_terminal_item = MenuItem::new_with(
                            "Set as Default Terminal",
                            Some(kaku_perform_key_assignment_sel),
                            "",
                        );
                        set_default_terminal_item.set_represented_item(
                            RepresentedItem::KeyAssignment(KeyAssignment::EmitEvent(
                                crate::frontend::SET_DEFAULT_TERMINAL_EVENT.to_string(),
                            )),
                        );
                        if let Some(conn) = Connection::get() {
                            set_default_terminal_item.set_state(conn.is_default_terminal());
                        }
                        menu.add_item(&set_default_terminal_item);

                        menu.add_item(&MenuItem::new_separator());

                        let services_menu = Menu::new_with_title("Services");
                        services_menu.assign_as_services_menu();
                        let services_item = MenuItem::new_with("Services", None, "");
                        menu.add_item(&services_item);
                        services_item.set_sub_menu(&services_menu);

                        menu.add_item(&MenuItem::new_separator());
                    }
                });

                // Insert a separator when the logical group changes
                if cmd.menubar.len() == 1 {
                    if let Some(pg) = prev_group {
                        if pg != group {
                            submenu.add_item(&MenuItem::new_separator());
                        }
                    }
                    prev_group = Some(group);
                }

                // Fill out any submenu hierarchy
                for sub_title in cmd.menubar.iter().skip(1) {
                    submenu = submenu.get_or_create_sub_menu(sub_title, |_menu| {});
                }

                let mut candidate = inputmap.locate_app_wide_key_assignment(&cmd.action);
                candidate.sort_by(|(a_key, a_mods), (b_key, b_mods)| {
                    fn score_mods(mods: &Modifiers) -> usize {
                        let mut score: usize = mods.bits() as usize;
                        // Prefer keys with CMD on macOS
                        if mods.contains(Modifiers::SUPER) {
                            score += 1000;
                        }
                        score
                    }

                    let a_mods = score_mods(a_mods);
                    let b_mods = score_mods(b_mods);

                    match b_mods.cmp(&a_mods) {
                        Ordering::Equal => {}
                        ordering => return ordering,
                    }

                    a_key.cmp(&b_key)
                });

                fn key_code_to_equivalent(key: &KeyCode) -> String {
                    match key {
                        KeyCode::Hyper
                        | KeyCode::Super
                        | KeyCode::Meta
                        | KeyCode::Cancel
                        | KeyCode::Composed(_)
                        | KeyCode::RawCode(_) => "".to_string(),
                        KeyCode::Char(c) => c.to_string(),
                        KeyCode::Physical(phys) => key_code_to_equivalent(&phys.to_key_code()),
                        _ => "".to_string(),
                    }
                }

                let short_cut = candidate
                    .get(0)
                    .map(|(key, _)| key_code_to_equivalent(key))
                    .unwrap_or_else(String::new);

                let represented_item = RepresentedItem::KeyAssignment(cmd.action.clone());
                let item = match submenu.get_item_with_represented_item(&represented_item) {
                    Some(existing) => {
                        existing.set_title(&cmd.brief);
                        existing.set_key_equivalent(&short_cut);
                        existing
                    }
                    None => {
                        let item = MenuItem::new_with(
                            &cmd.brief,
                            Some(kaku_perform_key_assignment_sel),
                            &short_cut,
                        );
                        submenu.add_item(&item);
                        item
                    }
                };

                if !short_cut.is_empty() {
                    let mods: Modifiers = candidate[0].1;
                    let mut equiv_mods = NSEventModifierFlags::empty();

                    equiv_mods.set(
                        NSEventModifierFlags::NSShiftKeyMask,
                        mods.contains(Modifiers::SHIFT),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSAlternateKeyMask,
                        mods.contains(Modifiers::ALT),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSControlKeyMask,
                        mods.contains(Modifiers::CTRL),
                    );
                    equiv_mods.set(
                        NSEventModifierFlags::NSCommandKeyMask,
                        mods.contains(Modifiers::SUPER),
                    );

                    item.set_key_equiv_modifier_mask(equiv_mods);
                }

                item.set_represented_item(represented_item);
                // Update the tag to indicate that this item should
                // not be removed by the sweep below
                item.set_tag(1);
            }
        }

        // Now sweep away any items that were not updated
        for item in candidates_for_removal {
            if item.get_tag() == 0 {
                item.get_menu().map(|menu| menu.remove_item(&item));
            }
        }
    }
}

/// Given "1" return "1st", "2" -> "2nd" and so on
fn english_ordinal(n: isize) -> String {
    let n = n.to_string();
    if n.ends_with('1') && !n.ends_with("11") {
        format!("{n}st")
    } else if n.ends_with('2') && !n.ends_with("12") {
        format!("{n}nd")
    } else if n.ends_with('3') && !n.ends_with("13") {
        format!("{n}rd")
    } else {
        format!("{n}th")
    }
}

fn spawn_command_from_action(action: &KeyAssignment) -> Option<&SpawnCommand> {
    match action {
        SplitPane(config::keyassignment::SplitPane { command, .. }) => Some(command),
        SplitHorizontal(command)
        | SplitVertical(command)
        | SpawnCommandInNewWindow(command)
        | SpawnCommandInNewTab(command) => Some(command),
        _ => None,
    }
}

fn label_string(action: &KeyAssignment, candidate: String) -> String {
    if let Some(label) = spawn_command_from_action(action).and_then(|cmd| cmd.label_for_palette()) {
        label
    } else {
        candidate
    }
}

/// Describes a key assignment action; returns a bunch
/// of metadata that is useful in the command palette/menubar context.
/// This function will be called for the result of compute_default_actions(),
/// but can also be used to describe user-provided commands
pub(crate) fn is_internal_emit_event_name(name: &str) -> bool {
    name.starts_with("user-defined-")
}

pub fn derive_command_from_key_assignment(action: &KeyAssignment) -> Option<CommandDef> {
    if matches!(action, EmitEvent(name) if is_internal_emit_event_name(name)) {
        return None;
    }

    Some(match action {
        PasteFrom(ClipboardPasteSource::PrimarySelection) => CommandDef {
            brief: "Paste primary selection".into(),
            doc: "Pastes text from the primary selection".into(),
            keys: vec![(Modifiers::SHIFT, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::PrimarySelection,
        }
        | CopyTo(ClipboardCopyDestination::PrimarySelection) => CommandDef {
            brief: "Copy to primary selection".into(),
            doc: "Copies text to the primary selection".into(),
            keys: vec![(Modifiers::CTRL, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::Clipboard,
        }
        | CopyTo(ClipboardCopyDestination::Clipboard) => CommandDef {
            brief: "Copy to clipboard".into(),
            doc: "Copies text to the clipboard".into(),
            keys: vec![
                (Modifiers::SUPER, "c".into()),
                (Modifiers::NONE, "Copy".into()),
            ],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        CopyTextTo {
            text: _,
            destination: ClipboardCopyDestination::ClipboardAndPrimarySelection,
        }
        | CopyTo(ClipboardCopyDestination::ClipboardAndPrimarySelection) => CommandDef {
            brief: "Copy to clipboard and primary selection".into(),
            doc: "Copies text to the clipboard and the primary selection".into(),
            keys: vec![(Modifiers::CTRL, "Insert".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PasteFrom(ClipboardPasteSource::Clipboard) => CommandDef {
            brief: "Paste from clipboard".into(),
            doc: "Pastes text from the clipboard".into(),
            keys: vec![
                (Modifiers::SUPER, "v".into()),
                (Modifiers::NONE, "Paste".into()),
            ],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        ToggleFullScreen => CommandDef {
            brief: "Toggle Full Screen".into(),
            doc: "Toggle full screen mode".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SUPER), "f".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        CenterWindow => CommandDef {
            brief: "Center".into(),
            doc: "Center the current window on the active display".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        MaximizeWindow => CommandDef {
            brief: "Fill".into(),
            doc: "Zoom the window to fill the active display (AppKit zoom)".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ToggleAlwaysOnTop => CommandDef {
            brief: "Always on Top".into(),
            doc: "Keep window above others".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "UpArrow".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ToggleAlwaysOnBottom => CommandDef {
            brief: "Always on Bottom".into(),
            doc: "Keep window behind others".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SetWindowLevel(WindowLevel::AlwaysOnTop) => CommandDef {
            brief: "Always on Top".into(),
            doc: "Set the window level to be on top of other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SetWindowLevel(WindowLevel::Normal) => CommandDef {
            brief: "Normal".into(),
            doc: "Set window level to normal".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SetWindowLevel(WindowLevel::AlwaysOnBottom) => CommandDef {
            brief: "Always on Bottom".into(),
            doc: "Set window to remain behind all other windows.".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        Hide => CommandDef {
            brief: "Minimize".into(),
            doc: "Minimize current window".into(),
            keys: vec![(Modifiers::SUPER, "m".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        Show => CommandDef {
            brief: "Show/Restore Window".into(),
            doc: "Show/Restore the current window".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        HideApplication => CommandDef {
            brief: "Hide Kaku".into(),
            doc: "Hide all Kaku windows".into(),
            keys: vec![(Modifiers::SUPER, "h".into())],
            args: &[],
            menubar: &["Kaku"],
            icon: None,
        },
        SpawnWindow => CommandDef {
            brief: "New Window".into(),
            doc: "Open a new window".into(),
            keys: vec![(Modifiers::SUPER, "n".into())],
            args: &[],
            menubar: &["Shell"],
            icon: None,
        },
        ClearScrollback(ScrollbackEraseMode::ScrollbackOnly) => CommandDef {
            brief: "Clear Scrollback".into(),
            doc: "Clear scrollback history".into(),
            keys: vec![(Modifiers::SUPER, "k".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        ClearScrollback(ScrollbackEraseMode::ScrollbackAndViewport) => CommandDef {
            brief: "Clear the scrollback and viewport".into(),
            doc: "Removes all content from the screen and scrollback".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        Search(Pattern::CurrentSelectionOrEmptyString) => CommandDef {
            brief: "Search".into(),
            doc: "Search in current pane".into(),
            keys: vec![(Modifiers::SUPER, "f".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        Search(_) => CommandDef {
            brief: "Search pane output".into(),
            doc: "Enters the search mode UI for the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ShowDebugOverlay => CommandDef {
            brief: "Kaku Doctor".into(),
            doc: "Run kaku doctor in the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "l".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        InputSelector(_) => CommandDef {
            brief: "Prompt the user to choose from a list".into(),
            doc: "Activates the selector overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        Confirmation(_) => CommandDef {
            brief: "Prompt the user for confirmation".into(),
            doc: "Activates the confirmation overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        PromptInputLine(_) => CommandDef {
            brief: "Prompt the user for a line of text".into(),
            doc: "Activates the prompt overlay and wait for input".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        QuickSelect => CommandDef {
            brief: "QuickSelect".into(),
            doc: "Quick selection mode".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "Space".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Edit"],
            icon: None,
        },
        QuickSelectArgs(_) => CommandDef {
            brief: "Enter QuickSelect mode".into(),
            doc: "Activates the quick selection UI for the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        CharSelect(_) => CommandDef {
            brief: "Enter Emoji / Character selection mode".into(),
            doc: "Activates the character selection UI for the current pane".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "u".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::Activate,
            ..
        }) => CommandDef {
            brief: "Select Pane".into(),
            doc: "Select a pane interactively".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "p".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: None,
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::SwapWithActive,
            ..
        }) => CommandDef {
            brief: "Swap a pane with the active pane".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::SwapWithActiveKeepFocus,
            ..
        }) => CommandDef {
            brief: "Swap a pane with the active pane, keeping focus".into(),
            doc: "Activates the pane selection UI".into(),
            keys: vec![], // FIXME: find a new assignment
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::MoveToNewTab,
            ..
        }) => CommandDef {
            brief: "Move Pane to New Tab".into(),
            doc: "Move selected pane to a new tab".into(),
            keys: vec![(
                Modifiers::SUPER
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "t".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: None,
        },
        PaneSelect(PaneSelectArguments {
            mode: PaneSelectMode::MoveToNewWindow,
            ..
        }) => CommandDef {
            brief: "Move Pane to New Window".into(),
            doc: "Move selected pane to a new window".into(),
            keys: vec![(
                Modifiers::SUPER
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "n".into(),
            )],
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: None,
        },
        DecreaseFontSize => CommandDef {
            brief: "Decrease Font Size".into(),
            doc: "Make text smaller".into(),
            keys: vec![
                (Modifiers::SUPER, "-".into()),
                (Modifiers::CTRL, "-".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View"],
            icon: None,
        },
        IncreaseFontSize => CommandDef {
            brief: "Increase Font Size".into(),
            doc: "Make text larger".into(),
            keys: vec![
                (Modifiers::SUPER, "=".into()),
                (Modifiers::CTRL, "=".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View"],
            icon: None,
        },
        ResetFontSize => CommandDef {
            brief: "Reset Font Size".into(),
            doc: "Reset to configured font size".into(),
            keys: vec![
                (Modifiers::SUPER, "0".into()),
                (Modifiers::CTRL, "0".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["View"],
            icon: None,
        },
        ResetFontAndWindowSize => CommandDef {
            brief: "Reset Window & Font Size".into(),
            doc: "Reset window and font to defaults".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["View"],
            icon: None,
        },
        SpawnTab(SpawnTabDomain::CurrentPaneDomain) => CommandDef {
            brief: "New Tab".into(),
            doc: "Open a new tab".into(),
            keys: vec![(Modifiers::SUPER, "t".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        SpawnTab(SpawnTabDomain::DefaultDomain) => CommandDef {
            brief: "New Tab".into(),
            doc: "New tab in default domain".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        SpawnTab(SpawnTabDomain::DomainName(name)) => CommandDef {
            brief: format!("New Tab {name}").into(),
            doc: format!("New tab in {name} domain").into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SpawnTab(SpawnTabDomain::DomainId(id)) => CommandDef {
            brief: format!("New Tab Domain {id}").into(),
            doc: format!("New tab in domain {id}").into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SpawnCommandInNewTab(cmd) => CommandDef {
            brief: label_string(action, format!("Spawn a new Tab with {cmd:?}").to_string()).into(),
            doc: format!("Spawn a new Tab with {cmd:?}").into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SpawnCommandInNewWindow(cmd) => CommandDef {
            brief: label_string(
                action,
                format!("Spawn a new Window with {cmd:?}").to_string(),
            )
            .into(),
            doc: format!("Spawn a new Window with {cmd:?}").into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        ActivateTab(-1) => CommandDef {
            brief: "Activate right-most tab".into(),
            doc: "Activates the tab on the far right".into(),
            keys: vec![(Modifiers::SUPER, "9".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ActivateTab(n) => {
            let n = *n;
            let ordinal = english_ordinal(n + 1);
            let keys = if n >= 0 && n <= 7 {
                vec![(Modifiers::SUPER, (n + 1).to_string())]
            } else {
                vec![]
            };
            CommandDef {
                brief: format!("Activate {ordinal} Tab").into(),
                doc: format!("Activates the {ordinal} tab").into(),
                keys,
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivatePaneByIndex(n) => {
            let n = *n;
            let ordinal = english_ordinal(n as isize);
            CommandDef {
                brief: format!("Activate {ordinal} Pane").into(),
                doc: format!("Activates the {ordinal} Pane").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        SetPaneZoomState(true) => CommandDef {
            brief: format!("Zooms the current Pane").into(),
            doc: format!(
                "Places the current pane into the zoomed state, \
                             filling all of the space in the tab"
            )
            .into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SetPaneZoomState(false) => CommandDef {
            brief: format!("Un-Zooms the current Pane").into(),
            doc: format!("Takes the current pane out of the zoomed state").into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        SetPaneEncoding(encoding) => CommandDef {
            brief: format!("Set Pane Encoding to {encoding}").into(),
            doc: format!("Sets the current pane encoding to {encoding}").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        EmitEvent(name) => {
            if name == "kaku-ai-chat" {
                CommandDef {
                    brief: "AI Chat".into(),
                    doc: "Open AI conversation in current terminal pane".into(),
                    keys: vec![(Modifiers::SUPER, "l".into())],
                    args: &[ArgType::ActivePane],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else if name == "run-kaku-ai-config" {
                CommandDef {
                    brief: "AI Config".into(),
                    doc: "Open AI configuration".into(),
                    keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "a".into())],
                    args: &[ArgType::ActiveWindow],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else if name == "kaku-ai-apply-last-fix" {
                CommandDef {
                    brief: "Apply Last AI Fix".into(),
                    doc: "Apply the latest Kaku Assistant suggestion for the active pane".into(),
                    keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "e".into())],
                    args: &[ArgType::ActiveWindow],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else if name == "kaku-launch-lazygit" {
                CommandDef {
                    brief: "Lazygit".into(),
                    doc: "Open lazygit".into(),
                    keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "g".into())],
                    args: &[ArgType::ActiveWindow],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else if name == "kaku-launch-yazi" {
                CommandDef {
                    brief: "Yazi File Manager".into(),
                    doc: "Open Yazi file manager".into(),
                    keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "y".into())],
                    args: &[ArgType::ActiveWindow],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else if name == "kaku-open-remote-files" {
                CommandDef {
                    brief: "Remote Files".into(),
                    doc: "Open the current SSH domain in a local Yazi tab".into(),
                    keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "r".into())],
                    args: &[ArgType::ActiveWindow],
                    menubar: &["Shell"],
                    icon: None,
                }
            } else {
                CommandDef {
                    brief: format!("Emit event `{name}`").into(),
                    doc: format!(
                        "Emits the named event, causing any \
                             associated event handler(s) to trigger"
                    )
                    .into(),
                    keys: vec![],
                    args: &[ArgType::ActiveWindow],
                    menubar: &[],
                    icon: None,
                }
            }
        }
        CloseCurrentTab { confirm: true } => CommandDef {
            brief: "Close Tab".into(),
            doc: "Close current tab using the configured close confirmation policy".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "w".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Shell"],
            icon: None,
        },
        CloseCurrentTab { confirm: false } => CommandDef {
            brief: "Close Tab".into(),
            doc: "Close current tab without smart confirmation".into(),
            keys: vec![],
            args: &[ArgType::ActiveTab],
            menubar: &[],
            icon: None,
        },
        CloseCurrentPane { confirm: true } => CommandDef {
            brief: "Close Pane".into(),
            doc: "Close current pane using the configured close confirmation policy".into(),
            keys: vec![(Modifiers::SUPER, "w".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        CloseCurrentPane { confirm: false } => CommandDef {
            brief: "Close Pane".into(),
            doc: "Close current pane without smart confirmation".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ReopenLastClosedTab => CommandDef {
            brief: "Reopen Last Closed Tab".into(),
            doc: "Reopens the most recently closed tab, restoring its working directory.".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "t".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        RestorePreviousWindow => CommandDef {
            brief: "Restore Previous Window".into(),
            doc: "Restores the last saved window snapshot, including tabs and panes.".into(),
            keys: vec![(
                Modifiers::SUPER.union(Modifiers::ALT).union(Modifiers::SHIFT),
                "t".into(),
            )],
            args: &[ArgType::ActiveWindow],
            menubar: &["Shell"],
            icon: None,
        },
        ActivateWindow(n) => {
            let n = *n;
            let ordinal = english_ordinal(n as isize + 1);
            CommandDef {
                brief: format!("Activate {ordinal} Window").into(),
                doc: format!("Activates the {ordinal} window").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &["Window"],
                icon: None,
            }
        }
        ActivateWindowRelative(-1) => CommandDef {
            brief: "Previous Window".into(),
            doc: "Switch to previous window".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "`".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateWindowRelative(1) => CommandDef {
            brief: "Next Window".into(),
            doc: "Switch to next window".into(),
            keys: vec![(Modifiers::SUPER, "`".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateWindowRelative(n) => {
            let (direction, amount) = if *n < 0 {
                ("backwards", -n)
            } else {
                ("forwards", *n)
            };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} window {direction}").into(),
                doc: format!(
                    "Activates the {ordinal} window, moving {direction}. \
                         Wraps around to the other end"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateWindowRelativeNoWrap(-1) => CommandDef {
            brief: "Previous Window (No Wrap)".into(),
            doc: "Switch to previous window".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateWindowRelativeNoWrap(1) => CommandDef {
            brief: "Next Window (No Wrap)".into(),
            doc: "Switch to next window".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateWindowRelativeNoWrap(n) => {
            let (direction, amount) = if *n < 0 {
                ("backwards", -n)
            } else {
                ("forwards", *n)
            };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} window {direction}").into(),
                doc: format!("Activates the {ordinal} window, moving {direction}.").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateTabRelative(-1) => CommandDef {
            brief: "Previous Tab".into(),
            doc: "Switch to previous tab".into(),
            keys: vec![
                (Modifiers::SUPER.union(Modifiers::SHIFT), "[".into()),
                (Modifiers::CTRL.union(Modifiers::SHIFT), "Tab".into()),
                (Modifiers::CTRL, "PageUp".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateTabRelative(1) => CommandDef {
            brief: "Next Tab".into(),
            doc: "Switch to next tab".into(),
            keys: vec![
                (Modifiers::SUPER.union(Modifiers::SHIFT), "]".into()),
                (Modifiers::CTRL, "Tab".into()),
                (Modifiers::CTRL, "PageDown".into()),
            ],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ActivateTabRelative(n) => {
            let (direction, amount) = if *n < 0 { ("left", -n) } else { ("right", *n) };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} tab to the {direction}").into(),
                doc: format!(
                    "Activates the {ordinal} tab to the {direction}. \
                         Wraps around to the other end"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ActivateTabRelativeNoWrap(-1) => CommandDef {
            brief: "Activate the tab to the left (no wrapping)".into(),
            doc: "Activates the tab to the left. Stopping at the left-most tab".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ActivateTabRelativeNoWrap(1) => CommandDef {
            brief: "Activate the tab to the right (no wrapping)".into(),
            doc: "Activates the tab to the right. Stopping at the right-most tab".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ActivateTabRelativeNoWrap(n) => {
            let (direction, amount) = if *n < 0 { ("left", -n) } else { ("right", *n) };
            let ordinal = english_ordinal(amount + 1);
            CommandDef {
                brief: format!("Activate the {ordinal} tab to the {direction}").into(),
                doc: format!("Activates the {ordinal} tab to the {direction}").into(),
                keys: vec![],
                args: &[ArgType::ActiveWindow],
                menubar: &[],
                icon: None,
            }
        }
        ReloadConfiguration => CommandDef {
            brief: "Reload Configuration".into(),
            doc: "Reloads the configuration file.".into(),
            keys: vec![(Modifiers::SUPER | Modifiers::SHIFT, "R".into())],
            args: &[],
            menubar: &[],
            icon: None,
        },
        QuitApplication => CommandDef {
            brief: "Quit Kaku".into(),
            doc: "Quits Kaku".into(),
            keys: vec![(Modifiers::SUPER, "q".into())],
            args: &[],
            menubar: &["Kaku"],
            icon: None,
        },
        MoveTabRelative(-1) => CommandDef {
            brief: "Move Tab Left".into(),
            doc: "Move current tab left".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "PageUp".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Window"],
            icon: None,
        },
        MoveTabRelative(1) => CommandDef {
            brief: "Move Tab Right".into(),
            doc: "Move current tab right".into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "PageDown".into())],
            args: &[ArgType::ActiveTab],
            menubar: &["Window"],
            icon: None,
        },
        MoveTabRelative(n) => {
            let (direction, amount, _icon) = if *n < 0 {
                ("left", (-n).to_string(), "md_chevron_double_left")
            } else {
                ("right", n.to_string(), "md_chevron_double_right")
            };

            CommandDef {
                brief: format!("Move tab {amount} place(s) to the {direction}").into(),
                doc: format!(
                    "Rearranges the tabs so that the current tab moves \
            {amount} place(s) to the {direction}"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveTab],
                menubar: &[],
                icon: None,
            }
        }
        MoveTab(n) => {
            let n = (*n) + 1;
            CommandDef {
                brief: format!("Move tab to index {n}").into(),
                doc: format!(
                    "Rearranges the tabs so that the current tab \
                             moves to position {n}"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActiveTab],
                menubar: &[],
                icon: None,
            }
        }
        ScrollByPage(amount) => {
            let amount = amount.into_inner();
            if amount == -1.0 {
                CommandDef {
                    brief: "Scroll Up One Page".into(),
                    doc: "Scrolls the viewport up by 1 page".into(),
                    keys: vec![(Modifiers::SHIFT, "PageUp".into())],
                    args: &[ArgType::ActivePane],
                    menubar: &[],
                    icon: None,
                }
            } else if amount == 1.0 {
                CommandDef {
                    brief: "Scroll Down One Page".into(),
                    doc: "Scrolls the viewport down by 1 page".into(),
                    keys: vec![(Modifiers::SHIFT, "PageDown".into())],
                    args: &[ArgType::ActivePane],
                    menubar: &[],
                    icon: None,
                }
            } else if amount < 0.0 {
                let amount = -amount;
                CommandDef {
                    brief: format!("Scroll Up {amount} Page(s)").into(),
                    doc: format!("Scrolls the viewport up by {amount} pages").into(),
                    keys: vec![],
                    args: &[ArgType::ActivePane],
                    menubar: &[],
                    icon: None,
                }
            } else {
                CommandDef {
                    brief: format!("Scroll Down {amount} Page(s)").into(),
                    doc: format!("Scrolls the viewport down by {amount} pages").into(),
                    keys: vec![],
                    args: &[ArgType::ActivePane],
                    menubar: &[],
                    icon: None,
                }
            }
        }
        ScrollByLine(n) => {
            let (direction, amount) = if *n < 0 {
                ("up", (-n).to_string())
            } else {
                ("down", n.to_string())
            };
            CommandDef {
                brief: format!("Scroll {direction} {amount} line(s)").into(),
                doc: format!(
                    "Scrolls the viewport {direction} by \
                             {amount} line(s)"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        ScrollToPrompt(n) => {
            let (direction, amount) = if *n < 0 { ("up", -n) } else { ("down", *n) };
            let ordinal = english_ordinal(amount);
            CommandDef {
                brief: format!("Scroll {direction} {amount} prompt(s)").into(),
                doc: format!(
                    "Scrolls the viewport {direction} to the \
                             {ordinal} semantic prompt zone in that direction"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        ScrollByCurrentEventWheelDelta => CommandDef {
            brief: "Scrolls based on the mouse wheel position \
                in the current mouse event"
                .into(),
            doc: "Scrolls based on the mouse wheel position \
                in the current mouse event"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ScrollToBottom => CommandDef {
            brief: "Scroll to Bottom".into(),
            doc: "Scroll to bottom of output".into(),
            keys: vec![(Modifiers::SUPER, "End".into())],
            args: &[ArgType::ActivePane],
            menubar: &["View"],
            icon: None,
        },
        ScrollToTop => CommandDef {
            brief: "Scroll to Top".into(),
            doc: "Scroll to top of output".into(),
            keys: vec![(Modifiers::SUPER, "Home".into())],
            args: &[ArgType::ActivePane],
            menubar: &["View"],
            icon: None,
        },
        ActivateCopyMode => CommandDef {
            brief: "Activate Copy Mode".into(),
            doc: "Enter mouse-less copy mode to select text using only \
            the keyboard"
                .into(),
            keys: vec![(Modifiers::CTRL.union(Modifiers::SHIFT), "x".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        SplitVertical(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..
        }) => CommandDef {
            brief: label_string(action, "Split Pane Top/Bottom".to_string()).into(),
            doc: "Split pane horizontally".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "d".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        SplitHorizontal(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..
        }) => CommandDef {
            brief: label_string(action, "Split Pane Left/Right".to_string()).into(),
            doc: "Split pane vertically".into(),
            keys: vec![(Modifiers::SUPER, "d".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        SplitHorizontal(_) => CommandDef {
            brief: label_string(action, "Split Pane Left/Right".to_string()).into(),
            doc: "Split the current pane into left and right panes, by spawning \
            the default program into the right pane"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        SplitVertical(_) => CommandDef {
            brief: label_string(action, "Split Pane Top/Bottom".to_string()).into(),
            doc: "Split the current pane into top and bottom panes, by spawning \
            the default program into the bottom pane"
                .into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Left, amount) => CommandDef {
            brief: "Resize Split Left".into(),
            doc: format!("Move the current split divider left (step: {amount} cells)").into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::CTRL), "LeftArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Right, amount) => CommandDef {
            brief: "Resize Split Right".into(),
            doc: format!("Move the current split divider right (step: {amount} cells)").into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::CTRL), "RightArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Up, amount) => CommandDef {
            brief: "Resize Split Up".into(),
            doc: format!("Move the current split divider up (step: {amount} cells)").into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::CTRL), "UpArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Down, amount) => CommandDef {
            brief: "Resize Split Down".into(),
            doc: format!("Move the current split divider down (step: {amount} cells)").into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::CTRL), "DownArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AdjustPaneSize(PaneDirection::Next | PaneDirection::Prev, _) => return None,
        ActivatePaneDirection(PaneDirection::Next | PaneDirection::Prev) => return None,
        ActivatePaneDirection(PaneDirection::Left) => CommandDef {
            brief: "Activate Pane Left".into(),
            doc: "Activates the pane to the left of the current pane".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "LeftArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ActivatePaneDirection(PaneDirection::Right) => CommandDef {
            brief: "Activate Pane Right".into(),
            doc: "Activates the pane to the right of the current pane".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "RightArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ActivatePaneDirection(PaneDirection::Up) => CommandDef {
            brief: "Activate Pane Up".into(),
            doc: "Activates the pane to the top of the current pane".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "UpArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ActivatePaneDirection(PaneDirection::Down) => CommandDef {
            brief: "Activate Pane Down".into(),
            doc: "Activates the pane to the bottom of the current pane".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "DownArrow".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        TogglePaneZoomState => CommandDef {
            brief: "Zoom Pane".into(),
            doc: "Toggle pane zoom".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "Enter".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Window"],
            icon: None,
        },
        ActivateLastTab => CommandDef {
            brief: "Last Active Tab".into(),
            doc: "Switch to last active tab".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "t".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ToggleCurrentTabPanesInputBroadcast => CommandDef {
            brief: "Toggle Current Tab Panes".into(),
            doc: "Toggle broadcasting terminal keyboard input to every visible pane in the current tab.".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::ALT), "i".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ToggleAllPanesInputBroadcast => CommandDef {
            brief: "Toggle All Panes (All Tabs)".into(),
            doc: "Toggle broadcasting terminal keyboard input to every visible pane in every tab in the current window.".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "i".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        ClearKeyTableStack => CommandDef {
            brief: "Clear the key table stack".into(),
            doc: "Removes all entries from the stack".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        OpenLinkAtMouseCursor => CommandDef {
            brief: "Open link at mouse cursor".into(),
            doc: "If there is no link under the mouse cursor, has no effect.".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ShowLauncherArgs(_) | ShowLauncher => CommandDef {
            brief: "Launcher".into(),
            doc: "Open command launcher".into(),
            keys: vec![],
            args: &[ArgType::ActiveWindow],
            menubar: &[],
            icon: None,
        },
        ShowTabNavigator => CommandDef {
            brief: "Tab Navigator".into(),
            doc: "Interactive tab switcher".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "o".into())],
            args: &[ArgType::ActiveWindow],
            menubar: &["Window"],
            icon: None,
        },
        DetachDomain(SpawnTabDomain::CurrentPaneDomain) => CommandDef {
            brief: "Detach the domain of the active pane".into(),
            doc: "Detaches (disconnects from) the domain of the active pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        DetachDomain(SpawnTabDomain::DefaultDomain) => CommandDef {
            brief: "Detach the default domain".into(),
            doc: "Detaches (disconnects from) the default domain".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        DetachDomain(SpawnTabDomain::DomainName(name)) => CommandDef {
            brief: format!("Detach `{name}`").into(),
            doc: format!("Disconnect from `{name}` domain").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        DetachDomain(SpawnTabDomain::DomainId(id)) => CommandDef {
            brief: format!("Detach the domain with id {id}").into(),
            doc: format!("Detaches (disconnects from) the domain with id {id}").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        OpenUri(uri) => match uri.as_ref() {
            "https://github.com/tw93/Kaku" => CommandDef {
                brief: "Star on GitHub".into(),
                doc: "Star Kaku on GitHub".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: None,
            },
            "https://github.com/tw93/Kaku/discussions/" => CommandDef {
                brief: "Discuss on GitHub".into(),
                doc: "Visit Kaku's GitHub discussion".into(),
                keys: vec![],
                args: &[],
                menubar: &[],
                icon: None,
            },
            "https://github.com/tw93/Kaku/issues/" => CommandDef {
                brief: "Report Issue".into(),
                doc: "Submit bug report or feature request".into(),
                keys: vec![],
                args: &[],
                menubar: &["Help"],
                icon: None,
            },
            _ => CommandDef {
                brief: format!("Open {uri} in your browser").into(),
                doc: format!("Open {uri} in your browser").into(),
                keys: vec![],
                args: &[],
                menubar: &[],
                icon: None,
            },
        },
        SendString(text) => CommandDef {
            brief: format!(
                "Sends `{text}` to the active pane, \
                           as though you typed it"
            )
            .into(),
            doc: format!(
                "Sends `{text}` to the active pane, as \
                         though you typed it"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SendStringIfNotAltScreen(text) => CommandDef {
            brief: if text == "\x1f" {
                "Undo at the shell prompt".into()
            } else {
                format!("Sends `{text}` to the pane (normal screen only)").into()
            },
            doc: "Writes the given bytes to the pane only when the alt \
                  screen is inactive. Used to forward Cmd+Z to the shell's \
                  line-editor undo (zsh ZLE / bash readline) without \
                  interfering with full-screen TUI apps like vim."
                .into(),
            keys: if text == "\x1f" {
                #[cfg(target_os = "macos")]
                {
                    vec![(Modifiers::SUPER, "z".into())]
                }
                #[cfg(not(target_os = "macos"))]
                {
                    vec![]
                }
            } else {
                vec![]
            },
            args: &[],
            menubar: &[],
            icon: None,
        },
        SendKey(key) => CommandDef {
            brief: format!(
                "Sends {key:?} to the active pane, \
                           as though you typed it"
            )
            .into(),
            doc: format!(
                "Sends {key:?} to the active pane, \
                         as though you typed it"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        Nop => CommandDef {
            brief: "Does nothing".into(),
            doc: "Has no effect".into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        DisableDefaultAssignment => return None,
        SelectTextAtMouseCursor(mode) => CommandDef {
            brief: format!(
                "Selects text at the mouse cursor \
                           location using {mode:?}"
            )
            .into(),
            doc: format!(
                "Selects text at the mouse cursor \
                         location using {mode:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        ExtendSelectionToMouseCursor(mode) => CommandDef {
            brief: format!(
                "Extends the selection text to the mouse \
                           cursor location using {mode:?}"
            )
            .into(),
            doc: format!(
                "Extends the selection text to the mouse \
                         cursor location using {mode:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        ClearSelection => CommandDef {
            brief: "Clears the selection in the current pane".into(),
            doc: "Clears the selection in the current pane".into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        CompleteSelection(destination) => CommandDef {
            brief: format!("Completes selection; copies if copy_on_select is enabled").into(),
            doc: format!(
                "Completes text selection using the mouse. If \
                copy_on_select is enabled, copy to {destination:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        CompleteSelectionOrOpenLinkAtMouseCursor(destination) => CommandDef {
            brief: format!(
                "Open a URL or complete selection \
            with optional copy to {destination:?}"
            )
            .into(),
            doc: format!(
                "If the mouse is over a link, open it, otherwise, completes \
                text selection using the mouse. If copy_on_select is enabled, \
                copy to {destination:?}"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        StartWindowDrag => CommandDef {
            brief: "Requests a window drag operation from \
                the window environment"
                .into(),
            doc: "Requests a window drag operation from \
                the window environment"
                .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        Multiple(actions) => {
            let mut brief = String::new();
            for act in actions {
                if !brief.is_empty() {
                    brief.push_str(", ");
                }
                match derive_command_from_key_assignment(act) {
                    Some(cmd) => {
                        brief.push_str(&cmd.brief);
                    }
                    None => {
                        brief.push_str(&format!("{act:?}"));
                    }
                }
            }
            CommandDef {
                brief: brief.into(),
                doc: "Performs multiple nested actions".into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        SwitchToWorkspace {
            name: None,
            spawn: None,
        } => CommandDef {
            brief: format!(
                "Spawn the default program into a new \
                           workspace and switch to it"
            )
            .into(),
            doc: format!(
                "Spawn the default program into a new \
                         workspace and switch to it"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SwitchToWorkspace {
            name: Some(name),
            spawn: None,
        } => CommandDef {
            brief: format!(
                "Switch to workspace `{name}`, spawn the \
                           default program if that workspace doesn't already exist"
            )
            .into(),
            doc: format!(
                "Switch to workspace `{name}`, spawn the \
                         default program if that workspace doesn't already exist"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SwitchToWorkspace {
            name: Some(name),
            spawn: Some(prog),
        } => CommandDef {
            brief: format!(
                "Switch to workspace `{name}`, spawn {prog:?} \
                           if that workspace doesn't already exist"
            )
            .into(),
            doc: format!(
                "Switch to workspace `{name}`, spawn {prog:?} \
                         if that workspace doesn't already exist"
            )
            .into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SwitchToWorkspace {
            name: None,
            spawn: Some(prog),
        } => CommandDef {
            brief: format!("Spawn the {prog:?} into a new workspace and switch to it").into(),
            doc: format!("Spawn the {prog:?} into a new workspace and switch to it").into(),
            keys: vec![],
            args: &[],
            menubar: &[],
            icon: None,
        },
        SwitchWorkspaceRelative(n) => {
            let (direction, amount) = if *n < 0 {
                ("previous", -n)
            } else {
                ("next", *n)
            };
            let ordinal = english_ordinal(amount);
            CommandDef {
                brief: format!("Switch to {ordinal} {direction} workspace").into(),
                doc: format!(
                    "Switch to the {ordinal} {direction} workspace, \
                             ordered lexicographically by workspace name"
                )
                .into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: None,
            }
        }
        ActivateKeyTable { name, .. } => CommandDef {
            brief: format!("Activate key table `{name}`").into(),
            doc: format!("Activate key table `{name}`").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        PopKeyTable => CommandDef {
            brief: "Pop the current key table".into(),
            doc: "Pop the current key table".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        AttachDomain(name) => CommandDef {
            brief: format!("Attach domain `{name}`").into(),
            doc: format!("Attach domain `{name}`").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
        CopyMode(copy_mode) => CommandDef {
            brief: format!("{copy_mode:?}").into(),
            doc: "".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        RotatePanes(direction) => CommandDef {
            brief: format!("Rotate panes {direction:?}").into(),
            doc: format!("Rotate panes {direction:?}").into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        TogglePaneSplitDirection => CommandDef {
            brief: "Toggle Split Direction".into(),
            doc: "Toggle the split direction between horizontal and vertical".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "s".into())],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        SplitPane(split) => {
            let direction = split.direction;
            CommandDef {
                brief: label_string(action, format!("Split the current pane {direction:?}")).into(),
                doc: format!("Split the current pane {direction:?}").into(),
                keys: vec![],
                args: &[ArgType::ActivePane],
                menubar: &[],
                icon: match split.direction {
                    PaneDirection::Up | PaneDirection::Down => Some("cod_split_vertical"),
                    PaneDirection::Left | PaneDirection::Right => Some("cod_split_horizontal"),
                    PaneDirection::Next | PaneDirection::Prev => None,
                },
            }
        }
        ResetTerminal => CommandDef {
            brief: "Reset the terminal emulation state in the current pane".into(),
            doc: "Reset the terminal emulation state in the current pane".into(),
            keys: vec![],
            args: &[ArgType::ActivePane],
            menubar: &[],
            icon: None,
        },
        ActivateCommandPalette => CommandDef {
            brief: "Command Palette".into(),
            doc: "Open command palette".into(),
            keys: vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "p".into())],
            args: &[ArgType::ActivePane],
            menubar: &["Shell"],
            icon: None,
        },
    })
}

/// Returns a list of key assignment actions that should be
/// included in the default key assignments and command palette.
fn compute_default_actions() -> Vec<KeyAssignment> {
    // These are ordered by their position within the various menus
    let mut actions = vec![
        // ----------------- Kaku
        #[cfg(target_os = "macos")]
        HideApplication,
        #[cfg(target_os = "macos")]
        QuitApplication,
        // ----------------- Shell
        SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        SpawnWindow,
        EmitEvent("kaku-ai-chat".to_string()),
        EmitEvent("run-kaku-ai-config".to_string()),
        EmitEvent("kaku-launch-lazygit".to_string()),
        EmitEvent("kaku-launch-yazi".to_string()),
        EmitEvent("kaku-open-remote-files".to_string()),
        SplitVertical(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..Default::default()
        }),
        SplitHorizontal(SpawnCommand {
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..Default::default()
        }),
        CloseCurrentTab { confirm: true },
        CloseCurrentPane { confirm: true },
        ReopenLastClosedTab,
        RestorePreviousWindow,
        DetachDomain(SpawnTabDomain::CurrentPaneDomain),
        ResetTerminal,
        // ----------------- Edit
        #[cfg(not(target_os = "macos"))]
        PasteFrom(ClipboardPasteSource::PrimarySelection),
        #[cfg(not(target_os = "macos"))]
        CopyTo(ClipboardCopyDestination::PrimarySelection),
        CopyTo(ClipboardCopyDestination::Clipboard),
        PasteFrom(ClipboardPasteSource::Clipboard),
        // Cmd+Z at the shell prompt: forward Ctrl+_ (0x1f), which zsh (ZLE
        // `undo`) and bash (readline `undo`) interpret as line-editor undo.
        // Guarded against the alt screen so vim / less / htop / tmux keep
        // owning the keystroke.
        #[cfg(target_os = "macos")]
        SendStringIfNotAltScreen("\x1f".to_string()),
        ClearScrollback(ScrollbackEraseMode::ScrollbackOnly),
        ClearScrollback(ScrollbackEraseMode::ScrollbackAndViewport),
        QuickSelect,
        CharSelect(CharSelectArguments::default()),
        ActivateCopyMode,
        ClearKeyTableStack,
        ActivateCommandPalette,
        // ----------------- View
        DecreaseFontSize,
        IncreaseFontSize,
        ResetFontSize,
        ResetFontAndWindowSize,
        ScrollByPage(NotNan::new(-1.0).unwrap()),
        ScrollByPage(NotNan::new(1.0).unwrap()),
        ScrollToTop,
        ScrollToBottom,
        // ----------------- Window
        ToggleFullScreen,
        CenterWindow,
        MaximizeWindow,
        Hide,
        ToggleAlwaysOnTop,
        Search(Pattern::CurrentSelectionOrEmptyString),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::Activate,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::SwapWithActive,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::SwapWithActiveKeepFocus,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::MoveToNewTab,
            show_pane_ids: false,
        }),
        PaneSelect(PaneSelectArguments {
            alphabet: String::new(),
            mode: PaneSelectMode::MoveToNewWindow,
            show_pane_ids: false,
        }),
        RotatePanes(RotationDirection::Clockwise),
        RotatePanes(RotationDirection::CounterClockwise),
        TogglePaneSplitDirection,
        ActivateTab(0),
        ActivateTab(1),
        ActivateTab(2),
        ActivateTab(3),
        ActivateTab(4),
        ActivateTab(5),
        ActivateTab(6),
        ActivateTab(7),
        ActivateTab(-1),
        ActivateTabRelative(-1),
        ActivateTabRelative(1),
        ActivateWindowRelative(-1),
        ActivateWindowRelative(1),
        MoveTabRelative(-1),
        MoveTabRelative(1),
        AdjustPaneSize(PaneDirection::Left, 5),
        AdjustPaneSize(PaneDirection::Right, 5),
        AdjustPaneSize(PaneDirection::Up, 5),
        AdjustPaneSize(PaneDirection::Down, 5),
        ActivatePaneDirection(PaneDirection::Left),
        ActivatePaneDirection(PaneDirection::Right),
        ActivatePaneDirection(PaneDirection::Up),
        ActivatePaneDirection(PaneDirection::Down),
        TogglePaneZoomState,
        ActivateLastTab,
        ShowTabNavigator,
        ToggleCurrentTabPanesInputBroadcast,
        ToggleAllPanesInputBroadcast,
        // ----------------- Help
        OpenUri("https://github.com/tw93/Kaku".to_string()),
        OpenUri("https://github.com/tw93/Kaku/issues/".to_string()),
        ShowDebugOverlay,
        // ----------------- Misc
        OpenLinkAtMouseCursor,
    ];

    actions.extend(
        PaneEncoding::ordered_list()
            .into_iter()
            .map(KeyAssignment::SetPaneEncoding),
    );

    actions
}

#[cfg(test)]
mod tests {
    use super::{derive_command_from_key_assignment, CommandDef};
    use config::keyassignment::KeyAssignment;
    use config::ConfigHandle;
    use window::Modifiers;

    #[test]
    fn toggle_current_tab_panes_input_broadcast_has_default_shortcut() {
        let cmd =
            derive_command_from_key_assignment(&KeyAssignment::ToggleCurrentTabPanesInputBroadcast)
                .expect("command");

        assert_eq!(
            cmd.keys,
            vec![(Modifiers::SUPER.union(Modifiers::ALT), "i".into())]
        );
    }

    #[test]
    fn toggle_all_panes_input_broadcast_has_default_shortcut() {
        let cmd = derive_command_from_key_assignment(&KeyAssignment::ToggleAllPanesInputBroadcast)
            .expect("command");

        assert_eq!(
            cmd.keys,
            vec![(Modifiers::SUPER.union(Modifiers::SHIFT), "i".into())]
        );
    }

    #[test]
    fn toggle_current_tab_panes_input_broadcast_is_in_default_assignments() {
        let config = ConfigHandle::default_config();

        assert!(CommandDef::default_key_assignments(&config)
            .iter()
            .any(|(_, _, action)| *action == KeyAssignment::ToggleCurrentTabPanesInputBroadcast));
    }

    #[test]
    fn toggle_all_panes_input_broadcast_is_in_default_assignments() {
        let config = ConfigHandle::default_config();

        assert!(CommandDef::default_key_assignments(&config)
            .iter()
            .any(|(_, _, action)| *action == KeyAssignment::ToggleAllPanesInputBroadcast));
    }

    #[test]
    fn restore_previous_window_has_default_shortcut() {
        let cmd = derive_command_from_key_assignment(&KeyAssignment::RestorePreviousWindow)
            .expect("command");

        assert_eq!(cmd.brief, "Restore Previous Window");
        assert_eq!(
            cmd.keys,
            vec![(
                Modifiers::SUPER
                    .union(Modifiers::ALT)
                    .union(Modifiers::SHIFT),
                "t".into()
            )]
        );
        assert_eq!(cmd.menubar, &["Shell"]);
    }

    #[test]
    fn restore_previous_window_is_in_command_palette() {
        let config = ConfigHandle::default_config();

        assert!(CommandDef::actions_for_palette_only(&config)
            .iter()
            .any(|cmd| cmd.action == KeyAssignment::RestorePreviousWindow));
    }
}
