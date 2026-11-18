use crate::domain::addr::{CellAddr, CellRange};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    Cell,
    Row,
    Column,
    Block,
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

    pub fn column(anchor: CellAddr) -> Self {
        Self {
            mode: SelectionMode::Column,
            anchor,
            cursor: anchor,
        }
    }

    pub fn block(anchor: CellAddr) -> Self {
        Self {
            mode: SelectionMode::Block,
            anchor,
            cursor: anchor,
        }
    }

    pub fn extend(&mut self, cursor: CellAddr) {
        self.cursor = cursor;
    }

    pub fn range(&self, rows: u32, cols: u32) -> CellRange {
        match self.mode {
            SelectionMode::Cell | SelectionMode::Block => CellRange::new(self.anchor, self.cursor),
            SelectionMode::Row => CellRange::new(
                CellAddr::new(self.anchor.row, 0),
                CellAddr::new(self.cursor.row, cols.saturating_sub(1)),
            ),
            SelectionMode::Column => CellRange::new(
                CellAddr::new(0, self.anchor.col),
                CellAddr::new(rows.saturating_sub(1), self.cursor.col),
            ),
        }
    }

    pub fn contains(&self, rows: u32, cols: u32, addr: CellAddr) -> bool {
        self.range(rows, cols).contains(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(row: u32, col: u32) -> CellAddr {
        CellAddr::new(row, col)
    }

    #[test]
    fn cell_and_block_ranges_use_anchor_and_cursor() {
        let mut selection = Selection::block(addr(2, 3));
        selection.extend(addr(4, 1));
        assert_eq!(
            selection.range(10, 10),
            CellRange::new(addr(2, 1), addr(4, 3))
        );
    }

    #[test]
    fn row_selection_spans_all_columns() {
        let mut selection = Selection::row(addr(2, 3));
        selection.extend(addr(4, 1));
        assert_eq!(
            selection.range(10, 8),
            CellRange::new(addr(2, 0), addr(4, 7))
        );
    }

    #[test]
    fn column_selection_spans_all_rows() {
        let mut selection = Selection::column(addr(2, 3));
        selection.extend(addr(4, 1));
        assert_eq!(
            selection.range(10, 8),
            CellRange::new(addr(0, 1), addr(9, 3))
        );
    }
}
