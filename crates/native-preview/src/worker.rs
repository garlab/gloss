//! Isolated rendering, shared wire protocol and process supervision.
use crate::{
    MAX_PIXELS, Raster, RasterOptions,
    mermaid::{self, MAX_SOURCE},
};
use std::{
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
/// A worker exits after this many requests; applications can recycle it sooner.
pub const MAX_REQUESTS: usize = 32;
#[cfg(target_os = "macos")]
const MAX_RESIDENT_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
    // SAFETY: proc_pidinfo initializes this exact structure on a full-size result.
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count == size {
        Some(unsafe { info.assume_init() }.pti_resident_size)
    } else {
        None
    }
}

/// Private child entry point; invoke before application initialization.
pub fn worker_main() -> io::Result<()> {
    restrict_worker()?;
    serve(&mut io::stdin().lock(), &mut io::stdout().lock())
}

fn restrict_worker() -> io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: valid pointers to process-local resource limits. Fail closed if
        // the CPU/core-dump backstops cannot be applied to this dedicated child.
        for (resource, seconds) in [(libc::RLIMIT_CPU, 8), (libc::RLIMIT_CORE, 0)] {
            let limit = libc::rlimit {
                rlim_cur: seconds,
                rlim_max: seconds,
            };
            if unsafe { libc::setrlimit(resource, &limit) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // Lower priority is best effort; it does not enforce a resource limit.
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::{CStr, c_char};
        unsafe extern "C" {
            fn sandbox_init(profile: *const c_char, flags: u64, error: *mut *mut c_char) -> i32;
            fn sandbox_free_error(error: *mut c_char);
        }
        // All fonts are embedded; this worker needs no document paths, external
        // images, system fonts, writes, network, or child processes. Fail closed.
        let profile = c"(version 1)(allow default)(deny network*)(deny file-write*)(deny file-read-data)(deny process-exec)(deny process-fork)";
        let mut error = std::ptr::null_mut();
        // SAFETY: static NUL-terminated profile and valid out-pointer.
        if unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) } != 0 {
            let message = if error.is_null() {
                "Cannot restrict diagram worker".into()
            } else {
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            if !error.is_null() {
                unsafe { sandbox_free_error(error) };
            }
            return Err(io::Error::other(message));
        }
    }
    Ok(())
}

fn serve(input: &mut impl Read, output: &mut impl Write) -> io::Result<()> {
    for _ in 0..MAX_REQUESTS {
        let mut header = [0; 25];
        match input.read(&mut header[..1])? {
            0 => return Ok(()),
            _ => input.read_exact(&mut header[1..])?,
        }
        let number = |slice: &[u8]| u32::from_le_bytes(slice.try_into().unwrap());
        let length = number(&header[12..16]) as usize;
        let options = RasterOptions {
            bounds: (number(&header[4..8]), number(&header[8..12])),
            scale: f64::from_le_bytes(header[17..25].try_into().unwrap()),
        };
        if &header[..4] != b"NPV1"
            || !options.valid()
            || length == 0
            || length > MAX_SOURCE
            || header[16] > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid diagram request",
            ));
        }
        let mut source = vec![0; length];
        input.read_exact(&mut source)?;
        let pixels = std::str::from_utf8(&source)
            .ok()
            .and_then(|s| mermaid::render(s, header[16] == 1, options));
        if let Some(p) = pixels {
            for n in [
                p.width,
                p.height,
                p.rgba.len() as u32,
                p.original_width,
                p.original_height,
            ] {
                output.write_all(&n.to_le_bytes())?;
            }
            output.write_all(&p.rgba)?;
        } else {
            output.write_all(&[0; 20])?;
        }
        output.flush()?;
    }
    Ok(())
}

fn response(input: &mut impl Read, bounds: (u32, u32)) -> io::Result<Option<Raster>> {
    let mut header = [0; 20];
    input.read_exact(&mut header)?;
    let number = |slice: &[u8]| u32::from_le_bytes(slice.try_into().unwrap());
    let (width, height, length) = (
        number(&header[..4]),
        number(&header[4..8]),
        number(&header[8..12]) as usize,
    );
    let original_width = number(&header[12..16]);
    let original_height = number(&header[16..20]);
    if width == 0 && height == 0 && length == 0 && original_width == 0 && original_height == 0 {
        return Ok(None);
    }
    let pixels = u64::from(width) * u64::from(height);
    if original_width == 0
        || original_height == 0
        || original_width > 8192
        || original_height > 8192
        || u64::from(original_width) * u64::from(original_height) > 4_000_000
        || width == 0
        || height == 0
        || width > bounds.0
        || height > bounds.1
        || pixels > MAX_PIXELS as u64
        || pixels * 4 != length as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Mermaid pixels",
        ));
    }
    let mut rgba = vec![0; length];
    input.read_exact(&mut rgba)?;
    Ok(Some(Raster {
        original_width,
        original_height,
        width,
        height,
        rgba,
    }))
}

type Request = (Vec<u8>, (u32, u32));
/// One child process with bounded messages, cancellation and failure cleanup.
/// Applications own the scheduling queue and decide when to recycle or retire it.
pub struct Worker {
    child: Child,
    requests: Option<mpsc::SyncSender<Request>>,
    replies: mpsc::Receiver<io::Result<Option<Raster>>>,
    thread: Option<JoinHandle<()>>,
    completed: usize,
}
impl Worker {
    pub fn requests_completed(&self) -> usize {
        self.completed
    }
    pub fn spawn(program: &Path, worker_argument: &str) -> io::Result<Self> {
        let child = Command::new(program)
            .arg(worker_argument)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Self::connect(child)
    }

