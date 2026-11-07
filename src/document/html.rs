//! HTML image markup. A fragment holding `<img>` and only its usual wrappers
//! becomes the events of the equivalent Markdown, so a local image renders and
//! any other keeps its `[image: alt] (src)` reference, like `![alt](src)`.
//! Fragments with other markup keep their source. There is no DOM, CSS, script
//! engine or resource loader, and nothing is fetched.

use std::{collections::VecDeque, ops::Range};

use html5gum::{DefaultEmitter, StartTag, Token, Tokenizer};
use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};

use super::SpannedEvent;

const MAX_FRAGMENT: usize = 256 * 1024;
const MAX_TOKENS: usize = 32_768;
const MAX_ATTRIBUTE: usize = 16 * 1024;
const MAX_DEPTH: usize = 128;

/// Reads the rest of the HTML block or inline run that `first` starts, and
/// queues its events: converted when possible, otherwise unchanged. Returns the
/// event that ended an inline run, which still needs ordinary processing.
pub(super) fn read<'a>(
    first: SpannedEvent<'a>,
    mut pull: impl FnMut() -> Option<SpannedEvent<'a>>,
    output: &mut VecDeque<SpannedEvent<'a>>,
) -> Option<SpannedEvent<'a>> {
    if matches!(first.event, Event::Start(Tag::HtmlBlock)) {
        let mut pieces = Vec::new();
        let mut end = None;
        while let Some(item) = pull() {
            if matches!(item.event, Event::End(TagEnd::HtmlBlock)) {
                end = Some(item);
                break;
            }
            pieces.push(item);
        }
        output.push_back(first);
        match convert(&pieces, Mode::Block) {
            Some(events) => output.extend(events),
            None => output.extend(pieces),
        }
        output.extend(end);
        return None;
    }
    // Adjacent inline tags, separated only by whitespace, form one fragment,
    // such as `<a href="..."><img src="..."></a>`.
    let mut size = html(&first.event).map_or(0, str::len);
    let mut pieces = vec![first];
    let mut spaces = Vec::new();
    let mut next = None;
    while let Some(item) = pull() {
        let length = match &item.event {
            Event::InlineHtml(text) => text.len(),
            Event::SoftBreak => 1,
            Event::Text(text) if text.trim_ascii().is_empty() => text.len(),
            _ => 0,
        };
        if length == 0 || size + length > MAX_FRAGMENT {
            next = Some(item);
            break;
        }
        size += length;
        if matches!(item.event, Event::InlineHtml(_)) {
            pieces.append(&mut spaces);
            pieces.push(item);
        } else {
            spaces.push(item);
        }
    }
    match convert(&pieces, Mode::Inline) {
        Some(events) => output.extend(events),
        None => output.extend(pieces),
    }
    // Whitespace after the last tag is not part of the fragment.
    output.extend(spaces);
    next
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Block,
    Inline,
}

fn html<'b>(event: &'b Event<'_>) -> Option<&'b str> {
    match event {
        Event::Html(text) | Event::InlineHtml(text) | Event::Text(text) => Some(text),
        Event::SoftBreak => Some("\n"),
        _ => None,
    }
}

/// Fragment text and the source offset of each piece; quote and list markers
/// between lines are not part of the HTML.
struct Fragment {
    text: String,
    pieces: Vec<(usize, usize)>,
}

impl Fragment {
    fn new(pieces: &[SpannedEvent<'_>]) -> Option<Self> {
        let mut fragment = Self {
            text: String::new(),
            pieces: Vec::with_capacity(pieces.len()),
        };
        for piece in pieces {
            let text = html(&piece.event)?;
            if fragment.text.len() + text.len() > MAX_FRAGMENT {
                return None;
            }
            fragment
                .pieces
                .push((fragment.text.len(), piece.source.start));
            fragment.text.push_str(text);
        }
        Some(fragment)
    }

    fn offset(&self, offset: usize) -> usize {
        let index = self
            .pieces
            .partition_point(|&(start, _)| start <= offset)
            .saturating_sub(1);
        let (start, source) = self.pieces.get(index).copied().unwrap_or_default();
        source + offset - start
    }

    fn source(&self, range: Range<usize>) -> Range<usize> {
        let start = self.offset(range.start);
        if range.end <= range.start {
            return start..start;
        }
        start..self.offset(range.end - 1) + 1
    }
}

#[derive(Clone, Copy)]
struct Frame {
    name: &'static str,
    link: bool,
}

struct Converter {
    mode: Mode,
    events: Vec<SpannedEvent<'static>>,
    frames: Vec<Frame>,
    paragraph: bool,
    line_start: bool,
    space: bool,
    images: usize,
}

impl Converter {
    fn push(&mut self, event: Event<'static>, source: Range<usize>) {
        self.events.push(SpannedEvent { event, source });
    }

    fn links(&self) -> bool {
        self.frames.iter().any(|frame| frame.link)
    }

    /// Block text starts a paragraph lazily, so wrappers alone add no blank lines.
    fn content(&mut self, source: &Range<usize>) {
        if self.mode == Mode::Block && !self.paragraph {
            self.push(Event::Start(Tag::Paragraph), source.start..source.start);
            self.paragraph = true;
        }
        if std::mem::take(&mut self.space) && !self.line_start {
            self.push(Event::Text(" ".into()), source.start..source.start);
        }
        self.line_start = false;
    }

    fn boundary(&mut self, offset: usize) {
        if self.links() {
            return;
        }
        if std::mem::take(&mut self.paragraph) {
            self.push(Event::End(TagEnd::Paragraph), offset..offset);
        }
        self.line_start = true;
        self.space = false;
    }

