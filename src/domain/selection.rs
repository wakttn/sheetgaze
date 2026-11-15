use crate::domain::addr::{CellAddr, CellRange};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    Cell,
    Row,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub mode: SelectionMode,
    pub anchor: CellAddr,
    pub cursor: CellAddr,
}

impl Selection {
    pub fn new(anchor: CellAddr) -> Self {
        Self {
            mode: SelectionMode::Cell,
            anchor,
            cursor: anchor,
        }
    }

    pub fn row(anchor: CellAddr) -> Self {
        Self {
            mode: SelectionMode::Row,
            anchor,
            cursor: anchor,
        }
    }

    pub fn extend(&mut self, cursor: CellAddr) {
        self.cursor = cursor;
    }

    pub fn range(&self, row_cols: u32) -> CellRange {
        match self.mode {
            SelectionMode::Cell => CellRange::new(self.anchor, self.cursor),
            SelectionMode::Row => CellRange::new(
                CellAddr::new(self.anchor.row, 0),
                CellAddr::new(self.cursor.row, row_cols.saturating_sub(1)),
            ),
        }
    }

    pub fn contains(&self, row_cols: u32, addr: CellAddr) -> bool {
        self.range(row_cols).contains(addr)
    }
}
