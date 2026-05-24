use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::utilsprites::RenderMetrics;
use ::window::{Dimensions, ResizeIncrement, Window, WindowOps, WindowState};
use config::{Config, ConfigHandle, DimensionContext};
use mux::Mux;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use wezterm_font::FontConfiguration;
use wezterm_term::TerminalSize;

const FONT_SCALE_PTY_RESIZE_DEBOUNCE: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowsAndCols {
    pub rows: usize,
    pub cols: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeferredFontScalePtyResize {
    epoch: u64,
    last_cells: RowsAndCols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredFontScalePtyResizeAction {
    Stale,
    Remove,
    Keep(DeferredFontScalePtyResize),
}

static DEFERRED_FONT_SCALE_PTY_RESIZES: OnceLock<
    Mutex<HashMap<usize, DeferredFontScalePtyResize>>,
> = OnceLock::new();

fn deferred_font_scale_pty_resizes() -> &'static Mutex<HashMap<usize, DeferredFontScalePtyResize>> {
    DEFERRED_FONT_SCALE_PTY_RESIZES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mark_deferred_font_scale_pty_resize(
    window_id: usize,
    current_cells: RowsAndCols,
) -> DeferredFontScalePtyResize {
    let mut deferred = deferred_font_scale_pty_resizes().lock().unwrap();
    let prior = deferred.get(&window_id).copied();
    let state = DeferredFontScalePtyResize {
        epoch: prior.map(|state| state.epoch.wrapping_add(1)).unwrap_or(1),
        last_cells: current_cells,
    };
    deferred.insert(window_id, state);
    state
}

fn has_deferred_font_scale_pty_resize(window_id: usize) -> bool {
    deferred_font_scale_pty_resizes()
        .lock()
        .unwrap()
        .contains_key(&window_id)
}

fn finish_deferred_font_scale_pty_resize(
    window_id: usize,
    epoch: u64,
    current_cells: RowsAndCols,
) -> DeferredFontScalePtyResizeAction {
    let mut deferred = deferred_font_scale_pty_resizes().lock().unwrap();
    match deferred.get(&window_id).copied() {
        Some(state) if state.epoch == epoch => {
            if state.last_cells != current_cells {
                let next = DeferredFontScalePtyResize {
                    epoch: state.epoch.wrapping_add(1),
                    last_cells: current_cells,
                };
                deferred.insert(window_id, next);
                DeferredFontScalePtyResizeAction::Keep(next)
            } else {
                deferred.remove(&window_id);
                DeferredFontScalePtyResizeAction::Remove
            }
        }
        _ => DeferredFontScalePtyResizeAction::Stale,
    }
}

#[cfg(test)]
fn clear_deferred_font_scale_pty_resize_for_test(window_id: usize) {
    deferred_font_scale_pty_resizes()
        .lock()
        .unwrap()
        .remove(&window_id);
}

fn schedule_deferred_font_scale_pty_resize_epoch(window: &Window, epoch: u64) {
    let window = window.clone();
    promise::spawn::spawn_into_main_thread(async move {
        smol::Timer::after(FONT_SCALE_PTY_RESIZE_DEBOUNCE).await;
        window.notify(super::TermWindowNotif::Apply(Box::new(move |tw| {
            tw.flush_deferred_font_scale_pty_resize(epoch);
        })));
    })
    .detach();
}

#[derive(Debug)]
pub enum ScaleChange {
    Absolute(f64),
    Relative(f64),
}

fn should_normalize_fullscreen_state_on_resize(
    tab_bar_at_bottom: bool,
    was_fullscreen: bool,
    incoming_fullscreen: bool,
    is_focused: bool,
    dimensions_unchanged: bool,
) -> bool {
    !tab_bar_at_bottom
        && was_fullscreen
        && !incoming_fullscreen
        && (!is_focused || dimensions_unchanged)
}

fn should_rebalance_top_tab_visible_bottom_gap(
    user_has_custom_padding: bool,
    show_tab_bar: bool,
    tab_bar_at_bottom: bool,
    is_fullscreen: bool,
) -> bool {
    !user_has_custom_padding && show_tab_bar && !tab_bar_at_bottom && !is_fullscreen
}

fn should_rebalance_bottom_tab_quantization_slack(
    user_has_custom_padding: bool,
    tab_bar_at_bottom: bool,
) -> bool {
    !user_has_custom_padding && tab_bar_at_bottom
}

fn should_rebalance_maximized_quantization_slack(
    user_has_custom_padding: bool,
    uses_edge_to_edge_padding: bool,
) -> bool {
    // When maximized (edge-to-edge padding), absorb any quantization slack
    // into top padding so terminal content fills to the bottom edge.
    // This applies regardless of tab bar configuration.
    !user_has_custom_padding && uses_edge_to_edge_padding
}

fn rebalance_top_padding_for_bottom_gap(
    padding_top: usize,
    padding_bottom: usize,
    row_quantization_slack: usize,
    max_bottom_gap: usize,
) -> (usize, usize) {
    let bottom_gap = padding_bottom.saturating_add(row_quantization_slack);
    if bottom_gap <= max_bottom_gap {
        return (padding_top, 0);
    }

    let shift = bottom_gap - max_bottom_gap;
    (padding_top.saturating_add(shift), shift)
}

fn should_preserve_terminal_cells_on_scale_change(
    font_scale_changed: bool,
    simple_dpi_change: bool,
    allow_terminal_size_preservation: bool,
) -> bool {
    font_scale_changed || (simple_dpi_change && allow_terminal_size_preservation)
}

fn should_defer_screen_change_scale_update(
    live_resizing: bool,
    screen_changed: bool,
    has_pending_screen_change_resize: bool,
    current_dpi: usize,
    incoming_dpi: usize,
) -> bool {
    live_resizing
        && incoming_dpi != current_dpi
        && (screen_changed || has_pending_screen_change_resize)
}

impl super::TermWindow {
    pub fn resize(
        &mut self,
        dimensions: Dimensions,
        window_state: WindowState,
        window: &Window,
        live_resizing: bool,
        screen_changed: bool,
    ) {
        log::trace!(
            "resize event, live={} screen_changed={} current cells: {:?}, current dims: {:?}, new dims: {:?} window_state:{:?}",
            live_resizing,
            screen_changed,
            self.current_cell_dimensions(),
            self.dimensions,
            dimensions,
            window_state,
        );
        if dimensions.pixel_width == 0 || dimensions.pixel_height == 0 {
            // on windows, this can happen when minimizing the window.
            // NOP!
            log::trace!("new dimensions are zero: NOP!");
            return;
        }
        let mut normalized_window_state = window_state;
        if live_resizing
            && self.pending_screen_change_resize
            && dimensions.dpi == self.dimensions.dpi
        {
            // The user dragged back onto the original screen before the live resize
            // ended, so the deferred DPI transition is no longer relevant.
            self.pending_screen_change_resize = false;
        }
        let dimensions_unchanged = dimensions == self.dimensions;
        let was_fullscreen = self.window_state.contains(WindowState::FULL_SCREEN);
        let incoming_fullscreen = normalized_window_state.contains(WindowState::FULL_SCREEN);
        let is_focused = self.focused.is_some();
        if should_normalize_fullscreen_state_on_resize(
            self.config.tab_bar_at_bottom,
            was_fullscreen,
            incoming_fullscreen,
            is_focused,
            dimensions_unchanged,
        ) {
            // During macOS Space/focus transitions, fullscreen can be reported as
            // false for a transient resize callback even though the window size
            // did not change. Keep fullscreen sticky for this frame to avoid a
            // one-frame layout jump. We also keep it sticky while unfocused to
            // absorb cases where AppKit reports a transient geometry drift.
            normalized_window_state |= WindowState::FULL_SCREEN;
            self.arm_layout_sticky_fullscreen();
        }

        if dimensions_unchanged && self.window_state == normalized_window_state {
            // Even if the geometry didn't change, live resize state transitions
            // still matter for flushing deferred work.
            let was_live_resizing = self.live_resizing;
            self.live_resizing = live_resizing;

            if was_live_resizing && !live_resizing {
                self.flush_pending_pty_resize();
                if self.pending_config_reload_after_resize {
                    self.pending_config_reload_after_resize = false;
                    self.schedule_silent_config_reload(window);
                }
                self.emit_window_event("window-resized", None);
            }

            log::trace!("dimensions/window_state didn't change NOP!");
            return;
        }
        let last_state = self.window_state;
        self.window_state = normalized_window_state;
        self.live_resizing = live_resizing;
        self.quad_generation += 1;
        // Refresh per-screen OS parameters (eg: safe-area/border metrics)
        // on each resize so dragging between monitors doesn't use stale values.
        self.load_os_parameters();
        self.sync_tab_bar_visibility_for_window_state("resize:sync_tab_bar");
        let fullscreen_transition = last_state.contains(WindowState::FULL_SCREEN)
            != self.window_state.contains(WindowState::FULL_SCREEN);
        let maximize_transition = last_state.contains(WindowState::MAXIMIZED)
            != self.window_state.contains(WindowState::MAXIMIZED);

        if let Some(webgpu) = self.webgpu() {
            webgpu.resize(dimensions);
        }

        if should_defer_screen_change_scale_update(
            live_resizing,
            screen_changed,
            self.pending_screen_change_resize,
            self.dimensions.dpi,
            dimensions.dpi,
        ) {
            log::trace!(
                "deferring cross-screen dpi update until live resize ends: current_dpi={} incoming_dpi={} dims={:?}",
                self.dimensions.dpi,
                dimensions.dpi,
                dimensions,
            );
            self.pending_screen_change_resize = true;

            let mut stabilized = dimensions;
            stabilized.dpi = self.dimensions.dpi;
            self.apply_dimensions(
                &stabilized,
                Some(self.current_cell_dimensions()),
                window,
                false,
            );
        } else if !live_resizing
            && self.pending_screen_change_resize
            && dimensions.dpi != self.dimensions.dpi
        {
            log::trace!(
                "committing deferred cross-screen dpi update after live resize: current_dpi={} incoming_dpi={} dims={:?}",
                self.dimensions.dpi,
                dimensions.dpi,
                dimensions,
            );
            self.pending_screen_change_resize = false;
            self.scaling_changed(dimensions, self.fonts.get_font_scale(), window, false);

        // Align fullscreen transition handling with maximize/restore behavior:
        // keep current dpi for this transition frame so text doesn't pop larger/smaller.
        } else if (fullscreen_transition || maximize_transition)
            && self.dimensions.dpi != dimensions.dpi
        {
            let mut stabilized = dimensions;
            stabilized.dpi = self.dimensions.dpi;
            self.apply_dimensions(&stabilized, None, window, true);
            // Force a render_metrics refresh against the stabilized dpi so a
            // later fullscreen / maximize transition on an external display
            // can't leave cell_size stale (mouse y → row off-by-N selection
            // on external monitors, #312/#356).
            self.apply_scale_change(&stabilized, self.fonts.get_font_scale());
        } else if live_resizing && self.dimensions.dpi == dimensions.dpi {
            // For simple, user-interactive resizes where the dpi doesn't change,
            // skip our scaling recalculation.
            self.apply_dimensions(&dimensions, None, window, true);
        } else {
            self.scaling_changed(
                dimensions,
                self.fonts.get_font_scale(),
                window,
                !screen_changed,
            );
        }
        if let Some(modal) = self.get_modal() {
            modal.reconfigure(self);
        }
        if !live_resizing {
            self.flush_pending_pty_resize();
            if self.pending_config_reload_after_resize {
                self.pending_config_reload_after_resize = false;
                self.schedule_silent_config_reload(window);
            }
            self.emit_window_event("window-resized", None);
        }
    }

    fn flush_all_pane_pty_sizes(&mut self) {
        let mux = Mux::get();
        if let Some(window) = mux.get_window(self.mux_window_id) {
            for tab in window.iter() {
                tab.flush_pane_pty_sizes();
            }
        };
    }

    fn flush_pending_pty_resize(&mut self) {
        if !self.pending_pty_flush_after_resize {
            return;
        }
        self.pending_pty_flush_after_resize = false;
        self.flush_all_pane_pty_sizes();
    }

    fn schedule_deferred_font_scale_pty_resize(&mut self, window: &Window) {
        let state =
            mark_deferred_font_scale_pty_resize(self.mux_window_id, self.current_cell_dimensions());
        schedule_deferred_font_scale_pty_resize_epoch(window, state.epoch);
    }

    fn flush_deferred_font_scale_pty_resize(&mut self, epoch: u64) {
        match finish_deferred_font_scale_pty_resize(
            self.mux_window_id,
            epoch,
            self.current_cell_dimensions(),
        ) {
            DeferredFontScalePtyResizeAction::Keep(next) => {
                if let Some(window) = self.window.as_ref() {
                    schedule_deferred_font_scale_pty_resize_epoch(window, next.epoch);
                }
            }
            DeferredFontScalePtyResizeAction::Remove | DeferredFontScalePtyResizeAction::Stale => {}
        }
    }

    pub fn apply_pending_scale_changes(&mut self) {
        while self.resizes_pending == 0 {
            match self.pending_scale_changes.pop_front() {
                Some(ScaleChange::Relative(change)) => {
                    if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
                        self.adjust_font_scale(self.fonts.get_font_scale() * change, &window);
                    }
                }
                Some(ScaleChange::Absolute(change)) => {
                    if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
                        self.adjust_font_scale(change, &window);
                    }
                }
                None => break,
            }
        }
    }

    pub fn apply_scale_change(&mut self, dimensions: &Dimensions, font_scale: f64) {
        let config = &self.config;
        let font_size = config.font_size * font_scale;
        let theoretical_height = font_size * dimensions.dpi as f64 / 72.0;

        if theoretical_height < 2.0 {
            log::warn!(
                "refusing to go to an unreasonably small font scale {:?}
                       font_scale={} would yield font_height {}",
                dimensions,
                font_scale,
                theoretical_height
            );
            return;
        }

        // change_scaling below clears the font/metrics caches unconditionally,
        // forcing a ~330ms default-font reresolve on macOS.
        if font_scale == self.fonts.get_font_scale() && dimensions.dpi == self.fonts.get_dpi() {
            return;
        }

        let (prior_font, prior_dpi) = self.fonts.change_scaling(font_scale, dimensions.dpi);
        match RenderMetrics::new(&self.fonts) {
            Ok(metrics) => {
                self.render_metrics = metrics;
            }
            Err(err) => {
                log::error!(
                    "{:#} while attempting to scale font to {} with {:?}",
                    err,
                    font_scale,
                    dimensions
                );
                // Restore prior scaling factors
                self.fonts.change_scaling(prior_font, prior_dpi);
            }
        }

        if let Err(err) = self.recreate_texture_atlas(None) {
            log::error!("recreate_texture_atlas: {:#}", err);
        }
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
    }

    pub fn apply_dimensions(
        &mut self,
        dimensions: &Dimensions,
        mut scale_changed_cells: Option<RowsAndCols>,
        window: &Window,
        allow_speculative_window_resize: bool,
    ) {
        log::trace!(
            "apply_dimensions {:?} scale_changed_cells {:?} allow_speculative_window_resize={}. window_state {:?}",
            dimensions,
            scale_changed_cells,
            allow_speculative_window_resize,
            self.window_state
        );
        let saved_dims = self.dimensions;
        self.dimensions = *dimensions;
        self.quad_generation += 1;

        if scale_changed_cells.is_some() && !self.window_state.can_resize() {
            log::warn!(
                "cannot resize window to match {:?} because window_state is {:?}",
                scale_changed_cells,
                self.window_state
            );
            scale_changed_cells.take();
        }

        // Technically speaking, we should compute the rows and cols
        // from the new dimensions and apply those to the tabs, and
        // then for the scaling changed case, try to re-apply the
        // original rows and cols, but if we do that we end up
        // double resizing the tabs, so we speculatively apply the
        // final size, which in that case should result in a NOP
        // change to the tab size.

        let config = &self.config;
        let uses_edge_to_edge_padding = self.layout_uses_edge_to_edge_padding();
        let user_has_custom_padding = user_has_custom_window_padding_assignment();

        let tab_bar_height = if self.show_tab_bar {
            // Use the real height only when title_font is already cached;
            // fall back to the estimated value (cell_height * 1.75) to avoid
            // triggering the expensive ~480ms CoreText first-load on the
            // synthetic resize event that fires inside Window::new_window.
            if self.fonts.is_title_font_loaded() {
                self.tab_bar_pixel_height().unwrap_or(0.)
            } else {
                Self::estimated_tab_bar_pixel_height(&self.config, &self.render_metrics)
            }
        } else {
            0.
        };
        let tab_bar_height_px = tab_bar_height as usize;

        let border = self.get_os_border();

        let (size, dims, ri_calc) = if let Some(cell_dims) = scale_changed_cells {
            // Scaling preserves existing terminal dimensions, yielding a new
            // overall set of window dimensions
            let size = TerminalSize {
                rows: cell_dims.rows,
                cols: cell_dims.cols,
                pixel_height: cell_dims.rows * self.render_metrics.cell_size.height as usize,
                pixel_width: cell_dims.cols * self.render_metrics.cell_size.width as usize,
                dpi: dimensions.dpi as u32,
            };

            let rows = size.rows;
            let cols = size.cols;

            let h_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: size.pixel_width as f32,
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
            let (padding_top, padding_bottom) = effective_vertical_padding_with_policy(
                config,
                v_context,
                self.show_tab_bar,
                self.config.tab_bar_at_bottom,
                tab_bar_height_px,
                uses_edge_to_edge_padding,
                user_has_custom_padding,
            );
            let padding_right = effective_right_padding(&config, h_context);

            let pixel_height = (rows * self.render_metrics.cell_size.height as usize)
                + (padding_top + padding_bottom)
                + (border.top + border.bottom).get() as usize
                + tab_bar_height as usize;

            let pixel_width = (cols * self.render_metrics.cell_size.width as usize)
                + (padding_left + padding_right)
                + (border.left + border.right).get() as usize;

            let dims = Dimensions {
                pixel_width: pixel_width as usize,
                pixel_height: pixel_height as usize,
                dpi: dimensions.dpi,
            };

            let ri_calc = ResizeIncrementCalculator {
                x: self.render_metrics.cell_size.width as u16,
                y: self.render_metrics.cell_size.height as u16,
                padding_left: padding_left,
                padding_top: padding_top,
                padding_right: padding_right,
                padding_bottom: padding_bottom,
                border: border,
                tab_bar_height: tab_bar_height as usize,
            };

            (size, dims, ri_calc)
        } else {
            // Resize of the window dimensions may result in changed terminal dimensions

            let h_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_width as f32,
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
            let (mut padding_top, mut padding_bottom) = effective_vertical_padding_with_policy(
                config,
                v_context,
                self.show_tab_bar,
                self.config.tab_bar_at_bottom,
                tab_bar_height_px,
                uses_edge_to_edge_padding,
                user_has_custom_padding,
            );
            let padding_right = effective_right_padding(&config, h_context);

            // When the AI chat overlay is active on the focused pane, shrink
            // the bottom padding so the chat box sits closer to the window /
            // tab-bar edge. This only affects layout while the overlay is open;
            // closing it triggers a resize that restores the normal padding.
            if self.has_ai_chat_overlay_on_active_pane() {
                let chat_bottom = if self.show_tab_bar { 0 } else { 4 };
                padding_bottom = padding_bottom.min(chat_bottom);
            }

            let avail_width = dimensions.pixel_width.saturating_sub(
                (padding_left + padding_right) as usize
                    + (border.left + border.right).get() as usize,
            );
            let avail_height = dimensions
                .pixel_height
                .saturating_sub(
                    (padding_top + padding_bottom) as usize
                        + (border.top + border.bottom).get() as usize,
                )
                .saturating_sub(tab_bar_height as usize);

            let cell_height = self.render_metrics.cell_size.height as usize;
            let rows = avail_height / cell_height;
            let cols = avail_width / self.render_metrics.cell_size.width as usize;

            let row_quantization_slack = avail_height.saturating_sub(rows * cell_height);

            if should_rebalance_maximized_quantization_slack(
                user_has_custom_padding,
                uses_edge_to_edge_padding,
            ) {
                // Maximized windows use edge-to-edge layout: absorb all slack into top.
                padding_top = padding_top.saturating_add(row_quantization_slack);
            } else if should_rebalance_top_tab_visible_bottom_gap(
                user_has_custom_padding,
                self.show_tab_bar,
                self.config.tab_bar_at_bottom,
                self.layout_is_effective_fullscreen(),
            ) {
                let (rebalanced_top, _) = rebalance_top_padding_for_bottom_gap(
                    padding_top,
                    padding_bottom,
                    row_quantization_slack,
                    TOP_TAB_VISIBLE_BOTTOM_GAP,
                );
                padding_top = rebalanced_top;
            } else if should_rebalance_bottom_tab_quantization_slack(
                user_has_custom_padding,
                self.config.tab_bar_at_bottom,
            ) {
                let (rebalanced_top, _) = rebalance_top_padding_for_bottom_gap(
                    padding_top,
                    padding_bottom,
                    row_quantization_slack,
                    BOTTOM_TAB_VISIBLE_MIN_GAP,
                );
                padding_top = rebalanced_top;
            }

            let size = TerminalSize {
                rows,
                cols,
                // Take care to use the exact pixel dimensions of the cells, rather
                // than the available space, so that apps that are sensitive to
                // the pixels-per-cell have consistent values at a given font size.
                // https://github.com/wezterm/wezterm/issues/535
                pixel_height: rows * self.render_metrics.cell_size.height as usize,
                pixel_width: cols * self.render_metrics.cell_size.width as usize,
                dpi: dimensions.dpi as u32,
            };

            let ri_calc = ResizeIncrementCalculator {
                x: self.render_metrics.cell_size.width as u16,
                y: self.render_metrics.cell_size.height as u16,
                padding_left: padding_left,
                padding_top: padding_top,
                padding_right: padding_right,
                padding_bottom: padding_bottom,
                border: border,
                tab_bar_height: tab_bar_height as usize,
            };

            (size, *dimensions, ri_calc)
        };

        log::trace!("apply_dimensions computed size {:?}, dims {:?}", size, dims);

        self.terminal_size = size;

        let live = self.live_resizing;
        let defer_font_scale_pty_resize =
            !live && has_deferred_font_scale_pty_resize(self.mux_window_id);
        let mux = Mux::get();
        if let Some(window) = mux.get_window(self.mux_window_id) {
            for tab in window.iter() {
                if live || defer_font_scale_pty_resize {
                    tab.resize_visual(size);
                } else {
                    tab.resize(size);
                }
            }
            for tab in window.iter() {
                for pos in tab.iter_panes_ignoring_zoom() {
                    let dims = pos.pane.get_dimensions();
                    let mut state = self.pane_state(pos.pane.pane_id());
                    state.viewport = Self::normalize_viewport(state.viewport, dims);
                }
            }
        };
        if live {
            self.pending_pty_flush_after_resize = true;
        }
        if defer_font_scale_pty_resize {
            self.schedule_deferred_font_scale_pty_resize(window);
        }
        self.resize_overlays();
        self.invalidate_fancy_tab_bar();
        self.update_title();

        window.set_resize_increments(if self.config.use_resize_increments {
            ri_calc.into()
        } else {
            ResizeIncrement::disabled()
        });

        // Queue up a speculative resize in order to preserve the number of rows+cols
        if let Some(cell_dims) = scale_changed_cells {
            // If we don't think the dimensions have changed, don't request
            // the window to change.  This seems to help on Wayland where
            // we won't know what size the compositor thinks we should have
            // when we're first opened, until after it sends us a configure event.
            // If we send this too early, it will trump that configure event
            // and we'll end up with weirdness where our window renders in the
            // middle of a larger region that the compositor thinks we live in.
            // Wayland is weird!
            if allow_speculative_window_resize && saved_dims != dims {
                log::trace!(
                    "scale changed so resize from {:?} to {:?} {:?} (event called with {:?})",
                    saved_dims,
                    dims,
                    cell_dims,
                    dimensions
                );
                // Stash this size pre-emptively. Without this, on Windows,
                // when the font scaling is changed we can end up not seeing
                // these dimensions and the scaling_changed logic ends up
                // comparing two dimensions that have the same DPI and recomputing
                // an adjusted terminal size.
                // eg: rather than a simple old-dpi -> new dpi transition, we'd
                // see old-dpi -> new dpi, call set_inner_size, then see a
                // new-dpi -> new-dpi adjustment with a slightly different
                // pixel geometry which is considered to be a user-driven resize.
                // Stashing the dimensions here avoids that misconception.
                self.dimensions = dims;
                self.set_inner_size(window, dims.pixel_width, dims.pixel_height);
            } else if !allow_speculative_window_resize {
                log::trace!(
                    "skipping speculative resize during deferred screen-change scaling: saved_dims={:?} computed_dims={:?} {:?}",
                    saved_dims,
                    dims,
                    cell_dims,
                );
            }
        }
    }

    pub fn current_cell_dimensions(&self) -> RowsAndCols {
        RowsAndCols {
            rows: self.terminal_size.rows as usize,
            cols: self.terminal_size.cols as usize,
        }
    }

    #[allow(clippy::float_cmp)]
    pub fn scaling_changed(
        &mut self,
        dimensions: Dimensions,
        font_scale: f64,
        window: &Window,
        allow_terminal_size_preservation: bool,
    ) {
        fn dpi_adjusted(n: usize, dpi: usize) -> f32 {
            n as f32 / dpi as f32
        }

        /// On Windows, scaling changes may adjust the pixel geometry by a few pixels,
        /// so this function checks if we're in a close-enough ballpark.
        fn close_enough(a: f32, b: f32) -> bool {
            let diff = (a - b).abs();
            diff < 10.
        }

        // Distinguish between eg: dpi being detected as double the initial dpi (where
        // the pixel dimensions don't change), and the dpi change being detected, but
        // where the window manager also decides to tile/resize the window.
        // In the latter case, we don't want to preserve the terminal rows/cols.
        let simple_dpi_change = dimensions.dpi != self.dimensions.dpi
            && ((close_enough(
                dpi_adjusted(dimensions.pixel_height, dimensions.dpi),
                dpi_adjusted(self.dimensions.pixel_height, self.dimensions.dpi),
            ) && close_enough(
                dpi_adjusted(dimensions.pixel_width, dimensions.dpi),
                dpi_adjusted(self.dimensions.pixel_width, self.dimensions.dpi),
            )) || (close_enough(
                dimensions.pixel_width as f32,
                self.dimensions.pixel_width as f32,
            ) && close_enough(
                dimensions.pixel_height as f32,
                self.dimensions.pixel_height as f32,
            )));

        if simple_dpi_change && cfg!(target_os = "macos") {
            // On macOS, NSWindow::isZoomed can falsely return YES when the
            // current screen changes. The window layer now suppresses isZoomed
            // when screen_changed is true, but this belt-and-suspenders check
            // also clears MAXIMIZED when only the DPI changed, so that the
            // can_resize check in adjust_font_scale does not block adaptation
            // to the new DPI.
            // <https://github.com/wezterm/wezterm/issues/3503>
            // <https://github.com/tw93/Kaku/issues/131>
            self.window_state -= WindowState::MAXIMIZED;
        }

        let dpi_changed = dimensions.dpi != self.dimensions.dpi;
        let font_scale_changed = font_scale != self.fonts.get_font_scale();
        let scale_changed = dpi_changed || font_scale_changed;

        log::trace!(
            "dpi_changed={}, font_scale_changed={} scale_changed={} simple_dpi_change={}",
            dpi_changed,
            font_scale_changed,
            scale_changed,
            simple_dpi_change
        );

        let cell_dims = self.current_cell_dimensions();

        if scale_changed {
            self.apply_scale_change(&dimensions, font_scale);
        }

        let preserve_terminal_cells = should_preserve_terminal_cells_on_scale_change(
            font_scale_changed,
            simple_dpi_change,
            allow_terminal_size_preservation,
        );

        let scale_changed_cells = if preserve_terminal_cells {
            Some(cell_dims)
        } else {
            None
        };

        log::trace!(
            "scaling_changed, follow with applying dimensions. allow_terminal_size_preservation={} scale_changed_cells={:?}",
            allow_terminal_size_preservation,
            scale_changed_cells,
        );
        self.apply_dimensions(&dimensions, scale_changed_cells, window, true);
    }

    /// Used for applying font size changes only; this takes into account
    /// the `adjust_window_size_when_changing_font_size` configuration and
    /// revises the scaling/resize change accordingly
    pub fn adjust_font_scale(&mut self, font_scale: f64, window: &Window) {
        mark_deferred_font_scale_pty_resize(self.mux_window_id, self.current_cell_dimensions());

        let adjust_window_size_when_changing_font_size =
            match self.config.adjust_window_size_when_changing_font_size {
                Some(value) => value,
                None => {
                    let is_tiling = self
                        .config
                        .tiling_desktop_environments
                        .iter()
                        .any(|item| item.as_str() == self.connection_name.as_str());
                    !is_tiling
                }
            };

        if self.window_state.can_resize() && adjust_window_size_when_changing_font_size {
            self.scaling_changed(self.dimensions, font_scale, window, true);
        } else {
            let dimensions = self.dimensions;
            // Compute new font metrics
            self.apply_scale_change(&dimensions, font_scale);
            // Now revise the pty size to fit the window
            self.apply_dimensions(&dimensions, None, window, true);
        }

        persist_current_font_size(&self.config, self.fonts.get_font_scale());
    }

    pub fn decrease_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Relative(1.0 / 1.1));
        self.apply_pending_scale_changes();
    }

    pub fn increase_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Relative(1.1));
        self.apply_pending_scale_changes();
    }

    pub fn reset_font_size(&mut self) {
        self.pending_scale_changes
            .push_back(ScaleChange::Absolute(1.0));
        self.apply_pending_scale_changes();
    }

    pub fn set_window_size(&mut self, size: TerminalSize, window: &Window) -> anyhow::Result<()> {
        let config = &self.config;
        let fontconfig = Rc::new(FontConfiguration::new(
            Some(config.clone()),
            self.dimensions.dpi,
        )?);
        let render_metrics = RenderMetrics::new(&fontconfig)?;

        let terminal_size = TerminalSize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: (render_metrics.cell_size.width as usize * size.cols),
            pixel_height: (render_metrics.cell_size.height as usize * size.rows),
            dpi: size.dpi,
        };

        let show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        let tab_bar_height = if show_tab_bar {
            self.tab_bar_pixel_height()? as usize
        } else {
            0
        };

        let h_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_width as f32,
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        let v_context = DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_height as f32,
            pixel_cell: render_metrics.cell_size.height as f32,
        };
        let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
        let (padding_top, padding_bottom) = effective_vertical_padding(
            config,
            v_context,
            show_tab_bar,
            config.tab_bar_at_bottom,
            tab_bar_height,
            self.layout_uses_edge_to_edge_padding(),
        );

        let dimensions = Dimensions {
            pixel_width: ((terminal_size.cols as usize * render_metrics.cell_size.width as usize)
                + padding_left
                + effective_right_padding(&config, h_context)),
            pixel_height: ((terminal_size.rows as usize * render_metrics.cell_size.height as usize)
                + padding_top
                + padding_bottom) as usize
                + tab_bar_height,
            dpi: self.dimensions.dpi,
        };

        self.apply_scale_change(&dimensions, 1.0);
        self.apply_dimensions(
            &dimensions,
            Some(RowsAndCols {
                rows: size.rows as usize,
                cols: size.cols as usize,
            }),
            window,
            true,
        );
        Ok(())
    }

    pub fn reset_font_and_window_size(&mut self, window: &Window) -> anyhow::Result<()> {
        let size = self.config.initial_size(
            self.dimensions.dpi as u32,
            Some(crate::cell_pixel_dims(
                &self.config,
                self.dimensions.dpi as f64,
            )?),
        );
        self.set_window_size(size, window)
    }

    pub fn effective_right_padding(&self, config: &Config) -> usize {
        effective_right_padding(
            config,
            DimensionContext {
                pixel_cell: self.render_metrics.cell_size.width as f32,
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_width as f32,
            },
        )
    }
}

