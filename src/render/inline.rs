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

fn logical_lines(runs: &[Run]) -> impl Iterator<Item = LogicalLine> + '_ {
    let mut runs = runs.iter();
    let mut active: Option<(&Run, std::str::SplitInclusive<'_, char>)> = None;
    let mut current = Vec::new();
    std::iter::from_fn(move || {
        loop {
            if let Some((run, pieces)) = &mut active {
                if let Some(piece) = pieces.next() {
                    let value = piece.strip_suffix('\n').unwrap_or(piece);
                    if !value.is_empty() {
                        current.push(Run {
                            text: value.to_owned(),
                            style: run.style,
                            math: None,
                            target: run.target.clone(),
                            source: run.source,
                        });
                    }
                    if piece.ends_with('\n') {
                        return Some(LogicalLine {
                            runs: std::mem::take(&mut current),
                            style: run.style,
                        });
                    }
                    continue;
                }
                active = None;
            }
            match runs.next() {
                Some(run) if run.math.is_some() => current.push(run.clone()),
                Some(run) => active = Some((run, run.text.split_inclusive('\n'))),
                None if !current.is_empty() => {
                    return Some(LogicalLine {
                        style: current[0].style,
                        runs: std::mem::take(&mut current),
                    });
                }
                None => return None,
            }
        }
    })
}

#[derive(Clone)]
pub(super) struct Frame {
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
    pub(super) fn for_block(prefix: &[Run], style: Style, options: &Options) -> Self {
        Self::new(
            &mut LogicalLine {
                runs: prefix.to_vec(),
                style,
            },
            options,
        )
    }

    pub(super) fn continued(&self) -> Self {
        Self {
            first: self.continuation.clone(),
            ..self.clone()
        }
    }

    pub(super) fn width(&self) -> usize {
        self.width
    }

    pub(super) fn cell(&self, number: usize, first: bool) -> Self {
        let label = fit_prefix(&format!("{number}: "), self.width.saturating_sub(2));
        let columns = UnicodeWidthStr::width(label.as_str());
        let prefix = if first {
            &self.first
        } else {
            &self.continuation
        };
        Self {
            first: format!("{prefix}{label}"),
            continuation: format!("{}{}", self.continuation, " ".repeat(columns)),
            style: self.style,
            prefix_style: Style {
                role: Role::TableBorder,
                ..self.prefix_style
            },
            width: self.width.saturating_sub(columns).max(1),
            right: 0,
            fill: None,
        }
    }

    fn new(line: &mut LogicalLine, options: &Options) -> Self {
        let viewport = options.viewport();
        let marker_role = |role| matches!(role, Role::ListMarker | Role::QuoteMarker);
        // Token colors apply to code content, never the frame or padding.
        let style = Style {
            syntax: None,
            ..line.context()
        };
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
    edge_quotes: (usize, usize),
) -> io::Result<()> {
    let mut lines = logical_lines(runs).peekable();
    let mut previous_quote = edge_quotes.0;
    while let Some(mut logical) = lines.next() {
        if logical.runs.is_empty() && logical.style.role == Role::Body {
            let after = lines
                .peek()
                .map_or(edge_quotes.1, |next| next.context().quote_depth);
            logical.style.quote_depth = previous_quote.min(after);
        }
        previous_quote = logical.context().quote_depth;
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
                    target: None,
                    style: frame.style,
                }],
                columns: frame.width,
            };
            write_line(out, &line, &frame, true, options)?;
            continue;
        }
        let mut image_budget = usize::MAX;
        let parts = prepare_parts(
            logical.runs,
            frame.width,
            engine,
            options,
            warnings,
            Some(&mut image_budget),
        );
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

