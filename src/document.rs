//! Parsing retains original Markdown alongside source ranges, independently of
//! terminal styling and image placement. Copying must never depend on pixels.

mod extensions;
mod html;
mod latex;

use std::{
    collections::VecDeque,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
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

enum Kind {
    Markdown,
    Latex(Latex),
    Code(String),
    Image(PathBuf),
}

/// A LaTeX document read with its included files.
struct Latex {
    index: Arc<latex_reader::Index>,
    /// The source with included files in place, when there were any. Event
    /// ranges refer to this text.
    expanded: Option<Arc<str>>,
    /// Included files that were read, for watching.
    files: Vec<PathBuf>,
    /// Directories from `\graphicspath`, relative to the image base.
    graphics: Vec<String>,
}

impl std::fmt::Debug for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Markdown => f.write_str("Markdown"),
            Kind::Latex(latex) => write!(
                f,
                "Latex({} checkpoints, {} included files)",
                latex.index.checkpoints().len(),
                latex.files.len()
            ),
            Kind::Code(language) => f.debug_tuple("Code").field(language).finish(),
            Kind::Image(path) => f.debug_tuple("Image").field(path).finish(),
        }
    }
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

    /// LaTeX is read once into an index of events, numbers, and macro scopes.
    /// Included files are not read; their commands stay visible as source.
    pub fn latex(source: String) -> Self {
        Self::latex_with(source, None, |_, _| None)
    }

    /// Reads LaTeX with the files that `\input` and similar commands name,
    /// relative to `base` as LaTeX resolves them. `jobname` locates the `.bbl`
    /// bibliography.
    pub fn latex_with_files(source: String, base: &Path, jobname: Option<&str>) -> Self {
        Self::latex_with(source, jobname, |name, kind| {
            crate::input::read_included(&base.join(name), kind == latex_reader::InputKind::Exists)
        })
        .with_base(base)
    }

    fn latex_with(
        source: String,
        jobname: Option<&str>,
        mut read: impl FnMut(&str, latex_reader::InputKind) -> Option<String>,
    ) -> Self {
        let limits = latex_reader::InputLimits {
            max_bytes: crate::input::MAX_SOURCE as usize,
            ..Default::default()
        };
        let expanded = latex_reader::expand(&source, jobname, &limits, &mut read);
        let files: Vec<PathBuf> = expanded.files.iter().map(PathBuf::from).collect();
        let expanded = match expanded.text {
            std::borrow::Cow::Borrowed(_) => None,
            std::borrow::Cow::Owned(text) => Some(Arc::<str>::from(text)),
        };
        let index = latex_reader::index(
            expanded.as_deref().unwrap_or(&source),
            &latex_reader::Options::default(),
            &AtomicBool::new(false),
        )
        .expect("indexing without a cancellation request completes");
        let graphics = index.graphics_path().to_vec();
        Self {
            source,
            prepared: Default::default(),
            kind: Kind::Latex(Latex {
                index: Arc::new(index),
                expanded,
                files,
                graphics,
            }),
        }
    }

    /// Makes included file paths relative to the working directory.
    fn with_base(mut self, base: &Path) -> Self {
        if let Kind::Latex(latex) = &mut self.kind {
            for file in &mut latex.files {
                *file = base.join(&*file);
            }
        }
        self
    }

    /// The index of a LaTeX document, for macro contexts at formula offsets.
    pub(crate) fn latex_index(&self) -> Option<&Arc<latex_reader::Index>> {
        if let Kind::Latex(latex) = &self.kind {
            Some(&latex.index)
        } else {
            None
        }
    }

    /// `\graphicspath` directories of a LaTeX document, or `None` for other
    /// documents, whose image targets are exact.
    pub(crate) fn graphics_path(&self) -> Option<&[String]> {
        if let Kind::Latex(latex) = &self.kind {
            Some(&latex.graphics)
        } else {
            None
        }
    }

    /// Files read besides the source, such as LaTeX `\input` files.
    pub fn dependencies(&self) -> &[PathBuf] {
        if let Kind::Latex(latex) = &self.kind {
            &latex.files
        } else {
            &[]
        }
    }

    /// The expanded text of LaTeX with included files, shared without copying.
    pub fn expanded_text(&self) -> Option<Arc<str>> {
        match &self.kind {
            Kind::Latex(latex) => latex.expanded.clone(),
            _ => None,
        }
    }

    /// The text that event ranges refer to: the source, or for LaTeX with
    /// included files, the expanded text.
    pub fn text(&self) -> &str {
        match &self.kind {
            Kind::Latex(Latex {
                expanded: Some(text),
                ..
            }) => text,
            _ => &self.source,
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
        let mut latex = match &self.kind {
            Kind::Latex(latex) => Some(latex::Adapter::new(&latex.index, self.text())),
            _ => None,
        };
        // Events read ahead with an HTML fragment.
        let mut queued = VecDeque::new();
        std::iter::from_fn(move || {
            if let Some(adapter) = latex.as_mut() {
                return adapter.next();
            }
            if let Some(item) = queued.pop_front() {
                return Some(item);
            }
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
            let (event, range) = parsed.next()?;
            let range = self.prepared.original_range(range);
            if matches!(event, Event::Start(Tag::HtmlBlock) | Event::InlineHtml(_)) {
                return self.html(event, range, parsed, &mut queued);
            }
            let event = self.math(event, &range);
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

    /// Restores TeX that a syntax projection replaced with a placeholder.
    #[inline]
    fn math<'a>(&'a self, event: Event<'a>, range: &Range<usize>) -> Event<'a> {
        if matches!(event, Event::InlineMath(_) | Event::DisplayMath(_))
            && let Some(tex) = self.prepared.math(range)
        {
            return if matches!(event, Event::InlineMath(_)) {
                Event::InlineMath(tex.into())
            } else {
                Event::DisplayMath(tex.into())
            };
        }
        event
    }

    /// Kept out of line, so documents without HTML take the same path as before.
    #[inline(never)]
    fn html<'a>(
        &'a self,
        event: Event<'a>,
        source: Range<usize>,
        parsed: &mut impl Iterator<Item = (Event<'a>, Range<usize>)>,
        queued: &mut VecDeque<SpannedEvent<'a>>,
    ) -> Option<SpannedEvent<'a>> {
        let pull = || {
            parsed.next().map(|(event, range)| SpannedEvent {
                event,
                source: self.prepared.original_range(range),
            })
        };
        // The event ending an inline run is inline content, never a code fence.
        if let Some(next) = html::read(SpannedEvent { event, source }, pull, queued) {
            queued.push_back(SpannedEvent {
                event: self.math(next.event, &next.source),
                source: next.source,
            });
        }
        queued.pop_front()
    }

    /// Returns the exact Markdown or LaTeX for a source range, including
    /// delimiters. LaTeX ranges index the text with included files in place.
    pub fn source_slice(&self, range: impl Into<Range<usize>>) -> Option<&str> {
        self.text().get(range.into())
    }
}
