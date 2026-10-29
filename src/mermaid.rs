//! Bounded native Mermaid rendering in one lazy, invocation-local worker.
//! The worker uses this executable, private pipes, and bundled fonts only.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{images::Loader, raster::Raster};

pub(crate) const MAX_SOURCE: usize = 16 * 1024;
const MAX_PIXELS: usize = 1_000_000;
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const TOTAL_TIME: Duration = Duration::from_secs(5);
const MAX_ATTEMPTS: usize = 32;

struct Cached {
    source: String,
    bounds: (u32, u32),
    light: bool,
    pixels: Option<Arc<Raster>>,
}
impl Cached {
    fn bytes(&self) -> usize {
        self.source.len() + self.pixels.as_ref().map_or(0, |p| p.rgba.len())
    }
}

#[derive(Default)]
pub(crate) struct Engine {
    worker: Option<Worker>,
    failed: bool,
    attempts: usize,
    elapsed: Duration,
    cache: VecDeque<Cached>,
    cache_bytes: usize,
}

impl Engine {
    pub(crate) fn render(
        &mut self,
        source: &str,
        bounds: (u32, u32),
        light: bool,
    ) -> Option<Arc<Raster>> {
        if !admissible(source) || bounds.0 == 0 || bounds.1 == 0 {
            return None;
        }
        let bounds = (bounds.0.min(8192), bounds.1.min(8192));
        if let Some(index) = self.cache.iter().position(|entry| {
            entry.source == source && entry.bounds == bounds && entry.light == light
        }) {
            let entry = self.cache.remove(index).unwrap();
            let pixels = entry.pixels.clone();
            self.cache.push_back(entry);
            return pixels;
        }
        if self.failed || self.attempts >= MAX_ATTEMPTS || self.elapsed >= TOTAL_TIME {
            return None;
        }
        self.attempts += 1;
        let started = Instant::now();
        let timeout = REQUEST_TIMEOUT.min(TOTAL_TIME.saturating_sub(self.elapsed));
        let result = (|| {
            if self.worker.is_none() {
                self.worker = Some(Worker::spawn(&std::env::current_exe()?)?);
            }
            self.worker
                .as_mut()
                .unwrap()
                .render(source, bounds, light, timeout)
        })();
        self.elapsed += started.elapsed();
        let pixels = match result {
            Ok(pixels) => pixels.map(Arc::new),
            Err(_) => {
                // A broken protocol, timeout, or crash disables new work for this
                // invocation. Successful cache entries remain available.
                self.failed = true;
                self.worker = None;
                return None;
            }
        };
        let entry = Cached {
            source: source.into(),
            bounds,
            light,
            pixels: pixels.clone(),
        };
        let cost = entry.bytes();
        if cost <= CACHE_BYTES {
            while self.cache_bytes + cost > CACHE_BYTES || self.cache.len() >= 32 {
                self.cache_bytes -= self.cache.pop_front().unwrap().bytes();
            }
            self.cache_bytes += cost;
            self.cache.push_back(entry);
        }
        pixels
    }
}

/// Reject excessive input and configuration before starting a helper or parser.
fn admissible(source: &str) -> bool {
    if source.is_empty()
        || !source.is_ascii()
        || source.len() > MAX_SOURCE
        || source.lines().count() > 256
        || source.bytes().filter(|b| *b == b';').count() > 256
        || source.bytes().filter(|b| *b == b'&').count() > 64
        || source.contains("%%{")
        || source.trim_start().starts_with("---")
        || source
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return false;
    }
    let header = source
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty() && !s.starts_with("%%"))
        .and_then(|s| s.split(|c: char| c.is_whitespace() || c == ';').next())
        .unwrap_or_default();
    if !matches!(
        header,
        "flowchart"
            | "graph"
            | "sequenceDiagram"
            | "classDiagram"
            | "stateDiagram"
            | "stateDiagram-v2"
            | "erDiagram"
    ) {
        return false;
    }
    // A conservative lexical nesting guard bounds work before native parsing.
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for character in source.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        if matches!(character, '[' | '(' | '{') {
            depth += 1;
            if depth > 32 {
                return false;
            }
        } else if matches!(character, ']' | ')' | '}') {
            depth = depth.saturating_sub(1);
        }
    }
    !matches!(header, "flowchart" | "graph") || (depth == 0 && !quoted)
}

