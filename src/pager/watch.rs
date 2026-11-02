use std::{
    fs::{self, File},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::{Duration, Instant},
};

pub const MAX_SOURCE: u64 = 64 * 1024 * 1024;
pub fn read(mut input: impl Read) -> io::Result<String> {
    let mut source = String::new();
    (&mut input)
        .take(MAX_SOURCE + 1)
        .read_to_string(&mut source)?;
    if source.len() as u64 > MAX_SOURCE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pager input exceeds 64 MiB; use direct output or a smaller document",
        ));
    }
    Ok(source)
}
pub fn read_file(path: &Path) -> io::Result<String> {
    // Nonblocking open prevents a replaced path becoming a FIFO from hanging the UI.
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pager requires a regular file",
        ));
    }
    if metadata.len() > MAX_SOURCE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pager input exceeds 64 MiB; use direct output or a smaller document",
        ));
    }
    read(file)
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp {
        device: m.dev(),
        inode: m.ino(),
        size: m.len(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}
pub(super) struct Watch {
    seen: Option<Stamp>,
    changed: Option<Instant>,
    checked: Instant,
}
impl Watch {
    pub fn new(path: &Path) -> Self {
        Self {
            seen: stamp(path),
            changed: None,
            checked: Instant::now(),
        }
    }
    pub fn poll(&mut self, path: &Path) -> bool {
        let now = Instant::now();
        if now.duration_since(self.checked) < Duration::from_millis(250) {
            return false;
        }
        self.checked = now;
        let next = stamp(path);
        if next != self.seen {
            self.seen = next;
            self.changed = Some(now);
            return false;
        }
        if self
            .changed
            .is_some_and(|at| now.duration_since(at) >= Duration::from_millis(150))
        {
            self.changed = None;
            return true;
        }
        false
    }
}