fn font_size_state_file() -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(".kaku_font_size")
}

fn persist_current_font_size(config: &ConfigHandle, font_scale: f64) {
    let file_name = font_size_state_file();

    if (font_scale - 1.0).abs() < 0.0001 {
        if let Err(err) = std::fs::remove_file(&file_name) {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to clear persisted font size at {:?}: {:#}",
                    file_name,
                    err
                );
            }
        }
        return;
    }

    if let Some(parent) = file_name.parent() {
        if let Err(err) = config::create_user_owned_dirs(parent) {
            log::warn!(
                "Failed to create config directory for persisted font size {:?}: {:#}",
                parent,
                err
            );
            return;
        }
    }

    let font_size = config.font_size * font_scale;
    if !font_size.is_finite() || font_size <= 0.0 {
        return;
    }

    if let Err(err) = std::fs::write(&file_name, format!("{font_size:.3}\n")) {
        log::warn!(
            "Failed to persist font size {} to {:?}: {:#}",
            font_size,
            file_name,
            err
        );
    }
}

pub(super) fn load_persisted_font_scale(config: &ConfigHandle) -> Option<f64> {
    let file_name = font_size_state_file();
    let saved_font_size = std::fs::read_to_string(&file_name)
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())?;

    if !saved_font_size.is_finite() || !(2.0..=256.0).contains(&saved_font_size) {
        log::warn!(
            "Ignoring invalid persisted font size {} from {:?}",
            saved_font_size,
            file_name
        );
        return None;
    }

    if !config.font_size.is_finite() || config.font_size <= 0.0 {
        return None;
    }

    let scale = saved_font_size / config.font_size;
    if !scale.is_finite() || !(0.1..=20.0).contains(&scale) {
        log::warn!(
            "Ignoring invalid persisted font scale {} from {:?}",
            scale,
            file_name
        );
        return None;
    }

    Some(scale)
}

