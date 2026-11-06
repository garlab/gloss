//! Output backends consume the same layout. Presentation never changes the
//! original document or the independent text export.

mod code;
mod diagram;
mod inline;
pub mod kitty;
mod table;
pub mod text;
mod theme;
mod viewport;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    document::Document,
    layout::{Block, Layout, Role, Run, Style},
    math::MathEngine,
    terminal::Geometry,
};

use theme::Theme;
use viewport::Viewport;

#[derive(Clone)]
pub struct Options {
    pub styled: bool,
    pub links: bool,
    /// Color known fenced languages; ignored for unstyled/plain output.
    pub highlight: bool,
    /// Number logical code lines in terminal output; independent of syntax colors.
    pub line_numbers: bool,
    pub kitty: Option<Geometry>,
    /// Terminal geometry for formatted text, including sessions without graphics.
    pub terminal: Option<Geometry>,
    /// Maximum content columns, excluding margins; zero uses available space.
    pub width: u16,
    pub margin: u16,
    pub font_size: f64,
    pub light_background: bool,
    pub show_math_source: bool,
    /// Enable local images, resolving relative references against this directory.
    /// None bypasses all image file access and decoding.
    pub image_base: Option<PathBuf>,
    /// Render supported Mermaid fences with the bundled native worker.
    pub mermaid: bool,
    /// Keep a selectable code panel below rendered diagrams.
    pub show_mermaid_source: bool,
    pub sgf: bool,
    pub sgf_selection: crate::sgf::Selection,
    pub show_sgf_source: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            styled: false,
            links: false,
            highlight: true,
            line_numbers: false,
            kitty: None,
            terminal: None,
            width: 80,
            margin: 2,
            font_size: 24.,
            light_background: false,
            show_math_source: false,
            image_base: None,
            mermaid: false,
            show_mermaid_source: false,
            sgf: false,
            sgf_selection: Default::default(),
            show_sgf_source: false,
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

/// Bounded math/diagram caches survive pager reloads; file-image loaders and
/// per-document work/output budgets are deliberately fresh on every render.
#[derive(Default)]
pub(crate) struct RenderCache {
    math: Option<MathEngine>,
    mermaid: Option<crate::mermaid::Engine>,
    sgf: Option<crate::sgf::Engine>,
}

impl RenderCache {
    fn begin_document(&mut self) {
        if let Some(math) = &mut self.math {
            math.begin_document();
        }
        if let Some(mermaid) = &mut self.mermaid {
            mermaid.begin_document();
        }
        if let Some(sgf) = &mut self.sgf {
            sgf.begin_document();
        }
    }

    fn finish_document(&mut self) {
        if let Some(mermaid) = &mut self.mermaid {
            mermaid.finish_document();
        }
    }
}

/// Render a retained layout, returning non-fatal math warnings.
pub fn write(
    out: &mut impl Write,
    document: &Document,
    layout: &Layout,
    options: &Options,
) -> io::Result<Vec<String>> {
    let mut cache = RenderCache::default();
    let cancel = AtomicBool::new(false);
    let mut renderer = Renderer::new(out, document, options, &mut cache, &cancel);
    for (index, block) in layout.blocks.iter().enumerate() {
        let before = index
            .checked_sub(1)
            .and_then(|index| layout.blocks.get(index));
        renderer.block(
            block,
            (
                quote(before, false),
                quote(layout.blocks.get(index + 1), true),
            ),
        )?;
    }
    Ok(renderer.warnings)
}

/// The CLI needs only a completed block and one-block lookahead for quote rails.
/// Highlighters, raster caches, and the diagram worker are reused across blocks.
pub fn write_document(
    out: &mut impl Write,
    document: &Document,
    options: &Options,
) -> io::Result<Vec<String>> {
    write_document_with(
        out,
        document,
        options,
        &mut RenderCache::default(),
        &AtomicBool::new(false),
        |_, _| Ok(()),
    )
}

/// A recording backend can associate rendered rows with their original block.
/// The ordinary streaming path supplies a no-op observer.
pub(crate) fn write_document_with<W: Write>(
    out: &mut W,
    document: &Document,
    options: &Options,
    cache: &mut RenderCache,
    cancel: &AtomicBool,
    mut before_block: impl FnMut(&mut W, &Block) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    cache.begin_document();
    let result = (|| {
        let mut renderer = Renderer::new(out, document, options, cache, cancel);
        let mut pending = None;
        let mut previous = 0;
        crate::layout::visit(
            document,
            true,
            options.image_base.is_some(),
            options.links,
            |next| {
                if let Some(block) = pending.take() {
                    before_block(renderer.out, &block)?;
                    renderer.block(&block, (previous, quote(Some(&next), true)))?;
                    previous = quote(Some(&block), false);
                }
                pending = Some(next);
                Ok::<_, io::Error>(())
            },
        )?;
        if let Some(block) = pending {
            before_block(renderer.out, &block)?;
            renderer.block(&block, (previous, 0))?;
        }
        Ok(renderer.warnings)
    })();
    cache.finish_document();
    result
}

fn quote(block: Option<&Block>, first: bool) -> usize {
    match block {
        Some(Block::Text(runs) | Block::Code { runs, .. }) => {
            let mut styles = runs.iter().filter(|run| {
                !run.text.trim().is_empty()
                    && !matches!(run.style.role, Role::ListMarker | Role::QuoteMarker)
            });
            (if first {
                styles.next()
            } else {
                styles.next_back()
            })
            .map_or(0, |run| run.style.quote_depth)
        }
        Some(Block::Table(table)) => table.style.quote_depth,
        Some(Block::Image(image)) => image.style.quote_depth,
        Some(Block::DisplayMath { style, .. }) => style.quote_depth,
        None => 0,
    }
}

struct Renderer<'a, W> {
    out: &'a mut W,
    document: &'a Document,
    options: &'a Options,
    cache: &'a mut RenderCache,
    cancel: &'a AtomicBool,
    highlighter: Option<crate::highlight::Highlighter>,
    images: Option<crate::images::Loader>,
    warnings: Vec<String>,
    started: bool,
}

