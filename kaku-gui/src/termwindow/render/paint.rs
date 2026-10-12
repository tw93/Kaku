use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::termwindow::box_model::*;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::render::forces_opaque_kaku_tui_window_background;
use crate::termwindow::{DimensionContext, RenderFrame, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::WindowOps;
use anyhow::Context;
use config::Dimension;
use smol::Timer;
use std::time::{Duration, Instant};
use wezterm_font::ClearShapeCache;
use window::color::LinearRgba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowImage {
    Yes,
    Scale(usize),
    No,
}

const STATUS_DOT_SIZE: f32 = 14.0;

static ACTIVE_PANE_INDICATOR_POLY: &[Poly] = &[Poly {
    path: &[PolyCommand::Circle {
        center: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
        radius: BlockCoord::Frac(1, 2),
    }],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];

fn toast_colors_for_palette(
    palette: &wezterm_term::color::ColorPalette,
    alpha: f32,
) -> (LinearRgba, LinearRgba) {
    if crate::termwindow::is_light_color(&palette.background) {
        let bg_linear = palette.colors.0[3].to_linear();
        (
            LinearRgba(bg_linear.0, bg_linear.1, bg_linear.2, 0.9 * alpha),
            LinearRgba(0.1, 0.1, 0.1, alpha),
        )
    } else {
        let bg_linear = palette.colors.0[13].to_linear();
        (
            LinearRgba(bg_linear.0, bg_linear.1, bg_linear.2, 0.9 * alpha),
            LinearRgba(1.0, 1.0, 1.0, alpha),
        )
    }
}

impl crate::TermWindow {
    pub fn paint_impl(&mut self, frame: &mut RenderFrame) -> anyhow::Result<()> {
        self.num_frames += 1;
        let is_first_paint = !self.first_paint_logged;
        if is_first_paint {
            crate::startup_trace::mark("first paint_impl start");
        }
        // If nothing on screen needs animating, then we can avoid
        // invalidating as frequently
        *self.has_animation.borrow_mut() = None;
        // Start with the assumption that we should allow images to render
        self.allow_images = AllowImage::Yes;

        let start = Instant::now();

        {
            let diff = start.duration_since(self.last_fps_check_time);
            if diff > Duration::from_secs(1) {
                let seconds = diff.as_secs_f32();
                self.fps = self.num_frames as f32 / seconds;
                self.num_frames = 0;
                self.last_fps_check_time = start;
            }
        }

        // Limit retries to prevent a busy-loop when render state cannot converge
        // (e.g. during display reconfiguration after lock-screen return).
        const MAX_PAINT_RETRIES: usize = 8;
        // Set while the most recent pass had to grow the quad buffers: the
        // vertices staged by that pass were truncated to the old capacity,
        // so the frame we are about to draw is incomplete.
        let mut quads_grew = false;
        'pass: for pass in 0..MAX_PAINT_RETRIES {
            match self.paint_pass() {
                Ok(_) => match self.render_state.as_mut().unwrap().allocated_more_quads() {
                    Ok(allocated) => {
                        if !allocated {
                            quads_grew = false;
                            break 'pass;
                        }
                        quads_grew = true;
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                    }
                    Err(err) => {
                        // The grow failed (allocation), so capacity is
                        // unchanged and a forced repaint would fail the same
                        // way every frame. Draw the clamped frame and stop:
                        // do not request another pass.
                        quads_grew = false;
                        log::error!("{:#}", err);
                        break 'pass;
                    }
                },
                Err(err) => {
                    if let Some(&OutOfTextureSpace {
                        size: Some(size),
                        current_size,
                    }) = err.root_cause().downcast_ref::<OutOfTextureSpace>()
                    {
                        let result = if pass == 0 {
                            // Let's try clearing out the atlas and trying again
                            // self.clear_texture_atlas()
                            log::trace!("recreate_texture_atlas");
                            self.recreate_texture_atlas(Some(current_size))
                        } else {
                            log::trace!("grow texture atlas to {}", size);
                            self.recreate_texture_atlas(Some(size))
                        };
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();

                        if let Err(err) = result {
                            self.allow_images = match self.allow_images {
                                AllowImage::Yes => AllowImage::Scale(2),
                                AllowImage::Scale(2) => AllowImage::Scale(4),
                                AllowImage::Scale(4) => AllowImage::Scale(8),
                                AllowImage::Scale(8) => AllowImage::No,
                                AllowImage::No | _ => {
                                    log::error!(
                                        "Failed to {} texture: {}",
                                        if pass == 0 { "clear" } else { "resize" },
                                        err
                                    );
                                    break 'pass;
                                }
                            };

                            log::info!(
                                "Not enough texture space ({:#}); \
                                     will retry render with {:?}",
                                err,
                                self.allow_images,
                            );
                        }
                    } else if err.root_cause().downcast_ref::<ClearShapeCache>().is_some() {
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                        self.shape_generation += 1;
                        self.shape_cache.borrow_mut().clear();
                        self.line_to_ele_shape_cache.borrow_mut().clear();
                    } else {
                        log::error!("paint_pass failed: {:#}", err);
                        break 'pass;
                    }
                }
            }
        }

        if start.elapsed() > Duration::from_millis(200) {
            log::warn!(
                "paint_impl retry loop exceeded {}ms ({}ms), likely display reconfiguration",
                200,
                start.elapsed().as_millis()
            );
        }

        if quads_grew {
            // We ran out of passes while still growing the quad buffers, so
            // this frame is drawn with truncated geometry. The buffers are
            // now large enough, so ask for one more frame to fill it in
            // rather than leaving a partially rendered window on screen.
            log::warn!(
                "paint_pass did not converge within {} passes; \
                 quad buffers were grown, repainting",
                MAX_PAINT_RETRIES
            );
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }

        log::debug!("paint_impl before call_draw elapsed={:?}", start.elapsed());

        self.call_draw(frame)?;
        self.last_frame_duration = start.elapsed();
        log::debug!(
            "paint_impl elapsed={:?}, fps={}",
            self.last_frame_duration,
            self.fps
        );
        metrics::histogram!("gui.paint.impl").record(self.last_frame_duration);
        metrics::histogram!("gui.paint.impl.rate").record(1.);

        // If self.has_animation is some, then the last render detected
        // image attachments with multiple frames, so we also need to
        // invalidate the viewport when the next frame is due
        if self.focused.is_some() {
            if let Some(next_due) = *self.has_animation.borrow() {
                let prior = self.scheduled_animation.borrow_mut().take();
                match prior {
                    Some(prior) if prior <= next_due => {
                        // Already due before that time
                    }
                    _ => {
                        self.scheduled_animation.borrow_mut().replace(next_due);
                        let window = self.window.clone().take().unwrap();
                        promise::spawn::spawn(async move {
                            Timer::at(next_due).await;
                            let win = window.clone();
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.scheduled_animation.borrow_mut().take();
                                // Modal content is cached, so blinking carets and other
                                // time-based modal elements must be explicitly reconfigured.
                                tw.invalidate_modal();
                                win.invalidate();
                            })));
                        })
                        .detach();
                    }
                }
            }
        }

        if is_first_paint {
            self.first_paint_logged = true;
            crate::startup_trace::mark("first paint_impl done");
            // Start the update checker only after the first frame is on
            // screen: its synchronous notification-center init (and the
            // first-launch permission prompt on macOS) has no business on
            // the first-paint critical path.
            promise::spawn::spawn_with_low_priority(async {
                crate::update::start_update_checker();
            })
            .detach();
        }

        Ok(())
    }

    pub fn paint_modal(&mut self) -> anyhow::Result<()> {
        if let Some(modal) = self.get_modal() {
            for computed in modal.computed_element(self)?.iter() {
                let mut ui_items = computed.ui_items();

                let gl_state = self.render_state.as_ref().unwrap();
                self.render_element(&computed, gl_state, None)?;

                self.ui_items.append(&mut ui_items);
            }
        }

        Ok(())
    }

    pub fn paint_pass(&mut self) -> anyhow::Result<()> {
        {
            let gl_state = self.render_state.as_ref().unwrap();
            for layer in gl_state.layers.borrow().iter() {
                layer.clear_quad_allocation();
            }
        }

        // Clear out UI item positions; we'll rebuild these as we render
        self.ui_items.clear();

        let panes = self.get_panes_to_render();
        let focused = self.focused.is_some();
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;
        let force_opaque_window_background = forces_opaque_kaku_tui_window_background(&panes);
        let effective_window_is_transparent =
            window_is_transparent && !force_opaque_window_background;

        let start = Instant::now();
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state
            .layer_for_zindex(0)
            .context("layer_for_zindex(0)")?;
        let mut layers = layer.quad_allocator();
        log::trace!("quad map elapsed {:?}", start.elapsed());
        metrics::histogram!("quad.map").record(start.elapsed());

        let mut paint_terminal_background = false;

        // Render the full window background
        if force_opaque_window_background {
            paint_terminal_background = true;
        } else {
            match (self.window_background.is_empty(), self.allow_images) {
                (false, AllowImage::Yes | AllowImage::Scale(_)) => {
                    let bg_color = self.palette().background.to_linear();

                    let top = panes
                        .iter()
                        .find(|p| p.is_active)
                        .map(|p| match self.effective_viewport(&p.pane) {
                            Some(top) => top,
                            None => p.pane.get_dimensions().physical_top,
                        })
                        .unwrap_or(0);

                    let loaded_any = self
                        .render_backgrounds(bg_color, top)
                        .context("render_backgrounds")?;

                    if !loaded_any {
                        // Either there was a problem loading the background(s)
                        // or they haven't finished loading yet.
                        // Use the regular terminal background until that changes.
                        paint_terminal_background = true;
                    }
                }
                _ if effective_window_is_transparent => {
                    // Avoid doubling up the background color for the main pane area.
                    // We still need to paint strips that are intentionally excluded
                    // from pane background quads.
                    let strip_background = panes
                        .iter()
                        .find(|pane| pane.is_active)
                        .or_else(|| panes.first())
                        .map(|pane| pane.pane.palette().background)
                        .unwrap_or_else(|| self.palette().background)
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity);
                    let border = self.get_os_border();
                    let tab_bar_height = if self.show_tab_bar {
                        self.tab_bar_pixel_height()
                            .context("tab_bar_pixel_height")?
                    } else {
                        0.0
                    };
                    let (_, padding_bottom) = self.effective_vertical_padding();
                    let padding_bottom = padding_bottom as f32;
                    // Only cover the OS border area; the tab bar paints its own
                    // transparent background in paint_tab_bar(), so including
                    // tab_bar_height here would double-paint that region.
                    // When the tab bar is at top, it owns the titlebar strip
                    // and paints that area itself, so no separate top fill is needed.
                    // paint_window_borders() repaints the full top OS border
                    // with the pane background (on a higher layer) whenever the
                    // integrated-buttons inset is active. Filling it here too
                    // would stack two semi-transparent quads, making the top
                    // strip read as a darker, near-opaque band under
                    // window_background_opacity < 1, so skip it in that case.
                    let top_fill_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                        0.0
                    } else if super::borders::integrated_buttons_top_inset(
                        &self.config,
                        self.effective_layout_window_state(),
                        self.show_tab_bar && !self.config.tab_bar_at_bottom,
                        self.dimensions.dpi as f32,
                    ) > 0
                    {
                        0.0
                    } else {
                        border.top.get() as f32
                    };
                    // Same reasoning as top: only cover the OS border + padding.
                    // The tab bar paints its own transparent background.
                    // When tab bar is at bottom, it covers the bottom border area,
                    // so only fill the padding area.
                    let bottom_fill_height = if self.show_tab_bar && self.config.tab_bar_at_bottom {
                        padding_bottom
                    } else {
                        padding_bottom + border.bottom.get() as f32
                    };
                    let right_fill_width = self.effective_right_padding(&self.config) as f32
                        + border.right.get() as f32;
                    // Use content_left_inset() to include content-alignment gap;
                    // the leftmost pane background will start at this boundary so
                    // the two regions don't overlap.
                    let left_fill_width = self.content_left_inset();
                    let window_width = self.dimensions.pixel_width as f32;
                    let window_height = self.dimensions.pixel_height as f32;

                    if top_fill_height > 0.0 {
                        self.filled_rectangle(
                            &mut layers,
                            0,
                            euclid::rect(
                                0.0,
                                0.0,
                                window_width,
                                top_fill_height.min(window_height),
                            ),
                            strip_background,
                        )
                        .context("filled_rectangle for transparent top strip")?;
                    }

                    if bottom_fill_height > 0.0 {
                        let clamped_height = bottom_fill_height.min(window_height);
                        self.filled_rectangle(
                            &mut layers,
                            0,
                            euclid::rect(
                                0.0,
                                (window_height - clamped_height).max(0.0),
                                window_width,
                                clamped_height,
                            ),
                            strip_background,
                        )
                        .context("filled_rectangle for transparent bottom strip")?;
                    }

                    // The tab bar paints its own full-width background, so the
                    // left/right side strips must skip the tab bar region to avoid
                    // double-painting.
                    let tab_bar_top_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom
                    {
                        tab_bar_height
                    } else {
                        0.0
                    };
                    let tab_bar_bottom_height =
                        if self.show_tab_bar && self.config.tab_bar_at_bottom {
                            tab_bar_height
                        } else {
                            0.0
                        };
                    let side_fill_y = (top_fill_height + tab_bar_top_height).min(window_height);
                    // When tab bar is at bottom, side fills should extend to tab bar top,
                    // not to (bottom_fill_height + tab_bar_height) which would leave a gap.
                    let side_fill_height = if self.show_tab_bar && self.config.tab_bar_at_bottom {
                        (window_height - side_fill_y - tab_bar_height).max(0.0)
                    } else {
                        (window_height
                            - side_fill_y
                            - (bottom_fill_height + tab_bar_bottom_height).min(window_height))
                        .max(0.0)
                    };

                    if right_fill_width > 0.0 {
                        let clamped_width = right_fill_width.min(window_width);
                        self.filled_rectangle(
                            &mut layers,
                            0,
                            euclid::rect(
                                window_width - clamped_width,
                                side_fill_y,
                                clamped_width,
                                side_fill_height,
                            ),
                            strip_background,
                        )
                        .context("filled_rectangle for transparent right strip")?;
                    }

                    if left_fill_width > 0.0 {
                        let clamped_width = left_fill_width.min(window_width);
                        self.filled_rectangle(
                            &mut layers,
                            0,
                            euclid::rect(0.0, side_fill_y, clamped_width, side_fill_height),
                            strip_background,
                        )
                        .context("filled_rectangle for transparent left strip")?;
                    }
                }
                _ => {
                    paint_terminal_background = true;
                }
            }
        }

        if paint_terminal_background {
            // Regular window background color
            let background = if panes.len() == 1 {
                // If we're the only pane, use the pane's palette
                // to draw the padding background
                panes[0].pane.palette().background
            } else {
                self.palette().background
            }
            .to_linear()
            .mul_alpha(if force_opaque_window_background {
                1.0
            } else {
                self.config.window_background_opacity
            });

            self.filled_rectangle(
                &mut layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                background,
            )
            .context("filled_rectangle for window background")?;
        }

        let hide_transition_content = self
            .window
            .as_ref()
            .map(|window| window.is_zoom_animation_active())
            .unwrap_or(false);
        if hide_transition_content {
            // During fullscreen transition, keep only a stable background to avoid
            // one-frame text scale pops.
            let hide_background = self.palette().background.to_linear();
            self.filled_rectangle(
                &mut layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                hide_background,
            )
            .context("filled_rectangle for fullscreen transition hide")?;
            drop(layers);
            return Ok(());
        }

        let num_panes = panes.len();
        let mut active_pane_top_right: Vec<(f32, f32, bool)> = vec![];

        for pos in panes {
            if num_panes > 1 && pos.is_active {
                let cell_width = self.render_metrics.cell_size.width as f32;
                let cell_height = self.render_metrics.cell_size.height as f32;
                let (_, padding_top) = self.padding_left_top();
                let border = self.get_os_border();
                let tab_bar_height = if self.show_tab_bar {
                    self.tab_bar_pixel_height().unwrap_or(0.)
                } else {
                    0.
                };
                let top_bar_height = if self.config.tab_bar_at_bottom {
                    0.0
                } else {
                    tab_bar_height
                };
                let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

                let x = self.content_left_inset() + ((pos.left + pos.width) as f32 * cell_width);
                let y = top_pixel_y + (pos.top as f32 * cell_height);
                let is_top_pane = pos.top == 0;
                active_pane_top_right.push((x, y, is_top_pane));
            }
            if pos.is_active {
                if self.get_modal().is_none() {
                    self.update_text_cursor(&pos);
                }
                if focused {
                    pos.pane.advise_focus();
                    mux::Mux::get().record_focus_for_current_identity(pos.pane.pane_id());
                }
                // Attention state is cleared for this focused pane after panes render.
            }
            self.paint_pane(&pos, &mut layers).context("paint_pane")?;
        }

        const RIGHT_INSET: f32 = 3.0;
        const TOP_PANE_MARGIN_WITH_TAB_BAR: f32 = 24.0;
        const TOP_PANE_MARGIN_NO_TAB_BAR: f32 = 14.0;
        const LOWER_PANE_MARGIN: f32 = 20.0;

        // Draw a dot indicator for the active pane when the tab is split.
        for (dot_x, dot_y, is_top_pane) in active_pane_top_right {
            let top_pane_margin = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                TOP_PANE_MARGIN_WITH_TAB_BAR
            } else {
                TOP_PANE_MARGIN_NO_TAB_BAR
            };
            let margin_top = if is_top_pane {
                top_pane_margin
            } else {
                LOWER_PANE_MARGIN
            };

            const DOT_ALPHA: f32 = 0.5;
            let dot_color = self.palette().cursor_bg.to_linear().mul_alpha(DOT_ALPHA);

            self.poly_quad(
                &mut layers,
                2,
                euclid::point2(dot_x - STATUS_DOT_SIZE - RIGHT_INSET, dot_y + margin_top),
                ACTIVE_PANE_INDICATOR_POLY,
                1,
                euclid::size2(STATUS_DOT_SIZE, STATUS_DOT_SIZE),
                dot_color,
            )
            .context("active pane indicator")?;
        }

        if let Some(pane) = self.get_active_pane_or_overlay() {
            let splits = self.get_splits();
            for split in &splits {
                self.paint_split(&mut layers, split, &splits, &pane)
                    .context("paint_split")?;
            }
        }

        // Clear attention only for the focused pane. Other panes in the active
        // tab keep their unread state until the user focuses them.
        self.clear_active_pane_attention_state();

        // Draw visual notification dot on inactive tabs with unread bell
        if self.show_tab_bar {
            self.paint_tab_bar(&mut layers).context("paint_tab_bar")?;
            self.paint_tab_bell_indicators(&mut layers)?;
        }

        self.paint_window_borders(&mut layers)
            .context("paint_window_borders")?;
        drop(layers);
        self.paint_modal().context("paint_modal")?;
        self.paint_toast().context("paint_toast")?;

        Ok(())
    }

    /// Clear notification and bell state for the focused pane only.
    /// Called every paint cycle so state stays in sync even when the tab bar is hidden.
    fn clear_active_pane_attention_state(&mut self) {
        if self.focused.is_none() {
            return;
        }
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        let mut state = self.pane_state(pane.pane_id());
        state.has_unread_notification = false;
        if state.has_unread_bell {
            state.has_unread_bell = false;
            drop(state);
            crate::frontend::front_end().adjust_unread_bell_count(-1);
        }
    }

    /// Draw a dot on inactive tabs that have panes with unread bell events.
    fn paint_tab_bell_indicators(
        &mut self,
        _layers: &mut crate::quad::TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        // Kaku renders unread bell state inline in the bundled format-tab-title
        // callback, so the core tab-dot painter would be redundant.
        Ok(())
    }

    /// Render the toast notification
    pub fn paint_toast(&mut self) -> anyhow::Result<()> {
        let (toast_at, message, lifetime) = match &self.toast {
            Some((t, msg, lifetime)) if t.elapsed() < *lifetime => (*t, msg.clone(), *lifetime),
            _ => return Ok(()),
        };

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());

        // Fade out during the last 500ms of the configured lifetime.
        let elapsed_ms = toast_at.elapsed().as_millis() as f32;
        let lifetime_ms = lifetime.as_millis() as f32;
        let fade_start_ms = (lifetime_ms - 500.0).max(0.0);
        let alpha = if elapsed_ms > fade_start_ms {
            (1.0 - (elapsed_ms - fade_start_ms) / 500.0).max(0.0)
        } else {
            1.0
        };

        // Match the toast to the currently visible terminal palette so it
        // stays in sync with theme changes and client palette overrides.
        let palette = if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.palette()
        } else {
            self.palette().clone()
        };
        let (bg_color, text_color) = toast_colors_for_palette(&palette, alpha);
        let toast_radius = Dimension::Pixels(8.0);

        let text = Element::new(&font, ElementContent::Text(message.clone()))
            .colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: text_color.into(),
            })
            .display(DisplayType::Block);

        let element = Element::new(&font, ElementContent::Children(vec![text]))
            .colors(ElementColors {
                // Rounded corner polys use border colors even if the border
                // width is zero; match bg to avoid corner gaps.
                border: BorderColor::new(bg_color.into()),
                bg: bg_color.into(),
                text: text_color.into(),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.75),
                right: Dimension::Cells(0.75),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .border(BoxDimension::new(Dimension::Pixels(0.0)))
            .border_corners(Some(Corners {
                top_left: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: TOP_LEFT_ROUNDED_CORNER,
                },
                top_right: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: TOP_RIGHT_ROUNDED_CORNER,
                },
                bottom_left: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: BOTTOM_LEFT_ROUNDED_CORNER,
                },
                bottom_right: SizedPoly {
                    width: toast_radius,
                    height: toast_radius,
                    poly: BOTTOM_RIGHT_ROUNDED_CORNER,
                },
            }));

        let dimensions = self.dimensions;
        let border = self.get_os_border();
        // The toast repaints every frame while animating its fade, but the
        // message text is fixed for the toast's lifetime: shape it once and
        // reuse the measured width on subsequent frames.
        let config_generation = self.config.generation();
        let text_pixel_width: f32 = match &self.toast_shaped_width {
            Some((cached_message, cached_dpi, cached_generation, width))
                if *cached_message == message
                    && *cached_dpi == dimensions.dpi
                    && *cached_generation == config_generation =>
            {
                *width
            }
            _ => {
                let shape_window = self.window.as_ref().unwrap().clone();
                let shaped = font.shape(
                    &message,
                    move || shape_window.notify(TermWindowNotif::InvalidateShapeCache),
                    |_| {},
                    None,
                    wezterm_bidi::Direction::LeftToRight,
                    None,
                    None,
                )?;
                let width: f32 = shaped.iter().map(|g| g.x_advance.get() as f32).sum();
                self.toast_shaped_width =
                    Some((message.clone(), dimensions.dpi, config_generation, width));
                width
            }
        };
        // 1.5 cells matches the Cells(0.75) horizontal padding; ceil + 1 px guards against rounding.
        let approx_width = (text_pixel_width + 1.5 * metrics.cell_size.width as f32).ceil() + 1.0;
        let toast_height = metrics.cell_size.height as f32 * 1.5;
        // Use consistent margin based on cell size
        let h_margin = metrics.cell_size.width as f32 * 2.0;
        let v_margin = metrics.cell_size.height as f32 * 2.0;

        // Position at bottom-right with fixed margin from window edge
        let right_x =
            dimensions.pixel_width as f32 - approx_width - h_margin - border.right.get() as f32;
        let bottom_y =
            dimensions.pixel_height as f32 - toast_height - v_margin - border.bottom.get() as f32;

        let computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(right_x, bottom_y, approx_width, toast_height),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 120,
            },
            &element,
        )?;

        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)?;

        // Keep redrawing during fade-out
        if elapsed_ms > fade_start_ms {
            let next = Instant::now() + Duration::from_millis(16);
            let mut anim = self.has_animation.borrow_mut();
            match *anim {
                Some(existing) if existing <= next => {}
                _ => {
                    *anim = Some(next);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::toast_colors_for_palette;
    use wezterm_term::color::{ColorPalette, SrgbaTuple};
    use window::color::LinearRgba;

    #[test]
    fn light_palette_uses_gold_background_and_dark_text() {
        let mut palette = ColorPalette::default();
        palette.background = SrgbaTuple(0.95, 0.95, 0.95, 1.0);
        palette.colors.0[3] = SrgbaTuple(0.8, 0.7, 0.2, 1.0);

        let (bg, text) = toast_colors_for_palette(&palette, 1.0);
        let expected_bg = palette.colors.0[3].to_linear();

        assert_eq!(
            bg,
            LinearRgba(expected_bg.0, expected_bg.1, expected_bg.2, 0.9)
        );
        assert_eq!(text, LinearRgba(0.1, 0.1, 0.1, 1.0));
    }

    #[test]
    fn dark_palette_uses_accent_background_and_light_text() {
        let mut palette = ColorPalette::default();
        palette.background = SrgbaTuple(0.08, 0.08, 0.08, 1.0);
        palette.colors.0[13] = SrgbaTuple(0.5, 0.3, 0.8, 1.0);

        let (bg, text) = toast_colors_for_palette(&palette, 1.0);
        let expected_bg = palette.colors.0[13].to_linear();

        assert_eq!(
            bg,
            LinearRgba(expected_bg.0, expected_bg.1, expected_bg.2, 0.9)
        );
        assert_eq!(text, LinearRgba(1.0, 1.0, 1.0, 1.0));
    }
}