fn rasterize(source: &str, bounds: (u32, u32), light: bool, images: &mut Loader) -> Option<Raster> {
    use mermaid_rs_renderer::{
        DiagramKind, LayoutConfig, Theme, compute_layout, parse_mermaid_strict, render_svg,
    };
    if !admissible(source) {
        return None;
    }
    let parsed = parse_mermaid_strict(source).ok()?;
    let graph = &parsed.graph;
    // Attribute tables in this backend require OS-font metrics and can omit
    // types when those metrics are unavailable. Preserve the full code instead.
    if graph.kind == DiagramKind::Er && graph.nodes.values().any(|n| n.label.contains('\n')) {
        return None;
    }
    if parsed.init_config.is_some()
        || !matches!(
            graph.kind,
            DiagramKind::Flowchart
                | DiagramKind::Sequence
                | DiagramKind::Class
                | DiagramKind::State
                | DiagramKind::Er
        )
        || graph.nodes.len() > 128
        || graph.edges.len() > 256
        || graph.subgraphs.len() > 16
        || graph.subgraphs.iter().map(|g| g.nodes.len()).sum::<usize>() > 512
        || graph.sequence_participants.len() > 32
        || graph.sequence_frames.len() > 16
        || graph.sequence_notes.len() > 64
        || graph.sequence_activations.len() > 128
        || graph.sequence_boxes.len() > 16
        || graph.state_notes.len() > 64
        || !graph.node_links.is_empty()
        || graph
            .nodes
            .values()
            .any(|node| node.icon.is_some() || node.label.len() > 1024 || !node.label.is_ascii())
    {
        return None;
    }
    let mut theme = if light {
        Theme::modern()
    } else {
        Theme::dark()
    };
    if !light {
        theme.background = "#252a33".into();
        theme.primary_color = "#243447".into();
        theme.primary_border_color = "#63cce3".into();
        theme.primary_text_color = "#e3eaf2".into();
        theme.text_color = "#e3eaf2".into();
        theme.line_color = "#b1c3d4".into();
        theme.edge_label_background = theme.background.clone();
        theme.sequence_actor_fill = theme.primary_color.clone();
        theme.sequence_actor_border = theme.primary_border_color.clone();
    }
    theme.font_family = "KaTeX_SansSerif".into();
    theme.font_size = 16.;
    let config = LayoutConfig {
        fast_text_metrics: true,
        ..Default::default()
    };
    let layout = compute_layout(graph, &theme, &config);
    let svg = render_svg(&layout, &theme, &config);
    images.diagram_svg(svg.as_bytes(), bounds).ok()
}

/// Internal versioned protocol. No Markdown file paths or environment dumps are
/// sent to the worker, and neither side accepts an unbounded message length.
#[doc(hidden)]
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> io::Result<()> {
    #[cfg(unix)]
    {
        // Also bound CPU if the parent is abruptly killed during native layout.
        let limit = libc::rlimit {
            rlim_cur: 8,
            rlim_max: 8,
        };
        // SAFETY: valid pointer; affects only this dedicated worker process.
        unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) };
    }
    let mut images = Loader::default();
    let mut requests = 0;
    loop {
        let mut header = [0; 17];
        match input.read(&mut header[..1])? {
            0 => return Ok(()),
            _ => input.read_exact(&mut header[1..])?,
        }
        let number = |slice: &[u8]| u32::from_le_bytes(slice.try_into().unwrap());
        let bounds = (number(&header[4..8]), number(&header[8..12]));
        let length = number(&header[12..16]) as usize;
        if &header[..4] != b"MDM1"
            || length == 0
            || length > MAX_SOURCE
            || bounds.0 == 0
            || bounds.1 == 0
            || bounds.0 > 8192
            || bounds.1 > 8192
            || header[16] > 1
            || requests >= MAX_ATTEMPTS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Mermaid request",
            ));
        }
        requests += 1;
        let mut source = vec![0; length];
        input.read_exact(&mut source)?;
        let pixels = std::str::from_utf8(&source)
            .ok()
            .and_then(|s| rasterize(s, bounds, header[16] == 1, &mut images));
        if let Some(pixels) = pixels {
            output.write_all(&pixels.width.to_le_bytes())?;
            output.write_all(&pixels.height.to_le_bytes())?;
            output.write_all(&(pixels.rgba.len() as u32).to_le_bytes())?;
            output.write_all(&pixels.rgba)?;
        } else {
            output.write_all(&[0; 12])?;
        }
        output.flush()?;
    }
}

fn response(input: &mut impl Read, bounds: (u32, u32)) -> io::Result<Option<Raster>> {
    let mut header = [0; 12];
    input.read_exact(&mut header)?;
    let number = |slice: &[u8]| u32::from_le_bytes(slice.try_into().unwrap());
    let (width, height, length) = (
        number(&header[..4]),
        number(&header[4..8]),
        number(&header[8..]) as usize,
    );
    if width == 0 && height == 0 && length == 0 {
        return Ok(None);
    }
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
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
        width,
        height,
        rgba,
    }))
}

