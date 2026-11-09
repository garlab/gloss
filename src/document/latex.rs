//! LaTeX documents. `latex-preview` reads the source; this adapter maps its
//! events onto the CommonMark events the layout already renders, so equivalent
//! constructs look the same as in Markdown. No Markdown text is produced.

use std::{borrow::Cow, collections::VecDeque, ops::Range};

use latex_preview::{self as lp, Event as L, FigureKind, ListKind, Style, Tag as LT, TagKind as K};
use pulldown_cmark::{
    Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Tag, TagEnd,
};

use super::SpannedEvent;

fn cow<'a>(text: Cow<'a, str>) -> CowStr<'a> {
    match text {
        Cow::Borrowed(text) => CowStr::Borrowed(text),
        Cow::Owned(text) => CowStr::from(text),
    }
}

/// What a crate tag's end produces.
enum Open {
    /// Emit this end event; `true` when it closes inline content.
    End(TagEnd, bool),
    /// The tag has no CommonMark counterpart.
    Drop,
    /// A paragraph omitted so an item's first paragraph stays on its marker line.
    TightParagraph,
    Item {
        paragraphs: usize,
    },
    ItemLabel,
    Theorem {
        proof: bool,
    },
    Table {
        rows: usize,
    },
    Row,
    Cell {
        span: u16,
    },
    Unsupported,
    Capture,
}

enum CaptureKind {
    InlineCode,
    Monospace,
    Footnote(String),
    TheoremNote,
    FirstRow,
}

struct Capture<'a> {
    kind: CaptureKind,
    events: Vec<SpannedEvent<'a>>,
}

/// Text that begins the next paragraph, such as a theorem's head.
struct Prefix<'a> {
    events: Vec<SpannedEvent<'a>>,
    terminator: &'static str,
}

pub(super) struct Adapter<'a> {
    source: &'a str,
    events: lp::Events<'a>,
    queue: VecDeque<SpannedEvent<'a>>,
    stack: Vec<Open>,
    captures: Vec<Capture<'a>>,
    prefix: Option<Prefix<'a>>,
    footnotes: Vec<(String, Vec<SpannedEvent<'a>>, Range<usize>)>,
    /// A table's first row, held until a rule shows whether it is a header.
    first_row: Option<Vec<SpannedEvent<'a>>>,
    inline: usize,
    title: bool,
    chapters: bool,
    finished: bool,
}

impl<'a> Adapter<'a> {
    pub fn new(index: &'a lp::Index, source: &'a str) -> Self {
        Self {
            source,
            events: index.events(source),
            queue: VecDeque::new(),
            stack: Vec::new(),
            captures: Vec::new(),
            prefix: None,
            footnotes: Vec::new(),
            first_row: None,
            inline: 0,
            title: index.has_title(),
            chapters: index.has_chapters(),
            finished: false,
        }
    }

