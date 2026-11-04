//! Bounded local image decoding with an invocation-local, byte-accounted cache.
//! No network, nested SVG resources, system font scan, or persistent cache.

use std::{
    collections::{VecDeque, hash_map::DefaultHasher},
    fs::OpenOptions,
    hash::Hasher,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use document_graphics::images::{self as decoder, Format as ImageFormat};
#[cfg(test)]
use std::io::Cursor;
use std::sync::atomic::AtomicBool;

use crate::raster::Raster;
use document_graphics::svg::MAX_SOURCE as SVG_BYTES;

const FILE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const READ_BYTES: usize = 64 * 1024 * 1024;
const OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    Invalid,
    Limit,
    Io,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    Jpeg,
    Svg,
}

struct Cached {
    hash: u64,
    source: Box<[u8]>,
    format: Format,
    bounds: (u32, u32),
    raster: Arc<Raster>,
}

impl Cached {
    fn bytes(&self) -> usize {
        self.source.len() + self.raster.rgba.len()
    }
}

#[derive(Default)]
pub struct Loader {
    cache: VecDeque<Cached>,
    cache_bytes: usize,
    read_bytes: usize,
    output_bytes: u64,
    loads: usize,
    decodes: usize,
    decode_time: Duration,
}

impl Loader {
    pub fn load(
        &mut self,
        base: &Path,
        target: &str,
        width: u32,
        height: u32,
    ) -> Result<Arc<Raster>, Error> {
        let path = local_path(base, target)?;
        let format = Self::format(&path).ok_or(Error::Unsupported)?;
        self.load_file(&path, Some(format), width, height)
    }

    /// Explicit CLI paths are not URLs. Preserve spaces, percent signs and native
    /// path characters; unknown extensions may be sniffed within the same limits.
    pub(crate) fn load_path(
        &mut self,
        path: &Path,
        width: u32,
        height: u32,
    ) -> Result<Arc<Raster>, Error> {
        self.load_file(path, Self::format(path), width, height)
    }

    fn format(path: &Path) -> Option<Format> {
        match path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") => Some(Format::Png),
            Some("jpg" | "jpeg") => Some(Format::Jpeg),
            Some("svg") => Some(Format::Svg),
            _ => None,
        }
    }

    fn load_file(
        &mut self,
        path: &Path,
        format: Option<Format>,
        width: u32,
        height: u32,
    ) -> Result<Arc<Raster>, Error> {
        if self.loads >= 4096 || width == 0 || height == 0 {
            return Err(Error::Limit);
        }
        self.loads += 1;
        let mut open = OpenOptions::new();
        open.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = open.open(path).map_err(|_| Error::Io)?;
        let metadata = file.metadata().map_err(|_| Error::Io)?;
        let limit = if format == Some(Format::Svg) {
            SVG_BYTES
        } else {
            FILE_BYTES
        };
        if !metadata.is_file()
            || metadata.len() > limit as u64
            || metadata.len() > READ_BYTES.saturating_sub(self.read_bytes) as u64
        {
            return Err(Error::Limit);
        }
        // Fixed-size storage also bounds a file that grows after metadata was read.
        let mut bytes = vec![0; metadata.len() as usize + 1];
        let mut length = 0;
        while length < bytes.len() {
            let count = file.read(&mut bytes[length..]).map_err(|_| Error::Io)?;
            if count == 0 {
                break;
            }
            length += count;
        }
        self.read_bytes += length;
        if length == bytes.len() || self.read_bytes > READ_BYTES {
            return Err(Error::Limit);
        }
        bytes.truncate(length);
        let format = format.unwrap_or_else(|| {
            if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                Format::Png
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                Format::Jpeg
            } else {
                Format::Svg
            } // The bounded SVG validator rejects other data.
        });
        if format == Format::Svg && bytes.len() > SVG_BYTES {
            return Err(Error::Limit);
        }
        let mut hasher = DefaultHasher::new();
        hasher.write(&bytes);
        let hash = hasher.finish();
        let bounds = (width, height);
        if let Some(index) = self.cache.iter().position(|entry| {
            entry.hash == hash
                && entry.bounds == bounds
                && entry.format == format
                && entry.source.as_ref() == bytes.as_slice()
        }) {
            let entry = self.cache.remove(index).unwrap();
            let raster = entry.raster.clone();
            self.cache.push_back(entry);
            return Ok(raster);
        }
        if self.decodes >= 64 || self.decode_time >= Duration::from_secs(2) {
            return Err(Error::Limit);
        }
        self.decodes += 1;
        let started = Instant::now();
        let decoded = match format {
            Format::Png => bitmap(&bytes, ImageFormat::Png, bounds),
            Format::Jpeg => bitmap(&bytes, ImageFormat::Jpeg, bounds),
            Format::Svg => self.svg(&bytes, bounds),
        };
        self.decode_time += started.elapsed();
        let raster = Arc::new(decoded?);
        let cost = bytes.len() + raster.rgba.len();
        if cost <= CACHE_BYTES {
            while self.cache_bytes + cost > CACHE_BYTES || self.cache.len() >= 32 {
                self.cache_bytes -= self.cache.pop_front().unwrap().bytes();
            }
            self.cache.push_back(Cached {
                hash,
                source: bytes.into_boxed_slice(),
                format,
                bounds,
                raster: raster.clone(),
            });
            self.cache_bytes += cost;
        }
        Ok(raster)
    }

    /// Charge the actual cell-padded pixels, including cache hits.
    pub fn reserve_output(&mut self, pixels: u64) -> bool {
        let bytes = pixels.saturating_mul(4);
        if pixels == 0
            || pixels > 8_000_000
            || bytes > OUTPUT_BYTES.saturating_sub(self.output_bytes)
        {
            return false;
        }
        self.output_bytes += bytes;
        true
    }

    fn svg(&mut self, bytes: &[u8], bounds: (u32, u32)) -> Result<Raster, Error> {
        let source = std::str::from_utf8(bytes).map_err(|_| Error::Invalid)?;
        let bounds = (
            bounds.0.min(document_graphics::MAX_EDGE),
            bounds.1.min(document_graphics::MAX_EDGE),
        );
        document_graphics::svg::rasterize(source, document_graphics::RasterOptions::fit(bounds))
            .map(Into::into)
            .map_err(|error| match error {
                document_graphics::svg::Error::Invalid => Error::Invalid,
                document_graphics::svg::Error::Unsupported => Error::Unsupported,
                document_graphics::svg::Error::Limit => Error::Limit,
                _ => Error::Unsupported,
            })
    }
}

