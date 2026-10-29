//! Cell-aware wrapping of text and indivisible equation images. This layer has
//! no terminal escape sequences; it measures Unicode graphemes, not UTF-8 bytes.

use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{math::Raster, terminal::Geometry};

use super::Style;

#[derive(Clone)]
pub enum Part {
    Text {
        text: String,
        style: Style,
    },
    Math {
        image: Arc<Raster>,
        source: String,
        columns: usize,
    },
}

impl Part {
    pub fn columns(&self) -> usize {
        match self {
            Self::Text { text, .. } => UnicodeWidthStr::width(text.as_str()),
            Self::Math { columns, .. } => *columns,
        }
    }
}

#[derive(Default)]
pub struct Line {
    pub parts: Vec<Part>,
    pub columns: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub rows: u32,
    pub text_row: u32,
    pub baseline: f64,
}

impl Line {
    pub fn has_math(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, Part::Math { .. }))
    }

    /// Terminals report cell size, but not the font's baseline. The text baseline
    /// is estimated at 78% of a cell; enough complete rows are reserved above and
    /// below it to keep fractions, roots, and neighboring text from overlapping.
    pub fn metrics(&self, geometry: Geometry) -> Metrics {
        let cell_height = u32::from(geometry.cell_height.max(1));
        let ascent = (f64::from(cell_height) * 0.78).floor();
        let above = self
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Math { image, .. } => Some(image.baseline),
                _ => None,
            })
            .fold(ascent, f64::max);
        let text_row = ((above - ascent).max(0.) / f64::from(cell_height)).ceil() as u32;
        let baseline = f64::from(text_row * cell_height) + ascent;
        let bottom = self
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Math { image, .. } => {
                    Some((baseline - image.baseline).ceil() as u32 + image.height)
                }
                _ => None,
            })
            .fold((text_row + 1) * cell_height, u32::max);
        Metrics {
            rows: bottom.div_ceil(cell_height),
            text_row,
            baseline,
        }
    }

    fn push(&mut self, part: Part) {
        self.columns += part.columns();
        if let Part::Text { text, style } = &part
            && let Some(Part::Text {
                text: previous,
                style: previous_style,
            }) = self.parts.last_mut()
            && previous_style == style
        {
            previous.push_str(text);
            return;
        }
        self.parts.push(part);
    }

    fn trim_end(&mut self) {
        while let Some(Part::Text { text, .. }) = self.parts.last_mut() {
            text.truncate(text.trim_end().len());
            if !text.is_empty() {
                break;
            }
            self.parts.pop();
        }
        self.columns = self.parts.iter().map(Part::columns).sum();
    }
}

/// Equation images must already fit `width`. Adjacent styled text, equations,
/// and punctuation stay together as one word where possible. Oversized text
/// words split only at grapheme boundaries; equation images never split.
pub fn wrap(parts: Vec<Part>, width: usize) -> Vec<Line> {
    let mut builder = Wrapper {
        width: width.max(1),
        ..Default::default()
    };
    for part in parts {
        match part {
            Part::Math { .. } => builder.word.push(part),
            Part::Text { text, style } => {
                for grapheme in text.graphemes(true) {
                    match grapheme {
                        "\n" | "\r\n" => {
                            builder.word();
                            builder.line();
                        }
                        "\t" => {
                            builder.word();
                            let spaces = 4 - builder.current.columns % 4;
                            for _ in 0..spaces {
                                builder.space(" ", style);
                            }
                        }
                        value if value.chars().all(char::is_whitespace) && value != "\u{a0}" => {
                            builder.word();
                            builder.space(value, style);
                        }
                        value => builder.word.push(Part::Text {
                            text: value.to_owned(),
                            style,
                        }),
                    }
                }
            }
        }
    }
    builder.word();
    if !builder.current.parts.is_empty() {
        builder.line();
    }
    builder.lines
}

#[derive(Default)]
struct Wrapper {
    width: usize,
    word: Vec<Part>,
    current: Line,
    lines: Vec<Line>,
}

impl Wrapper {
    fn word(&mut self) {
        let word_width: usize = self.word.iter().map(Part::columns).sum();
        if word_width <= self.width && self.current.columns + word_width > self.width {
            self.line();
        }
        for part in std::mem::take(&mut self.word) {
            if self.current.columns > 0 && self.current.columns + part.columns() > self.width {
                self.line();
            }
            self.current.push(part);
        }
    }

    fn space(&mut self, text: &str, style: Style) {
        let part = Part::Text {
            text: text.to_owned(),
            style,
        };
        if self.current.columns + part.columns() <= self.width {
            self.current.push(part);
        }
    }

    fn line(&mut self) {
        self.current.trim_end();
        self.lines.push(std::mem::take(&mut self.current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Part {
        Part::Text {
            text: value.into(),
            style: Style::default(),
        }
    }

    fn displayed(line: &Line) -> String {
        line.parts
            .iter()
            .map(|part| match part {
                Part::Text { text, .. } => text.as_str(),
                Part::Math { .. } => "[math]",
            })
            .collect()
    }

    fn math(columns: usize, height: u32, baseline: f64) -> Part {
        Part::Math {
            image: Arc::new(Raster {
                width: columns as u32 * 10,
                height,
                baseline,
                rgba: Vec::new(),
            }),
            source: "$x$".into(),
            columns,
        }
    }

    #[test]
    fn wraps_words_across_styles_and_keeps_formula_punctuation() {
        let lines = wrap(
            vec![
                text("Start "),
                text("he"),
                Part::Text {
                    text: "llo".into(),
                    style: Style {
                        bold: true,
                        ..Default::default()
                    },
                },
                text(" "),
                math(4, 20, 15.),
                text(". End"),
            ],
            10,
        );
        assert_eq!(
            lines.iter().map(displayed).collect::<Vec<_>>(),
            ["Start", "hello", "[math]. End"]
        );
        assert!(lines.iter().all(|line| line.columns <= 10));
    }

    #[test]
    fn wraps_unicode_graphemes_without_splitting_emoji_or_accents() {
        let lines = wrap(vec![text("界界 👩‍💻👩‍💻 e\u{301}e\u{301}")], 4);
        assert_eq!(
            lines.iter().map(displayed).collect::<Vec<_>>(),
            ["界界", "👩‍💻👩‍💻", "e\u{301}e\u{301}"]
        );
        assert!(lines.iter().all(|line| line.columns <= 4));
    }

    #[test]
    fn preserves_blank_lines_and_code_indentation() {
        let lines = wrap(vec![text("Title\n\n\tlet x = 1;\n")], 30);
        assert_eq!(
            lines.iter().map(displayed).collect::<Vec<_>>(),
            ["Title", "", "    let x = 1;"]
        );
    }

    #[test]
    fn reserves_rows_on_both_sides_of_the_text_baseline() {
        let lines = wrap(
            vec![text("A "), math(4, 65, 50.), text(" B "), math(3, 55, 15.)],
            40,
        );
        let metrics = lines[0].metrics(Geometry::default());
        assert!(metrics.text_row >= 2);
        assert!(metrics.rows > metrics.text_row + 1);
        for part in &lines[0].parts {
            if let Part::Math { image, .. } = part {
                let top = (metrics.baseline - image.baseline).ceil();
                assert!(top >= 0.);
                assert!(top + f64::from(image.height) <= f64::from(metrics.rows * 20));
            }
        }
    }
}
