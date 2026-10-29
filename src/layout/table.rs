//! Semantic table cells retain decoded inline content and original source spans.

use std::ops::Range;

use pulldown_cmark::Alignment;

use super::{Run, Style};

#[derive(Debug)]
pub struct Cell {
    pub runs: Box<[Run]>,
    pub source: Range<usize>,
}

#[derive(Debug, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub header: bool,
}

#[derive(Debug)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Row>,
    pub prefix: Vec<Run>,
    pub style: Style,
    pub source: Range<usize>,
}

pub(super) struct Builder {
    pub table: Table,
    pub row: Row,
    pub cell_source: Range<usize>,
}

impl Builder {
    pub fn cell(&mut self, runs: Vec<Run>, end: usize) {
        self.row.cells.push(Cell {
            // Most cells contain a single run; do not retain Vec's spare slots
            // in every cell of a large document.
            runs: runs.into_boxed_slice(),
            source: self.cell_source.start..end,
        });
    }

    pub fn row(&mut self) {
        self.row.cells.shrink_to_fit();
        self.table.rows.push(std::mem::take(&mut self.row));
    }
}
