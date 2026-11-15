use std::collections::HashMap;

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, CellRead, EffectiveCell};
use crate::domain::conditional::{self, ConditionalFormat};
use crate::domain::error::LoadError;
use crate::domain::filter::{
    FilterDefinition, FilterEngine, FilterMaskBuilder, FilterMasks, FilterState, FilterValue,
    VisibilityMap, filter_column_indices,
};
use crate::domain::formula::{FormulaOverlay, FormulaStatus};
use crate::domain::freeze::FreezePane;
use crate::domain::layout::{LayoutLimits, LayoutSnapshot};
use crate::domain::merge::MergeIndex;
use crate::domain::row_store::{LazySheet, RowRange};
use crate::domain::style::CellBorders;

#[derive(Clone, Copy, Debug, Default)]
pub struct SheetMeta {
    pub row_count: u32,
    pub column_count: u32,
}

pub enum SheetData {
    Eager(HashMap<CellAddr, Cell>),
    Lazy(LazySheet),
}

/// Accumulates the first and last row that contain actual cell content.
#[derive(Default)]
pub struct DataRowsBuilder {
    first: Option<u32>,
    last: Option<u32>,
}

impl DataRowsBuilder {
    pub fn observe_row(&mut self, row: u32, has_content: bool) {
        if has_content {
            self.first = Some(self.first.map_or(row, |first| first.min(row)));
            self.last = Some(self.last.map_or(row, |last| last.max(row)));
        }
    }

    pub fn finish(self) -> Option<(u32, u32)> {
        self.first.zip(self.last)
    }
}

pub struct Sheet {
    pub name: String,
    pub meta: SheetMeta,
    pub used_range: CellRange,
    /// First and last rows that contain actual content, computed at load time.
    /// `None` when the sheet has no data (formatting-only cells are ignored).
    pub data_rows: Option<(u32, u32)>,
    pub data: SheetData,
    pub merges: MergeIndex,
    pub merge_borders: HashMap<CellAddr, CellBorders>,
    pub layout: LayoutSnapshot,
    pub layout_limits: LayoutLimits,
    pub freeze: FreezePane,
    pub conditional_formats: Vec<ConditionalFormat>,
    pub filters: Vec<FilterDefinition>,
    pub filter_masks: FilterMasks,
    pub filter_masks_dirty: bool,
    pub filter_state: FilterState,
    pub visibility: VisibilityMap,
    pub formula_overlay: FormulaOverlay,
    pub formula_cells: Vec<CellAddr>,
    pub warnings: Vec<String>,
}

impl Sheet {
    pub fn data_start_row(&self) -> u32 {
        self.data_rows.map(|(first, _)| first).unwrap_or(0)
    }

    pub fn data_end_row(&self) -> u32 {
        self.data_rows
            .map(|(_, last)| last)
            .unwrap_or(self.used_range.end.row)
    }

    /// Returns the first column in `row` whose displayed value is not blank.
    /// Whitespace-only cells are skipped; an empty row falls back to column 0.
    pub fn first_non_blank_col(&self, row: u32) -> u32 {
        (0..=self.used_range.end.col)
            .find(|col| match self.read_cell(CellAddr::new(row, *col)) {
                CellRead::Loaded(cell) => !cell.display.trim().is_empty(),
                CellRead::Pending | CellRead::Missing => false,
            })
            .unwrap_or(0)
    }

    pub fn cell(&self, addr: CellAddr) -> Option<&Cell> {
        self.resolve(addr)?;
        match &self.data {
            SheetData::Eager(cells) => cells.get(&addr),
            SheetData::Lazy(lazy) => lazy.cell(addr),
        }
    }

    pub fn resolve(&self, addr: CellAddr) -> Option<CellAddr> {
        if !self.used_range.contains(addr) {
            return None;
        }
        Some(self.merges.anchor_for(addr))
    }

    pub fn read_cell(&self, addr: CellAddr) -> CellRead {
        let Some(anchor) = self.resolve(addr) else {
            return CellRead::Missing;
        };
        match &self.data {
            SheetData::Eager(cells) => cells
                .get(&anchor)
                .map(|cell| CellRead::Loaded(self.effective_from_cell(anchor, cell)))
                .unwrap_or_else(|| {
                    CellRead::Loaded(EffectiveCell {
                        value: crate::domain::cell::CellValue::Empty,
                        display: String::new(),
                        style: Default::default(),
                        formula: None,
                        formula_status: FormulaStatus::NotRequested,
                    })
                }),
            SheetData::Lazy(lazy) => {
                if !lazy.row_loaded(anchor.row) {
                    CellRead::Pending
                } else {
                    lazy.cell(anchor)
                        .map(|cell| CellRead::Loaded(self.effective_from_cell(anchor, cell)))
                        .unwrap_or_else(|| {
                            CellRead::Loaded(EffectiveCell {
                                value: crate::domain::cell::CellValue::Empty,
                                display: String::new(),
                                style: Default::default(),
                                formula: None,
                                formula_status: FormulaStatus::NotRequested,
                            })
                        })
                }
            }
        }
    }

