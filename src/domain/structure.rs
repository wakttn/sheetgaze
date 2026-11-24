use std::collections::{BTreeMap, HashMap};

use thiserror::Error;

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::Cell;
use crate::domain::conditional::ConditionalFormat;
use crate::domain::filter::{FilterDefinition, FilterState};
use crate::domain::formula::{FormulaCellResult, FormulaOverlay};
use crate::domain::freeze::FreezePane;
use crate::domain::layout::LayoutSnapshot;
use crate::domain::merge::{MergeIndex, MergeRegion};
use crate::domain::sheet::{Sheet, SheetData, SheetMeta};
use crate::domain::style::CellBorders;

pub const EXCEL_MAX_ROWS: u32 = 1_048_576;
pub const EXCEL_MAX_COLS: u32 = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureMode {
    Insert,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructureOp {
    pub axis: Axis,
    pub mode: StructureMode,
    pub index: u32,
    pub count: u32,
}

impl StructureOp {
    pub fn insert(axis: Axis, index: u32, count: u32) -> Self {
        Self {
            axis,
            mode: StructureMode::Insert,
            index,
            count,
        }
    }

    pub fn delete(axis: Axis, index: u32, count: u32) -> Self {
        Self {
            axis,
            mode: StructureMode::Delete,
            index,
            count,
        }
    }

    pub fn axis_label(self) -> &'static str {
        match self.axis {
            Axis::Row => "row",
            Axis::Column => "column",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OriginMap {
    pub origin_rows: Vec<Option<u32>>,
    pub origin_cols: Vec<Option<u32>>,
}

impl OriginMap {
    pub fn from_sheet(sheet: &Sheet) -> Self {
        Self {
            origin_rows: sheet.origin_rows.clone(),
            origin_cols: sheet.origin_cols.clone(),
        }
    }

    /// Maps an original range to the current coordinates. Returns `None` when
    /// no row or column in the original range still exists.
    pub fn transform_range(&self, range: CellRange) -> Option<CellRange> {
        let rows = map_axis(&self.origin_rows, range.start.row, range.end.row)?;
        let cols = map_axis(&self.origin_cols, range.start.col, range.end.col)?;
        Some(CellRange::new(
            CellAddr::new(rows.0, cols.0),
            CellAddr::new(rows.1, cols.1),
        ))
    }

    pub fn current_row(&self, original: u32) -> Option<u32> {
        self.origin_rows
            .iter()
            .position(|origin| *origin == Some(original))
            .map(|index| index as u32)
    }

    pub fn current_col(&self, original: u32) -> Option<u32> {
        self.origin_cols
            .iter()
            .position(|origin| *origin == Some(original))
            .map(|index| index as u32)
    }
}

fn map_axis(origin: &[Option<u32>], start: u32, end: u32) -> Option<(u32, u32)> {
    let mut found = origin
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_some_and(|value| value >= start && value <= end))
        .map(|(index, _)| index as u32);
    let first = found.next()?;
    let last = found.next_back().unwrap_or(first);
    Some((first, last))
}

#[derive(Clone, Debug, Default)]
pub struct RemovedCells {
    pub base: Vec<(CellAddr, Cell)>,
    pub edits: Vec<(CellAddr, Cell)>,
    pub formula_overlay: Vec<(CellAddr, FormulaCellResult)>,
}

#[derive(Clone, Debug)]
pub struct StructureSnapshot {
    pub origin_rows: Vec<Option<u32>>,
    pub origin_cols: Vec<Option<u32>>,
    pub merges: Vec<MergeRegion>,
    pub merge_borders: HashMap<CellAddr, CellBorders>,
    pub filters: Vec<FilterDefinition>,
    pub filter_state: FilterState,
    pub layout: LayoutSnapshot,
    pub freeze: FreezePane,
    pub conditional_formats: Vec<ConditionalFormat>,
    pub used_range: CellRange,
    pub meta: SheetMeta,
    pub data_rows: Option<(u32, u32)>,
    pub formula_cells: Vec<CellAddr>,
    pub formula_overlay: Vec<(CellAddr, FormulaCellResult)>,
    pub formula_rewrites: BTreeMap<CellAddr, String>,
}

impl StructureSnapshot {
    pub fn capture(sheet: &Sheet) -> Self {
        let mut formula_overlay: Vec<(CellAddr, FormulaCellResult)> = sheet
            .formula_overlay
            .values
            .iter()
            .map(|(addr, value)| (*addr, value.clone()))
            .collect();
        formula_overlay.sort_by_key(|(addr, _)| (addr.row, addr.col));
        Self {
            origin_rows: sheet.origin_rows.clone(),
            origin_cols: sheet.origin_cols.clone(),
            merges: sheet.merges.regions().to_vec(),
            merge_borders: sheet.merge_borders.clone(),
            filters: sheet.filters.clone(),
            filter_state: sheet.filter_state.clone(),
            layout: sheet.layout.clone(),
            freeze: sheet.freeze,
            conditional_formats: sheet.conditional_formats.clone(),
            used_range: sheet.used_range,
            meta: sheet.meta,
            data_rows: sheet.data_rows,
            formula_cells: sheet.formula_cells.clone(),
            formula_overlay,
            formula_rewrites: sheet.formula_rewrites.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StructureWarning {
    MergeAdjusted {
        before: CellRange,
        after: Option<CellRange>,
    },
    FilterAdjusted {
        before: CellRange,
        after: Option<CellRange>,
    },
    SkippedCoveredCell {
        addr: CellAddr,
    },
    FormulaLeftUnchanged {
        sheet: String,
        addr: CellAddr,
        reason: String,
    },
}

#[derive(Clone, Debug)]
pub struct FormulaChange {
    pub sheet: usize,
    pub addr: CellAddr,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StructureOutcome {
    pub removed: RemovedCells,
    pub warnings: Vec<StructureWarning>,
    pub before: StructureSnapshot,
    pub after: StructureSnapshot,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum StructureError {
    #[error("sheet is too large for structural editing: {cells} cells (limit {limit})")]
    TooLarge { cells: u64, limit: u64 },
    #[error("count must be between 1 and 1000")]
    InvalidCount { count: u32 },
    #[error("operation is outside the sheet: {axis:?} {index}+{count}")]
    OutOfBounds { axis: Axis, index: u32, count: u32 },
    #[error("editing is not available for this workbook")]
    ReadOnly,
    #[error("the sheet must be materialized before structural editing")]
    NotMaterialized,
    #[error("formula {sheet}!{addr} cannot be shifted: {message}")]
    UnsupportedFormula {
        sheet: String,
        addr: CellAddr,
        message: String,
    },
    #[error("sheet error: {0}")]
    Sheet(String),
}

impl From<crate::domain::error::LoadError> for StructureError {
    fn from(error: crate::domain::error::LoadError) -> Self {
        Self::Sheet(error.to_string())
    }
}

pub struct StructureEngine;

impl StructureEngine {
    pub fn materialize(sheet: &mut Sheet, max_cells: u64) -> Result<(), StructureError> {
        let cells = (sheet.meta.row_count as u64).saturating_mul(sheet.meta.column_count as u64);
        if cells > max_cells {
            return Err(StructureError::TooLarge {
                cells,
                limit: max_cells,
            });
        }
        if let SheetData::Lazy(lazy) = &mut sheet.data {
            let mut cells_map = HashMap::new();
            lazy.scan_all(&mut |row| {
                for (col, cell) in &row.cells {
                    cells_map.insert(CellAddr::new(row.row, *col), cell.clone());
                }
                std::ops::ControlFlow::Continue(())
            })?;
            sheet.data = SheetData::Eager(cells_map);
        }
        while sheet.origin_rows.len() < sheet.meta.row_count as usize {
            sheet.origin_rows.push(None);
        }
        while sheet.origin_cols.len() < sheet.meta.column_count as usize {
            sheet.origin_cols.push(None);
        }
        Ok(())
    }

    pub fn apply(sheet: &mut Sheet, op: StructureOp) -> Result<StructureOutcome, StructureError> {
        validate(sheet, op)?;
        let before = StructureSnapshot::capture(sheet);
        let mut warnings = Vec::new();
        let removed = match (op.axis, op.mode) {
            (Axis::Row, StructureMode::Insert) => {
                insert_rows(sheet, op.index, op.count);
                RemovedCells::default()
            }
            (Axis::Row, StructureMode::Delete) => delete_rows(sheet, op.index, op.count),
            (Axis::Column, StructureMode::Insert) => {
                insert_cols(sheet, op.index, op.count);
                RemovedCells::default()
            }
            (Axis::Column, StructureMode::Delete) => delete_cols(sheet, op.index, op.count),
        };
        let after = StructureSnapshot::capture(sheet);
        invalidate_derived(sheet, op, &mut warnings)?;
        sheet.structural_dirty = true;
        Ok(StructureOutcome {
            removed,
            warnings,
            before,
            after,
        })
    }

    /// Applies the inverse of `op` and restores removed cells. Used by undo.
    pub fn invert(sheet: &mut Sheet, op: StructureOp, removed: &RemovedCells) {
        let inverse = StructureOp {
            axis: op.axis,
            mode: match op.mode {
                StructureMode::Insert => StructureMode::Delete,
                StructureMode::Delete => StructureMode::Insert,
            },
            index: op.index,
            count: op.count,
        };
        match (op.axis, op.mode) {
            (Axis::Row, StructureMode::Insert) => {
                let _ = delete_rows(sheet, op.index, op.count);
            }
            (Axis::Row, StructureMode::Delete) => insert_rows(sheet, op.index, op.count),
            (Axis::Column, StructureMode::Insert) => {
                let _ = delete_cols(sheet, op.index, op.count);
            }
            (Axis::Column, StructureMode::Delete) => insert_cols(sheet, op.index, op.count),
        };
        restore_removed(sheet, removed);
        let _ = inverse;
        sheet.structural_dirty = true;
        let _ = sheet.rebuild_visibility();
    }
}

fn validate(sheet: &Sheet, op: StructureOp) -> Result<(), StructureError> {
    if op.count == 0 || op.count > 1000 {
        return Err(StructureError::InvalidCount { count: op.count });
    }
    if !matches!(sheet.data, SheetData::Eager(_)) {
        return Err(StructureError::NotMaterialized);
    }
    let (limit, current) = match op.axis {
        Axis::Row => (EXCEL_MAX_ROWS, sheet.meta.row_count),
        Axis::Column => (EXCEL_MAX_COLS, sheet.meta.column_count),
    };
    match op.mode {
        StructureMode::Insert => {
            if op.index > current || current.saturating_add(op.count) > limit {
                return Err(StructureError::OutOfBounds {
                    axis: op.axis,
                    index: op.index,
                    count: op.count,
                });
            }
        }
        StructureMode::Delete => {
            if op.index >= current || op.index.saturating_add(op.count) > current {
                return Err(StructureError::OutOfBounds {
                    axis: op.axis,
                    index: op.index,
                    count: op.count,
                });
            }
        }
    }
    Ok(())
}

fn insert_rows(sheet: &mut Sheet, index: u32, count: u32) {
    for _ in 0..count {
        sheet.origin_rows.insert(index as usize, None);
    }
    sheet.meta.row_count = sheet.meta.row_count.saturating_add(count);
    if index <= sheet.used_range.end.row {
        sheet.used_range.end.row = sheet.used_range.end.row.saturating_add(count);
    }
    shift_data_rows_insert(&mut sheet.data_rows, index, count);
    shift_layout_rows_insert(&mut sheet.layout, index, count);
    shift_merge_borders(
        &mut sheet.merge_borders,
        Axis::Row,
        StructureMode::Insert,
        index,
        count,
    );
    shift_merges(
        &mut sheet.merges,
        Axis::Row,
        StructureMode::Insert,
        index,
        count,
    );
    shift_filters(
        &mut sheet.filters,
        Axis::Row,
        StructureMode::Insert,
        index,
        count,
    );
    shift_conditional(
        &mut sheet.conditional_formats,
        Axis::Row,
        StructureMode::Insert,
        index,
        count,
    );
    if index < sheet.freeze.frozen_rows {
        sheet.freeze.frozen_rows = sheet.freeze.frozen_rows.saturating_add(count);
    }
    shift_cells(sheet, Axis::Row, index, count, true);
    sheet.invalidate_filter_masks();
}

fn delete_rows(sheet: &mut Sheet, index: u32, count: u32) -> RemovedCells {
    let mut removed = RemovedCells::default();
    let end = index.saturating_add(count);
    if let SheetData::Eager(cells) = &mut sheet.data {
        let addresses: Vec<CellAddr> = cells
            .keys()
            .copied()
            .filter(|addr| addr.row >= index && addr.row < end)
            .collect();
        for addr in addresses {
            if let Some(cell) = cells.remove(&addr) {
                removed.base.push((addr, cell));
            }
        }
        shift_cells_map(cells, Axis::Row, index, count, false);
    }
    let addresses: Vec<CellAddr> = sheet
        .edits
        .keys()
        .copied()
        .filter(|addr| addr.row >= index && addr.row < end)
        .collect();
    for addr in addresses {
        if let Some(cell) = sheet.edits.remove(&addr) {
            removed.edits.push((addr, cell));
        }
    }
    shift_edits(&mut sheet.edits, Axis::Row, index, count, false);
    let overlay_addresses: Vec<CellAddr> = sheet
        .formula_overlay
        .values
        .keys()
        .copied()
        .filter(|addr| addr.row >= index && addr.row < end)
        .collect();
    for addr in overlay_addresses {
        if let Some(value) = sheet.formula_overlay.values.remove(&addr) {
            removed.formula_overlay.push((addr, value));
        }
    }
    shift_formula_overlay(&mut sheet.formula_overlay, Axis::Row, index, count, false);
    sheet.origin_rows.drain(index as usize..end as usize);
    sheet.meta.row_count = sheet.meta.row_count.saturating_sub(count).max(1);
    if index <= sheet.used_range.end.row {
        sheet.used_range.end.row = sheet
            .used_range
            .end
            .row
            .saturating_sub(count)
            .min(sheet.meta.row_count.saturating_sub(1));
    }
    shift_data_rows_delete(&mut sheet.data_rows, index, count);
    shift_layout_rows_delete(&mut sheet.layout, index, count);
    shift_merge_borders(
        &mut sheet.merge_borders,
        Axis::Row,
        StructureMode::Delete,
        index,
        count,
    );
    shift_merges(
        &mut sheet.merges,
        Axis::Row,
        StructureMode::Delete,
        index,
        count,
    );
    shift_filters(
        &mut sheet.filters,
        Axis::Row,
        StructureMode::Delete,
        index,
        count,
    );
    shift_conditional(
        &mut sheet.conditional_formats,
        Axis::Row,
        StructureMode::Delete,
        index,
        count,
    );
    if index < sheet.freeze.frozen_rows {
        sheet.freeze.frozen_rows = sheet.freeze.frozen_rows.saturating_sub(count);
    }
    sheet
        .formula_cells
        .retain(|addr| !(addr.row >= index && addr.row < end));
    shift_formula_cells(&mut sheet.formula_cells, Axis::Row, index, count, false);
    shift_rewrites_after_delete(sheet, Axis::Row, index, count);
    sheet.invalidate_filter_masks();
    removed
}

fn insert_cols(sheet: &mut Sheet, index: u32, count: u32) {
    for _ in 0..count {
        sheet.origin_cols.insert(index as usize, None);
    }
    sheet.meta.column_count = sheet.meta.column_count.saturating_add(count);
    if index <= sheet.used_range.end.col {
        sheet.used_range.end.col = sheet.used_range.end.col.saturating_add(count);
    }
    shift_layout_cols_insert(&mut sheet.layout, index, count);
    shift_merge_borders(
        &mut sheet.merge_borders,
        Axis::Column,
        StructureMode::Insert,
        index,
        count,
    );
    shift_merges(
        &mut sheet.merges,
        Axis::Column,
        StructureMode::Insert,
        index,
        count,
    );
    shift_filters(
        &mut sheet.filters,
        Axis::Column,
        StructureMode::Insert,
        index,
        count,
    );
    insert_filter_state_columns(&mut sheet.filter_state, index, count);
    shift_conditional(
        &mut sheet.conditional_formats,
        Axis::Column,
        StructureMode::Insert,
        index,
        count,
    );
    if index < sheet.freeze.frozen_cols {
        sheet.freeze.frozen_cols = sheet.freeze.frozen_cols.saturating_add(count);
    }
    shift_cells(sheet, Axis::Column, index, count, true);
    sheet.recompute_wrap_heights();
    sheet.invalidate_filter_masks();
}

fn delete_cols(sheet: &mut Sheet, index: u32, count: u32) -> RemovedCells {
    let mut removed = RemovedCells::default();
    let end = index.saturating_add(count);
    if let SheetData::Eager(cells) = &mut sheet.data {
        let addresses: Vec<CellAddr> = cells
            .keys()
            .copied()
            .filter(|addr| addr.col >= index && addr.col < end)
            .collect();
        for addr in addresses {
            if let Some(cell) = cells.remove(&addr) {
                removed.base.push((addr, cell));
            }
        }
        shift_cells_map(cells, Axis::Column, index, count, false);
    }
    let addresses: Vec<CellAddr> = sheet
        .edits
        .keys()
        .copied()
        .filter(|addr| addr.col >= index && addr.col < end)
        .collect();
    for addr in addresses {
        if let Some(cell) = sheet.edits.remove(&addr) {
            removed.edits.push((addr, cell));
        }
    }
    shift_edits(&mut sheet.edits, Axis::Column, index, count, false);
    let overlay_addresses: Vec<CellAddr> = sheet
        .formula_overlay
        .values
        .keys()
        .copied()
        .filter(|addr| addr.col >= index && addr.col < end)
        .collect();
    for addr in overlay_addresses {
        if let Some(value) = sheet.formula_overlay.values.remove(&addr) {
            removed.formula_overlay.push((addr, value));
        }
    }
    shift_formula_overlay(
        &mut sheet.formula_overlay,
        Axis::Column,
        index,
        count,
        false,
    );
    sheet.origin_cols.drain(index as usize..end as usize);
    sheet.meta.column_count = sheet.meta.column_count.saturating_sub(count).max(1);
    if index <= sheet.used_range.end.col {
        sheet.used_range.end.col = sheet
            .used_range
            .end
            .col
            .saturating_sub(count)
            .min(sheet.meta.column_count.saturating_sub(1));
    }
    shift_layout_cols_delete(&mut sheet.layout, index, count);
    shift_merge_borders(
        &mut sheet.merge_borders,
        Axis::Column,
        StructureMode::Delete,
        index,
        count,
    );
    shift_merges(
        &mut sheet.merges,
        Axis::Column,
        StructureMode::Delete,
        index,
        count,
    );
    shift_filters(
        &mut sheet.filters,
        Axis::Column,
        StructureMode::Delete,
        index,
        count,
    );
    delete_filter_state_columns(&mut sheet.filter_state, index, count);
    shift_conditional(
        &mut sheet.conditional_formats,
        Axis::Column,
        StructureMode::Delete,
        index,
        count,
    );
    if index < sheet.freeze.frozen_cols {
        sheet.freeze.frozen_cols = sheet.freeze.frozen_cols.saturating_sub(count);
    }
    sheet
        .formula_cells
        .retain(|addr| !(addr.col >= index && addr.col < end));
    shift_formula_cells(&mut sheet.formula_cells, Axis::Column, index, count, false);
    shift_rewrites_after_delete(sheet, Axis::Column, index, count);
    sheet.recompute_wrap_heights();
    sheet.invalidate_filter_masks();
    removed
}

fn shift_cells(sheet: &mut Sheet, axis: Axis, index: u32, count: u32, insert: bool) {
    if let SheetData::Eager(cells) = &mut sheet.data {
        shift_cells_map(cells, axis, index, count, insert);
    }
    shift_edits(&mut sheet.edits, axis, index, count, insert);
    shift_formula_overlay(&mut sheet.formula_overlay, axis, index, count, insert);
    shift_formula_cells(&mut sheet.formula_cells, axis, index, count, insert);
    sheet.formula_rewrites = shift_rewrites(&sheet.formula_rewrites, axis, index, count, insert);
}

fn shift_cells_map(
    cells: &mut HashMap<CellAddr, Cell>,
    axis: Axis,
    index: u32,
    count: u32,
    insert: bool,
) {
    let old = std::mem::take(cells);
    for (addr, cell) in old {
        let new_addr = shift_addr(addr, axis, index, count, insert);
        cells.insert(new_addr, cell);
    }
}

fn shift_edits(
    edits: &mut HashMap<CellAddr, Cell>,
    axis: Axis,
    index: u32,
    count: u32,
    insert: bool,
) {
    let old = std::mem::take(edits);
    for (addr, cell) in old {
        edits.insert(shift_addr(addr, axis, index, count, insert), cell);
    }
}

fn shift_formula_overlay(
    overlay: &mut FormulaOverlay,
    axis: Axis,
    index: u32,
    count: u32,
    insert: bool,
) {
    let old = std::mem::take(&mut overlay.values);
    for (addr, mut value) in old {
        let new_addr = shift_addr(addr, axis, index, count, insert);
        value.addr = new_addr;
        overlay.values.insert(new_addr, value);
    }
}

fn shift_formula_cells(cells: &mut [CellAddr], axis: Axis, index: u32, count: u32, insert: bool) {
    for addr in cells.iter_mut() {
        *addr = shift_addr(*addr, axis, index, count, insert);
    }
}

fn shift_rewrites(
    rewrites: &BTreeMap<CellAddr, String>,
    axis: Axis,
    index: u32,
    count: u32,
    insert: bool,
) -> BTreeMap<CellAddr, String> {
    rewrites
        .iter()
        .map(|(addr, value)| (shift_addr(*addr, axis, index, count, insert), value.clone()))
        .collect()
}

fn shift_addr(addr: CellAddr, axis: Axis, index: u32, count: u32, insert: bool) -> CellAddr {
    let shift = |value: u32| -> u32 {
        if insert {
            if value >= index {
                value.saturating_add(count)
            } else {
                value
            }
        } else if value >= index.saturating_add(count) {
            value.saturating_sub(count)
        } else if value >= index {
            index
        } else {
            value
        }
    };
    match axis {
        Axis::Row => CellAddr::new(shift(addr.row), addr.col),
        Axis::Column => CellAddr::new(addr.row, shift(addr.col)),
    }
}

fn restore_removed(sheet: &mut Sheet, removed: &RemovedCells) {
    if let SheetData::Eager(cells) = &mut sheet.data {
        for (addr, cell) in &removed.base {
            cells.insert(*addr, cell.clone());
        }
    }
    for (addr, cell) in &removed.edits {
        sheet.edits.insert(*addr, cell.clone());
    }
    for (addr, value) in &removed.formula_overlay {
        sheet.formula_overlay.values.insert(*addr, value.clone());
    }
    sheet.formula_cells = sheet.formula_overlay.values.keys().copied().collect();
    sheet.formula_cells.sort_by_key(|addr| (addr.row, addr.col));
}

fn shift_layout_rows_insert(layout: &mut LayoutSnapshot, index: u32, count: u32) {
    let index = index as usize;
    for _ in 0..count {
        layout
            .row_heights
            .insert(index.min(layout.row_heights.len()), 1);
    }
    layout.fixed_rows = layout
        .fixed_rows
        .iter()
        .map(|row| {
            if *row >= index as u32 {
                row + count
            } else {
                *row
            }
        })
        .collect();
    layout.wrap_row_heights = std::mem::take(&mut layout.wrap_row_heights)
        .into_iter()
        .map(|(row, height)| {
            let row = if row >= index as u32 {
                row.saturating_add(count)
            } else {
                row
            };
            (row, height)
        })
        .collect();
}

fn shift_layout_rows_delete(layout: &mut LayoutSnapshot, index: u32, count: u32) {
    let start = index as usize;
    let end = (index + count) as usize;
    if start < layout.row_heights.len() {
        layout
            .row_heights
            .drain(start..end.min(layout.row_heights.len()));
    }
    layout.fixed_rows = layout
        .fixed_rows
        .iter()
        .filter_map(|row| {
            if *row >= index + count {
                Some(row - count)
            } else if *row >= index {
                None
            } else {
                Some(*row)
            }
        })
        .collect();
    let wrap_start = index;
    let wrap_end = index.saturating_add(count);
    layout.wrap_row_heights = std::mem::take(&mut layout.wrap_row_heights)
        .into_iter()
        .filter_map(|(row, height)| {
            if row >= wrap_start && row < wrap_end {
                None
            } else if row >= wrap_end {
                Some((row.saturating_sub(count), height))
            } else {
                Some((row, height))
            }
        })
        .collect();
}

fn shift_layout_cols_insert(layout: &mut LayoutSnapshot, index: u32, count: u32) {
    let index = index as usize;
    for _ in 0..count {
        layout
            .column_widths
            .insert(index.min(layout.column_widths.len()), 8);
    }
    layout.fixed_columns = layout
        .fixed_columns
        .iter()
        .map(|col| {
            if *col >= index as u32 {
                col + count
            } else {
                *col
            }
        })
        .collect();
}

fn shift_layout_cols_delete(layout: &mut LayoutSnapshot, index: u32, count: u32) {
    let start = index as usize;
    let end = (index + count) as usize;
    if start < layout.column_widths.len() {
        layout
            .column_widths
            .drain(start..end.min(layout.column_widths.len()));
    }
    layout.fixed_columns = layout
        .fixed_columns
        .iter()
        .filter_map(|col| {
            if *col >= index + count {
                Some(col - count)
            } else if *col >= index {
                None
            } else {
                Some(*col)
            }
        })
        .collect();
}

fn shift_merge_borders(
    borders: &mut HashMap<CellAddr, CellBorders>,
    axis: Axis,
    mode: StructureMode,
    index: u32,
    count: u32,
) {
    let end = index.saturating_add(count);
    let old = std::mem::take(borders);
    for (addr, border) in old {
        let deleted = mode == StructureMode::Delete
            && match axis {
                Axis::Row => addr.row >= index && addr.row < end,
                Axis::Column => addr.col >= index && addr.col < end,
            };
        if deleted {
            continue;
        }
        let new_addr = shift_addr(addr, axis, index, count, mode == StructureMode::Insert);
        borders.insert(new_addr, border);
    }
}

fn shift_rewrites_after_delete(sheet: &mut Sheet, axis: Axis, index: u32, count: u32) {
    let end = index.saturating_add(count);
    let old = std::mem::take(&mut sheet.formula_rewrites);
    sheet.formula_rewrites = old
        .into_iter()
        .filter(|(addr, _)| match axis {
            Axis::Row => !(addr.row >= index && addr.row < end),
            Axis::Column => !(addr.col >= index && addr.col < end),
        })
        .map(|(addr, value)| (shift_addr(addr, axis, index, count, false), value))
        .collect();
}

fn shift_merges(merges: &mut MergeIndex, axis: Axis, mode: StructureMode, index: u32, count: u32) {
    let regions: Vec<MergeRegion> = merges
        .regions()
        .iter()
        .filter_map(|region| {
            let range = transform_range_op(region.range, axis, mode, index, count)?;
            Some(MergeRegion {
                anchor: range.start,
                range,
            })
        })
        .collect();
    *merges = MergeIndex::new(regions);
}

fn shift_filters(
    filters: &mut Vec<FilterDefinition>,
    axis: Axis,
    mode: StructureMode,
    index: u32,
    count: u32,
) {
    filters.retain_mut(|filter| {
        let Some(range) = transform_range_op(filter.range, axis, mode, index, count) else {
            return false;
        };
        let Some(data_range) = transform_range_op(filter.data_range, axis, mode, index, count)
        else {
            return false;
        };
        filter.range = range;
        filter.data_range = data_range;
        filter.header_row = match (axis, mode) {
            (Axis::Row, StructureMode::Insert) => {
                if filter.header_row >= index {
                    filter.header_row + count
                } else {
                    filter.header_row
                }
            }
            (Axis::Row, StructureMode::Delete) => {
                if filter.header_row >= index + count {
                    filter.header_row - count
                } else if filter.header_row >= index {
                    0
                } else {
                    filter.header_row
                }
            }
            _ => filter.header_row,
        };
        if axis == Axis::Column {
            for column in &mut filter.columns {
                column.col_id = shift_index(column.col_id, index, count, mode);
            }
            filter.columns.retain(|column| column.col_id != u32::MAX);
        }
        true
    });
}

fn shift_conditional(
    formats: &mut Vec<ConditionalFormat>,
    axis: Axis,
    mode: StructureMode,
    index: u32,
    count: u32,
) {
    formats.retain_mut(|format| {
        let Some(range) = transform_range_op(format.range, axis, mode, index, count) else {
            return false;
        };
        format.range = range;
        true
    });
}

fn shift_index(value: u32, index: u32, count: u32, mode: StructureMode) -> u32 {
    match mode {
        StructureMode::Insert => {
            if value >= index {
                value.saturating_add(count)
            } else {
                value
            }
        }
        StructureMode::Delete => {
            if value >= index + count {
                value - count
            } else if value >= index {
                u32::MAX
            } else {
                value
            }
        }
    }
}

fn insert_filter_state_columns(state: &mut FilterState, index: u32, count: u32) {
    let index = index as usize;
    for _ in 0..count {
        state
            .column_enabled
            .insert(index.min(state.column_enabled.len()), true);
    }
}

fn delete_filter_state_columns(state: &mut FilterState, index: u32, count: u32) {
    let start = index as usize;
    let end = (index + count) as usize;
    if start < state.column_enabled.len() {
        state
            .column_enabled
            .drain(start..end.min(state.column_enabled.len()));
    }
}

fn shift_data_rows_insert(rows: &mut Option<(u32, u32)>, index: u32, count: u32) {
    if let Some((first, last)) = rows {
        if index <= *first {
            *first += count;
        }
        if index <= *last {
            *last += count;
        }
    }
}

fn shift_data_rows_delete(rows: &mut Option<(u32, u32)>, index: u32, count: u32) {
    if let Some((first, last)) = rows {
        let new_first = if *first >= index + count {
            *first - count
        } else if *first >= index {
            index
        } else {
            *first
        };
        let new_last = if *last >= index + count {
            *last - count
        } else if *last >= index {
            index.saturating_sub(1)
        } else {
            *last
        };
        if new_first > new_last {
            *rows = None;
        } else {
            *rows = Some((new_first, new_last));
        }
    }
}

pub fn transform_range_op(
    range: CellRange,
    axis: Axis,
    mode: StructureMode,
    index: u32,
    count: u32,
) -> Option<CellRange> {
    match axis {
        Axis::Row => {
            let (start, end) = transform_axis(range.start.row, range.end.row, mode, index, count)?;
            Some(CellRange::new(
                CellAddr::new(start, range.start.col),
                CellAddr::new(end, range.end.col),
            ))
        }
        Axis::Column => {
            let (start, end) = transform_axis(range.start.col, range.end.col, mode, index, count)?;
            Some(CellRange::new(
                CellAddr::new(range.start.row, start),
                CellAddr::new(range.end.row, end),
            ))
        }
    }
}

pub fn transform_cell_addr(addr: CellAddr, op: StructureOp) -> Option<CellAddr> {
    match op.axis {
        Axis::Row => transform_axis_range(addr.row, addr.row, op.mode, op.index, op.count)
            .map(|(row, _)| CellAddr::new(row, addr.col)),
        Axis::Column => transform_axis_range(addr.col, addr.col, op.mode, op.index, op.count)
            .map(|(col, _)| CellAddr::new(addr.row, col)),
    }
}

pub fn transform_axis_range(
    start: u32,
    end: u32,
    mode: StructureMode,
    index: u32,
    count: u32,
) -> Option<(u32, u32)> {
    transform_axis(start, end, mode, index, count)
}

fn transform_axis(
    start: u32,
    end: u32,
    mode: StructureMode,
    index: u32,
    count: u32,
) -> Option<(u32, u32)> {
    match mode {
        StructureMode::Insert => {
            if end < index {
                Some((start, end))
            } else if start >= index {
                Some((start + count, end + count))
            } else {
                Some((start, end + count))
            }
        }
        StructureMode::Delete => {
            if end < index {
                Some((start, end))
            } else if start >= index + count {
                Some((start - count, end - count))
            } else {
                let new_start = if start < index { start } else { index };
                let new_end = if end >= index + count {
                    end - count
                } else {
                    index.saturating_sub(1)
                };
                if new_start > new_end {
                    None
                } else {
                    Some((new_start, new_end))
                }
            }
        }
    }
}

fn invalidate_derived(
    sheet: &mut Sheet,
    _op: StructureOp,
    _warnings: &mut Vec<StructureWarning>,
) -> Result<(), StructureError> {
    sheet.invalidate_filter_masks();
    sheet.rebuild_visibility()?;
    Ok(())
}

pub fn ensure_origin_length(sheet: &mut Sheet) {
    while sheet.origin_rows.len() < sheet.meta.row_count as usize {
        sheet.origin_rows.push(None);
    }
    while sheet.origin_cols.len() < sheet.meta.column_count as usize {
        sheet.origin_cols.push(None);
    }
}

pub fn origin_from_sheet(sheet: &Sheet) -> OriginMap {
    OriginMap::from_sheet(sheet)
}

pub fn row_origins(sheet: &Sheet) -> &[Option<u32>] {
    &sheet.origin_rows
}

pub fn col_origins(sheet: &Sheet) -> &[Option<u32>] {
    &sheet.origin_cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::filter::VisibilityMap;
    use crate::domain::formula::FormulaOverlay;
    use crate::domain::layout::LayoutLimits;
    use crate::domain::sheet::{Sheet, SheetData};
    use std::collections::HashMap;

    fn sheet(rows: u32, cols: u32) -> Sheet {
        let (origin_rows, origin_cols) = Sheet::identity_origins(rows, cols);
        Sheet {
            name: "Sheet1".to_string(),
            meta: SheetMeta {
                row_count: rows,
                column_count: cols,
            },
            used_range: CellRange::new(CellAddr::new(0, 0), CellAddr::new(rows - 1, cols - 1)),
            data_rows: Some((0, rows - 1)),
            data: SheetData::Eager(HashMap::new()),
            edits: HashMap::new(),
            origin_rows,
            origin_cols,
            structural_dirty: false,
            formula_rewrites: BTreeMap::new(),
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
            visibility: VisibilityMap::all(rows),
            formula_overlay: FormulaOverlay::default(),
            formula_cells: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn inserts_and_deletes_rows_preserving_origin() {
        let mut sheet = sheet(3, 3);
        if let SheetData::Eager(cells) = &mut sheet.data {
            cells.insert(
                CellAddr::new(1, 1),
                Cell::new(
                    crate::domain::cell::CellValue::Number(1.0),
                    Default::default(),
                ),
            );
        }
        StructureEngine::apply(&mut sheet, StructureOp::insert(Axis::Row, 1, 2)).unwrap();
        assert_eq!(sheet.meta.row_count, 5);
        assert_eq!(
            sheet.origin_rows,
            vec![Some(0), None, None, Some(1), Some(2)]
        );
        assert!(
            matches!(&sheet.data, SheetData::Eager(cells) if cells.contains_key(&CellAddr::new(3, 1)))
        );
        StructureEngine::apply(&mut sheet, StructureOp::delete(Axis::Row, 1, 2)).unwrap();
        assert_eq!(sheet.meta.row_count, 3);
        assert_eq!(sheet.origin_rows, vec![Some(0), Some(1), Some(2)]);
        assert!(
            matches!(&sheet.data, SheetData::Eager(cells) if cells.contains_key(&CellAddr::new(1, 1)))
        );
    }

    #[test]
    fn transforms_ranges_for_insert_and_delete() {
        let range = CellRange::new(CellAddr::new(1, 1), CellAddr::new(3, 3));
        assert_eq!(
            transform_range_op(range, Axis::Row, StructureMode::Insert, 2, 2),
            Some(CellRange::new(CellAddr::new(1, 1), CellAddr::new(5, 3)))
        );
        assert_eq!(
            transform_range_op(range, Axis::Row, StructureMode::Delete, 2, 2),
            Some(CellRange::new(CellAddr::new(1, 1), CellAddr::new(1, 3)))
        );
        assert_eq!(
            transform_range_op(range, Axis::Row, StructureMode::Delete, 1, 3),
            None
        );
    }

    #[test]
    fn origin_map_maps_original_ranges_to_current() {
        let map = OriginMap {
            origin_rows: vec![Some(0), None, Some(1), Some(2)],
            origin_cols: vec![Some(0), Some(1)],
        };
        assert_eq!(
            map.transform_range(CellRange::new(CellAddr::new(1, 0), CellAddr::new(2, 1))),
            Some(CellRange::new(CellAddr::new(2, 0), CellAddr::new(3, 1)))
        );
    }
}
