//! One bounded layer of embedded syntax, using the grammar's bundled queries.

use std::ops::{ControlFlow, Range};
use tree_sitter::{Query, QueryCursor, QueryCursorOptions, StreamingIterator as _, Tree};

use super::{Budget, MATCH_LIMIT, Span, languages::Language};

pub(super) const MAX_INJECTIONS: usize = 128;

pub(super) struct Injection {
    pub range: Range<usize>,
    pub language: Language,
}

pub(super) fn collect(
    query: &Query,
    tree: &Tree,
    text: &str,
    budget: &mut Budget,
) -> Option<Vec<Injection>> {
    let content = query.capture_index_for_name("injection.content")?;
    let deadline = budget.deadline;
    let mut cursor = QueryCursor::new();
    cursor.set_match_limit(MATCH_LIMIT);
    let mut result = Vec::new();
    {
        let mut progress = |_: &tree_sitter::QueryCursorState| {
            if std::time::Instant::now() >= deadline {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let mut matches = cursor.matches_with_options(
            query,
            tree.root_node(),
            text.as_bytes(),
            QueryCursorOptions::new().progress_callback(&mut progress),
        );
        while let Some(found) = matches.next() {
            budget.consume(found.captures().len())?;
            if budget.expired() {
                return None;
            }
            let language = query
                .property_settings(found.pattern_index)
                .iter()
                .find(|property| property.key.as_ref() == "injection.language")
                .and_then(|property| property.value.as_deref())
                .and_then(Language::find)
                .filter(|language| {
                    matches!(
                        language,
                        Language::Bash | Language::Json | Language::Yaml | Language::Toml
                    )
                });
            let Some(language) = language else {
                // Unsupported embedded languages retain the parent syntax.
                continue;
            };
            for capture in found
                .captures()
                .iter()
                .filter(|capture| capture.index == content)
            {
                let range = capture.node.byte_range();
                if range.is_empty() {
                    continue;
                }
                text.get(range.clone())?;
                if result.len() == MAX_INJECTIONS {
                    return None;
                }
                result.push(Injection { range, language });
            }
        }
    }
    if cursor.did_exceed_match_limit() || budget.expired() {
        return None;
    }
    result.sort_unstable_by_key(|injection| (injection.range.start, injection.range.end));
    result.dedup_by(|a, b| a.range == b.range && a.language == b.language);
    if result
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return None;
    }
    Some(result)
}

/// Embedded spans replace the parent's colors throughout their source region,
/// including plain text. Merge sorted, disjoint regions without per-byte storage.
pub(super) fn overlay(
    parent: Vec<Span>,
    replacements: Vec<(Range<usize>, Vec<Span>)>,
) -> Vec<Span> {
    let mut result = Vec::with_capacity(parent.len());
    let mut parent = parent.into_iter().peekable();
    for (range, embedded) in replacements {
        while let Some(span) = parent.peek_mut() {
            if span.range.start >= range.end {
                break;
            }
            if span.range.start < range.start {
                append(
                    &mut result,
                    Span {
                        range: span.range.start..span.range.end.min(range.start),
                        highlight: span.highlight,
                    },
                );
            }
            if span.range.end > range.end {
                span.range.start = range.end;
                break;
            }
            parent.next();
        }
        for mut span in embedded {
            span.range.start += range.start;
            span.range.end += range.start;
            append(&mut result, span);
        }
    }
    for span in parent {
        append(&mut result, span);
    }
    result
}

fn append(spans: &mut Vec<Span>, span: Span) {
    if let Some(last) = spans.last_mut()
        && last.range.end == span.range.start
        && last.highlight == span.highlight
    {
        last.range.end = span.range.end;
    } else {
        spans.push(span);
    }
}