    pub fn next(&mut self) -> Option<SpannedEvent<'a>> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Some(event);
            }
            if self.finished {
                return None;
            }
            match self.events.next() {
                Some(spanned) => self.translate(spanned.event, spanned.range),
                None => self.finish(),
            }
        }
    }

    fn out(&mut self, event: Event<'a>, source: Range<usize>) {
        let spanned = SpannedEvent { event, source };
        match self.captures.last_mut() {
            Some(capture) => capture.events.push(spanned),
            None => self.queue.push_back(spanned),
        }
    }

    fn out_all(&mut self, events: Vec<SpannedEvent<'a>>) {
        for spanned in events {
            self.out(spanned.event, spanned.source);
        }
    }

    fn text(&mut self, text: impl Into<CowStr<'a>>, range: &Range<usize>) {
        self.out(Event::Text(text.into()), range.clone());
    }

    fn strong(&mut self, text: String, range: &Range<usize>) {
        self.out(Event::Start(Tag::Strong), range.clone());
        self.text(text, range);
        self.out(Event::End(TagEnd::Strong), range.clone());
    }

    fn heading_level(&self, level: u8) -> HeadingLevel {
        let base = if self.chapters { 1 } else { 2 };
        let first = if self.title { 2 } else { 1 };
        let level = if level < base {
            1
        } else {
            first + usize::from(level - base)
        };
        HeadingLevel::try_from(level.min(6)).unwrap_or(HeadingLevel::H6)
    }

    /// A theorem or bibliography head waiting for a paragraph, written as its
    /// own paragraph before any other block.
    fn flush_prefix(&mut self, range: &Range<usize>) {
        if let Some(prefix) = self.prefix.take() {
            self.out(Event::Start(Tag::Paragraph), range.clone());
            self.out_all(prefix.events);
            self.text(prefix.terminator.trim_end(), range);
            self.out(Event::End(TagEnd::Paragraph), range.clone());
        }
    }

    fn begin_inline_block(&mut self, range: &Range<usize>) {
        if self.inline == 0 {
            self.flush_prefix(range);
        }
    }

    fn translate(&mut self, event: L<'a>, range: Range<usize>) {
        // A held first row becomes the header when a rule follows it.
        if let Some(row) = self.first_row.take() {
            let header = matches!(event, L::TableRule);
            let (start, end) = if header {
                (Tag::TableHead, TagEnd::TableHead)
            } else {
                (Tag::TableRow, TagEnd::TableRow)
            };
            self.out(Event::Start(start), range.clone());
            self.out_all(row);
            self.out(Event::End(end), range.clone());
            if header {
                return;
            }
        }
        match event {
            L::Start(tag) => self.start(tag, range),
            L::End(kind) => self.end(kind, range),
            L::Text(text) => self.text(cow(text), &range),
            L::Math(math) => {
                let tex = cow(math.prepared);
                let event = if math.display {
                    Event::DisplayMath(tex)
                } else {
                    Event::InlineMath(tex)
                };
                self.out(event, range);
            }
            L::Reference(reference) => self.text(reference.display(), &range),
            L::Citation(citation) => self.text(citation.display(), &range),
            L::Image(image) => {
                let wrap = self.inline == 0;
                if wrap {
                    self.flush_prefix(&range);
                    self.out(Event::Start(Tag::Paragraph), range.clone());
                }
                let label = image.path.rsplit('/').next().unwrap_or_default().to_owned();
                self.out(
                    Event::Start(Tag::Image {
                        link_type: LinkType::Inline,
                        dest_url: cow(image.path),
                        title: "".into(),
                        id: "".into(),
                    }),
                    range.clone(),
                );
                self.text(label, &range);
                self.out(Event::End(TagEnd::Image), range.clone());
                if wrap {
                    self.out(Event::End(TagEnd::Paragraph), range);
                }
            }
            L::Include(include) => {
                // A file that could not be read: show the command as written.
                let command = match self.source.get(range.clone()) {
                    Some(command) => command.trim_end().to_owned(),
                    None => format!("\\input{{{}}}", include.path),
                };
                self.raw(command, range);
            }
            L::Raw(text) => self.raw(text.into_owned(), range),
            L::LineBreak => self.out(Event::HardBreak, range),
            L::Hint(_) | L::TableRule => {}
        }
    }

    /// Source shown as is: a code span inline, otherwise a code block.
    fn raw(&mut self, text: String, range: Range<usize>) {
        if self.inline > 0 {
            self.out(Event::Code(text.into()), range);
        } else {
            self.flush_prefix(&range);
            self.out(
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced("".into()))),
                range.clone(),
            );
            let text = if text.ends_with('\n') {
                text
            } else {
                text + "\n"
            };
            self.text(text, &range);
            self.out(Event::End(TagEnd::CodeBlock), range);
        }
    }

    fn capture(&mut self, kind: CaptureKind) {
        self.captures.push(Capture {
            kind,
            events: Vec::new(),
        });
        self.stack.push(Open::Capture);
    }

    fn start(&mut self, tag: LT<'a>, range: Range<usize>) {
        let open = match tag {
            LT::TitleBlock | LT::Align(_) | LT::Figure(_) => Open::Drop,
            LT::Title => {
                self.out(Event::Start(heading(HeadingLevel::H1)), range.clone());
                self.inline += 1;
                Open::End(TagEnd::Heading(HeadingLevel::H1), true)
            }
            LT::Author | LT::Date => {
                self.out(Event::Start(Tag::Paragraph), range.clone());
                self.inline += 1;
                Open::End(TagEnd::Paragraph, true)
            }
            LT::Abstract => {
                self.flush_prefix(&range);
                self.out(Event::Start(Tag::BlockQuote(None)), range.clone());
                self.out(Event::Start(Tag::Paragraph), range.clone());
                self.strong("Abstract".into(), &range);
                self.out(Event::End(TagEnd::Paragraph), range.clone());
                Open::End(TagEnd::BlockQuote(None), false)
            }
            LT::Heading { level, number } => {
                let level = self.heading_level(level);
                self.out(Event::Start(heading(level)), range.clone());
                if let Some(number) = number {
                    self.text(format!("{number} "), &range);
                }
                self.inline += 1;
                Open::End(TagEnd::Heading(level), true)
            }
            LT::Paragraph { .. } => {
                self.inline += 1;
                let tight = match self.stack.last_mut() {
                    Some(Open::Item { paragraphs }) => {
                        *paragraphs += 1;
                        *paragraphs == 1
                    }
                    _ => false,
                };
                if !tight {
                    self.out(Event::Start(Tag::Paragraph), range.clone());
                }
                if let Some(prefix) = self.prefix.take() {
                    self.out_all(prefix.events);
                    self.text(prefix.terminator, &range);
                }
                if tight {
                    Open::TightParagraph
                } else {
                    Open::End(TagEnd::Paragraph, true)
                }
            }
            LT::List(kind) => {
                self.begin_inline_block(&range);
                let ordered = kind == ListKind::Enumerate;
                self.out(Event::Start(Tag::List(ordered.then_some(1))), range.clone());
                Open::End(TagEnd::List(ordered), false)
            }
            LT::Item { .. } => {
                self.out(Event::Start(Tag::Item), range.clone());
                Open::Item { paragraphs: 0 }
            }
            LT::ItemLabel => {
                self.out(Event::Start(Tag::Strong), range.clone());
                Open::ItemLabel
            }
            LT::Quote(_) => {
                self.begin_inline_block(&range);
                self.out(Event::Start(Tag::BlockQuote(None)), range.clone());
                Open::End(TagEnd::BlockQuote(None), false)
            }
            LT::Code { language } => {
                self.begin_inline_block(&range);
                let language = language.map_or(CowStr::Borrowed(""), cow);
                self.out(
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language))),
                    range.clone(),
                );
                Open::End(TagEnd::CodeBlock, false)
            }
            LT::InlineCode => {
                self.capture(CaptureKind::InlineCode);
                return;
            }
            LT::Theorem { name, number, .. } => {
                self.flush_prefix(&range);
                let head = match number {
                    Some(number) => format!("{name} {number}"),
                    None => name,
                };
                self.prefix = Some(Prefix {
                    events: strong_events(head, &range),
                    terminator: ". ",
                });
                Open::Theorem { proof: false }
            }
            LT::Proof => {
                self.flush_prefix(&range);
                self.prefix = Some(Prefix {
                    events: vec![
                        SpannedEvent {
                            event: Event::Start(Tag::Emphasis),
                            source: range.clone(),
                        },
                        SpannedEvent {
                            event: Event::Text("Proof".into()),
                            source: range.clone(),
                        },
                        SpannedEvent {
                            event: Event::End(TagEnd::Emphasis),
                            source: range.clone(),
                        },
                    ],
                    terminator: ". ",
                });
                Open::Theorem { proof: true }
            }
            LT::TheoremNote => {
                self.capture(CaptureKind::TheoremNote);
                return;
            }
            LT::Caption { number, kind } => {
                self.flush_prefix(&range);
                self.out(Event::Start(Tag::Paragraph), range.clone());
                let name = match kind {
                    FigureKind::Figure => "Figure",
                    FigureKind::Table => "Table",
                    FigureKind::Algorithm => "Algorithm",
                };
                if let Some(number) = number {
                    self.strong(format!("{name} {number}:"), &range);
                    self.text(" ", &range);
                }
                self.inline += 1;
                Open::End(TagEnd::Paragraph, true)
            }
            LT::Table { columns } => {
                self.begin_inline_block(&range);
                let alignments = columns
                    .iter()
                    .map(|column| match column.align {
                        lp::Align::Center => Alignment::Center,
                        lp::Align::Right => Alignment::Right,
                        lp::Align::Left | lp::Align::Justify => Alignment::Left,
                    })
                    .collect();
                self.out(Event::Start(Tag::Table(alignments)), range.clone());
                Open::Table { rows: 0 }
            }
            LT::Row => {
                let first = match self.stack.last_mut() {
                    Some(Open::Table { rows }) => {
                        *rows += 1;
                        *rows == 1
                    }
                    _ => false,
                };
                if first {
                    self.captures.push(Capture {
                        kind: CaptureKind::FirstRow,
                        events: Vec::new(),
                    });
                } else {
                    self.out(Event::Start(Tag::TableRow), range.clone());
                }
                Open::Row
            }
            LT::Cell { span, .. } => {
                self.out(Event::Start(Tag::TableCell), range.clone());
                self.inline += 1;
                Open::Cell { span }
            }
            LT::Style(style) => match style {
                Style::Emphasis | Style::Italic | Style::Slanted => {
                    self.out(Event::Start(Tag::Emphasis), range.clone());
                    Open::End(TagEnd::Emphasis, false)
                }
                Style::Bold => {
                    self.out(Event::Start(Tag::Strong), range.clone());
                    Open::End(TagEnd::Strong, false)
                }
                Style::Strike => {
                    self.out(Event::Start(Tag::Strikethrough), range.clone());
                    Open::End(TagEnd::Strikethrough, false)
                }
                Style::Monospace => {
                    self.capture(CaptureKind::Monospace);
                    return;
                }
                _ => Open::Drop,
            },
            LT::Link { url } => {
                self.out(
                    Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url: cow(url),
                        title: "".into(),
                        id: "".into(),
                    }),
                    range.clone(),
                );
                Open::End(TagEnd::Link, false)
            }
            LT::Footnote { number } => {
                let label = number.unwrap_or_else(|| "*".into());
                self.out(
                    Event::FootnoteReference(label.clone().into()),
                    range.clone(),
                );
                self.capture(CaptureKind::Footnote(label));
                return;
            }
            LT::Bibliography => {
                self.flush_prefix(&range);
                let level = self.heading_level(if self.chapters { 1 } else { 2 });
                self.out(Event::Start(heading(level)), range.clone());
                self.text("References", &range);
                self.out(Event::End(TagEnd::Heading(level)), range.clone());
                self.out(Event::Start(Tag::List(None)), range.clone());
                Open::End(TagEnd::List(false), false)
            }
            LT::BibItem { label } => {
                self.out(Event::Start(Tag::Item), range.clone());
                self.prefix = Some(Prefix {
                    events: vec![SpannedEvent {
                        event: Event::Text(label.into()),
                        source: range.clone(),
                    }],
                    terminator: " ",
                });
                Open::Item { paragraphs: 0 }
            }
            LT::Unsupported { .. } => Open::Unsupported,
        };
        self.stack.push(open);
    }

    fn end(&mut self, _kind: K, range: Range<usize>) {
        let Some(open) = self.stack.pop() else {
            return;
        };
        match open {
            Open::End(end, inline) => {
                if inline {
                    self.inline = self.inline.saturating_sub(1);
                }
                self.out(Event::End(end), range);
            }
            Open::Drop | Open::Unsupported => {}
            Open::TightParagraph => self.inline = self.inline.saturating_sub(1),
            Open::Item { .. } => {
                if let Some(prefix) = self.prefix.take() {
                    self.out_all(prefix.events);
                }
                self.out(Event::End(TagEnd::Item), range);
            }
            Open::ItemLabel => {
                self.out(Event::End(TagEnd::Strong), range.clone());
                self.text(" ", &range);
            }
            Open::Theorem { proof } => {
                self.flush_prefix(&range);
                if proof {
                    self.out(Event::Start(Tag::Paragraph), range.clone());
                    self.text("∎", &range);
                    self.out(Event::End(TagEnd::Paragraph), range);
                }
            }
            Open::Table { .. } => self.out(Event::End(TagEnd::Table), range),
            Open::Row => {
                if let Some(Capture {
                    kind: CaptureKind::FirstRow,
                    ..
                }) = self.captures.last()
                {
                    let capture = self.captures.pop().unwrap();
                    self.first_row = Some(capture.events);
                } else {
                    self.out(Event::End(TagEnd::TableRow), range);
                }
            }
            Open::Cell { span } => {
                self.inline = self.inline.saturating_sub(1);
                self.out(Event::End(TagEnd::TableCell), range.clone());
                for _ in 1..span {
                    self.out(Event::Start(Tag::TableCell), range.clone());
                    self.out(Event::End(TagEnd::TableCell), range.clone());
                }
            }
            Open::Capture => {
                let Some(capture) = self.captures.pop() else {
                    return;
                };
                match capture.kind {
                    CaptureKind::InlineCode => {
                        let text = plain(&capture.events);
                        self.out(Event::Code(text.into()), range);
                    }
                    CaptureKind::Monospace => {
                        if capture
                            .events
                            .iter()
                            .all(|e| matches!(e.event, Event::Text(_)))
                        {
                            let text = plain(&capture.events);
                            self.out(Event::Code(text.into()), range);
                        } else {
                            self.out_all(capture.events);
                        }
                    }
                    CaptureKind::Footnote(label) => {
                        self.footnotes.push((label, capture.events, range));
                    }
                    CaptureKind::TheoremNote => {
                        if let Some(prefix) = &mut self.prefix {
                            prefix.events.push(SpannedEvent {
                                event: Event::Text(" (".into()),
                                source: range.clone(),
                            });
                            prefix.events.extend(capture.events);
                            prefix.events.push(SpannedEvent {
                                event: Event::Text(")".into()),
                                source: range,
                            });
                        }
                    }
                    CaptureKind::FirstRow => self.first_row = Some(capture.events),
                }
            }
        }
    }

    fn finish(&mut self) {
        self.finished = true;
        for (label, events, range) in std::mem::take(&mut self.footnotes) {
            self.out(
                Event::Start(Tag::FootnoteDefinition(label.into())),
                range.clone(),
            );
            self.out(Event::Start(Tag::Paragraph), range.clone());
            self.out_all(events);
            self.out(Event::End(TagEnd::Paragraph), range.clone());
            self.out(Event::End(TagEnd::FootnoteDefinition), range);
        }
    }
}

fn heading<'a>(level: HeadingLevel) -> Tag<'a> {
    Tag::Heading {
        level,
        id: None,
        classes: Vec::new(),
        attrs: Vec::new(),
    }
}

fn strong_events<'a>(text: String, range: &Range<usize>) -> Vec<SpannedEvent<'a>> {
    vec![
        SpannedEvent {
            event: Event::Start(Tag::Strong),
            source: range.clone(),
        },
        SpannedEvent {
            event: Event::Text(text.into()),
            source: range.clone(),
        },
        SpannedEvent {
            event: Event::End(TagEnd::Strong),
            source: range.clone(),
        },
    ]
}

fn plain(events: &[SpannedEvent<'_>]) -> String {
    events
        .iter()
        .filter_map(|e| match &e.event {
            Event::Text(text) | Event::Code(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect()
}
