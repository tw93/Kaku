use crate::quad::{HeapQuadAllocator, QuadTrait, TripleLayerQuadAllocator};
use crate::selection::SelectionRange;
use crate::termwindow::box_model::*;
use crate::termwindow::render::{
    same_hyperlink, CursorProperties, LineQuadCacheKey, LineQuadCacheValue, LineToEleShapeCacheKey,
    RenderScreenLineParams,
};
use crate::termwindow::{ScrollHit, UIItem, UIItemType};
use ::window::bitmaps::TextureRect;
use ::window::DeadKeyStatus;
use anyhow::Context;
use config::{DimensionContext, VisualBellTarget};
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

    pub fn paint_pane(
        &mut self,
        pos: &PositionedPane,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        if self.config.use_box_model_render {
            return self.paint_pane_box_model(pos);
        }

        self.check_for_dirty_lines_and_invalidate_selection(&pos.pane);
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

        let (padding_left, padding_top) = self.padding_left_top();

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()
                .context("tab_bar_pixel_height")?
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        let cursor = pos.pane.get_cursor_position();
        if pos.is_active {
            self.prev_cursor.update(&cursor);
        }

        let pane_id = pos.pane.pane_id();
        let current_viewport = self.get_viewport(pane_id);
        let dims = pos.pane.get_dimensions();

        let gl_state = self.render_state.as_ref().unwrap();

        let cursor_border_color = palette.cursor_border.to_linear();
        let foreground = palette.foreground.to_linear();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();

        let window_is_transparent =
            !self.window_background.is_empty() || config.window_background_opacity != 1.0;

        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if window_is_transparent {
                0.
            } else {
                config.text_background_opacity
            });

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let gap = config.split_pane_gap as usize;
        let split_col_gutter = (1 + 2 * gap).max(1) as f32;
        let split_row_gutter = gap.max(1) as f32;
        let background_rect = {
            // We want to fill out to the edges of the splits
            let (x, width_delta) = if pos.left == 0 {
                (
                    0.,
                    padding_left + border.left.get() as f32 + (cell_width * split_col_gutter / 2.0),
                )
            } else {
                (
                    padding_left + border.left.get() as f32 - (cell_width * split_col_gutter / 2.0)
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
                let padding_right =
                    self.config
                        .window_padding
                        .right
                        .evaluate_as_pixels(DimensionContext {
                            dpi: self.dimensions.dpi as f32,
                            pixel_max: self.terminal_size.pixel_width as f32,
                            pixel_cell: cell_width,
                        });
                self.dimensions.pixel_width as f32 - x - padding_right - border.right.get() as f32
            } else {
                (pos.width as f32 * cell_width) + width_delta
            };

            // Calculate the height - respect bottom padding
            let height = if pos.top + pos.height >= self.terminal_size.rows as usize {
                // Bottom-most pane: extend to split center but respect window padding
                let padding_bottom =
                    self.config
                        .window_padding
                        .bottom
                        .evaluate_as_pixels(DimensionContext {
                            dpi: self.dimensions.dpi as f32,
                            pixel_max: self.terminal_size.pixel_height as f32,
                            pixel_cell: cell_height,
                        });
                self.dimensions.pixel_height as f32
                    - y
                    - padding_bottom
                    - bottom_bar_height
                    - border.bottom.get() as f32
            } else {
                (pos.height as f32 * cell_height) + height_delta as f32
            };

            euclid::rect(x, y, width, height)
        };

        if self.window_background.is_empty() {
            // Per-pane, palette-specified background

            let mut quad = self
                .filled_rectangle(
                    layers,
                    0,
                    background_rect,
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(config.window_background_opacity),
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
            let thumb_y_offset = top_bar_height as usize + border.top.get();

            let min_height = self.min_scroll_bar_height();

            let info = ScrollHit::thumb(
                &*pos.pane,
                current_viewport,
                self.dimensions.pixel_height.saturating_sub(
                    thumb_y_offset + border.bottom.get() + bottom_bar_height as usize,
                ),
                min_height as usize,
            );
            let abs_thumb_top = thumb_y_offset + info.top;
            let thumb_size = info.height;
            let color = palette.scrollbar_thumb.to_linear();

            // Adjust the scrollbar thumb position
            let config = &self.config;
            let padding = self.effective_right_padding(&config) as f32;

            let thumb_x = self.dimensions.pixel_width - padding as usize - border.right.get();

            // Register the scroll bar location
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: thumb_y_offset,
                height: info.top,
                item_type: UIItemType::AboveScrollThumb,
            });
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top,
                height: thumb_size,
                item_type: UIItemType::ScrollThumb,
            });
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top + thumb_size,
                height: self
                    .dimensions
                    .pixel_height
                    .saturating_sub(abs_thumb_top + thumb_size),
                item_type: UIItemType::BelowScrollThumb,
            });

            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    thumb_x as f32,
                    abs_thumb_top as f32,
                    padding,
                    thumb_size as f32,
                ),
                color,
            )
            .context("filled_rectangle")?;
        }

        // 获取活跃选区、淡出选区、或飞行动画选区
        const SELECTION_FADE_DURATION: f32 = 0.3; // 300ms 淡出
        const COPY_FLY_DURATION: f32 = 0.4; // 400ms 飞行

        // 超时兜底：防止 tab 切换后 copy_fly 残留（2 秒安全上限）
        if self
            .copy_fly
            .as_ref()
            .map_or(false, |f| f.started.elapsed().as_secs_f32() > 2.0)
        {
            self.copy_fly = None;
        }

        // 飞行动画进度（ease-out cubic）
        let fly_info: Option<(f32, f32, f32)> = if let Some(ref fly) = self.copy_fly {
            if fly.pane_id == pos.pane.pane_id() {
                let elapsed = fly.started.elapsed().as_secs_f32();
                let t = (elapsed / COPY_FLY_DURATION).min(1.0);
                let eased = 1.0 - (1.0 - t).powi(3); // ease-out cubic
                if t < 1.0 {
                    Some((fly.target_x, fly.target_y, eased))
                } else {
                    None // 动画结束
                }
            } else {
                None
            }
        } else {
            None
        };

        // 先检查是否有活跃选区（优先级最高，会取消所有动画）
        let (active_range, active_rect) = {
            let sel = self.selection(pos.pane.pane_id());
            (sel.range.clone(), sel.rectangular)
        };

        // 有活跃选区时立即取消飞行动画（Fix #3: 新选区优先）
        let fly_info = if active_range.is_some() {
            if self
                .copy_fly
                .as_ref()
                .map_or(false, |f| f.pane_id == pos.pane.pane_id())
            {
                self.copy_fly = None;
            }
            None
        } else {
            fly_info
        };

        let (selrange, rectangular, selection_fade_alpha) = if let Some((_tx, _ty, progress)) =
            fly_info
        {
            // 飞行动画期间：使用 copy_fly 的选区
            let fly = self.copy_fly.as_ref().unwrap();
            let alpha = 1.0 - progress * 0.7; // 1.0 → 0.3
            (Some(fly.range.clone()), fly.rectangular, alpha)
        } else {
            // 清理已结束的飞行动画
            if self
                .copy_fly
                .as_ref()
                .map_or(false, |f| f.pane_id == pos.pane.pane_id())
            {
                self.copy_fly = None;
            }

            if active_range.is_some() {
                self.fading_selection = None;
                (active_range, active_rect, 1.0f32)
            } else if let Some(ref fade) = self.fading_selection {
                if fade.pane_id == pos.pane.pane_id() {
                    let elapsed = fade.started.elapsed().as_secs_f32();
                    let alpha = (1.0 - elapsed / SELECTION_FADE_DURATION).max(0.0);
                    if alpha > 0.0 {
                        (Some(fade.range.clone()), fade.rectangular, alpha)
                    } else {
                        (None, false, 1.0)
                    }
                } else {
                    (active_range, active_rect, 1.0)
                }
            } else {
                (active_range, active_rect, 1.0)
            }
        };
        // 清理过期的淡出选区
        if let Some(ref fade) = self.fading_selection {
            if fade.started.elapsed().as_secs_f32() >= SELECTION_FADE_DURATION {
                self.fading_selection = None;
            }
        }
        // 动画期间持续重绘 (16ms ≈ 60fps)
        let has_anim = fly_info.is_some()
            || (selection_fade_alpha > 0.0 && selection_fade_alpha < 1.0);
        if has_anim {
            let next = Instant::now() + std::time::Duration::from_millis(16);
            let mut anim = self.has_animation.borrow_mut();
            match *anim {
                Some(existing) if existing <= next => {}
                _ => {
                    *anim = Some(next);
                }
            }
        }

        let start = Instant::now();
        // 飞行/淡出时：fg 设为透明让文字保持原色
        let selection_fg = if selection_fade_alpha < 1.0 {
            LinearRgba::TRANSPARENT
        } else {
            palette.selection_fg.to_linear()
        };
        let selection_bg = palette.selection_bg.to_linear().mul_alpha(selection_fade_alpha);
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
                /// 飞行动画参数: (target_x, target_y, progress)
                selection_fly: Option<(f32, f32, f32)>,
                /// 动画期间绕过行缓存（飞行或淡出）
                bypass_line_cache: bool,
            }

            // Content starts exactly at the cell position allocated by the mux gutter.
            // The split_pane_gap in mux already reserves gutter columns between panes,
            // so no additional pixel offset is needed here.
            let pane_pixel_width = pos.width as f32 * cell_width;

            let left_pixel_x = padding_left
                + border.left.get() as f32
                + (pos.left as f32 * self.render_metrics.cell_size.width as f32);

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
                window_is_transparent,
                layers,
                error: None,
                selection_fly: fly_info,
                bypass_line_cache: has_anim,
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

                    let (cursor, composing, password_input) = if self.cursor.y == stable_row {
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
                            match (self.pos.is_active, &self.term_window.dead_key_status) {
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
                        pane_is_active: self.pos.is_active,
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
                    };

                    // 飞行动画期间跳过行缓存（每帧位置都变）
                    if !self.bypass_line_cache {
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
                    }

                    let mut buf = HeapQuadAllocator::default();
                    let next_due = self.term_window.has_animation.borrow_mut().take();

                    let shape_key = LineToEleShapeCacheKey {
                        shape_hash,
                        shape_generation: quad_key.shape_generation,
                        composing: if self.cursor.y == stable_row && self.pos.is_active {
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
                                is_active: self.pos.is_active,
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
                                selection_fly: self.selection_fly,
                            },
                            &mut TripleLayerQuadAllocator::Heap(&mut buf),
                        )
                        .context("render_screen_line")?;

                    let expires = self.term_window.has_animation.borrow().as_ref().cloned();
                    self.term_window.update_next_frame_time(next_due);

                    buf.apply_to(self.layers)
                        .context("HeapQuadAllocator::apply_to")?;

                    // 飞行动画期间不写入缓存（变形矩形会污染后续正常渲染）
                    if !self.bypass_line_cache {
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
                    }

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
        let (padding_left, padding_top) = self.padding_left_top();
        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        // We want to fill out to the edges of the splits
        let (x, width_delta) = if pos.left == 0 {
            (
                0.,
                padding_left + border.left.get() as f32 + (cell_width * split_col_gutter / 2.0),
            )
        } else {
            (
                padding_left + border.left.get() as f32 - (cell_width * split_col_gutter / 2.0)
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
            let padding_right =
                self.config
                    .window_padding
                    .right
                    .evaluate_as_pixels(DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.terminal_size.pixel_width as f32,
                        pixel_cell: cell_width,
                    });
            self.dimensions.pixel_width as f32 - x - padding_right - border.right.get() as f32
        } else {
            (pos.width as f32 * cell_width) + width_delta
        };

        // Calculate the height - respect bottom padding
        let height = if pos.top + pos.height >= self.terminal_size.rows as usize {
            // Bottom-most pane: extend to split center but respect window padding
            let padding_bottom =
                self.config
                    .window_padding
                    .bottom
                    .evaluate_as_pixels(DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.terminal_size.pixel_height as f32,
                        pixel_cell: cell_height,
                    });
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
            padding_left + border.left.get() as f32 + (pos.left as f32 * cell_width),
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
