//! ANSI styling stays here; plain exports never contain styling sequences.

use std::{
    borrow::Cow,
    io::{self, Write},
};

use crate::layout::{Run, Style};

use super::theme::Theme;

pub fn write_runs(out: &mut impl Write, runs: &[Run], styled: bool) -> io::Result<()> {
    for run in runs {
        write_span(out, &run.text, run.style, styled, Theme { light: false })?;
    }
    Ok(())
}

pub(super) fn write_span(
    out: &mut impl Write,
    text: &str,
    style: Style,
    styled: bool,
    theme: Theme,
) -> io::Result<()> {
    let mut painter = Painter::new(styled, theme);
    painter.span(out, text, style)?;
    painter.finish(out)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Attributes {
    foreground: Option<u8>,
    background: Option<u8>,
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
}

/// A physical line can share unchanged ANSI state between spans. Reset before
/// leaving the line or drawing graphics so attributes never leak to other rows.
pub(super) struct Painter {
    styled: bool,
    theme: Theme,
    current: Attributes,
}

impl Painter {
    pub fn new(styled: bool, theme: Theme) -> Self {
        Self {
            styled,
            theme,
            current: Attributes::default(),
        }
    }

    fn style(&mut self, out: &mut impl Write, style: Style) -> io::Result<()> {
        if !self.styled {
            return Ok(());
        }
        let (foreground, background) = self.theme.colors(style);
        let next = Attributes {
            foreground: Some(foreground),
            background,
            bold: style.bold,
            italic: style.italic,
            strike: style.strike,
            underline: style.link,
        };
        let mut sgr = Sgr::default();
        for (old, new, on, off) in [
            (self.current.bold, next.bold, 1, 22),
            (self.current.italic, next.italic, 3, 23),
            (self.current.strike, next.strike, 9, 29),
            (self.current.underline, next.underline, 4, 24),
        ] {
            if old != new {
                sgr.push(if new { on } else { off });
            }
        }
        if self.current.foreground != next.foreground {
            sgr.push(38);
            sgr.push(5);
            sgr.push(foreground);
        }
        if self.current.background != next.background {
            if let Some(background) = background {
                sgr.push(48);
                sgr.push(5);
                sgr.push(background);
            } else {
                sgr.push(49);
            }
        }
        sgr.write(out)?;
        self.current = next;
        Ok(())
    }

    pub fn span(&mut self, out: &mut impl Write, text: &str, style: Style) -> io::Result<()> {
        if !text.is_empty() {
            self.style(out, style)?;
            write_sanitized(out, text)?;
        }
        Ok(())
    }

    pub fn spaces(&mut self, out: &mut impl Write, count: usize, style: Style) -> io::Result<()> {
        if count > 0 {
            self.style(out, style)?;
            write_spaces(out, count)?;
        }
        Ok(())
    }

    pub fn finish(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.current != Attributes::default() {
            out.write_all(b"\x1b[0m")?;
            self.current = Attributes::default();
        }
        Ok(())
    }
}

struct Sgr {
    // At most four attribute changes plus two three-number palette changes:
    // ten numbers of at most three digits, each with a separator.
    bytes: [u8; 40],
    len: usize,
}

impl Default for Sgr {
    fn default() -> Self {
        Self {
            bytes: [0; 40],
            len: 0,
        }
    }
}

impl Sgr {
    fn push(&mut self, value: u8) {
        if self.len > 0 {
            self.bytes[self.len] = b';';
            self.len += 1;
        }
        if value >= 100 {
            self.bytes[self.len] = b'0' + value / 100;
            self.len += 1;
        }
        if value >= 10 {
            self.bytes[self.len] = b'0' + value / 10 % 10;
            self.len += 1;
        }
        self.bytes[self.len] = b'0' + value % 10;
        self.len += 1;
    }

    fn write(&self, out: &mut impl Write) -> io::Result<()> {
        if self.len > 0 {
            out.write_all(b"\x1b[")?;
            out.write_all(&self.bytes[..self.len])?;
            out.write_all(b"m")?;
        }
        Ok(())
    }
}

pub(super) fn write_spaces(out: &mut impl Write, mut count: usize) -> io::Result<()> {
    const SPACES: [u8; 128] = [b' '; 128];
    while count > 0 {
        let chunk = count.min(SPACES.len());
        out.write_all(&SPACES[..chunk])?;
        count -= chunk;
    }
    Ok(())
}

fn write_sanitized(out: &mut impl Write, text: &str) -> io::Result<()> {
    let mut start = 0;
    for (offset, ch) in text.char_indices() {
        if filtered(ch) {
            out.write_all(&text.as_bytes()[start..offset])?;
            start = offset + ch.len_utf8();
        }
    }
    out.write_all(&text.as_bytes()[start..])
}

/// Document text cannot inject terminal commands. Source export is deliberately
/// separate and preserves bytes when explicitly requested.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if text.chars().any(filtered) {
        Cow::Owned(sanitize_owned(text.to_owned()))
    } else {
        Cow::Borrowed(text)
    }
}

pub(super) fn sanitize_owned(mut text: String) -> String {
    text.retain(|ch| !filtered(ch));
    text
}

fn filtered(ch: char) -> bool {
    ch.is_control() && !matches!(ch, '\n' | '\t')
}
