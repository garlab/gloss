//! Terminal policy for native diagrams: invocation budgets and fitted-image cache.
use crate::raster::Raster;
pub(crate) use document_graphics::mermaid::MAX_SOURCE;
pub use document_graphics::worker::worker_main;
use document_graphics::{RasterOptions, mermaid::admissible, worker::Worker};
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
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
    pub(crate) fn begin_document(&mut self) {
        self.finish_document();
        self.failed = false;
        self.attempts = 0;
        self.elapsed = Duration::ZERO;
    }

    /// Retain only the bounded cache while a pager is idle.
    pub(crate) fn finish_document(&mut self) {
        self.worker = None;
    }

    pub(crate) fn render(
        &mut self,
        source: &str,
        bounds: (u32, u32),
        light: bool,
        cancel: &AtomicBool,
    ) -> io::Result<Option<Arc<Raster>>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if !admissible(source) || bounds.0 == 0 || bounds.1 == 0 {
            return Ok(None);
        }
        let bounds = (bounds.0.min(8192), bounds.1.min(8192));
        if let Some(index) = self.cache.iter().position(|entry| {
            entry.source == source && entry.bounds == bounds && entry.light == light
        }) {
            let entry = self.cache.remove(index).unwrap();
            let pixels = entry.pixels.clone();
            self.cache.push_back(entry);
            return Ok(pixels);
        }
        if self.failed || self.attempts >= MAX_ATTEMPTS || self.elapsed >= TOTAL_TIME {
            return Ok(None);
        }
        self.attempts += 1;
        let started = Instant::now();
        let timeout = REQUEST_TIMEOUT.min(TOTAL_TIME.saturating_sub(self.elapsed));
        let result = (|| {
            if self.worker.is_none() {
                self.worker = Some(Worker::spawn(
                    &std::env::current_exe()?,
                    "--mermaid-worker",
                )?);
            }
            self.worker.as_mut().unwrap().render(
                source,
                light,
                RasterOptions::fit(bounds),
                timeout,
                cancel,
            )
        })();
        self.elapsed += started.elapsed();
        let pixels = match result {
            Ok(pixels) => pixels.map(|pixels| Arc::new(pixels.into())),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                self.worker = None;
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            Err(_) => {
                // A broken protocol, timeout, or crash disables new work for this
                // invocation. Successful cache entries remain available.
                self.failed = true;
                self.worker = None;
                return Ok(None);
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
        Ok(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_budgets_reset_without_losing_cached_pixels() {
        let pixels = Arc::new(Raster {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        let mut engine = Engine::default();
        let source = "flowchart LR; A-->B";
        let entry = Cached {
            source: source.into(),
            bounds: (500, 300),
            light: false,
            pixels: Some(pixels.clone()),
        };
        engine.cache_bytes = entry.bytes();
        engine.cache.push_back(entry);
        engine.attempts = MAX_ATTEMPTS;
        engine.failed = true;
        engine.elapsed = TOTAL_TIME;
        engine.begin_document();
        let hit = engine
            .render(source, (500, 300), false, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&pixels, &hit));
        assert_eq!(engine.attempts, 0);
        assert_eq!(engine.elapsed, Duration::ZERO);
        assert!(!engine.failed && engine.worker.is_none());
        // Changed bounds/theme must not reuse a previously fitted raster.
        engine.attempts = MAX_ATTEMPTS;
        assert!(
            engine
                .render(source, (400, 300), false, &AtomicBool::new(false))
                .unwrap()
                .is_none()
        );
        assert!(
            engine
                .render(source, (500, 300), true, &AtomicBool::new(false))
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_interrupts_and_reaps_an_unresponsive_worker() {
        let mut engine = Engine {
            worker: Some(Worker::spawn(std::path::Path::new("/bin/sleep"), "30").unwrap()),
            ..Default::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            signal.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let error = engine
            .render("flowchart LR; A-->B", (500, 300), false, &cancel)
            .err()
            .unwrap();
        thread.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(engine.worker.is_none() && engine.cache.is_empty());
    }

    #[test]
    fn unsupported_input_never_starts_worker_or_spends_terminal_budget() {
        let mut engine = Engine::default();
        for source in [
            "gantt\nTask: 2025-01-01, 1d",
            "%%{init: {}}%%\nflowchart LR; A-->B",
            "flowchart LR; A\x1b[2J-->B",
        ] {
            assert!(
                engine
                    .render(source, (500, 300), false, &AtomicBool::new(false))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(engine.worker.is_none());
        assert_eq!(engine.attempts, 0);
    }
}
