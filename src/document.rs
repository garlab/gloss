//! Parsing retains original Markdown alongside source ranges, independently of
//! terminal styling and image placement. Copying must never depend on pixels.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Debug)]
pub struct SpannedEvent {
    pub event: Event<'static>,
    pub source: Range<usize>,
}

#[derive(Debug)]
pub struct Document {
    source: String,
    events: Vec<SpannedEvent>,
}

impl Document {
    pub fn parse(source: String) -> Self {
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH;
        let mut parsed = Parser::new_ext(&source, options).into_offset_iter();
        let mut events = Vec::new();
        while let Some((event, range)) = parsed.next() {
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
                events.push(SpannedEvent {
                    event: Event::DisplayMath(tex.into()),
                    source: range.start..end,
                });
            } else {
                events.push(SpannedEvent {
                    event: event.into_static(),
                    source: range,
                });
            }
        }
        Self { source, events }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn events(&self) -> &[SpannedEvent] {
        &self.events
    }

    /// Returns the exact Markdown for a source range, including delimiters.
    pub fn source_slice(&self, range: Range<usize>) -> Option<&str> {
        self.source.get(range)
    }
}
