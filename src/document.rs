//! Parsing retains original Markdown alongside source ranges, independently of
//! terminal styling and image placement. Copying must never depend on pixels.

mod extensions;

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

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
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Markdown,
    Code(String),
    Image(PathBuf),
}

impl Document {
    pub fn parse(source: String) -> Self {
        let prepared = extensions::prepare(&source);
        Self {
            source,
            prepared,
            kind: Kind::Markdown,
        }
    }

    /// Standalone source remains literal code, without synthesizing Markdown fences.
    pub fn code(source: String, language: impl Into<String>) -> Self {
        Self {
            source,
            prepared: Default::default(),
            kind: Kind::Code(language.into()),
        }
    }

    /// A standalone image retains a native path, independently of Markdown URLs.
    pub fn image(path: PathBuf) -> Self {
        Self {
            source: path.to_string_lossy().into_owned(),
            prepared: Default::default(),
            kind: Kind::Image(path),
        }
    }

    pub(crate) fn image_path(&self) -> Option<&Path> {
        if let Kind::Image(path) = &self.kind {
            Some(path)
        } else {
            None
        }
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
        let mut parsed = matches!(self.kind, Kind::Markdown)
            .then(|| Parser::new_ext(input, options).into_offset_iter());
        let mut stage = 0;
        std::iter::from_fn(move || {
            if !matches!(self.kind, Kind::Markdown) {
                let event = match (&self.kind, stage) {
                    (Kind::Code(language), 0) => Event::Start(Tag::CodeBlock(
                        CodeBlockKind::Fenced(language.as_str().into()),
                    )),
                    (Kind::Code(_), 1) => Event::Text(self.source.as_str().into()),
                    (Kind::Code(_), 2) => Event::End(TagEnd::CodeBlock),
                    (Kind::Image(_), 0) => Event::Start(Tag::Image {
                        link_type: pulldown_cmark::LinkType::Inline,
                        dest_url: self.source.as_str().into(),
                        title: "".into(),
                        id: "".into(),
                    }),
                    (Kind::Image(path), 1) => Event::Text(
                        path.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("image")
                            .into(),
                    ),
                    (Kind::Image(_), 2) => Event::End(TagEnd::Image),
                    _ => return None,
                };
                stage += 1;
                return Some(SpannedEvent {
                    event,
                    source: 0..self.source.len(),
                });
            }
            let parsed = parsed.as_mut()?;
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
