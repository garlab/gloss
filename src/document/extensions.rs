//! Small source projections for syntax the CommonMark parser does not recognize.
//! The original bytes stay authoritative; edits carry an offset map. Ordinary
//! documents borrow their source and do not allocate a second document string.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

const MAX_FRONT_MATTER: usize = 16 * 1024;
const MAX_DETAILS: usize = 256 * 1024;
const MAX_DETAILS_DEPTH: usize = 16;
const MAX_MATH: usize = 16 * 1024;
const MAX_EDITS: usize = 16_384;
const MAX_MATH_SOURCE: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(super) struct Prepared {
    pub text: Option<String>,
    maps: Vec<Vec<Edit>>,
    maths: Vec<(Range<usize>, String)>,
}

#[derive(Debug)]
struct Edit {
    source: Range<usize>,
    projected: Range<usize>,
    replacement: String,
    math: Option<String>,
}

impl Prepared {
    pub fn original_range(&self, mut range: Range<usize>) -> Range<usize> {
        for map in self.maps.iter().rev() {
            range = original_offset(map, range.start, false)..original_offset(map, range.end, true);
        }
        range
    }

    pub fn math(&self, range: &Range<usize>) -> Option<&str> {
        let index = self
            .maths
            .partition_point(|(item, _)| item.start < range.start);
        self.maths
            .get(index)
            .filter(|(item, _)| item == range)
            .map(|(_, tex)| tex.as_str())
    }

    fn apply(&mut self, original: &str, mut edits: Vec<Edit>) {
        if edits.is_empty() {
            return;
        }
        edits.sort_by_key(|edit| edit.source.start);
        let input = self.text.as_deref().unwrap_or(original);
        let mut text = String::with_capacity(input.len());
        let mut cursor = 0;
        edits.retain(|edit| {
            if edit.source.start < cursor {
                return false;
            }
            cursor = edit.source.end;
            true
        });
        cursor = 0;
        for edit in &mut edits {
            text.push_str(&input[cursor..edit.source.start]);
            let start = text.len();
            text.push_str(&edit.replacement);
            edit.projected = start..text.len();
            cursor = edit.source.end;
        }
        text.push_str(&input[cursor..]);
        for edit in &mut edits {
            if let Some(tex) = edit.math.take() {
                self.maths
                    .push((self.original_range(edit.source.clone()), tex));
            }
            // Only ranges are needed after projection; release replacement text.
            edit.replacement = String::new();
        }
        self.text = Some(text);
        self.maps.push(edits);
    }
}

fn original_offset(edits: &[Edit], offset: usize, end: bool) -> usize {
    let index = edits.partition_point(|edit| {
        edit.projected.end < offset || (!end && edit.projected.end == offset)
    });
    if let Some(edit) = edits.get(index)
        && offset > edit.projected.start
    {
        return if end {
            edit.source.end
        } else {
            edit.source.start
        };
    }
    if let Some(edit) = edits.get(index)
        && offset == edit.projected.start
    {
        return edit.source.start;
    }
    if index == 0 {
        offset
    } else {
        let previous = &edits[index - 1];
        offset - previous.projected.end + previous.source.end
    }
}

fn edit(source: Range<usize>, replacement: String) -> Edit {
    Edit {
        source,
        projected: 0..0,
        replacement,
        math: None,
    }
}

pub(super) fn prepare(source: &str) -> Prepared {
    let metadata = front_matter(source);
    let details = source.contains("<details>") || source.contains("<details open>");
    let math = source.contains("\\(") || source.contains("\\[");
    if metadata.is_none() && !details && !math {
        return Prepared::default();
    }
    let mut prepared = Prepared::default();
    let mut edits = Vec::new();
    if let Some(range) = &metadata {
        // Visible inert metadata, including the original delimiters. Tags,
        // aliases, and metadata-controlled viewer settings are never evaluated.
        let body = &source[range.clone()];
        let longest = body.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(3.max(longest + 1));
        let newline = if body.ends_with('\n') { "" } else { "\n" };
        edits.push(edit(
            range.clone(),
            format!("{fence}text\n{body}{newline}{fence}\n"),
        ));
    }
    if details {
        let mut protected = protected_ranges(source, false).0;
        if let Some(range) = metadata {
            protected.push(range);
        }
        merge_ranges(&mut protected);
        details_edits(source, &protected, &mut edits);
    }
    prepared.apply(source, edits);
    if math {
        let input = prepared.text.as_deref().unwrap_or(source);
        let (protected, containers, visible) = protected_ranges(input, true);
        let mut edits = Vec::new();
        math_edits(input, &containers, &protected, &visible, &mut edits);
        prepared.apply(source, edits);
    }
    prepared
}

