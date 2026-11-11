//! Common README HTML. A fragment of images, links, headings, paragraphs,
//! styles, code, line breaks, and rules becomes the events of the equivalent
//! Markdown, so `<h1>` reads like `#` and `<img>` like `![alt](src)`: a local
//! image renders and any other keeps its `[image: alt] (src)` reference.
//! Fragments with other markup keep their source. There is no DOM, CSS, script
//! engine or resource loader, and nothing is fetched.

use std::{collections::VecDeque, ops::Range};

use html5gum::{DefaultEmitter, StartTag, Token, Tokenizer};
use pulldown_cmark::{CowStr, Event, HeadingLevel, LinkType, Tag, TagEnd};

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
enum Element {
    Image,
    /// A `<picture>` alternative; its `<img>` is used instead.
    Source,
    Break,
    Rule,
    Link,
    Picture,
    /// `<p>` and `<div>` separate paragraphs.
    Block,
    Heading(HeadingLevel),
    /// Strong, emphasis, or strikethrough, named by the event that ends it.
    Style(TagEnd),
    Code,
}

/// Every element a converted fragment may contain.
const ELEMENTS: [(&str, Element); 21] = [
    ("img", Element::Image),
    ("source", Element::Source),
    ("br", Element::Break),
    ("hr", Element::Rule),
    ("a", Element::Link),
    ("picture", Element::Picture),
    ("p", Element::Block),
    ("div", Element::Block),
    ("h1", Element::Heading(HeadingLevel::H1)),
    ("h2", Element::Heading(HeadingLevel::H2)),
    ("h3", Element::Heading(HeadingLevel::H3)),
    ("h4", Element::Heading(HeadingLevel::H4)),
    ("h5", Element::Heading(HeadingLevel::H5)),
    ("h6", Element::Heading(HeadingLevel::H6)),
    ("b", Element::Style(TagEnd::Strong)),
    ("strong", Element::Style(TagEnd::Strong)),
    ("i", Element::Style(TagEnd::Emphasis)),
    ("em", Element::Style(TagEnd::Emphasis)),
    ("s", Element::Style(TagEnd::Strikethrough)),
    ("del", Element::Style(TagEnd::Strikethrough)),
    ("code", Element::Code),
];

fn element(name: &[u8]) -> Option<(&'static str, Element)> {
    ELEMENTS
        .iter()
        .find(|(known, _)| known.as_bytes().eq_ignore_ascii_case(name))
        .copied()
}

/// Whether some start tag names a supported element; other HTML skips the
/// tokenizer.
fn candidate(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(index, &byte)| {
        byte == b'<' && {
            let name = &bytes[index + 1..];
            let length = name
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric())
                .count();
            element(&name[..length]).is_some()
        }
    })
}

#[derive(Clone, Copy)]
struct Frame {
    name: &'static str,
    /// The event that closes this element, if it opened one.
    end: Option<TagEnd>,
}

struct Converter {
    mode: Mode,
    events: Vec<SpannedEvent<'static>>,
    frames: Vec<Frame>,
    paragraph: bool,
    heading: bool,
    /// The text of an open `<code>`, and where its start tag begins.
    code: Option<(String, usize)>,
    line_start: bool,
    space: bool,
    visible: bool,
}

impl Converter {
    fn push(&mut self, event: Event<'static>, source: Range<usize>) {
        self.events.push(SpannedEvent { event, source });
    }

    fn enter(&mut self, name: &'static str, end: Option<TagEnd>) -> Option<()> {
        if self.frames.len() >= MAX_DEPTH {
            return None;
        }
        self.frames.push(Frame { name, end });
        Some(())
    }

    /// Open links and styles keep their paragraph across `<p>` and `<div>`.
    fn inline(&self) -> bool {
        self.frames.iter().any(|frame| {
            frame
                .end
                .is_some_and(|end| !matches!(end, TagEnd::Heading(_)))
        })
    }

    /// Block content starts a paragraph lazily, so wrappers alone add no blank
    /// lines, and keeps a pending space between words.
    fn open(&mut self, source: &Range<usize>) {
        if self.mode == Mode::Block && !self.paragraph && !self.heading {
            self.push(Event::Start(Tag::Paragraph), source.start..source.start);
            self.paragraph = true;
        }
        if std::mem::take(&mut self.space) && !self.line_start {
            self.push(Event::Text(" ".into()), source.start..source.start);
        }
    }

    fn content(&mut self, source: &Range<usize>) {
        self.open(source);
        self.line_start = false;
        self.visible = true;
    }