fn local_path(base: &Path, target: &str) -> Result<PathBuf, Error> {
    if target.is_empty() || target.len() > 4096 || target.contains(['?', '#']) {
        return Err(Error::Unsupported);
    }
    let mut decoded = Vec::with_capacity(target.len());
    let mut bytes = target.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let digit = |byte: u8| {
                (byte as char)
                    .to_digit(16)
                    .map(|n| n as u8)
                    .ok_or(Error::Invalid)
            };
            digit(bytes.next().ok_or(Error::Invalid)?)? * 16
                + digit(bytes.next().ok_or(Error::Invalid)?)?
        } else {
            byte
        });
    }
    let decoded = String::from_utf8(decoded).map_err(|_| Error::Invalid)?;
    if decoded.starts_with("//")
        || decoded.contains([':', '\\'])
        || decoded.chars().any(char::is_control)
    {
        return Err(Error::Unsupported);
    }
    Ok(base.join(decoded))
}

fn bitmap(bytes: &[u8], format: ImageFormat, bounds: (u32, u32)) -> Result<Raster, Error> {
    decoder::decode(
        bytes,
        format,
        bounds,
        decoder::Limits::default(),
        &AtomicBool::new(false),
    )
    .map(Into::into)
    .map_err(|e| match e {
        decoder::Error::Limit => Error::Limit,
        _ => Error::Invalid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "mdviewer-images-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).unwrap();
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn png_pixels_paths_and_content_cache_preserve_alpha_and_invalidate_changes() {
        let dir = Temp::new();
        let data = png(2, 3, [255, 0, 0, 128]);
        dir.write("red image.png", &data);
        dir.write("alias.png", &data);
        let mut loader = Loader::default();
        let red = loader.load(&dir.0, "red%20image.png", 20, 20).unwrap();
        assert_eq!((red.width, red.height), (2, 3));
        assert_eq!(&red.rgba[..4], &[255, 0, 0, 128]);
        assert!(Arc::ptr_eq(
            &red,
            &loader.load(&dir.0, "alias.png", 20, 20).unwrap()
        ));
        assert_eq!(loader.decodes, 1);
        dir.write("red image.png", &png(2, 3, [0, 0, 255, 255]));
        let blue = loader.load(&dir.0, "red%20image.png", 20, 20).unwrap();
        assert!(!Arc::ptr_eq(&red, &blue));
        assert_eq!(&blue.rgba[..4], &[0, 0, 255, 255]);
        let small = loader.load(&dir.0, "alias.png", 1, 1).unwrap();
        assert_eq!((small.width, small.height), (1, 1));
        for reference in [
            "https://example.com/a.png",
            "file:///image.png",
            "//example.com/a.png",
            "data:image/png;base64,abc",
            "a.png?x=1",
            "%00.png",
            "a%GG.png",
        ] {
            assert!(local_path(&dir.0, reference).is_err(), "{reference}");
        }
    }

    #[test]
    fn jpeg_orientation_is_applied_before_fitting() {
        let dir = Temp::new();
        let mut jpeg = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(6, 3, image::Rgb([200, 100, 50]))
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
        oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend_from_slice(exif);
        oriented.extend_from_slice(&jpeg.get_ref()[2..]);
        dir.write("rotated.jpg", &oriented);
        let raster = Loader::default()
            .load(&dir.0, "rotated.jpg", 20, 20)
            .unwrap();
        assert_eq!((raster.width, raster.height), (3, 6));
        assert!(
            raster
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
    }

    #[test]
    fn svg_uses_straight_alpha_and_loads_only_bundled_fonts_when_needed() {
        let mut loader = Loader::default();
        let shape = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red" opacity="0.5"/></svg>"#;
        let raster = loader.svg(shape, (100, 100)).unwrap();
        assert_eq!(&raster.rgba[..4], &[255, 0, 0, 128]);
        let raster = loader.svg(shape, (u32::MAX, u32::MAX)).unwrap();
        assert_eq!((raster.width, raster.height), (20, 10));
        let text = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="2" y="22" font-size="20">Hello</text></svg>"#;
        let raster = loader.svg(text, (100, 100)).unwrap();
        assert!(
            raster
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        assert!(matches!(loader.svg("<svg xmlns='http://www.w3.org/2000/svg' width='80' height='20'><text x='0' y='16'>中文</text></svg>".as_bytes(), (100, 100)), Err(Error::Unsupported)));
    }

    #[test]
    fn malformed_expensive_and_external_svg_content_falls_back() {
        for source in [
            "<!DOCTYPE svg [<!ENTITY e SYSTEM 'file:///missing'>]><svg>&e;</svg>",
            "<svg><image href='missing.png'/></svg>",
            "<svg><image href='data:image/png;base64,AA=='/></svg>",
            "<svg><use href='#x'/></svg>",
            "<svg><filter/></svg>",
            "<svg><rect fill='url(https://example.com/paint)'/></svg>",
            "<svg><style>@import 'theme.css';</style></svg>",
            "<svg><script>anything</script></svg>",
        ] {
            assert!(
                document_graphics::svg::validate(source).is_err(),
                "{source}"
            );
        }
        assert!(
            document_graphics::svg::validate(&format!(
                "<svg>{}{}</svg>",
                "<g>".repeat(40),
                "</g>".repeat(40)
            ))
            .is_err()
        );
        assert!(
            document_graphics::svg::validate(&format!("<svg>{}</svg>", "<path/>".repeat(4096)))
                .is_err()
        );
        assert!(
            document_graphics::svg::validate(&format!(
                "<svg><path d='{}'/></svg>",
                "M0 0 ".repeat(14_000)
            ))
            .is_err()
        );
    }

    #[test]
    fn file_decode_cache_and_output_limits_are_bounded() {
        let dir = Temp::new();
        let file = std::fs::File::create(dir.0.join("large.png")).unwrap();
        file.set_len(FILE_BYTES as u64 + 1).unwrap();
        let mut loader = Loader::default();
        assert!(matches!(
            loader.load(&dir.0, "large.png", 80, 80),
            Err(Error::Limit)
        ));
        assert_eq!(loader.read_bytes, 0);
        dir.write("wide.png", &png(8193, 1, [0, 0, 0, 255]));
        assert!(loader.load(&dir.0, "wide.png", 80, 80).is_err());
        for index in 0..10 {
            dir.write("cache.png", &png(512, 512, [index, 0, 0, 255]));
            loader.load(&dir.0, "cache.png", 600, 600).unwrap();
            assert!(loader.cache_bytes <= CACHE_BYTES);
        }
        assert!(loader.cache.len() < 10);
        assert_eq!(
            loader.cache_bytes,
            loader.cache.iter().map(Cached::bytes).sum::<usize>()
        );
        loader.decodes = 64;
        dir.write("miss.png", &png(2, 2, [100, 0, 0, 255]));
        assert!(matches!(
            loader.load(&dir.0, "miss.png", 80, 80),
            Err(Error::Limit)
        ));
        assert!(loader.load(&dir.0, "cache.png", 600, 600).is_ok());
        assert!(loader.reserve_output(8_000_000));
        assert!(loader.reserve_output(8_000_000));
        assert!(!loader.reserve_output(8_000_000));
    }
}
