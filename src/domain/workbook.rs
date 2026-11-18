use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::addr::CellAddr;
use crate::domain::cell::{Cell, CellRead, CellValue, EffectiveCell};
use crate::domain::copy::{CopyLimits, CopyPayload, CopySession};
use crate::domain::error::CopyError;
use crate::domain::error::LoadError;
use crate::domain::formula::{FormulaOverlay, FormulaResults, FormulaStatus};
use crate::domain::row_store::RowRange;
use crate::domain::selection::Selection;
use crate::domain::sheet::{Sheet, SheetData};
use crate::domain::task::Generation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    Xlsx,
    Csv,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileFingerprint {
    pub len: u64,
    pub modified_ms: u128,
    pub present: bool,
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub format: SourceFormat,
    pub fingerprint: FileFingerprint,
    pub encoding: Option<String>,
    pub lazy: bool,
}

pub struct WorkbookModel {
    pub source: SourceInfo,
    pub sheets: Vec<Sheet>,
    pub active_sheet: usize,
    pub generation: Generation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormulaApplySummary {
    pub changed: bool,
    pub layout_changed: bool,
    pub filter_masks_dirty: bool,
}

impl WorkbookModel {
    pub fn new(source: SourceInfo, sheets: Vec<Sheet>) -> Result<Self, LoadError> {
        if sheets.is_empty() {
            return Err(LoadError::EmptyWorkbook { path: source.path });
        }
        Ok(Self {
            source,
            sheets,
            active_sheet: 0,
            generation: Generation(0),
        })
    }

    pub fn sheet(&self, index: usize) -> Option<&Sheet> {
        self.sheets.get(index)
    }

    pub fn active(&self) -> Option<&Sheet> {
        self.sheet(self.active_sheet)
    }

    pub fn active_mut(&mut self) -> Option<&mut Sheet> {
        self.sheets.get_mut(self.active_sheet)
    }

    pub fn read_cell(&self, sheet: usize, addr: CellAddr) -> CellRead {
        self.sheets
            .get(sheet)
            .map(|sheet| sheet.read_cell(addr))
            .unwrap_or(CellRead::Missing)
    }

    pub fn effective_cell(&self, sheet: usize, addr: CellAddr) -> Option<EffectiveCell> {
        self.sheets
            .get(sheet)
            .and_then(|sheet| sheet.effective_cell(addr))
    }

    pub fn formula_overlay(&self, sheet: usize) -> Arc<FormulaOverlay> {
        self.sheets
            .get(sheet)
            .map(|sheet| Arc::new(sheet.formula_overlay.clone()))
            .unwrap_or_default()
    }

    pub fn ensure_rows(&mut self, sheet: usize, range: RowRange) -> Result<(), LoadError> {
        self.sheets
            .get_mut(sheet)
            .ok_or_else(|| LoadError::SheetIndex {
                sheet: sheet.to_string(),
                message: "sheet index out of bounds".to_string(),
            })?
            .ensure_rows(range)
    }

    pub fn copy_selection(
        &mut self,
        sheet_index: usize,
        selection: &Selection,
        limits: &CopyLimits,
    ) -> Result<CopyPayload, CopyError> {
        let Some(sheet) = self.sheets.get_mut(sheet_index) else {
            return Err(CopyError::Source {
                sheet: sheet_index.to_string(),
                message: "sheet index out of bounds".to_string(),
            });
        };
        let rows = sheet
            .meta
            .row_count
            .max(sheet.used_range.end.row.saturating_add(1));
        let cols = sheet.used_range.end.col + 1;
        let range = selection.range(rows, cols);
        let edits = sheet.edits.clone();
        let mut session =
            CopySession::start(sheet, selection, &sheet.merges, &sheet.visibility, limits)?;
        let overlay = sheet.formula_overlay.clone();
        match &mut sheet.data {
            SheetData::Eager(cells) => {
                for row in range.rows() {
                    if !sheet.visibility.is_visible(row) {
                        continue;
                    }
                    let mut values = Vec::new();
                    for col in range.cols() {
                        let addr = CellAddr::new(row, col);
                        values.push(
                            edits
                                .get(&addr)
                                .or_else(|| cells.get(&addr))
                                .map(|cell| effective_with_overlay(&overlay, addr, cell))
                                .unwrap_or_else(empty_effective),
                        );
                    }
                    session.feed_row(row, &values)?;
                }
            }
            SheetData::Lazy(lazy) => {
                let visibility = sheet.visibility.clone();
                let mut copy_error = None;
                let _ = lazy.scan_all(&mut |row| {
                    if row.row < range.start.row
                        || row.row > range.end.row
                        || !visibility.is_visible(row.row)
                    {
                        return std::ops::ControlFlow::Continue(());
                    }
                    let mut values = Vec::new();
                    for col in range.cols() {
                        let addr = CellAddr::new(row.row, col);
                        values.push(
                            edits
                                .get(&addr)
                                .or_else(|| row.cell(col))
                                .map(|cell| effective_with_overlay(&overlay, addr, cell))
                                .unwrap_or_else(empty_effective),
                        );
                    }
                    if let Err(error) = session.feed_row(row.row, &values) {
                        copy_error = Some(error);
                        return std::ops::ControlFlow::Break(());
                    }
                    std::ops::ControlFlow::Continue(())
                });
                if let Some(error) = copy_error {
                    return Err(error);
                }
            }
        }
        session.finish()
    }