    fn boundary(&mut self, offset: usize) {
        if self.inline() {
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
        let blank = |ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0c');
        if let Some((code, _)) = &mut self.code {
            for ch in value.chars() {
                if !blank(ch) {
                    code.push(ch);
                } else if !code.is_empty() && !code.ends_with(' ') {
                    code.push(' ');
                }
            }
            return;
        }
        let mut text = String::new();
        for ch in value.chars() {
            if blank(ch) {
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
            if let Some(end) = frame.end {
                self.heading &= !matches!(end, TagEnd::Heading(_));
                self.push(Event::End(end), offset..offset);
            }
        }
    }
}

fn convert(pieces: &[SpannedEvent<'_>], mode: Mode) -> Option<Vec<SpannedEvent<'static>>> {
    if !pieces
        .iter()
        .any(|piece| html(&piece.event).is_some_and(candidate))
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
        heading: false,
        code: None,
        line_start: mode == Mode::Block,
        space: false,
        visible: false,
    };
    // Headings and rules are blocks of their own, never inside a style or link.
    let block = |out: &Converter| mode == Mode::Block && !out.heading && !out.inline();
    for (count, token) in Tokenizer::new_with_emitter(&fragment.text, emitter).enumerate() {
        if count >= MAX_TOKENS {
            return None;
        }
        let Ok(token) = token;
        match token {
            Token::StartTag(tag) => {
                // Code holds text only.
                if out.code.is_some()
                    || tag
                        .attributes
                        .values()
                        .any(|value| value.value.len() > MAX_ATTRIBUTE)
                {
                    return None;
                }
                let source = fragment.source(tag.span.start..tag.span.end);
                let (name, element) = element(&tag.name)?;
                match element {
                    Element::Image => {
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
                    }
                    // A picture's alternatives are not chosen; its <img> is used.
                    Element::Source => {}
                    Element::Break if mode == Mode::Block => {
                        if out.paragraph || out.heading {
                            out.push(Event::HardBreak, source);
                        }
                        out.line_start = true;
                        out.space = false;
                    }
                    Element::Rule if block(&out) => {
                        out.boundary(source.start);
                        out.push(Event::Rule, source);
                        out.visible = true;
                    }
                    Element::Heading(level) if block(&out) => {
                        out.boundary(source.start);
                        out.push(
                            Event::Start(Tag::Heading {
                                level,
                                id: None,
                                classes: Vec::new(),
                                attrs: Vec::new(),
                            }),
                            source,
                        );
                        out.heading = true;
                        out.enter(name, Some(TagEnd::Heading(level)))?;
                    }
                    Element::Block if mode == Mode::Block && !out.heading => {
                        out.boundary(source.start);
                        out.enter(name, None)?;
                    }
                    Element::Picture => out.enter(name, None)?,
                    Element::Link => {
                        let mut end = None;
                        if let Some(href) = attribute(&tag, "href") {
                            out.open(&source);
                            out.push(
                                Event::Start(Tag::Link {
                                    link_type: LinkType::Inline,
                                    dest_url: href.trim_ascii().to_owned().into(),
                                    title: attribute(&tag, "title").unwrap_or_default().into(),
                                    id: "".into(),
                                }),
                                source,
                            );
                            end = Some(TagEnd::Link);
                        }
                        out.enter(name, end)?;
                    }
                    Element::Style(end) => {
                        out.open(&source);
                        let start = match end {
                            TagEnd::Strong => Tag::Strong,
                            TagEnd::Emphasis => Tag::Emphasis,
                            _ => Tag::Strikethrough,
                        };
                        out.push(Event::Start(start), source);
                        out.enter(name, Some(end))?;
                    }
                    Element::Code => out.code = Some((String::new(), source.start)),
                    _ => return None,
                }
            }
            Token::EndTag(tag) => {
                let source = fragment.source(tag.span.start..tag.span.end);
                let name = tag.name.as_slice();
                if let Some((code, start)) = out.code.take() {
                    if name != b"code" {
                        return None;
                    }
                    let code = code.trim_end();
                    if !code.is_empty() {
                        let source = start..source.end;
                        out.content(&source);
                        out.push(Event::Code(code.to_owned().into()), source);
                    }
                    continue;
                }
                match out.frames.iter().rposition(|f| f.name.as_bytes() == name) {
                    Some(index) => {
                        out.close(index, source.start);
                        if matches!(
                            element(name),
                            Some((_, Element::Block | Element::Heading(_)))
                        ) {
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
    if !out.visible || out.code.is_some() || (mode == Mode::Inline && !out.frames.is_empty()) {
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
