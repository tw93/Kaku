use crate::tabbar::TabBarItem;
use crate::termwindow::tab_rename::TabRenameModal;
use crate::termwindow::{
    GuiWin, MouseCapture, PositionedSplit, ScrollHit, TermWindowNotif, UIItem, UIItemType, TMB,
};
use ::window::{
    MouseButtons as WMB, MouseCursor, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps,
    WindowState,
};
use config::keyassignment::{KeyAssignment, MouseEventTrigger, SpawnTabDomain};
use config::MouseEventAltScreen;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseDispatchTarget {
    Ui,
    TitleArea,
    Terminal,
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

fn should_zoom_title_area(
    window_decorations: window::WindowDecorations,
    click_streak: Option<usize>,
) -> bool {
    window_decorations
        == (window::WindowDecorations::INTEGRATED_BUTTONS | window::WindowDecorations::RESIZE)
        && click_streak == Some(2)
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

impl super::TermWindow {
    const TAB_DRAG_THRESHOLD: isize = 6;

    fn finish_mouse_release(&mut self, press: MousePress) {
        self.current_mouse_capture = None;
        self.current_mouse_buttons.retain(|p| p != &press);
    }

    fn start_tab_drag(&mut self, tab_idx: usize, start_event: MouseEvent) {
        self.tab_drag_state = Some(super::TabDragState {
            tab_idx,
            start_event,
            has_dragged: false,
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

        let target_idx = self.drag_tab_target_idx(state.tab_idx, event.coords.x);

        if let Some(target_idx) = target_idx {
            if target_idx != state.tab_idx {
                if let Err(err) = self.move_tab(target_idx) {
                    log::debug!("move_tab({target_idx}) failed while dragging tab: {err:#}");
                } else {
                    state.tab_idx = target_idx;
                    context.invalidate();
                }
            }
        }

        self.tab_drag_state = Some(state);
        true
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
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        self.current_mouse_event.replace(event.clone());
        self.update_scrollbar_hovering(&pane, context);
        // Mouse interaction should cancel any synthetic prompt-selection state
        // tracked from keyboard shortcuts (Cmd+A/Shift+Arrow, etc).
        self.clear_line_editor_selection();

        let border = self.get_os_border();

        let first_line_offset = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height().unwrap_or(0.) as isize
        } else {
            0
        } + border.top.get() as isize;

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

        self.last_mouse_coords = (x, y);

        // Keep modal focus exclusive: forward all mouse events to it and stop
        // routing into pane/tab UI while active.
        if let Some(modal) = self.get_modal() {
            let modal_event = wezterm_term::MouseEvent {
                kind: match event.kind {
                    WMEK::Move => TMEK::Move,
                    WMEK::VertWheel(_) | WMEK::HorzWheel(_) | WMEK::Press(_) => TMEK::Press,
                    WMEK::Release(_) => TMEK::Release,
                },
                button: match event.kind {
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
                },
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
                if press == &MousePress::Left && self.edge_drag_in_progress {
                    self.edge_drag_in_progress = false;
                    self.finish_mouse_release(*press);
                    return;
                }
                if press == &MousePress::Left {
                    let was_dragging_window = self.is_window_dragging;
                    self.is_window_dragging = false;
                    let had_manual_drag_anchor = self.window_drag_position.take().is_some();
                    if had_manual_drag_anchor || was_dragging_window {
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
                if press == &MousePress::Left && self.tab_drag_state.take().is_some() {
                    self.finish_mouse_release(*press);
                    return;
                }
            }

            WMEK::Press(ref press) => {
                // If a previous edge drag never received its Release, reset now.
                self.edge_drag_in_progress = false;
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

                let click = match self.last_mouse_click.take() {
                    None => LastMouseClick::new(button, click_position),
                    Some(click) => click.add(button, click_position),
                };
                self.last_mouse_click = Some(click);
                self.current_mouse_buttons.retain(|p| p != press);
                self.current_mouse_buttons.push(*press);

                if press == &MousePress::Left
                    && first_line_offset > 0
                    && (event.coords.y as usize) < first_line_offset as usize
                {
                    // A left press in the title/tab strip may turn into a native
                    // window drag. Enter drag-protection immediately so we don't
                    // route follow-up motion/wheel into terminal selection/scroll.
                    self.current_mouse_capture = Some(MouseCapture::UI);
                    self.is_window_dragging = true;
                }
            }

            WMEK::Move => {
                if self.edge_drag_in_progress {
                    return;
                }
                if let Some(start) = self.window_drag_position.clone() {
                    if event.mouse_buttons != WMB::LEFT {
                        self.window_drag_position = None;
                        self.is_window_dragging = false;
                        self.current_mouse_capture = None;
                    } else {
                        // Dragging the window
                        // Compute the distance since the initial event
                        let delta_x = start.screen_coords.x - event.screen_coords.x;
                        let delta_y = start.screen_coords.y - event.screen_coords.y;

                        // Now compute a new window position.
                        // We don't have a direct way to get the position,
                        // but we can infer it by comparing the mouse coords
                        // with the screen coords in the initial event.
                        // This computes the original top_left position,
                        // and applies the total drag delta to it.
                        let top_left = ::window::ScreenPoint::new(
                            (start.screen_coords.x - start.coords.x) - delta_x,
                            (start.screen_coords.y - start.coords.y) - delta_y,
                        );
                        // and now tell the window to go there
                        context.set_window_position(top_left);
                        return;
                    }
                }
                if self.is_window_dragging {
                    if event.mouse_buttons == WMB::NONE {
                        // Defensive reset in case release was consumed by native drag.
                        self.is_window_dragging = false;
                        self.current_mouse_capture = None;
                    } else {
                        // We requested a native drag move; while it is active,
                        // suppress terminal mouse handling to avoid accidental scrolling.
                        return;
                    }
                }
                if event.mouse_buttons != WMB::NONE
                    && self.current_mouse_buttons.is_empty()
                    && self.current_mouse_capture.is_none()
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
            }
            WMEK::VertWheel(_) | WMEK::HorzWheel(_) => {
                if self.is_window_dragging {
                    return;
                }
                if event.mouse_buttons != WMB::NONE
                    && !matches!(
                        self.current_mouse_capture,
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

        let prior_ui_item = self.last_ui_item.clone();

        let ui_item = if matches!(self.current_mouse_capture, None | Some(MouseCapture::UI)) {
            let ui_item = self.resolve_ui_item(&event);

            match (self.last_ui_item.take(), &ui_item) {
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
            self.current_mouse_capture.as_ref(),
        ) {
            MouseDispatchTarget::Ui => {
                let item = ui_item
                    .clone()
                    .expect("ui item must exist when dispatching to UI");
                if capture_mouse {
                    self.current_mouse_capture = Some(MouseCapture::UI);
                }
                self.mouse_event_ui_item(item, pane, y, event, context);
            }
            MouseDispatchTarget::TitleArea => {
                // Event landed in title/padding area above terminal content but missed all UI items.
                match event.kind {
                    WMEK::Press(MousePress::Left) => {
                        let maximized = self
                            .window_state
                            .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN);
                        // Double-click title area to zoom window
                        if self.last_mouse_click.as_ref().map(|c| c.streak) == Some(2) {
                            if let Some(ref window) = self.window {
                                if maximized {
                                    window.restore();
                                } else {
                                    window.maximize();
                                }
                            }
                            return;
                        }
                        self.current_mouse_capture = Some(MouseCapture::UI);
                        self.is_window_dragging = true;
                        if !maximized && !cfg!(target_os = "macos") {
                            self.window_drag_position.replace(event.clone());
                        }
                        context.request_drag_move();
                        return;
                    }
                    WMEK::Move if self.current_mouse_capture.is_none() => {
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

        if prior_ui_item != ui_item && !self.is_window_dragging {
            self.update_title_post_status();
        }
    }

    pub fn mouse_leave_impl(&mut self, context: &dyn WindowOps) {
        self.current_mouse_event = None;
        self.scrollbar_hovering = false;
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
                self.drag_split(item, split, start_event, x, y, context);
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
        self.last_ui_item.replace(item.clone());
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
                self.close_specific_tab(idx, false);
            }
            _ => {}
        }
        context.set_cursor(Some(MouseCursor::Arrow));
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
            WMEK::Press(MousePress::Left) => match item {
                TabBarItem::Tab { tab_idx, active } => {
                    if self.last_mouse_click.as_ref().map(|c| c.streak) == Some(2) {
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
                    let maximized = self
                        .window_state
                        .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN);
                    if let Some(ref window) = self.window {
                        if should_zoom_title_area(
                            self.config.window_decorations,
                            self.last_mouse_click.as_ref().map(|c| c.streak),
                        ) {
                            if maximized {
                                window.restore();
                            } else {
                                window.maximize();
                            }
                            return;
                        }
                    }
                    self.is_window_dragging = true;
                    if !maximized && !cfg!(target_os = "macos") {
                        self.window_drag_position.replace(event.clone());
                    }
                    context.request_drag_move();
                }
                TabBarItem::WindowButton(button) => {
                    self.tab_drag_state = None;
                    use window::IntegratedTitleButton as Button;
                    if let Some(ref window) = self.window {
                        match button {
                            Button::Hide => window.hide(),
                            Button::Maximize => {
                                let maximized = self
                                    .window_state
                                    .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN);
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
            },
            WMEK::Press(MousePress::Middle) => match item {
                TabBarItem::Tab { tab_idx, .. } => {
                    self.tab_drag_state = None;
                    self.close_specific_tab(tab_idx, false);
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
                TabBarItem::Tab { .. } => {
                    self.tab_drag_state = None;
                    self.show_tab_navigator();
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
                    let item = self.last_ui_item.clone().unwrap();
                    let bounds: ::window::ScreenRect = euclid::rect(
                        item.x as isize - (event.coords.x as isize - event.screen_coords.x),
                        item.y as isize - (event.coords.y as isize - event.screen_coords.y),
                        item.width as isize,
                        item.height as isize,
                    );
                    context.set_maximize_button_position(bounds);
                }
                TabBarItem::WindowButton(_)
                | TabBarItem::Tab { .. }
                | TabBarItem::NewTabButton { .. } => {}
            },
            WMEK::VertWheel(n) => {
                if self.config.mouse_wheel_scrolls_tabs {
                    if let Err(err) = self.activate_tab_relative(if n < 1 { 1 } else { -1 }, true) {
                        log::debug!("activate_tab_relative on wheel failed: {err:#}");
                    }
                }
            }
            _ => {}
        }
        context.set_cursor(Some(MouseCursor::Arrow));
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
            self.current_mouse_capture,
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
            self.current_mouse_capture = Some(MouseCapture::TerminalPane(pane.pane_id()));
        }

        if matches!(event.kind, WMEK::Press(MousePress::Left)) && in_resize_zone {
            self.edge_drag_in_progress = true;
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
                self.is_click_to_focus_window = true;
                context.invalidate();
                log::trace!("enter click to focus");
                return;
            }
        }
        if self.is_click_to_focus_window && matches!(&event.kind, WMEK::Release(_)) {
            // Exiting click to focus state
            self.is_click_to_focus_window = false;
            context.invalidate();
            log::trace!("exit click to focus");
            return;
        }

        let allow_action = if self.is_click_to_focus_window || !is_focused {
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
            || self.edge_drag_in_progress
        {
            MouseCursor::Arrow
        } else {
            MouseCursor::Text
        }));

        let event_trigger_type = match &event.kind {
            WMEK::Press(press) => {
                let press = mouse_press_to_tmb(press);
                match self.last_mouse_click.as_ref() {
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
                match self.last_mouse_click.as_ref() {
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
                if !self.current_mouse_buttons.is_empty() {
                    if let Some(LastMouseClick { streak, button, .. }) =
                        self.last_mouse_click.as_ref()
                    {
                        if Some(*button)
                            == self.current_mouse_buttons.last().map(mouse_press_to_tmb)
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
        let bypass_wheel_assignment_in_alt =
            is_wheel_event && pane.is_alt_screen_active() && !pane.is_mouse_grabbed();
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

        if allow_action && !self.edge_drag_in_progress && !bypass_wheel_assignment_in_alt {
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

                if let Some(action) = self.input_map.lookup_mouse(event_trigger_type, mouse_mods) {
                    if let Err(err) = self.perform_key_assignment(&pane, &action) {
                        log::debug!("mouse assignment failed: {err:#}");
                    }
                    return;
                }
            }
        }

        let mouse_event = wezterm_term::MouseEvent {
            kind: match event.kind {
                WMEK::Move => TMEK::Move,
                WMEK::VertWheel(_) | WMEK::HorzWheel(_) | WMEK::Press(_) => TMEK::Press,
                WMEK::Release(_) => TMEK::Release,
            },
            button: match event.kind {
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
            },
            x: column,
            y: row,
            x_pixel_offset,
            y_pixel_offset,
            modifiers: event.modifiers,
        };

        if allow_action
            && !self.edge_drag_in_progress
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

#[cfg(test)]
mod tests {
    use super::{
        mouse_dispatch_target, should_preserve_tmux_bypass_reporting, should_zoom_title_area,
        MouseDispatchTarget,
    };
    use crate::termwindow::MouseCapture;
    use mux::pane::PaneId;
    use window::{Modifiers, WindowDecorations};

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
            Some(2),
        ));
        assert!(!should_zoom_title_area(
            WindowDecorations::INTEGRATED_BUTTONS | WindowDecorations::RESIZE,
            Some(1),
        ));
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
}