    fn connect(mut child: Child) -> io::Result<Self> {
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (requests, incoming) = mpsc::sync_channel::<Request>(1);
        let (replies, outgoing) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("mermaid-io".into())
            .spawn(move || {
                while let Ok((message, bounds)) = incoming.recv() {
                    let result = input
                        .write_all(&message)
                        .and_then(|()| input.flush())
                        .and_then(|()| response(&mut output, bounds));
                    let failed = result.is_err();
                    if replies.send(result).is_err() || failed {
                        break;
                    }
                }
            });
        match thread {
            Ok(thread) => Ok(Self {
                child,
                requests: Some(requests),
                replies: outgoing,
                thread: Some(thread),
                completed: 0,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    /// Render one diagram off the UI thread. Any error stops and reaps the
    /// worker; subsequent calls fail until the application creates a new handle.
    pub fn render(
        &mut self,
        source: &str,
        light: bool,
        options: RasterOptions,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> io::Result<Option<Raster>> {
        let result = self.render_inner(source, light, options, timeout, cancel);
        if result.is_err() {
            self.stop();
        }
        result
    }
    fn render_inner(
        &mut self,
        source: &str,
        light: bool,
        options: RasterOptions,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> io::Result<Option<Raster>> {
        if self.requests.is_none() {
            return Err(io::Error::other("Diagram worker stopped"));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if !options.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid raster options",
            ));
        }
        if !mermaid::admissible(source) {
            return Ok(None);
        }
        if self.completed >= MAX_REQUESTS {
            return Err(io::Error::other("Diagram worker request limit reached"));
        }
        let bounds = options.bounds;
        let mut message = Vec::with_capacity(25 + source.len());
        message.extend_from_slice(b"NPV1");
        message.extend_from_slice(&bounds.0.to_le_bytes());
        message.extend_from_slice(&bounds.1.to_le_bytes());
        message.extend_from_slice(&(source.len() as u32).to_le_bytes());
        message.push(u8::from(light));
        message.extend_from_slice(&options.scale.to_le_bytes());
        message.extend_from_slice(source.as_bytes());
        self.requests
            .as_ref()
            .unwrap()
            .try_send((message, bounds))
            .map_err(|_| io::Error::other("Mermaid worker unavailable"))?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid render timeout"))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Diagram cancelled",
                ));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Diagram timed out"));
            }
            #[cfg(target_os = "macos")]
            if resident_bytes(self.child.id()).is_some_and(|n| n > MAX_RESIDENT_BYTES) {
                return Err(io::Error::other("Diagram exceeded its memory limit"));
            }
            match self.replies.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => {
                    self.completed += 1;
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(io::Error::other("Diagram worker stopped")),
            }
        }
    }
    fn stop(&mut self) {
        // The worker starts no descendants. Killing it closes both pipes and
        // unblocks the I/O thread even after a partial write or partial reply.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_EDGE;
    use std::sync::Arc;
    #[test]
    fn worker_protocol_rejects_invalid_or_truncated_data_before_allocating() {
        let mut data = Vec::new();
        for n in [2u32, 1, 8, 1, 1] {
            data.extend_from_slice(&n.to_le_bytes());
        }
        data.extend_from_slice(&[255, 0, 0, 128, 0, 0, 255, 255]);
        let p = response(&mut &data[..], (MAX_EDGE, MAX_EDGE))
            .unwrap()
            .unwrap();
        assert_eq!((p.width, p.height), (2, 1));
        assert_eq!(p.rgba, data[20..]);
        assert!(response(&mut &data[..27], (MAX_EDGE, MAX_EDGE)).is_err());
        assert!(response(&mut &data[..], (1, 1)).is_err());
        assert!(
            response(&mut &[0; 20][..], (MAX_EDGE, MAX_EDGE))
                .unwrap()
                .is_none()
        );
        data[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(response(&mut &data[..], (MAX_EDGE, MAX_EDGE)).is_err());
        let mut request = Vec::from(&b"NPV1"[..]);
        for n in [MAX_EDGE, MAX_EDGE, MAX_SOURCE as u32 + 1] {
            request.extend_from_slice(&n.to_le_bytes());
        }
        request.push(0);
        request.extend_from_slice(&1_f64.to_le_bytes());
        let mut output = Vec::new();
        assert!(serve(&mut &request[..], &mut output).is_err());
        assert!(output.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn timeouts_and_cancellation_kill_and_reap_partial_replies() {
        for cancel_after in [None, Some(Duration::from_millis(30))] {
            let child = Command::new("sh")
                .args(["-c", "printf '\\001'; exec sleep 30"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut worker = Worker::connect(child).unwrap();
            let pid = worker.child.id();
            let cancel = Arc::new(AtomicBool::new(false));
            let cancellation = cancel_after.map(|duration| {
                let cancel = cancel.clone();
                thread::spawn(move || {
                    thread::sleep(duration);
                    cancel.store(true, Ordering::Relaxed);
                })
            });
            let started = Instant::now();
            let error = worker
                .render(
                    "flowchart LR; A-->B",
                    false,
                    RasterOptions::scaled(1.),
                    Duration::from_millis(100),
                    &cancel,
                )
                .err()
                .unwrap();
            assert_eq!(
                error.kind(),
                if cancel_after.is_some() {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::TimedOut
                }
            );
            assert!(worker.requests.is_none());
            if let Some(thread) = cancellation {
                thread.join().unwrap();
            }
            assert!(started.elapsed() < Duration::from_secs(3));
            // SAFETY: signal zero only checks whether the reaped child exists.
            assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            assert!(
                worker
                    .render(
                        "flowchart LR; A-->B",
                        false,
                        RasterOptions::scaled(1.),
                        Duration::from_millis(100),
                        &AtomicBool::new(false)
                    )
                    .is_err()
            );
        }
    }
}
