//! Opt-in interactive viewing. Direct rendering and exports never enter here.
mod record;
mod tty;
mod watch;
mod worker;

pub use crate::input::{read, read_file};
use crate::{render, terminal::Geometry};
use record::Page;
use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tty::{Key, Session};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use worker::{Request, Worker};

const HELP: &str =
    "j/k scroll | space/b page | g/G ends | /? search | n/N next | r reload | w warnings | q quit";

fn safe(value: &str, width: usize) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    let mut used = 0;
    clean
        .graphemes(true)
        .take_while(|s| {
            used += s.width();
            used <= width
        })
        .collect()
}
fn prepare(mut options: render::Options) -> (Geometry, render::Options) {
    let mut screen = options.terminal.unwrap_or_default();
    screen.columns = screen.columns.clamp(1, 4096);
    screen.rows = screen.rows.clamp(1, 4096);
    let content = Geometry {
        rows: screen.rows.saturating_sub(1).max(1),
        ..screen
    };
    options.terminal = Some(content);
    options.kitty = options.kitty.map(|_| content);
    if screen.rows < 5 || screen.columns < 4 {
        options.kitty = None;
        options.image_base = None;
        options.mermaid = false;
        options.sgf = false;
    }
    (screen, options)
}
fn max_top(page: &Page, screen: Geometry) -> usize {
    page.rows
        .len()
        .saturating_sub(usize::from(screen.rows.saturating_sub(1)))
}

