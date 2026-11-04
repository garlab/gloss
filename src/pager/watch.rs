use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

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
