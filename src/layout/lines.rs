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

    fn push_text(&mut self, text: &str, style: Style, columns: usize) {
        if text.is_empty() {
            return;
        }
        self.columns += columns;
        if let Some(Part::Text {
            text: previous,
            style: previous_style,
        }) = self.parts.last_mut()
            && *previous_style == style
        {
            previous.push_str(text);
            return;
        }
        self.parts.push(Part::Text {
            text: text.to_owned(),
            style,
        });
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
    for part in &parts {
        match part {
            Part::Math { columns, .. } => {
                builder.word.push(Fragment::Math(part));
                builder.word_columns += columns;
            }
            Part::Text { text, style } => {
                let mut start = 0;
                let mut columns = 0;
                for (offset, grapheme) in text.grapheme_indices(true) {
                    let whitespace =
                        grapheme.chars().all(char::is_whitespace) && grapheme != "\u{a0}";
                    if !whitespace {
                        columns += UnicodeWidthStr::width(grapheme);
                        continue;
                    }
                    builder.fragment(&text[start..offset], *style, columns);
                    start = offset + grapheme.len();
                    columns = 0;
                    match grapheme {
                        "\n" | "\r\n" => {
                            builder.word();
                            builder.line();
                        }
                        "\t" => {
                            builder.word();
                            let spaces = 4 - builder.current.columns % 4;
                            for _ in 0..spaces {
                                builder.space(" ", *style, 1);
                            }
                        }
                        value => {
                            builder.word();
                            builder.space(value, *style, UnicodeWidthStr::width(value));
                        }
                    }
                }
                builder.fragment(&text[start..], *style, columns);
            }
        }
    }
    builder.word();
    if !builder.current.parts.is_empty() {
        builder.line();
    }
    builder.lines
}

enum Fragment<'a> {
    Text {
        text: &'a str,
        style: Style,
        columns: usize,
    },
    Math(&'a Part),
}

#[derive(Default)]
struct Wrapper<'a> {
    width: usize,
    word: Vec<Fragment<'a>>,
    word_columns: usize,
    current: Line,
    lines: Vec<Line>,
}

impl<'a> Wrapper<'a> {
    fn fragment(&mut self, text: &'a str, style: Style, columns: usize) {
        if !text.is_empty() {
            self.word.push(Fragment::Text {
                text,
                style,
                columns,
            });
            self.word_columns += columns;
        }
    }

    fn word(&mut self) {
        let word_width = std::mem::take(&mut self.word_columns);
        if word_width <= self.width && self.current.columns + word_width > self.width {
            self.line();
        }
        // Reuse the word buffer; text fragments borrow their input and allocate
        // only when appended to an output line.
        let mut word = std::mem::take(&mut self.word);
        for fragment in word.drain(..) {
            match fragment {
                Fragment::Text {
                    text,
                    style,
                    columns,
                } => {
                    if self.current.columns + columns <= self.width {
                        self.current.push_text(text, style, columns);
                    } else {
                        self.split_text(text, style);
                    }
                }
                Fragment::Math(part) => {
                    if self.current.columns > 0
                        && self.current.columns + part.columns() > self.width
                    {
                        self.line();
                    }
                    self.current.columns += part.columns();
                    self.current.parts.push(part.clone());
                }
            }
        }
        self.word = word;
    }

    fn split_text(&mut self, text: &str, style: Style) {
        let mut start = 0;
        let mut columns = 0;
        for (offset, grapheme) in text.grapheme_indices(true) {
            let size = UnicodeWidthStr::width(grapheme);
            if self.current.columns + columns > 0
                && self.current.columns + columns + size > self.width
            {
                self.current.push_text(&text[start..offset], style, columns);
                self.line();
                start = offset;
                columns = 0;
            }
            columns += size;
        }
        self.current.push_text(&text[start..], style, columns);
    }

    fn space(&mut self, text: &str, style: Style, columns: usize) {
        if self.current.columns + columns <= self.width {
            self.current.push_text(text, style, columns);
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
    fn splits_oversized_words_across_style_boundaries_without_losing_text() {
        let lines = wrap(
            vec![
                text("X ab"),
                Part::Text {
                    text: "cdefghij".into(),
                    style: Style {
                        bold: true,
                        ..Default::default()
                    },
                },
                text("kl"),
            ],
            4,
        );
        assert_eq!(
            lines.iter().map(displayed).collect::<Vec<_>>(),
            ["X ab", "cdef", "ghij", "kl"]
        );
        assert!(
            lines[1]
                .parts
                .iter()
                .all(|part| matches!(part, Part::Text { style, .. } if style.bold))
        );
        let wide = wrap(vec![text("界e\u{301}界")], 1);
        assert_eq!(
            wide.iter().map(displayed).collect::<Vec<_>>(),
            ["界", "e\u{301}", "界"]
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
