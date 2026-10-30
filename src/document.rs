//! Parsing retains original Markdown alongside source ranges, independently of
//! terminal styling and image placement. Copying must never depend on pixels.

mod extensions;

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
    prepared: extensions::Prepared,
}

impl Document {
    pub fn parse(source: String) -> Self {
        let prepared = extensions::prepare(&source);
        Self { source, prepared }
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
            | Options::ENABLE_MATH
            | Options::ENABLE_GFM;
        let input = self.prepared.text.as_deref().unwrap_or(&self.source);
        let mut parsed = Parser::new_ext(input, options).into_offset_iter();
        std::iter::from_fn(move || {
            let (mut event, range) = parsed.next()?;
            let range = self.prepared.original_range(range);
            if matches!(event, Event::InlineMath(_) | Event::DisplayMath(_))
                && let Some(tex) = self.prepared.math(&range)
            {
                event = if matches!(event, Event::InlineMath(_)) {
                    Event::InlineMath(tex.into())
                } else {
                    Event::DisplayMath(tex.into())
                };
            }
            // A fenced `math` block has the same rendering semantics as $$...$$,
            // but its source range still includes the original fence.
            if matches!(&event, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.split_whitespace().next() == Some("math"))
            {
                let mut tex = String::new();
                let mut end = range.end;
                for (part, part_range) in parsed.by_ref() {
                    end = self.prepared.original_range(part_range).end;
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
    pub fn source_slice(&self, range: impl Into<Range<usize>>) -> Option<&str> {
        self.source.get(range.into())
    }
}
