//! Document flow and measured terminal lines. Runs retain source TeX independently
//! of image rendering; exports add no width wraps.

pub mod lines;
pub mod table;

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};

use crate::{document::Document, highlight::Highlight};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Body,
    Heading(u8),
    CodeBlock,
    ListMarker,
    QuoteMarker,
    LinkTarget,
    Rule,
    MathSource,
    TableHeader,
    TableBorder,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
    pub syntax: Option<Highlight>,
    pub role: Role,
    pub indent: usize,
    pub quote_depth: usize,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub style: Style,
    /// The equation without delimiters. `text` retains the original TeX for
    /// plain output and copying, even when the terminal shows an image.
    pub math: Option<String>,
    /// A source span, not a byte-for-byte mapping: entities and escapes decode.
    pub source: Option<Range<usize>>,
}

#[derive(Debug)]
pub enum Block {
    Text(Vec<Run>),
    Table(table::Table),
    Code {
        language: Option<String>,
        runs: Vec<Run>,
        source: Range<usize>,
    },
    DisplayMath {
        tex: String,
        source: Range<usize>,
        style: Style,
    },
}

#[derive(Debug)]
pub struct Layout {
    pub blocks: Vec<Block>,
}

pub fn layout(document: &Document) -> Layout {
    build_layout(document, true)
}

/// Plain export does not need independently owned code/table metadata or
/// per-block vectors. Keep its allocation behavior independent of decoration.
pub(crate) fn plain_layout(document: &Document) -> Layout {
    build_layout(document, false)
}

