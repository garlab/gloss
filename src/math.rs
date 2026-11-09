//! Native math uses katex-native for layout and rasterization, independently
//! of any terminal protocol.

use std::{
    collections::{HashMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use katex_native::{
    Color,
    katex::{KatexContext, Settings, macro_expander::MacroMap, macros::MacroDefinition},
    raster,
};

/// Math-specific baseline metadata around compressed pixels. Equations are
/// placed whole at native size, so their pixels are compressed once, right
/// after rasterization, and cache hits transmit the same bytes.
#[derive(Clone)]
pub struct Raster {
    pub pixels: Arc<crate::raster::Compressed>,
    pub baseline: f64,
}

impl std::ops::Deref for Raster {
    type Target = crate::raster::Compressed;
    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

impl TryFrom<raster::Raster> for Raster {
    type Error = String;
    fn try_from(value: raster::Raster) -> Result<Self, String> {
        let pixels = crate::raster::Raster {
            width: value.width,
            height: value.height,
            rgba: value.rgba,
        };
        Ok(Self {
            baseline: value.baseline,
            pixels: Arc::new(pixels.compress().map_err(|error| error.to_string())?),
        })
    }
}

pub struct MathEngine {
    context: KatexContext,
    next_number: u32,
    cache: Cache,
    /// A LaTeX document's index: formulas use the macros visible where they start.
    document: Option<Arc<latex_reader::Index>>,
    /// KaTeX macro maps built per visible-binding hash, most recent last.
    macro_maps: VecDeque<(u128, MacroMap)>,
}

const MACRO_MAPS: usize = 8;

/// Compressed bytes, source, and a fixed overhead count toward the budget.
/// Compressed equations are several times smaller than their RGBA pixels.
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_ENTRIES: usize = 4096;
const CACHE_SOURCE_BYTES: usize = 16 * 1024;

#[derive(PartialEq, Eq, Hash)]
struct Parameters {
    display: bool,
    font_size: u64,
    color: [u8; 4],
    width: u32,
    height: u32,
    number: u32,
    macros: u128,
}

struct Entry {
    tex: String,
    parameters: Parameters,
    raster: Raster,
    next_number: u32,
    bytes: usize,
    /// When the entry was last used; older positions in `order` are stale.
    used: u64,
}

/// Least-recently-used rasters, indexed by a hash of formula and parameters.
/// A hash collision is a miss: entries are compared in full.
#[derive(Default)]
struct Cache {
    entries: HashMap<u64, Entry>,
    /// Uses in order, oldest first; an entry's latest use is the live one.
    order: VecDeque<(u64, u64)>,
    clock: u64,
    bytes: usize,
}

impl Cache {
    fn key(tex: &str, parameters: &Parameters) -> u64 {
        let mut hasher = DefaultHasher::new();
        tex.hash(&mut hasher);
        parameters.hash(&mut hasher);
        hasher.finish()
    }

    fn get(&mut self, key: u64, tex: &str, parameters: &Parameters) -> Option<(Raster, u32)> {
        let entry = self.entries.get_mut(&key)?;
        if entry.tex != tex || entry.parameters != *parameters {
            return None;
        }
        self.clock += 1;
        entry.used = self.clock;
        let found = (entry.raster.clone(), entry.next_number);
        self.order.push_back((key, self.clock));
        // Repeated hits leave stale positions; drop them before they dominate.
        if self.order.len() > 2 * self.entries.len() + 64 {
            let entries = &self.entries;
            self.order
                .retain(|(key, used)| entries.get(key).is_some_and(|e| e.used == *used));
        }
        Some(found)
    }

    fn insert(&mut self, key: u64, mut entry: Entry) {
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        while self.entries.len() >= CACHE_ENTRIES || self.bytes + entry.bytes > CACHE_BYTES {
            let Some((key, used)) = self.order.pop_front() else {
                break;
            };
            if self.entries.get(&key).is_some_and(|e| e.used == used) {
                let old = self.entries.remove(&key).unwrap();
                self.bytes -= old.bytes;
            }
        }
        self.clock += 1;
        entry.used = self.clock;
        self.order.push_back((key, self.clock));
        self.bytes += entry.bytes;
        self.entries.insert(key, entry);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for MathEngine {
    fn default() -> Self {
        Self {
            context: KatexContext::default(),
            next_number: 1,
            cache: Cache::default(),
            document: None,
            macro_maps: VecDeque::new(),
        }
    }
}

impl MathEngine {
    /// Keep bounded fitted rasters across pager renders, but restart numbering.
    pub(crate) fn begin_document(&mut self) {
        self.next_number = 1;
    }

    /// Formulas of a LaTeX document see the macros visible at their offset.
    pub(crate) fn set_document(&mut self, document: Option<Arc<latex_reader::Index>>) {
        self.document = document;
    }

    /// Builds the visible bindings into a KaTeX macro map. Plain bodies go in
    /// directly; other definitions are replayed with persistence enabled.
    fn macro_map(&mut self, macros: &latex_reader::MacroContext) -> MacroMap {
        if let Some(index) = self
            .macro_maps
            .iter()
            .position(|(hash, _)| *hash == macros.hash())
        {
            let entry = self.macro_maps.remove(index).unwrap();
            let map = entry.1.clone();
            self.macro_maps.push_back(entry);
            return map;
        }
        let settings = Settings::builder()
            .global_group(true)
            .max_expand(1000)
            .build();
        for binding in macros.bindings() {
            match binding.simple_body() {
                Some(body) => {
                    settings.macros.borrow_mut().insert(
                        binding.name.clone(),
                        MacroDefinition::String(body.to_owned()),
                    );
                }
                None => {
                    if let Some(definition) = binding.katex_definition() {
                        let _ = katex_native::katex::parse_tree::parse_tree(
                            &self.context,
                            &definition,
                            &settings,
                        );
                    }
                }
            }
        }
        let map = settings.macros.into_inner();
        if self.macro_maps.len() >= MACRO_MAPS {
            self.macro_maps.pop_front();
        }
        self.macro_maps.push_back((macros.hash(), map.clone()));
        map
    }

    /// Rasterize an inline formula without advancing display equation numbering.
    pub fn render_inline(
        &mut self,
        tex: &str,
        at: Option<usize>,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        self.render(tex, at, false, font_size, color, max_width, max_height)
    }

    /// Rasterize display math, shrinking to fit the given pixel bounds.
    /// Tags follow the body with a one-em gap in this initial implementation.
    pub fn render_display(
        &mut self,
        tex: &str,
        at: Option<usize>,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        self.render(tex, at, true, font_size, color, max_width, max_height)
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        tex: &str,
        at: Option<usize>,
        display: bool,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        if !font_size.is_finite() || font_size <= 0. || max_width == 0 || max_height == 0 {
            return Err("invalid formula dimensions".into());
        }
        let macros = match (&self.document, at) {
            (Some(document), Some(at)) => Some(document.macro_context(at)),
            _ => None,
        };
        if macros.as_ref().is_some_and(|m| m.is_incomplete()) {
            return Err("a macro definition before this formula could not be applied".into());
        }
        let macros = macros.filter(|m| !m.is_empty());
        let parameters = Parameters {
            display,
            font_size: font_size.to_bits(),
            color: [color.r, color.g, color.b, color.a],
            width: max_width,
            height: max_height,
            number: self.next_number,
            macros: macros.as_ref().map_or(0, |m| m.hash()),
        };
        let key = Cache::key(tex, &parameters);
        if let Some((raster, next_number)) = self.cache.get(key, tex, &parameters) {
            self.next_number = next_number;
            return Ok(raster);
        }
        let settings = match &macros {
            Some(macros) => Settings::builder()
                .display_mode(display)
                .macros(self.macro_map(macros))
                .build(),
            None => Settings::builder().display_mode(display).build(),
        };
        let layout = katex_native::layout_numbered(&self.context, tex, &settings, self.next_number)
            .map_err(|err| err.to_string())?;
        let tag_width = layout.tag.as_ref().map_or(0., |tag| 1. + tag.width);
        let above = layout
            .height
            .max(layout.tag.as_ref().map_or(0., |t| t.height))
            .max(0.);
        let below = layout
            .depth
            .max(layout.tag.as_ref().map_or(0., |t| t.depth))
            .max(0.);
        let width = layout.width + tag_width;
        let height = above + below;
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
            return Err("the formula is empty or has invalid dimensions".into());
        }
        // Leave a few pixels for rounding when aligning separately rasterized tags.
        let scale = font_size
            .min(f64::from(max_width.saturating_sub(4).max(1)) / width)
            .min(f64::from(max_height.saturating_sub(4).max(1)) / height);
        if width * height * scale * scale > 8_000_000. {
            return Err("the formula exceeds the raster size limit".into());
        }
        let body =
            raster::rasterize(&layout, scale, color).ok_or("could not rasterize the formula")?;
        let result = if let Some(tag) = &layout.tag {
            let tag = raster::rasterize(tag, scale, color)
                .ok_or("could not rasterize the equation tag")?;
            beside(body, tag, scale.ceil() as u32)?
        } else {
            body
        };
        if display {
            self.next_number = self.next_number.saturating_add(layout.equation_numbers);
        }
        let result = Raster::try_from(result)?;
        let bytes = result
            .data
            .len()
            .saturating_add(tex.len())
            .saturating_add(256);
        if bytes <= CACHE_BYTES && tex.len() <= CACHE_SOURCE_BYTES {
            self.cache.insert(
                key,
                Entry {
                    tex: tex.into(),
                    parameters,
                    raster: result.clone(),
                    next_number: self.next_number,
                    bytes,
                    used: 0,
                },
            );
        }
        Ok(result)
    }
}

fn beside(body: raster::Raster, tag: raster::Raster, gap: u32) -> Result<raster::Raster, String> {
    let baseline = body.baseline.max(tag.baseline).ceil().max(0.);
    let body_top = (baseline - body.baseline).ceil().max(0.) as u32;
    let tag_top = (baseline - tag.baseline).ceil().max(0.) as u32;
    let width = body
        .width
        .checked_add(gap)
        .and_then(|n| n.checked_add(tag.width))
        .ok_or("formula width overflow")?;
    let height = body_top
        .checked_add(body.height)
        .zip(tag_top.checked_add(tag.height))
        .map(|(a, b)| a.max(b))
        .ok_or("formula height overflow")?;
    if u64::from(width) * u64::from(height) > 8_000_000 {
        return Err("the formula exceeds the raster size limit".into());
    }
    let mut result = raster::Raster {
        width,
        height,
        baseline,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    for (raster, x, y) in [(&body, 0, body_top), (&tag, body.width + gap, tag_top)] {
        for row in 0..raster.height {
            let from = row as usize * raster.width as usize * 4;
            let to = ((row + y) as usize * width as usize + x as usize) * 4;
            let bytes = raster.width as usize * 4;
            result.rgba[to..to + bytes].copy_from_slice(&raster.rgba[from..from + bytes]);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_shares_pixels_but_separates_bounds_colors_and_numbering() {
        let mut engine = MathEngine::default();
        let color = Color {
            r: 200,
            g: 200,
            b: 200,
            a: 255,
        };
        let first = engine
            .render_inline("x+1", None, 24., color, 800, 400)
            .unwrap();
        let repeated = engine
            .render_inline("x+1", None, 24., color, 800, 400)
            .unwrap();
        assert!(Arc::ptr_eq(&first.pixels, &repeated.pixels));
        let other = engine
            .render_inline("x+1", None, 24., color, 40, 400)
            .unwrap();
        assert!(!Arc::ptr_eq(&first.pixels, &other.pixels));
        let other = engine
            .render_inline("x+1", None, 24., Color { r: 0, ..color }, 800, 400)
            .unwrap();
        assert!(!Arc::ptr_eq(&first.pixels, &other.pixels));
        let formula = r"\begin{equation}x=1\end{equation}";
        let numbered = engine
            .render_display(formula, None, 24., color, 800, 400)
            .unwrap();
        let second = engine
            .render_display(formula, None, 24., color, 800, 400)
            .unwrap();
        assert_ne!(numbered.data, second.data);
        assert_eq!(engine.next_number, 3);
        engine.begin_document();
        let restarted = engine
            .render_display(formula, None, 24., color, 800, 400)
            .unwrap();
        assert!(Arc::ptr_eq(&numbered.pixels, &restarted.pixels));
        assert_eq!(engine.next_number, 2);
    }

    #[test]
    fn cache_evicts_least_recently_used_entries_within_both_budgets() {
        let parameters = |number| Parameters {
            display: false,
            font_size: 0,
            color: [0; 4],
            width: 1,
            height: 1,
            number,
            macros: 0,
        };
        let entry = |tex: &str, number, bytes| Entry {
            tex: tex.into(),
            parameters: parameters(number),
            raster: Raster {
                pixels: Arc::new(crate::raster::Compressed {
                    width: 1,
                    height: 1,
                    data: Vec::new(),
                    zlib: false,
                }),
                baseline: 0.,
            },
            next_number: number,
            bytes,
            used: 0,
        };
        let mut cache = Cache::default();
        let kept = Cache::key("kept", &parameters(1));
        cache.insert(kept, entry("kept", 1, 300));
        for index in 0..CACHE_ENTRIES + 100 {
            let tex = format!("x+{index}");
            cache.insert(Cache::key(&tex, &parameters(1)), entry(&tex, 1, 300));
            // Recently used entries survive eviction.
            assert!(cache.get(kept, "kept", &parameters(1)).is_some());
            assert!(cache.get(kept, "kept", &parameters(2)).is_none());
            assert!(cache.order.len() <= 2 * cache.len() + 65);
        }
        assert_eq!(cache.len(), CACHE_ENTRIES);
        assert!(
            cache
                .get(Cache::key("x+0", &parameters(1)), "x+0", &parameters(1))
                .is_none()
        );
        // Bytes bound the cache before the entry count does.
        let large = CACHE_BYTES / 10;
        for index in 0..20 {
            let tex = format!("large {index}");
            cache.insert(Cache::key(&tex, &parameters(1)), entry(&tex, 1, large));
        }
        assert!(cache.bytes <= CACHE_BYTES && cache.len() < 20);
        let sum: usize = cache.entries.values().map(|e| e.bytes).sum();
        assert_eq!(sum, cache.bytes);
    }
}
