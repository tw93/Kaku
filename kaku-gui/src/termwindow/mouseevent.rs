use crate::tabbar::TabBarItem;
use crate::termwindow::tab_rename::TabRenameModal;
use crate::termwindow::{
    GuiWin, MouseCapture, PositionedSplit, ScrollHit, TermWindowNotif, UIItem, UIItemType, TMB,
};
use ::window::{
    MouseButtons as WMB, MouseCursor, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps,
    WindowState,
};
use config::keyassignment::{
    ClipboardCopyDestination, ClipboardPasteSource, KeyAssignment, MouseEventTrigger, Pattern,
    SpawnTabDomain,
};
use config::{MouseEventAltScreen, SelectionWheelScrollBehavior};
use mux::pane::{CachePolicy, Pane, WithPaneLines};
use mux::tab::SplitDirection;
use mux::Mux;
use mux_lua::MuxPane;
use std::convert::TryInto;
use std::ops::Sub;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::Line;
use wezterm_dynamic::ToDynamic;
use wezterm_term::input::{MouseButton, MouseEventKind as TMEK};
use wezterm_term::{ClickPosition, KeyCode, KeyModifiers, LastMouseClick, StableRowIndex};
#[cfg(target_os = "macos")]
use window::os::macos::menu::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseDispatchTarget {
    Ui,
    TitleArea,
    Terminal,
}

fn should_show_terminal_context_menu(
    event_kind: &WMEK,
    modifiers: window::Modifiers,
    bypass_modifiers: window::Modifiers,
    mouse_grabbed: bool,
    is_already_captured: bool,
    has_mouse_assignment: bool,
) -> bool {
    matches!(event_kind, WMEK::Press(MousePress::Right))
        && !is_already_captured
        && !has_mouse_assignment
        && (!mouse_grabbed || modifiers.contains(bypass_modifiers))
}

fn terminal_context_menu_actions(has_selection: bool) -> Vec<(&'static str, KeyAssignment)> {
    let split = |direction| {
        KeyAssignment::SplitPane(config::keyassignment::SplitPane {
            direction,
            size: Default::default(),
            command: Default::default(),
            top_level: false,
        })
    };

    let mut actions = Vec::new();
    // Copy is offered only with a live selection, so users who turn off
    // copy_on_select still have a mouse path to the clipboard (#558).
    if has_selection {
        actions.push((
            "Copy",
            KeyAssignment::CopyTo(ClipboardCopyDestination::Clipboard),
        ));
    }
    actions.extend([
        (
            "Paste",
            KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard),
        ),
        ("Search", KeyAssignment::Search(Pattern::default())),
        (
            "AI Chat",
            KeyAssignment::EmitEvent("kaku-ai-chat".to_string()),
        ),
        ("Command Palette", KeyAssignment::ActivateCommandPalette),
        (
            "Split Left",
            split(config::keyassignment::PaneDirection::Left),
        ),
        (
            "Split Right",
            split(config::keyassignment::PaneDirection::Right),
        ),
        ("Split Up", split(config::keyassignment::PaneDirection::Up)),
        (
            "Split Down",
            split(config::keyassignment::PaneDirection::Down),
        ),
        (
            "New Tab",
            KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        ),
        (
            "Close Pane",
            KeyAssignment::CloseCurrentPane { confirm: true },
        ),
        (
            "Close Tab",
            KeyAssignment::CloseCurrentTab { confirm: true },
        ),
    ]);
    actions
}