struct Container {
    range: Range<usize>,
    quotes: usize,
}

type Ranges = (Vec<Range<usize>>, Vec<Container>, Vec<Range<usize>>);

fn protected_ranges(source: &str, math: bool) -> Ranges {
    let options = Options::ENABLE_TABLES | Options::ENABLE_MATH | Options::ENABLE_GFM;
    let mut protected = Vec::new();
    let mut containers = Vec::new();
    let mut visible = Vec::new();
    let mut quotes: usize = 0;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_)) => quotes += 1,
            Event::End(TagEnd::BlockQuote(_)) => quotes = quotes.saturating_sub(1),
            Event::Text(_) if math => visible.push(range),
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => protected.push(range),
            Event::InlineMath(_) | Event::DisplayMath(_) if math => protected.push(range),
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell) if math => {
                containers.push(Container { range, quotes })
            }
            Event::Html(_) | Event::InlineHtml(_) if math => protected.push(range),
            Event::Start(Tag::HtmlBlock) if !math => {
                let value = source[range.clone()].trim_start();
                if !["<details>", "<details open>", "</details>", "<summary>"]
                    .iter()
                    .any(|tag| value.starts_with(tag))
                {
                    protected.push(range);
                }
            }
            Event::End(TagEnd::CodeBlock) => {}
            _ => {}
        }
    }
    merge_ranges(&mut protected);
    containers.sort_by_key(|item| item.range.start);
    merge_ranges(&mut visible);
    (protected, containers, visible)
}

fn merge_ranges(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_by_key(|range| range.start);
    let mut count = 0;
    for index in 0..ranges.len() {
        if count > 0 && ranges[index].start <= ranges[count - 1].end {
            ranges[count - 1].end = ranges[count - 1].end.max(ranges[index].end);
        } else {
            ranges.swap(count, index);
            count += 1;
        }
    }
    ranges.truncate(count);
}

fn front_matter(source: &str) -> Option<Range<usize>> {
    let first = source.lines().next()?;
    if !matches!(first, "---" | "+++") {
        return None;
    }
    let mut end = source.find('\n')? + 1;
    for line in source[end..].split_inclusive('\n') {
        end += line.len();
        if end > MAX_FRONT_MATTER {
            return None;
        }
        let value = line.trim_end_matches(['\r', '\n']);
        if value == first || (first == "---" && value == "...") {
            // Avoid turning an ordinary empty thematic break into metadata.
            if source[first.len()..end - line.len()].trim().is_empty() {
                return None;
            }
            return Some(0..end);
        }
    }
    None
}

fn overlaps(ranges: &[Range<usize>], range: &Range<usize>) -> bool {
    let index = ranges.partition_point(|item| item.end <= range.start);
    ranges.get(index).is_some_and(|item| item.start < range.end)
}

