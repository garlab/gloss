//! Paint measured lines with native terminal text and cell-aligned formula
//! images. Only equations become pixels; ordinary text stays selectable.

use std::{
    io::{self, Write},
    sync::Arc,
};

use crate::{
    layout::{
        Run,
        lines::{self, Line, Part},
    },
    math::MathEngine,
    terminal::Geometry,
};

use super::{Options, kitty, text};

pub(super) fn write(
    out: &mut impl Write,
    runs: &[Run],
    engine: &mut Option<MathEngine>,
    geometry: Geometry,
    options: &Options,
    warnings: &mut Vec<String>,
) -> io::Result<()> {
    let columns = usize::from(geometry.columns.saturating_sub(1).max(1));
    let cell_width = u32::from(geometry.cell_width.max(1));
    let cell_height = u32::from(geometry.cell_height.max(1));
    let max_width = columns as u32 * cell_width;
    // Different formulas may reach above and below the text baseline. Reserving
    // at most half the viewport for each guarantees the combined line fits.
    let max_height = u32::from(geometry.rows.saturating_sub(4).max(1)) * cell_height / 2;
    let mut parts = Vec::new();
    for run in runs {
        if let Some(tex) = &run.math {
            let math = engine.get_or_insert_with(MathEngine::default);
            match math.render_inline(
                tex,
                options.font_size,
                options.ink(),
                max_width,
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
        parts.push(Part::Text {
            text: text::sanitize(&run.text),
            style: run.style,
        });
    }
    for line in lines::wrap(parts, columns) {
        write_line(out, &line, geometry, options)?;
    }
    Ok(())
}

fn write_line(
    out: &mut impl Write,
    line: &Line,
    geometry: Geometry,
    options: &Options,
) -> io::Result<()> {
    if !line.has_math() {
        for part in &line.parts {
            if let Part::Text { text: value, style } = part {
                text::write_span(out, value, *style, options.styled)?;
            }
        }
        return writeln!(out);
    }
    let metrics = line.metrics(geometry);
    let height = metrics.rows * u32::from(geometry.cell_height.max(1));
    kitty::reserve(out, metrics.rows)?;
    if metrics.text_row > 0 {
        write!(out, "\x1b[{}B", metrics.text_row)?;
    }
    for part in &line.parts {
        match part {
            Part::Text { text: value, style } => {
                text::write_span(out, value, *style, options.styled)?
            }
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
        writeln!(out, "TeX: {}", sources.join("  "))?;
    }
    Ok(())
}
