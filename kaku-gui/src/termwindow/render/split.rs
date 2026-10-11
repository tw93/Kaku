use crate::termwindow::render::TripleLayerQuadAllocator;
use crate::termwindow::{UIItem, UIItemType};
use mux::pane::Pane;
use mux::tab::{PositionedSplit, SplitDirection};
use std::sync::Arc;

/// Rows from a top|bottom split's `top` row to the center of its row
/// gutter. Mux places `split.top` at `first.rows + gutter / 2`, so an even
/// gutter (the bundled `split_pane_gap = 2`) centers on the top edge of that
/// row and an odd one on its middle. The pane backgrounds meet at that
/// center; drawing the line half a row lower let one pane's background
/// cross it (#562).
pub(crate) fn split_row_center_offset(row_gutter: usize) -> f32 {
    if row_gutter % 2 == 0 {
        0.0
    } else {
        0.5
    }
}

/// Row a top|bottom split drag is aiming at: the row whose drawn line is
/// nearest the pointer. `rows_from_origin` is the pointer's distance from the
/// grid's top edge in fractional rows. For odd gutters this is the plain
/// floored row; for even ones the line sits on a row edge, so a grab in the
/// upper half of the hit area must not count as one row up.
pub(crate) fn split_drag_row(rows_from_origin: f32, row_center: f32) -> i64 {
    (rows_from_origin - row_center + 0.5).floor() as i64
}

/// Column a left|right split drag is aiming at. `cols_from_origin` is the
/// pointer's distance from the grid's left edge in fractional columns and
/// `grab` is how far from the line's center the drag started, so grabbing
/// anywhere in the gutter moves the line by the pointer's motion only.
pub(crate) fn split_drag_col(cols_from_origin: f32, grab: f32) -> i64 {
    (cols_from_origin - grab - 0.5).round() as i64
}

fn dims_inactive_panes(hsb: &config::HsbTransform) -> bool {
    hsb.hue != 1.0 || hsb.saturation != 1.0 || hsb.brightness != 1.0
}

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

        // With inactive panes dimmed, the brightness change already marks the
        // active pane, and a contrasting line only adds noise there. Drawing it
        // in the active pane's background keeps it invisible beside the active
        // pane while still separating two dimmed neighbours.
        let line_color = if dims_inactive_panes(&self.config.inactive_pane_hsb) {
            palette.background
        } else {
            palette.split
        };
        let foreground = line_color
            .to_linear()
            .mul_alpha(self.config.window_background_opacity);

        let first_row_offset = self.terminal_first_row_offset();

        let (_, padding_top) = self.padding_left_top();
        let content_left = self.content_left_inset();
        let row_y = |row: usize| row as f32 * cell_height + first_row_offset + padding_top;
        let pos_y = row_y(split.top);
        let row_center =
            split_row_center_offset(self.config.split_pane_gap.max(1) as usize) * cell_height;
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

            // Extend to the boundary split's line, or stop at the content edge.
            let y_start = match boundary_top {
                Some(b) => row_y(b.top) + row_center,
                None => pos_y,
            };
            let y_end = match boundary_bottom {
                Some(b) => row_y(b.top) + row_center,
                None => row_y(split.top + split.size),
            };
            let height = y_end - y_start;

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
                    pos_y + row_center - (split_thickness / 2.0),
                    width,
                    split_thickness,
                ),
                foreground,
            )?;
        }

        // UI item for hit testing. It spans the whole gutter rather than the
        // one-cell line, which is hard to hit on a scaled-down remote screen;
        // the gutter holds no pane text, so it steals no clicks.
        let gap = self.config.split_pane_gap as usize;
        let (x, y, width, height) = if is_horizontal {
            let gutter_cols = 1 + 2 * gap;
            (
                content_left as usize + (split.left.saturating_sub(gap) * cell_width as usize),
                padding_top as usize + first_row_offset as usize + split.top * cell_height as usize,
                gutter_cols * cell_width as usize,
                split.size * cell_height as usize,
            )
        } else {
            let gutter_rows = gap.max(1) as f32;
            let height = gutter_rows * cell_height;
            (
                content_left as usize + (split.left * cell_width as usize),
                // Centered on the drawn line.
                (pos_y + row_center - height / 2.0).max(0.0) as usize,
                split.size * cell_width as usize,
                height as usize,
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

#[cfg(test)]
mod test {
    use super::{dims_inactive_panes, split_drag_col, split_drag_row, split_row_center_offset};

    #[test]
    fn grabbing_a_wide_split_gutter_does_not_jump_the_line() {
        // A 5-column gutter (split_pane_gap = 2) around a line at column 40.
        let line_center = 40.5f32;
        for start in [38.0f32, 39.2, 40.5, 41.7, 42.9] {
            let grab = start - line_center;
            assert_eq!(split_drag_col(start, grab), 40, "grab at {start}");
            assert_eq!(split_drag_col(start + 1.0, grab), 41, "grab at {start}");
            assert_eq!(split_drag_col(start - 3.0, grab), 37, "grab at {start}");
        }
    }

    #[test]
    fn grabbing_anywhere_on_a_split_line_does_not_move_it() {
        let split_top = 11i64;
        for gutter in 1..=4usize {
            let center = split_row_center_offset(gutter);
            let line = split_top as f32 + center;
            // Within half a row of the drawn line the row does not change.
            for offset in [-0.49f32, -0.25, 0.0, 0.25, 0.49] {
                assert_eq!(
                    split_drag_row(line + offset, center),
                    split_top,
                    "gutter {gutter} offset {offset}"
                );
            }
            assert_eq!(split_drag_row(line + 1.0, center), split_top + 1);
            assert_eq!(split_drag_row(line - 1.0, center), split_top - 1);
        }
    }

    #[test]
    fn split_line_blends_in_only_when_inactive_panes_are_dimmed() {
        let identity = config::HsbTransform::default();
        assert!(!dims_inactive_panes(&identity));
        assert!(dims_inactive_panes(&config::HsbTransform {
            brightness: 0.7,
            ..identity
        }));
        assert!(dims_inactive_panes(&config::HsbTransform {
            saturation: 0.5,
            ..identity
        }));
    }

    #[test]
    fn top_bottom_split_line_sits_on_the_background_seam() {
        let first_rows = 10usize;
        for gutter in 1..=4usize {
            // Mux: split.top = first.rows + gutter / 2.
            let split_top = first_rows + gutter / 2;
            let line = split_top as f32 + split_row_center_offset(gutter);
            // pane.rs: the backgrounds meet half a gutter below the first pane.
            let seam = first_rows as f32 + gutter as f32 / 2.0;
            assert_eq!(line, seam, "gutter {gutter}");
        }
    }

    #[test]
    fn pane_backgrounds_split_lines_and_mouse_share_the_first_row_origin() {
        // Production code only, so this test's own text cannot satisfy it.
        for (name, source) in [
            ("pane.rs", include_str!("pane.rs")),
            ("split.rs", include_str!("split.rs")),
            ("mouseevent.rs", include_str!("../mouseevent.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(
                production.contains("self.terminal_first_row_offset()"),
                "{} must take its top origin from terminal_first_row_offset (#562)",
                name
            );
        }
    }
}
