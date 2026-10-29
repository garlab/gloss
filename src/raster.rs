//! Terminal-independent straight RGBA pixels. Placement and baselines live above
//! this type so equations and ordinary images share transport without semantics.

#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
