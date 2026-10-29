//! Text export is generated from the source model, never from terminal output.

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
    let mut output = String::new();
    for block in layout::layout(document).blocks {
        match block {
            Block::Text(runs) => {
                for run in runs {
                    output.push_str(&sanitize(&run.text));
                }
            }
            Block::DisplayMath { source, .. } => {
                output.push_str(&sanitize(document.source_slice(source).unwrap_or_default()));
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push('\n');
            }
        }
    }
    output
}
