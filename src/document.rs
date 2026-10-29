//! Parsing retains original Markdown alongside source ranges, independently of
//! terminal styling and image placement. Copying must never depend on pixels.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Debug)]
pub struct SpannedEvent<'a> {
    pub event: Event<'a>,
    pub source: Range<usize>,
}

#[derive(Debug)]
pub struct Document {
    source: String,
}

impl Document {
    pub fn parse(source: String) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Parse on iteration, borrowing unchanged text directly from the source.
    /// Repeated calls start fresh; events are not retained beside the layout.
    pub fn events(&self) -> impl Iterator<Item = SpannedEvent<'_>> {
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH;
        let mut parsed = Parser::new_ext(&self.source, options).into_offset_iter();
        std::iter::from_fn(move || {
            let (event, range) = parsed.next()?;
            // A fenced `math` block has the same rendering semantics as $$...$$,
            // but its source range still includes the original fence.
            if matches!(&event, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.split_whitespace().next() == Some("math"))
            {
                let mut tex = String::new();
                let mut end = range.end;
                for (part, part_range) in parsed.by_ref() {
                    end = part_range.end;
                    match part {
                        Event::Text(text) => tex.push_str(&text),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                Some(SpannedEvent {
                    event: Event::DisplayMath(tex.into()),
                    source: range.start..end,
                })
            } else {
                Some(SpannedEvent {
                    event,
                    source: range,
                })
            }
        })
    }

    /// Returns the exact Markdown for a source range, including delimiters.
    pub fn source_slice(&self, range: Range<usize>) -> Option<&str> {
        self.source.get(range)
    }
}