/// Right-click menu for a tab in the tab bar. Closing a tab otherwise needs a
/// middle button or the keyboard, which pointer-only setups such as touch over
/// remote desktop do not have.
fn tab_context_menu_actions() -> [(&'static str, KeyAssignment); 3] {
    [
        (
            "New Tab",
            KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        ),
        ("Tab Navigator", KeyAssignment::ShowTabNavigator),
        (
            "Close Tab",
            KeyAssignment::CloseCurrentTab { confirm: true },
        ),
    ]
}

/// Context-menu items deliberately have no AppKit keyEquivalent. Kaku's
/// configurable InputMap remains the shortcut source of truth.
#[cfg(target_os = "macos")]
fn pane_action_menu_item(title: &str, action: KeyAssignment, pane_id: usize) -> MenuItem {
    let item = MenuItem::new_with(title, Some(sel!(kakuPerformKeyAssignment:)), "");
    item.set_represented_item(RepresentedItem::KeyAssignmentForPane { action, pane_id });
    item
}

/// Per-window state describing in-flight window-level drag interactions
/// (manual title-bar drag, OS edge resize suppression, click-to-focus
/// suppression). Bundled to keep the drag invariants in one place.
#[derive(Default)]
pub(crate) struct WindowDragState {
    /// Latest mouse event captured when we began driving a manual window
    /// drag via the title bar. None means no drag in progress.
    pub position: Option<MouseEvent>,
    /// Set when a mouse press occurs near the window edge (resize zone).
    /// Suppresses subsequent Move/Release events to prevent unwanted
    /// text selection in TUI applications during OS-level window resize.
    pub edge_drag_in_progress: bool,
    /// True while the manual title-bar drag is active.
    pub is_window_dragging: bool,
    /// True for the duration of the click-press that brought this window
    /// into focus, so we can ignore the corresponding release.
    pub is_click_to_focus: bool,
}

/// Per-window mouse input state: pointer position, held buttons, capture,
/// click counting, and UI hover tracking. Bundled to keep the `TermWindow`
/// field list shorter.
pub(crate) struct MouseInputState {
    pub last_ui_item: Option<UIItem>,
    pub last_mouse_coords: (usize, i64),
    pub current_mouse_event: Option<MouseEvent>,
    pub current_mouse_buttons: Vec<MousePress>,
    pub current_mouse_capture: Option<MouseCapture>,
    /// True while the held left button is driving a terminal text selection,
    /// i.e. the press/drag actually resolved to a SelectTextAtMouseCursor or
    /// ExtendSelectionToMouseCursor assignment. A left press forwarded to a
    /// mouse-reporting application (claude code, vim, tmux with mouse on)
    /// leaves this false so wheel events are not hijacked into
    /// selection-extension (#455).
    pub selection_drag_active: bool,
    /// Keeps track of double and triple clicks
    pub last_mouse_click: Option<LastMouseClick>,
    /// Button of a press that reached the tab bar through the touch band
    /// above it. The rest of that gesture belongs to the tab bar, so its
    /// motion and release must not fall through to terminal selection.
    pub touch_band_press: Option<MousePress>,
}

impl Default for MouseInputState {
    fn default() -> Self {
        Self {
            last_ui_item: None,
            last_mouse_coords: (0, -1),
            current_mouse_event: None,
            current_mouse_buttons: vec![],
            current_mouse_capture: None,
            selection_drag_active: false,
            last_mouse_click: None,
            touch_band_press: None,
        }
    }
}

fn mouse_dispatch_target(
    has_ui_item: bool,
    coords_y: isize,
    terminal_origin_y: isize,
    capture: Option<&super::MouseCapture>,
) -> MouseDispatchTarget {
    if matches!(capture, Some(super::MouseCapture::TerminalPane(_))) {
        MouseDispatchTarget::Terminal
    } else if has_ui_item {
        MouseDispatchTarget::Ui
    } else if coords_y < terminal_origin_y {
        MouseDispatchTarget::TitleArea
    } else {
        MouseDispatchTarget::Terminal
    }
}

fn should_zoom_title_area(window_decorations: window::WindowDecorations, click_count: u8) -> bool {
    window_decorations
        == (window::WindowDecorations::INTEGRATED_BUTTONS | window::WindowDecorations::RESIZE)
        && click_count == 2
}

fn tab_bar_item_starts_window_drag(item: TabBarItem) -> bool {
    matches!(
        item,
        TabBarItem::None | TabBarItem::LeftStatus | TabBarItem::RightStatus
    )
}

/// How far a press may land above a bottom tab bar and still hit a tab. The
/// retro bar is one glyph row (about 22pt at 2x), which is hard to hit with a
/// finger over remote desktop; this keeps the visuals and widens the target
/// into the bottom of the last terminal row.
const BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT: usize = 12;

/// Maps a press just above a bottom tab bar onto the bar's top edge. The band
/// covers the padding strip between the last terminal row and the bar, plus
/// `extension` pixels of the last row itself.
fn bottom_tab_bar_touch_y(
    y: isize,
    bar_top: isize,
    terminal_bottom: isize,
    extension: isize,
) -> Option<isize> {
    let band_top = bar_top.min(terminal_bottom) - extension;
    (y >= band_top && y < bar_top).then_some(bar_top)
}

/// Only a plain left press (a tap) is widened. A middle press there is a
/// paste into the last row and a right press is its context menu; neither
/// may turn into closing or opening a tab.
fn press_takes_touch_extension(kind: &WMEK, modifiers: window::Modifiers) -> bool {
    matches!(kind, WMEK::Press(MousePress::Left)) && modifiers.remove_positional_mods().is_empty()
}

/// Only tabs and the new-tab button take the widened target. Status and
/// empty bar regions start a window drag, which must never begin from inside
/// the terminal.
fn tab_bar_item_takes_touch_extension(item: &UIItemType) -> bool {
    matches!(
        item,
        UIItemType::TabBar(TabBarItem::Tab { .. } | TabBarItem::NewTabButton)
    )
}

fn should_use_manual_window_drag(_window_state: WindowState) -> bool {
    // AppKit owns edge tiling and drag tracking. Other platforms already used
    // their native drag path; changing macOS must not enable manual drag there.
    false
}

fn should_use_native_maximized_window_drag(window_state: WindowState) -> bool {
    cfg!(target_os = "macos")
        && window_state.contains(WindowState::MAXIMIZED)
        && !window_state.contains(WindowState::FULL_SCREEN)
}

/// New window top-left for a manual title-bar drag: the window origin captured
/// by the platform at the anchor event, shifted by the screen-space mouse
/// delta. Both inputs live in `screen_coords` space, so this stays correct
/// when the window is on a display whose backing scale differs from the
/// primary display's (#456). `event.coords` must not participate here: it is
/// in the window's own backing-pixel scale.
fn manual_drag_window_top_left(start: &MouseEvent, event: &MouseEvent) -> ::window::ScreenPoint {
    ::window::ScreenPoint::new(
        start.window_origin.x + (event.screen_coords.x - start.screen_coords.x),
        start.window_origin.y + (event.screen_coords.y - start.screen_coords.y),
    )
}

#[derive(Default)]
struct OptionClickRowInfo {
    wrapped: bool,
    cells: Vec<(usize, usize)>,
}

/// Arrow sequence for a cursor move that is provably confined to one shell
/// editing line. Hard-newline rows are ambiguous scrollback: emitting Up/Down
/// there can mutate shell history rather than position the active prompt.
fn option_click_cursor_bytes(
    rows: &[OptionClickRowInfo],
    cursor_row: usize,
    cursor_col: usize,
    target_row: usize,
    target_col: usize,
    application_cursor_keys: bool,
) -> Vec<u8> {
    if rows.is_empty() || cursor_row >= rows.len() || target_row >= rows.len() {
        return Vec::new();
    }

    let (from, to) = if (cursor_row, cursor_col) <= (target_row, target_col) {
        ((cursor_row, cursor_col), (target_row, target_col))
    } else {
        ((target_row, target_col), (cursor_row, cursor_col))
    };
    let same_row = from.0 == to.0;
    let soft_wrapped_chain = rows[from.0..to.0].iter().all(|row| row.wrapped);
    if !same_row && !soft_wrapped_chain {
        return Vec::new();
    }

    let mut count = 0usize;
    for (row_index, info) in rows.iter().enumerate().take(to.0 + 1).skip(from.0) {
        for &(cell_index, width) in &info.cells {
            let after_start = row_index > from.0 || cell_index + width > from.1;
            let before_end = row_index < to.0 || cell_index < to.1;
            if after_start && before_end {
                count += 1;
            }
        }
    }

    let moving_right = (target_row, target_col) > (cursor_row, cursor_col);
    // DECCKM (application cursor keys) changes the arrow encoding; inline
    // raw-mode TUIs that enabled it expect SS3-style arrows.
    let arrow: &[u8] = match (moving_right, application_cursor_keys) {
        (true, false) => b"\x1b[C",
        (false, false) => b"\x1b[D",
        (true, true) => b"\x1bOC",
        (false, true) => b"\x1bOD",
    };
    arrow.repeat(count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleAreaZoomAction {
    Maximize,
    Restore,
}

fn title_area_double_click_zoom_action(window_state: WindowState) -> TitleAreaZoomAction {
    if window_state.contains(WindowState::FULL_SCREEN) {
        return TitleAreaZoomAction::Restore;
    }

    if cfg!(target_os = "macos") {
        // NSWindow::zoom: toggles using AppKit's actual zoom state. That is more
        // reliable than Kaku's cached WindowState after cross-screen/multi-window moves.
        return TitleAreaZoomAction::Maximize;
    }

    if window_state.contains(WindowState::MAXIMIZED) {
        TitleAreaZoomAction::Restore
    } else {
        TitleAreaZoomAction::Maximize
    }
}

fn should_preserve_tmux_bypass_reporting(
    is_wheel_event: bool,
    modifiers: window::Modifiers,
    bypass_modifiers: window::Modifiers,
    alt_screen: bool,
    mouse_grabbed: bool,
    in_tmux_process_tree: bool,
) -> bool {
    is_wheel_event
        && alt_screen
        && mouse_grabbed
        && in_tmux_process_tree
        && modifiers.contains(bypass_modifiers)
}

fn should_bypass_wheel_assignment_in_alt(
    is_wheel_event: bool,
    alt_screen: bool,
    mouse_grabbed: bool,
    alternate_screen_wheel_scrolls_terminal: bool,
) -> bool {
    is_wheel_event && alt_screen && !mouse_grabbed && !alternate_screen_wheel_scrolls_terminal
}

/// Action to take when a wheel event arrives in the middle of a left-button
/// terminal selection drag.
///
/// Returned by [`wheel_during_terminal_selection_action`]. `None` means
/// "the wheel event is not happening during a terminal selection drag, let
/// the caller route it through the normal path".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionDragWheelAction {
    /// Drop the wheel event (legacy behavior, configured by
    /// `SelectionWheelScrollBehavior::Ignore`).
    Suppress,
    /// Scroll the viewport but do not touch the selection range.
    ScrollOnly,
    /// Scroll the viewport AND stretch the selection to follow the cursor.
    ScrollAndExtend,
}

/// Detect whether the current wheel event is happening inside a left-button
/// terminal selection drag, and if so return the action that the configured
/// [`SelectionWheelScrollBehavior`] resolves to.
///
/// Returning `None` is the "not in a selection drag" signal; callers should
/// continue through the normal wheel routing path.
///
/// `selection_drag_active` must come from the binding lookup: a left press in
/// a mouse-reporting pane is forwarded to the application without starting a
/// selection, and its wheel events must keep flowing to the application
/// instead of conjuring a selection out of thin air (#455).
fn wheel_during_terminal_selection_action(
    capture: Option<&super::MouseCapture>,
    current_mouse_buttons: &[MousePress],
    mouse_buttons: WMB,
    selection_drag_active: bool,
    behavior: SelectionWheelScrollBehavior,
) -> Option<SelectionDragWheelAction> {
    let is_terminal_selection_drag = selection_drag_active
        && matches!(capture, Some(super::MouseCapture::TerminalPane(_)))
        && (current_mouse_buttons.contains(&MousePress::Left) || mouse_buttons == WMB::LEFT);

    if !is_terminal_selection_drag {
        return None;
    }

    Some(match behavior {
        SelectionWheelScrollBehavior::Ignore => SelectionDragWheelAction::Suppress,
        SelectionWheelScrollBehavior::ScrollOnly => SelectionDragWheelAction::ScrollOnly,
        SelectionWheelScrollBehavior::Extend => SelectionDragWheelAction::ScrollAndExtend,
    })
}

impl super::TermWindow {
    const TAB_DRAG_THRESHOLD: isize = 6;

    fn finish_mouse_release(&mut self, press: MousePress) {
        self.mouse.current_mouse_capture = None;
        self.mouse.current_mouse_buttons.retain(|p| p != &press);
        if press == MousePress::Left {
            self.mouse.selection_drag_active = false;
        }
    }

    /// Handle a wheel event that arrived while a left-button terminal
    /// selection drag is in progress.
    ///
    /// The dispatcher in `mouse_event_impl` only forwards us here for the
    /// three behaviors that need special-casing (`Suppress`, `ScrollOnly`,
    /// `ScrollAndExtend`). Everything else flows through the normal wheel
    /// routing path unchanged.
    fn handle_wheel_during_terminal_selection(
        &mut self,
        action: SelectionDragWheelAction,
        _event: &MouseEvent,
        pane: &Arc<dyn Pane>,
        context: &dyn WindowOps,
    ) {
        match action {
            SelectionDragWheelAction::Suppress => {
                log::trace!(
                    "selection_wheel_scroll_behavior=Ignore, \
                     dropping wheel during selection drag"
                );
            }
            SelectionDragWheelAction::ScrollOnly => {
                if let Err(err) = self.scroll_by_current_event_wheel_delta(pane) {
                    log::debug!(
                        "scroll_by_current_event_wheel_delta failed during \
                         selection drag (ScrollOnly): {err:#}"
                    );
                }
                context.invalidate();
            }
            SelectionDragWheelAction::ScrollAndExtend => {
                if let Err(err) = self.scroll_by_current_event_wheel_delta(pane) {
                    log::debug!(
                        "scroll_by_current_event_wheel_delta failed during \
                         selection drag (ScrollAndExtend): {err:#}"
                    );
                }

                // The viewport just moved. The same physical mouse row now
                // points at a different StableRowIndex, so recompute it and
                // refresh `pane_state.mouse_terminal_coords` so that the
                // selection-extension helper picks up the post-scroll target.
                let (_, mouse_screen_row) = self.mouse.last_mouse_coords;
                let dims = pane.get_dimensions();
                let new_stable_row = self.effective_viewport(pane).unwrap_or(dims.physical_top)
                    + mouse_screen_row as StableRowIndex;

                {
                    let mut state = self.pane_state(pane.pane_id());
                    if let Some((click_pos, _)) = state.mouse_terminal_coords.clone() {
                        state.mouse_terminal_coords = Some((click_pos, new_stable_row));
                    }
                }

                self.extend_selection_at_mouse_cursor(crate::selection::SelectionMode::Cell, pane);
                context.invalidate();
            }
        }
    }

    fn start_tab_drag(&mut self, tab_idx: usize, start_event: MouseEvent) {
        self.tab_drag_state = Some(super::TabDragState {
            tab_idx,
            start_event,
            has_dragged: false,
            drag_offset_x: 0.0,
        });
    }

    fn last_tab_index(&self) -> Option<usize> {
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        let len = window.len();
        (len > 0).then_some(len - 1)
    }

    fn tab_ui_item(&self, tab_idx: usize) -> Option<UIItem> {
        self.ui_items.iter().find_map(|item| match item.item_type {
            UIItemType::TabBar(TabBarItem::Tab {
                tab_idx: item_tab_idx,
                ..
            }) if item_tab_idx == tab_idx => Some(item.clone()),
            _ => None,
        })
    }

    fn drag_tab_target_idx(&self, current_tab_idx: usize, cursor_x: isize) -> Option<usize> {
        if let Some(prev_idx) = current_tab_idx.checked_sub(1) {
            if let Some(prev) = self.tab_ui_item(prev_idx) {
                let prev_mid_x = prev.x as isize + prev.width as isize / 2;
                if cursor_x < prev_mid_x {
                    return Some(prev_idx);
                }
            }
        }

        if current_tab_idx < self.last_tab_index()? {
            if let Some(next) = self.tab_ui_item(current_tab_idx + 1) {
                let next_mid_x = next.x as isize + next.width as isize / 2;
                if cursor_x > next_mid_x {
                    return Some(current_tab_idx + 1);
                }
            }
        }

        None
    }

    fn begin_tab_rename(&mut self, tab_idx: usize, item: UIItem) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow::anyhow!("no such window"))?;
        let tab = window
            .get_by_idx(tab_idx)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no such tab index"))?;
        drop(window);

        let modal = TabRenameModal::new(self, tab.tab_id(), item)?;
        self.set_modal(Rc::new(modal));
        Ok(())
    }

    fn drag_tab(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(mut state) = self.tab_drag_state.take() else {
            return false;
        };

        if event.mouse_buttons != WMB::LEFT {
            self.tab_drag_state = Some(state);
            return false;
        }

        let delta_x = (event.coords.x - state.start_event.coords.x).abs();
        let delta_y = (event.coords.y - state.start_event.coords.y).abs();
        if !state.has_dragged && delta_x.max(delta_y) < Self::TAB_DRAG_THRESHOLD {
            self.tab_drag_state = Some(state);
            return true;
        }

        state.has_dragged = true;
        context.set_cursor(Some(MouseCursor::Grabbing));

        // Update drag offset for real-time visual feedback
        state.drag_offset_x = (event.coords.x - state.start_event.coords.x) as f32;

        let target_idx = self.drag_tab_target_idx(state.tab_idx, event.coords.x);

        if let Some(target_idx) = target_idx {
            if target_idx != state.tab_idx {
                if let Err(err) = self.move_tab(target_idx) {
                    log::debug!("move_tab({target_idx}) failed while dragging tab: {err:#}");
                } else {
                    // Trigger neighbor animation
                    self.start_tab_swap_animation(state.tab_idx, target_idx);

                    state.tab_idx = target_idx;
                    // Adjust start_event.coords.x so drag_offset_x is relative to new position
                    if let Some(new_item) = self.tab_ui_item(target_idx) {
                        state.start_event.coords.x = new_item.x as isize;
                    }
                    context.invalidate();
                }
            }
        }

        // Invalidate even without swap to update dragged tab position
        context.invalidate();

        self.tab_drag_state = Some(state);
        true
    }

    fn start_tab_swap_animation(&mut self, old_idx: usize, new_idx: usize) {
        use crate::colorease::ColorEase;
        use config::EasingFunction;
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::time::Instant;

        let Some(old_item) = self.tab_ui_item(old_idx) else {
            return;
        };
        let Some(new_item) = self.tab_ui_item(new_idx) else {
            return;
        };

        let swap_distance = (new_item.x as f32 - old_item.x as f32).abs();

        // After move_tab(), the displaced neighbor sits at old_idx. To make it
        // appear to slide from its old position (new_idx) toward old_idx:
        // - dragged right (new_idx > old_idx): neighbor came from the right, so
        //   initial offset is positive (starts to the right of its new slot).
        // - dragged left (new_idx < old_idx): neighbor came from the left, so
        //   initial offset is negative (starts to the left of its new slot).
        let start_offset = if new_idx > old_idx {
            swap_distance
        } else {
            -swap_distance
        };

        let ease = Rc::new(RefCell::new(ColorEase::new(
            150,
            EasingFunction::EaseOut,
            0,
            EasingFunction::Linear,
            Some(Instant::now()),
        )));

        // Animate the neighbor, which now lives at old_idx.
        self.tab_position_animations
            .insert(old_idx, (start_offset, ease));
    }

    fn start_tab_settle_animation(&mut self, tab_idx: usize, current_offset: f32) {
        use crate::colorease::ColorEase;
        use config::EasingFunction;
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::time::Instant;

        if current_offset.abs() < 1.0 {
            return;
        }

        let ease = Rc::new(RefCell::new(ColorEase::new(
            120,
            EasingFunction::EaseOut,
            0,
            EasingFunction::Linear,
            Some(Instant::now()),
        )));

        // Animate from current_offset back to 0.
        // start_offset is the actual drag position; the animation interpolates
        // start_offset * intensity → 0, so the tab springs back from where it is.
        self.tab_position_animations
            .insert(tab_idx, (current_offset, ease));
    }

    fn resolve_ui_item(&self, event: &MouseEvent) -> Option<UIItem> {
        let x = event.coords.x;
        let y = event.coords.y;
        self.ui_items
            .iter()
            .rev()
            .find(|item| item.hit_test(x, y))
            .cloned()
    }

    /// Resolves a plain left press (a tap) just above a bottom tab bar to the
    /// tab under it. Middle and right presses stay with the terminal, so a
    /// middle-click paste on the last row can never close a tab. Hover, wheel,
    /// modified clicks and gestures already owned by the terminal keep their
    /// normal routing.
    fn resolve_bottom_tab_bar_touch_item(
        &self,
        event: &MouseEvent,
        terminal_bottom: isize,
    ) -> Option<UIItem> {
        if !self.show_tab_bar
            || !self.config.tab_bar_at_bottom
            || !press_takes_touch_extension(&event.kind, event.modifiers)
            || self.mouse.current_mouse_capture.is_some()
        {
            return None;
        }
        let bar_top = self
            .ui_items
            .iter()
            .filter(|item| matches!(item.item_type, UIItemType::TabBar(_)))
            .map(|item| item.y)
            .min()? as isize;
        #[cfg(target_os = "macos")]
        let base_dpi: usize = 72;
        #[cfg(not(target_os = "macos"))]
        let base_dpi: usize = 96;
        let extension = (BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT * self.dimensions.dpi / base_dpi)
            .max(BOTTOM_TAB_BAR_TOUCH_EXTENSION_PT) as isize;
        let y = bottom_tab_bar_touch_y(event.coords.y, bar_top, terminal_bottom, extension)?;
        self.ui_items
            .iter()
            .rev()
            .find(|item| {
                // Probe at each item's own top: fancy tabs sit below the bar top.
                tab_bar_item_takes_touch_extension(&item.item_type)
                    && item.hit_test(event.coords.x, y.max(item.y as isize))
            })
            .cloned()
    }

    fn leave_ui_item(&mut self, item: &UIItem) {
        match item.item_type {
            UIItemType::TabBar(_) => {
                self.update_title_post_status();
            }
            UIItemType::CloseTab(_)
            | UIItemType::AboveScrollThumb
            | UIItemType::BelowScrollThumb
            | UIItemType::ScrollThumb
            | UIItemType::Split(_) => {}
        }
    }

    fn enter_ui_item(&mut self, item: &UIItem) {
        match item.item_type {
            UIItemType::TabBar(_) => {}
            UIItemType::CloseTab(_)
            | UIItemType::AboveScrollThumb
            | UIItemType::BelowScrollThumb
            | UIItemType::ScrollThumb
            | UIItemType::Split(_) => {}
        }
    }

    pub fn mouse_event_impl(&mut self, event: MouseEvent, context: &dyn WindowOps) {
        log::trace!("{:?}", event);
        if self.config.debug_key_events && matches!(event.kind, WMEK::Press(_) | WMEK::Release(_)) {
            // Remote desktop clients map touch gestures to clicks in their own
            // ways (a long press may arrive as Ctrl+click or a plain press), so
            // log what actually arrives alongside the key events.
            log::info!(
                "mouse_event {:?} mods={:?} buttons={:?} clicks={}",
                event.kind,
                event.modifiers,
                event.mouse_buttons,
                event.platform_click_count
            );
        }
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        self.mouse.current_mouse_event.replace(event.clone());
        self.update_scrollbar_hovering(&pane, context);
        // Mouse interaction should cancel any synthetic prompt-selection state
        // tracked from keyboard shortcuts (Cmd+A/Shift+Arrow, etc).
        self.clear_line_editor_selection();

        if matches!(event.kind, WMEK::VertWheel(_) | WMEK::HorzWheel(_)) {
            if let Some(action) = wheel_during_terminal_selection_action(
                self.mouse.current_mouse_capture.as_ref(),
                &self.mouse.current_mouse_buttons,
                event.mouse_buttons,
                self.mouse.selection_drag_active,
                self.config.selection_wheel_scroll_behavior,
            ) {
                self.handle_wheel_during_terminal_selection(action, &event, &pane, context);
                return;
            }
        }

        let border = self.get_os_border();

        let first_line_offset = self.terminal_first_row_offset() as isize;

        let (padding_left, padding_top) = self.padding_left_top();
        let terminal_origin_y = first_line_offset + padding_top as isize;

        let y = (event
            .coords
            .y
            .sub(padding_top as isize)
            .sub(first_line_offset)
            .max(0)
            / self.render_metrics.cell_size.height) as i64;

        let x = (event
            .coords
            .x
            .sub((padding_left + border.left.get() as f32) as isize)
            .max(0) as f32)
            / self.render_metrics.cell_size.width as f32;
        let x = if !pane.is_mouse_grabbed() {
            // Round the x coordinate so that we're a bit more forgiving of
            // the horizontal position when selecting cells
            x.round()
        } else {
            x
        }
        .trunc() as usize;

        let mut y_pixel_offset = event
            .coords
            .y
            .sub(padding_top as isize)
            .sub(first_line_offset);
        if y > 0 {
            y_pixel_offset = y_pixel_offset.max(0) % self.render_metrics.cell_size.height;
        }

        let mut x_pixel_offset = event
            .coords
            .x
            .sub((padding_left + border.left.get() as f32) as isize);
        if x > 0 {
            x_pixel_offset = x_pixel_offset.max(0) % self.render_metrics.cell_size.width;
        }

        self.mouse.last_mouse_coords = (x, y);

        // Keep modal focus exclusive: forward all mouse events to it and stop
        // routing into pane/tab UI while active.
        if let Some(modal) = self.get_modal() {
            if let WMEK::Release(press) = &event.kind {
                self.finish_mouse_release(*press);
            }

            let (kind, button) = wmek_to_tmek_and_button(&event);
            let modal_event = wezterm_term::MouseEvent {
                kind,
                button,
                x,
                y,
                x_pixel_offset,
                y_pixel_offset,
                modifiers: event.modifiers,
            };
            if let Err(err) = modal.mouse_event(modal_event, self) {
                log::error!("modal mouse event: {err:#}");
            }
            return;
        }

        let mut capture_mouse = false;
        let release_button = match &event.kind {
            WMEK::Release(press) => Some(*press),
            _ => None,
        };

        match event.kind {
            WMEK::Release(ref press) => {
                let ends_touch_band_press = self.mouse.touch_band_press == Some(*press);
                if ends_touch_band_press {
                    self.mouse.touch_band_press = None;
                }
                if press == &MousePress::Left && self.window_drag.edge_drag_in_progress {
                    self.window_drag.edge_drag_in_progress = false;
                    self.finish_mouse_release(*press);
                    return;
                }
                if press == &MousePress::Left {
                    let was_dragging_window = self.window_drag.is_window_dragging;
                    self.window_drag.is_window_dragging = false;
                    let had_manual_drag_anchor = self.window_drag.position.take().is_some();
                    if had_manual_drag_anchor || was_dragging_window {
                        if let Some(state) = self.tab_drag_state.take() {
                            if state.has_dragged {
                                self.start_tab_settle_animation(state.tab_idx, state.drag_offset_x);
                            }
                        }
                        // Completed a window drag
                        self.finish_mouse_release(*press);
                        return;
                    }
                }
                if press == &MousePress::Left && self.dragging.take().is_some() {
                    // Completed a split drag: notify PTY of final sizes
                    // using the tab_id captured at drag start.
                    if let Some(state) = self.split_drag_state.take() {
                        let mux = Mux::get();
                        if let Some(tab) = mux.get_tab(state.tab_id) {
                            tab.flush_pane_pty_sizes();
                            context.invalidate();
                        }
                    }
                    self.finish_mouse_release(*press);
                    return;
                }
                if press == &MousePress::Left {
                    if let Some(state) = self.tab_drag_state.take() {
                        if state.has_dragged {
                            self.start_tab_settle_animation(state.tab_idx, state.drag_offset_x);
                        }
                        self.finish_mouse_release(*press);
                        return;
                    }
                }
                if ends_touch_band_press {
                    self.finish_mouse_release(*press);
                    return;
                }
            }

            WMEK::Press(ref press) => {
                // If a previous edge drag never received its Release, reset now.
                self.window_drag.edge_drag_in_progress = false;
                self.mouse.touch_band_press = None;
                capture_mouse = true;

                // Perform click counting
                let button = mouse_press_to_tmb(press);

                // Use sentinel row value for title/padding area clicks to prevent
                // chaining with terminal first row (row=0) as a double-click
                let click_row = if event.coords.y < terminal_origin_y {
                    i64::MIN
                } else {
                    y
                };
                let click_position = ClickPosition {
                    column: x,
                    row: click_row,
                    x_pixel_offset,
                    y_pixel_offset,
                };

                let click = match self.mouse.last_mouse_click.take() {
                    None => LastMouseClick::new(button, click_position),
                    Some(click) => click.add(button, click_position),
                };
                self.mouse.last_mouse_click = Some(click);
                self.mouse.current_mouse_buttons.retain(|p| p != press);
                self.mouse.current_mouse_buttons.push(*press);
                if press == &MousePress::Left {
                    // Re-evaluated by the binding lookup for this press; a
                    // press that is forwarded to a mouse-reporting app must
                    // not inherit a stale selection-drag state (#455).
                    self.mouse.selection_drag_active = false;
                }

                if press == &MousePress::Left
                    && terminal_origin_y > 0
                    && (event.coords.y as isize) < terminal_origin_y
                {
                    // A left press above the terminal's first row may turn into
                    // a native window drag (title / tab strip). Enter
                    // drag-protection so follow-up motion/wheel isn't routed
                    // into terminal selection/scroll. Use terminal_origin_y
                    // rather than first_line_offset so the band of top
                    // padding above row 0 isn't claimed as draggable; that
                    // band is part of the terminal pane (#356, 3-finger drag).
                    self.mouse.current_mouse_capture = Some(MouseCapture::UI);
                    self.window_drag.is_window_dragging = true;
                }
            }

            WMEK::Move => {
                if self.window_drag.edge_drag_in_progress {
                    return;
                }
                if let Some(start) = self.window_drag.position.clone() {
                    if event.mouse_buttons != WMB::LEFT {
                        self.window_drag.position = None;
                        self.window_drag.is_window_dragging = false;
                        self.mouse.current_mouse_capture = None;
                    } else {
                        // Dragging the window: apply the screen-space delta
                        // since the anchor event to the window origin that the
                        // platform captured at the anchor. Do not infer the
                        // origin from screen_coords - coords: those use
                        // different pixel scales when the window is on a
                        // display whose scale differs from the primary one,
                        // which teleported the window at drag start (#456).
                        let top_left = manual_drag_window_top_left(&start, &event);
                        context.set_window_position(top_left);
                        return;
                    }
                }
                if self.window_drag.is_window_dragging {
                    if event.mouse_buttons == WMB::NONE {
                        // Defensive reset in case release was consumed by native drag.
                        self.window_drag.is_window_dragging = false;
                        self.mouse.current_mouse_capture = None;
                    } else {
                        // We requested a native drag move; while it is active,
                        // suppress terminal mouse handling to avoid accidental scrolling.
                        return;
                    }
                }
                if event.mouse_buttons != WMB::NONE
                    && self.mouse.current_mouse_buttons.is_empty()
                    && self.mouse.current_mouse_capture.is_none()
                {
                    // Ignore drag motion that started outside the terminal view
                    // (for example, dragging the native title bar and crossing
                    // into content), so we don't accidentally select/scroll.
                    return;
                }

                if let Some((item, start_event)) = self.dragging.take() {
                    self.drag_ui_item(item, start_event, x, y, event, context);
                    return;
                }
                if self.drag_tab(&event, context) {
                    return;
                }
                if self.mouse.touch_band_press.is_some() {
                    if event.mouse_buttons == WMB::NONE {
                        // The release never arrived; stop owning the gesture.
                        self.mouse.touch_band_press = None;
                    } else {
                        return;
                    }
                }
            }
            WMEK::VertWheel(_) | WMEK::HorzWheel(_) => {
                if self.window_drag.is_window_dragging {
                    if event.mouse_buttons == WMB::NONE {
                        // Defensive reset, mirroring the Move arm: a native
                        // drag can end without us seeing the release (AppKit
                        // consumes it). Without this, a stale drag flag would
                        // suppress wheel scrolling forever until a Move event
                        // happens to arrive.
                        self.window_drag.is_window_dragging = false;
                        self.mouse.current_mouse_capture = None;
                    } else {
                        // Drag still in progress; suppress wheel handling.
                        return;
                    }
                }
                if event.mouse_buttons != WMB::NONE
                    && !matches!(
                        self.mouse.current_mouse_capture,
                        Some(MouseCapture::TerminalPane(_))
                    )
                {
                    return;
                }
                if matches!(
                    self.resolve_ui_item(&event).map(|item| item.item_type),
                    Some(UIItemType::TabBar(_))
                ) {
                    return;
                }
            }
        }

        let prior_ui_item = self.mouse.last_ui_item.clone();

        let ui_item = if matches!(
            self.mouse.current_mouse_capture,
            None | Some(MouseCapture::UI)
        ) {
            let terminal_bottom = terminal_origin_y + self.terminal_size.pixel_height as isize;
            let direct_item = self.resolve_ui_item(&event);
            let touch_band_item = match direct_item {
                Some(_) => None,
                None => self.resolve_bottom_tab_bar_touch_item(&event, terminal_bottom),
            };
            if let (Some(_), WMEK::Press(press)) = (&touch_band_item, &event.kind) {
                self.mouse.touch_band_press = Some(*press);
                // The press was counted on the last terminal row; move it off
                // the grid so it cannot chain into a terminal double-click.
                if let Some(click) = self.mouse.last_mouse_click.as_mut() {
                    click.position.row = i64::MIN;
                }
            }
            let ui_item = direct_item.or(touch_band_item);

            match (self.mouse.last_ui_item.take(), &ui_item) {
                (Some(prior), Some(item)) => {
                    if prior != *item || !self.config.use_fancy_tab_bar {
                        self.leave_ui_item(&prior);
                        self.enter_ui_item(item);
                        context.invalidate();
                    }
                }
                (Some(prior), None) => {
                    self.leave_ui_item(&prior);
                    context.invalidate();
                }
                (None, Some(item)) => {
                    self.enter_ui_item(item);
                    context.invalidate();
                }
                (None, None) => {}
            }

            ui_item
        } else {
            None
        };

        match mouse_dispatch_target(
            ui_item.is_some(),
            event.coords.y,
            terminal_origin_y,
            self.mouse.current_mouse_capture.as_ref(),
        ) {
            MouseDispatchTarget::Ui => {
                let item = ui_item
                    .clone()
                    .expect("ui item must exist when dispatching to UI");
                if capture_mouse {
                    self.mouse.current_mouse_capture = Some(MouseCapture::UI);
                }
                self.mouse_event_ui_item(item, pane, y, event, context);
            }
            MouseDispatchTarget::TitleArea => {
                // Event landed in title/padding area above terminal content but missed all UI items.
                match event.kind {
                    WMEK::Press(MousePress::Left) => {
                        let fullscreen = self.window_state.contains(WindowState::FULL_SCREEN);
                        let maximized = self.window_state.contains(WindowState::MAXIMIZED);
                        // Use platform click count to avoid the manual streak counter
                        // false-positiving on title-bar single clicks (#414).
                        if event.platform_click_count == 2 {
                            if let Some(ref window) = self.window {
                                match title_area_double_click_zoom_action(self.window_state) {
                                    TitleAreaZoomAction::Maximize => window.maximize(),
                                    TitleAreaZoomAction::Restore => window.restore(),
                                }
                            }
                            return;
                        }
                        self.mouse.current_mouse_capture = Some(MouseCapture::UI);
                        self.window_drag.is_window_dragging = true;
                        if should_use_manual_window_drag(self.window_state)
                            || (!maximized && !fullscreen && !cfg!(target_os = "macos"))
                        {
                            self.window_drag.position.replace(event.clone());
                        }
                        if !should_use_manual_window_drag(self.window_state) {
                            if should_use_native_maximized_window_drag(self.window_state) {
                                context.request_drag_move_from_maximized();
                            } else {
                                context.request_drag_move();
                            }
                        }
                        return;
                    }
                    WMEK::Move if self.mouse.current_mouse_capture.is_none() => {
                        // Set Arrow cursor for move events when no capture is active.
                        // Prevents macOS NSTextInputClient from defaulting to IBeam.
                        context.set_cursor(Some(MouseCursor::Arrow));
                    }
                    _ => {}
                }
            }
            MouseDispatchTarget::Terminal => {
                self.mouse_event_terminal(
                    pane,
                    ClickPosition {
                        column: x,
                        row: y,
                        x_pixel_offset,
                        y_pixel_offset,
                    },
                    event,
                    context,
                    capture_mouse,
                );
            }
        }

        if let Some(press) = release_button {
            // Keep the original capture alive until the release has been
            // dispatched, otherwise drags that end outside the content area
            // never complete the selection.
            self.finish_mouse_release(press);
        }

        if prior_ui_item != ui_item && !self.window_drag.is_window_dragging {
            self.update_title_post_status();
        }
    }

    pub fn mouse_leave_impl(&mut self, context: &dyn WindowOps) {
        self.mouse.current_mouse_event = None;
        self.scrollbar.hovering = false;
        self.update_title();
        context.set_cursor(Some(MouseCursor::Arrow));
        context.invalidate();
    }

    fn drag_split(
        &mut self,
        mut item: UIItem,
        split: PositionedSplit,
        start_event: MouseEvent,
        x: usize,
        y: i64,
        grab: f32,
        context: &dyn WindowOps,
    ) {
        let mux = Mux::get();

        // On the first drag event, capture the tab_id from the active tab.
        // All subsequent frames (and the final release) use this tab_id
        // so we always operate on the same tab even if tabs switch mid-drag.
        let tab = if let Some(ref state) = self.split_drag_state {
            match mux.get_tab(state.tab_id) {
                Some(tab) => tab,
                None => {
                    // The original tab was closed mid-drag. End this drag
                    // instead of retargeting another tab with stale split metadata.
                    self.split_drag_state = None;
                    return;
                }
            }
        } else {
            let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                Some(tab) => tab,
                None => return,
            };
            self.split_drag_state = Some(super::SplitDragState {
                tab_id: tab.tab_id(),
                grab,
            });
            tab
        };

        let delta = match split.direction {
            SplitDirection::Horizontal => (x as isize).saturating_sub(split.left as isize),
            SplitDirection::Vertical => (y as isize).saturating_sub(split.top as isize),
        };

        if delta != 0 {
            // Use visual-only resize during drag: updates terminal state
            // for smooth content reflow but does NOT notify the PTY,
            // so the shell won't receive rapid SIGWINCH signals.
            tab.resize_split_by_visual(split.index, delta);
            if let Some(split) = tab.iter_splits().into_iter().nth(split.index) {
                item.item_type = UIItemType::Split(split);
                context.invalidate();
            }
        }
        self.dragging.replace((item, start_event));
    }

    fn drag_scroll_thumb(
        &mut self,
        item: UIItem,
        start_event: MouseEvent,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let dims = pane.get_dimensions();
        let current_viewport = self.effective_viewport(&pane);

        let Some(track) = self.scrollbar_track_for_pane(&pane) else {
            return;
        };

        let from_top = start_event.coords.y.saturating_sub(item.y as isize);
        let effective_thumb_top = event
            .coords
            .y
            .saturating_sub(track.top as isize + from_top)
            .max(0) as usize;

        // Convert thumb top into a row index by reversing the math
        // in ScrollHit::thumb
        let row = ScrollHit::thumb_top_to_scroll_top(
            effective_thumb_top,
            &*pane,
            current_viewport,
            track.height,
            self.min_scroll_bar_height() as usize,
        );
        self.reveal_scrollbar();
        self.set_viewport(pane.pane_id(), Some(row), dims);
        context.invalidate();
        self.dragging.replace((item, start_event));
    }

    fn drag_ui_item(
        &mut self,
        item: UIItem,
        start_event: MouseEvent,
        x: usize,
        y: i64,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match item.item_type {
            UIItemType::Split(split) => {
                // Measure drags against the drawn line rather than the floored
                // grid cell; with an even row gutter the line sits on a row
                // edge (#562). The hit area spans the whole gutter, so offset by
                // where the drag started instead of snapping to the pointer.
                let (padding_left, padding_top) = self.padding_left_top();
                let cell_width = self.render_metrics.cell_size.width as f32;
                let cell_height = self.render_metrics.cell_size.height as f32;
                let col_origin = padding_left + self.get_os_border().left.get() as f32;
                let row_origin = self.terminal_first_row_offset() + padding_top;
                let row_center = crate::termwindow::render::split::split_row_center_offset(
                    self.config.split_pane_gap.max(1) as usize,
                );
                let pos = |coords: &::window::Point| -> f32 {
                    match split.direction {
                        SplitDirection::Horizontal => (coords.x as f32 - col_origin) / cell_width,
                        SplitDirection::Vertical => (coords.y as f32 - row_origin) / cell_height,
                    }
                };
                let grab = match &self.split_drag_state {
                    Some(state) => state.grab,
                    // First frame: `split` is still the line as it was pressed.
                    None => {
                        let line = match split.direction {
                            SplitDirection::Horizontal => split.left as f32 + 0.5,
                            SplitDirection::Vertical => split.top as f32 + row_center,
                        };
                        pos(&start_event.coords) - line
                    }
                };
                let x = match split.direction {
                    SplitDirection::Horizontal => {
                        crate::termwindow::render::split::split_drag_col(pos(&event.coords), grab)
                            .max(0) as usize
                    }
                    SplitDirection::Vertical => x,
                };
                let y = if y < 0 || split.direction == SplitDirection::Horizontal {
                    y
                } else {
                    crate::termwindow::render::split::split_drag_row(
                        pos(&event.coords) - grab,
                        row_center,
                    )
                    .max(0)
                };
                self.drag_split(item, split, start_event, x, y, grab, context);
            }
            UIItemType::ScrollThumb => {
                self.drag_scroll_thumb(item, start_event, event, context);
            }
            _ => {
                log::error!("drag not implemented for {:?}", item);
            }
        }
    }

    fn mouse_event_ui_item(
        &mut self,
        item: UIItem,
        pane: Arc<dyn Pane>,
        _y: i64,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        self.mouse.last_ui_item.replace(item.clone());
        match item.item_type.clone() {
            UIItemType::TabBar(tab_bar_item) => {
                self.mouse_event_tab_bar(tab_bar_item, item, event, context);
            }
            UIItemType::AboveScrollThumb => {
                self.mouse_event_above_scroll_thumb(item, pane, event, context);
            }
            UIItemType::ScrollThumb => {
                self.mouse_event_scroll_thumb(item, pane, event, context);
            }
            UIItemType::BelowScrollThumb => {
                self.mouse_event_below_scroll_thumb(item, pane, event, context);
            }
            UIItemType::Split(split) => {
                self.mouse_event_split(item, split, event, context);
            }
            UIItemType::CloseTab(idx) => {
                self.mouse_event_close_tab(idx, event, context);
            }
        }
    }

    pub fn mouse_event_close_tab(
        &mut self,
        idx: usize,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match event.kind {
            WMEK::Press(MousePress::Left) => {
                log::debug!("Should close tab {}", idx);
                self.close_specific_tab(idx, true);
            }
            _ => {}
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    /// Opens the tab menu for the clicked tab. Its actions target that tab's
    /// active pane, so they apply to the tab under the pointer even when it is
    /// not the active one.
    fn show_tab_context_menu(
        &mut self,
        tab_idx: usize,
        event: &MouseEvent,
        context: &dyn WindowOps,
    ) {
        #[cfg(target_os = "macos")]
        {
            let pane_id = Mux::get()
                .get_window(self.mux_window_id)
                .and_then(|window| window.get_by_idx(tab_idx).cloned())
                .and_then(|tab| tab.get_active_pane())
                .map(|pane| pane.pane_id().as_usize());
            if let Some(pane_id) = pane_id {
                let menu = Menu::new_with_title("");
                for (title, action) in tab_context_menu_actions() {
                    if matches!(action, KeyAssignment::CloseCurrentTab { .. }) {
                        menu.add_item(&MenuItem::new_separator());
                    }
                    menu.add_item(&pane_action_menu_item(title, action, pane_id));
                }
                context.show_context_menu(menu, event.screen_coords);
                return;
            }
        }
        let _ = (tab_idx, event, context);
        self.show_tab_navigator();
    }

    fn do_new_tab_button_click(&mut self, button: MousePress) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };
        let action = match button {
            MousePress::Left => Some(KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain)),
            MousePress::Right => None,
            MousePress::Middle => None,
        };

        async fn dispatch_new_tab_button(
            lua: Option<Rc<mlua::Lua>>,
            window: GuiWin,
            pane: MuxPane,
            button: MousePress,
            action: Option<KeyAssignment>,
        ) -> anyhow::Result<()> {
            let default_action = match lua {
                Some(lua) => {
                    let args = lua.pack_multi((
                        window.clone(),
                        pane,
                        format!("{button:?}"),
                        action.clone(),
                    ))?;
                    config::lua::emit_event(&lua, ("new-tab-button-click".to_string(), args))
                        .await
                        .map_err(|e| {
                            log::error!("while processing new-tab-button-click event: {:#}", e);
                            e
                        })?
                }
                None => true,
            };
            if let (true, Some(assignment)) = (default_action, action) {
                window.window.notify(TermWindowNotif::PerformAssignment {
                    pane_id: pane.0,
                    assignment,
                    tx: None,
                });
            }
            Ok(())
        }
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());
        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            dispatch_new_tab_button(lua, window, pane, button, action)
        }))
        .detach();
    }

    pub fn mouse_event_tab_bar(
        &mut self,
        item: TabBarItem,
        ui_item: UIItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match event.kind {
            WMEK::Press(MousePress::Left) => {
                if !tab_bar_item_starts_window_drag(item) {
                    self.window_drag.is_window_dragging = false;
                    self.window_drag.position = None;
                }

                match item {
                    TabBarItem::Tab { tab_idx, active } => {
                        if event.platform_click_count == 2 {
                            self.tab_drag_state = None;
                            if let Err(err) = self.begin_tab_rename(tab_idx, ui_item) {
                                log::debug!("begin_tab_rename({tab_idx}) failed: {err:#}");
                            }
                            context.set_cursor(Some(MouseCursor::Arrow));
                            return;
                        }
                        if !active {
                            if let Err(err) = self.activate_tab(tab_idx as isize) {
                                log::debug!("activate_tab({tab_idx}) failed: {err:#}");
                            }
                        }
                        self.start_tab_drag(tab_idx, event.clone());
                    }
                    TabBarItem::NewTabButton { .. } => {
                        self.tab_drag_state = None;
                        self.do_new_tab_button_click(MousePress::Left);
                    }
                    TabBarItem::None | TabBarItem::LeftStatus | TabBarItem::RightStatus => {
                        self.tab_drag_state = None;
                        let fullscreen = self.window_state.contains(WindowState::FULL_SCREEN);
                        let maximized = self.window_state.contains(WindowState::MAXIMIZED);
                        if let Some(ref window) = self.window {
                            if should_zoom_title_area(
                                self.config.window_decorations,
                                event.platform_click_count,
                            ) {
                                match title_area_double_click_zoom_action(self.window_state) {
                                    TitleAreaZoomAction::Maximize => window.maximize(),
                                    TitleAreaZoomAction::Restore => window.restore(),
                                }
                                return;
                            }
                        }
                        self.window_drag.is_window_dragging = true;
                        if should_use_manual_window_drag(self.window_state)
                            || (!maximized && !fullscreen && !cfg!(target_os = "macos"))
                        {
                            self.window_drag.position.replace(event.clone());
                        }
                        if !should_use_manual_window_drag(self.window_state) {
                            if should_use_native_maximized_window_drag(self.window_state) {
                                context.request_drag_move_from_maximized();
                            } else {
                                context.request_drag_move();
                            }
                        }
                    }
                    TabBarItem::WindowButton(button) => {
                        self.tab_drag_state = None;
                        use window::IntegratedTitleButton as Button;
                        if let Some(ref window) = self.window {
                            match button {
                                Button::Hide => window.hide(),
                                Button::Maximize => {
                                    let maximized = self.window_state.intersects(
                                        WindowState::MAXIMIZED | WindowState::FULL_SCREEN,
                                    );
                                    if maximized {
                                        window.restore();
                                    } else {
                                        window.maximize();
                                    }
                                }
                                Button::Close => self.close_requested(&window.clone()),
                            }
                        }
                    }
                }
            }
            WMEK::Press(MousePress::Middle) => match item {
                TabBarItem::Tab { tab_idx, .. } => {
                    self.tab_drag_state = None;
                    self.close_specific_tab(tab_idx, true);
                }
                TabBarItem::NewTabButton { .. } => {
                    self.tab_drag_state = None;
                    self.do_new_tab_button_click(MousePress::Middle);
                }
                TabBarItem::None
                | TabBarItem::LeftStatus
                | TabBarItem::RightStatus
                | TabBarItem::WindowButton(_) => {}
            },
            WMEK::Press(MousePress::Right) => match item {
                TabBarItem::Tab { tab_idx, .. } => {
                    self.tab_drag_state = None;
                    self.show_tab_context_menu(tab_idx, &event, context);
                }
                TabBarItem::NewTabButton { .. } => {
                    self.tab_drag_state = None;
                    self.do_new_tab_button_click(MousePress::Right);
                }
                TabBarItem::None
                | TabBarItem::LeftStatus
                | TabBarItem::RightStatus
                | TabBarItem::WindowButton(_) => {}
            },
            WMEK::Move => match item {
                TabBarItem::None | TabBarItem::LeftStatus | TabBarItem::RightStatus => {
                    context.set_window_drag_position(event.screen_coords);
                }
                TabBarItem::WindowButton(window::IntegratedTitleButton::Maximize) => {
                    if let Some(item) = self.mouse.last_ui_item.clone() {
                        let bounds: ::window::ScreenRect = euclid::rect(
                            item.x as isize + event.window_origin.x,
                            item.y as isize + event.window_origin.y,
                            item.width as isize,
                            item.height as isize,
                        );
                        context.set_maximize_button_position(bounds);
                    }
                }
                TabBarItem::WindowButton(_)
                | TabBarItem::Tab { .. }
                | TabBarItem::NewTabButton { .. } => {}
            },
            WMEK::VertWheel(n) => {
                let scrolls_tabs = self.config.mouse_wheel_scrolls_tabs;
                if scrolls_tabs {
                    if let Err(err) = self.activate_tab_relative(if n < 1 { 1 } else { -1 }, true) {
                        log::debug!("activate_tab_relative on wheel failed: {err:#}");
                    }
                }
            }
            _ => {}
        }
        let cursor = match item {
            TabBarItem::Tab { .. }
            | TabBarItem::NewTabButton { .. }
            | TabBarItem::WindowButton(_) => MouseCursor::Hand,
            _ => MouseCursor::Arrow,
        };
        context.set_cursor(Some(cursor));
    }

    pub fn mouse_event_above_scroll_thumb(
        &mut self,
        _item: UIItem,
        pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            let dims = pane.get_dimensions();
            let current_viewport = self.effective_viewport(&pane);
            // Page up
            self.reveal_scrollbar();
            self.set_viewport(
                pane.pane_id(),
                Some(
                    current_viewport
                        .unwrap_or(dims.physical_top)
                        .saturating_sub(self.terminal_size.rows.try_into().unwrap()),
                ),
                dims,
            );
            context.invalidate();
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_below_scroll_thumb(
        &mut self,
        _item: UIItem,
        pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            let dims = pane.get_dimensions();
            let current_viewport = self.effective_viewport(&pane);
            // Page down
            self.reveal_scrollbar();
            self.set_viewport(
                pane.pane_id(),
                Some(
                    current_viewport
                        .unwrap_or(dims.physical_top)
                        .saturating_add(self.terminal_size.rows.try_into().unwrap()),
                ),
                dims,
            );
            // Exit peek mode when scrolling to bottom
            if pane.is_primary_peek() && self.effective_viewport(&pane).is_none() {
                pane.set_primary_peek(false);
            }
            context.invalidate();
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_scroll_thumb(
        &mut self,
        item: UIItem,
        _pane: Arc<dyn Pane>,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        if let WMEK::Press(MousePress::Left) = event.kind {
            // Start a scroll drag
            // self.scroll_drag_start = Some(from_top);
            self.reveal_scrollbar();
            self.dragging = Some((item, event));
        }
        context.set_cursor(Some(MouseCursor::Arrow));
    }

    pub fn mouse_event_split(
        &mut self,
        item: UIItem,
        split: PositionedSplit,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        context.set_cursor(Some(match &split.direction {
            SplitDirection::Horizontal => MouseCursor::SizeLeftRight,
            SplitDirection::Vertical => MouseCursor::SizeUpDown,
        }));

        if event.kind == WMEK::Press(MousePress::Left) {
            self.dragging.replace((item, event));
        }
    }

    fn mouse_event_terminal(
        &mut self,
        mut pane: Arc<dyn Pane>,
        position: ClickPosition,
        event: MouseEvent,
        context: &dyn WindowOps,
        capture_mouse: bool,
    ) {
        let mut is_click_to_focus_pane = false;

        let ClickPosition {
            mut column,
            mut row,
            mut x_pixel_offset,
            mut y_pixel_offset,
        } = position;

        let is_already_captured = matches!(
            self.mouse.current_mouse_capture,
            Some(MouseCapture::TerminalPane(_))
        );

        for pos in self.get_panes_to_render() {
            if !is_already_captured
                && row >= pos.top as i64
                && row <= (pos.top + pos.height) as i64
                && column >= pos.left
                && column <= pos.left + pos.width
            {
                if pane.pane_id() != pos.pane.pane_id() {
                    // We're over a pane that isn't active
                    match &event.kind {
                        WMEK::Press(_) => {
                            let mux = Mux::get();
                            mux.get_active_tab_for_window(self.mux_window_id)
                                .map(|tab| tab.set_active_idx(pos.index));

                            pane = Arc::clone(&pos.pane);
                            is_click_to_focus_pane = true;
                        }
                        WMEK::Move => {
                            if self.config.pane_focus_follows_mouse {
                                let mux = Mux::get();
                                mux.get_active_tab_for_window(self.mux_window_id)
                                    .map(|tab| tab.set_active_idx(pos.index));

                                pane = Arc::clone(&pos.pane);
                                context.invalidate();
                            }
                        }
                        WMEK::Release(_) | WMEK::HorzWheel(_) => {}
                        WMEK::VertWheel(_) => {
                            // Let wheel events route to the hovered pane,
                            // even if it doesn't have focus
                            pane = Arc::clone(&pos.pane);
                            context.invalidate();
                        }
                    }
                }
                column = column.saturating_sub(pos.left);
                row = row.saturating_sub(pos.top as i64);
                break;
            } else if is_already_captured && pane.pane_id() == pos.pane.pane_id() {
                column = column.saturating_sub(pos.left);
                row = row.saturating_sub(pos.top as i64).max(0);

                if position.column < pos.left {
                    x_pixel_offset -= self.render_metrics.cell_size.width
                        * (pos.left as isize - position.column as isize);
                }
                if position.row < pos.top as i64 {
                    y_pixel_offset -= self.render_metrics.cell_size.height
                        * (pos.top as isize - position.row as isize);
                }

                break;
            }
        }

        // Detect when the mouse is in the OS resize handle zone.
        // Only used to prevent mouse capture and to seed edge_drag_in_progress;
        // event suppression is driven by edge_drag_in_progress state, not position.
        let outside_window = event.coords.x < 0
            || event.coords.x as usize > self.dimensions.pixel_width
            || event.coords.y < 0
            || event.coords.y as usize > self.dimensions.pixel_height;

        #[cfg(target_os = "macos")]
        let base_dpi: usize = 72;
        #[cfg(not(target_os = "macos"))]
        let base_dpi: usize = 96;
        let resize_zone_pt: usize = 5;
        let resize_zone =
            (resize_zone_pt * self.dimensions.dpi / base_dpi).max(resize_zone_pt) as isize;
        let in_resize_zone = event.coords.x < resize_zone
            || (event.coords.x as usize)
                >= self
                    .dimensions
                    .pixel_width
                    .saturating_sub(resize_zone as usize)
            || event.coords.y < resize_zone
            || (event.coords.y as usize)
                >= self
                    .dimensions
                    .pixel_height
                    .saturating_sub(resize_zone as usize);

        if capture_mouse && !in_resize_zone {
            self.mouse.current_mouse_capture = Some(MouseCapture::TerminalPane(pane.pane_id()));
        }

        if matches!(event.kind, WMEK::Press(MousePress::Left)) && in_resize_zone {
            self.window_drag.edge_drag_in_progress = true;
        }

        let is_focused = if let Some(focused) = self.focused.as_ref() {
            !self.config.swallow_mouse_click_on_window_focus
                || (focused.elapsed() > Duration::from_millis(200))
        } else {
            false
        };

        if self.focused.is_some() && !is_focused {
            if matches!(&event.kind, WMEK::Press(_))
                && self.config.swallow_mouse_click_on_window_focus
            {
                // Entering click to focus state
                self.window_drag.is_click_to_focus = true;
                context.invalidate();
                log::trace!("enter click to focus");
                return;
            }
        }
        if self.window_drag.is_click_to_focus && matches!(&event.kind, WMEK::Release(_)) {
            // Exiting click to focus state
            self.window_drag.is_click_to_focus = false;
            context.invalidate();
            log::trace!("exit click to focus");
            return;
        }

        let allow_action = if self.window_drag.is_click_to_focus || !is_focused {
            matches!(&event.kind, WMEK::VertWheel(_) | WMEK::HorzWheel(_))
        } else {
            true
        };

        log::trace!(
            "is_focused={} allow_action={} event={:?}",
            is_focused,
            allow_action,
            event
        );

        let dims = pane.get_dimensions();
        let stable_row =
            self.effective_viewport(&pane).unwrap_or(dims.physical_top) + row as StableRowIndex;

        self.pane_state(pane.pane_id())
            .mouse_terminal_coords
            .replace((
                ClickPosition {
                    column,
                    row,
                    x_pixel_offset,
                    y_pixel_offset,
                },
                stable_row,
            ));

        // apply_hyperlinks internally uses Screen::for_each_logical_line_in_stable_range_mut
        // which already walks backwards/forwards to cover the full logical line, so
        // passing the hovered row is sufficient even when the URL wraps across physical rows.
        pane.apply_hyperlinks(stable_row..stable_row + 1, &self.config.hyperlink_rules);

        struct FindCurrentLink {
            current: Option<Arc<Hyperlink>>,
            stable_row: StableRowIndex,
            column: usize,
        }

        impl WithPaneLines for FindCurrentLink {
            fn with_lines_mut(&mut self, stable_top: StableRowIndex, lines: &mut [&mut Line]) {
                if stable_top == self.stable_row {
                    if let Some(line) = lines.get(0) {
                        if let Some(cell) = line.get_cell(self.column) {
                            self.current = cell.attrs().hyperlink().cloned();
                        }
                    }
                }
            }
        }

        let mut find_link = FindCurrentLink {
            current: None,
            stable_row,
            column,
        };
        pane.with_lines_mut(stable_row..stable_row + 1, &mut find_link);
        let new_highlight = find_link.current;

        match (self.current_highlight.as_ref(), new_highlight) {
            (Some(old_link), Some(new_link)) if Arc::ptr_eq(&old_link, &new_link) => {
                // Unchanged
            }
            (None, None) => {
                // Unchanged
            }
            (_, rhs) => {
                // We're hovering over a different URL, so invalidate and repaint
                // so that we render the underline correctly
                self.current_highlight = rhs;
                context.invalidate();
            }
        };

        context.set_cursor(Some(if self.current_highlight.is_some() {
            // When hovering over a hyperlink, show an appropriate
            // mouse cursor to give the cue that it is clickable
            MouseCursor::Hand
        } else if pane.is_mouse_grabbed()
            || outside_window
            || in_resize_zone
            || self.window_drag.edge_drag_in_progress
        {
            MouseCursor::Arrow
        } else {
            MouseCursor::Text
        }));

        let event_trigger_type = match &event.kind {
            WMEK::Press(press) => {
                let press = mouse_press_to_tmb(press);
                match self.mouse.last_mouse_click.as_ref() {
                    Some(LastMouseClick { streak, button, .. }) if *button == press => {
                        Some(MouseEventTrigger::Down {
                            streak: *streak,
                            button: press,
                        })
                    }
                    _ => None,
                }
            }
            WMEK::Release(press) => {
                let press = mouse_press_to_tmb(press);
                match self.mouse.last_mouse_click.as_ref() {
                    Some(LastMouseClick { streak, button, .. }) if *button == press => {
                        Some(MouseEventTrigger::Up {
                            streak: *streak,
                            button: press,
                        })
                    }
                    _ => None,
                }
            }
            WMEK::Move => {
                if !self.mouse.current_mouse_buttons.is_empty() {
                    if let Some(LastMouseClick { streak, button, .. }) =
                        self.mouse.last_mouse_click.as_ref()
                    {
                        if Some(*button)
                            == self
                                .mouse
                                .current_mouse_buttons
                                .last()
                                .map(mouse_press_to_tmb)
                        {
                            Some(MouseEventTrigger::Drag {
                                streak: *streak,
                                button: *button,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            WMEK::VertWheel(amount) => Some(match *amount {
                0 => return,
                1.. => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelUp(*amount as usize),
                },
                _ => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelDown(-amount as usize),
                },
            }),
            WMEK::HorzWheel(amount) => Some(match *amount {
                0 => return,
                1.. => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelLeft(*amount as usize),
                },
                _ => MouseEventTrigger::Down {
                    streak: 1,
                    button: MouseButton::WheelRight(-amount as usize),
                },
            }),
        };

        // Some less setups run without alt screen. In that mode, the default
        // wheel binding scrolls terminal scrollback instead of less content.
        // Detect less and map wheel to arrow keys directly.
        let is_wheel_event = matches!(event.kind, WMEK::VertWheel(_) | WMEK::HorzWheel(_));
        let foreground_process = if is_wheel_event {
            pane.get_foreground_process_name(CachePolicy::AllowStale)
        } else {
            None
        };
        let foreground_process_info = if is_wheel_event {
            pane.get_foreground_process_info(CachePolicy::AllowStale)
        } else {
            None
        };
        let foreground_bin = foreground_process
            .as_deref()
            .and_then(|name| name.rsplit('/').next());
        let in_tmux_process_tree = foreground_bin == Some("tmux")
            || foreground_process_info
                .as_ref()
                .map(|info| info.flatten_to_exe_names().contains("tmux"))
                .unwrap_or(false);
        let less_without_alt = is_wheel_event
            && !pane.is_alt_screen_active()
            && !pane.is_mouse_grabbed()
            && foreground_bin == Some("less");
        let preserve_tmux_bypass_reporting = should_preserve_tmux_bypass_reporting(
            is_wheel_event,
            event.modifiers,
            self.config.bypass_mouse_reporting_modifiers,
            pane.is_alt_screen_active(),
            pane.is_mouse_grabbed(),
            in_tmux_process_tree,
        );
        let bypass_wheel_assignment_in_alt = should_bypass_wheel_assignment_in_alt(
            is_wheel_event,
            pane.is_alt_screen_active(),
            pane.is_mouse_grabbed(),
            self.config.alternate_screen_wheel_scrolls_terminal,
        );
        if less_without_alt {
            let (key, amount) = match event.kind {
                WMEK::VertWheel(amount) if amount > 0 => (KeyCode::UpArrow, amount as usize),
                WMEK::VertWheel(amount) if amount < 0 => (KeyCode::DownArrow, (-amount) as usize),
                WMEK::HorzWheel(amount) if amount > 0 => (KeyCode::LeftArrow, amount as usize),
                WMEK::HorzWheel(amount) if amount < 0 => (KeyCode::RightArrow, (-amount) as usize),
                _ => (KeyCode::DownArrow, 0),
            };
            for _ in 0..amount {
                if let Err(err) = pane.key_down(key.clone(), KeyModifiers::default()) {
                    log::debug!("forwarding wheel as key to less failed: {err:#}");
                    break;
                }
            }
            context.invalidate();
            return;
        }

        if bypass_wheel_assignment_in_alt {
            if let Err(err) = self.scroll_by_current_event_wheel_delta(&pane) {
                log::debug!("scroll_by_current_event_wheel_delta failed: {err:#}");
            }
            context.invalidate();
            return;
        }

        if allow_action
            && !self.window_drag.edge_drag_in_progress
            && !bypass_wheel_assignment_in_alt
        {
            // Cmd+Click should open a hovered link even inside a mouse-reporting
            // pane (claude/codex/vim/tmux) so the same shortcut works everywhere.
            // Normally the OpenLinkAtMouseCursor binding is gated on
            // mouse_reporting=false, so in those panes the click is forwarded to
            // the application and you'd otherwise need Shift+Cmd+Click. Re-run the
            // link-open lookup as if reporting were off, but ONLY when the cursor
            // is over a link and the user is not holding the bypass modifier (the
            // bypass case is already handled by the normal path below). Swallow
            // both the press and the release so the application never sees a
            // dangling half-click.
            if pane.is_mouse_grabbed()
                && self.current_highlight.is_some()
                && !event
                    .modifiers
                    .contains(self.config.bypass_mouse_reporting_modifiers)
                && matches!(
                    event.kind,
                    WMEK::Press(MousePress::Left) | WMEK::Release(MousePress::Left)
                )
            {
                let link_mods = config::MouseEventTriggerMods {
                    mods: event.modifiers,
                    mouse_reporting: false,
                    alt_screen: if pane.is_alt_screen_active() {
                        MouseEventAltScreen::True
                    } else {
                        MouseEventAltScreen::False
                    },
                };
                if let Some(
                    action @ (KeyAssignment::OpenLinkAtMouseCursor
                    | KeyAssignment::CompleteSelectionOrOpenLinkAtMouseCursor(_)),
                ) = self.keyboard.input_map.lookup_mouse(
                    MouseEventTrigger::Up {
                        streak: 1,
                        button: MouseButton::Left,
                    },
                    link_mods,
                ) {
                    if matches!(event.kind, WMEK::Release(MousePress::Left)) {
                        if let Err(err) = self.perform_key_assignment(&pane, &action) {
                            log::debug!("cmd+click link open failed: {err:#}");
                        }
                    }
                    return;
                }
            }

            if let Some(mut event_trigger_type) = event_trigger_type {
                self.current_event = Some(event_trigger_type.to_dynamic());
                let mut modifiers = event.modifiers;

                // Since we use shift to force assessing the mouse bindings, pretend
                // that shift is not one of the mods when the mouse is grabbed.
                let mut mouse_reporting = pane.is_mouse_grabbed();
                if mouse_reporting {
                    if modifiers.contains(self.config.bypass_mouse_reporting_modifiers)
                        && !preserve_tmux_bypass_reporting
                    {
                        modifiers.remove(self.config.bypass_mouse_reporting_modifiers);
                        mouse_reporting = false;
                    }
                }

                if mouse_reporting {
                    // If they were scrolled back prior to launching an
                    // application that captures the mouse, then mouse based
                    // scrolling assignments won't have any effect.
                    // Ensure that we scroll to the bottom if they try to
                    // use the mouse so that things are less surprising
                    self.scroll_to_bottom(&pane);
                }

                // Option+Click: move the terminal cursor to the clicked cell by
                // synthesizing arrow keypresses, like iTerm2.
                // Only fires when the shell owns the prompt (no mouse grab, no
                // alt screen) and only for a clean click: the press falls
                // through to start a block selection, so if the user dragged,
                // a selection range exists by release time and we leave the
                // event to the selection bindings instead.
                if !pane.is_mouse_grabbed()
                    && !pane.is_alt_screen_active()
                    && matches!(event.kind, WMEK::Release(MousePress::Left))
                    && modifiers.contains(window::Modifiers::ALT)
                    && !modifiers.contains(window::Modifiers::SHIFT)
                    && stable_row >= dims.physical_top
                    && self.selection(pane.pane_id()).range.is_none()
                {
                    let cursor = pane.get_cursor_position();
                    let top = stable_row.min(cursor.y);
                    let bottom = stable_row.max(cursor.y);

                    #[derive(Default)]
                    struct GatherRows {
                        rows: Vec<OptionClickRowInfo>,
                    }
                    impl WithPaneLines for GatherRows {
                        fn with_lines_mut(
                            &mut self,
                            _first_row: StableRowIndex,
                            lines: &mut [&mut Line],
                        ) {
                            for line in lines.iter() {
                                let mut info = OptionClickRowInfo {
                                    wrapped: line.last_cell_was_wrapped(),
                                    ..Default::default()
                                };
                                for cell in line.visible_cells() {
                                    let idx = cell.cell_index();
                                    let width = cell.width();
                                    info.cells.push((idx, width));
                                }
                                self.rows.push(info);
                            }
                        }
                    }

                    let mut gather = GatherRows::default();
                    pane.with_lines_mut(top..bottom + 1, &mut gather);
                    let rows = gather.rows;

                    let cursor_row = (cursor.y - top) as usize;
                    let target_row = (stable_row - top) as usize;
                    let bytes = option_click_cursor_bytes(
                        &rows,
                        cursor_row,
                        cursor.x,
                        target_row,
                        column,
                        pane.application_cursor_keys_enabled(),
                    );

                    if !bytes.is_empty() {
                        if let Err(err) = self.write_terminal_input_bytes(&pane, &bytes) {
                            log::debug!("option+click cursor move failed: {err:#}");
                        }
                        self.maybe_scroll_to_bottom_for_input(&pane);
                        // The press started a block selection origin; drop it
                        // so a later shift-click does not extend from a stale
                        // point.
                        self.selection(pane.pane_id()).clear();
                        return;
                    }
                    // No movement was possible (e.g. the click crossed a hard
                    // newline). Drop the stale selection origin but let the
                    // release fall through so user Alt+LeftUp mouse bindings
                    // stay reachable.
                    self.selection(pane.pane_id()).clear();
                }

                // normalize delta and streak to make mouse assignment
                // easier to wrangle
                match event_trigger_type {
                    MouseEventTrigger::Down {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    }
                    | MouseEventTrigger::Up {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    }
                    | MouseEventTrigger::Drag {
                        ref mut streak,
                        button:
                            MouseButton::WheelUp(ref mut delta)
                            | MouseButton::WheelDown(ref mut delta)
                            | MouseButton::WheelLeft(ref mut delta)
                            | MouseButton::WheelRight(ref mut delta),
                    } => {
                        *streak = 1;
                        *delta = 1;
                    }
                    _ => {}
                };

                let alt_screen = pane.is_alt_screen_active();
                let mouse_mods = config::MouseEventTriggerMods {
                    mods: modifiers,
                    mouse_reporting,
                    alt_screen: if alt_screen {
                        MouseEventAltScreen::True
                    } else {
                        MouseEventAltScreen::False
                    },
                };

                let mouse_assignment = self
                    .keyboard
                    .input_map
                    .lookup_mouse(event_trigger_type, mouse_mods);

                #[cfg(target_os = "macos")]
                if should_show_terminal_context_menu(
                    &event.kind,
                    event.modifiers,
                    self.config.bypass_mouse_reporting_modifiers,
                    pane.is_mouse_grabbed(),
                    is_already_captured,
                    mouse_assignment.is_some(),
                ) {
                    let target_pane_id = Mux::get()
                        .get_active_tab_for_window(self.mux_window_id)
                        .and_then(|tab| tab.get_active_pane())
                        .map(|pane| pane.pane_id().as_usize());
                    if let Some(pane_id) = target_pane_id {
                        let has_selection =
                            !self.selection(mux::pane::PaneId::new(pane_id)).is_empty();
                        let menu = Menu::new_with_title("");
                        for (title, action) in terminal_context_menu_actions(has_selection) {
                            menu.add_item(&pane_action_menu_item(title, action, pane_id));
                        }
                        context.show_context_menu(menu, event.screen_coords);
                        return;
                    }
                }

                if let Some(action) = mouse_assignment {
                    if matches!(
                        action,
                        KeyAssignment::SelectTextAtMouseCursor(_)
                            | KeyAssignment::ExtendSelectionToMouseCursor(_)
                    ) {
                        // The held left button is genuinely driving a text
                        // selection; wheel events may now scroll-and-extend it.
                        self.mouse.selection_drag_active = true;
                    }
                    if let Err(err) = self.perform_key_assignment(&pane, &action) {
                        log::debug!("mouse assignment failed: {err:#}");
                    }
                    return;
                }
            }
        }

        // A plain left press that reaches this point is being forwarded to a
        // mouse-reporting application (claude code, vim, tmux with mouse on)
        // instead of matching the SelectTextAtMouseCursor binding, which is
        // gated on mouse_reporting=false. Clear any existing GUI selection
        // here, matching iTerm2: without this the highlight has no remaining
        // clear path and stays on the pane forever (#455).
        if matches!(event.kind, WMEK::Press(MousePress::Left))
            && allow_action
            && !self.window_drag.edge_drag_in_progress
            && !(self.config.swallow_mouse_click_on_pane_focus && is_click_to_focus_pane)
        {
            let needs_clear = {
                let selection = self.selection(pane.pane_id());
                selection.range.is_some() || selection.origin.is_some()
            };
            if needs_clear {
                self.selection(pane.pane_id()).clear();
                context.invalidate();
            }
        }

        let (kind, button) = wmek_to_tmek_and_button(&event);
        let mouse_event = wezterm_term::MouseEvent {
            kind,
            button,
            x: column,
            y: row,
            x_pixel_offset,
            y_pixel_offset,
            modifiers: event.modifiers,
        };

        if allow_action
            && !self.window_drag.edge_drag_in_progress
            && !(self.config.swallow_mouse_click_on_pane_focus && is_click_to_focus_pane)
        {
            if let Err(err) = pane.mouse_event(mouse_event) {
                log::debug!("forwarding mouse event to pane failed: {err:#}");
            }
        }

        match event.kind {
            WMEK::Move => {}
            _ => {
                context.invalidate();
            }
        }
    }
}

fn mouse_press_to_tmb(press: &MousePress) -> TMB {
    match press {
        MousePress::Left => TMB::Left,
        MousePress::Right => TMB::Right,
        MousePress::Middle => TMB::Middle,
    }
}

/// Maps a window-layer `MouseEvent` into the `(kind, button)` pair expected by
/// `wezterm_term::MouseEvent`. The same mapping was previously inlined in two
/// places (the modal forwarding path and the regular pane path), so updating
/// one without the other risked behavior drift.
fn wmek_to_tmek_and_button(event: &MouseEvent) -> (TMEK, TMB) {
    let kind = match event.kind {
        WMEK::Move => TMEK::Move,
        WMEK::VertWheel(_) | WMEK::HorzWheel(_) | WMEK::Press(_) => TMEK::Press,
        WMEK::Release(_) => TMEK::Release,
    };
    let button = match event.kind {
        WMEK::Release(ref press) | WMEK::Press(ref press) => mouse_press_to_tmb(press),
        WMEK::Move => {
            if event.mouse_buttons == WMB::LEFT {
                TMB::Left
            } else if event.mouse_buttons == WMB::RIGHT {
                TMB::Right
            } else if event.mouse_buttons == WMB::MIDDLE {
                TMB::Middle
            } else {
                TMB::None
            }
        }
        WMEK::VertWheel(amount) => {
            if amount > 0 {
                TMB::WheelUp(amount as usize)
            } else {
                TMB::WheelDown((-amount) as usize)
            }
        }
        WMEK::HorzWheel(amount) => {
            if amount > 0 {
                TMB::WheelLeft(amount as usize)
            } else {
                TMB::WheelRight((-amount) as usize)
            }
        }
    };
    (kind, button)
}

#[cfg(test)]
mod tests {
    use super::{
        bottom_tab_bar_touch_y, manual_drag_window_top_left, mouse_dispatch_target,
        option_click_cursor_bytes, press_takes_touch_extension,
        should_bypass_wheel_assignment_in_alt, should_preserve_tmux_bypass_reporting,
        should_show_terminal_context_menu, should_use_manual_window_drag,
        should_use_native_maximized_window_drag, should_zoom_title_area,
        tab_bar_item_starts_window_drag, tab_bar_item_takes_touch_extension,
        tab_context_menu_actions, terminal_context_menu_actions,
        title_area_double_click_zoom_action, wheel_during_terminal_selection_action,
        MouseDispatchTarget, OptionClickRowInfo, SelectionDragWheelAction, TitleAreaZoomAction,
    };
    use crate::tabbar::TabBarItem;
    use crate::termwindow::MouseCapture;
    use config::keyassignment::{ClipboardCopyDestination, KeyAssignment};
    use config::SelectionWheelScrollBehavior;
    use mux::pane::PaneId;
    use window::{
        IntegratedTitleButton, Modifiers, MouseButtons, MouseEvent, MouseEventKind, MousePress,
        WindowDecorations, WindowState,
    };

    fn drag_event(
        coords: (isize, isize),
        screen_coords: (isize, isize),
        window_origin: (isize, isize),
    ) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Move,
            coords: euclid::point2(coords.0, coords.1),
            screen_coords: euclid::point2(screen_coords.0, screen_coords.1),
            window_origin: euclid::point2(window_origin.0, window_origin.1),
            mouse_buttons: MouseButtons::LEFT,
            modifiers: Modifiers::NONE,
            platform_click_count: 1,
        }
    }

    #[test]
    fn manual_drag_applies_screen_delta_to_platform_origin() {
        // Window on a 1x display while the primary display is 2x: coords are
        // in 1x backing pixels but screen_coords are normalized to the 2x
        // primary scale, so origin inference from screen_coords - coords would
        // be off by the in-window click offset (#456). The platform-captured
        // origin must be used as-is.
        let start = drag_event((400, 20), (4000, 1040), (3200, 1000));
        let moved = drag_event((400, 20), (4030, 1100), (3200, 1000));
        assert_eq!(
            manual_drag_window_top_left(&start, &moved),
            euclid::point2(3230, 1060)
        );
    }

    #[test]
    fn manual_drag_without_motion_keeps_window_origin() {
        let start = drag_event((123, 45), (5000, 600), (700, 300));
        assert_eq!(
            manual_drag_window_top_left(&start, &start),
            euclid::point2(700, 300)
        );
    }

    #[test]
    fn option_click_never_crosses_a_hard_newline() {
        let rows = vec![
            OptionClickRowInfo {
                wrapped: false,
                cells: (0..5).map(|column| (column, 1)).collect(),
            },
            OptionClickRowInfo {
                wrapped: false,
                cells: (0..5).map(|column| (column, 1)).collect(),
            },
        ];

        assert!(option_click_cursor_bytes(&rows, 1, 2, 0, 2, false).is_empty());
        assert!(option_click_cursor_bytes(&rows, 0, 2, 1, 2, false).is_empty());
    }

    #[test]
    fn option_click_uses_horizontal_arrows_across_soft_wraps() {
        let rows = vec![
            OptionClickRowInfo {
                wrapped: true,
                cells: vec![(0, 1), (1, 2), (3, 1)],
            },
            OptionClickRowInfo {
                wrapped: false,
                cells: vec![(0, 1), (1, 1)],
            },
        ];

        assert_eq!(
            option_click_cursor_bytes(&rows, 0, 1, 1, 1, false),
            b"\x1b[C\x1b[C\x1b[C".to_vec()
        );
    }

    #[test]
    fn option_click_honors_application_cursor_keys() {
        let rows = vec![OptionClickRowInfo {
            wrapped: false,
            cells: (0..5).map(|column| (column, 1)).collect(),
        }];

        assert_eq!(
            option_click_cursor_bytes(&rows, 0, 1, 0, 3, true),
            b"\x1bOC\x1bOC".to_vec()
        );
        assert_eq!(
            option_click_cursor_bytes(&rows, 0, 3, 0, 1, true),
            b"\x1bOD\x1bOD".to_vec()
        );
    }

    #[test]
    fn manual_drag_ignores_window_backing_coords() {
        // The drag must be driven only by the screen-space delta and the
        // platform-captured origin; the in-window `coords` (window backing
        // scale) must never enter the math (#456). Vary `coords` wildly
        // between the two events while holding the screen delta fixed at
        // (30, 60): the result must be identical to the same-coords case.
        // This pins the invariant against a future change that reintroduces
        // a `coords` term into the origin formula, which the equal-coords
        // tests above cannot catch.
        let start = drag_event((400, 20), (4000, 1040), (3200, 1000));
        let moved = drag_event((999, 777), (4030, 1100), (3200, 1000));
        assert_eq!(
            manual_drag_window_top_left(&start, &moved),
            euclid::point2(3230, 1060)
        );
    }

    #[test]
    fn terminal_capture_keeps_release_routed_to_terminal() {
        assert_eq!(
            mouse_dispatch_target(
                true,
                0,
                24,
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
            ),
            MouseDispatchTarget::Terminal
        );
    }

    #[test]
    fn ui_item_wins_when_terminal_is_not_captured() {
        assert_eq!(
            mouse_dispatch_target(true, 0, 24, Some(&MouseCapture::UI)),
            MouseDispatchTarget::Ui
        );
    }

    #[test]
    fn title_area_wins_without_ui_or_terminal_capture() {
        assert_eq!(
            mouse_dispatch_target(false, 0, 24, None),
            MouseDispatchTarget::TitleArea
        );
    }

    #[test]
    fn title_area_double_click_zooms_instead_of_dragging() {
        assert!(should_zoom_title_area(
            WindowDecorations::INTEGRATED_BUTTONS | WindowDecorations::RESIZE,
            2,
        ));
        assert!(!should_zoom_title_area(
            WindowDecorations::INTEGRATED_BUTTONS | WindowDecorations::RESIZE,
            1,
        ));
    }

    #[test]
    fn macos_title_area_double_click_uses_appkit_zoom_toggle() {
        if cfg!(target_os = "macos") {
            assert_eq!(
                title_area_double_click_zoom_action(WindowState::empty()),
                TitleAreaZoomAction::Maximize
            );
            assert_eq!(
                title_area_double_click_zoom_action(WindowState::MAXIMIZED),
                TitleAreaZoomAction::Maximize
            );
            assert_eq!(
                title_area_double_click_zoom_action(WindowState::FULL_SCREEN),
                TitleAreaZoomAction::Restore
            );
        } else {
            assert_eq!(
                title_area_double_click_zoom_action(WindowState::MAXIMIZED),
                TitleAreaZoomAction::Restore
            );
        }
    }

    #[test]
    fn tab_bar_tabs_and_controls_do_not_start_window_drags() {
        assert!(!tab_bar_item_starts_window_drag(TabBarItem::Tab {
            tab_idx: 1,
            active: false,
        }));
        assert!(!tab_bar_item_starts_window_drag(TabBarItem::NewTabButton));
        assert!(!tab_bar_item_starts_window_drag(TabBarItem::WindowButton(
            IntegratedTitleButton::Close,
        )));
    }

    #[test]
    fn tab_bar_empty_and_status_regions_start_window_drags() {
        assert!(tab_bar_item_starts_window_drag(TabBarItem::None));
        assert!(tab_bar_item_starts_window_drag(TabBarItem::LeftStatus));
        assert!(tab_bar_item_starts_window_drag(TabBarItem::RightStatus));
    }

    #[test]
    fn macos_title_window_drag_uses_native_path() {
        if cfg!(target_os = "macos") {
            assert!(!should_use_manual_window_drag(WindowState::empty()));
            assert!(!should_use_manual_window_drag(WindowState::MAXIMIZED));
            assert!(!should_use_manual_window_drag(
                WindowState::MAXIMIZED | WindowState::FULL_SCREEN
            ));
        } else {
            assert!(!should_use_manual_window_drag(WindowState::empty()));
            assert!(!should_use_manual_window_drag(WindowState::MAXIMIZED));
        }
    }

    #[test]
    fn macos_maximized_drag_uses_native_window_drag() {
        if cfg!(target_os = "macos") {
            assert!(should_use_native_maximized_window_drag(
                WindowState::MAXIMIZED
            ));
            assert!(!should_use_native_maximized_window_drag(
                WindowState::empty()
            ));
            assert!(!should_use_native_maximized_window_drag(
                WindowState::MAXIMIZED | WindowState::FULL_SCREEN
            ));
        } else {
            assert!(!should_use_native_maximized_window_drag(
                WindowState::MAXIMIZED
            ));
        }
    }

    #[test]
    fn preserves_shift_wheel_reporting_for_tmux_alt_screen() {
        assert!(should_preserve_tmux_bypass_reporting(
            true,
            Modifiers::SHIFT,
            Modifiers::SHIFT,
            true,
            true,
            true,
        ));
    }

    #[test]
    fn does_not_preserve_when_tmux_is_not_grabbing_mouse() {
        assert!(!should_preserve_tmux_bypass_reporting(
            true,
            Modifiers::SHIFT,
            Modifiers::SHIFT,
            true,
            false,
            true,
        ));
    }

    #[test]
    fn bypasses_alt_screen_wheel_by_default() {
        assert!(should_bypass_wheel_assignment_in_alt(
            true, true, false, false,
        ));
    }

    #[test]
    fn forwards_alt_screen_wheel_when_terminal_scroll_is_enabled() {
        assert!(!should_bypass_wheel_assignment_in_alt(
            true, true, false, true,
        ));
    }

    #[test]
    fn does_not_bypass_wheel_when_alt_app_grabs_mouse() {
        assert!(!should_bypass_wheel_assignment_in_alt(
            true, true, true, false,
        ));
    }

    fn terminal_selection_buttons() -> Vec<MousePress> {
        vec![MousePress::Left]
    }

    #[test]
    fn default_behavior_extends_selection_during_terminal_drag() {
        let buttons = terminal_selection_buttons();
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
                &buttons,
                MouseButtons::LEFT,
                true,
                SelectionWheelScrollBehavior::default(),
            ),
            Some(SelectionDragWheelAction::ScrollAndExtend)
        );
    }

    #[test]
    fn ignore_behavior_suppresses_wheel_during_terminal_selection() {
        let buttons = terminal_selection_buttons();
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
                &buttons,
                MouseButtons::LEFT,
                true,
                SelectionWheelScrollBehavior::Ignore,
            ),
            Some(SelectionDragWheelAction::Suppress)
        );
    }

    #[test]
    fn scroll_only_behavior_scrolls_without_extending_selection() {
        let buttons = terminal_selection_buttons();
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
                &buttons,
                MouseButtons::LEFT,
                true,
                SelectionWheelScrollBehavior::ScrollOnly,
            ),
            Some(SelectionDragWheelAction::ScrollOnly)
        );
    }

    #[test]
    fn right_button_drag_routes_wheel_through_normal_path() {
        let buttons = vec![MousePress::Right];
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
                &buttons,
                MouseButtons::RIGHT,
                true,
                SelectionWheelScrollBehavior::Extend,
            ),
            None
        );
    }

    /// Regression guard for #455: a left press forwarded to a mouse-reporting
    /// application (claude code, vim) sets capture and button state but never
    /// matches a selection binding. Wheel events during that hold must route
    /// through the normal path instead of creating/extending a selection.
    #[test]
    fn forwarded_press_in_mouse_reporting_pane_does_not_extend_selection() {
        let buttons = terminal_selection_buttons();
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::TerminalPane(PaneId::new(1))),
                &buttons,
                MouseButtons::LEFT,
                false,
                SelectionWheelScrollBehavior::Extend,
            ),
            None
        );
    }

    #[test]
    fn ui_capture_routes_wheel_through_normal_path() {
        let buttons = terminal_selection_buttons();
        assert_eq!(
            wheel_during_terminal_selection_action(
                Some(&MouseCapture::UI),
                &buttons,
                MouseButtons::LEFT,
                true,
                SelectionWheelScrollBehavior::Extend,
            ),
            None
        );
    }

    #[test]
    fn no_capture_routes_wheel_through_normal_path() {
        let buttons: Vec<MousePress> = vec![];
        assert_eq!(
            wheel_during_terminal_selection_action(
                None,
                &buttons,
                MouseButtons::NONE,
                true,
                SelectionWheelScrollBehavior::Extend,
            ),
            None
        );
    }

    #[test]
    fn default_behavior_value_is_extend() {
        assert_eq!(
            SelectionWheelScrollBehavior::default(),
            SelectionWheelScrollBehavior::Extend
        );
    }

    #[test]
    fn bottom_tab_bar_touch_band_covers_gap_and_part_of_last_row() {
        // Terminal ends at 1000, 20px of padding, bar starts at 1020, 24px extension.
        let resolve = |y| bottom_tab_bar_touch_y(y, 1020, 1000, 24);
        assert_eq!(resolve(1019), Some(1020), "padding strip above the bar");
        assert_eq!(resolve(1000), Some(1020), "bottom edge of the last row");
        assert_eq!(resolve(976), Some(1020), "top of the extension");
        assert_eq!(resolve(975), None, "rest of the last row stays terminal");
        assert_eq!(resolve(1020), None, "the bar itself resolves directly");
        // Maximized: no padding strip, only the extension remains.
        assert_eq!(bottom_tab_bar_touch_y(976, 1000, 1000, 24), Some(1000));
        assert_eq!(bottom_tab_bar_touch_y(975, 1000, 1000, 24), None);
    }

    #[test]
    fn only_a_plain_left_press_takes_the_touch_extension() {
        assert!(press_takes_touch_extension(
            &MouseEventKind::Press(MousePress::Left),
            Modifiers::NONE
        ));
        assert!(press_takes_touch_extension(
            &MouseEventKind::Press(MousePress::Left),
            Modifiers::ENHANCED_KEY
        ));
        for press in [MousePress::Middle, MousePress::Right] {
            assert!(!press_takes_touch_extension(
                &MouseEventKind::Press(press),
                Modifiers::NONE
            ));
        }
        assert!(!press_takes_touch_extension(
            &MouseEventKind::Press(MousePress::Left),
            Modifiers::SUPER
        ));
        assert!(!press_takes_touch_extension(
            &MouseEventKind::Move,
            Modifiers::NONE
        ));
    }

    #[test]
    fn only_tabs_and_new_tab_button_take_the_touch_extension() {
        use crate::tabbar::TabBarItem;
        use crate::termwindow::UIItemType;
        assert!(tab_bar_item_takes_touch_extension(&UIItemType::TabBar(
            TabBarItem::Tab {
                tab_idx: 0,
                active: false
            }
        )));
        assert!(tab_bar_item_takes_touch_extension(&UIItemType::TabBar(
            TabBarItem::NewTabButton
        )));
        for item in [
            TabBarItem::None,
            TabBarItem::LeftStatus,
            TabBarItem::RightStatus,
        ] {
            assert!(!tab_bar_item_takes_touch_extension(&UIItemType::TabBar(
                item
            )));
        }
        assert!(!tab_bar_item_takes_touch_extension(
            &UIItemType::ScrollThumb
        ));
    }

    #[test]
    fn context_menus_offer_tab_lifecycle_with_confirmation() {
        use config::keyassignment::SpawnTabDomain;
        let new_tab = KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain);
        let close_tab = KeyAssignment::CloseCurrentTab { confirm: true };
        let terminal = terminal_context_menu_actions(false);
        assert!(terminal.contains(&("New Tab", new_tab.clone())));
        assert!(terminal.contains(&("Close Tab", close_tab.clone())));

        let tab = tab_context_menu_actions();
        assert_eq!(
            tab.iter().map(|(title, _)| *title).collect::<Vec<_>>(),
            ["New Tab", "Tab Navigator", "Close Tab"]
        );
        assert!(tab.contains(&("New Tab", new_tab)));
        assert!(tab.contains(&("Tab Navigator", KeyAssignment::ShowTabNavigator)));
        assert!(tab.contains(&("Close Tab", close_tab)));
    }

    #[test]
    fn context_menu_respects_mouse_reporting_bypass_configuration() {
        assert!(should_show_terminal_context_menu(
            &MouseEventKind::Press(MousePress::Right),
            Modifiers::ALT,
            Modifiers::ALT,
            true,
            false,
            false,
        ));
        assert!(!should_show_terminal_context_menu(
            &MouseEventKind::Press(MousePress::Right),
            Modifiers::SHIFT,
            Modifiers::ALT,
            true,
            false,
            false,
        ));
    }

    #[test]
    fn context_menu_never_overrides_capture_or_custom_mouse_assignment() {
        let right_press = MouseEventKind::Press(MousePress::Right);
        assert!(!should_show_terminal_context_menu(
            &right_press,
            Modifiers::NONE,
            Modifiers::SHIFT,
            false,
            true,
            false,
        ));
        assert!(!should_show_terminal_context_menu(
            &right_press,
            Modifiers::NONE,
            Modifiers::SHIFT,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn context_menu_offers_copy_only_with_selection() {
        let copy = KeyAssignment::CopyTo(ClipboardCopyDestination::Clipboard);
        let with_selection = terminal_context_menu_actions(true);
        assert_eq!(with_selection.first(), Some(&("Copy", copy.clone())));
        assert!(!terminal_context_menu_actions(false)
            .iter()
            .any(|(_, action)| *action == copy));
    }

    #[test]
    fn context_menu_close_uses_confirmation_policy() {
        let close = terminal_context_menu_actions(false)
            .into_iter()
            .find(|(title, _)| *title == "Close Pane")
            .map(|(_, action)| action);

        assert_eq!(
            close,
            Some(KeyAssignment::CloseCurrentPane { confirm: true })
        );
    }

    #[test]
    fn context_menu_dispatch_retains_stable_pane_id() {
        // Inspect production code only so these assertions cannot satisfy themselves.
        let mouse_source = include_str!("mouseevent.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("mouse event production source");
        let window_source = include_str!("mod.rs");

        assert!(mouse_source
            .contains("item.set_represented_item(RepresentedItem::KeyAssignmentForPane {"));
        assert!(window_source
            .contains("WindowEvent::PerformKeyAssignmentForPane { action, pane_id } =>"));
        assert!(window_source.contains("if mux.focus_pane_and_containing_tab(pane_id).is_err() {"));
    }
}