    pub fn effective_cell(&self, addr: CellAddr) -> Option<EffectiveCell> {
        match self.read_cell(addr) {
            CellRead::Loaded(cell) => Some(cell),
            CellRead::Pending | CellRead::Missing => None,
        }
    }

    pub fn effective_from_cell(&self, addr: CellAddr, cell: &Cell) -> EffectiveCell {
        let mut effective = if let Some(result) = self.formula_overlay.get(addr) {
            let mut formula = cell.formula.clone();
            if let Some(info) = formula.as_mut() {
                info.evaluated = Some(result.value.clone());
                info.status = result.status.clone();
            }
            EffectiveCell {
                value: result.value.clone(),
                display: result
                    .value
                    .display_text_with_format(cell.number_format.as_deref()),
                style: cell.style,
                formula,
                formula_status: result.status.clone(),
            }
        } else {
            let status = cell
                .formula
                .as_ref()
                .map(|formula| formula.status.clone())
                .unwrap_or(FormulaStatus::NotRequested);
            cell.effective(status)
        };
        if let Some(style) =
            conditional::style_for(&self.conditional_formats, addr, &effective.value)
        {
            effective.style.apply_conditional(style);
        }
        effective
    }

    pub fn invalidate_filter_masks(&mut self) {
        self.filter_masks_dirty = !self.filters.is_empty();
    }

    pub fn rebuild_visibility(&mut self) -> Result<(), LoadError> {
        if self.filters.is_empty() {
            self.visibility = VisibilityMap::all(self.meta.row_count);
            return Ok(());
        }
        if self.filter_masks_dirty || self.filter_masks.row_count != self.meta.row_count {
            self.refresh_filter_masks()?;
        }
        let mut visibility = FilterEngine::combine(
            &self.filters,
            &self.filter_masks,
            &self.filter_state,
            self.meta.row_count,
        );
        visibility.force_visible(0, self.freeze.active_rows().min(self.meta.row_count));
        self.visibility = visibility;
        Ok(())
    }

    pub fn refresh_filter_masks(&mut self) -> Result<(), LoadError> {
        if self.filters.is_empty() {
            self.filter_masks = FilterMasks::default();
            self.filter_masks_dirty = false;
            return Ok(());
        }
        let filters = self.filters.clone();
        let row_count = self.meta.row_count;
        let columns = filter_column_indices(&filters);
        let overlay = self.formula_overlay.clone();
        let mut builder = FilterMaskBuilder::new(&filters, row_count);
        match &mut self.data {
            SheetData::Eager(cells) => {
                for row in 0..row_count {
                    let values = columns
                        .iter()
                        .map(|col| {
                            let addr = CellAddr::new(row, *col);
                            cells
                                .get(&addr)
                                .map(|cell| FilterValue::from_cell(addr, cell, &overlay))
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>();
                    builder.observe_row(row, &values);
                }
            }
            SheetData::Lazy(lazy) => {
                lazy.scan_all(&mut |row| {
                    let values = columns
                        .iter()
                        .map(|col| {
                            let addr = CellAddr::new(row.row, *col);
                            row.cell(*col)
                                .map(|cell| FilterValue::from_cell(addr, cell, &overlay))
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>();
                    builder.observe_row(row.row, &values);
                    std::ops::ControlFlow::Continue(())
                })?;
            }
        }
        self.filter_masks = builder.finish();
        self.filter_masks_dirty = false;
        Ok(())
    }

    pub fn ensure_rows(&mut self, range: RowRange) -> Result<(), LoadError> {
        if let SheetData::Lazy(lazy) = &mut self.data {
            lazy.ensure_rows(range)?;
        }
        Ok(())
    }

    pub fn row_loaded(&self, row: u32) -> bool {
        match &self.data {
            SheetData::Eager(_) => true,
            SheetData::Lazy(lazy) => lazy.row_loaded(row),
        }
    }

    pub fn all_cells(&self) -> Vec<(CellAddr, &Cell)> {
        match &self.data {
            SheetData::Eager(cells) => cells.iter().map(|(addr, cell)| (*addr, cell)).collect(),
            SheetData::Lazy(_) => Vec::new(),
        }
    }
}
