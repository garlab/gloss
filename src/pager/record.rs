//! Record only the renderer's own small escape-sequence vocabulary. This is not
//! a general terminal emulator and never consumes unfiltered document escapes.
//! Completed rows and compressed image payloads live in anonymous temporary files.
use crate::{document::Document, layout::Block, render, terminal::Geometry};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use unicode_width::UnicodeWidthStr;

const MAX_SPOOL: u64 = 256 * 1024 * 1024;
const MAX_ROWS: usize = 1_000_000;
const MAX_IMAGES: usize = 32_768;
const MAX_ROW_BYTES: usize = 256 * 1024;
const MAX_EDITABLE_BYTES: usize = 8 * 1024 * 1024;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn limit() -> io::Error {
    invalid("pager storage limit exceeded; use direct output or a smaller document")
}

#[derive(Clone, Copy)]
pub(super) struct Row {
    offset: u64,
    length: u32,
    pub anchor: u32,
}
#[derive(Clone, Debug)]
pub(super) struct Picture {
    offset: u64,
    length: u64,
    pub row: usize,
    pub column: usize,
    pub columns: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    compressed: bool,
}
pub(super) struct Page {
    text: File,
    pixels: File,
    pub rows: Vec<Row>,
    pub pictures: Vec<Picture>,
    pub geometry: Geometry,
    pub warnings: Vec<String>,
    pub max_image_rows: usize,
}
impl Page {
    pub fn row(&mut self, index: usize) -> io::Result<(Vec<u8>, String)> {
        let row = self.rows[index];
        self.text.seek(SeekFrom::Start(row.offset))?;
        let mut data = vec![0; row.length as usize];
        self.text.read_exact(&mut data)?;
        let split = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize + 4;
        let plain = String::from_utf8(data[split..].to_vec())
            .map_err(|_| invalid("invalid recorded text"))?;
        Ok((data[4..split].to_vec(), plain))
    }
    pub fn upload(&mut self, index: usize, number: u32, out: &mut impl Write) -> io::Result<()> {
        let p = &self.pictures[index];
        self.pixels.seek(SeekFrom::Start(p.offset))?;
        let mut left = p.length;
        let mut chunk = [0; 4096];
        let mut first = true;
        while left > 0 {
            let count = left.min(chunk.len() as u64) as usize;
            self.pixels.read_exact(&mut chunk[..count])?;
            left -= count as u64;
            if first {
                write!(
                    out,
                    "\x1b_Ga=t,t=d,f=32,I={number},s={},v={},q=2{},m={};",
                    p.width,
                    p.height,
                    if p.compressed { ",o=z" } else { "" },
                    u8::from(left > 0)
                )?;
                first = false;
            } else {
                write!(out, "\x1b_Gq=2,m={};", u8::from(left > 0))?;
            }
            out.write_all(&chunk[..count])?;
            out.write_all(b"\x1b\\")?;
        }
        Ok(())
    }
    pub fn location(&self, top: usize) -> (u32, usize) {
        let index = top.min(self.rows.len().saturating_sub(1));
        let anchor = self.rows.get(index).map_or(0, |r| r.anchor);
        let start = self.rows[..=index]
            .iter()
            .rposition(|r| r.anchor != anchor)
            .map_or(0, |i| i + 1);
        (anchor, index - start)
    }
    pub fn restore(&self, (anchor, offset): (u32, usize)) -> usize {
        let start = self
            .rows
            .partition_point(|r| r.anchor < anchor)
            .min(self.rows.len().saturating_sub(1));
        let end = self.rows[start..]
            .iter()
            .position(|r| r.anchor != self.rows[start].anchor)
            .map_or(self.rows.len(), |n| start + n);
        start + offset.min(end.saturating_sub(start + 1))
    }
}

