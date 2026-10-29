//! Output backends consume the same layout. Graphics are a presentation detail;
//! their presence never changes the original document or text export.

mod inline;
pub mod kitty;
pub mod text;

use std::io::{self, Write};

use katex_native::Color;

use crate::{
    document::Document,
    layout::{Block, Layout},
    math::MathEngine,
    terminal::Geometry,
};

pub struct Options {
    pub styled: bool,
    pub kitty: Option<Geometry>,
    pub font_size: f64,
    pub light_background: bool,
    pub show_math_source: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            styled: false,
            kitty: None,
            font_size: 24.,
            light_background: false,
            show_math_source: false,
        }
    }
}

impl Options {
    fn ink(&self) -> Color {
        let channel = if self.light_background { 25 } else { 230 };
        Color {
            r: channel,
            g: channel,
            b: channel,
            a: 255,
        }
    }
}

/// Returns non-fatal math errors. Failed equations are shown as their source.
pub fn write(
    out: &mut impl Write,
    document: &Document,
    layout: &Layout,
    options: &Options,
) -> io::Result<Vec<String>> {
    let mut engine = None;
    let mut warnings = Vec::new();
    for block in &layout.blocks {
        match block {
            Block::Text(runs) => {
                if let Some(geometry) = options.kitty {
                    inline::write(out, runs, &mut engine, geometry, options, &mut warnings)?;
                } else {
                    text::write_runs(out, runs, options.styled)?;
                }
            }
            Block::DisplayMath { tex, source } => {
                let original = document.source_slice(source.clone()).unwrap_or_default();
                let mut rendered = false;
                if let Some(geometry) = options.kitty {
                    let width = u32::from(geometry.columns.saturating_sub(1).max(1))
                        * u32::from(geometry.cell_width.max(1));
                    let height = u32::from(geometry.rows.saturating_sub(2).max(1))
                        * u32::from(geometry.cell_height.max(1));
                    let math = engine.get_or_insert_with(MathEngine::default);
                    match math.render_display(tex, options.font_size, options.ink(), width, height)
                    {
                        Ok(raster) => {
                            kitty::write_display(out, &raster, geometry)?;
                            rendered = true;
                        }
                        Err(error) => warnings.push(format!(
                            "math at byte {}: {error}; showing TeX",
                            source.start
                        )),
                    }
                }
                if !rendered || options.show_math_source {
                    write!(out, "{}", text::sanitize(original))?;
                    if !original.ends_with('\n') {
                        writeln!(out)?;
                    }
                    writeln!(out)?;
                }
            }
        }
    }
    Ok(warnings)
}
