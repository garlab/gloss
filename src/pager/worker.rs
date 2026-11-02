//! One active render and one replaceable pending request, independent of input.
use super::{
    record::{self, Page},
    watch,
};
use crate::render;
use std::{
    io,
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
}
impl Worker {
    pub fn new() -> io::Result<Self> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let state = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("pager-render".into())
            .spawn(move || {
                let (mutex, ready) = &*state;
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
                        record::build(&source, &request.options, Arc::clone(&cancel)).map(Some)
                    }))
                    .unwrap_or_else(|_| Err(io::Error::other("pager renderer failed")));
                    let mut state = mutex.lock().unwrap();
                    state.running = None;
                    if !cancel.load(Ordering::Relaxed) && !state.shutdown {
                        state.result = Some(Completed { source, page });
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
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
        self.shared.0.lock().unwrap().result.take()
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