    fn text(&mut self, value: &str, source: Range<usize>) {
        // HTML collapses ASCII whitespace; text inside an HTML block is not Markdown.
        let mut text = String::new();
        for ch in value.chars() {
            if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0c') {
                self.space |= !self.line_start || !text.is_empty();
            } else {
                if text.is_empty() {
                    self.content(&source);
                } else if std::mem::take(&mut self.space) {
                    text.push(' ');
                }
                text.push(ch);
            }
        }
        if !text.is_empty() {
            self.push(Event::Text(text.into()), source);
        }
    }

    fn close(&mut self, index: usize, offset: usize) {
        while self.frames.len() > index {
            let frame = self.frames.pop().unwrap();
            if frame.link {
                self.push(Event::End(TagEnd::Link), offset..offset);
            }
        }
    }
}

fn convert(pieces: &[SpannedEvent<'_>], mode: Mode) -> Option<Vec<SpannedEvent<'static>>> {
    // Only fragments with an image convert; other HTML skips the tokenizer.
    let image = |text: &str| {
        text.as_bytes()
            .windows(4)
            .any(|bytes| bytes[0] == b'<' && bytes[1..].eq_ignore_ascii_case(b"img"))
    };
    if !pieces
        .iter()
        .any(|piece| html(&piece.event).is_some_and(image))
    {
        return None;
    }
    let fragment = Fragment::new(pieces)?;
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    emitter.naively_switch_states(true);
    let mut out = Converter {
        mode,
        events: Vec::new(),
        frames: Vec::new(),
        paragraph: false,
        line_start: mode == Mode::Block,
        space: false,
        images: 0,
    };
    for (count, token) in Tokenizer::new_with_emitter(&fragment.text, emitter).enumerate() {
        if count >= MAX_TOKENS {
            return None;
        }
        let Ok(token) = token;
        match token {
            Token::StartTag(tag) => {
                if tag
                    .attributes
                    .values()
                    .any(|value| value.value.len() > MAX_ATTRIBUTE)
                {
                    return None;
                }
                let source = fragment.source(tag.span.start..tag.span.end);
                match tag.name.as_slice() {
                    b"img" => {
                        let target = attribute(&tag, "src")?;
                        let target = target.trim_ascii();
                        if target.is_empty() {
                            return None;
                        }
                        // Captions are one line, like Markdown alt text.
                        let alt = attribute(&tag, "alt").unwrap_or_default();
                        let alt = alt.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
                        let title = attribute(&tag, "title").unwrap_or_default();
                        out.content(&source);
                        out.push(
                            Event::Start(Tag::Image {
                                link_type: LinkType::Inline,
                                dest_url: CowStr::from(target.to_owned()),
                                title: title.into(),
                                id: "".into(),
                            }),
                            source.clone(),
                        );
                        if !alt.is_empty() {
                            out.push(Event::Text(alt.into()), source.clone());
                        }
                        out.push(Event::End(TagEnd::Image), source);
                        out.images += 1;
                    }
                    // A picture's alternatives are not chosen; its <img> is used.
                    b"source" => {}
                    b"br" if mode == Mode::Block => {
                        if out.paragraph {
                            out.push(Event::HardBreak, source);
                        }
                        out.line_start = true;
                        out.space = false;
                    }
                    name @ (b"a" | b"picture" | b"p" | b"div") => {
                        if out.frames.len() >= MAX_DEPTH {
                            return None;
                        }
                        let name = match name {
                            b"a" => "a",
                            b"picture" => "picture",
                            b"p" => "p",
                            _ => "div",
                        };
                        let mut link = false;
                        match name {
                            "a" => {
                                if let Some(href) = attribute(&tag, "href") {
                                    out.content(&source);
                                    out.push(
                                        Event::Start(Tag::Link {
                                            link_type: LinkType::Inline,
                                            dest_url: href.trim_ascii().to_owned().into(),
                                            title: attribute(&tag, "title")
                                                .unwrap_or_default()
                                                .into(),
                                            id: "".into(),
                                        }),
                                        source,
                                    );
                                    link = true;
                                }
                            }
                            "p" | "div" if mode == Mode::Block => out.boundary(source.start),
                            "picture" => {}
                            _ => return None,
                        }
                        out.frames.push(Frame { name, link });
                    }
                    _ => return None,
                }
            }
            Token::EndTag(tag) => {
                let source = fragment.source(tag.span.start..tag.span.end);
                let name = tag.name.as_slice();
                match out.frames.iter().rposition(|f| f.name.as_bytes() == name) {
                    Some(index) => {
                        out.close(index, source.start);
                        if matches!(name, b"p" | b"div") {
                            out.boundary(source.end);
                        }
                    }
                    // Void elements have no end; a wrapper may have opened in an
                    // earlier block, such as a centered <div>.
                    None if matches!(name, b"img" | b"source") => {}
                    None if mode == Mode::Block
                        && matches!(name, b"a" | b"picture" | b"p" | b"div") => {}
                    None => return None,
                }
            }
            Token::String(text) => {
                let value = std::str::from_utf8(&text.value).ok()?;
                out.text(value, fragment.source(text.span.start..text.span.end));
            }
            Token::Comment(_) => {}
            Token::Doctype(_) | Token::Error(_) => return None,
        }
    }
    // Inline wrappers must close within their run, before the surrounding Markdown.
    if out.images == 0 || (mode == Mode::Inline && !out.frames.is_empty()) {
        return None;
    }
    let end = pieces.last().map_or(0, |piece| piece.source.end);
    out.close(0, end);
    out.boundary(end);
    Some(out.events)
}

fn attribute(tag: &StartTag<usize>, name: &str) -> Option<String> {
    let value = &tag.attributes.get(name.as_bytes())?.value;
    Some(String::from_utf8_lossy(value).into_owned())
}
