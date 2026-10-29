//! Paint a shared reading column with native text and optional equation images.
//! Logical lines retain Markdown context for headings, lists, quotes, and code.

use std::{
    io::{self, Write},
    sync::Arc,
};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    layout::{
        Role, Run, Style,
        lines::{self, Line, Part},
    },
    math::MathEngine,
};

use super::{Options, kitty, text};

struct LogicalLine {
    runs: Vec<Run>,
    style: Style,
}

impl LogicalLine {
    fn context(&self) -> Style {
        self.runs
            .iter()
            .find(|run| !matches!(run.style.role, Role::ListMarker | Role::QuoteMarker))
            .map_or(self.style, |run| run.style)
    }
}

fn logical_lines(runs: &[Run]) -> Vec<LogicalLine> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    for run in runs {
        if run.math.is_some() {
            current.push(run.clone());
            continue;
        }
        for piece in run.text.split_inclusive('\n') {
            let value = piece.strip_suffix('\n').unwrap_or(piece);
            if !value.is_empty() {
                current.push(Run {
                    text: value.to_owned(),
                    ..run.clone()
                });
            }
            if piece.ends_with('\n') {
                result.push(LogicalLine {
                    runs: std::mem::take(&mut current),
                    style: run.style,
                });
            }
        }
    }
    if !current.is_empty() {
        let style = current[0].style;
        result.push(LogicalLine {
            runs: current,
            style,
        });
    }
    result
}

struct Frame {
    first: String,
    continuation: String,
    style: Style,
    prefix_style: Style,
    width: usize,
    right: usize,
    fill: Option<Style>,
}

fn fit_prefix(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut columns = 0;
    for grapheme in value.graphemes(true) {
        let size = UnicodeWidthStr::width(grapheme);
        if columns + size > width {
            break;
        }
        result.push_str(grapheme);
        columns += size;
    }
    result
}

impl Frame {
    fn new(line: &mut LogicalLine, options: &Options) -> Self {
        let viewport = options.viewport();
        let marker_role = |role| matches!(role, Role::ListMarker | Role::QuoteMarker);
        let style = line.context();
        let marker = line
            .runs
            .iter()
            .find(|run| run.style.role == Role::ListMarker)
            .map(|run| text::sanitize(&run.text).replacen("- ", "• ", 1));
        while line
            .runs
            .first()
            .is_some_and(|run| marker_role(run.style.role))
        {
            line.runs.remove(0);
        }
        let quotes = "│ ".repeat(style.quote_depth.min(viewport.width / 2));
        let indent = " ".repeat(style.indent.min(viewport.width));
        let mut first = format!("{quotes}{}", marker.as_deref().unwrap_or(&indent));
        let mut continuation = format!("{quotes}{indent}");
        let mut right = 0;
        let mut fill = None;
        let mut prefix_style = Style {
            role: if style.quote_depth > 0 {
                Role::QuoteMarker
            } else {
                Role::ListMarker
            },
            ..style
        };
        match style.role {
            Role::Heading(1) => {
                let prefix = if options.styled { " " } else { "# " };
                first.push_str(prefix);
                continuation.push_str(&" ".repeat(prefix.len()));
                if options.styled && viewport.width > 2 {
                    right = 1;
                    fill = Some(style);
                }
                prefix_style = style;
            }
            Role::Heading(level) => {
                let prefix = format!("{} ", "#".repeat(usize::from(level)));
                first.push_str(&prefix);
                continuation.push_str(&" ".repeat(prefix.len()));
                prefix_style = style;
            }
            Role::CodeBlock => {
                first.push_str("│ ");
                continuation.push_str("│ ");
                fill = Some(style);
                prefix_style = style;
            }
            _ => {}
        }
        let limit = viewport.width.saturating_sub(right + 2);
        first = fit_prefix(&first, limit);
        continuation = fit_prefix(&continuation, limit);
        let prefix_width = UnicodeWidthStr::width(first.as_str())
            .max(UnicodeWidthStr::width(continuation.as_str()));
        Self {
            first,
            continuation,
            style,
            prefix_style,
            width: viewport.width.saturating_sub(prefix_width + right).max(1),
            right,
            fill,
        }
    }
}

