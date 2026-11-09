//! Basic Kitty graphics placements, without Unicode placeholders or tmux
//! passthrough. The caller must opt in and ensure stdout is a terminal.
//!
//! Protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::io::{self, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    raster::{Compressed, Raster},
    terminal::Geometry,
};

pub fn write_display(
    out: &mut impl Write,
    raster: &Compressed,
    geometry: Geometry,
    left: usize,
    content_width: usize,
) -> io::Result<()> {
    let columns = raster.width.div_ceil(u32::from(geometry.cell_width.max(1)));
    let rows = raster
        .height
        .div_ceil(u32::from(geometry.cell_height.max(1)));
    let height = rows * u32::from(geometry.cell_height.max(1));
    reserve(out, rows)?;
    let margin = left as u32 + (content_width as u32).saturating_sub(columns) / 2;
    if margin > 0 {
        write!(out, "\x1b[{margin}C")?;
    }
    transmit_native(out, raster, (height - raster.height) / 2)?;
    write!(out, "\x1b[{rows}B\r\n")
}

/// Reserve all rows before painting so formulas near the bottom scroll into
/// view as a whole. Returns with the cursor at the region's upper-left corner.
pub(crate) fn reserve(out: &mut impl Write, rows: u32) -> io::Result<()> {
    for _ in 0..rows {
        write!(out, "\r\n")?;
    }
    write!(out, "\x1b[{rows}A\r")
}

/// Transmit and display at the current cursor, without moving it or requesting
/// a reply. Image IDs are allocated by the terminal to preserve older output.
pub(crate) fn transmit(
    out: &mut impl Write,
    raster: &Raster,
    columns: u32,
    rows: u32,
) -> io::Result<()> {
    // Compress before base64, as specified by the graphics protocol.
    transmit_placement(out, &raster.compress()?, Some((columns, rows)), 0)
}

/// Display native pixels without cell scaling. The caller moves to the starting
/// cell and supplies a vertical offset smaller than that cell's height.
pub(crate) fn transmit_native(out: &mut impl Write, raster: &Compressed, y: u32) -> io::Result<()> {
    transmit_placement(out, raster, None, y)
}

fn transmit_placement(
    out: &mut impl Write,
    raster: &Compressed,
    cells: Option<(u32, u32)>,
    y: u32,
) -> io::Result<()> {
    if raster.width == 0
        || raster.height == 0
        || raster.data.is_empty()
        || cells.is_some_and(|(columns, rows)| columns == 0 || rows == 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid RGBA raster",
        ));
    }
    // Encoding uses a fixed stack buffer instead of another full-size copy.
    let compression = if raster.zlib { ",o=z" } else { "" };
    let mut chunks = raster.data.chunks(3072).peekable();
    let mut first = true;
    let mut encoded = [0_u8; 4096];
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            // q=2 suppresses replies; C=1 leaves cursor management to us.
            // Terminal-allocated IDs preserve output from previous invocations.
            write!(
                out,
                "\x1b_Ga=T,t=d,f=32,s={},v={}",
                raster.width, raster.height
            )?;
            if let Some((columns, rows)) = cells {
                write!(out, ",c={columns},r={rows}")?;
            } else {
                // Omitting c/r preserves native size; offsets never stretch ink
                // to fill the surrounding text's reserved cell rectangle.
                write!(out, ",Y={y}")?;
            }
            write!(out, ",C=1,q=2{compression},m={more};")?;
            first = false;
        } else {
            write!(out, "\x1b_Gq=2,m={more};")?;
        }
        let count = STANDARD
            .encode_slice(chunk, &mut encoded)
            .expect("3072 bytes fit the base64 buffer");
        out.write_all(&encoded[..count])?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Place straight RGBA pixels in a transparent cell-aligned rectangle.
pub(crate) fn pad(raster: &Raster, width: u32, height: u32, x: u32, y: u32) -> io::Result<Raster> {
    let pixels = u64::from(width) * u64::from(height);
    let fits = x.checked_add(raster.width).is_some_and(|end| end <= width)
        && y.checked_add(raster.height)
            .is_some_and(|end| end <= height);
    if !fits
        || pixels == 0
        || pixels > 8_000_000
        || u64::from(raster.width) * u64::from(raster.height) * 4 != raster.rgba.len() as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid padded image dimensions",
        ));
    }
    let mut rgba = vec![0; pixels as usize * 4];
    let bytes = raster.width as usize * 4;
    for row in 0..raster.height {
        let source = row as usize * bytes;
        let destination = ((row + y) as usize * width as usize + x as usize) * 4;
        rgba[destination..destination + bytes]
            .copy_from_slice(&raster.rgba[source..source + bytes]);
    }
    Ok(Raster {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn decode(raster: &Raster) -> (Vec<u8>, usize, bool) {
        let mut output = Vec::new();
        transmit(&mut output, raster, raster.width, raster.height).unwrap();
        let output = String::from_utf8(output).unwrap();
        let mut payload = String::new();
        let mut chunks = 0;
        let compressed = output.contains(",o=z,");
        for command in output.split("\x1b_G").skip(1) {
            let (control, data) = command.split_once(';').unwrap();
            let data = data.strip_suffix("\x1b\\").unwrap();
            assert!(data.len() <= 4096 && data.len() % 4 == 0);
            assert!(control.contains("q=2"));
            payload.push_str(data);
            chunks += 1;
        }
        let mut bytes = STANDARD.decode(payload).unwrap();
        if compressed {
            let mut pixels = Vec::new();
            flate2::read::ZlibDecoder::new(bytes.as_slice())
                .read_to_end(&mut pixels)
                .unwrap();
            bytes = pixels;
        }
        (bytes, chunks, compressed)
    }

    #[test]
    fn compressed_and_incompressible_pixels_round_trip_with_bounded_chunks() {
        let mut random = 1_u32;
        let rgba: Vec<_> = (0..100 * 100 * 4)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                random as u8
            })
            .collect();
        let raster = Raster {
            width: 100,
            height: 100,
            rgba,
        };
        let (bytes, chunks, compressed) = decode(&raster);
        assert_eq!(bytes, raster.rgba);
        assert!(chunks > 1 && !compressed);
        let raster = Raster {
            rgba: vec![255; 100 * 100 * 4],
            ..raster
        };
        let (bytes, _, compressed) = decode(&raster);
        assert_eq!(bytes, raster.rgba);
        assert!(compressed);
    }
}