#[derive(Default)]
struct RowData {
    encoded: Vec<u8>,
    plain: String,
    anchor: u32,
}
#[derive(Clone, Default)]
struct Attributes {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    fg: Option<u8>,
    bg: Option<u8>,
}
impl Attributes {
    fn update(&mut self, params: &str) -> io::Result<()> {
        let mut codes = params.split(';').map(|s| {
            if s.is_empty() {
                Ok(0)
            } else {
                s.parse::<u8>().map_err(|_| invalid("invalid SGR"))
            }
        });
        while let Some(code) = codes.next() {
            match code? {
                0 => *self = Self::default(),
                1 => self.bold = true,
                22 => self.bold = false,
                3 => self.italic = true,
                23 => self.italic = false,
                4 => self.underline = true,
                24 => self.underline = false,
                9 => self.strike = true,
                29 => self.strike = false,
                39 => self.fg = None,
                49 => self.bg = None,
                code @ (38 | 48) => {
                    if codes.next().transpose()? != Some(5) {
                        return Err(invalid("unsupported SGR color"));
                    }
                    let color = codes
                        .next()
                        .transpose()?
                        .ok_or_else(|| invalid("missing SGR color"))?;
                    if code == 38 {
                        self.fg = Some(color);
                    } else {
                        self.bg = Some(color);
                    }
                }
                _ => return Err(invalid("unsupported renderer SGR")),
            }
        }
        Ok(())
    }
    fn write(&self, out: &mut Vec<u8>) -> io::Result<()> {
        out.extend_from_slice(b"\x1b[0m");
        for (enabled, code) in [
            (self.bold, 1),
            (self.italic, 3),
            (self.underline, 4),
            (self.strike, 9),
        ] {
            if enabled {
                write!(out, "\x1b[{code}m")?;
            }
        }
        if let Some(c) = self.fg {
            write!(out, "\x1b[38;5;{c}m")?;
        }
        if let Some(c) = self.bg {
            write!(out, "\x1b[48;5;{c}m")?;
        }
        Ok(())
    }
}

