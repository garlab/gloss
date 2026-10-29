//! A bounded reading column shared by text and equation rendering.

use crate::terminal::Geometry;

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub left: usize,
    pub width: usize,
}

impl Viewport {
    pub fn new(geometry: Geometry, max_width: u16, margin: u16) -> Self {
        // Leave the last terminal column unused to avoid implicit cursor wrapping.
        let available = usize::from(geometry.columns.saturating_sub(1).max(1));
        // Give narrow windows their space back before making the reading column
        // smaller than twenty cells. Explicit large margins are bounded too.
        let left = usize::from(margin).min(available.saturating_sub(20) / 2);
        let width = available - left * 2;
        Self {
            left,
            width: if max_width == 0 {
                width
            } else {
                width.min(usize::from(max_width))
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_width_and_margins_fit_wide_and_narrow_terminals() {
        let wide = Viewport::new(
            Geometry {
                columns: 160,
                ..Default::default()
            },
            80,
            2,
        );
        assert_eq!((wide.left, wide.width), (2, 80));
        let narrow = Viewport::new(
            Geometry {
                columns: 12,
                ..Default::default()
            },
            80,
            2,
        );
        assert_eq!((narrow.left, narrow.width), (0, 11));
        for columns in 1..200 {
            for margin in [0, 2, 12, u16::MAX] {
                let view = Viewport::new(
                    Geometry {
                        columns,
                        ..Default::default()
                    },
                    0,
                    margin,
                );
                assert!(view.width > 0);
                assert!(
                    view.left * 2 + view.width <= usize::from(columns.saturating_sub(1).max(1))
                );
            }
        }
    }
}
