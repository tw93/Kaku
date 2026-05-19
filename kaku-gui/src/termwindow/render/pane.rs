use crate::quad::{HeapQuadAllocator, QuadTrait, TripleLayerQuadAllocator};
use crate::selection::SelectionRange;
use crate::termwindow::box_model::*;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::render::{
    forces_opaque_kaku_tui_background, same_hyperlink, CursorProperties, LineQuadCacheKey,
    LineQuadCacheValue, LineToEleShapeCacheKey, RenderScreenLineParams,
};
use crate::termwindow::{ScrollHit, UIItem, UIItemType};
use ::window::bitmaps::TextureRect;
use ::window::DeadKeyStatus;
use anyhow::Context;
use config::VisualBellTarget;
use mux::pane::{PaneId, WithPaneLines};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::PositionedPane;
use ordered_float::NotNan;
use std::time::Instant;
use wezterm_dynamic::Value;
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;

impl crate::TermWindow {
    fn paint_pane_box_model(&mut self, pos: &PositionedPane) -> anyhow::Result<()> {
        let computed = self.build_pane(pos)?;
        let mut ui_items = computed.ui_items();
        self.ui_items.append(&mut ui_items);
        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)
    }

    fn paint_scrollbar_thumb(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        let radius = (width.min(height) / 4)
            .clamp(2, 4)
            .min(width / 2)
            .min(height / 2);

        if radius == 0 || width <= radius * 2 || height <= radius * 2 {
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(x as f32, y as f32, width as f32, height as f32),
                color,
            )
            .context("scrollbar thumb")?;
            return Ok(());
        }

        let radius_f = radius as f32;
        let inner_width = width.saturating_sub(radius * 2) as f32;
        let inner_height = height.saturating_sub(radius * 2) as f32;

        // Fill the center column and side columns first, then soften the
        // corners with small rounded masks so the thumb feels intentional
        // without turning into a capsule with circular endpoints.
        self.filled_rectangle(
            layers,
            2,
            euclid::rect((x + radius) as f32, y as f32, inner_width, height as f32),
            color,
        )
        .context("scrollbar thumb center")?;

        self.filled_rectangle(
            layers,
            2,
            euclid::rect(x as f32, (y + radius) as f32, radius_f, inner_height),
            color,
        )
        .context("scrollbar thumb left")?;

        self.filled_rectangle(
            layers,
            2,
            euclid::rect(
                (x + width - radius) as f32,
                (y + radius) as f32,
                radius_f,
                inner_height,
            ),
            color,
        )
        .context("scrollbar thumb right")?;

        self.poly_quad(
            layers,
            2,
            euclid::point2(x as f32, y as f32),
            TOP_LEFT_ROUNDED_CORNER,
            1,
            euclid::size2(radius_f, radius_f),
            color,
        )
        .context("scrollbar thumb top-left")?
        .set_grayscale();

        self.poly_quad(
            layers,
            2,
            euclid::point2((x + width - radius) as f32, y as f32),
            TOP_RIGHT_ROUNDED_CORNER,
            1,
            euclid::size2(radius_f, radius_f),
            color,
        )
        .context("scrollbar thumb top-right")?
        .set_grayscale();

        self.poly_quad(
            layers,
            2,
            euclid::point2(x as f32, (y + height - radius) as f32),
            BOTTOM_LEFT_ROUNDED_CORNER,
            1,
            euclid::size2(radius_f, radius_f),
            color,
        )
        .context("scrollbar thumb bottom-left")?
        .set_grayscale();

        self.poly_quad(
            layers,
            2,
            euclid::point2((x + width - radius) as f32, (y + height - radius) as f32),
            BOTTOM_RIGHT_ROUNDED_CORNER,
            1,
            euclid::size2(radius_f, radius_f),
            color,
        )
        .context("scrollbar thumb bottom-right")?
        .set_grayscale();

        Ok(())
    }

    pub fn paint_pane(
        &mut self,
        pos: &PositionedPane,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        if self.config.use_box_model_render {
            return self.paint_pane_box_model(pos);
        }

        // Keep user selection until explicit user action clears it.
        /*
        let zone = {
            let dims = pos.pane.get_dimensions();
            let position = self
                .get_viewport(pos.pane.pane_id())
                .unwrap_or(dims.physical_top);

            let zones = self.get_semantic_zones(&pos.pane);
            let idx = match zones.binary_search_by(|zone| zone.start_y.cmp(&position)) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) - 1).max(0) as usize;
            zones.get(idx).cloned()
        };
        */

        let global_cursor_fg = self.palette().cursor_fg;
        let global_cursor_bg = self.palette().cursor_bg;
        let config = self.config.clone();
        let palette = pos.pane.palette();

        let (_, padding_top) = self.padding_left_top();
        let content_left = self.content_left_inset();

        let orientation = self.tab_bar_orientation();
        let tab_bar_height = if self.show_tab_bar && orientation.is_horizontal() {
            self.tab_bar_pixel_height()
                .context("tab_bar_pixel_height")?
        } else {
            0.
        };
        let top_bar_height = if orientation.is_at_bottom() {
            0.0
        } else {
            tab_bar_height
        };
        let border = self.get_os_border();
        // When tab bar is at top, it covers the titlebar area, so don't add
        // border.top which includes the integrated buttons inset.
        let effective_border_top = if self.is_top_tab_bar_visible() {
            0.0
        } else {
            border.top.get() as f32
        };
        let top_pixel_y = top_bar_height + padding_top + effective_border_top;

        let cursor = pos.pane.get_cursor_position();
        if pos.is_active {
            self.prev_cursor.update(&cursor);
        }

        let pane_id = pos.pane.pane_id();
        let current_viewport = self.effective_viewport(&pos.pane);
        let dims = pos.pane.get_dimensions();

        let gl_state = self.render_state.as_ref().unwrap();

        let cursor_border_color = palette.cursor_border.to_linear();
        let foreground = palette.foreground.to_linear();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();

        let window_is_transparent =
            !self.window_background.is_empty() || config.window_background_opacity != 1.0;
        let force_opaque_background = forces_opaque_kaku_tui_background(&pos.pane);
        let effective_window_is_transparent = window_is_transparent && !force_opaque_background;

        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if effective_window_is_transparent {
                0.
            } else {
                config.text_background_opacity
            });

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let (_, effective_padding_bottom) = self.effective_vertical_padding();
        let effective_padding_bottom = effective_padding_bottom as f32;
        let gap = config.split_pane_gap as usize;
        let split_col_gutter = (1 + 2 * gap).max(1) as f32;
        let split_row_gutter = gap.max(1) as f32;
        let background_rect = {
            // We want to fill out to the edges of the splits.
            // When transparent fill strips are active (no window background
            // image, opacity < 1.0), the left padding area is already covered
            // by the fill strip in paint_pass().  Starting the pane bg at x=0
            // would double-paint that region, making it appear more opaque.
            let transparent_fill_strips_active =
                self.window_background.is_empty() && effective_window_is_transparent;
            let (x, width_delta) = if pos.left == 0 {
                if transparent_fill_strips_active {
                    (content_left, cell_width * split_col_gutter / 2.0)
                } else {
                    (0., content_left + (cell_width * split_col_gutter / 2.0))
                }
            } else {
                (
                    content_left - (cell_width * split_col_gutter / 2.0)
                        + (pos.left as f32 * cell_width),
                    cell_width * split_col_gutter,
                )
            };

            let (y, height_delta) = if pos.top == 0 {
                (
                    (top_pixel_y - padding_top),
                    padding_top + (cell_height * split_row_gutter / 2.0),
                )
            } else {
                (
                    top_pixel_y + (pos.top as f32 * cell_height)
                        - (cell_height * split_row_gutter / 2.0),
                    cell_height * split_row_gutter,
                )
            };

            // Calculate the width - respect right padding
            let width = if pos.left + pos.width >= self.terminal_size.cols as usize {
                // Right-most pane: extend to split center but respect window padding.
                // For Right-oriented vertical sidebars, also stop short of the
                // sidebar so pane bg doesn't paint over it.
                let padding_right = self.effective_right_padding(&config) as f32;
                let sidebar_right_inset = if self.is_right_tab_bar_visible() {
                    self.vertical_sidebar_inset()
                } else {
                    0.0
                };
                self.dimensions.pixel_width as f32
                    - x
                    - padding_right
                    - border.right.get() as f32
                    - sidebar_right_inset
            } else {
                (pos.width as f32 * cell_width) + width_delta
            };

            // Calculate the height - respect bottom padding
            let height = if pos.top + pos.height >= self.terminal_size.rows as usize {
                // Bottom-most pane: extend to split center but respect window padding.
                // effective_vertical_padding already accounts for bottom tab bar height
                // (it subtracts tab_bar_height from bottom padding), so we should not
                // subtract bottom_bar_height again here to avoid a gap.
                // When tab bar is at bottom, it covers the bottom border area, so
                // don't subtract border.bottom which would create a gap.
                let padding_bottom = effective_padding_bottom;
                let effective_border_bottom = if self.is_bottom_tab_bar_visible() {
                    0.0
                } else {
                    border.bottom.get() as f32
                };
                self.dimensions.pixel_height as f32 - y - padding_bottom - effective_border_bottom
            } else {
                (pos.height as f32 * cell_height) + height_delta as f32
            };

            euclid::rect(x, y, width, height)
        };

        if self.window_background.is_empty() || force_opaque_background {
            // Per-pane, palette-specified background

            let mut quad = self
                .filled_rectangle(
                    layers,
                    0,
                    background_rect,
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(if force_opaque_background {
                            1.0
                        } else {
                            config.window_background_opacity
                        }),
                )
                .context("filled_rectangle")?;
            quad.set_hsv(if pos.is_active {
                None
            } else {
                Some(config.inactive_pane_hsb)
            });
        }

        {
            // If the bell is ringing, we draw another background layer over the
            // top of this in the configured bell color
            if let Some(intensity) = self.get_intensity_if_bell_target_ringing(
                &pos.pane,
                &config,
                VisualBellTarget::BackgroundColor,
            ) {
                // target background color
                let LinearRgba(r, g, b, _) = config
                    .resolved_palette
                    .visual_bell
                    .as_deref()
                    .unwrap_or(&palette.foreground)
                    .to_linear();

                let background = if window_is_transparent {
                    // for transparent windows, we fade in the target color
                    // by adjusting its alpha
                    LinearRgba::with_components(r, g, b, intensity)
                } else {
                    // otherwise We'll interpolate between the background color
                    // and the the target color
                    let (r1, g1, b1, a) = palette
                        .background
                        .to_linear()
                        .mul_alpha(config.window_background_opacity)
                        .tuple();
                    LinearRgba::with_components(
                        r1 + (r - r1) * intensity,
                        g1 + (g - g1) * intensity,
                        b1 + (b - b1) * intensity,
                        a,
                    )
                };
                log::trace!("bell color is {:?}", background);

                let mut quad = self
                    .filled_rectangle(layers, 0, background_rect, background)
                    .context("filled_rectangle")?;

                quad.set_hsv(if pos.is_active {
                    None
                } else {
                    Some(config.inactive_pane_hsb)
                });
            }
        }

        // TODO: we only have a single scrollbar in a single position.
        // We only update it for the active pane, but we should probably
        // do a per-pane scrollbar.  That will require more extensive
        // changes to ScrollHit, mouse positioning, PositionedPane
        // and tab size calculation.
        if pos.is_active && self.show_scroll_bar {
            if let Some(track) = self.scrollbar_track_for_pane(&pos.pane) {
                let info = ScrollHit::thumb(
                    &*pos.pane,
                    current_viewport,
                    track.height,
                    self.min_scroll_bar_height() as usize,
                );

                if info.height > 0 {
                    if let Some(alpha) = self.scrollbar_thumb_alpha(
                        &pos.pane,
                        track.x,
                        track.top,
                        track.width,
                        track.height,
                    ) {
                        let abs_thumb_top = track.top + info.top;
                        let thumb_size = info.height;
                        let is_light = crate::termwindow::is_light_color(&palette.background);
                        let thumb_color = if is_light {
                            LinearRgba::with_components(0.20, 0.20, 0.22, alpha * 0.78)
                        } else {
                            LinearRgba::with_components(0.78, 0.78, 0.82, alpha * 0.68)
                        };

                        self.ui_items.push(UIItem {
                            x: track.x,
                            width: track.width,
                            y: track.top,
                            height: info.top,
                            item_type: UIItemType::AboveScrollThumb,
                        });
                        self.ui_items.push(UIItem {
                            x: track.x,
                            width: track.width,
                            y: abs_thumb_top,
                            height: thumb_size,
                            item_type: UIItemType::ScrollThumb,
                        });
                        self.ui_items.push(UIItem {
                            x: track.x,
                            width: track.width,
                            y: abs_thumb_top + thumb_size,
                            height: track.height.saturating_sub(info.top + thumb_size),
                            item_type: UIItemType::BelowScrollThumb,
                        });

                        self.paint_scrollbar_thumb(
                            layers,
                            track.thumb_x,
                            abs_thumb_top,
                            track.thumb_width,
                            thumb_size,
                            thumb_color,
                        )?;
                    }
                }
            }
        }

        let (selrange, rectangular) = {
            let sel = self.selection(pos.pane.pane_id());
            (sel.range.clone(), sel.rectangular)
        };

        let start = Instant::now();
        let selection_fg = palette.selection_fg.to_linear();
        let selection_bg = palette.selection_bg.to_linear();
        let cursor_fg = palette.cursor_fg.to_linear();
        let cursor_bg = palette.cursor_bg.to_linear();
        let cursor_is_default_color =
            palette.cursor_fg == global_cursor_fg && palette.cursor_bg == global_cursor_bg;

        {
            let stable_range = match current_viewport {
                Some(top) => top..top + dims.viewport_rows as StableRowIndex,
                None => dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex,
            };

            pos.pane
                .apply_hyperlinks(stable_range.clone(), &self.config.hyperlink_rules);

            struct LineRender<'a, 'b> {
                term_window: &'a mut crate::TermWindow,
                selrange: Option<SelectionRange>,
                rectangular: bool,
                dims: RenderableDimensions,
                top_pixel_y: f32,
                left_pixel_x: f32,
                content_pixel_width: f32,
                pos: &'a PositionedPane,
                pane_id: PaneId,
                cursor: &'a StableCursorPosition,
                palette: &'a ColorPalette,
                default_bg: LinearRgba,
                cursor_border_color: LinearRgba,
                selection_fg: LinearRgba,
                selection_bg: LinearRgba,
                cursor_fg: LinearRgba,
                cursor_bg: LinearRgba,
                foreground: LinearRgba,
                cursor_is_default_color: bool,
                white_space: TextureRect,
                filled_box: TextureRect,
                window_is_transparent: bool,
                layers: &'a mut TripleLayerQuadAllocator<'b>,
                error: Option<anyhow::Error>,
            }

            // Content starts exactly at the cell position allocated by the mux gutter.
            // The split_pane_gap in mux already reserves gutter columns between panes,
            // so no additional pixel offset is needed here.
            let pane_pixel_width = pos.width as f32 * cell_width;

            let left_pixel_x =
                content_left + (pos.left as f32 * self.render_metrics.cell_size.width as f32);

            let content_pixel_width = pane_pixel_width;

            let mut render = LineRender {
                term_window: self,
                selrange,
                rectangular,
                dims,
                top_pixel_y,
                left_pixel_x,
                content_pixel_width,
                pos,
                pane_id,
                cursor: &cursor,
                palette: &palette,
                cursor_border_color,
                selection_fg,
                selection_bg,
                cursor_fg,
                default_bg,
                cursor_bg,
                foreground,
                cursor_is_default_color,
                white_space,
                filled_box,
                window_is_transparent: effective_window_is_transparent,
                layers,
                error: None,
            };

            impl<'a, 'b> LineRender<'a, 'b> {
                fn render_line(
                    &mut self,
                    stable_top: StableRowIndex,
                    line_idx: usize,
                    line: &&mut Line,
                ) -> anyhow::Result<()> {
                    let stable_row = stable_top + line_idx as StableRowIndex;
                    let selrange = self
                        .selrange
                        .map_or(0..0, |sel| sel.cols_for_row(stable_row, self.rectangular));
                    // Constrain to the pane width!
                    let selrange = selrange.start..selrange.end.min(self.dims.cols);
                    let show_terminal_cursor = self.term_window.get_modal().is_none();
                    let pane_is_active_for_cursor = show_terminal_cursor
                        && (self.pos.is_active || self.term_window.broadcast_input_visual_mode());

                    let (cursor, composing, password_input) = if pane_is_active_for_cursor
                        && self.cursor.y == stable_row
                    {
                        (
                            Some(CursorProperties {
                                position: StableCursorPosition {
                                    y: 0,
                                    ..*self.cursor
                                },
                                dead_key_or_leader: self.term_window.dead_key_status
                                    != DeadKeyStatus::None
                                    || self.term_window.leader_is_active(),
                                cursor_fg: self.cursor_fg,
                                cursor_bg: self.cursor_bg,
                                cursor_border_color: self.cursor_border_color,
                                cursor_is_default_color: self.cursor_is_default_color,
                            }),
                            match (pane_is_active_for_cursor, &self.term_window.dead_key_status) {
                                (true, DeadKeyStatus::Composing(composing)) => {
                                    Some(composing.to_string())
                                }
                                _ => None,
                            },
                            if self.term_window.config.detect_password_input {
                                match self.pos.pane.get_metadata() {
                                    Value::Object(obj) => {
                                        match obj.get(&Value::String("password_input".to_string()))
                                        {
                                            Some(Value::Bool(b)) => *b,
                                            _ => false,
                                        }
                                    }
                                    _ => false,
                                }
                            } else {
                                false
                            },
                        )
                    } else {
                        (None, None, false)
                    };

                    let shape_hash = self.term_window.shape_hash_for_line(line);

                    let quad_key = LineQuadCacheKey {
                        pane_id: self.pane_id,
                        password_input,
                        pane_is_active: pane_is_active_for_cursor,
                        config_generation: self.term_window.config.generation(),
                        shape_generation: self.term_window.shape_generation,
                        quad_generation: self.term_window.quad_generation,
                        composing: composing.clone(),
                        selection: selrange.clone(),
                        cursor,
                        shape_hash,
                        top_pixel_y: NotNan::new(self.top_pixel_y).unwrap()
                            + (line_idx + self.pos.top) as f32
                                * self.term_window.render_metrics.cell_size.height as f32,
                        left_pixel_x: NotNan::new(self.left_pixel_x).unwrap(),
                        phys_line_idx: line_idx,
                        reverse_video: self.dims.reverse_video,
                        window_is_transparent: self.window_is_transparent,
                    };

                    if let Some(cached_quad) =
                        self.term_window.line_quad_cache.borrow_mut().get(&quad_key)
                    {
                        let expired = cached_quad
                            .expires
                            .map(|i| Instant::now() >= i)
                            .unwrap_or(false);
                        let hover_changed = if cached_quad.invalidate_on_hover_change {
                            !same_hyperlink(
                                cached_quad.current_highlight.as_ref(),
                                self.term_window.current_highlight.as_ref(),
                            )
                        } else {
                            false
                        };
                        if !expired && !hover_changed {
                            cached_quad
                                .layers
                                .apply_to(self.layers)
                                .context("cached_quad.layers.apply_to")?;
                            self.term_window.update_next_frame_time(cached_quad.expires);
                            return Ok(());
                        }
                    }

                    let mut buf = HeapQuadAllocator::default();
                    let next_due = self.term_window.has_animation.borrow_mut().take();

                    let shape_key = LineToEleShapeCacheKey {
                        shape_hash,
                        shape_generation: quad_key.shape_generation,
                        window_is_transparent: self.window_is_transparent,
                        composing: if self.cursor.y == stable_row && pane_is_active_for_cursor {
                            if let DeadKeyStatus::Composing(composing) =
                                &self.term_window.dead_key_status
                            {
                                Some((self.cursor.x, composing.to_string()))
                            } else {
                                None
                            }
                        } else {
                            None
                        },
                    };

                    let render_result = self
                        .term_window
                        .render_screen_line(
                            RenderScreenLineParams {
                                top_pixel_y: *quad_key.top_pixel_y,
                                left_pixel_x: self.left_pixel_x,
                                pixel_width: self.content_pixel_width,
                                stable_line_idx: Some(stable_row),
                                line: &line,
                                selection: selrange.clone(),
                                cursor: &self.cursor,
                                palette: &self.palette,
                                dims: &self.dims,
                                config: &self.term_window.config,
                                cursor_border_color: self.cursor_border_color,
                                foreground: self.foreground,
                                is_active: pane_is_active_for_cursor,
                                pane: Some(&self.pos.pane),
                                selection_fg: self.selection_fg,
                                selection_bg: self.selection_bg,
                                cursor_fg: self.cursor_fg,
                                cursor_bg: self.cursor_bg,
                                cursor_is_default_color: self.cursor_is_default_color,
                                white_space: self.white_space,
                                filled_box: self.filled_box,
                                window_is_transparent: self.window_is_transparent,
                                default_bg: self.default_bg,
                                font: None,
                                style: None,
                                use_pixel_positioning: self
                                    .term_window
                                    .config
                                    .experimental_pixel_positioning,
                                render_metrics: self.term_window.render_metrics,
                                shape_key: Some(shape_key),
                                password_input,
                            },
                            &mut TripleLayerQuadAllocator::Heap(&mut buf),
                        )
                        .context("render_screen_line")?;

                    let expires = self.term_window.has_animation.borrow().as_ref().cloned();
                    self.term_window.update_next_frame_time(next_due);

                    buf.apply_to(self.layers)
                        .context("HeapQuadAllocator::apply_to")?;

                    let quad_value = LineQuadCacheValue {
                        layers: buf,
                        expires,
                        invalidate_on_hover_change: render_result.invalidate_on_hover_change,
                        current_highlight: if render_result.invalidate_on_hover_change {
                            self.term_window.current_highlight.clone()
                        } else {
                            None
                        },
                    };

                    self.term_window
                        .line_quad_cache
                        .borrow_mut()
                        .put(quad_key, quad_value);

                    Ok(())
                }
            }

            impl<'a, 'b> WithPaneLines for LineRender<'a, 'b> {
                fn with_lines_mut(&mut self, stable_top: StableRowIndex, lines: &mut [&mut Line]) {
                    for (line_idx, line) in lines.iter().enumerate() {
                        if let Err(err) = self.render_line(stable_top, line_idx, line) {
                            self.error.replace(err);
                            return;
                        }
                    }
                }
            }

            pos.pane.with_lines_mut(stable_range.clone(), &mut render);
            if let Some(error) = render.error.take() {
                return Err(error).context("error while calling with_lines_mut");
            }
        }

        /*
        if let Some(zone) = zone {
            // TODO: render a thingy to jump to prior prompt
        }
        */
        metrics::histogram!("paint_pane.lines").record(start.elapsed());
        log::trace!("lines elapsed {:?}", start.elapsed());

        Ok(())
    }

    pub fn build_pane(&mut self, pos: &PositionedPane) -> anyhow::Result<ComputedElement> {
        // First compute the bounds for the pane background

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let gap = self.config.split_pane_gap as usize;
        let split_col_gutter = (1 + 2 * gap).max(1) as f32;
        let split_row_gutter = gap.max(1) as f32;
        let (_, padding_top) = self.padding_left_top();
        let content_left = self.content_left_inset();
        let orientation = self.tab_bar_orientation();
        let tab_bar_height = if self.show_tab_bar && orientation.is_horizontal() {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if orientation.is_at_bottom() {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };
        let (_, effective_padding_bottom) = self.effective_vertical_padding();
        let effective_padding_bottom = effective_padding_bottom as f32;

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        // We want to fill out to the edges of the splits
        let (x, width_delta) = if pos.left == 0 {
            (0., content_left + (cell_width * split_col_gutter / 2.0))
        } else {
            (
                content_left - (cell_width * split_col_gutter / 2.0)
                    + (pos.left as f32 * cell_width),
                cell_width * split_col_gutter,
            )
        };

        let (y, height_delta) = if pos.top == 0 {
            (
                (top_pixel_y - padding_top),
                padding_top + (cell_height * split_row_gutter / 2.0),
            )
        } else {
            (
                top_pixel_y + (pos.top as f32 * cell_height)
                    - (cell_height * split_row_gutter / 2.0),
                cell_height * split_row_gutter,
            )
        };

        // Calculate the width - respect right padding
        let width = if pos.left + pos.width >= self.terminal_size.cols as usize {
            // Right-most pane: extend to split center but respect window padding
            // and the vertical sidebar when it lives on the right edge.
            let padding_right = self.effective_right_padding(&self.config) as f32;
            let sidebar_right_inset = if self.is_right_tab_bar_visible() {
                self.vertical_sidebar_inset()
            } else {
                0.0
            };
            self.dimensions.pixel_width as f32
                - x
                - padding_right
                - border.right.get() as f32
                - sidebar_right_inset
        } else {
            (pos.width as f32 * cell_width) + width_delta
        };

        // Calculate the height - respect bottom padding
        let height = if pos.top + pos.height >= self.terminal_size.rows as usize {
            // Bottom-most pane: extend to split center but respect window padding.
            let padding_bottom = effective_padding_bottom;
            self.dimensions.pixel_height as f32
                - y
                - padding_bottom
                - bottom_bar_height
                - border.bottom.get() as f32
        } else {
            (pos.height as f32 * cell_height) + height_delta as f32
        };

        let background_rect = euclid::rect(x, y, width, height);

        // Content starts exactly at the cell position allocated by the mux gutter.
        let pane_pixel_width = pos.width as f32 * cell_width;
        let pane_pixel_height = pos.height as f32 * cell_height;
        let content_rect = euclid::rect(
            content_left + (pos.left as f32 * cell_width),
            top_pixel_y + (pos.top as f32 * cell_height),
            pane_pixel_width,
            pane_pixel_height,
        );

        let palette = pos.pane.palette();

        // TODO: visual bell background layer
        // TODO: scrollbar

        Ok(ComputedElement {
            item_type: None,
            zindex: 0,
            bounds: background_rect,
            border: PixelDimension::default(),
            border_rect: background_rect,
            border_corners: None,
            colors: ElementColors {
                border: BorderColor::default(),
                bg: if self.window_background.is_empty() {
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity)
                        .into()
                } else {
                    InheritableColor::Inherited
                },
                text: InheritableColor::Inherited,
            },
            hover_colors: None,
            padding: background_rect,
            content_rect,
            baseline: 1.0,
            content: ComputedElementContent::Children(vec![]),
        })
    }
}