pub(super) struct Recorder {
    text: BufWriter<File>,
    pixels: BufWriter<File>,
    text_bytes: u64,
    pixel_bytes: u64,
    rows: Vec<Row>,
    pictures: Vec<Picture>,
    active: VecDeque<RowData>,
    active_bytes: usize,
    row: usize,
    column: usize,
    anchor: u32,
    keep: usize,
    geometry: Geometry,
    attrs: Attributes,
    link: String,
    text_pending: Vec<u8>,
    escape: Vec<u8>,
    picture: Option<Picture>,
    cancel: Arc<AtomicBool>,
}
impl Recorder {
    pub fn new(geometry: Geometry, cancel: Arc<AtomicBool>) -> io::Result<Self> {
        Ok(Self {
            text: BufWriter::new(tempfile::tempfile()?),
            pixels: BufWriter::new(tempfile::tempfile()?),
            text_bytes: 0,
            pixel_bytes: 0,
            rows: Vec::new(),
            pictures: Vec::new(),
            active: VecDeque::new(),
            active_bytes: 0,
            row: 0,
            column: 0,
            anchor: 0,
            keep: usize::from(geometry.rows) + 16,
            geometry,
            attrs: Attributes::default(),
            link: String::new(),
            text_pending: Vec::new(),
            escape: Vec::new(),
            picture: None,
            cancel,
        })
    }
    fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "render cancelled",
            ));
        }
        if self.text_bytes + self.pixel_bytes + self.active_bytes as u64 > MAX_SPOOL {
            return Err(limit());
        }
        Ok(())
    }
    fn current(&mut self) -> io::Result<&mut RowData> {
        if self.row < self.rows.len() {
            return Err(invalid("renderer rewound beyond its reserved rows"));
        }
        while self.rows.len() + self.active.len() <= self.row {
            if self.rows.len() + self.active.len() >= MAX_ROWS {
                return Err(limit());
            }
            self.active.push_back(RowData {
                anchor: self.anchor,
                ..Default::default()
            });
        }
        Ok(&mut self.active[self.row - self.rows.len()])
    }
    fn commit(&mut self) -> io::Result<()> {
        let row = self.active.pop_front().unwrap();
        let length = 4 + row.encoded.len() + row.plain.len();
        if length > MAX_ROW_BYTES
            || self.text_bytes + self.pixel_bytes + self.active_bytes as u64 + 4 > MAX_SPOOL
        {
            return Err(limit());
        }
        self.rows.push(Row {
            offset: self.text_bytes,
            length: length as u32,
            anchor: row.anchor,
        });
        self.text
            .write_all(&(row.encoded.len() as u32).to_le_bytes())?;
        self.text.write_all(&row.encoded)?;
        self.text.write_all(row.plain.as_bytes())?;
        self.text_bytes += length as u64;
        self.active_bytes -= length - 4;
        Ok(())
    }
    fn advance(&mut self, count: usize) -> io::Result<()> {
        self.row = self
            .row
            .checked_add(count)
            .filter(|r| *r < MAX_ROWS)
            .ok_or_else(limit)?;
        self.current()?;
        while self.active.len() > self.keep {
            self.commit()?;
        }
        Ok(())
    }
    fn text(&mut self) -> io::Result<()> {
        if self.text_pending.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::take(&mut self.text_pending);
        let value = std::str::from_utf8(&bytes).map_err(|_| invalid("invalid renderer UTF-8"))?;
        let width = UnicodeWidthStr::width(value);
        if self.column + width > usize::from(self.geometry.columns) {
            return Err(invalid("renderer text exceeded pager width"));
        }
        let mut encoded = Vec::new();
        write!(encoded, "\x1b[{}G", self.column + 1)?;
        self.attrs.write(&mut encoded)?;
        if !self.link.is_empty() {
            write!(encoded, "\x1b]8;;{}\x1b\\", self.link)?;
        }
        encoded.extend_from_slice(&bytes);
        if !self.link.is_empty() {
            encoded.extend_from_slice(b"\x1b]8;;\x1b\\");
        }
        encoded.extend_from_slice(b"\x1b[0m");
        let anchor = self.anchor;
        let added = encoded.len() + value.len();
        if self.active_bytes + added > MAX_EDITABLE_BYTES
            || self.text_bytes + self.pixel_bytes + (self.active_bytes + added) as u64 > MAX_SPOOL
        {
            return Err(limit());
        }
        let row = self.current()?;
        if row.encoded.is_empty() {
            row.anchor = anchor;
        }
        if row.encoded.len() + row.plain.len() + encoded.len() + value.len() > MAX_ROW_BYTES {
            return Err(limit());
        }
        row.encoded.extend_from_slice(&encoded);
        row.plain.push_str(value);
        self.active_bytes += added;
        self.column += width;
        self.text_pending = bytes;
        self.text_pending.clear();
        Ok(())
    }
    fn command(&mut self) -> io::Result<()> {
        let bytes = std::mem::take(&mut self.escape);
        let command =
            std::str::from_utf8(&bytes).map_err(|_| invalid("invalid renderer escape"))?;
        if let Some(csi) = command.strip_prefix("\x1b[") {
            let (params, final_byte) = csi.split_at(csi.len() - 1);
            if final_byte == "m" {
                self.attrs.update(params)?;
            } else {
                let count = if params.is_empty() {
                    1
                } else {
                    params
                        .parse::<usize>()
                        .map_err(|_| invalid("invalid cursor movement"))?
                        .max(1)
                };
                match final_byte {
                    "A" => {
                        self.row = self
                            .row
                            .checked_sub(count)
                            .ok_or_else(|| invalid("negative renderer row"))?
                    }
                    "B" => self.advance(count)?,
                    "C" => {
                        self.column = self
                            .column
                            .checked_add(count)
                            .filter(|c| *c <= self.geometry.columns as usize)
                            .ok_or_else(limit)?
                    }
                    _ => return Err(invalid("unsupported renderer cursor command")),
                }
            }
        } else if let Some(link) = command
            .strip_prefix("\x1b]8;;")
            .and_then(|s| s.strip_suffix("\x1b\\"))
        {
            if !link.is_empty() && !crate::links::allowed(link) {
                return Err(invalid("invalid recorded hyperlink"));
            }
            self.link = link.into();
        } else if let Some(graphic) = command
            .strip_prefix("\x1b_G")
            .and_then(|s| s.strip_suffix("\x1b\\"))
        {
            self.graphic(graphic)?;
        } else {
            return Err(invalid("unsupported renderer escape"));
        }
        self.escape = bytes;
        self.escape.clear();
        Ok(())
    }
    fn graphic(&mut self, graphic: &str) -> io::Result<()> {
        let (control, data) = graphic
            .split_once(';')
            .ok_or_else(|| invalid("invalid graphics packet"))?;
        let fields: std::collections::HashMap<_, _> = control
            .split(',')
            .filter_map(|p| p.split_once('='))
            .collect();
        let number = |key| {
            fields
                .get(key)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| invalid("invalid graphics dimensions"))
        };
        if fields.contains_key("a") {
            if fields.get("a") != Some(&"T")
                || self.picture.is_some()
                || self.pictures.len() >= MAX_IMAGES
            {
                return Err(limit());
            }
            let p = Picture {
                offset: self.pixel_bytes,
                length: 0,
                row: self.row,
                column: self.column,
                columns: number("c")?,
                rows: number("r")?,
                width: number("s")?,
                height: number("v")?,
                compressed: fields.get("o") == Some(&"z"),
            };
            if p.rows == 0
                || p.columns == 0
                || p.rows as usize >= self.keep
                || p.width != p.columns * u32::from(self.geometry.cell_width)
                || p.height != p.rows * u32::from(self.geometry.cell_height)
            {
                return Err(invalid("invalid recorded image rectangle"));
            }
            self.picture = Some(p);
        }
        if data.len() > 4096 || !data.len().is_multiple_of(4) {
            return Err(invalid("invalid graphics chunk"));
        }
        if self.text_bytes + self.pixel_bytes + self.active_bytes as u64 + data.len() as u64
            > MAX_SPOOL
        {
            return Err(limit());
        }
        let picture = self
            .picture
            .as_mut()
            .ok_or_else(|| invalid("missing image header"))?;
        self.pixels.write_all(data.as_bytes())?;
        picture.length += data.len() as u64;
        self.pixel_bytes += data.len() as u64;
        match fields.get("m") {
            Some(&"0") => self.pictures.push(self.picture.take().unwrap()),
            Some(&"1") => {}
            _ => return Err(invalid("missing image continuation flag")),
        }
        Ok(())
    }
    pub fn anchor(&mut self, block: &Block) -> io::Result<()> {
        self.text()?;
        let start = match block {
            Block::Text(runs) => runs.iter().filter_map(|r| r.source.map(|s| s.start)).min(),
            Block::Code { source, .. } | Block::DisplayMath { source, .. } => Some(source.start),
            Block::Image(i) => Some(i.source.start),
            Block::Table(t) => Some(t.source.start),
        };
        if let Some(start) = start {
            self.anchor = self.anchor.max(start as u32);
        }
        self.check()
    }
    fn finish(mut self, warnings: Vec<String>) -> io::Result<Page> {
        self.text()?;
        if !self.escape.is_empty() || self.picture.is_some() {
            return Err(invalid("unfinished renderer command"));
        }
        self.current()?;
        while !self.active.is_empty() {
            self.commit()?;
        }
        self.text.flush()?;
        self.pixels.flush()?;
        self.pictures.sort_by_key(|p| p.row);
        let max_image_rows = self
            .pictures
            .iter()
            .map(|p| p.rows as usize)
            .max()
            .unwrap_or(0);
        Ok(Page {
            text: self.text.into_inner().map_err(|e| e.into_error())?,
            pixels: self.pixels.into_inner().map_err(|e| e.into_error())?,
            rows: self.rows,
            pictures: self.pictures,
            geometry: self.geometry,
            warnings,
            max_image_rows,
        })
    }
}
impl Write for Recorder {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        for &b in bytes {
            if !self.escape.is_empty() {
                self.escape.push(b);
                if self.escape.len() > 8192 {
                    return Err(limit());
                }
                let csi = self.escape.starts_with(b"\x1b[")
                    && self.escape.len() > 2
                    && (0x40..=0x7e).contains(&b);
                if csi || self.escape.ends_with(b"\x1b\\") {
                    self.command()?;
                }
            } else if b == 0x1b {
                self.text()?;
                self.escape.push(b);
            } else if b == b'\n' {
                self.text()?;
                self.advance(1)?;
                self.column = 0;
            } else if b == b'\r' {
                self.text()?;
                self.column = 0;
            } else {
                self.text_pending.push(b);
                if self.text_pending.len() > MAX_ROW_BYTES {
                    return Err(limit());
                }
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.text()
    }
}

pub(super) fn build(
    source: &str,
    options: &render::Options,
    cancel: Arc<AtomicBool>,
) -> io::Result<Page> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::ConnectionAborted.into());
    }
    let document = Document::parse(source.to_owned());
    let mut recorder = Recorder::new(options.terminal.unwrap_or_default(), cancel)?;
    let warnings =
        render::write_document_with(&mut recorder, &document, options, Recorder::anchor)?;
    recorder.finish(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_documents_spool_rows_and_preserve_anchors() {
        let source: String = (0..10_000)
            .map(|i| format!("Paragraph {i:05}.\n\n"))
            .collect();
        let options = render::Options {
            terminal: Some(Geometry::default()),
            ..Default::default()
        };
        let mut page = build(&source, &options, Arc::default()).unwrap();
        assert!(page.rows.len() >= 10_000);
        assert!((0..4).any(|i| page.row(i).unwrap().1.contains("Paragraph 00000")));
        assert!(
            page.rows
                .windows(2)
                .all(|pair| pair[0].anchor <= pair[1].anchor)
        );
        let top = page.rows.len() / 2;
        assert_eq!(page.restore(page.location(top)), top);
        assert!(
            (page.rows.len().saturating_sub(4)..page.rows.len()).any(|i| page
                .row(i)
                .unwrap()
                .1
                .contains("Paragraph 09999"))
        );
    }

    #[test]
    fn recorder_handles_fragmented_text_rewinds_and_cancellation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut recorder = Recorder::new(Geometry::default(), Arc::clone(&cancel)).unwrap();
        for byte in "\r\n\r\n\x1b[2A\r\x1b[38;5;5mcafé\x1b[0m\x1b[2B\r".bytes() {
            recorder.write_all(&[byte]).unwrap();
        }
        let mut page = recorder.finish(vec![]).unwrap();
        assert_eq!(page.row(0).unwrap().1, "café");
        assert!(
            page.row(0)
                .unwrap()
                .0
                .windows(9)
                .any(|s| s == b"\x1b[38;5;5m")
        );
        cancel.store(true, Ordering::Relaxed);
        let mut recorder = Recorder::new(Geometry::default(), cancel).unwrap();
        assert_eq!(
            recorder.write_all(b"x").unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
    }
}
