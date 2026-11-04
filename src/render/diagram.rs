use std::{
    io::{self, Write},
    sync::atomic::AtomicBool,
};

use super::{Options, inline};
use crate::{
    images::Loader,
    layout::{Role, Run, Style},
    mermaid::Engine,
    sgf,
};

pub(super) enum Kind<'a> {
    Mermaid(&'a mut Option<Engine>),
    Sgf(&'a mut Option<sgf::Engine>, sgf::Selection),
}

pub(super) fn write(
    out: &mut impl Write,
    runs: &[Run],
    kind: Kind<'_>,
    images: &mut Option<Loader>,
    options: &Options,
    cancel: &AtomicBool,
) -> io::Result<bool> {
    let code = || runs.iter().filter(|run| run.style.role == Role::CodeBlock);
    let Some(length) = code().try_fold(0usize, |n, run| n.checked_add(run.text.len())) else {
        return Ok(false);
    };
    let limit = match &kind {
        Kind::Mermaid(_) => crate::mermaid::MAX_SOURCE,
        Kind::Sgf(..) => sgf::MAX_SOURCE,
    };
    if length == 0 || length > limit {
        return Ok(false);
    }
    let mut source = String::with_capacity(length);
    for run in code() {
        source.push_str(&run.text);
    }
    let style = Style {
        role: Role::Body,
        code: false,
        bold: false,
        ..code().next().unwrap().style
    };
    let prefix: Vec<_> = runs
        .iter()
        .take_while(|run| run.style.role != Role::CodeBlock)
        .cloned()
        .collect();
    let frame = inline::Frame::for_block(&prefix, style, options);
    let geometry = options.geometry();
    let height = match &kind {
        Kind::Mermaid(_) => document_graphics::MAX_EDGE,
        Kind::Sgf(..) => {
            u32::from(geometry.rows.saturating_sub(2).max(1))
                * u32::from(geometry.cell_height.max(1))
        }
    };
    let bounds = (
        frame.width() as u32 * u32::from(geometry.cell_width.max(1)),
        height,
    );
    // A 16-unit Mermaid label occupies 80% of a terminal row. Scale with physical
    // cell pixels so Retina text is readable; width and raster caps still apply.
    let scale = (f64::from(geometry.cell_height.max(1)) / 20.).clamp(0.25, 4.);
    let (rendered, show_source) = match kind {
        Kind::Mermaid(engine) => (
            engine
                .get_or_insert_with(Engine::default)
                .render(&source, bounds, scale, options.light_background, cancel)?
                .map(|pixels| (pixels, None)),
            options.show_mermaid_source,
        ),
        Kind::Sgf(engine, selection) => (
            engine
                .get_or_insert_with(sgf::Engine::default)
                .render(
                    &source,
                    selection,
                    document_graphics::RasterOptions { bounds, scale },
                    options.light_background,
                    cancel,
                )?
                .map(|p| (p.pixels.clone(), Some(p.caption.clone()))),
            options.show_sgf_source,
        ),
    };
    let Some((pixels, caption)) = rendered else {
        return Ok(false);
    };
    let images = images.get_or_insert_with(Loader::default);
    if !inline::write_picture(out, pixels, &frame, options, images)? {
        return Ok(false);
    }
    if let Some(caption) = caption {
        inline::write_cell(
            out,
            &[Run {
                text: caption,
                style: Style {
                    role: Role::LinkTarget,
                    ..style
                },
                math: None,
                target: None,
                source: code().next().and_then(|run| run.source),
            }],
            &frame.continued(),
            options,
            false,
        )?;
    }
    if show_source {
        // The image consumed the list bullet; retain indentation and quote rails.
        let source: Vec<_> = code().cloned().collect();
        inline::write(out, &source, &mut None, options, &mut Vec::new(), (0, 0))?;
    }
    Ok(true)
}
