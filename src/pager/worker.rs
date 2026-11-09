//! One active render and one replaceable pending request, independent of input.
use super::record::{self, Page};
use crate::{
    input::{Format, Input},
    render,
};
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
    pub input: Input,
    pub options: render::Options,
    pub force: bool,
    /// Publish a provisional page once this many rows are recorded, then the
    /// complete one. For a first render, when nothing is shown yet.
    pub preview: Option<usize>,
}
pub(super) struct Completed {
    pub source: Arc<String>,
    /// The text that row anchors index, when it is not `source`: LaTeX with
    /// included files.
    pub text: Option<Arc<str>>,
    /// Files read besides `source`, for watching.
    pub dependencies: Vec<PathBuf>,
    pub page: io::Result<Option<Page>>,
    /// `page` holds only the first rows; the complete page follows.
    pub partial: bool,
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
                    let mut text = None;
                    let mut dependencies = Vec::new();
                    let page = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(io::ErrorKind::Interrupted.into());
                        }
                        if request.input.path.is_some() {
                            let updated = request.input.read(true)?;
                            if !request.force
                                && request.input.format != Format::Image
                                && updated == *source
                            {
                                return Ok(None);
                            }
                            source = Arc::new(updated);
                        }
                        let document = request.input.document(source.as_ref().clone());
                        text = document.expanded_text();
                        dependencies = document.dependencies().to_vec();
                        if let Some(rows) = request.preview {
                            let (page, complete) = record::build_preview(
                                &document,
                                &request.options,
                                Arc::clone(&cancel),
                                &mut cache,
                                rows,
                            )?;
                            if complete {
                                return Ok(Some(page));
                            }
                            let mut state = mutex.lock().unwrap();
                            if cancel.load(Ordering::Relaxed) || state.shutdown {
                                return Err(io::ErrorKind::Interrupted.into());
                            }
                            state.result = Some(Completed {
                                source: Arc::clone(&source),
                                text: text.clone(),
                                dependencies: dependencies.clone(),
                                page: Ok(Some(page)),
                                partial: true,
                            });
                            let _ = (&notification).write(&[1]);
                        }
                        record::build(&document, &request.options, Arc::clone(&cancel), &mut cache)
                            .map(Some)
                    }))
                    .unwrap_or_else(|_| {
                        cache = render::RenderCache::default();
                        Err(io::Error::other("pager renderer failed"))
                    });
                    let mut state = mutex.lock().unwrap();
                    state.running = None;
                    if !cancel.load(Ordering::Relaxed) && !state.shutdown {
                        state.result = Some(Completed {
                            source,
                            text,
                            dependencies,
                            page,
                            partial: false,
                        });
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
            input: Input::new(None, Format::Auto, None).unwrap(),
            force: true,
            preview: None,
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
