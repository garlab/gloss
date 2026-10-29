//! Output backends consume the same layout. Presentation never changes the
//! original document or the independent text export.

mod code;
mod inline;
pub mod kitty;
pub mod text;
mod theme;
mod viewport;

use std::io::{self, Write};

use crate::{
    document::Document,
    layout::{Block, Layout, Role, Style},
    math::MathEngine,
    terminal::Geometry,
};

use theme::Theme;
use viewport::Viewport;

pub struct Options {
    pub styled: bool,
    /// Color known fenced languages; ignored for unstyled/plain output.
    pub highlight: bool,
    pub kitty: Option<Geometry>,
    /// Terminal geometry for formatted text, including sessions without graphics.
    pub terminal: Option<Geometry>,
    /// Maximum content columns, excluding margins; zero uses available space.
    pub width: u16,
    pub margin: u16,
    pub font_size: f64,
    pub light_background: bool,
    pub show_math_source: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            styled: false,
            highlight: true,
            kitty: None,
            terminal: None,
            width: 80,
            margin: 2,
            font_size: 24.,
            light_background: false,
            show_math_source: false,
        }
    }
}

impl Options {
    fn theme(&self) -> Theme {
        Theme {
            light: self.light_background,
        }
    }

    fn geometry(&self) -> Geometry {
        self.terminal.or(self.kitty).unwrap_or_default()
    }

    fn viewport(&self) -> Viewport {
        Viewport::new(self.geometry(), self.width, self.margin)
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
    let mut highlighter = None;
    let mut warnings = Vec::new();
    let formatted = options.terminal.is_some() || options.kitty.is_some() || options.styled;
    let geometry = options.geometry();
    let viewport = options.viewport();
    if formatted && !layout.blocks.is_empty() {
        if options.styled {
            out.write_all(b"\x1b[0m")?;
        }
        writeln!(out)?;
    }
    for (index, block) in layout.blocks.iter().enumerate() {
        match block {
            Block::Text(runs) => {
                if formatted {
                    let code_quote = |block: Option<&Block>| match block {
                        Some(Block::Code { runs, .. }) => runs
                            .iter()
                            .find(|run| run.style.role == Role::CodeBlock)
                            .map_or(0, |run| run.style.quote_depth),
                        _ => 0,
                    };
                    let edges = (
                        code_quote(
                            index
                                .checked_sub(1)
                                .and_then(|previous| layout.blocks.get(previous)),
                        ),
                        code_quote(layout.blocks.get(index + 1)),
                    );
                    inline::write(out, runs, &mut engine, options, &mut warnings, edges)?;
                } else {
                    text::write_runs(out, runs, false)?;
                }
            }
            Block::Code { language, runs, .. } => {
                if formatted {
                    let highlighted = if options.styled && options.highlight {
                        code::highlight(runs, language.as_deref(), &mut highlighter)
                    } else {
                        None
                    };
                    inline::write(
                        out,
                        highlighted.as_deref().unwrap_or(runs),
                        &mut engine,
                        options,
                        &mut warnings,
                        (0, 0),
                    )?;
                } else {
                    text::write_runs(out, runs, false)?;
                }
            }
            Block::DisplayMath { tex, source, style } => {
                let original = document.source_slice(source.clone()).unwrap_or_default();
                let mut rendered = false;
                if options.kitty.is_some() {
                    let inset = (style.quote_depth.saturating_mul(2) + style.indent)
                        .min(viewport.width.saturating_sub(4));
                    let content_width = viewport.width - inset;
                    let width = content_width as u32 * u32::from(geometry.cell_width.max(1));
                    let height = u32::from(geometry.rows.saturating_sub(2).max(1))
                        * u32::from(geometry.cell_height.max(1));
                    let math = engine.get_or_insert_with(MathEngine::default);
                    match math.render_display(
                        tex,
                        options.font_size,
                        options.theme().ink(*style),
                        width,
                        height,
                    ) {
                        Ok(raster) => {
                            kitty::write_display(
                                out,
                                &raster,
                                geometry,
                                viewport.left + inset,
                                content_width,
                            )?;
                            rendered = true;
                        }
                        Err(error) => warnings.push(format!(
                            "math at byte {}: {error}; showing TeX",
                            source.start
                        )),
                    }
                }
                if !rendered || options.show_math_source {
                    let mut value = text::sanitize(original).into_owned();
                    if !value.ends_with('\n') {
                        value.push('\n');
                    }
                    value.push('\n');
                    if formatted {
                        inline::write_text(
                            out,
                            &value,
                            Style {
                                role: Role::MathSource,
                                ..*style
                            },
                            options,
                        )?;
                    } else {
                        write!(out, "{value}")?;
                    }
                }
            }
        }
    }
    Ok(warnings)
}
