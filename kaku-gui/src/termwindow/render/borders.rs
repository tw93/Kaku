use crate::quad::TripleLayerQuadAllocator;
use crate::utilsprites::RenderMetrics;
use ::window::ULength;
use config::{ConfigHandle, DimensionContext};
use window::color::LinearRgba;

const INTEGRATED_BUTTONS_TOP_INSET: usize = 16;

pub(crate) fn integrated_buttons_top_inset(
    config: &ConfigHandle,
    is_fullscreen: bool,
    _top_tab_bar_visible: bool,
) -> usize {
    if !is_fullscreen
        && config
            .window_decorations
            .contains(::window::WindowDecorations::INTEGRATED_BUTTONS)
    {
        INTEGRATED_BUTTONS_TOP_INSET
    } else {
        0
    }
}

impl crate::TermWindow {
    pub fn paint_window_borders(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let is_fullscreen = self.layout_is_effective_fullscreen();
        // Keep border geometry consistent with pane layout.
        // In fullscreen we still need user window_frame border widths;
        // OS border (eg: notch safe-area) is merged by get_os_border().
        let border_dimensions = self.get_os_border();
        let fullscreen_border_color = border_dimensions.color;

        if border_dimensions.top.get() > 0
            || border_dimensions.bottom.get() > 0
            || border_dimensions.left.get() > 0
            || border_dimensions.right.get() > 0
        {
            let height = self.dimensions.pixel_height as f32;
            let width = self.dimensions.pixel_width as f32;
            let integrated_top_inset = integrated_buttons_top_inset(
                &self.config,
                is_fullscreen,
                self.show_tab_bar && !self.config.tab_bar_at_bottom,
            )
            .min(border_dimensions.top.get()) as f32;

            // In fullscreen, use palette background color for all borders.
            // In windowed mode, use configured border colors if available.
            let border_color = |default: LinearRgba| -> LinearRgba {
                if is_fullscreen {
                    fullscreen_border_color
                } else {
                    default
                }
            };

            let border_top = border_dimensions.top.get() as f32;
            if border_top > 0.0 {
                if integrated_top_inset > 0.0 {
                    let background = self
                        .get_active_pane_or_overlay()
                        .map(|pane| pane.palette().background)
                        .unwrap_or_else(|| self.palette().background)
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity);
                    self.filled_rectangle(
                        layers,
                        1,
                        euclid::rect(0.0, 0.0, width, border_top),
                        background,
                    )?;
                } else {
                    let color = border_color(
                        self.config
                            .window_frame
                            .border_top_color
                            .map(|c| c.to_linear())
                            .unwrap_or(border_dimensions.color),
                    );
                    self.filled_rectangle(
                        layers,
                        1,
                        euclid::rect(0.0, 0.0, width, border_top),
                        color,
                    )?;
                }
            }

            let border_left = border_dimensions.left.get() as f32;
            if border_left > 0.0 {
                let color = border_color(
                    self.config
                        .window_frame
                        .border_left_color
                        .map(|c| c.to_linear())
                        .unwrap_or(border_dimensions.color),
                );
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(0.0, 0.0, border_left, height),
                    color,
                )?;
            }

            let border_bottom = border_dimensions.bottom.get() as f32;
            if border_bottom > 0.0 {
                let color = border_color(
                    self.config
                        .window_frame
                        .border_bottom_color
                        .map(|c| c.to_linear())
                        .unwrap_or(border_dimensions.color),
                );
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(0.0, height - border_bottom, width, height),
                    color,
                )?;
            }

            let border_right = border_dimensions.right.get() as f32;
            if border_right > 0.0 {
                let color = border_color(
                    self.config
                        .window_frame
                        .border_right_color
                        .map(|c| c.to_linear())
                        .unwrap_or(border_dimensions.color),
                );
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(width - border_right, 0.0, border_right, height),
                    color,
                )?;
            }
        }

        // macOS simple fullscreen can occasionally show a 1px seam at the
        // window edge due to compositor rounding. Cover edges explicitly.
        let is_simple_fullscreen_with_notch_padding = is_fullscreen
            && self
                .os_parameters
                .as_ref()
                .and_then(|p| p.border_dimensions.as_ref())
                .map(|b| {
                    b.top.get() > 0 || b.left.get() > 0 || b.right.get() > 0 || b.bottom.get() > 0
                })
                .unwrap_or(false);

        if is_simple_fullscreen_with_notch_padding {
            let height = self.dimensions.pixel_height as f32;
            let width = self.dimensions.pixel_width as f32;
            let edge = 1.0f32;

            if width > 0.0 && height > 0.0 {
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(0.0, 0.0, width, edge),
                    fullscreen_border_color,
                )?;
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(0.0, (height - edge).max(0.0), width, edge),
                    fullscreen_border_color,
                )?;
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect(0.0, 0.0, edge, height),
                    fullscreen_border_color,
                )?;
                self.filled_rectangle(
                    layers,
                    1,
                    euclid::rect((width - edge).max(0.0), 0.0, edge, height),
                    fullscreen_border_color,
                )?;
            }
        }

        Ok(())
    }

    pub fn get_os_border_impl(
        os_parameters: &Option<window::parameters::Parameters>,
        config: &ConfigHandle,
        dimensions: &crate::Dimensions,
        render_metrics: &RenderMetrics,
    ) -> window::parameters::Border {
        let mut border = os_parameters
            .as_ref()
            .and_then(|p| p.border_dimensions.clone())
            .unwrap_or_default();

        border.left += ULength::new(
            config
                .window_frame
                .border_left_width
                .evaluate_as_pixels(DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: render_metrics.cell_size.width as f32,
                })
                .ceil() as usize,
        );
        border.right += ULength::new(
            config
                .window_frame
                .border_right_width
                .evaluate_as_pixels(DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: render_metrics.cell_size.width as f32,
                })
                .ceil() as usize,
        );
        border.top += ULength::new(
            config
                .window_frame
                .border_top_height
                .evaluate_as_pixels(DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: render_metrics.cell_size.height as f32,
                })
                .ceil() as usize,
        );
        border.bottom += ULength::new(
            config
                .window_frame
                .border_bottom_height
                .evaluate_as_pixels(DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: render_metrics.cell_size.height as f32,
                })
                .ceil() as usize,
        );

        border
    }

    pub fn get_os_border(&self) -> window::parameters::Border {
        let mut border = Self::get_os_border_impl(
            &self.os_parameters,
            &self.config,
            &self.dimensions,
            &self.render_metrics,
        );

        let is_fullscreen = self.layout_is_effective_fullscreen();
        let extra_top = integrated_buttons_top_inset(
            &self.config,
            is_fullscreen,
            self.show_tab_bar && !self.config.tab_bar_at_bottom,
        );
        if extra_top > 0 {
            border.top += ULength::new(extra_top);
        }

        border
    }
}
