//! A controlling-terminal input channel keeps piped Markdown separate from keys.
use signal_hook::{SigId, consts::signal::*};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    os::fd::{AsRawFd, RawFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Key {
    Char(char),
    Escape,
    Enter,
    Backspace,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

#[derive(Default)]
struct Decoder {
    bytes: Vec<u8>,
    escape_since: Option<Instant>,
    paste: bool,
}
impl Decoder {
    fn next(&mut self) -> Option<Key> {
        loop {
            if self.paste {
                if let Some(end) = self.bytes.windows(6).position(|s| s == b"\x1b[201~") {
                    self.bytes.drain(..end + 6);
                    self.paste = false;
                } else {
                    // Retain only a possible fragmented closing delimiter.
                    let discard = self.bytes.len().saturating_sub(5);
                    self.bytes.drain(..discard);
                    return None;
                }
            }
            let &first = self.bytes.first()?;
            if first == 0x1b {
                let since = *self.escape_since.get_or_insert_with(Instant::now);
                if self.bytes.len() == 1 {
                    if since.elapsed() < Duration::from_millis(40) {
                        return None;
                    }
                    self.bytes.clear();
                    self.escape_since = None;
                    return Some(Key::Escape);
                }
                if matches!(self.bytes[1], b'[' | b'O') {
                    if let Some(last) = self.bytes[2..]
                        .iter()
                        .position(|b| (0x40..=0x7e).contains(b))
                    {
                        let seq: Vec<_> = self.bytes.drain(..last + 3).collect();
                        self.escape_since = None;
                        let key = match seq.as_slice() {
                            b"\x1b[A" | b"\x1bOA" => Some(Key::Up),
                            b"\x1b[B" | b"\x1bOB" => Some(Key::Down),
                            b"\x1b[5~" => Some(Key::PageUp),
                            b"\x1b[6~" => Some(Key::PageDown),
                            b"\x1b[H" | b"\x1bOH" | b"\x1b[1~" | b"\x1b[7~" => Some(Key::Home),
                            b"\x1b[F" | b"\x1bOF" | b"\x1b[4~" | b"\x1b[8~" => Some(Key::End),
                            b"\x1b[200~" => {
                                self.paste = true;
                                None
                            }
                            _ => None,
                        };
                        if key.is_some() {
                            return key;
                        }
                        continue;
                    }
                    if self.bytes.len() < 64 && since.elapsed() < Duration::from_millis(100) {
                        return None;
                    }
                    self.bytes.clear();
                    self.escape_since = None;
                    return None;
                }
                // Ignore Alt-key sequences rather than interpreting the suffix as a command.
                self.bytes.drain(..2);
                self.escape_since = None;
                continue;
            }
            self.escape_since = None;
            let count = match first {
                0..=127 => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => {
                    self.bytes.remove(0);
                    continue;
                }
            };
            if self.bytes.len() < count {
                return None;
            }
            let ch = std::str::from_utf8(&self.bytes[..count])
                .ok()
                .and_then(|s| s.chars().next());
            self.bytes.drain(..count);
            if let Some(ch) = ch {
                return Some(match ch {
                    '\r' | '\n' => Key::Enter,
                    '\x7f' | '\x08' => Key::Backspace,
                    _ => Key::Char(ch),
                });
            }
        }
    }
}

pub(super) struct Session {
    input: File,
    output: BufWriter<io::Stdout>,
    saved: libc::termios,
    active: bool,
    signals: Vec<SigId>,
    stop: Arc<AtomicBool>,
    suspend: Arc<AtomicBool>,
    decoder: Decoder,
}
impl Session {
    pub fn open() -> io::Result<Self> {
        let input = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|e| {
                io::Error::new(e.kind(), format!("pager needs a controlling terminal: {e}"))
            })?;
        let mut saved = std::mem::MaybeUninit::uninit();
        // SAFETY: the live descriptor and correctly sized writable termios are valid.
        if unsafe { libc::tcgetattr(input.as_raw_fd(), saved.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr initialized saved on success.
        let saved = unsafe { saved.assume_init() };
        let mut session = Self {
            input,
            output: BufWriter::new(io::stdout()),
            saved,
            active: false,
            signals: Vec::new(),
            stop: Arc::default(),
            suspend: Arc::default(),
            decoder: Decoder::default(),
        };
        for signal in [SIGINT, SIGTERM, SIGHUP, SIGQUIT] {
            session.signals.push(signal_hook::flag::register(
                signal,
                Arc::clone(&session.stop),
            )?);
        }
        session.signals.push(signal_hook::flag::register(
            SIGTSTP,
            Arc::clone(&session.suspend),
        )?);
        session.enter()?;
        Ok(session)
    }
    pub fn out(&mut self) -> &mut BufWriter<io::Stdout> {
        &mut self.output
    }
    fn enter(&mut self) -> io::Result<()> {
        let mut raw = self.saved;
        // SAFETY: cfmakeraw mutates our initialized local termios; tcsetattr borrows it.
        unsafe {
            libc::cfmakeraw(&mut raw);
        }
        if unsafe { libc::tcsetattr(self.input.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.active = true;
        self.output
            .write_all(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[0m")?;
        self.output.flush()
    }
    pub fn leave(&mut self) {
        if !self.active {
            return;
        }
        let _ = self
            .output
            .write_all(b"\x1b]8;;\x1b\\\x1b[0m\x1b[?2004l\x1b[?25h\x1b[?1049l");
        let _ = self.output.flush();
        // SAFETY: saved is the original termios and input remains open until Drop finishes.
        unsafe {
            libc::tcsetattr(self.input.as_raw_fd(), libc::TCSANOW, &self.saved);
        }
        self.active = false;
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn suspend_requested(&self) -> bool {
        self.suspend.swap(false, Ordering::Relaxed)
    }
    pub fn suspend(&mut self) -> io::Result<()> {
        self.leave();
        signal_hook::low_level::raise(SIGSTOP)?;
        self.decoder = Decoder::default();
        self.enter()
    }
    pub fn key(&mut self, timeout_ms: i32, wakeup: RawFd) -> io::Result<Option<Key>> {
        if let Some(key) = self.decoder.next() {
            return Ok(Some(key));
        }
        let fd = self.input.as_raw_fd();
        if [fd, wakeup]
            .iter()
            .any(|&fd| fd < 0 || fd as usize >= libc::FD_SETSIZE)
        {
            return Err(io::Error::other("pager descriptor exceeds select capacity"));
        }
        let mut set = std::mem::MaybeUninit::<libc::fd_set>::uninit();
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: (timeout_ms * 1000) as _,
        };
        // SAFETY: FD_ZERO initializes the set, both descriptors are within
        // FD_SETSIZE, and select borrows valid storage. Unlike poll, select
        // supports /dev/tty on macOS.
        let ready = unsafe {
            libc::FD_ZERO(set.as_mut_ptr());
            libc::FD_SET(fd, set.as_mut_ptr());
            libc::FD_SET(wakeup, set.as_mut_ptr());
            libc::select(
                fd.max(wakeup) + 1,
                set.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut timeout,
            )
        };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(error);
        }
        // SAFETY: select initialized the set and fd was checked above. A render
        // notification alone must not cause a blocking read from the terminal.
        if ready > 0 && unsafe { libc::FD_ISSET(fd, set.as_ptr()) } {
            let mut bytes = [0; 1024];
            let count = match self.input.read(&mut bytes) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Ok(None),
                other => other?,
            };
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "terminal input closed",
                ));
            }
            self.decoder.bytes.extend_from_slice(&bytes[..count]);
        }
        Ok(self.decoder.next())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.leave();
        for signal in self.signals.drain(..) {
            signal_hook::low_level::unregister(signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_keys_and_pastes_do_not_leak_commands() {
        let mut decoder = Decoder::default();
        decoder.bytes.extend_from_slice(b"\x1b[");
        assert_eq!(decoder.next(), None);
        decoder.bytes.extend_from_slice(b"6~");
        assert_eq!(decoder.next(), Some(Key::PageDown));
        decoder
            .bytes
            .extend_from_slice(b"\x1b[200~qG/whatever\x1b[20");
        assert_eq!(decoder.next(), None);
        decoder.bytes.extend_from_slice(b"1~j");
        assert_eq!(decoder.next(), Some(Key::Char('j')));
        decoder.bytes.extend_from_slice(&[0xc3]);
        assert_eq!(decoder.next(), None);
        decoder.bytes.push(0xa9);
        assert_eq!(decoder.next(), Some(Key::Char('é')));
        decoder.bytes.extend_from_slice(b"\x1b[999~k");
        assert_eq!(decoder.next(), Some(Key::Char('k')));
    }
}
