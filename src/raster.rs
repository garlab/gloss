//! Terminal-independent straight RGBA pixels. Placement and baselines live above
//! this type so equations and ordinary images share transport without semantics.

use std::io::{self, Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Straight RGBA pixels compressed once with zlib, or kept raw when compression
/// does not help: the form Kitty graphics transmit. Equations are only ever
/// transmitted whole, so they and their cache keep this form, which is several
/// times smaller than pixels and need not be compressed again.
#[derive(Debug)]
pub struct Compressed {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// `data` is zlib-compressed rather than raw RGBA.
    pub zlib: bool,
}

impl Raster {
    pub fn compress(&self) -> io::Result<Compressed> {
        let expected = u64::from(self.width) * u64::from(self.height) * 4;
        if self.width == 0 || self.height == 0 || expected != self.rgba.len() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid RGBA raster",
            ));
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&self.rgba)?;
        let compressed = encoder.finish()?;
        let zlib = compressed.len() + 16 < self.rgba.len();
        Ok(Compressed {
            width: self.width,
            height: self.height,
            data: if zlib { compressed } else { self.rgba.clone() },
            zlib,
        })
    }
}

impl Compressed {
    /// The straight RGBA pixels again, for checks rather than rendering.
    pub fn decompress(&self) -> io::Result<Vec<u8>> {
        if !self.zlib {
            return Ok(self.data.clone());
        }
        let mut rgba = Vec::new();
        ZlibDecoder::new(self.data.as_slice()).read_to_end(&mut rgba)?;
        Ok(rgba)
    }
}

impl From<document_graphics::Raster> for Raster {
    fn from(pixels: document_graphics::Raster) -> Self {
        Self {
            width: pixels.width,
            height: pixels.height,
            rgba: pixels.rgba,
        }
    }
}
