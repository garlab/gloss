//! Text export is generated from the source model, never from terminal output.

use std::io::{self, Write};

use crate::{
    document::Document,
    layout::{self, Block},
    render::text::sanitize,
};

pub fn source(document: &Document) -> &str {
    document.source()
}

/// Readable, unstyled text with source TeX. Does not add terminal-width wrapping.
pub fn text(document: &Document) -> String {
    let mut output = Vec::new();
    write(&mut output, document).expect("writing a Vec cannot fail");
    String::from_utf8(output).expect("document text is UTF-8")
}

/// Stream completed plain blocks; exports need no retained terminal layout.
pub fn write(out: &mut impl Write, document: &Document) -> io::Result<()> {
    layout::visit(document, false, false, false, |block| {
        match block {
            Block::Text(runs) | Block::Code { runs, .. } => {
                for run in runs {
                    out.write_all(sanitize(&run.text).as_bytes())?;
                }
            }
            Block::DisplayMath { source, .. } => {
                let text = sanitize(document.source_slice(source).unwrap_or_default());
                out.write_all(text.as_bytes())?;
                if !text.ends_with('\n') {
                    writeln!(out)?;
                }
                writeln!(out)?;
            }
            Block::Table(_) | Block::Image(_) => {
                unreachable!("plain layout keeps tables and images as text runs")
            }
        }
        Ok(())
    })
}
