use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The named file plus at most this many included files are polled.
const MAX_DEPENDENCIES: usize = 63;

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
    /// The named file first, then the files its last render read.
    seen: Vec<(PathBuf, Option<Stamp>)>,
    /// When the last change was seen, and whether it included a dependency.
    changed: Option<(Instant, bool)>,
    checked: Instant,
}
impl Watch {
    pub fn new(path: &Path) -> Self {
        Self {
            seen: vec![(path.to_path_buf(), stamp(path))],
            changed: None,
            checked: Instant::now(),
        }
    }
    /// Replaces the watched dependencies after a render, keeping the stamps
    /// of files that were already watched.
    pub fn track(&mut self, dependencies: &[PathBuf]) {
        let mut seen = Vec::with_capacity(dependencies.len().min(MAX_DEPENDENCIES) + 1);
        seen.push(self.seen[0].clone());
        for path in dependencies.iter().take(MAX_DEPENDENCIES) {
            if seen.iter().any(|(p, _)| p == path) {
                continue;
            }
            let known = self.seen.iter().find(|(p, _)| p == path);
            let stamp = known.map_or_else(|| stamp(path), |(_, s)| s.clone());
            seen.push((path.clone(), stamp));
        }
        self.seen = seen;
    }
    /// After a change has stayed stable for a poll, returns whether a
    /// dependency changed rather than only the named file.
    pub fn poll(&mut self) -> Option<bool> {
        let now = Instant::now();
        if now.duration_since(self.checked) < Duration::from_millis(250) {
            return None;
        }
        self.checked = now;
        let mut dependency = None;
        for (i, (path, seen)) in self.seen.iter_mut().enumerate() {
            let next = stamp(path);
            if next != *seen {
                *seen = next;
                dependency = Some(dependency.unwrap_or(false) || i > 0);
            }
        }
        if let Some(dependency) = dependency {
            let earlier = self.changed.is_some_and(|(_, d)| d);
            self.changed = Some((now, dependency || earlier));
            return None;
        }
        match self.changed {
            Some((at, dependency)) if now.duration_since(at) >= Duration::from_millis(150) => {
                self.changed = None;
                Some(dependency)
            }
            _ => None,
        }
    }
}
