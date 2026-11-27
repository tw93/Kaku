use crate::resize_increment_calculator::ResizeIncrementCalculator;
use ::window::{Dimensions, ResizeIncrement, Window, WindowOps, WindowState};
use config::{Config, ConfigHandle, DimensionContext};
use mux::Mux;
use std::collections::{HashMap, LinkedList};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use wezterm_font::FontConfiguration;
use wezterm_term::TerminalSize;

/// Like tmux's pane resize timer: during a font zoom the PTY hears the first
/// step at once, then at most one (latest) size per window, so a burst of
/// Cmd+= steps does not flood the app with SIGWINCH. The window is Ghostty's
/// 25ms resize coalesce rather than tmux's 250ms: until the app is told, it
/// keeps drawing frames for the old size into the already reflowed grid.
const FONT_SCALE_PTY_RESIZE_THROTTLE: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowsAndCols {
    pub rows: usize,
    pub cols: usize,
}

/// Present while a font zoom is in progress for a window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DeferredFontScalePtyResize {
    timer_running: bool,
    /// A step reflowed the grid after the PTY was last told.
    pending: bool,
}

static DEFERRED_FONT_SCALE_PTY_RESIZES: OnceLock<
    Mutex<HashMap<usize, DeferredFontScalePtyResize>>,
> = OnceLock::new();

fn deferred_font_scale_pty_resizes() -> &'static Mutex<HashMap<usize, DeferredFontScalePtyResize>> {
    DEFERRED_FONT_SCALE_PTY_RESIZES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mark_deferred_font_scale_pty_resize(window_id: usize) {
    deferred_font_scale_pty_resizes()
        .lock()
        .unwrap()
        .entry(window_id)
        .or_default();
}

fn has_deferred_font_scale_pty_resize(window_id: usize) -> bool {
    deferred_font_scale_pty_resizes()
        .lock()
        .unwrap()
        .contains_key(&window_id)
}

/// Records a zoom step. Returns true when the PTY should be told now and the
/// throttle timer started; otherwise the step waits for the timer.
fn note_deferred_font_scale_pty_resize(window_id: usize) -> bool {
    let mut deferred = deferred_font_scale_pty_resizes().lock().unwrap();
    let state = deferred.entry(window_id).or_default();
    if state.timer_running {
        state.pending = true;
        false
    } else {
        state.timer_running = true;
        true
    }
}

/// Called when the throttle timer fires. Returns true when a step is waiting:
/// the PTY should be told now and the timer restarted. Otherwise the zoom is
/// over and the window leaves the deferred path.
fn finish_deferred_font_scale_pty_resize(window_id: usize) -> bool {
    let mut deferred = deferred_font_scale_pty_resizes().lock().unwrap();
    match deferred.get_mut(&window_id) {
        Some(state) if state.pending => {
            state.pending = false;
            true
        }
        _ => {
            deferred.remove(&window_id);
            false
        }
    }
}

pub(super) fn clear_deferred_font_scale_pty_resize(window_id: usize) {
    deferred_font_scale_pty_resizes()
        .lock()
        .unwrap()
        .remove(&window_id);
}

fn schedule_deferred_font_scale_pty_resize_timer(window: &Window) {
    let window = window.clone();
    promise::spawn::spawn_into_main_thread(async move {
        smol::Timer::after(FONT_SCALE_PTY_RESIZE_THROTTLE).await;
        window.notify(super::TermWindowNotif::Apply(Box::new(move |tw| {
            tw.flush_deferred_font_scale_pty_resize();
        })));
    })
    .detach();
}

#[derive(Debug)]
pub enum ScaleChange {
    Absolute(f64),
    Relative(f64),
}

/// Per-window resize lifecycle: in-flight `set_inner_size` requests and the
/// work deferred until they (or a live resize) complete. Bundled to keep the
/// `TermWindow` field list shorter.
#[derive(Default)]
pub(crate) struct ResizeLifecycleState {
    pub resizes_pending: usize,
    pub is_repaint_pending: bool,
    pub pending_scale_changes: LinkedList<ScaleChange>,
    /// Tracks whether we are currently in a live resize operation
    pub live_resizing: bool,
    pub pending_screen_change_resize: bool,
    pub pending_pty_flush_after_resize: bool,
    pub pending_config_reload_after_resize: bool,
    pub deferred_layout_relayout_epoch: usize,
}

fn resize_needs_layout_refresh(
    dimensions_unchanged: bool,
    window_state_unchanged: bool,
    screen_changed: bool,
) -> bool {
    // Equal geometry can belong to a different screen with a different safe area.
    screen_changed || !dimensions_unchanged || !window_state_unchanged
}

