#![doc = include_str!("../README.md")]

pub mod mermaid;
pub mod svg;
pub mod worker;

pub const MAX_PIXELS: usize = 1_000_000;
pub const MAX_EDGE: u32 = 8192;

/// Straight RGBA pixels and unscaled SVG dimensions. Moving this value transfers
/// its pixel buffer; callers need not copy it into an intermediate image format.
#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub rgba: Vec<u8>,
}

/// Raster fitting is independent of logical layout. Bounds are physical pixels;
/// scale is the requested physical pixels per logical SVG unit (1x or Retina).
/// All output remains subject to the crate's pixel, edge and source-size caps.
#[derive(Clone, Copy, Debug)]
pub struct RasterOptions {
    pub bounds: (u32, u32),
    pub scale: f64,
}
impl RasterOptions {
    pub const fn fit(bounds: (u32, u32)) -> Self {
        Self { bounds, scale: 1. }
    }
    pub const fn scaled(scale: f64) -> Self {
        Self {
            bounds: (MAX_EDGE, MAX_EDGE),
            scale,
        }
    }
    pub(crate) fn valid(self) -> bool {
        self.bounds.0 > 0
            && self.bounds.1 > 0
            && self.bounds.0 <= MAX_EDGE
            && self.bounds.1 <= MAX_EDGE
            && self.scale.is_finite()
            && self.scale > 0.
            && self.scale <= 4.
    }
}
