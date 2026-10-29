//! Conservative terminal discovery without reading input or sending queries.
//! Automatic graphics selection is limited to known Kitty/Ghostty hints;
//! callers can explicitly choose Kitty graphics for other compatible terminals.

use std::io::{self, IsTerminal};

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub columns: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            columns: 80,
            rows: 24,
            cell_width: 10,
            cell_height: 20,
        }
    }
}

#[derive(Debug)]
pub struct Terminal {
    pub interactive: bool,
    pub styled: bool,
    pub multiplexer: bool,
    pub geometry: Geometry,
    pub kitty_hint: bool,
    pub graphics_allowed: bool,
}

impl Terminal {
    pub fn detect() -> Self {
        let interactive = io::stdout().is_terminal();
        let term = std::env::var("TERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let multiplexer = ["TMUX", "STY", "ZELLIJ"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()));
        let geometry = geometry().unwrap_or_default();
        let styled = interactive
            && term != "dumb"
            && std::env::var_os("NO_COLOR").is_none_or(|s| s.is_empty());
        Self {
            interactive,
            styled,
            multiplexer,
            geometry,
            kitty_hint: kitty_hint(&term, &program),
            graphics_allowed: interactive
                && !multiplexer
                && term != "dumb"
                && geometry.columns >= 4
                && geometry.rows >= 5,
        }
    }
}

fn kitty_hint(term: &str, program: &str) -> bool {
    matches!(term, "xterm-kitty" | "xterm-ghostty") || matches!(program, "kitty" | "ghostty")
}

#[cfg(unix)]
fn geometry() -> Option<Geometry> {
    use std::os::fd::AsRawFd;
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: TIOCGWINSZ writes one winsize into valid, correctly sized storage;
    // stdout's borrowed fd remains open for the duration of the call.
    if unsafe {
        libc::ioctl(
            io::stdout().as_raw_fd(),
            libc::TIOCGWINSZ,
            size.as_mut_ptr(),
        )
    } != 0
    {
        return None;
    }
    // SAFETY: ioctl succeeded and initialized the zeroed winsize storage.
    let size = unsafe { size.assume_init() };
    if size.ws_col == 0 || size.ws_row == 0 {
        return None;
    }
    let defaults = Geometry::default();
    Some(
        Geometry {
            columns: size.ws_col,
            rows: size.ws_row,
            cell_width: (size.ws_xpixel / size.ws_col).max(1),
            cell_height: (size.ws_ypixel / size.ws_row).max(1),
        }
        .with_cell_fallback(size.ws_xpixel == 0, size.ws_ypixel == 0, defaults),
    )
}

#[cfg(unix)]
impl Geometry {
    fn with_cell_fallback(mut self, width: bool, height: bool, defaults: Self) -> Self {
        if width {
            self.cell_width = defaults.cell_width;
        }
        if height {
            self.cell_height = defaults.cell_height;
        }
        self
    }
}

#[cfg(not(unix))]
fn geometry() -> Option<Geometry> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_graphics_requires_a_known_terminal_hint() {
        assert!(kitty_hint("xterm-ghostty", ""));
        assert!(kitty_hint("xterm-256color", "ghostty"));
        assert!(kitty_hint("xterm-kitty", ""));
        assert!(!kitty_hint("xterm-256color", ""));
        assert!(!kitty_hint("xterm-256color", "Apple_Terminal"));
        assert!(!kitty_hint("screen-256color", ""));
    }
}
