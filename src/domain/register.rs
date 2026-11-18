use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterKind {
    Cells,
    Rows,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteSource {
    Unnamed,
    Yank,
    System,
}

#[derive(Clone, Debug)]
pub struct ClipboardBlock {
    pub kind: RegisterKind,
    pub width: u32,
    pub height: u32,
    /// A dense block writes empty cells over the target. Sparse row/column
    /// registers only write the cells they contain.
    pub complete: bool,
    /// Cell positions are local to the block: 0..width and 0..height.
    pub cells: BTreeMap<(u32, u32), Cell>,
}

impl ClipboardBlock {
    pub fn new(kind: RegisterKind, width: u32, height: u32, complete: bool) -> Self {
        Self {
            kind,
            width,
            height,
            complete,
            cells: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, row: u32, col: u32, cell: Cell) {
        if row < self.height && col < self.width {
            self.cells.insert((row, col), cell);
        }
    }

    pub fn cell(&self, row: u32, col: u32) -> Option<&Cell> {
        self.cells.get(&(row, col))
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cell_count(&self) -> usize {
        (self.width as usize).saturating_mul(self.height as usize)
    }
}

#[derive(Clone, Debug, Default)]
pub struct RegisterStore {
    unnamed: Option<ClipboardBlock>,
    yank: Option<ClipboardBlock>,
    last_written_text: Option<String>,
}

impl RegisterStore {
    pub fn set_yank(&mut self, block: ClipboardBlock) {
        self.unnamed = Some(block.clone());
        self.yank = Some(block);
    }

    pub fn set_cut(&mut self, block: ClipboardBlock) {
        self.unnamed = Some(block);
    }

    pub fn get(&self, source: PasteSource) -> Option<&ClipboardBlock> {
        match source {
            PasteSource::Unnamed => self.unnamed.as_ref(),
            PasteSource::Yank => self.yank.as_ref(),
            PasteSource::System => None,
        }
    }

    pub fn note_write_success(&mut self, text: String) {
        self.last_written_text = Some(text);
    }

    pub fn last_text_matches(&self, text: &str) -> bool {
        self.last_written_text.as_deref() == Some(text)
    }

    pub fn clear(&mut self) {
        self.unnamed = None;
        self.yank = None;
        self.last_written_text = None;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RegisterLimits {
    pub max_cells: usize,
}

impl Default for RegisterLimits {
    fn default() -> Self {
        Self { max_cells: 200_000 }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RegisterError {
    #[error("selection is too large: {cells} cells (limit {limit})")]
    TooLarge { cells: usize, limit: usize },
    #[error("nothing to paste")]
    Empty,
    #[error("invalid TSV: {message}")]
    InvalidTsv { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cell::CellValue;
    use crate::domain::style::CellStyle;

    fn cell(value: &str) -> Cell {
        Cell::new(CellValue::Text(value.to_string()), CellStyle::default())
    }

    #[test]
    fn block_keeps_local_coordinates() {
        let mut block = ClipboardBlock::new(RegisterKind::Rows, 5, 1, false);
        block.insert(0, 4, cell("x"));
        assert_eq!(block.cell(0, 4).unwrap().display, "x");
        assert!(block.cell(0, 0).is_none());
    }

    #[test]
    fn yank_updates_both_registers_and_cut_only_unnamed() {
        let mut store = RegisterStore::default();
        store.set_yank(ClipboardBlock::new(RegisterKind::Cells, 1, 1, true));
        assert!(store.get(PasteSource::Unnamed).is_some());
        assert!(store.get(PasteSource::Yank).is_some());
        store.set_cut(ClipboardBlock::new(RegisterKind::Cells, 1, 1, true));
        assert!(store.get(PasteSource::Unnamed).is_some());
    }

    #[test]
    fn last_text_tracks_successful_writes_only() {
        let mut store = RegisterStore::default();
        assert!(!store.last_text_matches("a"));
        store.note_write_success("a".to_string());
        assert!(store.last_text_matches("a"));
    }
}
