//! Basic Kitty graphics placements, without Unicode placeholders or tmux
//! passthrough. The caller must opt in and ensure stdout is a terminal.
//!
//! Protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::io::{self, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{raster::Raster, terminal::Geometry};

pub fn write_display(
    out: &mut impl Write,
    raster: &Raster,
    geometry: Geometry,
    left: usize,
    content_width: usize,
) -> io::Result<()> {
    let columns = raster.width.div_ceil(u32::from(geometry.cell_width.max(1)));
    let rows = raster
        .height
        .div_ceil(u32::from(geometry.cell_height.max(1)));
    let width = columns * u32::from(geometry.cell_width.max(1));
    let height = rows * u32::from(geometry.cell_height.max(1));
    // Padding to the exact cell rectangle prevents the terminal from stretching
    // tightly cropped glyphs to fill whole rows, changing the font size.
    let padded = pad(raster, width, height, 0, (height - raster.height) / 2)?;
    reserve(out, rows)?;
    let margin = left as u32 + (content_width as u32).saturating_sub(columns) / 2;
    if margin > 0 {
        write!(out, "\x1b[{margin}C")?;
    }
    transmit(out, &padded, columns, rows)?;
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
    let expected = u64::from(raster.width) * u64::from(raster.height) * 4;
    if raster.width == 0
        || raster.height == 0
        || columns == 0
        || rows == 0
        || expected != raster.rgba.len() as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid RGBA raster",
        ));
    }
    let encoded = STANDARD.encode(&raster.rgba);
    let mut chunks = encoded.as_bytes().chunks(4096).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            // q=2 suppresses replies; C=1 leaves cursor management to us.
            // No explicit image ID: the terminal allocates one without replacing
            // equations left in scrollback by a previous mdviewer invocation.
            write!(
                out,
                "\x1b_Ga=T,t=d,f=32,s={},v={},c={columns},r={rows},C=1,q=2,m={more};",
                raster.width, raster.height
            )?;
            first = false;
        } else {
            write!(out, "\x1b_Gq=2,m={more};")?;
        }
        out.write_all(chunk)?;
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
