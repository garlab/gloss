//! One active render and one replaceable pending request, independent of input.
use super::{
    record::{self, Page},
    watch,
};
use crate::render;
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

pub(super) struct Request {
    pub source: Arc<String>,
    pub path: Option<PathBuf>,
    pub options: render::Options,
    pub force: bool,
}
pub(super) struct Completed {
    pub source: Arc<String>,
    pub page: io::Result<Option<Page>>,
}
#[derive(Default)]
struct State {
    pending: Option<Request>,
    running: Option<Arc<AtomicBool>>,
    result: Option<Completed>,
    shutdown: bool,
}
pub(super) struct Worker {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    wakeup: UnixStream,
}
impl Worker {
    pub fn new() -> io::Result<Self> {
        let (wakeup, mut notification) = UnixStream::pair()?;
        wakeup.set_nonblocking(true)?;
        notification.set_nonblocking(true)?;
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let state = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("pager-render".into())
            .spawn(move || {
                let (mutex, ready) = &*state;
                let mut cache = render::RenderCache::default();
                loop {
                    let (request, cancel) = {
                        let mut state = mutex.lock().unwrap();
                        while state.pending.is_none() && !state.shutdown {
                            state = ready.wait(state).unwrap();
                        }
                        if state.shutdown {
                            break;
                        }
                        let cancel = Arc::new(AtomicBool::new(false));
                        state.running = Some(Arc::clone(&cancel));
                        (state.pending.take().unwrap(), cancel)
                    };
                    let mut source = request.source;
                    let page = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(io::ErrorKind::Interrupted.into());
                        }
                        if let Some(path) = request.path {
                            let updated = watch::read_file(&path)?;
                            if !request.force && updated == *source {
                                return Ok(None);
                            }
                            source = Arc::new(updated);
                        }
                        record::build(&source, &request.options, Arc::clone(&cancel), &mut cache)
                            .map(Some)
                    }))
                    .unwrap_or_else(|_| {
                        cache = render::RenderCache::default();
                        Err(io::Error::other("pager renderer failed"))
                    });
                    let mut state = mutex.lock().unwrap();
                    state.running = None;
                    if !cancel.load(Ordering::Relaxed) && !state.shutdown {
                        state.result = Some(Completed { source, page });
                        // A full socket is already readable. Never block rendering
                        // or shutdown while the input thread handles other events.
                        let _ = notification.write(&[1]);
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
            wakeup,
        })
    }
    pub fn submit(&self, request: Request) {
        let (mutex, ready) = &*self.shared;
        let mut state = mutex.lock().unwrap();
        if let Some(cancel) = &state.running {
            cancel.store(true, Ordering::Relaxed);
        }
        state.result = None;
        state.pending = Some(request);
        ready.notify_one();
    }
    pub fn result(&self) -> Option<Completed> {
        let mut bytes = [0; 64];
        while (&self.wakeup).read(&mut bytes).is_ok_and(|n| n > 0) {}
        self.shared.0.lock().unwrap().result.take()
    }
    pub fn wakeup_fd(&self) -> RawFd {
        self.wakeup.as_raw_fd()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        {
            let mut state = mutex.lock().unwrap();
            state.shutdown = true;
            state.pending = None;
            state.result = None;
            if let Some(cancel) = &state.running {
                cancel.store(true, Ordering::Relaxed);
            }
            ready.notify_one();
        }
        // The terminal guard is dropped first. Joining also reaps any diagram subprocess.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_render_wakes_input_without_a_key_or_polling_delay() {
        let worker = Worker::new().unwrap();
        worker.wakeup.set_nonblocking(false).unwrap();
        worker
            .wakeup
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        worker.submit(Request {
            source: Arc::new("Ready.\n".into()),
            path: None,
            force: true,
            options: render::Options {
                terminal: Some(crate::terminal::Geometry::default()),
                ..Default::default()
            },
        });
        let mut byte = [0];
        (&worker.wakeup).read_exact(&mut byte).unwrap();
        worker.wakeup.set_nonblocking(true).unwrap();
        assert!(worker.result().unwrap().page.unwrap().is_some());
        assert!(worker.result().is_none());
    }
}
