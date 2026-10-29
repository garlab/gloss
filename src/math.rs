//! Native math uses katex-native for layout and rasterization, independently
//! of any terminal protocol.

use katex_native::{
    Color,
    katex::{KatexContext, Settings},
    raster,
};

/// Math-specific baseline metadata around the shared pixel representation.
pub struct Raster {
    pub pixels: crate::raster::Raster,
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
            pixels: crate::raster::Raster {
                width: value.width,
                height: value.height,
                rgba: value.rgba,
            },
        }
    }
}

pub struct MathEngine {
    context: KatexContext,
    next_number: u32,
}

impl Default for MathEngine {
    fn default() -> Self {
        Self {
            context: KatexContext::default(),
            next_number: 1,
        }
    }
}

impl MathEngine {
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
        Ok(result.into())
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
