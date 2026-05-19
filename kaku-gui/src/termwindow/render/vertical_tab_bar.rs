use crate::customglyph::*;
use crate::quad::TripleLayerQuadAllocator;
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::*;
use crate::termwindow::UIItemType;
use crate::utilsprites::RenderMetrics;
use config::{Dimension, DimensionContext, TabBarColors, TabBarOrientation};
use wezterm_term::color::{ColorAttribute, ColorPalette};

const PLUS_BUTTON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

impl crate::TermWindow {
    pub fn invalidate_vertical_tab_bar(&mut self) {
        self.fancy_tab_bar.take();
    }

    /// Top inset reserved for the macOS traffic-light buttons. Kaku draws
    /// without a system titlebar, so the buttons live inside the window's
    /// content area and must not be painted over. The value covers the
    /// AppKit title-bar height (~28 px), the button drop-shadow, and a
    /// macOS-sidebar-style breathing strip so the first row sits clearly
    /// below the buttons (like Mail / Notes / Finder).
    pub fn vertical_sidebar_top_inset(&self) -> f32 {
        if cfg!(target_os = "macos") && !self.layout_is_effective_fullscreen() {
            60.0
        } else {
            0.0
        }
    }

    pub fn build_vertical_tab_bar(
        &self,
        palette: &ColorPalette,
    ) -> anyhow::Result<ComputedElement> {
        let orientation = self.tab_bar_orientation();
        debug_assert!(orientation.is_vertical());

        let sidebar_width = self.tab_bar_pixel_width();
        let top_inset = self.vertical_sidebar_top_inset();
        let sidebar_height = (self.dimensions.pixel_height as f32 - top_inset).max(0.0);
        let border = self.get_os_border();

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let items = self.tab_bar.items();
        let colors = self
            .config
            .colors
            .as_ref()
            .and_then(|c| c.tab_bar.as_ref())
            .cloned()
            .unwrap_or_else(TabBarColors::default);

        let bar_colors = ElementColors {
            border: BorderColor::default(),
            bg: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_bg
            } else {
                self.config.window_frame.inactive_titlebar_bg
            }
            .to_linear()
            .into(),
            text: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_fg
            } else {
                self.config.window_frame.inactive_titlebar_fg
            }
            .to_linear()
            .into(),
        };

        // Generous internal padding so titles get breathing room and the
        // hover/active pill reads as a button rather than a full-width strip.
        let row_padding_h = Dimension::Pixels((0.5 * metrics.cell_size.width as f32) + 8.0);
        let row_padding_v = Dimension::Cells(0.35);
        // Horizontal gutter the row leaves on both sides so its background
        // pill doesn't touch the sidebar edges. Matches the macOS sidebar
        // look in Mail/Finder/Notes.
        let row_gutter_h = Dimension::Pixels(8.0);
        let row_gutter_px = 8.0_f32;

        let mut children: Vec<Element> = vec![];

        // Top "+" new-tab button row. Only emitted if the tab bar already
        // exposes a NewTabButton item — keeps the show_new_tab_button_in_tab_bar
        // config field authoritative.
        if items
            .iter()
            .any(|i| matches!(i.item, TabBarItem::NewTabButton))
        {
            let new_tab = colors.new_tab();
            let new_tab_hover = colors.new_tab_hover();
            let plus_button = Element::new(
                &font,
                ElementContent::Poly {
                    line_width: metrics.underline_height.max(2),
                    poly: SizedPoly {
                        poly: PLUS_BUTTON,
                        width: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                        height: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                    },
                },
            )
            .display(DisplayType::Block)
            .vertical_align(VerticalAlign::Middle)
            .item_type(UIItemType::TabBar(TabBarItem::NewTabButton))
            .margin(BoxDimension {
                left: row_gutter_h,
                right: row_gutter_h,
                top: Dimension::Cells(0.35),
                bottom: Dimension::Cells(0.35),
            })
            .padding(BoxDimension {
                left: row_padding_h,
                right: row_padding_h,
                top: row_padding_v,
                bottom: row_padding_v,
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .colors(ElementColors {
                border: BorderColor::default(),
                bg: new_tab.bg_color.to_linear().into(),
                text: new_tab.fg_color.to_linear().into(),
            })
            .hover_colors(Some(ElementColors {
                border: BorderColor::default(),
                bg: new_tab_hover.bg_color.to_linear().into(),
                text: new_tab_hover.fg_color.to_linear().into(),
            }));
            children.push(plus_button);
        }

        for entry in items {
            let (tab_idx, active) = match entry.item {
                TabBarItem::Tab { tab_idx, active } => (tab_idx, active),
                _ => continue,
            };

            let bg_color = entry
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().background() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_bg(col)),
                });
            let fg_color = entry
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().foreground() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_fg(col)),
                });

            let active_tab = colors.active_tab();
            let inactive_tab = colors.inactive_tab();
            let inactive_tab_hover = colors.inactive_tab_hover();

            let row_bg = if active {
                bg_color.unwrap_or_else(|| active_tab.bg_color.into())
            } else {
                bg_color.unwrap_or_else(|| inactive_tab.bg_color.into())
            };
            let row_fg = if active {
                fg_color.unwrap_or_else(|| active_tab.fg_color.into())
            } else {
                fg_color.unwrap_or_else(|| inactive_tab.fg_color.into())
            };

            let row_colors = ElementColors {
                border: BorderColor::new(row_bg.to_linear()),
                bg: row_bg.to_linear().into(),
                text: row_fg.to_linear().into(),
            };

            let row_hover_colors = if active {
                None
            } else {
                let hover_bg = bg_color.unwrap_or_else(|| inactive_tab_hover.bg_color.into());
                let hover_fg = fg_color.unwrap_or_else(|| inactive_tab_hover.fg_color.into());
                Some(ElementColors {
                    border: BorderColor::new(hover_bg.to_linear()),
                    bg: hover_bg.to_linear().into(),
                    text: hover_fg.to_linear().into(),
                })
            };

            let title = Element::with_line(&font, &entry.title, palette)
                .item_type(UIItemType::TabBar(TabBarItem::Tab { tab_idx, active }));

            // Vertical sidebar rows intentionally omit the close-X button:
            // narrow rows make it visually busy, and tabs are still closable
            // via keybindings / the close-tab command. Tracked as a follow-up
            // alongside drag-to-reorder.
            let row_children: Vec<Element> = vec![title];

            // Row width = sidebar width minus the gutter on each side, so
            // the row's bg pill sits inside the sidebar with breathing room.
            let row_inner_width = (sidebar_width - 2.0 * row_gutter_px).max(0.0);
            let row = Element::new(&font, ElementContent::Children(row_children))
                .display(DisplayType::Block)
                .item_type(UIItemType::TabBar(TabBarItem::Tab { tab_idx, active }))
                .min_width(Some(Dimension::Pixels(row_inner_width)))
                .margin(BoxDimension {
                    left: row_gutter_h,
                    right: row_gutter_h,
                    top: Dimension::Cells(0.15),
                    bottom: Dimension::Cells(0.15),
                })
                .padding(BoxDimension {
                    left: row_padding_h,
                    right: row_padding_h,
                    top: row_padding_v,
                    bottom: row_padding_v,
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .colors(row_colors)
                .hover_colors(row_hover_colors);
            children.push(row);
        }

        let content = ElementContent::Children(children);
        let sidebar = Element::new(&font, content)
            .display(DisplayType::Block)
            .item_type(UIItemType::TabBar(TabBarItem::None))
            .min_width(Some(Dimension::Pixels(sidebar_width)))
            .min_height(Some(Dimension::Pixels(sidebar_height)))
            .colors(bar_colors);

        // Build at x=0 in local space, then translate the entire element to
        // the correct edge. compute_element places children relative to
        // bounds.min_x and the final translate makes positioning explicit
        // for the Right orientation (which sits at the window's right edge).
        let mut computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: sidebar_height,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: sidebar_width,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(0., 0., sidebar_width, sidebar_height),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 10,
            },
            &sidebar,
        )?;

        let bounds_x = match orientation {
            TabBarOrientation::Right => {
                (self.dimensions.pixel_width as f32) - sidebar_width - border.right.get() as f32
            }
            _ => border.left.get() as f32,
        };
        computed.translate(euclid::vec2(bounds_x, top_inset));

        Ok(computed)
    }

    pub fn paint_vertical_tab_bar(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let orientation = self.tab_bar_orientation();
        if !orientation.is_vertical() {
            return Ok(());
        }

        if self.fancy_tab_bar.is_none() {
            let palette = self.palette().clone();
            let bar = self.build_vertical_tab_bar(&palette)?;
            self.fancy_tab_bar.replace(bar);
        }

        let sidebar_width = self.tab_bar_pixel_width();
        let top_inset = self.vertical_sidebar_top_inset();
        let sidebar_height = (self.dimensions.pixel_height as f32 - top_inset).max(0.0);
        let border = self.get_os_border();
        let sidebar_x = match orientation {
            TabBarOrientation::Right => {
                (self.dimensions.pixel_width as f32) - sidebar_width - border.right.get() as f32
            }
            _ => border.left.get() as f32,
        };

        // Background fill. The element tree paints its own row backgrounds
        // but the empty area below the last row needs a base color. Start at
        // top_inset so the macOS traffic-light buttons (drawn by AppKit over
        // the window) remain visible and clickable.
        let tab_bar_colors = self
            .config
            .colors
            .as_ref()
            .and_then(|c| c.tab_bar.as_ref())
            .cloned()
            .unwrap_or_else(TabBarColors::default);
        let bg = tab_bar_colors.background().to_linear();
        self.filled_rectangle(
            layers,
            0,
            euclid::rect(sidebar_x, top_inset, sidebar_width, sidebar_height),
            bg,
        )?;

        let computed = self
            .fancy_tab_bar
            .as_ref()
            .expect("vertical_tab_bar populated above");
        let ui_items = computed.ui_items();

        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(computed, gl_state, None)?;

        self.ui_items.extend(ui_items);
        Ok(())
    }
}