pub(super) fn prepare_parts(
    runs: impl IntoIterator<Item = Run>,
    width: usize,
    engine: &mut Option<MathEngine>,
    options: &Options,
    warnings: &mut Vec<String>,
    mut image_budget: Option<&mut usize>,
) -> Vec<Part> {
    let geometry = options.geometry();
    let cell_width = u32::from(geometry.cell_width.max(1));
    let cell_height = u32::from(geometry.cell_height.max(1));
    // Limit each formula to half the viewport so combined baselines still fit.
    let max_height = u32::from(geometry.rows.saturating_sub(4).max(1)) * cell_height / 2;
    let mut parts = Vec::new();
    for run in runs {
        if let Some(tex) = &run.math
            && options.kitty.is_some()
            && let Some(remaining) = image_budget.as_deref_mut()
            && *remaining / 4 >= width * cell_width as usize
        {
            let height = max_height.max(1).min(
                (*remaining / 4 / (width * cell_width as usize)).min(u32::MAX as usize) as u32,
            );
            let math = engine.get_or_insert_with(MathEngine::default);
            match math.render_inline(
                tex,
                options.font_size,
                options.theme().ink(run.style),
                width as u32 * cell_width,
                height,
            ) {
                Ok(image) => {
                    *remaining = remaining.saturating_sub(image.rgba.len());
                    parts.push(Part::Image {
                        columns: image.width.div_ceil(cell_width) as usize,
                        baseline: image.baseline,
                        image: image.pixels,
                        source: Some(run.text),
                    });
                    continue;
                }
                Err(error) => warnings.push(format!(
                    "inline math at byte {}: {error}; showing TeX",
                    run.source.as_ref().map_or(0, |source| source.start),
                )),
            }
        }
        let value = text::sanitize_owned(run.text);
        let value = if run.style.code && run.style.role != Role::CodeBlock && run.math.is_none() {
            format!(" {value} ")
        } else {
            value
        };
        parts.push(Part::Text {
            text: value,
            target: if options.links { run.target } else { None },
            style: run.style,
        });
    }
    parts
}

/// Stacked cells use the ordinary logical-line pipeline and source TeX. This
/// avoids retaining a large row of wrapped cells or math rasters on fallback.
pub(super) fn write_cell(
    out: &mut impl Write,
    runs: &[Run],
    frame: &Frame,
    options: &Options,
    header: bool,
) -> io::Result<()> {
    let mut first = true;
    for logical in logical_lines(runs) {
        let runs = logical.runs.into_iter().map(|mut run| {
            if header {
                run.style.bold = true;
                run.style.role = Role::TableHeader;
            }
            run
        });
        let parts = prepare_parts(runs, frame.width, &mut None, options, &mut Vec::new(), None);
        let mut wrapped = lines::wrap(parts, frame.width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        for line in &wrapped {
            write_line(out, line, frame, first, options)?;
            first = false;
        }
    }
    if first {
        write_line(out, &Line::default(), frame, true, options)?;
    }
    Ok(())
}

/// Use the same reserved rows, quote rails, padding, and transport as equations.
/// Tall pictures are emitted in contiguous bands so cursor movement stays on
/// screen. Charge the complete padded image before writing any of its bands.
pub(super) fn write_picture(
    out: &mut impl Write,
    image: Arc<crate::raster::Raster>,
    frame: &Frame,
    options: &Options,
    loader: &mut crate::images::Loader,
) -> io::Result<bool> {
    let geometry = options.geometry();
    let cell_width = u32::from(geometry.cell_width.max(1));
    let cell_height = u32::from(geometry.cell_height.max(1));
    if image.width == 0
        || image.height == 0
        || u64::from(image.width) * u64::from(image.height) > 8_000_000
        || u64::from(image.width) * u64::from(image.height) * 4 != image.rgba.len() as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid picture",
        ));
    }
    let columns = image.width.div_ceil(cell_width) as usize;
    let inset = frame.width.saturating_sub(columns) / 2;
    let line = |image: Arc<crate::raster::Raster>| Line {
        columns: inset + columns,
        parts: vec![
            Part::Text {
                text: " ".repeat(inset),
                target: None,
                style: frame.style,
            },
            Part::Image {
                baseline: image.height as f64
                    - (cell_height as f64 - (cell_height as f64 * 0.78).floor()),
                image,
                columns,
                source: None,
            },
        ],
    };
    let rows = image.height.div_ceil(cell_height);
    if !loader.reserve_output(
        columns as u64 * u64::from(cell_width) * u64::from(rows) * u64::from(cell_height),
    ) {
        return Ok(false);
    }
    let band_rows = u32::from(geometry.rows.saturating_sub(2).max(1));
    if rows <= band_rows {
        write_line(out, &line(image), frame, true, options)?;
    } else {
        let height = rows * cell_height;
        let top_padding = height - image.height;
        let stride = image.width as usize * 4;
        let band_height = band_rows * cell_height;
        for top in (0..height).step_by(band_height as usize) {
            let bottom = (top + band_height).min(height);
            let mut rgba = vec![0; stride * (bottom - top) as usize];
            // Pad only the top of the whole picture, never a band boundary.
            let start = top.max(top_padding);
            if start < bottom {
                let source = (start - top_padding) as usize * stride;
                let destination = (start - top) as usize * stride;
                let length = (bottom - start) as usize * stride;
                rgba[destination..destination + length]
                    .copy_from_slice(&image.rgba[source..source + length]);
            }
            let band = Arc::new(crate::raster::Raster {
                width: image.width,
                height: bottom - top,
                rgba,
            });
            write_line(out, &line(band), frame, top == 0, options)?;
        }
    }
    Ok(true)
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
            target: None,
            source: None,
        }],
        &mut None,
        options,
        &mut Vec::new(),
        (0, 0),
    )
}

