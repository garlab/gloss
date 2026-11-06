//! Bounded, terminal-independent syntax spans. No editor or display dependencies.

mod injections;
mod languages;

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, hash_map::Entry},
    ops::{ControlFlow, Range},
    time::{Duration, Instant},
};

use languages::{Compiled, Language};
use tree_sitter::{ParseOptions, Parser, QueryCursor, QueryCursorOptions, StreamingIterator as _};

/// Semantic token colors; terminal palettes are chosen by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlight {
    Keyword,
    Function,
    Type,
    String,
    Escape,
    Number,
    Constant,
    Comment,
    Property,
    Tag,
    Attribute,
    Label,
    Operator,
    Punctuation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureKind {
    Highlight(Highlight),
    Plain,
    Ignore,
}

impl CaptureKind {
    fn from_name(name: &str) -> Self {
        use Highlight::*;
        let mut parts = name.split('.');
        let first = parts.next().unwrap_or_default();
        let second = parts.next();
        let kind = match (first, second) {
            ("comment", _) => Comment,
            ("string", Some("escape" | "regexp" | "regex")) | ("escape", _) => Escape,
            ("string" | "character", _) => String,
            ("number" | "float", _) => Number,
            ("boolean" | "constant", _) => Constant,
            (
                "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass",
                _,
            ) => Keyword,
            ("variable", Some("builtin")) => Keyword,
            ("function" | "method" | "constructor", _) => Function,
            ("type" | "module" | "namespace", _) => Type,
            ("property" | "field", _) => Property,
            ("tag", _) => Tag,
            ("attribute", _) => Attribute,
            ("label", _) => Label,
            ("operator", _) => Operator,
            ("punctuation", _) => Punctuation,
            ("variable" | "embedded" | "none", _) => return Self::Plain,
            _ => return Self::Ignore,
        };
        Self::Highlight(kind)
    }
}

/// Oversized blocks remain complete, uncolored code.
pub const MAX_BLOCK_BYTES: usize = 128 * 1024;
const MAX_CAPTURES: usize = 16_384;
const MATCH_LIMIT: u32 = 1_024;
const BLOCK_TIME: Duration = Duration::from_millis(25);
const TOTAL_BYTES: usize = 8 * 1024 * 1024;
const TOTAL_TIME: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub highlight: Highlight,
}

/// One block, including embedded languages. Query compilation is bounded by the
/// fixed grammar set and invocation deadline; parse/query work shares one budget.
struct Budget {
    deadline: Instant,
    invocation_deadline: Instant,
    captures: usize,
}

impl Budget {
    fn new(start: Instant, remaining: Duration) -> Self {
        Self {
            deadline: start + BLOCK_TIME.min(remaining),
            invocation_deadline: start + remaining,
            captures: MAX_CAPTURES,
        }
    }

    fn compiled(&mut self, elapsed: Duration) {
        self.deadline = (self.deadline + elapsed).min(self.invocation_deadline);
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn consume(&mut self, count: usize) -> Option<()> {
        self.captures = self.captures.checked_sub(count)?;
        (!self.captures.is_multiple_of(64) || !self.expired()).then_some(())
    }
}

/// One rendering invocation. Only compiled queries and parser scratch space are
/// retained; syntax trees, captures, and token spans belong to a single block.
pub struct Highlighter {
    parser: Option<Parser>,
    queries: HashMap<Language, Option<Compiled>>,
    remaining_bytes: usize,
    remaining_time: Duration,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self {
            parser: None,
            queries: HashMap::new(),
            remaining_bytes: TOTAL_BYTES,
            remaining_time: TOTAL_TIME,
        }
    }
}

impl Highlighter {
    pub fn supports(label: &str) -> bool {
        Language::find(label).is_some()
    }

    /// Sorted, disjoint UTF-8 spans, or an uncolored fallback on any work limit.
    /// Incomplete snippets use Tree-sitter's error recovery where possible.
    pub fn highlight(&mut self, label: &str, text: &str) -> Option<Vec<Span>> {
        let language = Language::find(label)?;
        if text.is_empty()
            || text.len() > MAX_BLOCK_BYTES
            || text.len() > self.remaining_bytes
            || self.remaining_time.is_zero()
        {
            return None;
        }
        self.remaining_bytes -= text.len();
        let start = Instant::now();
        let result = self.block(language, text, start);
        self.remaining_time = self.remaining_time.saturating_sub(start.elapsed());
        result
    }

