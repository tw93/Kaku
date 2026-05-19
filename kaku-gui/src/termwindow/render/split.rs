use crate::termwindow::render::TripleLayerQuadAllocator;
use crate::termwindow::{UIItem, UIItemType};
use mux::pane::Pane;
use mux::tab::{PositionedSplit, SplitDirection};
use std::sync::Arc;

impl crate::TermWindow {
    pub fn paint_split(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        split: &PositionedSplit,
        all_splits: &[PositionedSplit],
        pane: &Arc<dyn Pane>,
    ) -> anyhow::Result<()> {
        let palette = pane.palette();
        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;

        let foreground = palette
            .split
            .to_linear()
            .mul_alpha(self.config.window_background_opacity);

        let border = self.get_os_border();
        let first_row_offset = if self.is_top_tab_bar_visible() {
            self.tab_bar_pixel_height()?
        } else {
            0.
        } + border.top.get() as f32;

        let (_, padding_top) = self.padding_left_top();
        let content_left = self.content_left_inset();
        let pos_y = split.top as f32 * cell_height + first_row_offset + padding_top;
        let pos_x = split.left as f32 * cell_width + content_left;

        let split_thickness = self.config.split_thickness;
        let is_horizontal = split.direction == SplitDirection::Horizontal;

        if is_horizontal {
            // Vertical line (left-right split): find the nearest boundary
            // horizontal splits at the top and bottom to align precisely.
            // Use max/min by key to select the closest candidate in nested layouts.
            let boundary_top = all_splits
                .iter()
                .filter(|s| {
                    s.direction == SplitDirection::Vertical
                        && s.top <= split.top
                        && split.left >= s.left
                        && split.left < s.left + s.size
                })
                .max_by_key(|s| s.top);
            let boundary_bottom = all_splits
                .iter()
                .filter(|s| {
                    s.direction == SplitDirection::Vertical
                        && s.top >= split.top + split.size
                        && split.left >= s.left
                        && split.left < s.left + s.size
                })
                .min_by_key(|s| s.top);

            // Extend to boundary split center, or stop at content edge
            let extend_top = match boundary_top {
                Some(b) => split.top as f32 - b.top as f32 - 1.0,
                None => -0.5,
            };
            let extend_bottom = match boundary_bottom {
                Some(b) => b.top as f32 - (split.top + split.size) as f32,
                None => -0.5,
            };
            let y_start = pos_y - (cell_height / 2.0) - extend_top * cell_height;
            let height = (1.0 + split.size as f32 + extend_top + extend_bottom) * cell_height;

            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    pos_x + (cell_width / 2.0) - (split_thickness / 2.0),
                    y_start,
                    split_thickness,
                    height,
                ),
                foreground,
            )?;
        } else {
            // Horizontal line (top-bottom split): find the nearest boundary
            // vertical splits at the left and right to align precisely.
            // Use max/min by key to select the closest candidate in nested layouts.
            let boundary_left = all_splits
                .iter()
                .filter(|s| {
                    s.direction == SplitDirection::Horizontal
                        && s.left <= split.left
                        && split.top >= s.top
                        && split.top < s.top + s.size
                })
                .max_by_key(|s| s.left);
            let boundary_right = all_splits
                .iter()
                .filter(|s| {
                    s.direction == SplitDirection::Horizontal
                        && s.left >= split.left + split.size
                        && split.top >= s.top
                        && split.top < s.top + s.size
                })
                .min_by_key(|s| s.left);

            // Extend to boundary split center, or stop at content edge
            let extend_left = match boundary_left {
                Some(b) => split.left as f32 - b.left as f32 - 1.0,
                None => -0.5,
            };
            let extend_right = match boundary_right {
                Some(b) => b.left as f32 - (split.left + split.size) as f32,
                None => -0.5,
            };
            let x_start = pos_x - (cell_width / 2.0) - extend_left * cell_width;
            let width = (1.0 + split.size as f32 + extend_left + extend_right) * cell_width;

            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    x_start,
                    pos_y + (cell_height / 2.0) - (split_thickness / 2.0),
                    width,
                    split_thickness,
                ),
                foreground,
            )?;
        }

        // UI item for hit testing
        let (x, y, width, height) = if is_horizontal {
            (
                content_left as usize + (split.left * cell_width as usize),
                padding_top as usize + first_row_offset as usize + split.top * cell_height as usize,
                cell_width as usize,
                split.size * cell_height as usize,
            )
        } else {
            (
                content_left as usize + (split.left * cell_width as usize),
                padding_top as usize + first_row_offset as usize + split.top * cell_height as usize,
                split.size * cell_width as usize,
                cell_height as usize,
            )
        };

        self.ui_items.push(UIItem {
            x,
            y,
            width,
            height,
            item_type: UIItemType::Split(split.clone()),
        });

        Ok(())
    }
}