/// Visual spacing adjustment for tab bar layouts.
const VISUAL_SPACING: usize = 4;
/// When the top tab bar is visible, slightly tighten the top content gap so
/// the first terminal row doesn't feel too far from the tab strip.
const TOP_TAB_VISIBLE_TOP_TIGHTENING: usize = 6;
/// Bottom gap for top-tab layouts when the tab bar is **hidden** (single-tab,
/// hide-if-only-one-tab mode). A larger value gives comfortable breathing room
/// when no bar occupies the top of the window.
/// We set this to 10 to keep single-tab top-tab mode from feeling too tight.
const TOP_TAB_HIDDEN_BOTTOM_GAP: usize = 10;
/// Bottom gap for top-tab layouts when the tab bar is visible.
/// Match the hidden-tab baseline so top-tab windows keep the same content
/// inset whether the bar is visible or not.
const TOP_TAB_VISIBLE_BOTTOM_GAP: usize = TOP_TAB_HIDDEN_BOTTOM_GAP;
/// Minimum gap kept between terminal content and the bottom tab bar while the
/// bar is visible. Kept small so the bar still feels visually attached to the
/// content.
const BOTTOM_TAB_VISIBLE_MIN_GAP: usize = 2;
/// Baseline gap used when the bottom tab bar is hidden. This avoids the
/// content feeling glued to the window edge in single-tab mode.
const BOTTOM_TAB_HIDDEN_GAP: usize = 16;

