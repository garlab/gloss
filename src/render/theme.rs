//! Terminal colors are presentation choices, separate from document semantics.

use katex_native::Color;

use crate::layout::{Role, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub light: bool,
}

impl Theme {
    pub fn colors(self, style: Style) -> (u8, Option<u8>) {
        let body = if self.light { 238 } else { 252 };
        let muted = if self.light { 243 } else { 245 };
        let panel = if self.light { 254 } else { 236 };
        let accent = if self.light { 25 } else { 81 };
        let mut colors = match style.role {
            Role::Heading(1) => (231, Some(if self.light { 61 } else { 63 })),
            Role::Heading(2) => (accent, None),
            Role::Heading(_) => (if self.light { 61 } else { 147 }, None),
            Role::CodeBlock => (body, Some(panel)),
            Role::ListMarker => (if self.light { 29 } else { 114 }, None),
            Role::QuoteMarker | Role::Rule => (if self.light { 245 } else { 240 }, None),
            Role::LinkTarget | Role::MathSource => (muted, None),
            Role::Body if style.quote_depth > 0 => (muted, None),
            Role::Body => (body, None),
        };
        if style.link {
            colors.0 = if self.light { 29 } else { 86 };
        }
        if style.code && style.role != Role::CodeBlock {
            colors = (if self.light { 130 } else { 210 }, Some(panel));
        }
        colors
    }

    pub fn ink(self, style: Style) -> Color {
        let (index, _) = self.colors(Style {
            code: false,
            ..style
        });
        let (r, g, b) = if index >= 232 {
            let channel = 8 + 10 * (index - 232);
            (channel, channel, channel)
        } else {
            let index = index.saturating_sub(16);
            let levels = [0, 95, 135, 175, 215, 255];
            (
                levels[usize::from(index / 36)],
                levels[usize::from(index / 6 % 6)],
                levels[usize::from(index % 6)],
            )
        };
        Color { r, g, b, a: 255 }
    }
}