fn details_edits(source: &str, protected: &[Range<usize>], edits: &mut Vec<Edit>) {
    // Only balanced standalone wrappers are recognized. A single-line summary
    // becomes a bold paragraph; body Markdown is always expanded. Other HTML
    // remains literal, including attributes we do not understand.
    let mut stack: Vec<(usize, Vec<Edit>)> = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    let mut remaining = MAX_EDITS.saturating_sub(edits.len());
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if stack
            .first()
            .is_some_and(|(begin, _)| offset - begin > MAX_DETAILS)
        {
            stack.clear();
        }
        let value = line.trim_end_matches(['\r', '\n']);
        let trimmed = value.trim_start_matches(' ');
        let indent = value.len() - trimmed.len();
        let bytes = trimmed.as_bytes();
        if indent <= 3 && matches!(bytes.first(), Some(b'`' | b'~')) {
            let count = bytes.iter().take_while(|&&byte| byte == bytes[0]).count();
            if let Some((marker, length)) = fence {
                if bytes[0] == marker && count >= length && trimmed[count..].trim().is_empty() {
                    fence = None;
                }
            } else if count >= 3 {
                fence = Some((bytes[0], count));
            }
            continue;
        }
        if fence.is_some() || indent > 3 {
            continue;
        }
        if overlaps(protected, &(start..offset)) {
            continue;
        }
        match trimmed {
            "<details>" | "<details open>" => {
                if stack.len() >= MAX_DETAILS_DEPTH || remaining == 0 {
                    return;
                }
                remaining -= 1;
                stack.push((start, vec![edit(start..offset, "\n".into())]));
            }
            "</details>" if !stack.is_empty() => {
                let (begin, mut pending) = stack.pop().unwrap();
                if offset - begin > MAX_DETAILS {
                    continue;
                }
                if remaining == 0 {
                    return;
                }
                remaining -= 1;
                pending.push(edit(start..offset, "\n".into()));
                if let Some((_, parent)) = stack.last_mut() {
                    parent.extend(pending);
                } else {
                    edits.extend(pending);
                }
            }
            _ if !stack.is_empty() => {
                if let Some(summary) = trimmed
                    .strip_prefix("<summary>")
                    .and_then(|s| s.strip_suffix("</summary>"))
                    && !summary.contains(['<', '>'])
                {
                    let pending = &mut stack.last_mut().unwrap().1;
                    if remaining < 2 {
                        return;
                    }
                    remaining -= 2;
                    let content_start = start + indent + "<summary>".len();
                    let content_end = content_start + summary.len();
                    pending.push(edit(start..content_start, "\n**".into()));
                    pending.push(edit(content_end..offset, "**\n\n".into()));
                }
            }
            _ => {}
        }
    }
}

fn math_edits(
    source: &str,
    containers: &[Container],
    protected: &[Range<usize>],
    visible: &[Range<usize>],
    edits: &mut Vec<Edit>,
) {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut remaining = MAX_MATH_SOURCE;
    while cursor + 1 < bytes.len() && edits.len() < MAX_EDITS {
        if bytes[cursor] != b'\\' {
            cursor += 1;
            continue;
        }
        if bytes[cursor + 1] == b'\\' {
            cursor += 2;
            continue;
        }
        let close = match bytes[cursor + 1] {
            b'(' => b')',
            b'[' => b']',
            _ => {
                cursor += 2;
                continue;
            }
        };
        let start = cursor;
        cursor += 2;
        let index = containers.partition_point(|item| item.range.end <= start);
        let Some(container) = containers
            .get(index)
            .filter(|item| item.range.start <= start)
        else {
            continue;
        };
        if !overlaps(visible, &(start + 1..cursor)) || overlaps(protected, &(start..cursor)) {
            continue;
        }
        let limit = container
            .range
            .end
            .min(start.saturating_add(MAX_MATH))
            .min(bytes.len());
        let mut end = cursor;
        while end + 1 < limit {
            if bytes[end] == b'\\' {
                if bytes[end + 1] == close && overlaps(visible, &(end + 1..end + 2)) {
                    break;
                }
                end += 2;
            } else {
                end += 1;
            }
        }
        if end + 1 >= limit {
            cursor = end.max(cursor);
            continue;
        }
        let range = start..end + 2;
        // A rejected pair must also advance past its closing delimiter. Without
        // this, many openers before an excluded code span rescan the same tail.
        cursor = end + 2;
        if overlaps(protected, &range) {
            continue;
        }
        let tex = &source[start + 2..end];
        if tex.trim().is_empty() {
            continue;
        }
        let mut replacement = edit(range, if close == b')' { "$m$" } else { "$$m$$" }.into());
        if tex.len() > remaining {
            break;
        }
        remaining -= tex.len();
        replacement.math = Some(unquote(tex, container.quotes));
        edits.push(replacement);
    }
}

fn unquote(tex: &str, quotes: usize) -> String {
    if quotes == 0 {
        return tex.to_owned();
    }
    let mut output = String::new();
    for (index, mut line) in tex.split_inclusive('\n').enumerate() {
        if index > 0 {
            for _ in 0..quotes {
                let value = line.trim_start_matches(' ');
                if let Some(rest) = value.strip_prefix('>') {
                    line = rest.strip_prefix(' ').unwrap_or(rest);
                } else {
                    break;
                }
            }
        }
        output.push_str(line);
    }
    output
}
