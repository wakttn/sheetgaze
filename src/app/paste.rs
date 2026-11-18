use crate::domain::addr::CellAddr;
use crate::domain::cell::{Cell, CellValue};
use crate::domain::register::{ClipboardBlock, RegisterError, RegisterKind};
use crate::domain::sheet::Sheet;
use crate::domain::structure::{Axis, StructureOp, StructureWarning};

#[derive(Clone, Debug)]
pub struct CellWrite {
    pub addr: CellAddr,
    pub cell: Cell,
}

#[derive(Clone, Debug)]
pub struct PastePlan {
    pub sheet: usize,
    pub inserts: Vec<StructureOp>,
    pub writes: Vec<CellWrite>,
    pub warnings: Vec<StructureWarning>,
    pub focus: CellAddr,
}

pub struct PastePlanner;

impl PastePlanner {
    pub fn plan(
        sheet: &Sheet,
        sheet_index: usize,
        focus: CellAddr,
        before: bool,
        block: &ClipboardBlock,
    ) -> Result<PastePlan, RegisterError> {
        let mut writes = Vec::new();
        let mut inserts = Vec::new();
        let mut target = focus;
        match block.kind {
            RegisterKind::Rows => {
                let index = if before {
                    focus.row
                } else {
                    focus.row.saturating_add(1)
                }
                .min(sheet.meta.row_count);
                inserts.push(StructureOp::insert(Axis::Row, index, block.height));
                for (&(row, col), cell) in &block.cells {
                    writes.push(CellWrite {
                        addr: CellAddr::new(index.saturating_add(row), col),
                        cell: cell.clone(),
                    });
                }
                target = CellAddr::new(index, 0);
            }
            RegisterKind::Columns => {
                let index = if before {
                    focus.col
                } else {
                    focus.col.saturating_add(1)
                }
                .min(sheet.meta.column_count);
                inserts.push(StructureOp::insert(Axis::Column, index, block.width));
                for (&(row, col), cell) in &block.cells {
                    writes.push(CellWrite {
                        addr: CellAddr::new(row, index.saturating_add(col)),
                        cell: cell.clone(),
                    });
                }
                target = CellAddr::new(0, index);
            }
            RegisterKind::Cells => {
                if block.complete {
                    for row in 0..block.height {
                        for col in 0..block.width {
                            let cell = block.cell(row, col).cloned().unwrap_or_else(empty_cell);
                            writes.push(CellWrite {
                                addr: CellAddr::new(
                                    focus.row.saturating_add(row),
                                    focus.col.saturating_add(col),
                                ),
                                cell,
                            });
                        }
                    }
                } else {
                    for (&(row, col), cell) in &block.cells {
                        writes.push(CellWrite {
                            addr: CellAddr::new(
                                focus.row.saturating_add(row),
                                focus.col.saturating_add(col),
                            ),
                            cell: cell.clone(),
                        });
                    }
                }
            }
        }
        Ok(PastePlan {
            sheet: sheet_index,
            inserts,
            writes,
            warnings: Vec::new(),
            focus: target,
        })
    }
}

fn empty_cell() -> Cell {
    Cell::new(CellValue::Empty, Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cell::CellValue;
    use crate::domain::filter::VisibilityMap;
    use crate::domain::formula::FormulaOverlay;
    use crate::domain::freeze::FreezePane;
    use crate::domain::layout::{LayoutLimits, LayoutSnapshot};
    use crate::domain::merge::MergeIndex;
    use crate::domain::sheet::{Sheet, SheetData, SheetMeta};
    use std::collections::HashMap;

    fn sheet() -> Sheet {
        let (origin_rows, origin_cols) = Sheet::identity_origins(3, 3);
        Sheet {
            name: "Sheet1".to_string(),
            meta: SheetMeta {
                row_count: 3,
                column_count: 3,
            },
            used_range: crate::domain::addr::CellRange::new(
                CellAddr::new(0, 0),
                CellAddr::new(2, 2),
            ),
            data_rows: Some((0, 2)),
            data: SheetData::Eager(HashMap::new()),
            edits: HashMap::new(),
            origin_rows,
            origin_cols,
            structural_dirty: false,
            formula_rewrites: Default::default(),
            merges: MergeIndex::empty(),
            merge_borders: HashMap::new(),
            layout: LayoutSnapshot::default(),
            layout_limits: LayoutLimits::default(),
            freeze: FreezePane::none(),
            conditional_formats: Vec::new(),
            filters: Vec::new(),
            filter_masks: Default::default(),
            filter_masks_dirty: false,
            filter_state: Default::default(),
            visibility: VisibilityMap::all(3),
            formula_overlay: FormulaOverlay::default(),
            formula_cells: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn plans_row_insert_and_writes() {
        let mut block = ClipboardBlock::new(RegisterKind::Rows, 3, 1, false);
        block.insert(0, 1, Cell::new(CellValue::Number(1.0), Default::default()));
        let plan = PastePlanner::plan(&sheet(), 0, CellAddr::new(1, 1), false, &block).unwrap();
        assert_eq!(plan.inserts, vec![StructureOp::insert(Axis::Row, 2, 1)]);
        assert_eq!(plan.writes[0].addr, CellAddr::new(2, 1));
        assert_eq!(plan.focus, CellAddr::new(2, 0));
    }

    #[test]
    fn plans_dense_cell_block_overwrite() {
        let mut block = ClipboardBlock::new(RegisterKind::Cells, 2, 1, true);
        block.insert(
            0,
            0,
            Cell::new(CellValue::Text("a".to_string()), Default::default()),
        );
        let plan = PastePlanner::plan(&sheet(), 0, CellAddr::new(1, 1), true, &block).unwrap();
        assert_eq!(plan.inserts, Vec::<StructureOp>::new());
        assert_eq!(plan.writes.len(), 2);
        assert_eq!(plan.writes[1].addr, CellAddr::new(1, 2));
    }
}