/// Relocate a source anchor past insertions/deletions outside the edited region.
fn relocate(old: &str, new: &str, anchor: u32) -> u32 {
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    if anchor as usize <= prefix {
        return anchor;
    }
    let suffix = old.as_bytes()[prefix..]
        .iter()
        .rev()
        .zip(new.as_bytes()[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    if anchor as usize >= old.len() - suffix {
        (new.len() - (old.len() - anchor as usize)) as u32
    } else {
        prefix as u32
    }
}

struct Graphics {
    uploaded: BTreeMap<usize, u32>,
    next: u32,
}
impl Graphics {
    fn new() -> Self {
        Self {
            uploaded: BTreeMap::new(),
            next: (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u32
                ^ std::process::id())
            .max(1),
        }
    }
    fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        for number in self.uploaded.values() {
            write!(out, "\x1b_Ga=d,d=N,I={number},q=2;\x1b\\")?;
        }
        self.uploaded.clear();
        Ok(())
    }
    fn paint(
        &mut self,
        page: &mut Page,
        top: usize,
        height: usize,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let start = page
            .pictures
            .partition_point(|p| p.row < top.saturating_sub(page.max_image_rows));
        let visible: Vec<usize> = (start..page.pictures.len())
            .take_while(|&i| page.pictures[i].row < top + height)
            .filter(|&i| page.pictures[i].row + page.pictures[i].rows as usize > top)
            .collect();
        // Bound the terminal's decoded image cache as well as our compressed spool.
        let bytes: u64 = visible
            .iter()
            .map(|&i| u64::from(page.pictures[i].width) * u64::from(page.pictures[i].height) * 4)
            .sum();
        if bytes > 64 * 1024 * 1024 {
            return Err(io::Error::other(
                "visible pager graphics exceed 64 MiB; reduce --cell-size or use text rendering",
            ));
        }
        for (&index, &number) in &self.uploaded {
            let delete = if visible.binary_search(&index).is_ok() {
                'n'
            } else {
                'N'
            };
            write!(out, "\x1b_Ga=d,d={delete},I={number},q=2;\x1b\\")?;
        }
        self.uploaded
            .retain(|index, _| visible.binary_search(index).is_ok());
        for index in visible {
            let number = if let Some(&number) = self.uploaded.get(&index) {
                number
            } else {
                self.next = self.next.wrapping_add(1).max(1);
                page.upload(index, self.next, out)?;
                self.uploaded.insert(index, self.next);
                self.next
            };
            let p = &page.pictures[index];
            if p.native {
                let cell_height = usize::from(page.geometry.cell_height);
                let start = p.row * cell_height + p.offset_y as usize;
                let first = start.max(top * cell_height);
                let end = (start + p.height as usize).min((top + height) * cell_height);
                let y = first - start;
                let h = end - first;
                write!(
                    out,
                    "\x1b[{};{}H\x1b_Ga=p,I={number},x=0,y={y},w={},h={h},Y={},C=1,q=2;\x1b\\",
                    first / cell_height - top + 1,
                    p.column + 1,
                    p.width,
                    first % cell_height
                )?;
                continue;
            }
            let first = p.row.max(top);
            let rows = (p.row + p.rows as usize).min(top + height) - first;
            let y = (first - p.row) * usize::from(page.geometry.cell_height);
            let h = rows * usize::from(page.geometry.cell_height);
            write!(
                out,
                "\x1b[{};{}H\x1b_Ga=p,I={number},x=0,y={y},w={},h={h},c={},r={rows},C=1,q=2;\x1b\\",
                first - top + 1,
                p.column + 1,
                p.width,
                p.columns
            )?;
        }
        Ok(())
    }
}

struct Search {
    next: usize,
    remaining: usize,
    forward: bool,
}
impl Search {
    fn new(top: usize, count: usize, forward: bool) -> Self {
        Self {
            next: if forward {
                (top + 1) % count
            } else {
                (top + count - 1) % count
            },
            remaining: count,
            forward,
        }
    }
    fn step(&mut self, page: &mut Page, query: &str) -> io::Result<Option<usize>> {
        let deadline = Instant::now() + Duration::from_millis(5);
        for _ in 0..512 {
            if self.remaining == 0 || Instant::now() >= deadline {
                break;
            }
            let index = self.next;
            self.remaining -= 1;
            self.next = if self.forward {
                (index + 1) % page.rows.len()
            } else {
                (index + page.rows.len() - 1) % page.rows.len()
            };
            if page.row(index)?.1.contains(query) {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }
}

struct View {
    page: Option<Page>,
    top: usize,
    screen: Geometry,
    graphics: Graphics,
    message: String,
    prompt: Option<(bool, String)>,
    query: String,
    forward: bool,
    search: Option<Search>,
    loading: bool,
    watch: bool,
    title: String,
    warning: usize,
}
impl View {
    fn draw(&mut self, options: &render::Options, tty: &mut Session) -> io::Result<()> {
        let out = tty.out();
        let height = usize::from(self.screen.rows.saturating_sub(1));
        // Erase text one row at a time: ED 2 would also evict Kitty image data.
        for row in 1..=self.screen.rows {
            write!(out, "\x1b[{row};1H\x1b[0m\x1b[2K")?;
        }
        let ready = self
            .page
            .as_ref()
            .is_some_and(|p| Some(p.geometry) == options.terminal);
        if ready {
            let page = self.page.as_mut().unwrap();
            self.top = self.top.min(max_top(page, self.screen));
            for (row, index) in (self.top..page.rows.len()).take(height).enumerate() {
                write!(out, "\x1b[{};1H", row + 1)?;
                out.write_all(&page.row(index)?.0)?;
            }
            self.graphics.paint(page, self.top, height, out)?;
        } else {
            self.graphics.clear(out)?;
        }
        let status = if let Some((forward, query)) = &self.prompt {
            format!("{}{query}_", if *forward { '/' } else { '?' })
        } else {
            let position = self.page.as_ref().map_or_else(
                || "0/0".into(),
                |p| {
                    format!(
                        "{}-{}/{}",
                        self.top + 1,
                        (self.top + height).min(p.rows.len()),
                        p.rows.len()
                    )
                },
            );
            let state = if self.loading {
                " rendering"
            } else if self.search.is_some() {
                " searching"
            } else {
                ""
            };
            let warning = self
                .page
                .as_ref()
                .filter(|p| !p.warnings.is_empty())
                .map_or(String::new(), |p| {
                    format!(" | {} warnings (w)", p.warnings.len())
                });
            let message = if self.message.is_empty() {
                format!("{} | q quit / search h help", self.title)
            } else {
                self.message.clone()
            };
            format!(
                "{position}{}{state}{warning} | {message}",
                if self.watch { " watch" } else { "" }
            )
        };
        write!(
            out,
            "\x1b[{};1H\x1b[0m{}{}\x1b[0m",
            self.screen.rows,
            if options.styled { "\x1b[7m" } else { "" },
            safe(&status, usize::from(self.screen.columns.saturating_sub(1)))
        )?;
        out.flush()
    }
    fn find(&mut self, forward: bool) {
        if self.query.is_empty() {
            self.message = "Type / or ? to search rendered text".into();
            return;
        }
        if let Some(page) = &self.page {
            self.search = Some(Search::new(self.top, page.rows.len(), forward));
            self.message.clear();
        }
    }
    /// False exits. Native mouse selection is deliberately left to the terminal.
    fn key(&mut self, key: Key) -> bool {
        if key == Key::Char('\x03') {
            return false;
        }
        if let Some((forward, query)) = &mut self.prompt {
            match key {
                Key::Escape => self.prompt = None,
                Key::Enter => {
                    self.forward = *forward;
                    if !query.is_empty() {
                        self.query = std::mem::take(query);
                    }
                    self.prompt = None;
                    self.find(self.forward);
                }
                Key::Backspace => {
                    if let Some((index, _)) = query.grapheme_indices(true).next_back() {
                        query.truncate(index);
                    }
                }
                Key::Char('\x15') => query.clear(),
                Key::Char(ch) if !ch.is_control() && query.len() + ch.len_utf8() <= 256 => {
                    query.push(ch)
                }
                _ => {}
            }
            return true;
        }
        let height = usize::from(self.screen.rows.saturating_sub(1)).max(1);
        let end = self.page.as_ref().map_or(0, |p| max_top(p, self.screen));
        self.search = None;
        match key {
            Key::Char('q') | Key::Escape => return false,
            Key::Down | Key::Enter | Key::Char('j' | '\x0e') => self.top = (self.top + 1).min(end),
            Key::Up | Key::Char('k' | '\x10') => self.top = self.top.saturating_sub(1),
            Key::PageDown | Key::Char(' ' | 'f' | '\x06') => {
                self.top = (self.top + height.saturating_sub(1).max(1)).min(end)
            }
            Key::PageUp | Key::Char('b' | '\x02') => {
                self.top = self.top.saturating_sub(height.saturating_sub(1).max(1))
            }
            Key::Char('d' | '\x04') => self.top = (self.top + (height / 2).max(1)).min(end),
            Key::Char('u' | '\x15') => self.top = self.top.saturating_sub((height / 2).max(1)),
            Key::Home | Key::Char('g') => self.top = 0,
            Key::End | Key::Char('G') => self.top = end,
            Key::Char('/' | '?') => {
                self.prompt = Some((key == Key::Char('/'), String::new()));
            }
            Key::Char('n') => self.find(self.forward),
            Key::Char('N') => self.find(!self.forward),
            Key::Char('h') => self.message = HELP.into(),
            Key::Char('w') => {
                if let Some(page) = &self.page {
                    if let Some(warning) =
                        page.warnings.get(self.warning % page.warnings.len().max(1))
                    {
                        self.message = warning.clone();
                        self.warning = self.warning.wrapping_add(1);
                    } else {
                        self.message = "No rendering warnings".into();
                    }
                }
            }
            _ => {}
        }
        true
    }
}

/// Run until quit, restoring the caller's terminal on every return path.
/// `options` is re-evaluated on resize; it must report the full terminal geometry.
pub fn run(
    source: String,
    input: crate::input::Input,
    watch: bool,
    options: impl Fn() -> render::Options,
) -> io::Result<()> {
    // Declaration order makes terminal restoration precede worker shutdown.
    let worker = Worker::new()?;
    let mut tty = Session::open()?;
    let (screen, mut current_options) = prepare(options());
    let mut source = Arc::new(source);
    let path = &input.path;
    let mut watcher = watch.then(|| watch::Watch::new(path.as_ref().unwrap()));
    let mut view = View {
        page: None,
        top: 0,
        screen,
        graphics: Graphics::new(),
        message: String::new(),
        prompt: None,
        query: String::new(),
        forward: true,
        search: None,
        loading: true,
        watch,
        title: path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "stdin".into(), |s| s.to_string_lossy().into_owned()),
        warning: 0,
    };
    worker.submit(Request {
        source: Arc::clone(&source),
        input: input.clone(),
        options: current_options.clone(),
        force: true,
    });
    let mut dirty = true;
    let mut checked = Instant::now();
    loop {
        if tty.stopped() {
            break;
        }
        if tty.suspend_requested() {
            view.graphics.clear(tty.out())?;
            tty.suspend()?;
            dirty = true;
        }
        if let Some(result) = worker.result() {
            view.loading = false;
            match result.page {
                Ok(Some(page)) => {
                    view.graphics.clear(tty.out())?;
                    if let Some(previous) = &view.page {
                        let at_end = view.top > 0
                            && view.top
                                >= previous
                                    .rows
                                    .len()
                                    .saturating_sub(usize::from(previous.geometry.rows));
                        let (mut anchor, offset) = previous.location(view.top);
                        if !Arc::ptr_eq(&source, &result.source) {
                            anchor = relocate(&source, &result.source, anchor);
                        }
                        view.top = if at_end {
                            max_top(&page, view.screen)
                        } else {
                            page.restore((anchor, offset))
                                .min(max_top(&page, view.screen))
                        };
                    }
                    source = result.source;
                    view.page = Some(page);
                    view.search = None;
                    view.message.clear();
                    view.warning = 0;
                }
                Ok(None) => view.message.clear(),
                Err(error) if view.page.is_none() && !watch => return Err(error),
                Err(error) => view.message = format!("Reload failed: {error} (r retries)"),
            }
            dirty = true;
        }
        let mut reload = false;
        let mut force = false;
        if let (Some(watcher), Some(path)) = (&mut watcher, &path) {
            reload |= watcher.poll(path);
        }
        if let (Some(search), Some(page)) = (&mut view.search, &mut view.page) {
            if let Some(found) = search.step(page, &view.query)? {
                view.top = found.min(max_top(page, view.screen));
                view.search = None;
                view.message = format!("Found: {}", view.query);
                dirty = true;
            } else if search.remaining == 0 {
                view.search = None;
                view.message = format!("Not found: {}", view.query);
                dirty = true;
            }
        }
        // A key or search result may request a redraw before the idle resize
        // poll. Never emit rows laid out for a wider viewport after a resize.
        if dirty || checked.elapsed() >= Duration::from_millis(200) {
            checked = Instant::now();
            let (screen, next) = prepare(options());
            if screen != view.screen {
                view.screen = screen;
                current_options = next;
                view.graphics.clear(tty.out())?;
                force = true;
                reload = true;
            }
        }
        if dirty {
            view.draw(&current_options, &mut tty)?;
            dirty = false;
        }
        if let Some(key) = tty.key(
            if view.search.is_some() { 0 } else { 50 },
            worker.wakeup_fd(),
        )? {
            if key == Key::Char('\x1a') {
                view.graphics.clear(tty.out())?;
                tty.suspend()?;
                dirty = true;
                continue;
            }
            if view.prompt.is_none() && key == Key::Char('r') {
                force = true;
                reload = true;
            } else if !view.key(key) {
                break;
            }
            dirty = true;
        }
        if reload {
            // An unchanged file must still finish an outstanding resize/initial render.
            force |= view
                .page
                .as_ref()
                .is_none_or(|p| Some(p.geometry) != current_options.terminal);
            worker.submit(Request {
                source: Arc::clone(&source),
                input: input.clone(),
                options: current_options.clone(),
                force,
            });
            view.loading = true;
            view.message.clear();
            view.search = None;
            dirty = true;
        }
    }
    view.graphics.clear(tty.out())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_anchors_follow_edits_and_status_is_cell_bounded() {
        assert_eq!(relocate("a\nb\nc\n", "intro\na\nb\nc\n", 4), 10);
        assert_eq!(relocate("intro\na\nb\nc\n", "a\nb\nc\n", 10), 4);
        assert_eq!(relocate("abc", "abcXYZ", 1), 1);
        assert_eq!(safe("a\x1b\n界é", 4), "a界é");
        assert_eq!(safe("e\u{301}界", 2), "e\u{301}");
    }
}
