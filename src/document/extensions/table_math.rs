//! GFM splits rows before parsing inline math, even discarding excess cells.
//! Hide candidate math pipes while finding tables, including in their headers.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{Edit, MAX_EDITS, edit, math_edits, protected_ranges};
use crate::document::markdown_options;

pub(super) fn mask(source: &str) -> Option<String> {
    let mut masked = None;
    let mut end = 0;
    let mut previous = None;
    let mut in_rows = false;
    for raw in source.split_inclusive(['\r', '\n']) {
        let start = end;
        end += raw.len();
        if raw == "\n" && source[..start].ends_with('\r') {
            continue;
        }
        let line = raw.trim_ascii();
        if line.is_empty() {
            in_rows = false;
        } else if line.contains('|')
            && line.contains('-')
            && line
                .bytes()
                .all(|b| matches!(b, b'|' | b'-' | b':' | b'>' | b' ' | b'\t'))
        {
            if !in_rows && let Some((header, offset)) = previous {
                mask_row(source, header, offset, &mut masked);
            }
            in_rows = true;
        } else if in_rows {
            mask_row(source, raw, start, &mut masked);
        }
        previous = Some((raw, start));
    }
    masked.map(|bytes| String::from_utf8(bytes).expect("only ASCII pipes were replaced"))
}

fn mask_row(source: &str, raw: &str, start: usize, masked: &mut Option<Vec<u8>>) {
    if !raw.contains('|') {
        return;
    }
    // A nested list can indent a row by four or more spaces.
    let line = raw.trim_start_matches([' ', '\t']);
    let offset = start + raw.len() - line.len();
    let mut hide = |range: Range<usize>| {
        for (i, _) in line[range.clone()].match_indices('|') {
            let i = range.start + i;
            if !line[..i].ends_with('\\') {
                let bytes = masked.get_or_insert_with(|| source.as_bytes().to_vec());
                bytes[offset + i] = b'x';
            }
        }
    };
    if let Some((_, rest)) = line.split_once('$')
        && rest
            .rsplit_once('$')
            .is_some_and(|(body, _)| body.contains('|'))
    {
        for (event, range) in
            Parser::new_ext(line, markdown_options() - Options::ENABLE_TABLES).into_offset_iter()
        {
            if matches!(event, Event::InlineMath(_) | Event::DisplayMath(_)) {
                hide(range);
            }
        }
    }
    if line.contains("\\(") || line.contains("\\[") {
        let (protected, containers, visible) = protected_ranges(line, true);
        let mut pairs = Vec::new();
        math_edits(line, &containers, &protected, &visible, &mut pairs);
        for pair in pairs {
            hide(pair.source);
        }
    }
}

/// Pulldown removes a table escape before passing the pipe to the math engine.
/// Add it only where the parser confirms both a table cell and dollar math.
/// Backslash-delimited math already has its own source-preserving projection.
pub(super) fn escape_math_pipes(source: &str, masked: &str, edits: &mut Vec<Edit>) {
    let mut in_table = false;
    for (event, range) in Parser::new_ext(masked, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => in_table = true,
            Event::End(TagEnd::Table) => in_table = false,
            Event::InlineMath(_) | Event::DisplayMath(_) if in_table => {
                let original = &source[range.clone()];
                let probe = &masked[range.clone()];
                if original != probe {
                    if edits.len() == MAX_EDITS {
                        break;
                    }
                    let mut escaped = String::with_capacity(original.len());
                    for (ch, marked) in original.chars().zip(probe.chars()) {
                        if ch != marked {
                            escaped.push('\\');
                        }
                        escaped.push(ch);
                    }
                    edits.push(edit(range, escaped));
                }
            }
            _ => {}
        }
    }
}
