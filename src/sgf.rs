//! Bounded terminal SGF previews using the shared native Go renderer.
use crate::raster::Raster;
use document_graphics::{
    RasterOptions,
    sgf::{self, Collection},
};
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) const MAX_SOURCE: usize = 256 * 1024;
const MAX_NODES: usize = 8192;
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTEMPTS: usize = 32;
const TOTAL_TIME: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// One-based game within an SGF collection.
    pub game: usize,
    /// Main-line move ordinal (passes count); None selects the final node.
    /// Zero selects the root, including its setup and any root move.
    pub at_move: Option<usize>,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            game: 1,
            at_move: None,
        }
    }
}
impl Selection {
    pub(crate) fn attributes(mut self, attributes: Option<&str>) -> Option<Self> {
        let mut game = false;
        let mut at_move = false;
        for word in attributes.unwrap_or_default().split_whitespace() {
            let (key, value) = word.split_once('=')?;
            match key {
                "game" if !game => {
                    self.game = value.parse().ok()?;
                    game = true;
                }
                "move" if !at_move => {
                    self.at_move = if value == "last" {
                        None
                    } else {
                        Some(value.parse().ok()?)
                    };
                    at_move = true;
                }
                _ => return None,
            }
        }
        (self.game > 0).then_some(self)
    }
}

pub(crate) struct Preview {
    pub pixels: Arc<Raster>,
    pub caption: String,
}
struct Cached {
    source: String,
    selection: Selection,
    fitting: RasterOptions,
    light: bool,
    preview: Option<Arc<Preview>>,
}
impl Cached {
    fn bytes(&self) -> usize {
        self.source.len()
            + self
                .preview
                .as_ref()
                .map_or(0, |p| p.pixels.rgba.len() + p.caption.len())
    }
}

#[derive(Default)]
pub(crate) struct Engine {
    attempts: usize,
    elapsed: Duration,
    cache: VecDeque<Cached>,
    cache_bytes: usize,
}
impl Engine {
    pub fn begin_document(&mut self) {
        self.attempts = 0;
        self.elapsed = Duration::ZERO;
    }

    pub fn render(
        &mut self,
        source: &str,
        selection: Selection,
        fitting: RasterOptions,
        light: bool,
        cancel: &AtomicBool,
    ) -> io::Result<Option<Arc<Preview>>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if source.is_empty()
            || source.len() > MAX_SOURCE
            || selection.game == 0
            || fitting.bounds.0 == 0
            || fitting.bounds.1 == 0
            || !fitting.scale.is_finite()
            || fitting.scale <= 0.
            || fitting.scale > 4.
        {
            return Ok(None);
        }
        if let Some(index) = self.cache.iter().position(|c| {
            c.source == source
                && c.selection == selection
                && c.fitting.bounds == fitting.bounds
                && c.fitting.scale == fitting.scale
                && c.light == light
        }) {
            let entry = self.cache.remove(index).unwrap();
            let result = entry.preview.clone();
            self.cache.push_back(entry);
            return Ok(result);
        }
        if self.attempts >= MAX_ATTEMPTS || self.elapsed >= TOTAL_TIME {
            return Ok(None);
        }
        self.attempts += 1;
        let start = Instant::now();
        let rendered = preview(source, selection, fitting, light, cancel);
        self.elapsed += start.elapsed();
        let preview = match rendered {
            Ok(p) => Some(Arc::new(p)),
            Err(sgf::Error::Cancelled) => return Err(io::ErrorKind::ConnectionAborted.into()),
            Err(_) => None,
        };
        let entry = Cached {
            source: source.into(),
            selection,
            fitting,
            light,
            preview: preview.clone(),
        };
        let cost = entry.bytes();
        if cost <= CACHE_BYTES {
            while self.cache_bytes + cost > CACHE_BYTES || self.cache.len() >= 32 {
                self.cache_bytes -= self.cache.pop_front().unwrap().bytes();
            }
            self.cache_bytes += cost;
            self.cache.push_back(entry);
        }
        Ok(preview)
    }
}

fn node(
    collection: &Collection,
    selection: Selection,
    cancel: &AtomicBool,
) -> Result<(usize, usize), sgf::Error> {
    let mut node = collection
        .games()
        .get(selection.game - 1)
        .ok_or(sgf::Error::Unsupported("SGF game is out of range"))?
        .root;
    let mut moves = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(sgf::Error::Cancelled);
        }
        if collection.text(node, "B").is_some() || collection.text(node, "W").is_some() {
            moves += 1;
        }
        if selection.at_move == Some(0) || selection.at_move == Some(moves) {
            return Ok((node, moves));
        }
        match collection.nodes()[node].first_child {
            Some(child) => node = child,
            None if selection.at_move.is_none() => return Ok((node, moves)),
            None => return Err(sgf::Error::Unsupported("SGF move is out of range")),
        }
    }
}

