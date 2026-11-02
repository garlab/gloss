//! Conservative terminal discovery without reading input or sending queries.
//! Automatic graphics selection is limited to known Kitty/Ghostty hints;
//! callers can explicitly choose Kitty graphics for other compatible terminals.

use std::io::{self, IsTerminal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub links_allowed: bool,
    pub light_background: bool,
}

impl Terminal {
    pub fn detect() -> Self {
        let interactive = io::stdout().is_terminal();
        let term = std::env::var("TERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let multiplexer = ["TMUX", "STY", "ZELLIJ"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()));
        let geometry = geometry().unwrap_or_else(|| Geometry {
            columns: dimension("COLUMNS").unwrap_or(80),
            rows: dimension("LINES").unwrap_or(24),
            ..Geometry::default()
        });
        let styled = interactive
            && term != "dumb"
            && std::env::var_os("NO_COLOR").is_none_or(|s| s.is_empty());
        Self {
            interactive,
            links_allowed: interactive && !multiplexer && term != "dumb",
            light_background: std::env::var("COLORFGBG")
                .ok()
                .and_then(|value| light_background(&value))
                .unwrap_or(false),
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

fn dimension(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()?
        .parse::<u16>()
        .ok()
        .filter(|&n| n > 0)
}

/// COLORFGBG is an optional terminal hint, not a query of the actual palette.
/// Explicit theme selection takes precedence; unknown/missing hints stay dark.
fn light_background(value: &str) -> Option<bool> {
    if value.len() > 32 {
        return None;
    }
    let mut parts = value.split(';');
    parts.next()?.parse::<u8>().ok()?;
    let second = parts.next()?;
    let background = match parts.next() {
        Some(last) if parts.next().is_none() => last,
        Some(_) => return None,
        None => second,
    }
    .parse::<u8>()
    .ok()?;
    let (r, g, b) = match background {
        0..=15 => {
            const BASIC: [(u8, u8, u8); 16] = [
                (0, 0, 0),
                (128, 0, 0),
                (0, 128, 0),
                (128, 128, 0),
                (0, 0, 128),
                (128, 0, 128),
                (0, 128, 128),
                (192, 192, 192),
                (128, 128, 128),
                (255, 0, 0),
                (0, 255, 0),
                (255, 255, 0),
                (0, 0, 255),
                (255, 0, 255),
                (0, 255, 255),
                (255, 255, 255),
            ];
            BASIC[background as usize]
        }
        16..=231 => {
            let index = background - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            (
                levels[(index / 36) as usize],
                levels[(index / 6 % 6) as usize],
                levels[(index % 6) as usize],
            )
        }
        _ => {
            let level = 8 + 10 * (background - 232);
            (level, level, level)
        }
    };
    Some((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) >= 128_000)
}

/// Explicit pixel geometry for terminals that omit pixel dimensions in winsize.
pub fn cell_size(value: &str) -> Result<(u16, u16), String> {
    let parse = || {
        let (width, height) = value.split_once('x')?;
        let width = width
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=512).contains(n))?;
        let height = height
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=512).contains(n))?;
        Some((width, height))
    };
    parse().ok_or_else(|| "use WIDTHxHEIGHT in pixels, each between 1 and 512".into())
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
    let defaults = Geometry::default();
    let columns = if size.ws_col > 0 {
        size.ws_col
    } else {
        dimension("COLUMNS").unwrap_or(defaults.columns)
    };
    let rows = if size.ws_row > 0 {
        size.ws_row
    } else {
        dimension("LINES").unwrap_or(defaults.rows)
    };
    Some(
        Geometry {
            columns,
            rows,
            cell_width: (size.ws_xpixel / columns).max(1),
            cell_height: (size.ws_ypixel / rows).max(1),
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
    fn theme_hints_and_cell_overrides_are_validated() {
        for value in ["0;15", "0;default;15", "0;255", "0;7"] {
            assert_eq!(light_background(value), Some(true));
        }
        for value in ["15;0", "15;232", "7;4"] {
            assert_eq!(light_background(value), Some(false));
        }
        for value in ["", "15", "unknown;15", "15;-1", "0;256", "0;1;2;3"] {
            assert!(light_background(value).is_none());
        }
        assert_eq!(cell_size("12x24").unwrap(), (12, 24));
        for value in ["0x20", "10x0", "513x20", "12,24", "-1x2", "12x24x1"] {
            assert!(cell_size(value).is_err());
        }
    }

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