fn span(out: &mut impl Write, value: &str, style: Style, options: &Options) -> io::Result<()> {
    text::write_span(out, value, style, options.styled, options.theme())
}

fn move_rows(out: &mut impl Write, from: u32, to: u32) -> io::Result<()> {
    if to > from {
        write!(out, "\x1b[{}B", to - from)?;
    } else if from > to {
        write!(out, "\x1b[{}A", from - to)?;
    }
    Ok(())
}

pub(super) fn write_line(
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
    if line.has_images() {
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
        paint_vertical_borders(out, line, prefix, frame.prefix_style, metrics, options)?;
        if metrics.text_row > 0 {
            write!(out, "\x1b[{}B", metrics.text_row)?;
        }
    }
    text::write_spaces(out, viewport.left)?;
    let mut painter = text::Painter::new(options.styled, options.theme());
    painter.span(out, prefix, frame.prefix_style)?;
    for part in &line.parts {
        match part {
            Part::Text {
                text: value,
                style,
                target,
            } => painter.linked_span(out, value, *style, target.as_deref().map(String::as_str))?,
            Part::Image {
                image,
                columns,
                baseline,
                source,
            } => {
                painter.finish(out)?;
                let y = (metrics.baseline - baseline).ceil().max(0.) as u32;
                // Only equations carry TeX source. Ordinary images keep their
                // cell-aligned band transport and shared output accounting.
                let native = source.is_some();
                let cell_height = u32::from(geometry.cell_height.max(1));
                let image_row = if native { y / cell_height } else { 0 };
                move_rows(out, metrics.text_row, image_row)?;
                if native {
                    kitty::transmit_native(out, image, y % cell_height)?;
                } else {
                    let width = *columns as u32 * u32::from(geometry.cell_width.max(1));
                    let image = kitty::pad(image, width, height, 0, y)?;
                    kitty::transmit(out, &image, *columns as u32, metrics.rows)?;
                }
                move_rows(out, image_row, metrics.text_row)?;
                write!(out, "\x1b[{columns}C")?;
            }
        }
    }
    if let Some(style) = frame.fill {
        let remaining = fill_width.saturating_sub(prefix_columns + line.columns);
        painter.spaces(out, remaining, style)?;
    }
    painter.finish(out)?;
    if line.has_images() {
        write!(out, "\x1b[{}B\r", metrics.rows - metrics.text_row)?;
        if options.show_math_source {
            let sources: Vec<_> = line
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Image {
                        source: Some(source),
                        ..
                    } => Some(text::sanitize(source)),
                    _ => None,
                })
                .collect();
            if sources.is_empty() {
                return Ok(());
            }
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

/// Table separators and quote rails span the complete reserved image rectangle,
/// including rows above and below the text baseline. Paint before the images.
fn paint_vertical_borders(
    out: &mut impl Write,
    line: &Line,
    prefix: &str,
    prefix_style: Style,
    metrics: lines::Metrics,
    options: &Options,
) -> io::Result<()> {
    if metrics.rows <= 1 {
        return Ok(());
    }
    let mut markers = Vec::new();
    let mut column = options.viewport().left;
    for character in prefix.chars() {
        if character == '│' {
            markers.push((column, prefix_style));
        }
        column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
    }
    for part in &line.parts {
        if let Part::Text { text, style, .. } = part
            && style.role == Role::TableBorder
        {
            let mut offset = 0;
            for character in text.chars() {
                if character == '│' {
                    markers.push((column + offset, *style));
                }
                offset += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            }
        }
        column += part.columns();
    }
    if markers.is_empty() {
        return Ok(());
    }
    for row in 0..metrics.rows {
        if row != metrics.text_row {
            for (column, style) in &markers {
                write!(out, "\r")?;
                if *column > 0 {
                    write!(out, "\x1b[{column}C")?;
                }
                span(out, "│", *style, options)?;
            }
        }
        if row + 1 < metrics.rows {
            write!(out, "\x1b[1B")?;
        }
    }
    write!(out, "\x1b[{}A\r", metrics.rows - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::io::Read;

    #[test]
    fn picture_bands_preserve_every_pixel_and_only_pad_outer_edges() {
        let image = Arc::new(crate::raster::Raster {
            width: 13,
            height: 157,
            rgba: (0..13 * 157 * 4).map(|i| (i % 251) as u8).collect(),
        });
        let expected = kitty::pad(&image, 20, 160, 0, 3).unwrap();
        for rows in [3, 5, 24] {
            let options = Options {
                kitty: Some(crate::terminal::Geometry {
                    rows,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let frame = Frame::for_block(&[], Style::default(), &options);
            let mut output = Vec::new();
            assert!(
                write_picture(
                    &mut output,
                    image.clone(),
                    &frame,
                    &options,
                    &mut crate::images::Loader::default()
                )
                .unwrap()
            );
            let mut pixels = Vec::new();
            let mut payload = String::new();
            let mut compressed = false;
            for command in std::str::from_utf8(&output)
                .unwrap()
                .split("\x1b_G")
                .skip(1)
            {
                let (control, data) = command.split_once(';').unwrap();
                if control.starts_with("a=T,") {
                    compressed = control.contains(",o=z,");
                    let band_rows: u16 = control
                        .split(',')
                        .find_map(|field| field.strip_prefix("r="))
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert!(band_rows <= rows - 2);
                }
                payload.push_str(data.split_once("\x1b\\").unwrap().0);
                if control.ends_with("m=0") {
                    let bytes = STANDARD.decode(&payload).unwrap();
                    if compressed {
                        flate2::read::ZlibDecoder::new(bytes.as_slice())
                            .read_to_end(&mut pixels)
                            .unwrap();
                    } else {
                        pixels.extend(bytes);
                    }
                    payload.clear();
                }
            }
            assert!(payload.is_empty());
            assert_eq!(pixels, expected.rgba);
        }
    }

    #[test]
    fn picture_budget_failure_writes_no_partial_bands() {
        let options = Options {
            kitty: Some(Default::default()),
            ..Default::default()
        };
        let frame = Frame::for_block(&[], Style::default(), &options);
        let mut loader = crate::images::Loader::default();
        assert!(loader.reserve_output(8_000_000));
        assert!(loader.reserve_output(8_000_000));
        assert!(loader.reserve_output(64 * 1024 * 1024 / 4 - 16_000_001));
        let image = Arc::new(crate::raster::Raster {
            width: 1,
            height: 1000,
            rgba: vec![255; 4000],
        });
        let mut output = Vec::new();
        assert!(!write_picture(&mut output, image, &frame, &options, &mut loader).unwrap());
        assert!(output.is_empty());
    }

    #[test]
    fn exhausted_image_budget_keeps_source_without_initializing_math() {
        let mut engine = None;
        let mut warnings = Vec::new();
        let mut budget = 0;
        let options = Options {
            kitty: Some(Default::default()),
            ..Default::default()
        };
        let parts = prepare_parts(
            [Run {
                text: "$x$".into(),
                math: Some("x".into()),
                target: None,
                style: Style::default(),
                source: None,
            }],
            10,
            &mut engine,
            &options,
            &mut warnings,
            Some(&mut budget),
        );
        assert!(engine.is_none() && warnings.is_empty());
        assert!(matches!(&parts[..], [Part::Text { text, .. }] if text == "$x$"));
    }
}
