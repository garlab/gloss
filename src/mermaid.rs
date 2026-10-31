//! Terminal policy for native diagrams: invocation budgets and fitted-image cache.
use crate::raster::Raster;
pub(crate) use document_graphics::mermaid::MAX_SOURCE;
pub use document_graphics::worker::worker_main;
use document_graphics::{RasterOptions, mermaid::admissible, worker::Worker};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::AtomicBool},
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
                &AtomicBool::new(false),
            )
        })();
        self.elapsed += started.elapsed();
        let pixels = match result {
            Ok(pixels) => pixels.map(|pixels| Arc::new(pixels.into())),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_input_never_starts_worker_or_spends_terminal_budget() {
        let mut engine = Engine::default();
        for source in [
            "gantt\nTask: 2025-01-01, 1d",
            "%%{init: {}}%%\nflowchart LR; A-->B",
            "flowchart LR; A\x1b[2J-->B",
        ] {
            assert!(engine.render(source, (500, 300), false).is_none());
        }
        assert!(engine.worker.is_none());
        assert_eq!(engine.attempts, 0);
    }
}
