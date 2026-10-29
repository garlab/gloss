use crate::{
    highlight::{Highlighter, MAX_BLOCK_BYTES},
    layout::{Role, Run, Style},
};

pub(super) fn highlight(
    runs: &[Run],
    language: Option<&str>,
    engine: &mut Option<Highlighter>,
) -> Option<Vec<Run>> {
    let language = language.filter(|label| Highlighter::supports(label))?;
    let length = runs
        .iter()
        .filter(|run| run.style.role == Role::CodeBlock)
        .try_fold(0usize, |size, run| size.checked_add(run.text.len()))?;
    if length == 0 || length > MAX_BLOCK_BYTES {
        return None;
    }
    let mut text = String::with_capacity(length);
    for run in runs.iter().filter(|run| run.style.role == Role::CodeBlock) {
        text.push_str(&run.text);
    }
    let spans = engine
        .get_or_insert_with(Highlighter::default)
        .highlight(language, &text)?;
    if spans.is_empty() {
        return None;
    }
    let mut output = Vec::new();
    let mut offset = 0;
    let mut span_index = 0;
    for run in runs {
        if run.style.role != Role::CodeBlock {
            output.push(run.clone());
            continue;
        }
        let end = offset + run.text.len();
        let mut position = offset;
        while position < end {
            while span_index < spans.len() && spans[span_index].range.end <= position {
                span_index += 1;
            }
            let (next, syntax) = match spans.get(span_index) {
                Some(span) if span.range.start <= position => {
                    (span.range.end.min(end), Some(span.highlight))
                }
                Some(span) => (span.range.start.min(end), None),
                None => (end, None),
            };
            output.push(Run {
                text: run.text[position - offset..next - offset].to_owned(),
                style: Style {
                    syntax,
                    ..run.style
                },
                math: None,
                // This is still a source span: Markdown can remove quote/list
                // indentation, so decoded offsets must not be added blindly.
                source: run.source.clone(),
            });
            position = next;
        }
        offset = end;
    }
    Some(output)
}