type Request = (Vec<u8>, (u32, u32));
struct Worker {
    child: Child,
    requests: Option<mpsc::SyncSender<Request>>,
    replies: mpsc::Receiver<io::Result<Option<Raster>>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn spawn(program: &Path) -> io::Result<Self> {
        let child = Command::new(program)
            .arg("--mermaid-worker")
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
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    fn render(
        &mut self,
        source: &str,
        bounds: (u32, u32),
        light: bool,
        timeout: Duration,
    ) -> io::Result<Option<Raster>> {
        let mut message = Vec::with_capacity(17 + source.len());
        message.extend_from_slice(b"MDM1");
        message.extend_from_slice(&bounds.0.to_le_bytes());
        message.extend_from_slice(&bounds.1.to_le_bytes());
        message.extend_from_slice(&(source.len() as u32).to_le_bytes());
        message.push(u8::from(light));
        message.extend_from_slice(source.as_bytes());
        self.requests
            .as_ref()
            .unwrap()
            .try_send((message, bounds))
            .map_err(|_| io::Error::other("Mermaid worker unavailable"))?;
        self.replies
            .recv_timeout(timeout)
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Mermaid worker timed out"))?
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_diagrams_fit_with_bundled_fonts_and_missing_glyphs_fall_back() {
        let mut loader = Loader::default();
        for source in [
            "flowchart LR\n A[Start] --> B{Ready?}\n B -->|Yes| C[Done]\n",
            "sequenceDiagram\n Alice->>Bob: Hello\n Bob-->>Alice: Reply\n",
            "classDiagram\n class Note {\n +String title\n }\n Note --> Reader\n",
            "stateDiagram-v2\n [*] --> Ready\n Ready --> [*]\n",
            "erDiagram\n BOOK ||--o{ PAGE : contains\n",
            "flowchart TD\n subgraph Input\n A-->B\n end\n B-->C\n",
        ] {
            for light in [false, true] {
                let raster = rasterize(source, (500, 300), light, &mut loader)
                    .unwrap_or_else(|| panic!("failed: {source}"));
                assert!(raster.width <= 500 && raster.height <= 300);
                assert_eq!(
                    raster.rgba.len(),
                    (raster.width * raster.height * 4) as usize
                );
                assert!(raster.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            }
        }
        assert!(
            rasterize(
                "flowchart LR\n A[中文]-->B\n",
                (500, 300),
                false,
                &mut loader
            )
            .is_none()
        );
    }

    #[test]
    fn unsupported_excessive_and_external_content_retains_source() {
        let mut engine = Engine::default();
        for source in [
            "pie\n\"A\":1\n",
            "%%{init: {}}%%\nflowchart LR\nA-->B\n",
            "---\ntheme: dark\n---\nflowchart LR; A-->B",
            "flowchart LR\nA\x1b[2J-->B",
        ] {
            assert!(engine.render(source, (500, 300), false).is_none());
        }
        assert!(engine.worker.is_none());
        assert_eq!(engine.attempts, 0);
        assert!(!admissible(&format!(
            "flowchart LR\n{}",
            "A".repeat(MAX_SOURCE)
        )));
        assert!(!admissible(&format!("flowchart LR\n{}", "A;".repeat(257))));
        assert!(!admissible(&format!(
            "flowchart LR\nA{}B{}",
            "[".repeat(33),
            "]".repeat(33)
        )));
        let mut loader = Loader::default();
        assert!(
            rasterize(
                "erDiagram\n BOOK {\n string title\n }\n",
                (500, 300),
                false,
                &mut loader
            )
            .is_none()
        );
        let large = format!(
            "flowchart LR\n{}",
            (0..129)
                .map(|i| format!("N{i}[Node {i}]\n"))
                .collect::<String>()
        );
        assert!(rasterize(&large, (500, 300), false, &mut loader).is_none());
        assert!(
            rasterize(
                "flowchart LR\nA-->B\nclick A \"https://example.com\"\n",
                (500, 300),
                false,
                &mut loader
            )
            .is_none()
        );
        assert!(rasterize("flowchart LR\nA[Broken", (500, 300), false, &mut loader).is_none());
        assert!(rasterize("flowchart LR\nA-->B", (500, 300), false, &mut loader).is_some());
    }

    #[test]
    fn worker_responses_are_bounded_and_truncation_is_an_error() {
        let mut data = Vec::new();
        for n in [2u32, 1, 8] {
            data.extend_from_slice(&n.to_le_bytes());
        }
        data.extend_from_slice(&[255, 0, 0, 128, 0, 0, 255, 255]);
        let pixels = response(&mut &data[..], (10, 10)).unwrap().unwrap();
        assert_eq!((pixels.width, pixels.height), (2, 1));
        assert_eq!(pixels.rgba, data[12..]);
        assert!(response(&mut &data[..19], (10, 10)).is_err());
        assert!(response(&mut &data[..], (1, 1)).is_err());
        assert!(response(&mut &[0; 12][..], (10, 10)).unwrap().is_none());
        data[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(response(&mut &data[..], (10, 10)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_and_reaps_a_worker_with_a_partial_reply() {
        let child = Command::new("sh")
            .args(["-c", "printf '\\001'; exec sleep 30"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut worker = Worker::connect(child).unwrap();
        let pid = worker.child.id();
        let started = Instant::now();
        let error = worker
            .render(
                "flowchart LR; A-->B",
                (500, 300),
                false,
                Duration::from_millis(50),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(3));
        // SAFETY: signal zero only checks whether our reaped child still exists.
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
}
