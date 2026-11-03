use std::{
    io::{self, Write},
    sync::atomic::AtomicBool,
};

use super::{Options, inline};
use crate::{
    images::Loader,
    layout::{Role, Run, Style},
    mermaid::{Engine, MAX_SOURCE},
};

pub(super) fn write(
    out: &mut impl Write,
    runs: &[Run],
    engine: &mut Option<Engine>,
    images: &mut Option<Loader>,
    options: &Options,
    cancel: &AtomicBool,
) -> io::Result<bool> {
    let code = || runs.iter().filter(|run| run.style.role == Role::CodeBlock);
    let Some(length) = code().try_fold(0usize, |n, run| n.checked_add(run.text.len())) else {
        return Ok(false);
    };
    if length == 0 || length > MAX_SOURCE {
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
    let bounds = (
        frame.width() as u32 * u32::from(geometry.cell_width.max(1)),
        document_graphics::MAX_EDGE,
    );
    let Some(pixels) = engine.get_or_insert_with(Engine::default).render(
        &source,
        bounds,
        options.light_background,
        cancel,
    )?
    else {
        return Ok(false);
    };
    let images = images.get_or_insert_with(Loader::default);
    if !inline::write_picture(out, pixels, &frame, options, images)? {
        return Ok(false);
    }
    if options.show_mermaid_source {
        // The image consumed the list bullet; retain indentation and quote rails.
        let source: Vec<_> = code().cloned().collect();
        inline::write(out, &source, &mut None, options, &mut Vec::new(), (0, 0))?;
    }
    Ok(true)
}