#[derive(Clone)]
struct UserPaddingCache {
    path: PathBuf,
    modified: Option<SystemTime>,
    has_custom_padding: bool,
}

fn line_assigns_config_key(trimmed_line: &str, key: &str) -> bool {
    if trimmed_line.starts_with("--") {
        return false;
    }

    let Some(rest) = trimmed_line.strip_prefix(key) else {
        return false;
    };

    rest.trim_start().starts_with('=')
}

fn detect_user_custom_window_padding(path: &PathBuf) -> bool {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return false,
    };

    content
        .lines()
        .any(|line| line_assigns_config_key(line.trim(), "config.window_padding"))
}

fn user_custom_window_padding_config_path() -> PathBuf {
    config::config_file_override().unwrap_or_else(config::user_config_path)
}

fn user_has_custom_window_padding_assignment() -> bool {
    static CACHE: OnceLock<Mutex<Option<UserPaddingCache>>> = OnceLock::new();

    let path = user_custom_window_padding_config_path();
    let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut cache = cache.lock().unwrap();

    if let Some(cached) = cache.as_ref() {
        if cached.path == path && cached.modified == modified {
            return cached.has_custom_padding;
        }
    }

    let has_custom_padding = detect_user_custom_window_padding(&path);
    *cache = Some(UserPaddingCache {
        path,
        modified,
        has_custom_padding,
    });
    has_custom_padding
}