pub(super) fn write(
    out: &mut impl Write,
    runs: &[Run],
    engine: &mut Option<MathEngine>,
    options: &Options,
    warnings: &mut Vec<String>,
) -> io::Result<()> {
    let geometry = options.geometry();
    let cell_width = u32::from(geometry.cell_width.max(1));
    let cell_height = u32::from(geometry.cell_height.max(1));
    // Different formulas may extend above and below the baseline. Each is
    // limited to half the viewport so the combined line still fits.
    let max_height = u32::from(geometry.rows.saturating_sub(4).max(1)) * cell_height / 2;
    let mut logical = logical_lines(runs);
    for index in 1..logical.len().saturating_sub(1) {
        if logical[index].runs.is_empty() && logical[index].style.role == Role::Body {
            logical[index].style.quote_depth = logical[index - 1]
                .context()
                .quote_depth
                .min(logical[index + 1].context().quote_depth);
        }
    }
    for mut logical in logical {
        if logical.runs.is_empty()
            && logical.style.role != Role::CodeBlock
            && logical.style.quote_depth == 0
        {
            writeln!(out)?;
            continue;
        }
        let frame = Frame::new(&mut logical, options);
        if frame.style.role == Role::Rule {
            let line = Line {
                parts: vec![Part::Text {
                    text: "─".repeat(frame.width),
                    style: frame.style,
                }],
                columns: frame.width,
            };
            write_line(out, &line, &frame, true, options)?;
            continue;
        }
        let mut parts = Vec::new();
        for run in &logical.runs {
            if let Some(tex) = &run.math
                && options.kitty.is_some()
            {
                let math = engine.get_or_insert_with(MathEngine::default);
                match math.render_inline(
                    tex,
                    options.font_size,
                    options.theme().ink(run.style),
                    frame.width as u32 * cell_width,
                    max_height.max(1),
                ) {
                    Ok(image) => {
                        parts.push(Part::Math {
                            columns: image.width.div_ceil(cell_width) as usize,
                            image: Arc::new(image),
                            source: run.text.clone(),
                        });
                        continue;
                    }
                    Err(error) => warnings.push(format!(
                        "inline math at byte {}: {error}; showing TeX",
                        run.source.as_ref().map_or(0, |source| source.start),
                    )),
                }
            }
            let value = text::sanitize(&run.text);
            let value = if run.style.code && run.style.role != Role::CodeBlock && run.math.is_none()
            {
                format!(" {value} ")
            } else {
                value
            };
            parts.push(Part::Text {
                text: value,
                style: run.style,
            });
        }
        let mut wrapped = lines::wrap(parts, frame.width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        for (index, line) in wrapped.iter().enumerate() {
            write_line(out, line, &frame, index == 0, options)?;
        }
    }
    Ok(())
}

pub(super) fn write_text(
    out: &mut impl Write,
    value: &str,
    style: Style,
    options: &Options,
) -> io::Result<()> {
    write(
        out,
        &[Run {
            text: value.to_owned(),
            style,
            math: None,
            source: None,
        }],
        &mut None,
        options,
        &mut Vec::new(),
    )
}

fn span(out: &mut impl Write, value: &str, style: Style, options: &Options) -> io::Result<()> {
    text::write_span(out, value, style, options.styled, options.theme())
}

fn write_line(
    out: &mut impl Write,
    line: &Line,
    frame: &Frame,
    first: bool,
    options: &Options,
) -> io::Result<()> {
    let geometry = options.geometry();
    let viewport = options.viewport();
    let prefix = if first {
        &frame.first
    } else {
        &frame.continuation
    };
    let prefix_columns = UnicodeWidthStr::width(prefix.as_str());
    let metrics = line.metrics(geometry);
    let height = metrics.rows * u32::from(geometry.cell_height.max(1));
    let fill_width = if frame.style.role == Role::CodeBlock {
        viewport.width
    } else {
        prefix_columns + line.columns + frame.right
    };
    if line.has_math() {
        kitty::reserve(out, metrics.rows)?;
        // Paint a heading's background before transparent images are placed.
        if let Some(style) = frame.fill
            && options.styled
        {
            for row in 0..metrics.rows {
                write!(out, "\r{}", " ".repeat(viewport.left))?;
                span(out, &" ".repeat(fill_width), style, options)?;
                if row + 1 < metrics.rows {
                    write!(out, "\x1b[1B")?;
                }
            }
            if metrics.rows > 1 {
                write!(out, "\x1b[{}A", metrics.rows - 1)?;
            }
            write!(out, "\r")?;
        }
        if metrics.text_row > 0 {
            write!(out, "\x1b[{}B", metrics.text_row)?;
        }
    }
    write!(out, "{}", " ".repeat(viewport.left))?;
    span(out, prefix, frame.prefix_style, options)?;
    for part in &line.parts {
        match part {
            Part::Text { text: value, style } => span(out, value, *style, options)?,
            Part::Math { image, columns, .. } => {
                let width = *columns as u32 * u32::from(geometry.cell_width.max(1));
                let y = (metrics.baseline - image.baseline).ceil().max(0.) as u32;
                let image = kitty::pad(image, width, height, 0, y)?;
                if metrics.text_row > 0 {
                    write!(out, "\x1b[{}A", metrics.text_row)?;
                }
                kitty::transmit(out, &image, *columns as u32, metrics.rows)?;
                if metrics.text_row > 0 {
                    write!(out, "\x1b[{}B", metrics.text_row)?;
                }
                write!(out, "\x1b[{columns}C")?;
            }
        }
    }
    if let Some(style) = frame.fill {
        let remaining = fill_width.saturating_sub(prefix_columns + line.columns);
        span(out, &" ".repeat(remaining), style, options)?;
    }
    if line.has_math() {
        write!(out, "\x1b[{}B\r", metrics.rows - metrics.text_row)?;
        if options.show_math_source {
            let sources: Vec<_> = line
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Math { source, .. } => Some(text::sanitize(source)),
                    _ => None,
                })
                .collect();
            write_text(
                out,
                &format!("TeX: {}\n", sources.join("  ")),
                Style {
                    role: Role::MathSource,
                    bold: false,
                    code: false,
                    ..frame.style
                },
                options,
            )?;
        }
    } else {
        writeln!(out)?;
    }
    Ok(())
}
