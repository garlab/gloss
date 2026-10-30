//! Bounded width estimation and row-at-a-time table rendering.

use std::io::{self, Write};

use pulldown_cmark::Alignment;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    layout::{
        Role, Style,
        lines::{self, Line, Part},
        table::{Cell, Row, Table},
    },
    math::MathEngine,
};

use super::{Options, inline, text};

const MAX_COLUMNS: usize = 16;
const MEASURE_ROWS: usize = 64;
const MEASURE_BYTES: usize = 64 * 1024;
const MAX_CELL_BYTES: usize = 64 * 1024;
const MAX_ROW_BYTES: usize = 256 * 1024;
const MAX_ROW_RUNS: usize = 4096;
const MAX_ROW_FORMULAS: usize = 32;
const MAX_ROW_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MIN_WIDTH: usize = 3;

fn preferred_width(cell: &Cell, budget: &mut usize, limit: usize) -> usize {
    let mut column = 0;
    let mut widest = 0;
    for run in &cell.runs {
        if *budget == 0 {
            break;
        }
        let mut end = run.text.len().min(*budget);
        while !run.text.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            *budget = 0;
            break;
        }
        *budget -= end;
        if run.style.code && run.math.is_none() {
            column += 2;
        }
        let value = text::sanitize(&run.text[..end]);
        for grapheme in value.graphemes(true) {
            match grapheme {
                "\n" => {
                    widest = widest.max(column);
                    column = 0;
                }
                "\t" => column += 4 - column % 4,
                _ => column += UnicodeWidthStr::width(grapheme),
            }
            column = column.min(limit);
        }
        if end < run.text.len() {
            *budget = 0;
        }
    }
    widest.max(column).min(limit)
}

fn widths(table: &Table, available: usize) -> Option<Vec<usize>> {
    let count = table.alignments.len();
    if count == 0
        || count > MAX_COLUMNS
        || (count > 1 && available < 20)
        || count * MIN_WIDTH + (count - 1) * 3 > available
    {
        return None;
    }
    let mut desired = vec![MIN_WIDTH; count];
    let mut budget = MEASURE_BYTES;
    for row in table.rows.iter().take(MEASURE_ROWS) {
        for (cell, desired) in row.cells.iter().zip(&mut desired) {
            *desired = (*desired).max(preferred_width(cell, &mut budget, available));
        }
        if budget == 0 {
            break;
        }
    }
    if table.rows.len() > MEASURE_ROWS || budget == 0 {
        // Leave modest room for later identifiers and numbers that grow beyond
        // the bounded sample, without rescanning or retaining another model.
        for desired in &mut desired {
            *desired = desired.saturating_add(2).min(available);
        }
    }
    let mut widths = vec![MIN_WIDTH; count];
    let mut remaining = available - count * MIN_WIDTH - (count - 1) * 3;
    loop {
        let mut grew = false;
        for (width, desired) in widths.iter_mut().zip(&desired) {
            if remaining > 0 && *width < *desired {
                *width += 1;
                remaining -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    Some(widths)
}

fn bounded_row(row: &Row, columns: usize) -> bool {
    if row.cells.len() > columns {
        return false;
    }
    let (mut bytes, mut runs, mut formulas) = (0, 0, 0);
    for cell in &row.cells {
        let mut cell_bytes = 0;
        for run in &cell.runs {
            cell_bytes += run.text.len();
            bytes += run.text.len();
            runs += 1;
            formulas += usize::from(run.math.is_some());
            if cell_bytes > MAX_CELL_BYTES
                || bytes > MAX_ROW_BYTES
                || runs > MAX_ROW_RUNS
                || formulas > MAX_ROW_FORMULAS
            {
                return false;
            }
        }
    }
    true
}

fn push_text(line: &mut Line, value: String, style: Style) {
    if !value.is_empty() {
        line.columns += UnicodeWidthStr::width(value.as_str());
        line.parts.push(Part::Text {
            text: value,
            style,
            target: None,
        });
    }
}

fn separator(widths: &[usize], style: Style) -> Line {
    let mut line = Line::default();
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            push_text(&mut line, "─┼─".into(), style);
        }
        push_text(&mut line, "─".repeat(*width), style);
    }
    line
}

pub(super) fn write(
    out: &mut impl Write,
    table: &Table,
    engine: &mut Option<MathEngine>,
    options: &Options,
    warnings: &mut Vec<String>,
) -> io::Result<()> {
    let frame = inline::Frame::for_block(&table.prefix, table.style, options);
    let widths = widths(table, frame.width());
    let border = Style {
        role: Role::TableBorder,
        ..table.style
    };
    let mut first = true;
    for row in &table.rows {
        if let Some(widths) = &widths
            && bounded_row(row, widths.len())
        {
            let mut cells = Vec::with_capacity(widths.len());
            let mut height = 1;
            let mut image_budget = MAX_ROW_IMAGE_BYTES;
            for (index, width) in widths.iter().enumerate() {
                let runs = row
                    .cells
                    .get(index)
                    .into_iter()
                    .flat_map(|cell| cell.runs.iter())
                    .cloned()
                    .map(|mut run| {
                        if row.header {
                            run.style.bold = true;
                            run.style.role = Role::TableHeader;
                        }
                        run
                    });
                let parts = inline::prepare_parts(
                    runs,
                    *width,
                    engine,
                    options,
                    warnings,
                    Some(&mut image_budget),
                );
                let lines = lines::wrap(parts, *width);
                height = height.max(lines.len());
                cells.push(lines.into_iter());
            }
            for _ in 0..height {
                let mut line = Line::default();
                for (index, (cell, width)) in cells.iter_mut().zip(widths).enumerate() {
                    if index > 0 {
                        push_text(&mut line, " │ ".into(), border);
                    }
                    let content = cell.next().unwrap_or_default();
                    let spare = width.saturating_sub(content.columns);
                    let left = match table.alignments[index] {
                        Alignment::Center => spare / 2,
                        Alignment::Right => spare,
                        _ => 0,
                    };
                    push_text(&mut line, " ".repeat(left), table.style);
                    line.columns += content.columns;
                    line.parts.extend(content.parts);
                    push_text(&mut line, " ".repeat(spare - left), table.style);
                }
                inline::write_line(out, &line, &frame, first, options)?;
                first = false;
            }
            if row.header {
                inline::write_line(out, &separator(widths, border), &frame, false, options)?;
            }
        } else {
            for (index, cell) in row.cells.iter().enumerate() {
                inline::write_cell(
                    out,
                    &cell.runs,
                    &frame.cell(index + 1, first),
                    options,
                    row.header,
                )?;
                first = false;
            }
            if !row.cells.is_empty() {
                inline::write_line(out, &Line::default(), &frame, false, options)?;
            }
        }
    }
    Ok(())
}

/// Match the flat export's existing separators, source TeX, and line endings.
pub(super) fn write_plain(out: &mut impl Write, table: &Table) -> io::Result<()> {
    if !table.prefix.is_empty() {
        text::write_runs(out, &table.prefix, false)?;
        out.write_all(b"\n\n")?;
    }
    for row in &table.rows {
        for cell in &row.cells {
            text::write_runs(out, &cell.runs, false)?;
            out.write_all(b" | ")?;
        }
        writeln!(out)?;
    }
    Ok(())
}