fn preview(
    source: &str,
    selection: Selection,
    fitting: RasterOptions,
    light: bool,
    cancel: &AtomicBool,
) -> Result<Preview, sgf::Error> {
    let collection = Collection::parse(Arc::from(source.as_bytes()), cancel)?;
    if collection.nodes().len() > MAX_NODES {
        return Err(sgf::Error::Limit("SGF preview exceeds 8192 nodes"));
    }
    let (node, moves) = node(&collection, selection, cancel)?;
    let position = collection.position(node, cancel)?;
    let pixels = sgf::rasterize(
        &position,
        sgf::RenderOptions {
            dark: !light,
            move_numbers: false,
        },
        fitting,
        cancel,
    )?;
    Ok(Preview {
        pixels: Arc::new(pixels.into()),
        caption: format!(
            "SGF: game {}/{} · move {moves} · main line",
            selection.game,
            collection.games().len()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str =
        "(;FF[4]GM[1]SZ[9]AB[aa];B[bb];C[Note](;W[];B[cc]MN[20])(;W[dd]))(;GM[1]SZ[9];B[ee])";

    #[test]
    fn selects_main_line_moves_passes_root_and_collection_games() {
        let cancel = AtomicBool::new(false);
        let doc = Collection::parse(Arc::from(SOURCE.as_bytes()), &cancel).unwrap();
        let (last, moves) = node(&doc, Selection::default(), &cancel).unwrap();
        assert_eq!(moves, 3);
        assert_eq!(doc.text(last, "B").as_deref(), Some("cc"));
        let (pass, moves) = node(
            &doc,
            Selection {
                at_move: Some(2),
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(moves, 2);
        assert_eq!(doc.text(pass, "W").as_deref(), Some(""));
        let (root, moves) = node(
            &doc,
            Selection {
                at_move: Some(0),
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(moves, 0);
        assert_eq!(root, doc.games()[0].root);
        let (second, moves) = node(
            &doc,
            Selection {
                game: 2,
                at_move: None,
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(moves, 1);
        assert_eq!(doc.text(second, "B").as_deref(), Some("ee"));
        assert!(
            node(
                &doc,
                Selection {
                    at_move: Some(4),
                    ..Default::default()
                },
                &cancel
            )
            .is_err()
        );
        assert!(
            node(
                &doc,
                Selection {
                    game: 3,
                    at_move: None
                },
                &cancel
            )
            .is_err()
        );
    }

    #[test]
    fn fence_selectors_override_defaults_and_reject_ambiguous_input() {
        let defaults = Selection {
            game: 2,
            at_move: Some(7),
        };
        assert_eq!(
            defaults.attributes(Some("move=last game=1")),
            Some(Selection::default())
        );
        assert_eq!(
            defaults.attributes(Some("move=0")),
            Some(Selection {
                at_move: Some(0),
                ..defaults
            })
        );
        for bad in [
            "game=0",
            "move=-1",
            "game=1 game=2",
            "move=1 move=2",
            "moves=2",
            "move=",
            "extra",
        ] {
            assert!(defaults.attributes(Some(bad)).is_none(), "{bad}");
        }
    }

    #[test]
    fn rendering_cache_respects_selection_geometry_theme_and_document_limits() {
        let cancel = AtomicBool::new(false);
        let mut engine = Engine::default();
        let selection = Selection::default();
        let fitting = RasterOptions::fit((400, 400));
        let first = engine
            .render(SOURCE, selection, fitting, false, &cancel)
            .unwrap()
            .unwrap();
        assert!(first.caption.contains("game 1/2 · move 3"));
        let doc = Collection::parse(Arc::from(SOURCE.as_bytes()), &cancel).unwrap();
        let (last, _) = node(&doc, selection, &cancel).unwrap();
        let expected = sgf::rasterize(
            &doc.position(last, &cancel).unwrap(),
            sgf::RenderOptions {
                dark: true,
                move_numbers: false,
            },
            fitting,
            &cancel,
        )
        .unwrap();
        assert_eq!(first.pixels.rgba, expected.rgba);
        engine.attempts = MAX_ATTEMPTS;
        engine.elapsed = TOTAL_TIME;
        let cached = engine
            .render(SOURCE, selection, fitting, false, &cancel)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &cached));
        for (pick, fit, light) in [
            (
                Selection {
                    at_move: Some(1),
                    ..selection
                },
                fitting,
                false,
            ),
            (selection, RasterOptions::fit((300, 300)), false),
            (
                selection,
                RasterOptions {
                    scale: 2.,
                    ..fitting
                },
                false,
            ),
            (selection, fitting, true),
        ] {
            assert!(
                engine
                    .render(SOURCE, pick, fit, light, &cancel)
                    .unwrap()
                    .is_none()
            );
        }
        engine.begin_document();
        assert!(
            engine
                .render(SOURCE, selection, fitting, true, &cancel)
                .unwrap()
                .is_some()
        );
        assert!(engine.cache_bytes <= CACHE_BYTES);
        assert!(engine.cache.len() <= 32);
    }

    #[test]
    fn invalid_and_excessive_records_keep_source_without_poisoning_later_boards() {
        let cancel = AtomicBool::new(false);
        let mut engine = Engine::default();
        let fit = RasterOptions::fit((300, 300));
        assert!(
            engine
                .render(
                    &"x".repeat(MAX_SOURCE + 1),
                    Selection::default(),
                    fit,
                    false,
                    &cancel
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(engine.attempts, 0);
        for source in [
            "broken",
            "(;GM[2]SZ[9])",
            &format!("(;GM[1]SZ[9]{})", ";".repeat(MAX_NODES)),
        ] {
            assert!(
                engine
                    .render(source, Selection::default(), fit, false, &cancel)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            engine
                .render(SOURCE, Selection::default(), fit, false, &cancel)
                .unwrap()
                .is_some()
        );
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            engine
                .render(SOURCE, Selection::default(), fit, false, &cancel)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::ConnectionAborted
        );
    }
}
