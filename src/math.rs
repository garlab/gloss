//! Native math uses katex-native for layout and rasterization, independently
//! of any terminal protocol.

use std::{collections::VecDeque, sync::Arc};

use katex_native::{
    Color,
    katex::{KatexContext, Settings},
    raster,
};

/// Math-specific baseline metadata around the shared pixel representation.
#[derive(Clone)]
pub struct Raster {
    pub pixels: Arc<crate::raster::Raster>,
    pub baseline: f64,
}

impl std::ops::Deref for Raster {
    type Target = crate::raster::Raster;
    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

impl From<raster::Raster> for Raster {
    fn from(value: raster::Raster) -> Self {
        Self {
            baseline: value.baseline,
            pixels: Arc::new(crate::raster::Raster {
                width: value.width,
                height: value.height,
                rgba: value.rgba,
            }),
        }
    }
}

pub struct MathEngine {
    context: KatexContext,
    next_number: u32,
    cache: VecDeque<Entry>,
    cache_bytes: usize,
}

const CACHE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_ENTRIES: usize = 64;
const CACHE_SOURCE_BYTES: usize = 16 * 1024;

#[derive(PartialEq, Eq)]
struct Parameters {
    display: bool,
    font_size: u64,
    color: [u8; 4],
    width: u32,
    height: u32,
    number: u32,
}

struct Entry {
    tex: String,
    parameters: Parameters,
    raster: Raster,
    next_number: u32,
    bytes: usize,
}

impl Default for MathEngine {
    fn default() -> Self {
        Self {
            context: KatexContext::default(),
            next_number: 1,
            cache: VecDeque::new(),
            cache_bytes: 0,
        }
    }
}

impl MathEngine {
    /// Keep bounded fitted rasters across pager renders, but restart numbering.
    pub(crate) fn begin_document(&mut self) {
        self.next_number = 1;
    }

    /// Rasterize an inline formula without advancing display equation numbering.
    pub fn render_inline(
        &mut self,
        tex: &str,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        self.render(tex, false, font_size, color, max_width, max_height)
    }

    /// Rasterize display math, shrinking to fit the given pixel bounds.
    /// Tags follow the body with a one-em gap in this initial implementation.
    pub fn render_display(
        &mut self,
        tex: &str,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        self.render(tex, true, font_size, color, max_width, max_height)
    }

    fn render(
        &mut self,
        tex: &str,
        display: bool,
        font_size: f64,
        color: Color,
        max_width: u32,
        max_height: u32,
    ) -> Result<Raster, String> {
        if !font_size.is_finite() || font_size <= 0. || max_width == 0 || max_height == 0 {
            return Err("invalid formula dimensions".into());
        }
        let parameters = Parameters {
            display,
            font_size: font_size.to_bits(),
            color: [color.r, color.g, color.b, color.a],
            width: max_width,
            height: max_height,
            number: self.next_number,
        };
        if let Some(index) = self
            .cache
            .iter()
            .position(|entry| entry.parameters == parameters && entry.tex == tex)
        {
            let entry = self.cache.remove(index).unwrap();
            self.next_number = entry.next_number;
            let raster = entry.raster.clone();
            self.cache.push_back(entry);
            return Ok(raster);
        }
        let settings = Settings::builder().display_mode(display).build();
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
        let result: Raster = result.into();
        let bytes = result
            .rgba
            .len()
            .saturating_add(tex.len())
            .saturating_add(256);
        if bytes <= CACHE_BYTES && tex.len() <= CACHE_SOURCE_BYTES {
            while self.cache.len() >= CACHE_ENTRIES || self.cache_bytes + bytes > CACHE_BYTES {
                if let Some(entry) = self.cache.pop_front() {
                    self.cache_bytes -= entry.bytes;
                }
            }
            self.cache.push_back(Entry {
                tex: tex.into(),
                parameters,
                raster: result.clone(),
                next_number: self.next_number,
                bytes,
            });
            self.cache_bytes += bytes;
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
        let first = engine.render_inline("x+1", 24., color, 800, 400).unwrap();
        let repeated = engine.render_inline("x+1", 24., color, 800, 400).unwrap();
        assert!(Arc::ptr_eq(&first.pixels, &repeated.pixels));
        let other = engine.render_inline("x+1", 24., color, 40, 400).unwrap();
        assert!(!Arc::ptr_eq(&first.pixels, &other.pixels));
        let other = engine
            .render_inline("x+1", 24., Color { r: 0, ..color }, 800, 400)
            .unwrap();
        assert!(!Arc::ptr_eq(&first.pixels, &other.pixels));
        let formula = r"\begin{equation}x=1\end{equation}";
        let numbered = engine
            .render_display(formula, 24., color, 800, 400)
            .unwrap();
        let second = engine
            .render_display(formula, 24., color, 800, 400)
            .unwrap();
        assert_ne!(numbered.rgba, second.rgba);
        assert_eq!(engine.next_number, 3);
        engine.begin_document();
        let restarted = engine
            .render_display(formula, 24., color, 800, 400)
            .unwrap();
        assert!(Arc::ptr_eq(&numbered.pixels, &restarted.pixels));
        assert_eq!(engine.next_number, 2);
        for index in 0..80 {
            engine
                .render_inline(&format!("x+{index}"), 24., color, 800, 400)
                .unwrap();
        }
        assert!(engine.cache.len() <= CACHE_ENTRIES && engine.cache_bytes <= CACHE_BYTES);
    }
}