fn effective_top_padding(base_top: usize, default_top: usize) -> usize {
    if base_top == default_top {
        base_top + VISUAL_SPACING
    } else {
        base_top
    }
}

/// Computes vertical padding used for layout.
/// Bottom-tab layouts subtract the tab bar height from bottom padding so the
/// content block remains stable. Top-tab layouts preserve the full top padding
/// so the gap below the tab bar matches the no-tab baseline.
pub fn effective_vertical_padding(
    config: &Config,
    context: DimensionContext,
    show_tab_bar: bool,
    tab_bar_at_bottom: bool,
    tab_bar_height: usize,
    is_fullscreen: bool,
) -> (usize, usize) {
    effective_vertical_padding_with_policy(
        config,
        context,
        show_tab_bar,
        tab_bar_at_bottom,
        tab_bar_height,
        is_fullscreen,
        user_has_custom_window_padding_assignment(),
    )
}

fn effective_vertical_padding_with_policy(
    config: &Config,
    context: DimensionContext,
    show_tab_bar: bool,
    tab_bar_at_bottom: bool,
    tab_bar_height: usize,
    is_edge_to_edge: bool,
    user_has_custom_padding: bool,
) -> (usize, usize) {
    let base_top = config.window_padding.top.evaluate_as_pixels(context) as usize;
    let base_bottom = config.window_padding.bottom.evaluate_as_pixels(context) as usize;
    let default_top = config::WindowPadding::default()
        .top
        .evaluate_as_pixels(context) as usize;

    // Respect explicit user padding and only apply Kaku's visual heuristics
    // for the managed/default padding path.
    let mut top = if user_has_custom_padding {
        base_top
    } else {
        effective_top_padding(base_top, default_top)
    };

    // When maximized/fullscreen, eliminate bottom padding so terminal content
    // fills to the window bottom edge. The quantization slack is redistributed
    // to the top in the render path (padding_left_top).
    let mut bottom = if is_edge_to_edge && !user_has_custom_padding {
        0
    } else {
        base_bottom
    };

    // Top-tab visible mode uses a slightly tighter top gap than hidden mode.
    if !user_has_custom_padding && show_tab_bar && !tab_bar_at_bottom {
        top = top.saturating_sub(TOP_TAB_VISIBLE_TOP_TIGHTENING);
    }

    // Bottom-tab layouts subtract the tab bar height from bottom padding to
    // keep the content block stable. For top-tab layouts we intentionally keep
    // the full top padding so the tab bar gets the same breathing room as the
    // no-tab case.
    if show_tab_bar && tab_bar_at_bottom {
        bottom = bottom.saturating_sub(tab_bar_height);
    }

    if !is_edge_to_edge && !user_has_custom_padding && !tab_bar_at_bottom {
        let gap = if show_tab_bar {
            TOP_TAB_VISIBLE_BOTTOM_GAP
        } else {
            TOP_TAB_HIDDEN_BOTTOM_GAP
        };
        bottom = bottom.max(gap);
    }

    // Keep a tiny baseline gap in bottom-tab layouts so hidden/visible tab-bar
    // transitions cannot collapse pane content onto the window bottom edge.
    if !is_edge_to_edge && !user_has_custom_padding && tab_bar_at_bottom {
        let min_gap = if show_tab_bar {
            BOTTOM_TAB_VISIBLE_MIN_GAP
        } else {
            BOTTOM_TAB_HIDDEN_GAP
        };
        bottom = bottom.max(min_gap);
    }

    (top, bottom)
}