    fn block(&mut self, language: Language, text: &str, start: Instant) -> Option<Vec<Span>> {
        let mut budget = Budget::new(start, self.remaining_time);
        let (spans, injections) = self.part(language, text, &mut budget)?;
        if injections.is_empty() {
            return Some(spans);
        }
        let mut replacements = Vec::with_capacity(injections.len());
        for injection in injections {
            let (spans, nested) = self.part(
                injection.language,
                text.get(injection.range.clone())?,
                &mut budget,
            )?;
            // Only one embedded layer is supported. Disjoint ranges keep the
            // total embedded source length within the outer block's length.
            if !nested.is_empty() {
                return None;
            }
            replacements.push((injection.range, spans));
        }
        let spans = injections::overlay(spans, replacements);
        (!budget.expired()).then_some(spans)
    }

    fn part(
        &mut self,
        language: Language,
        text: &str,
        budget: &mut Budget,
    ) -> Option<(Vec<Span>, Vec<injections::Injection>)> {
        if budget.expired() {
            return None;
        }
        let compiled = match self.queries.entry(language) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let start = Instant::now();
                let compiled = language.compile();
                budget.compiled(start.elapsed());
                entry.insert(compiled)
            }
        }
        .as_ref()?;
        let deadline = budget.deadline;
        if budget.expired() {
            return None;
        }
        let parser = self.parser.get_or_insert_with(Parser::new);
        parser.reset();
        parser.set_language(&compiled.grammar).ok()?;
        let mut progress = |_: &tree_sitter::ParseState| {
            if Instant::now() >= deadline {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let tree = parser.parse_with_options(
            &mut |offset, _| &text.as_bytes()[offset.min(text.len())..],
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )?;
        if Instant::now() >= deadline {
            return None;
        }
        let mut cursor = QueryCursor::new();
        cursor.set_match_limit(MATCH_LIMIT);
        let mut captures = Vec::new();
        {
            let mut progress = |_: &tree_sitter::QueryCursorState| {
                if Instant::now() >= deadline {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            };
            let mut matches = cursor.captures_with_options(
                &compiled.query,
                tree.root_node(),
                text.as_bytes(),
                QueryCursorOptions::new().progress_callback(&mut progress),
            );
            while let Some((found, index)) = matches.next() {
                budget.consume(1)?;
                let capture = found.captures()[*index];
                let kind = compiled.kinds[capture.index as usize];
                let range = capture.node.byte_range();
                text.get(range.clone())?;
                if kind != CaptureKind::Ignore && !range.is_empty() {
                    captures.push(Capture {
                        range,
                        pattern: found.pattern_index,
                        kind,
                    });
                }
            }
        }
        if cursor.did_exceed_match_limit() || Instant::now() >= deadline {
            return None;
        }
        let injections = if let Some(query) = &compiled.injections {
            injections::collect(query, &tree, text, budget)?
        } else {
            Vec::new()
        };
        let spans = resolve(captures);
        (!budget.expired()).then_some((spans, injections))
    }
}

struct Capture {
    range: Range<usize>,
    pattern: usize,
    kind: CaptureKind,
}

/// Sweep capture boundaries, not input bytes. Inner nodes win; for identical
/// ranges, the earlier query pattern wins. Plain captures can clear a color.
/// Work is O(captures * log(captures)), with O(captures) scratch memory.
fn resolve(captures: Vec<Capture>) -> Vec<Span> {
    let mut boundaries = Vec::with_capacity(captures.len() * 2);
    for (index, capture) in captures.iter().enumerate() {
        boundaries.push((capture.range.start, true, index));
        boundaries.push((capture.range.end, false, index));
    }
    boundaries.sort_unstable();
    type Priority = (usize, Reverse<usize>, Reverse<usize>, Reverse<usize>);
    let mut active = BinaryHeap::<Priority>::new();
    let mut result: Vec<Span> = Vec::new();
    let mut previous = 0;
    for (position, entering, index) in boundaries {
        if position > previous
            && let Some(&(_, _, _, Reverse(winner))) = active.peek()
            && let CaptureKind::Highlight(highlight) = captures[winner].kind
        {
            match result.last_mut() {
                Some(span) if span.range.end == previous && span.highlight == highlight => {
                    span.range.end = position;
                }
                _ => result.push(Span {
                    range: previous..position,
                    highlight,
                }),
            }
        }
        if entering {
            let capture = &captures[index];
            active.push((
                capture.range.start,
                Reverse(capture.range.end),
                Reverse(capture.pattern),
                Reverse(index),
            ));
        }
        while active
            .peek()
            .is_some_and(|&(_, Reverse(end), _, _)| end <= position)
        {
            active.pop();
        }
        previous = position;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_at(spans: &[Span], offset: usize) -> Option<Highlight> {
        spans
            .iter()
            .find(|span| span.range.contains(&offset))
            .map(|span| span.highlight)
    }

    #[test]
    fn bundled_languages_compile_and_color_real_tokens() {
        for (label, text, token, expected) in [
            ("BASH", "echo \"hello\"\n", "\"hello\"", Highlight::String),
            (
                "go",
                "package main\nfunc main() {}\n",
                "func",
                Highlight::Keyword,
            ),
            (
                "jsx",
                "const view = <div />;\n",
                "const",
                Highlight::Keyword,
            ),
            ("json", "{\"value\": 42}\n", "42", Highlight::Number),
            (
                "py",
                "def hello():\n    return 42\n",
                "def",
                Highlight::Keyword,
            ),
            (
                "rs",
                "fn main() { let x = 42; }\n",
                "fn",
                Highlight::Keyword,
            ),
            ("toml", "value = 42\n", "42", Highlight::Number),
            (
                "ts",
                "const count: number = 42;\n",
                "const",
                Highlight::Keyword,
            ),
            (
                "tsx",
                "const view = <div />;\n",
                "const",
                Highlight::Keyword,
            ),
            ("yml", "value: 42\n", "42", Highlight::Number),
            ("dockerfile", "FROM scratch\n", "FROM", Highlight::Keyword),
            (
                "CONTAINERFILE",
                "from scratch\n",
                "from",
                Highlight::Keyword,
            ),
            ("docker", "CoPy . /app\n", "CoPy", Highlight::Keyword),
        ] {
            let spans = Highlighter::default()
                .highlight(label, text)
                .unwrap_or_else(|| panic!("{label}"));
            assert_eq!(
                color_at(&spans, text.find(token).unwrap()),
                Some(expected),
                "{label}: {spans:?}"
            );
            assert!(
                spans
                    .iter()
                    .all(|span| text.get(span.range.clone()).is_some())
            );
            assert!(
                spans
                    .windows(2)
                    .all(|pair| pair[0].range.end <= pair[1].range.start)
            );
        }
    }

    #[test]
    fn multiline_context_unicode_and_escapes_survive() {
        let text = "/* start\nstill a comment */\nfn café() { let s = \"中文\\n👩‍💻\"; }\n";
        let spans = Highlighter::default().highlight("rust", text).unwrap();
        assert_eq!(
            color_at(&spans, text.find("still").unwrap()),
            Some(Highlight::Comment)
        );
        assert_eq!(
            color_at(&spans, text.find("café").unwrap()),
            Some(Highlight::Function)
        );
        assert_eq!(
            color_at(&spans, text.find("中文").unwrap()),
            Some(Highlight::String)
        );
        assert_eq!(
            color_at(&spans, text.find("\\n").unwrap()),
            Some(Highlight::Escape)
        );
        assert!(
            spans
                .iter()
                .all(|span| text.get(span.range.clone()).is_some())
        );
    }

    #[test]
    fn dockerfile_colors_instructions_flags_shell_and_supported_heredocs() {
        let text = include_str!("../../tests/fixtures/Dockerfile");
        let spans = Highlighter::default()
            .highlight("dockerfile", text)
            .unwrap();
        for (token, expected) in [
            ("# syntax=", Highlight::Comment),
            ("FROM", Highlight::Keyword),
            ("from alpine", Highlight::Keyword),
            ("CoPy", Highlight::Keyword),
            ("${VERSION}", Highlight::Punctuation),
            ("VERSION}", Highlight::Constant),
            ("GREETING=", Highlight::Property),
            ("--chown=1000:1000", Highlight::Property),
            ("--from=build", Highlight::Property),
            ("8080", Highlight::Number),
            ("\"/bin/sh\"", Highlight::String),
            ("echo \"$GREETING\"", Highlight::Function),
            ("printf", Highlight::Function),
            ("if true", Highlight::Keyword),
            ("\"enabled\"", Highlight::String),
            ("true,", Highlight::Constant),
            ("true\nname:", Highlight::Constant),
            ("42", Highlight::Number),
            ("\"café\"", Highlight::String),
        ] {
            assert_eq!(
                color_at(&spans, text.find(token).unwrap()),
                Some(expected),
                "{token}"
            );
        }
        // Embedded syntax clears the parent heredoc's string color in whitespace,
        // while keeping its delimiters outside the embedded region.
        assert_eq!(color_at(&spans, text.find("    echo").unwrap()), None);
        assert_eq!(
            color_at(&spans, text.find("\nEOF").unwrap() + 1),
            Some(Highlight::Label)
        );
        assert!(
            spans
                .iter()
                .all(|span| text.get(span.range.clone()).is_some())
        );
        assert!(
            spans
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
    }

    #[test]
    fn dockerfile_heredocs_match_their_standalone_language_spans() {
        let source = include_str!("../../tests/fixtures/Dockerfile");
        let spans = Highlighter::default()
            .highlight("dockerfile", source)
            .unwrap();
        for (language, text) in [
            ("json", "{\"enabled\": true, \"count\": 2}\n"),
            ("yaml", "enabled: true\nname: \"café\"\n"),
            ("toml", "enabled = true\ncount = 42\n"),
            (
                "bash",
                "if true; then\n    echo \"hello from a heredoc\"\nfi\n",
            ),
        ] {
            let start = source.find(text).unwrap();
            let end = start + text.len();
            let actual: Vec<_> = spans
                .iter()
                .filter_map(|span| {
                    let left = span.range.start.max(start);
                    let right = span.range.end.min(end);
                    (left < right).then_some(Span {
                        range: left.saturating_sub(start)..right.saturating_sub(start),
                        highlight: span.highlight,
                    })
                })
                .collect();
            let expected = Highlighter::default().highlight(language, text).unwrap();
            assert_eq!(actual, expected, "{language}");
        }
    }

    #[test]
    fn dockerfile_embedded_documents_are_independent_and_unknown_languages_keep_parent_colors() {
        let text = "RUN echo \"unterminated\nRUN printf '%s' \"next\"\nCOPY <<EOF /app/file.xml\n<item/>\nEOF\n";
        let spans = Highlighter::default()
            .highlight("containerfile", text)
            .unwrap();
        assert_eq!(
            color_at(&spans, text.find("printf").unwrap()),
            Some(Highlight::Function)
        );
        assert_eq!(
            color_at(&spans, text.find("\"next\"").unwrap()),
            Some(Highlight::String)
        );
        assert_eq!(
            color_at(&spans, text.find("<item/>").unwrap()),
            Some(Highlight::String)
        );
    }

    #[test]
    fn dockerfile_injection_limits_fall_back_and_allow_later_blocks() {
        let mut engine = Highlighter::default();
        let text = "RUN echo hello\n".repeat(injections::MAX_INJECTIONS + 1);
        assert!(text.len() < MAX_BLOCK_BYTES);
        assert!(engine.highlight("dockerfile", &text).is_none());
        assert!(engine.highlight("dockerfile", "RUN echo hello\n").is_some());
        // A capture budget belongs to the entire outer block, not each RUN.
        let mut budget = Budget::new(Instant::now(), Duration::from_secs(1));
        let (_, embedded) = engine
            .part(Language::Dockerfile, "RUN echo hi\n", &mut budget)
            .unwrap();
        assert_eq!(embedded.len(), 1);
        budget.captures = 0;
        assert!(
            engine
                .part(Language::Bash, "echo hi", &mut budget)
                .is_none()
        );
    }

    #[test]
    fn unknown_and_oversized_blocks_do_not_initialize_resources() {
        let mut engine = Highlighter::default();
        assert!(engine.highlight("unknown", "fn main() {}").is_none());
        assert!(
            engine
                .highlight("rust", &"x".repeat(MAX_BLOCK_BYTES + 1))
                .is_none()
        );
        assert!(engine.parser.is_none() && engine.queries.is_empty());
        assert!(
            engine
                .highlight("dockerfile", &"#".repeat(MAX_BLOCK_BYTES + 1))
                .is_none()
        );
        assert!(engine.parser.is_none() && engine.queries.is_empty());
    }

    #[test]
    fn invocation_limits_keep_later_blocks_uncolored() {
        let mut engine = Highlighter {
            remaining_bytes: 3,
            ..Default::default()
        };
        assert!(engine.highlight("rust", "fn main() {}").is_none());
        assert!(engine.parser.is_none() && engine.queries.is_empty());
        engine.remaining_bytes = TOTAL_BYTES;
        engine.remaining_time = Duration::ZERO;
        assert!(engine.highlight("rust", "fn main() {}").is_none());
        assert!(engine.parser.is_none() && engine.queries.is_empty());
    }

    #[test]
    fn capture_heavy_input_falls_back_and_next_block_still_works() {
        let mut engine = Highlighter::default();
        let text = format!("[{}0]", "1,".repeat(MAX_CAPTURES));
        assert!(text.len() < MAX_BLOCK_BYTES);
        assert!(engine.highlight("json", &text).is_none());
        assert!(engine.highlight("json", "[42]").is_some());
    }

    #[test]
    fn overlapping_captures_respect_inner_plain_regions_and_pattern_order() {
        let colored = |range, pattern, color| Capture {
            range,
            pattern,
            kind: CaptureKind::Highlight(color),
        };
        let spans = resolve(vec![
            colored(0..12, 0, Highlight::String),
            Capture {
                range: 3..9,
                pattern: 0,
                kind: CaptureKind::Plain,
            },
            colored(5..7, 2, Highlight::Number),
            colored(5..7, 1, Highlight::Escape),
            colored(10..12, 0, Highlight::String),
        ]);
        assert_eq!(
            spans,
            vec![
                Span {
                    range: 0..3,
                    highlight: Highlight::String
                },
                Span {
                    range: 5..7,
                    highlight: Highlight::Escape
                },
                Span {
                    range: 9..12,
                    highlight: Highlight::String
                },
            ]
        );
    }
}