impl<'a, W: Write> Renderer<'a, W> {
    fn new(
        out: &'a mut W,
        document: &'a Document,
        options: &'a Options,
        cache: &'a mut RenderCache,
        cancel: &'a AtomicBool,
    ) -> Self {
        Self {
            out,
            document,
            options,
            cache,
            cancel,
            highlighter: None,
            images: None,
            warnings: Vec::new(),
            started: false,
        }
    }

    fn block(&mut self, block: &Block, edges: (usize, usize)) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let options = self.options;
        let document = self.document;
        let out = &mut self.out;
        let engine = &mut self.cache.math;
        let highlighter = &mut self.highlighter;
        let images = &mut self.images;
        let mermaid = &mut self.cache.mermaid;
        let sgf = &mut self.cache.sgf;
        let warnings = &mut self.warnings;
        let formatted = options.terminal.is_some() || options.kitty.is_some() || options.styled;
        let geometry = options.geometry();
        let viewport = options.viewport();
        if !self.started && formatted {
            if options.styled {
                out.write_all(b"\x1b[0m")?;
            }
            writeln!(out)?;
        }
        self.started = true;
        match block {
            Block::Text(runs) => {
                if formatted {
                    inline::write(out, runs, engine, options, warnings, edges)?;
                } else {
                    text::write_runs(out, runs, false)?;
                }
            }
            Block::Code {
                language,
                attributes,
                runs,
                ..
            } => {
                if formatted {
                    if options.mermaid
                        && language
                            .as_deref()
                            .is_some_and(|label| label.eq_ignore_ascii_case("mermaid"))
                        && diagram::write(
                            out,
                            runs,
                            diagram::Kind::Mermaid(mermaid),
                            images,
                            options,
                            self.cancel,
                        )?
                    {
                        return Ok(());
                    }
                    if options.sgf
                        && language
                            .as_deref()
                            .is_some_and(|label| label.eq_ignore_ascii_case("sgf"))
                        && let Some(selection) =
                            options.sgf_selection.attributes(attributes.as_deref())
                        && diagram::write(
                            out,
                            runs,
                            diagram::Kind::Sgf(sgf, selection),
                            images,
                            options,
                            self.cancel,
                        )?
                    {
                        return Ok(());
                    }
                    let highlighted = if options.styled && options.highlight {
                        code::highlight(runs, language.as_deref(), highlighter)
                    } else {
                        None
                    };
                    inline::write(
                        out,
                        highlighted.as_deref().unwrap_or(runs),
                        engine,
                        options,
                        warnings,
                        (0, 0),
                    )?;
                } else {
                    text::write_runs(out, runs, false)?;
                }
            }
            Block::Table(table) => {
                if formatted {
                    table::write(out, table, engine, options, warnings)?;
                } else {
                    table::write_plain(out, table)?;
                }
            }
            Block::Image(image) => {
                let frame = inline::Frame::for_block(&image.prefix, image.style, options);
                let mut rendered = false;
                if formatted && let Some(base) = &options.image_base {
                    let loader = images.get_or_insert_with(crate::images::Loader::default);
                    let width = frame.width() as u32 * u32::from(geometry.cell_width.max(1));
                    let height = u32::from(geometry.rows.saturating_sub(2).max(1))
                        * u32::from(geometry.cell_height.max(1));
                    let pixels = if let Some(path) = document.image_path() {
                        loader.load_path(path, width, height)
                    } else {
                        loader.load(base, &image.target, width, height)
                    };
                    if let Ok(pixels) = pixels {
                        rendered = inline::write_picture(out, pixels, &frame, options, loader)?;
                    }
                }
                let caption = Run {
                    text: format!("[image: {}] ({})", image.alt, image.target),
                    style: Style {
                        role: Role::LinkTarget,
                        code: false,
                        link: false,
                        ..image.style
                    },
                    math: None,
                    target: None,
                    source: Some(image.source.clone().into()),
                };
                if formatted {
                    let frame = if rendered { frame.continued() } else { frame };
                    inline::write_cell(out, &[caption], &frame, options, false)?;
                } else {
                    text::write_runs(out, &[caption], false)?;
                    writeln!(out)?;
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
        Ok(())
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn cached_reloads_match_fresh_rendering_after_edits_and_reflow() {
        let mut cache = RenderCache::default();
        let equation = "$$\\begin{equation}x=1\\end{equation}$$\n\n";
        let sources = [
            format!("Before $x^2$.\n\n{equation}{equation}"),
            format!("Changed $x^2$.\n\n{equation}"),
            format!("{equation}After $x^2$.\n\n{equation}"),
        ];
        for (width, light) in [(80, false), (40, false), (40, true), (80, false)] {
            let options = Options {
                kitty: Some(Geometry::default()),
                styled: true,
                width,
                light_background: light,
                ..Default::default()
            };
            for source in &sources {
                let document = Document::parse(source.clone());
                let mut cached = Vec::new();
                let mut fresh = Vec::new();
                let warnings = write_document_with(
                    &mut cached,
                    &document,
                    &options,
                    &mut cache,
                    &AtomicBool::new(false),
                    |_, _| Ok(()),
                )
                .unwrap();
                assert!(warnings.is_empty());
                write_document(&mut fresh, &document, &options).unwrap();
                assert_eq!(cached, fresh);
            }
        }
    }
}