    pub fn apply_formula_results(&mut self, results: FormulaResults) -> FormulaApplySummary {
        let mut summary = FormulaApplySummary::default();
        for update in results.updates {
            let Some(sheet) = self
                .sheets
                .iter_mut()
                .find(|sheet| sheet.name == update.sheet)
            else {
                continue;
            };
            let number_format = sheet
                .cell(update.addr)
                .and_then(|cell| cell.number_format.clone());
            let display = update
                .value
                .display_text_with_format(number_format.as_deref());
            sheet
                .formula_overlay
                .values
                .insert(update.addr, update.clone());
            if let SheetData::Eager(cells) = &mut sheet.data
                && let Some(cell) = cells.get_mut(&update.addr)
                && let Some(formula) = cell.formula.as_mut()
            {
                formula.evaluated = Some(update.value.clone());
                formula.status = update.status.clone();
                cell.display = display.clone();
            }
            if let Some(cell) = sheet.edits.get_mut(&update.addr)
                && let Some(formula) = cell.formula.as_mut()
            {
                formula.evaluated = Some(update.value.clone());
                formula.status = update.status.clone();
                cell.display = display.clone();
            }
            let _ = sheet.ensure_rows(RowRange::new(
                update.addr.row,
                update.addr.row.saturating_add(1),
            ));
            let wrap_text = sheet
                .cell(update.addr)
                .map(|cell| cell.wrap_text)
                .unwrap_or(false);
            let merge = sheet
                .merges
                .region_at(update.addr)
                .map(|region| region.range);
            let invalidation = sheet.layout.grow_for_cell(
                update.addr,
                &display,
                wrap_text,
                merge,
                &sheet.layout_limits,
            );
            if !invalidation.rows.is_empty() || !invalidation.columns.is_empty() {
                summary.layout_changed = true;
            }
            if !sheet.filters.is_empty() {
                sheet.invalidate_filter_masks();
                summary.filter_masks_dirty = true;
            }
            summary.changed = true;
        }
        summary
    }

    pub fn set_formula_skipped(&mut self, message: &str) {
        for sheet in &mut self.sheets {
            for cell in match &mut sheet.data {
                SheetData::Eager(cells) => cells.values_mut(),
                SheetData::Lazy(_) => continue,
            } {
                if let Some(formula) = cell.formula.as_mut() {
                    formula.status = FormulaStatus::CachedOnly;
                    let _ = message;
                }
            }
        }
    }
}

fn effective_with_overlay(overlay: &FormulaOverlay, addr: CellAddr, cell: &Cell) -> EffectiveCell {
    if let Some(result) = overlay.get(addr) {
        let mut formula = cell.formula.clone();
        if let Some(info) = formula.as_mut() {
            info.evaluated = Some(result.value.clone());
            info.status = result.status.clone();
        }
        return EffectiveCell {
            value: result.value.clone(),
            display: result
                .value
                .display_text_with_format(cell.number_format.as_deref()),
            style: cell.style,
            formula,
            formula_status: result.status.clone(),
        };
    }
    let status = cell
        .formula
        .as_ref()
        .map(|formula| formula.status.clone())
        .unwrap_or(FormulaStatus::NotRequested);
    cell.effective(status)
}

fn empty_effective() -> EffectiveCell {
    EffectiveCell {
        value: CellValue::Empty,
        display: String::new(),
        style: Default::default(),
        formula: None,
        formula_status: FormulaStatus::NotRequested,
    }
}