fn build_layout(document: &Document, semantic_blocks: bool) -> Layout {
    let mut flow = Flow::default();
    let mut style = Style::default();
    let mut styles = Vec::new();
    let mut links = Vec::new();
    let mut lists = Vec::new();
    let mut item_indents = Vec::new();
    let mut table: Option<table::Builder> = None;
    let mut events = document.events();
    while let Some(item) = events.next() {
        match &item.event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if !flow.prefix_pending {
                        flow.breaks(2);
                    }
                }
                Tag::Table(alignments) if semantic_blocks => {
                    if !flow.prefix_pending {
                        flow.breaks(2);
                    }
                    let prefix_start = if flow.prefix_pending {
                        flow.runs
                            .iter()
                            .rposition(|run| {
                                !matches!(run.style.role, Role::ListMarker | Role::QuoteMarker)
                            })
                            .map_or(0, |index| index + 1)
                    } else {
                        flow.runs.len()
                    };
                    let prefix = flow.runs.split_off(prefix_start);
                    flow.flush();
                    flow.trailing_newlines = 0;
                    flow.prefix_pending = false;
                    table = Some(table::Builder {
                        table: table::Table {
                            alignments: alignments.clone(),
                            rows: Vec::new(),
                            prefix,
                            style,
                            source: item.source.clone(),
                        },
                        row: table::Row::default(),
                        cell_source: item.source.clone(),
                    });
                }
                Tag::HtmlBlock | Tag::Table(_) => flow.breaks(2),
                Tag::CodeBlock(kind) if semantic_blocks => {
                    if !flow.prefix_pending {
                        flow.breaks(2);
                    }
                    // Keep pending list/quote markers attached to the code's
                    // first line when separating it from the surrounding text.
                    let prefix_start = if flow.prefix_pending {
                        flow.runs
                            .iter()
                            .rposition(|run| {
                                !matches!(run.style.role, Role::ListMarker | Role::QuoteMarker)
                            })
                            .map_or(0, |index| index + 1)
                    } else {
                        flow.runs.len()
                    };
                    let prefix = flow.runs.split_off(prefix_start);
                    flow.flush();
                    flow.runs = prefix;
                    let code_style = Style {
                        bold: false,
                        code: true,
                        role: Role::CodeBlock,
                        ..style
                    };
                    let mut end = item.source.end;
                    let mut has_text = false;
                    for part in events.by_ref() {
                        end = part.source.end;
                        match &part.event {
                            Event::Text(text) => {
                                has_text |= !text.is_empty();
                                flow.push(text, code_style, Some(part.source.clone()));
                            }
                            Event::End(TagEnd::CodeBlock) => break,
                            _ => {}
                        }
                    }
                    if has_text {
                        let language = match kind {
                            CodeBlockKind::Fenced(info) => {
                                info.split_whitespace().next().map(str::to_owned)
                            }
                            CodeBlockKind::Indented => None,
                        };
                        flow.blocks.push(Block::Code {
                            language,
                            runs: std::mem::take(&mut flow.runs),
                            source: item.source.start..end,
                        });
                    }
                    flow.breaks(2);
                }
                Tag::Heading { .. } | Tag::CodeBlock(_) => {
                    if !flow.prefix_pending {
                        flow.breaks(2);
                    }
                    styles.push(style);
                    style.bold = matches!(tag, Tag::Heading { .. });
                    style.code = matches!(tag, Tag::CodeBlock(_));
                    style.role = match tag {
                        Tag::Heading { level, .. } => Role::Heading(*level as u8),
                        _ => Role::CodeBlock,
                    };
                }
                Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {
                    styles.push(style);
                    match tag {
                        Tag::Emphasis => style.italic = true,
                        Tag::Strong => style.bold = true,
                        _ => style.strike = true,
                    }
                }
                Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                    styles.push(style);
                    style.link = true;
                    links.push(dest_url.to_string());
                    if matches!(tag, Tag::Image { .. }) {
                        flow.push("[image: ", style, None);
                    }
                }
                Tag::List(start) => {
                    flow.breaks(if lists.is_empty() { 2 } else { 1 });
                    lists.push(*start);
                }
                Tag::Item => {
                    flow.breaks(1);
                    let padding = "  ".repeat(lists.len().saturating_sub(1));
                    let marker = match lists.last_mut() {
                        Some(Some(next)) => {
                            let marker = format!("{next}. ");
                            *next = next.saturating_add(1);
                            marker
                        }
                        _ => "- ".into(),
                    };
                    item_indents.push(style.indent);
                    style.indent = padding.len() + marker.len();
                    flow.prefix(
                        &format!("{padding}{marker}"),
                        Style {
                            role: Role::ListMarker,
                            ..style
                        },
                    );
                }
                Tag::BlockQuote(_) => {
                    if !flow.prefix_pending {
                        flow.breaks(2);
                    }
                    styles.push(style);
                    style.quote_depth += 1;
                    flow.prefix(
                        "> ",
                        Style {
                            role: Role::QuoteMarker,
                            ..style
                        },
                    );
                }
                Tag::TableHead | Tag::TableRow if table.is_some() => {
                    table.as_mut().unwrap().row.header = matches!(tag, Tag::TableHead);
                }
                Tag::TableCell if table.is_some() => {
                    table.as_mut().unwrap().cell_source = item.source.clone();
                    flow.trailing_newlines = 0;
                }
                Tag::TableHead | Tag::TableRow => flow.breaks(1),
                Tag::FootnoteDefinition(label) => {
                    flow.breaks(2);
                    flow.prefix(&format!("[^{label}]: "), style);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Table if table.is_some() => {
                    let mut finished = table.take().unwrap().table;
                    finished.source.end = item.source.end;
                    flow.blocks.push(Block::Table(finished));
                    flow.trailing_newlines = 1;
                    flow.breaks(2);
                }
                TagEnd::Paragraph | TagEnd::HtmlBlock | TagEnd::Table => {
                    flow.breaks(2);
                }
                TagEnd::BlockQuote(_) => {
                    style = styles.pop().unwrap_or_default();
                    flow.breaks(2);
                }
                TagEnd::Heading(_) | TagEnd::CodeBlock => {
                    style = styles.pop().unwrap_or_default();
                    flow.breaks(2);
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    style = styles.pop().unwrap_or_default();
                }
                TagEnd::Link | TagEnd::Image => {
                    if *tag == TagEnd::Image {
                        flow.push("]", style, None);
                    }
                    if let Some(url) = links.pop() {
                        flow.push(
                            &format!(" ({url})"),
                            Style {
                                role: Role::LinkTarget,
                                link: false,
                                ..style
                            },
                            None,
                        );
                    }
                    style = styles.pop().unwrap_or_default();
                }
                TagEnd::List(_) => {
                    lists.pop();
                    flow.breaks(if lists.is_empty() { 2 } else { 1 });
                }
                TagEnd::Item => {
                    style.indent = item_indents.pop().unwrap_or_default();
                    flow.breaks(1);
                }
                TagEnd::TableHead | TagEnd::TableRow if table.is_some() => {
                    table.as_mut().unwrap().row();
                }
                TagEnd::TableCell if table.is_some() => {
                    table
                        .as_mut()
                        .unwrap()
                        .cell(std::mem::take(&mut flow.runs), item.source.end);
                    flow.trailing_newlines = 0;
                }
                TagEnd::TableHead | TagEnd::TableRow => flow.breaks(1),
                TagEnd::TableCell => flow.push(" | ", style, None),
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                flow.push(text, style, Some(item.source.clone()));
            }
            Event::Code(text) => {
                flow.push(
                    text,
                    Style {
                        code: true,
                        ..style
                    },
                    Some(item.source.clone()),
                );
            }
            Event::InlineMath(tex) | Event::DisplayMath(tex) if table.is_some() => {
                flow.push(
                    document
                        .source_slice(item.source.clone())
                        .unwrap_or_default(),
                    Style {
                        code: true,
                        ..style
                    },
                    Some(item.source.clone()),
                );
                if let Some(run) = flow.runs.last_mut() {
                    run.math = Some(tex.to_string());
                }
            }
            Event::InlineMath(tex) => {
                flow.push(
                    document
                        .source_slice(item.source.clone())
                        .unwrap_or_default(),
                    Style {
                        code: true,
                        ..style
                    },
                    Some(item.source.clone()),
                );
                if let Some(run) = flow.runs.last_mut() {
                    run.math = Some(tex.to_string());
                }
            }
            Event::DisplayMath(tex) => {
                flow.breaks(2);
                flow.flush();
                flow.blocks.push(Block::DisplayMath {
                    tex: tex.to_string(),
                    source: item.source.clone(),
                    style,
                });
                flow.trailing_newlines = 2;
            }
            Event::SoftBreak => flow.push(" ", style, None),
            Event::HardBreak => flow.breaks(1),
            Event::Rule => {
                flow.breaks(2);
                flow.push(
                    "---",
                    Style {
                        role: Role::Rule,
                        ..style
                    },
                    Some(item.source.clone()),
                );
                flow.breaks(2);
            }
            Event::TaskListMarker(checked) => {
                flow.push(if *checked { "[x] " } else { "[ ] " }, style, None);
            }
            Event::FootnoteReference(label) => {
                flow.push(&format!("[^{label}]"), style, Some(item.source.clone()));
            }
        }
    }
    flow.flush();
    Layout {
        blocks: flow.blocks,
    }
}

#[derive(Default)]
struct Flow {
    blocks: Vec<Block>,
    runs: Vec<Run>,
    trailing_newlines: usize,
    prefix_pending: bool,
}

impl Flow {
    fn push(&mut self, text: &str, style: Style, source: Option<Range<usize>>) {
        if text.is_empty() {
            return;
        }
        self.prefix_pending = false;
        let count = text.chars().rev().take_while(|&ch| ch == '\n').count();
        self.trailing_newlines = if count == text.len() {
            self.trailing_newlines + count
        } else {
            count
        };
        self.runs.push(Run {
            text: text.to_owned(),
            style,
            math: None,
            source,
        });
    }

    fn prefix(&mut self, text: &str, style: Style) {
        self.push(text, style, None);
        self.prefix_pending = true;
    }

    fn breaks(&mut self, count: usize) {
        if (!self.runs.is_empty() || !self.blocks.is_empty()) && self.trailing_newlines < count {
            self.push(
                &"\n".repeat(count - self.trailing_newlines),
                Style::default(),
                None,
            );
        }
    }

    fn flush(&mut self) {
        if !self.runs.is_empty() {
            self.blocks
                .push(Block::Text(std::mem::take(&mut self.runs)));
        }
    }
}
