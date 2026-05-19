use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::render::{forces_opaque_kaku_tui_window_background, RenderScreenLineParams};
use crate::utilsprites::RenderMetrics;
use config::{ConfigHandle, DimensionContext, TabBarOrientation};
use mux::renderable::RenderableDimensions;
use wezterm_term::color::ColorAttribute;
use window::color::LinearRgba;

impl crate::TermWindow {
    pub fn paint_tab_bar(&mut self, layers: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let _border = self.get_os_border();
        let orientation = self.tab_bar_orientation();
        // For vertical orientations, paint_vertical_tab_bar handles its own
        // placement and returns immediately at the dispatch boundary below;
        // the tab_bar_y here is only meaningful for the horizontal paths.
        let tab_bar_height = self.tab_bar_pixel_height()?;
        let tab_bar_y = if orientation.is_at_bottom() {
            // Position tab bar at the very bottom for a "flush" appearance.
            // The tab bar renders its own background, covering the bottom area.
            ((self.dimensions.pixel_height as f32) - tab_bar_height).max(0.)
        } else {
            // Position tab bar at the very top (y=0) for a "flush" appearance.
            // The fancy tab bar renders its own background, so it will cover
            // the titlebar area completely.
            0.0
        };

        if orientation.is_vertical() {
            return self.paint_vertical_tab_bar(layers);
        }
        let panes = self.get_panes_to_render();
        let force_opaque_tab_bar_background = forces_opaque_kaku_tui_window_background(&panes);

        if self.config.use_fancy_tab_bar {
            if self.fancy_tab_bar.is_none() {
                let palette = self.palette().clone();
                let tab_bar = self.build_fancy_tab_bar(&palette)?;
                self.fancy_tab_bar.replace(tab_bar);
            }

            // In transparent mode, fill the tab bar area with a transparent
            // background so it blends consistently with the window.
            let window_is_transparent =
                !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;
            if window_is_transparent && !force_opaque_tab_bar_background {
                let tab_bar_bg = if let Some(active) = self.get_active_pane_or_overlay() {
                    active
                        .palette()
                        .background
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity)
                } else {
                    self.palette()
                        .background
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity)
                };
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(
                        0.0,
                        tab_bar_y,
                        self.dimensions.pixel_width as f32,
                        tab_bar_height,
                    ),
                    tab_bar_bg,
                )?;
            }

            let mut fancy_ui_items = self.paint_fancy_tab_bar()?;
            self.ui_items.append(&mut fancy_ui_items);
            return Ok(());
        }

        let palette = self.palette().clone();

        // Register the tab bar location
        self.ui_items.append(&mut self.tab_bar.compute_ui_items(
            tab_bar_y as usize,
            self.render_metrics.cell_size.height as usize,
            self.render_metrics.cell_size.width as usize,
        ));

        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;
        let effective_window_is_transparent =
            window_is_transparent && !force_opaque_tab_bar_background;
        let gl_state = self.render_state.as_ref().unwrap();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();
        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if effective_window_is_transparent {
                0.
            } else {
                self.config.text_background_opacity
            });

        if effective_window_is_transparent {
            let tab_bar_bg = if let Some(active) = self.get_active_pane_or_overlay() {
                active
                    .palette()
                    .background
                    .to_linear()
                    .mul_alpha(self.config.window_background_opacity)
            } else {
                palette
                    .background
                    .to_linear()
                    .mul_alpha(self.config.window_background_opacity)
            };
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(
                    0.0,
                    tab_bar_y,
                    self.dimensions.pixel_width as f32,
                    tab_bar_height,
                ),
                tab_bar_bg,
            )?;
        }

        self.render_screen_line(
            RenderScreenLineParams {
                top_pixel_y: tab_bar_y,
                left_pixel_x: 0.,
                pixel_width: self.dimensions.pixel_width as f32,
                stable_line_idx: None,
                line: self.tab_bar.line(),
                selection: 0..0,
                cursor: &Default::default(),
                palette: &palette,
                dims: &RenderableDimensions {
                    cols: self.dimensions.pixel_width
                        / self.render_metrics.cell_size.width as usize,
                    physical_top: 0,
                    scrollback_rows: 0,
                    scrollback_top: 0,
                    viewport_rows: 1,
                    dpi: self.terminal_size.dpi,
                    pixel_height: self.render_metrics.cell_size.height as usize,
                    pixel_width: self.terminal_size.pixel_width,
                    reverse_video: false,
                },
                config: &self.config,
                cursor_border_color: LinearRgba::default(),
                foreground: palette.foreground.to_linear(),
                pane: None,
                is_active: true,
                selection_fg: LinearRgba::default(),
                selection_bg: LinearRgba::default(),
                cursor_fg: LinearRgba::default(),
                cursor_bg: LinearRgba::default(),
                cursor_is_default_color: true,
                white_space,
                filled_box,
                window_is_transparent: effective_window_is_transparent,
                default_bg,
                style: None,
                font: None,
                use_pixel_positioning: self.config.experimental_pixel_positioning,
                render_metrics: self.render_metrics,
                shape_key: None,
                password_input: false,
            },
            layers,
        )?;

        Ok(())
    }

    pub fn tab_bar_pixel_height_impl(
        config: &ConfigHandle,
        fontconfig: &wezterm_font::FontConfiguration,
        render_metrics: &RenderMetrics,
    ) -> anyhow::Result<f32> {
        if config.use_fancy_tab_bar {
            let font = fontconfig.title_font()?;
            Ok((font.metrics().cell_height.get() as f32 * 1.75).ceil())
        } else {
            Ok(render_metrics.cell_size.height as f32)
        }
    }

    /// Cheap approximation of tab bar height that avoids the ~485ms cost of
    /// resolving the title font on macOS cold start (CoreText substitution
    /// lookup + HarfBuzz shaper init). Used only to compute initial window
    /// dimensions; the real height is computed lazily on first render via
    /// `tab_bar_pixel_height()`.
    pub fn estimated_tab_bar_pixel_height(
        config: &ConfigHandle,
        render_metrics: &RenderMetrics,
    ) -> f32 {
        if config.use_fancy_tab_bar {
            // Mirror tab_bar_pixel_height_impl's fancy-path formula, but use
            // the terminal cell height as a stand-in for the title font cell
            // height. The two differ by ~1-2 pixels in typical configs.
            (render_metrics.cell_size.height as f32 * 1.75).ceil()
        } else {
            render_metrics.cell_size.height as f32
        }
    }

    pub fn tab_bar_pixel_height(&self) -> anyhow::Result<f32> {
        Self::tab_bar_pixel_height_impl(&self.config, &self.fonts, &self.render_metrics)
    }

    /// Width in pixels of a vertical (Left/Right) tab bar, evaluated against
    /// the current DPI / cell metrics. Returns 0 for horizontal orientations.
    pub fn tab_bar_pixel_width_impl(
        config: &ConfigHandle,
        render_metrics: &RenderMetrics,
        dpi: f32,
        viewport_pixel_width: f32,
    ) -> f32 {
        if !config.effective_tab_bar_orientation().is_vertical() {
            return 0.0;
        }
        let h_context = DimensionContext {
            dpi,
            pixel_max: viewport_pixel_width.max(1.0),
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        config.tab_bar_width.evaluate_as_pixels(h_context).max(0.0)
    }

    pub fn tab_bar_pixel_width(&self) -> f32 {
        Self::tab_bar_pixel_width_impl(
            &self.config,
            &self.render_metrics,
            self.dimensions.dpi as f32,
            self.dimensions.pixel_width as f32,
        )
    }

    /// Convenience accessor mirroring `config.effective_tab_bar_orientation()`.
    pub fn tab_bar_orientation(&self) -> TabBarOrientation {
        self.config.effective_tab_bar_orientation()
    }

    /// Pixel width consumed by a vertical sidebar including the visual
    /// breathing room between the sidebar's edge and the pane area. Other
    /// pixel-layout sites (resize, content_left_inset, modal positioning,
    /// mouse hit-test) should use this — only the sidebar painter itself
    /// uses the raw `tab_bar_pixel_width()`.
    pub fn vertical_sidebar_inset(&self) -> f32 {
        const SIDEBAR_CONTENT_GAP: f32 = 12.0;
        if self.show_tab_bar && self.tab_bar_orientation().is_vertical() {
            self.tab_bar_pixel_width() + SIDEBAR_CONTENT_GAP
        } else {
            0.0
        }
    }

    /// True when the tab bar is visible and rendered as a horizontal strip
    /// at the top of the window.
    pub fn is_top_tab_bar_visible(&self) -> bool {
        self.show_tab_bar && matches!(self.tab_bar_orientation(), TabBarOrientation::Top)
    }

    /// True when the tab bar is visible and rendered as a horizontal strip
    /// at the bottom of the window.
    pub fn is_bottom_tab_bar_visible(&self) -> bool {
        self.show_tab_bar && matches!(self.tab_bar_orientation(), TabBarOrientation::Bottom)
    }

    /// True when the tab bar is visible and rendered as a vertical sidebar
    /// on the left edge of the window.
    pub fn is_left_tab_bar_visible(&self) -> bool {
        self.show_tab_bar && matches!(self.tab_bar_orientation(), TabBarOrientation::Left)
    }

    /// True when the tab bar is visible and rendered as a vertical sidebar
    /// on the right edge of the window.
    pub fn is_right_tab_bar_visible(&self) -> bool {
        self.show_tab_bar && matches!(self.tab_bar_orientation(), TabBarOrientation::Right)
    }
}
