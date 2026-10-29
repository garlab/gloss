//! ANSI styling stays here; plain exports never contain styling sequences.

use std::io::{self, Write};

use crate::layout::{Run, Style};

pub fn write_runs(out: &mut impl Write, runs: &[Run], styled: bool) -> io::Result<()> {
    for run in runs {
        write_span(out, &run.text, run.style, styled)?;
    }
    Ok(())
}

pub fn write_span(out: &mut impl Write, text: &str, style: Style, styled: bool) -> io::Result<()> {
    let mut attributes = Vec::new();
    if style.bold {
        attributes.push("1");
    }
    if style.italic {
        attributes.push("3");
    }
    if style.strike {
        attributes.push("9");
    }
    if style.code {
        attributes.push("36");
    }
    if style.link {
        attributes.push("4");
    }
    let decorated = styled && !attributes.is_empty();
    if decorated {
        write!(out, "\x1b[{}m", attributes.join(";"))?;
    }
    write!(out, "{}", sanitize(text))?;
    if decorated {
        write!(out, "\x1b[0m")?;
    }
    Ok(())
}

/// Document text cannot inject terminal commands. Source export is deliberately
/// separate and preserves bytes when explicitly requested.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}