fn should_normalize_fullscreen_state_on_resize(
    tab_bar_at_bottom: bool,
    was_fullscreen: bool,
    incoming_state: WindowState,
    is_focused: bool,
    dimensions_unchanged: bool,
) -> bool {
    !tab_bar_at_bottom
        && !incoming_state.contains(WindowState::HOTKEY_WINDOW)
        && was_fullscreen
        && !incoming_state.contains(WindowState::FULL_SCREEN)
        && (!is_focused || dimensions_unchanged)
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

fn should_stabilize_transition_dpi(
    fullscreen_transition: bool,
    maximize_transition: bool,
    screen_changed: bool,
    current_dpi: usize,
    incoming_dpi: usize,
) -> bool {
    // A dpi change arriving with screen_changed comes from a display
    // reconfiguration (lock-screen return, monitor connect/disconnect) and is
    // authoritative. It can ride along with a phantom maximize transition:
    // scaling_changed strips MAXIMIZED from the GUI state on a simple dpi
    // change while the window layer keeps carrying it forward, so the
    // corrective resize after an unlock reads as a transition. Stabilizing
    // the dpi then pins the transient scale forever because the
    // reconfiguration storm ends without another resize event (#477).
    !screen_changed && (fullscreen_transition || maximize_transition) && incoming_dpi != current_dpi
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
        if normalized_window_state.contains(WindowState::HOTKEY_WINDOW) {
            self.layout_sticky_fullscreen_until = None;
        }
        if live_resizing
            && self.resize_state.pending_screen_change_resize
            && dimensions.dpi == self.dimensions.dpi
        {
            // The user dragged back onto the original screen before the live resize
            // ended, so the deferred DPI transition is no longer relevant.
            self.resize_state.pending_screen_change_resize = false;
        }
        let dimensions_unchanged = dimensions == self.dimensions;
        let was_fullscreen = self.window_state.contains(WindowState::FULL_SCREEN);
        let is_focused = self.focused.is_some();
        if should_normalize_fullscreen_state_on_resize(
            self.config.tab_bar_at_bottom,
            was_fullscreen,
            normalized_window_state,
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

        if !resize_needs_layout_refresh(
            dimensions_unchanged,
            self.window_state == normalized_window_state,
            screen_changed,
        ) {
            // Even if the geometry didn't change, live resize state transitions
            // still matter for flushing deferred work.
            let was_live_resizing = self.resize_state.live_resizing;
            self.resize_state.live_resizing = live_resizing;

            if was_live_resizing && !live_resizing {
                self.flush_pending_pty_resize();
                if self.resize_state.pending_config_reload_after_resize {
                    self.resize_state.pending_config_reload_after_resize = false;
                    self.schedule_silent_config_reload(window);
                }
                self.emit_window_event("window-resized", None);
            }

            log::trace!("dimensions/window_state didn't change NOP!");
            return;
        }
        let last_state = self.window_state;
        self.window_state = normalized_window_state;
        self.resize_state.live_resizing = live_resizing;
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
            self.resize_state.pending_screen_change_resize,
            self.dimensions.dpi,
            dimensions.dpi,
        ) {
            log::trace!(
                "deferring cross-screen dpi update until live resize ends: current_dpi={} incoming_dpi={} dims={:?}",
                self.dimensions.dpi,
                dimensions.dpi,
                dimensions,
            );
            self.resize_state.pending_screen_change_resize = true;

            let mut stabilized = dimensions;
            stabilized.dpi = self.dimensions.dpi;
            self.apply_dimensions(
                &stabilized,
                Some(self.current_cell_dimensions()),
                window,
                false,
            );
        } else if !live_resizing
            && self.resize_state.pending_screen_change_resize
            && dimensions.dpi != self.dimensions.dpi
        {
            log::trace!(
                "committing deferred cross-screen dpi update after live resize: current_dpi={} incoming_dpi={} dims={:?}",
                self.dimensions.dpi,
                dimensions.dpi,
                dimensions,
            );
            self.resize_state.pending_screen_change_resize = false;
            self.scaling_changed(dimensions, self.fonts.get_font_scale(), window, false);

        // Align fullscreen transition handling with maximize/restore behavior:
        // keep current dpi for this transition frame so text doesn't pop larger/smaller.
        } else if should_stabilize_transition_dpi(
            fullscreen_transition,
            maximize_transition,
            screen_changed,
            self.dimensions.dpi,
            dimensions.dpi,
        ) {
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
            if self.resize_state.pending_config_reload_after_resize {
                self.resize_state.pending_config_reload_after_resize = false;
                self.schedule_silent_config_reload(window);
            }
            self.emit_window_event("window-resized", None);
        }
        // A real dimension change reconfigured the wgpu surface, but the
        // CAMetalLayer drawable is only resized when a frame is actually
        // painted. Request a repaint so a window shown into an existing macOS
        // fullscreen Space sizes its drawable immediately instead of staying
        // stuck at its initial size until an incidental later resize (#452).
        window.invalidate();
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
        if !self.resize_state.pending_pty_flush_after_resize {
            return;
        }
        self.resize_state.pending_pty_flush_after_resize = false;
        self.flush_all_pane_pty_sizes();
    }

    fn schedule_deferred_font_scale_pty_resize(&mut self, window: &Window) {
        if note_deferred_font_scale_pty_resize(self.mux_window_id) {
            self.flush_all_pane_pty_sizes();
            schedule_deferred_font_scale_pty_resize_timer(window);
        }
    }

    fn flush_deferred_font_scale_pty_resize(&mut self) {
        // The deferred path reflowed the grids via resize_visual, so the PTY
        // has not seen the latest size yet.
        if finish_deferred_font_scale_pty_resize(self.mux_window_id) {
            self.flush_all_pane_pty_sizes();
            if let Some(window) = self.window.as_ref() {
                schedule_deferred_font_scale_pty_resize_timer(window);
            }
        }
    }

    pub fn apply_pending_scale_changes(&mut self) {
        while self.resize_state.resizes_pending == 0 {
            match self.resize_state.pending_scale_changes.pop_front() {
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
        // forcing a ~330ms default-font reresolve on macOS. Skip only when the
        // metrics are still current: a config reload keeps scale and dpi but
        // can change font, font_size, line_height or cell_width.
        let key = super::render_metrics_key(&self.config, dimensions.dpi, font_scale);
        if key == self.render_metrics_key {
            return;
        }

        let (prior_font, prior_dpi) = self.fonts.change_scaling(font_scale, dimensions.dpi);
        match super::render_metrics_from_cache_or_compute(
            &self.fonts,
            &self.config,
            dimensions.dpi,
            font_scale,
        ) {
            Ok((metrics, _cache_hit)) => {
                self.render_metrics = metrics;
                self.render_metrics_key = key;
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

        // change_scaling rebuilt the LoadedFonts, so cached box-model shaping
        // holds glyph advances from the old scale/DPI (and a reused Rc address
        // could otherwise satisfy a stale key). Drop it wholesale.
        self.box_text_shape_cache.borrow_mut().clear();

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
        let user_has_custom_padding = self.window_state.contains(WindowState::HOTKEY_WINDOW)
            || user_has_custom_window_padding_assignment();

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
                pixel_max: padding_pixel_max(
                    self.window_state,
                    self.dimensions.pixel_width,
                    size.pixel_width,
                ),
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: padding_pixel_max(
                    self.window_state,
                    self.dimensions.pixel_height,
                    size.pixel_height,
                ),
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = effective_window_padding(config, self.window_state)
                .left
                .evaluate_as_pixels(h_context) as usize;
            let (padding_top, padding_bottom) = effective_vertical_padding_with_policy(
                config,
                v_context,
                self.show_tab_bar,
                self.config.tab_bar_at_bottom,
                tab_bar_height_px,
                self.window_state,
                user_has_custom_padding,
            );
            let padding_right = effective_right_padding(config, h_context, self.window_state);

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
                pixel_max: padding_pixel_max(
                    self.window_state,
                    self.dimensions.pixel_width,
                    self.terminal_size.pixel_width,
                ),
                pixel_cell: self.render_metrics.cell_size.width as f32,
            };
            let v_context = DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: padding_pixel_max(
                    self.window_state,
                    self.dimensions.pixel_height,
                    self.terminal_size.pixel_height,
                ),
                pixel_cell: self.render_metrics.cell_size.height as f32,
            };
            let padding_left = effective_window_padding(config, self.window_state)
                .left
                .evaluate_as_pixels(h_context) as usize;
            let (padding_top, mut padding_bottom) = effective_vertical_padding_with_policy(
                config,
                v_context,
                self.show_tab_bar,
                self.config.tab_bar_at_bottom,
                tab_bar_height_px,
                self.window_state,
                user_has_custom_padding,
            );
            let padding_right = effective_right_padding(config, h_context, self.window_state);

            // When the AI chat overlay is active on the focused pane, shrink
            // the bottom padding so the chat box sits closer to the window /
            // tab-bar edge. This only affects layout while the overlay is open;
            // closing it triggers a resize that restores the normal padding.
            if !self.window_state.contains(WindowState::HOTKEY_WINDOW)
                && self.has_ai_chat_overlay_on_active_pane()
            {
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

        let live = self.resize_state.live_resizing;
        let defer_font_scale_pty_resize =
            !live && has_deferred_font_scale_pty_resize(self.mux_window_id);
        let mux = Mux::get();
        if let Some(window) = mux.get_window(self.mux_window_id) {
            for tab in window.iter() {
                // A tab a browser is driving keeps the browser's size
                // through relayouts the Mac does on its own; the window
                // draws the rows that fit.
                #[cfg(feature = "remote")]
                if !kaku_remote::window_relayout_resizes_tab(tab.tab_id().into(), live) {
                    continue;
                }
                if live || defer_font_scale_pty_resize {
                    tab.resize_visual(size);
                } else {
                    tab.resize(size);
                }
                if defer_font_scale_pty_resize {
                    // Unlike a live drag, zoom steps are discrete, so reflow each
                    // pane's grid now or the old rows/cols render at the new cell
                    // size until the PTY is told, which is throttled below.
                    for pos in tab.iter_panes() {
                        let _ = pos.pane.resize_visual(TerminalSize {
                            rows: pos.height,
                            cols: pos.width,
                            pixel_width: pos.pixel_width,
                            pixel_height: pos.pixel_height,
                            dpi: size.dpi,
                        });
                    }
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
            self.resize_state.pending_pty_flush_after_resize = true;
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
        mark_deferred_font_scale_pty_resize(self.mux_window_id);

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

        persist_current_font_size(&self.config, self.fonts.get_font_scale(), self.window_state);
    }

    pub fn decrease_font_size(&mut self) {
        self.resize_state
            .pending_scale_changes
            .push_back(ScaleChange::Relative(1.0 / 1.1));
        self.apply_pending_scale_changes();
    }

    pub fn increase_font_size(&mut self) {
        self.resize_state
            .pending_scale_changes
            .push_back(ScaleChange::Relative(1.1));
        self.apply_pending_scale_changes();
    }

    pub fn reset_font_size(&mut self) {
        self.resize_state
            .pending_scale_changes
            .push_back(ScaleChange::Absolute(1.0));
        self.apply_pending_scale_changes();
    }

    pub fn set_window_size(&mut self, size: TerminalSize, window: &Window) -> anyhow::Result<()> {
        let config = &self.config;
        let fontconfig = Rc::new(FontConfiguration::new(
            Some(config.clone()),
            self.dimensions.dpi,
        )?);
        // The fresh FontConfiguration above starts at font_scale 1.0, so the
        // cached-metrics key uses 1.0 rather than the window's live scale.
        let (render_metrics, _cache_hit) = super::render_metrics_from_cache_or_compute(
            &fontconfig,
            config,
            self.dimensions.dpi,
            1.0,
        )?;

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
        let padding_left = effective_window_padding(config, self.window_state)
            .left
            .evaluate_as_pixels(h_context) as usize;
        let (padding_top, padding_bottom) = effective_vertical_padding(
            config,
            v_context,
            show_tab_bar,
            config.tab_bar_at_bottom,
            tab_bar_height,
            self.window_state,
        );

        let dimensions = Dimensions {
            pixel_width: ((terminal_size.cols as usize * render_metrics.cell_size.width as usize)
                + padding_left
                + effective_right_padding(config, h_context, self.window_state)),
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
            self.window_state,
        )
    }
}

fn font_size_state_name(window_state: WindowState) -> &'static str {
    if window_state.contains(WindowState::HOTKEY_WINDOW) {
        ".kaku_hotkey_font_size"
    } else {
        ".kaku_font_size"
    }
}

fn font_size_state_file(window_state: WindowState) -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(font_size_state_name(window_state))
}

fn persist_current_font_size(config: &ConfigHandle, font_scale: f64, window_state: WindowState) {
    let file_name = font_size_state_file(window_state);

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

pub(super) fn load_persisted_font_scale(
    config: &ConfigHandle,
    window_state: WindowState,
) -> Option<f64> {
    if window_state.contains(WindowState::HOTKEY_WINDOW)
        && config.macos_global_hotkey_appearance.font_size.is_some()
    {
        return None;
    }
    let file_name = font_size_state_file(window_state);
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

pub(crate) fn user_has_custom_window_padding_assignment() -> bool {
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

pub(crate) fn effective_window_padding(
    config: &Config,
    _window_state: WindowState,
) -> &config::WindowPadding {
    // Hotkey overrides are resolved before layout, rendering and hit testing.
    &config.window_padding
}

pub(crate) fn padding_pixel_max(
    window_state: WindowState,
    window_pixels: usize,
    terminal_pixels: usize,
) -> f32 {
    if window_state.contains(WindowState::HOTKEY_WINDOW) {
        window_pixels as f32
    } else {
        terminal_pixels as f32
    }
}

/// Computes vertical padding used for layout. Rows rarely fill the height
/// exactly; `padding_left_top` splits the leftover between top and bottom.
pub fn effective_vertical_padding(
    config: &Config,
    context: DimensionContext,
    show_tab_bar: bool,
    tab_bar_at_bottom: bool,
    tab_bar_height: usize,
    window_state: WindowState,
) -> (usize, usize) {
    effective_vertical_padding_with_policy(
        config,
        context,
        show_tab_bar,
        tab_bar_at_bottom,
        tab_bar_height,
        window_state,
        user_has_custom_window_padding_assignment(),
    )
}

fn effective_vertical_padding_with_policy(
    config: &Config,
    context: DimensionContext,
    show_tab_bar: bool,
    tab_bar_at_bottom: bool,
    tab_bar_height: usize,
    window_state: WindowState,
    user_has_custom_padding: bool,
) -> (usize, usize) {
    if window_state.contains(WindowState::HOTKEY_WINDOW) {
        let padding = effective_window_padding(config, window_state);
        return (
            padding.top.evaluate_as_pixels(context) as usize,
            padding.bottom.evaluate_as_pixels(context) as usize,
        );
    }
    let is_edge_to_edge = window_state.contains(WindowState::MAXIMIZED);
    let base_top = config.window_padding.top.evaluate_as_pixels(context) as usize;
    let base_bottom = config.window_padding.bottom.evaluate_as_pixels(context) as usize;
    let bottom_tab_visible = show_tab_bar && tab_bar_at_bottom;

    // Explicit user padding is used as written; a bottom tab bar sits
    // inside it.
    if user_has_custom_padding {
        let bottom = if bottom_tab_visible {
            base_bottom.saturating_sub(tab_bar_height)
        } else {
            base_bottom
        };
        return (base_top, bottom);
    }

    // Managed padding derives every vertical gap from the top padding.
    // Keep the full inset below window chrome and half beside a tab bar or
    // the bottom edge, so a TUI status row does not float above the edge.
    let default_top = config::WindowPadding::default()
        .top
        .evaluate_as_pixels(context) as usize;
    let outer = effective_top_padding(base_top, default_top);
    let tab_bar_gap = outer / 2;

    let top = if show_tab_bar && !tab_bar_at_bottom {
        tab_bar_gap
    } else {
        outer
    };
    let bottom = if bottom_tab_visible {
        tab_bar_gap
    } else if is_edge_to_edge {
        // Maximized windows run content to the bottom edge.
        0
    } else {
        base_bottom.max(tab_bar_gap)
    };

    (top, bottom)
}

/// Computes the effective padding for the RHS.
/// This is needed because the default is 0, but if the user has
/// enabled the scroll bar then they will expect it to have a reasonable
/// size unless they've specified differently.
pub fn effective_right_padding(
    config: &Config,
    context: DimensionContext,
    window_state: WindowState,
) -> usize {
    let padding = effective_window_padding(config, window_state);
    if config.enable_scroll_bar && padding.right.is_zero() {
        context.pixel_cell as usize
    } else {
        padding.right.evaluate_as_pixels(context) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_deferred_font_scale_pty_resize, effective_top_padding,
        effective_vertical_padding_with_policy, finish_deferred_font_scale_pty_resize,
        has_deferred_font_scale_pty_resize, mark_deferred_font_scale_pty_resize,
        note_deferred_font_scale_pty_resize, should_defer_screen_change_scale_update,
        should_normalize_fullscreen_state_on_resize,
        should_preserve_terminal_cells_on_scale_change, user_custom_window_padding_config_path,
    };
    use ::window::WindowState;
    use config::{Config, DimensionContext};
    use std::path::PathBuf;

    fn context() -> DimensionContext {
        DimensionContext {
            dpi: 96.0,
            pixel_max: 800.0,
            pixel_cell: 16.0,
        }
    }

    #[test]
    fn hotkey_layout_consumes_inherited_window_padding_without_chrome_gaps() {
        let mut config = Config::default_config();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Pixels(21.0),
            right: config::Dimension::Pixels(23.0),
            top: config::Dimension::Pixels(7.0),
            bottom: config::Dimension::Pixels(9.0),
        };
        let state = WindowState::HOTKEY_WINDOW;
        let padding = super::effective_window_padding(&config, state);
        assert_eq!(padding.left.evaluate_as_pixels(context()), 21.0);
        assert_eq!(padding.right.evaluate_as_pixels(context()), 23.0);
        assert_eq!(
            effective_vertical_padding_with_policy(
                &config,
                context(),
                false,
                true,
                24,
                state,
                false
            ),
            (7, 9),
        );
    }

    #[test]
    fn hotkey_font_size_persistence_does_not_modify_ordinary_state() {
        assert_eq!(
            super::font_size_state_name(WindowState::empty()),
            ".kaku_font_size"
        );
        for state in [
            WindowState::HOTKEY_WINDOW,
            WindowState::HOTKEY_WINDOW | WindowState::MAXIMIZED,
        ] {
            assert_eq!(super::font_size_state_name(state), ".kaku_hotkey_font_size");
        }
        let source = include_str!("resize.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let adjustment = source.split("pub fn adjust_font_scale(").nth(1).unwrap();
        let adjustment = adjustment
            .split("pub fn decrease_font_size(")
            .next()
            .unwrap();
        let persistence = adjustment
            .split("persist_current_font_size(")
            .nth(1)
            .unwrap();
        let persistence = persistence.split(");").next().unwrap();
        assert!(persistence.contains("&self.config"));
        assert!(persistence.contains("self.fonts.get_font_scale()"));
        assert!(persistence.contains("self.window_state"));
        assert!(source.contains("let file_name = font_size_state_file(window_state);"));
    }

    #[test]
    fn font_size_persistence_roundtrip_isolates_drawer_and_ordinary() {
        let _config = crate::tabbar::test::CONFIG_TEST_LOCK.lock();
        const FIXTURE_ROOT: &str = "KAKU_TEST_FONT_STATE_ROOT";
        let Some(root) = std::env::var_os(FIXTURE_ROOT) else {
            // Lazy config directories must initialize in an isolated process, not race other tests.
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("home")).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "termwindow::resize::tests::font_size_persistence_roundtrip_isolates_drawer_and_ordinary",
                    "--nocapture",
                ])
                .env(FIXTURE_ROOT, dir.path())
                .env("HOME", dir.path().join("home"))
                .env("XDG_CONFIG_HOME", dir.path().join("config"))
                .env_remove("XDG_CONFIG_DIRS")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{}\n{}", stdout, stderr);
            assert!(stdout.contains("1 passed"), "{}", stdout);
            return;
        };

        let root = std::path::PathBuf::from(root);
        assert_eq!(*config::HOME_DIR, root.join("home"));
        assert_eq!(*config::CONFIG_DIRS, vec![root.join("config/kaku")]);
        let ordinary_state = WindowState::empty();
        let drawer_state = WindowState::HOTKEY_WINDOW;
        let ordinary_file = super::font_size_state_file(ordinary_state);
        let drawer_file = super::font_size_state_file(drawer_state);
        for file in [&ordinary_file, &drawer_file] {
            assert_eq!(file.parent().unwrap(), root.join("config/kaku"));
        }

        let mut base = Config::default_config();
        base.font_size = 16.0;
        config::use_this_configuration(base);
        let base = config::configuration();
        super::persist_current_font_size(&base, 1.25, ordinary_state);
        let ordinary_contents = std::fs::read_to_string(&ordinary_file).unwrap();
        assert_eq!(ordinary_contents, "20.000\n");
        super::persist_current_font_size(&base, 1.5, drawer_state);
        assert_eq!(
            std::fs::read_to_string(&ordinary_file).unwrap(),
            ordinary_contents,
            "drawer writes must not overwrite ordinary zoom"
        );
        assert_eq!(std::fs::read_to_string(&drawer_file).unwrap(), "24.000\n");
        assert_eq!(
            super::load_persisted_font_scale(&base, ordinary_state),
            Some(1.25)
        );
        assert_eq!(
            super::load_persisted_font_scale(&base, drawer_state),
            Some(1.5)
        );

        let mut changed_base = Config::default_config();
        changed_base.font_size = 20.0;
        config::use_this_configuration(changed_base.clone());
        let inherited = config::configuration();
        assert_eq!(
            super::load_persisted_font_scale(&inherited, ordinary_state),
            Some(1.0)
        );
        assert_eq!(
            super::load_persisted_font_scale(&inherited, drawer_state),
            Some(1.2)
        );
        changed_base.macos_global_hotkey_appearance.font_size = Some(18.0);
        config::use_this_configuration(changed_base);
        let explicit = config::configuration();
        assert_eq!(
            super::load_persisted_font_scale(&explicit, drawer_state),
            None,
            "explicit drawer font size must ignore persisted zoom"
        );
        assert_eq!(
            super::load_persisted_font_scale(&explicit, ordinary_state),
            Some(1.0)
        );
        assert_eq!(std::fs::read_to_string(&drawer_file).unwrap(), "24.000\n");
        let mut cleared_override = Config::default_config();
        cleared_override.font_size = 20.0;
        config::use_this_configuration(cleared_override);
        assert_eq!(
            super::load_persisted_font_scale(&config::configuration(), drawer_state),
            Some(1.2)
        );

        super::persist_current_font_size(&base, 1.0, drawer_state);
        assert!(!drawer_file.exists());
        assert_eq!(
            std::fs::read_to_string(&ordinary_file).unwrap(),
            ordinary_contents
        );
        super::persist_current_font_size(&base, 1.5, drawer_state);
        super::persist_current_font_size(&base, 1.0, ordinary_state);
        assert!(!ordinary_file.exists());
        assert_eq!(std::fs::read_to_string(&drawer_file).unwrap(), "24.000\n");
        for value in ["NaN", "0", "257", "invalid"] {
            std::fs::write(&drawer_file, value).unwrap();
            assert_eq!(super::load_persisted_font_scale(&base, drawer_state), None);
        }
    }

    #[test]
    fn screen_change_layout_refreshes_even_with_same_dimensions_and_state() {
        for dimensions_unchanged in [false, true] {
            for window_state_unchanged in [false, true] {
                assert!(super::resize_needs_layout_refresh(
                    dimensions_unchanged,
                    window_state_unchanged,
                    true,
                ));
                assert_eq!(
                    super::resize_needs_layout_refresh(
                        dimensions_unchanged,
                        window_state_unchanged,
                        false,
                    ),
                    !dimensions_unchanged || !window_state_unchanged,
                );
            }
        }
    }

    #[test]
    fn screen_change_layout_refresh_guard_is_used_before_loading_os_parameters() {
        let source = include_str!("resize.rs");
        let resize = source
            .split("pub fn resize(")
            .nth(1)
            .unwrap()
            .split("fn flush_all_pane_pty_sizes(")
            .next()
            .unwrap();
        let guard = resize
            .find("if !resize_needs_layout_refresh(")
            .expect("resize must use the tested layout-refresh decision");
        let refresh = resize.find("self.load_os_parameters()").unwrap();
        assert!(guard < refresh);
        let guarded_noop = &resize[guard..refresh];
        assert!(guarded_noop.contains(
            "dimensions_unchanged,\n            self.window_state == normalized_window_state,\n            screen_changed,"
        ));
        assert!(guarded_noop.contains("return;"));
        assert!(!resize[..guard]
            .contains("if dimensions_unchanged && self.window_state == normalized_window_state"));
        assert!(guarded_noop.contains("if was_live_resizing && !live_resizing"));
        assert!(guarded_noop.contains("self.flush_pending_pty_resize()"));
        assert!(guarded_noop.contains("self.schedule_silent_config_reload(window)"));
    }

    #[test]
    fn hotkey_padding_is_independent_of_ordinary_layout_heuristics() {
        let mut config = Config::default();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Pixels(40.0),
            right: config::Dimension::Pixels(50.0),
            top: config::Dimension::Pixels(40.0),
            bottom: config::Dimension::Pixels(30.0),
        };
        let ordinary_config = config.clone();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Pixels(3.0),
            right: config::Dimension::Pixels(5.0),
            top: config::Dimension::Pixels(2.0),
            bottom: config::Dimension::Pixels(4.0),
        };
        for state in [
            WindowState::HOTKEY_WINDOW,
            WindowState::HOTKEY_WINDOW | WindowState::MAXIMIZED,
        ] {
            for show in [false, true] {
                for bottom_tab in [false, true] {
                    for custom in [false, true] {
                        assert_eq!(
                            super::effective_vertical_padding_with_policy(
                                &config,
                                context(),
                                show,
                                bottom_tab,
                                24,
                                state,
                                custom
                            ),
                            (2, 4)
                        );
                    }
                }
            }
            assert_eq!(
                super::effective_window_padding(&config, state)
                    .left
                    .evaluate_as_pixels(context()),
                3.0
            );
            assert_eq!(super::effective_right_padding(&config, context(), state), 5);
        }
        assert_eq!(
            super::effective_window_padding(&ordinary_config, WindowState::empty())
                .left
                .evaluate_as_pixels(context()),
            40.0
        );
        assert_eq!(
            super::effective_right_padding(&ordinary_config, context(), WindowState::empty()),
            50
        );
        assert_eq!(
            super::effective_vertical_padding_with_policy(
                &ordinary_config,
                context(),
                false,
                false,
                0,
                WindowState::empty(),
                true
            ),
            (40, 30)
        );
    }

    #[test]
    fn hotkey_padding_supports_zero_and_units_without_visual_gaps() {
        let mut config = Config::default();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Cells(1.0),
            right: config::Dimension::Percent(0.01),
            top: config::Dimension::Points(3.0),
            bottom: config::Dimension::Pixels(0.0),
        };
        let context = DimensionContext {
            dpi: 192.0,
            ..context()
        };
        assert_eq!(
            super::effective_vertical_padding_with_policy(
                &config,
                context,
                true,
                true,
                24,
                WindowState::HOTKEY_WINDOW,
                false
            ),
            (8, 0)
        );
        assert_eq!(
            super::effective_window_padding(&config, WindowState::HOTKEY_WINDOW)
                .left
                .evaluate_as_pixels(context),
            16.0
        );
        assert_eq!(
            super::effective_right_padding(&config, context, WindowState::HOTKEY_WINDOW),
            8
        );
    }

    #[test]
    fn hotkey_zero_padding_stays_zero_without_scrollbar() {
        let mut config = Config::default();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Pixels(0.0),
            right: config::Dimension::Pixels(0.0),
            top: config::Dimension::Pixels(0.0),
            bottom: config::Dimension::Pixels(0.0),
        };
        for show in [false, true] {
            for bottom in [false, true] {
                assert_eq!(
                    super::effective_vertical_padding_with_policy(
                        &config,
                        context(),
                        show,
                        bottom,
                        24,
                        WindowState::HOTKEY_WINDOW,
                        false
                    ),
                    (0, 0)
                );
            }
        }
        assert_eq!(
            super::effective_window_padding(&config, WindowState::HOTKEY_WINDOW)
                .left
                .evaluate_as_pixels(context()),
            0.0
        );
        assert_eq!(
            super::effective_right_padding(&config, context(), WindowState::HOTKEY_WINDOW),
            0
        );
        config.enable_scroll_bar = true;
        assert_eq!(
            super::effective_right_padding(&config, context(), WindowState::HOTKEY_WINDOW),
            16
        );
    }

    #[test]
    fn hotkey_percentage_padding_uses_stable_window_bounds() {
        let percent = config::Dimension::Percent(0.1);
        for cells_before_or_after_resize in [1000, 800, 640] {
            let ctx = DimensionContext {
                pixel_max: super::padding_pixel_max(
                    WindowState::HOTKEY_WINDOW,
                    1000,
                    cells_before_or_after_resize,
                ),
                ..context()
            };
            assert_eq!(percent.evaluate_as_pixels(ctx), 100.0);
        }
        assert_eq!(
            super::padding_pixel_max(WindowState::empty(), 1000, 800),
            800.0
        );
    }

    #[test]
    fn deferred_font_scale_resize_throttles_pty_like_tmux() {
        let window_id = usize::MAX - 407;
        clear_deferred_font_scale_pty_resize(window_id);

        mark_deferred_font_scale_pty_resize(window_id);
        assert!(has_deferred_font_scale_pty_resize(window_id));

        // First step reaches the PTY at once; steps inside the window coalesce.
        assert!(note_deferred_font_scale_pty_resize(window_id));
        assert!(!note_deferred_font_scale_pty_resize(window_id));
        assert!(!note_deferred_font_scale_pty_resize(window_id));

        // Timer: the waiting step is sent and the window stays open.
        assert!(finish_deferred_font_scale_pty_resize(window_id));
        assert!(has_deferred_font_scale_pty_resize(window_id));

        // Timer with nothing new: the zoom is over.
        assert!(!finish_deferred_font_scale_pty_resize(window_id));
        assert!(!has_deferred_font_scale_pty_resize(window_id));
    }

    #[test]
    fn deferred_font_scale_resize_reflows_pane_grids_without_pty() {
        // Each Cmd+/- step must resize every pane's terminal state, or the old
        // grid renders at the new cell size until the debounce flush.
        let source = include_str!("resize.rs");
        let body = source.split("pub fn apply_dimensions(").nth(1).unwrap();
        let body = body.split("\n    }\n").next().unwrap();
        let branch = body
            .split("if defer_font_scale_pty_resize {")
            .nth(1)
            .unwrap();
        let branch = branch.split("for tab in window.iter()").next().unwrap();
        assert!(branch.contains("tab.iter_panes()"));
        assert!(branch.contains("pos.pane.resize_visual("));
        assert!(!branch.contains("pos.pane.resize("));
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
            if is_fullscreen {
                WindowState::MAXIMIZED
            } else {
                WindowState::empty()
            },
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
            if is_fullscreen {
                WindowState::MAXIMIZED
            } else {
                WindowState::empty()
            },
            true,
        )
    }

    #[test]
    fn explicit_top_padding_can_disable_visual_spacing() {
        assert_eq!(effective_top_padding(0, 8), 0);
        assert_eq!(effective_top_padding(12, 8), 12);
    }

    #[test]
    fn custom_padding_path_defaults_to_user_config() {
        let _config = crate::tabbar::test::CONFIG_TEST_LOCK.lock();
        config::clear_config_file_override();
        assert_eq!(
            user_custom_window_padding_config_path(),
            config::user_config_path()
        );
    }

    #[test]
    fn explicit_override_path_wins_for_custom_padding_detection() {
        let _config = crate::tabbar::test::CONFIG_TEST_LOCK.lock();
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
    fn managed_padding_derives_every_gap_from_the_top_padding() {
        let mut config = Config::default_config();
        config.window_padding = config::WindowPadding {
            left: config::Dimension::Pixels(40.0),
            right: config::Dimension::Pixels(40.0),
            top: config::Dimension::Pixels(40.0),
            bottom: config::Dimension::Pixels(0.0),
        };
        for state in [
            WindowState::empty(),
            WindowState::FULL_SCREEN,
            WindowState::MAXIMIZED,
        ] {
            let pad = |show, at_bottom| {
                effective_vertical_padding_with_policy(
                    &config,
                    context(),
                    show,
                    at_bottom,
                    58,
                    state,
                    false,
                )
            };
            let edge = if state.contains(WindowState::MAXIMIZED) {
                0
            } else {
                20
            };
            // The bottom edge uses the same compact gap as a visible bar.
            assert_eq!(pad(false, true), (40, edge), "{state:?}");
            assert_eq!(pad(false, false), (40, edge), "{state:?}");
            assert_eq!(pad(true, true), (40, 20), "{state:?}");
            assert_eq!(pad(true, false), (20, edge), "{state:?}");
        }
    }

    #[test]
    fn managed_padding_keeps_a_larger_explicit_bottom() {
        let mut config = Config::default_config();
        config.window_padding.top = config::Dimension::Pixels(40.0);
        config.window_padding.bottom = config::Dimension::Pixels(60.0);

        let (_, bottom) = effective_vertical_padding(&config, context(), false, true, 58, false);

        assert_eq!(bottom, 60);
    }

    #[test]
    fn custom_padding_puts_a_bottom_tab_bar_inside_the_bottom_padding() {
        let mut config = Config::default_config();
        config.window_padding.bottom = config::Dimension::Pixels(40.0);

        let (_, bottom) =
            effective_vertical_padding_user_custom(&config, context(), true, true, 24, false);
        assert_eq!(bottom, 16);
        let (_, bottom) =
            effective_vertical_padding_user_custom(&config, context(), true, true, 24, true);
        assert_eq!(bottom, 16);
    }

    #[test]
    fn normalize_fullscreen_on_unfocused_top_tab_resize_even_with_dim_change() {
        assert!(should_normalize_fullscreen_state_on_resize(
            false,                  // top-tab
            true,                   // was fullscreen
            WindowState::default(), // transient incoming non-fullscreen
            false,                  // unfocused
            false,                  // dimensions changed
        ));
    }

    #[test]
    fn normalize_fullscreen_on_focused_top_tab_requires_same_dimensions() {
        assert!(should_normalize_fullscreen_state_on_resize(
            false,
            true,
            WindowState::default(),
            true,
            true
        ));
        assert!(!should_normalize_fullscreen_state_on_resize(
            false,
            true,
            WindowState::default(),
            true,
            false
        ));
    }

    #[test]
    fn normalize_fullscreen_disabled_for_bottom_tab() {
        assert!(!should_normalize_fullscreen_state_on_resize(
            true,
            true,
            WindowState::default(),
            false,
            true
        ));
    }

    #[test]
    fn hotkey_drawer_never_keeps_the_ordinary_windows_fullscreen_state() {
        for focused in [false, true] {
            for same_dimensions in [false, true] {
                assert!(!should_normalize_fullscreen_state_on_resize(
                    false,
                    true,
                    WindowState::HOTKEY_WINDOW,
                    focused,
                    same_dimensions,
                ));
            }
        }
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
    fn screen_change_dpi_update_is_not_stabilized_as_transition() {
        use super::should_stabilize_transition_dpi;
        // Genuine same-screen transitions keep the current dpi for the frame.
        assert!(should_stabilize_transition_dpi(true, false, false, 72, 144));
        assert!(should_stabilize_transition_dpi(false, true, false, 72, 144));
        // Display reconfiguration (lock-screen return with an external
        // monitor): the incoming dpi is authoritative even when a phantom
        // maximize/fullscreen transition rides along (#477).
        assert!(!should_stabilize_transition_dpi(false, true, true, 72, 144));
        assert!(!should_stabilize_transition_dpi(true, false, true, 72, 144));
        // No dpi delta or no transition: nothing to stabilize.
        assert!(!should_stabilize_transition_dpi(
            false, true, false, 144, 144
        ));
        assert!(!should_stabilize_transition_dpi(
            false, false, false, 72, 144
        ));
    }
}