/// Computes the effective padding for the RHS.
/// This is needed because the default is 0, but if the user has
/// enabled the scroll bar then they will expect it to have a reasonable
/// size unless they've specified differently.
pub fn effective_right_padding(config: &Config, context: DimensionContext) -> usize {
    if config.enable_scroll_bar && config.window_padding.right.is_zero() {
        context.pixel_cell as usize
    } else {
        config.window_padding.right.evaluate_as_pixels(context) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_deferred_font_scale_pty_resize_for_test, effective_top_padding,
        effective_vertical_padding_with_policy, finish_deferred_font_scale_pty_resize,
        has_deferred_font_scale_pty_resize, mark_deferred_font_scale_pty_resize,
        rebalance_top_padding_for_bottom_gap, should_defer_screen_change_scale_update,
        should_normalize_fullscreen_state_on_resize,
        should_preserve_terminal_cells_on_scale_change,
        should_rebalance_top_tab_visible_bottom_gap, user_custom_window_padding_config_path,
        DeferredFontScalePtyResizeAction, RowsAndCols,
    };
    use config::{Config, ConfigHandle, DimensionContext};
    use std::path::PathBuf;

    fn context() -> DimensionContext {
        DimensionContext {
            dpi: 96.0,
            pixel_max: 800.0,
            pixel_cell: 16.0,
        }
    }

    fn base_vertical_padding(config: &ConfigHandle) -> (usize, usize) {
        (
            config.window_padding.top.evaluate_as_pixels(context()) as usize,
            config.window_padding.bottom.evaluate_as_pixels(context()) as usize,
        )
    }

    #[test]
    fn deferred_font_scale_resize_keeps_visual_only_until_cells_stabilize() {
        let window_id = usize::MAX - 407;
        clear_deferred_font_scale_pty_resize_for_test(window_id);

        let first = mark_deferred_font_scale_pty_resize(
            window_id,
            RowsAndCols {
                rows: 39,
                cols: 177,
            },
        );
        let second = mark_deferred_font_scale_pty_resize(
            window_id,
            RowsAndCols {
                rows: 35,
                cols: 168,
            },
        );

        assert!(has_deferred_font_scale_pty_resize(window_id));
        assert_eq!(second.epoch, first.epoch + 1);
        assert_eq!(
            second.last_cells,
            RowsAndCols {
                rows: 35,
                cols: 168
            }
        );

        assert_eq!(
            finish_deferred_font_scale_pty_resize(
                window_id,
                first.epoch,
                RowsAndCols {
                    rows: 35,
                    cols: 168,
                },
            ),
            DeferredFontScalePtyResizeAction::Stale
        );
        assert_eq!(
            finish_deferred_font_scale_pty_resize(
                window_id,
                second.epoch,
                RowsAndCols {
                    rows: 30,
                    cols: 135,
                },
            ),
            DeferredFontScalePtyResizeAction::Keep(super::DeferredFontScalePtyResize {
                epoch: second.epoch + 1,
                last_cells: RowsAndCols {
                    rows: 30,
                    cols: 135
                }
            })
        );
        assert!(has_deferred_font_scale_pty_resize(window_id));

        assert_eq!(
            finish_deferred_font_scale_pty_resize(
                window_id,
                second.epoch + 1,
                RowsAndCols {
                    rows: 30,
                    cols: 135,
                },
            ),
            DeferredFontScalePtyResizeAction::Remove
        );
        assert!(!has_deferred_font_scale_pty_resize(window_id));
    }

    fn effective_vertical_padding(
        config: &Config,
        context: DimensionContext,
        show_tab_bar: bool,
        tab_bar_at_bottom: bool,
        tab_bar_height: usize,
        is_fullscreen: bool,
    ) -> (usize, usize) {
        effective_vertical_padding_with_policy(
            config,
            context,
            show_tab_bar,
            tab_bar_at_bottom,
            tab_bar_height,
            is_fullscreen,
            false,
        )
    }

    fn effective_vertical_padding_user_custom(
        config: &Config,
        context: DimensionContext,
        show_tab_bar: bool,
        tab_bar_at_bottom: bool,
        tab_bar_height: usize,
        is_fullscreen: bool,
    ) -> (usize, usize) {
        effective_vertical_padding_with_policy(
            config,
            context,
            show_tab_bar,
            tab_bar_at_bottom,
            tab_bar_height,
            is_fullscreen,
            true,
        )
    }

    #[test]
    fn top_tab_mode_adjusts_top_even_when_tab_bar_hidden() {
        let config = ConfigHandle::default_config();
        let (base_top, base_bottom) = base_vertical_padding(&config);

        let (top, bottom) = effective_vertical_padding(&config, context(), false, false, 24, false);

        assert_eq!(top, base_top + 4);
        // Tab bar is hidden → use the larger hidden gap.
        assert_eq!(bottom, base_bottom.max(super::TOP_TAB_HIDDEN_BOTTOM_GAP));
    }

    #[test]
    fn explicit_top_padding_can_disable_visual_spacing() {
        assert_eq!(effective_top_padding(0, 8), 0);
        assert_eq!(effective_top_padding(12, 8), 12);
    }

    #[test]
    fn custom_padding_path_defaults_to_user_config() {
        config::clear_config_file_override();
        assert_eq!(
            user_custom_window_padding_config_path(),
            config::user_config_path()
        );
    }

    #[test]
    fn explicit_override_path_wins_for_custom_padding_detection() {
        config::clear_config_file_override();
        let override_path = PathBuf::from("/tmp/kaku-override.lua");
        config::set_config_file_override(&override_path);

        assert_eq!(user_custom_window_padding_config_path(), override_path);
        config::clear_config_file_override();
    }

    #[test]
    fn custom_padding_skips_visual_top_spacing() {
        let config = Config::default_config();
        let base_top = config.window_padding.top.evaluate_as_pixels(context()) as usize;

        let (top, _) =
            effective_vertical_padding_user_custom(&config, context(), false, false, 24, false);

        assert_eq!(top, base_top);
    }

    #[test]
    fn top_tab_bar_visible_adjusts_top_and_bottom_padding() {
        let config = ConfigHandle::default_config();
        let tab_bar_height = 24;
        let (base_top, base_bottom) = base_vertical_padding(&config);

        let (with_tab_top, with_tab_bottom) =
            effective_vertical_padding(&config, context(), true, false, tab_bar_height, false);

        assert_eq!(
            with_tab_top,
            base_top + super::VISUAL_SPACING - super::TOP_TAB_VISIBLE_TOP_TIGHTENING
        );
        assert_eq!(
            with_tab_bottom,
            base_bottom.max(super::TOP_TAB_VISIBLE_BOTTOM_GAP)
        );
    }

    #[test]
    fn top_tab_bar_visible_has_tighter_top_padding_than_hidden() {
        let config = ConfigHandle::default_config();

        let (hidden_top, hidden_bottom) =
            effective_vertical_padding(&config, context(), false, false, 24, false);
        let (visible_top, visible_bottom) =
            effective_vertical_padding(&config, context(), true, false, 24, false);

        assert_eq!(
            visible_top + super::TOP_TAB_VISIBLE_TOP_TIGHTENING,
            hidden_top
        );
        assert_eq!(visible_bottom, hidden_bottom);
    }

    #[test]
    fn top_tab_bar_visible_preserves_explicit_large_bottom_padding() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(20.0);

        let (_, bottom) = effective_vertical_padding(&config, context(), true, false, 24, false);

        assert_eq!(bottom, 20);
    }

    #[test]
    fn custom_padding_skips_top_tab_bottom_gap_floor() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(0.0);

        let (_, bottom) =
            effective_vertical_padding_user_custom(&config, context(), false, false, 24, false);

        assert_eq!(bottom, 0);
    }

    #[test]
    fn edge_to_edge_top_tab_bar_visible_eliminates_bottom_padding() {
        let config = ConfigHandle::default_config();

        let (_, bottom) = effective_vertical_padding(&config, context(), true, false, 24, true);

        assert_eq!(bottom, 0);
    }

    #[test]
    fn custom_padding_skips_fullscreen_top_tightening() {
        let mut config = Config::default_config();
        config.window_padding.top = config::Dimension::Pixels(40.0);

        let (top, _) =
            effective_vertical_padding_user_custom(&config, context(), false, false, 24, true);

        assert_eq!(top, 40);
    }

    #[test]
    fn custom_padding_skips_top_tab_visible_top_tightening() {
        let mut config = Config::default_config();
        config.window_padding.top = config::Dimension::Pixels(24.0);

        let (top, _) =
            effective_vertical_padding_user_custom(&config, context(), true, false, 24, false);

        assert_eq!(top, 24);
    }

    #[test]
    fn edge_to_edge_top_tab_hidden_eliminates_bottom_padding() {
        let config = ConfigHandle::default_config();

        let normal = effective_vertical_padding(&config, context(), false, false, 24, false);
        let edge = effective_vertical_padding(&config, context(), false, false, 24, true);

        // Top padding stays the same; bottom drops to 0.
        assert_eq!(edge.0, normal.0);
        assert_eq!(edge.1, 0);
    }

    #[test]
    fn edge_to_edge_top_tab_visible_eliminates_bottom_padding() {
        let config = ConfigHandle::default_config();

        let normal = effective_vertical_padding(&config, context(), true, false, 24, false);
        let edge = effective_vertical_padding(&config, context(), true, false, 24, true);

        // Top padding stays the same; bottom drops to 0.
        assert_eq!(edge.0, normal.0);
        assert_eq!(edge.1, 0);
    }

    #[test]
    fn bottom_tab_bar_adjusts_padding_even_when_hidden() {
        let config = ConfigHandle::default_config();
        let (base_top, base_bottom) = base_vertical_padding(&config);
        let (with_bottom_top, with_bottom_bottom) =
            effective_vertical_padding(&config, context(), false, true, 24, false);

        assert_eq!(with_bottom_top, base_top + 4);
        assert_eq!(
            with_bottom_bottom,
            base_bottom.max(super::BOTTOM_TAB_HIDDEN_GAP)
        );
    }

    #[test]
    fn bottom_tab_bar_hidden_with_zero_padding_keeps_min_gap() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(0.0);
        let (_, bottom) = effective_vertical_padding(&config, context(), false, true, 24, false);
        assert_eq!(bottom, super::BOTTOM_TAB_HIDDEN_GAP);
    }

    #[test]
    fn edge_to_edge_bottom_tab_bar_hidden_eliminates_bottom_padding() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(0.0);
        let (_, bottom) = effective_vertical_padding(&config, context(), false, true, 24, true);
        assert_eq!(bottom, 0);
    }

    #[test]
    fn bottom_tab_bar_visible_zero_height_keeps_min_gap() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(0.0);
        let (_, bottom) = effective_vertical_padding(&config, context(), true, true, 0, false);
        assert_eq!(bottom, super::BOTTOM_TAB_VISIBLE_MIN_GAP);
    }

    #[test]
    fn bottom_tab_bar_enforces_min_gap() {
        let tab_bar_height = 24;

        // Default config: saturating_sub gives 0, min-gap floor kicks in.
        let config = ConfigHandle::default_config();
        let (_, bottom) =
            effective_vertical_padding(&config, context(), true, true, tab_bar_height, false);
        assert_eq!(bottom, super::BOTTOM_TAB_VISIBLE_MIN_GAP);

        // Edge-to-edge: bottom drops to 0 (no min gap enforcement).
        let (_, bottom_edge) =
            effective_vertical_padding(&config, context(), true, true, tab_bar_height, true);
        assert_eq!(bottom_edge, 0);

        // Explicit large value: user gets their residual (already above the floor).
        let mut config2 = Config::default_config();
        config2.window_padding.bottom = config::Dimension::Pixels(40.0);
        let (_, bottom2) =
            effective_vertical_padding(&config2, context(), true, true, tab_bar_height, false);
        assert_eq!(bottom2, 40usize.saturating_sub(tab_bar_height)); // 16px
    }

    #[test]
    fn edge_to_edge_bottom_tab_bar_explicit_zero_eliminates_padding() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(0.0);

        let (_, bottom) = effective_vertical_padding(&config, context(), true, true, 24, true);

        assert_eq!(bottom, 0);
    }

    #[test]
    fn edge_to_edge_bottom_tab_bar_explicit_small_eliminates_padding() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(2.0);

        let (_, bottom) = effective_vertical_padding(&config, context(), true, true, 24, true);

        assert_eq!(bottom, 0);
    }

    #[test]
    fn normalize_fullscreen_on_unfocused_top_tab_resize_even_with_dim_change() {
        assert!(should_normalize_fullscreen_state_on_resize(
            false, // top-tab
            true,  // was fullscreen
            false, // transient incoming non-fullscreen
            false, // unfocused
            false, // dimensions changed
        ));
    }

    #[test]
    fn normalize_fullscreen_on_focused_top_tab_requires_same_dimensions() {
        assert!(should_normalize_fullscreen_state_on_resize(
            false, true, false, true, true
        ));
        assert!(!should_normalize_fullscreen_state_on_resize(
            false, true, false, true, false
        ));
    }

    #[test]
    fn normalize_fullscreen_disabled_for_bottom_tab() {
        assert!(!should_normalize_fullscreen_state_on_resize(
            true, true, false, false, true
        ));
    }

    #[test]
    fn rebalance_top_padding_caps_visible_top_tab_bottom_gap() {
        let (top, shifted) =
            rebalance_top_padding_for_bottom_gap(18, 4, 12, super::TOP_TAB_VISIBLE_BOTTOM_GAP);

        assert_eq!(top, 24);
        assert_eq!(shifted, 6);
    }

    #[test]
    fn rebalance_top_padding_noop_when_bottom_gap_within_limit() {
        let (top, shifted) =
            rebalance_top_padding_for_bottom_gap(18, 4, 4, super::TOP_TAB_VISIBLE_BOTTOM_GAP);

        assert_eq!(top, 18);
        assert_eq!(shifted, 0);
    }

    #[test]
    fn rebalance_applies_only_for_managed_top_tab_visible_non_fullscreen() {
        assert!(should_rebalance_top_tab_visible_bottom_gap(
            false, // managed padding
            true,  // tab bar visible
            false, // top-tab
            false, // not fullscreen
        ));
        assert!(!should_rebalance_top_tab_visible_bottom_gap(
            true, true, false, false
        ));
        assert!(!should_rebalance_top_tab_visible_bottom_gap(
            false, false, false, false
        ));
        assert!(!should_rebalance_top_tab_visible_bottom_gap(
            false, true, true, false
        ));
        assert!(!should_rebalance_top_tab_visible_bottom_gap(
            false, true, false, true
        ));
    }

    #[test]
    fn screen_change_disables_terminal_size_preservation_for_simple_dpi_change() {
        assert!(!should_preserve_terminal_cells_on_scale_change(
            false, true, false
        ));
        assert!(should_preserve_terminal_cells_on_scale_change(
            false, true, true
        ));
        assert!(should_preserve_terminal_cells_on_scale_change(
            true, false, false
        ));
    }

    #[test]
    fn live_resize_defers_cross_screen_dpi_updates() {
        assert!(should_defer_screen_change_scale_update(
            true, true, false, 96, 192
        ));
        assert!(should_defer_screen_change_scale_update(
            true, false, true, 96, 192
        ));
        assert!(!should_defer_screen_change_scale_update(
            false, true, false, 96, 192
        ));
        assert!(!should_defer_screen_change_scale_update(
            true, true, false, 96, 96
        ));
    }

    #[test]
    fn bottom_tab_rebalance_applies_only_for_managed_bottom_tab() {
        use super::should_rebalance_bottom_tab_quantization_slack;
        assert!(should_rebalance_bottom_tab_quantization_slack(
            false, // managed padding
            true,  // bottom-tab
        ));
        assert!(!should_rebalance_bottom_tab_quantization_slack(
            true, // user custom padding
            true,
        ));
        assert!(!should_rebalance_bottom_tab_quantization_slack(
            false, // managed padding
            false, // top-tab - handled by top-tab rebalance
        ));
    }

    #[test]
    fn bottom_tab_rebalance_absorbs_quantization_slack_into_top_padding() {
        // Simulate: cell_height=23, avail_height=1372, rows=59, slack=15
        // padding_bottom=2 (BOTTOM_TAB_VISIBLE_MIN_GAP), padding_top=36
        // bottom_gap = 2 + 15 = 17 > BOTTOM_TAB_VISIBLE_MIN_GAP(2)
        // shift = 17 - 2 = 15 → padding_top increases by 15, bottom strip eliminated
        let (top, shifted) =
            rebalance_top_padding_for_bottom_gap(36, 2, 15, super::BOTTOM_TAB_VISIBLE_MIN_GAP);
        assert_eq!(shifted, 15);
        assert_eq!(top, 51);
    }

    #[test]
    fn bottom_tab_rebalance_noop_when_no_slack() {
        let (top, shifted) =
            rebalance_top_padding_for_bottom_gap(36, 2, 0, super::BOTTOM_TAB_VISIBLE_MIN_GAP);
        assert_eq!(shifted, 0);
        assert_eq!(top, 36);
    }
}
