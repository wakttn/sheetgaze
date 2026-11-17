use std::collections::{BTreeMap, BTreeSet};

use textwrap::Options;
use unicode_width::UnicodeWidthStr;

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, CellValue, EffectiveCell};
use crate::domain::filter::VisibilityMap;
use crate::domain::formula::FormulaStatus;
use crate::domain::freeze::EffectiveFreeze;
use crate::domain::merge::MergeIndex;
use crate::domain::search::SearchIndex;
use crate::domain::selection::Selection;
use crate::domain::sheet::{Sheet, SheetData};
use crate::domain::style::CellStyle;

#[derive(Clone, Copy, Debug)]
pub struct LayoutLimits {
    pub max_col_width: u16,
    pub max_row_lines: u16,
    pub min_col_width: u16,
    pub default_col_width: u16,
}

impl Default for LayoutLimits {
    fn default() -> Self {
        Self {
            max_col_width: 65,
            max_row_lines: 10,
            min_col_width: 3,
            default_col_width: 8,
        }
    }
}

/// Reference row height in points used to map Excel row heights to terminal
/// lines. Excel's default row height is normally 15 points.
pub const EXCEL_REFERENCE_ROW_HEIGHT_POINTS: f32 = 15.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutMode {
    #[default]
    Excel,
    Content,
}

/// Excel layout values read from the worksheet XML. Column widths are in
/// characters and row heights are in points; conversion to terminal cells is
/// performed by the layout engine.
#[derive(Clone, Debug, Default)]
pub struct ExcelLayout {
    /// 0-based column index to Excel character width.
    pub column_widths: BTreeMap<u32, f32>,
    /// 0-based row index to Excel row height in points.
    pub row_heights: BTreeMap<u32, f32>,
    pub default_column_width: Option<f32>,
    pub default_row_height: Option<f32>,
}

impl ExcelLayout {
    pub fn is_empty(&self) -> bool {
        self.column_widths.is_empty()
            && self.row_heights.is_empty()
            && self.default_column_width.is_none()
            && self.default_row_height.is_none()
    }
}

pub fn excel_column_width(width: f32, limits: &LayoutLimits) -> u16 {
    if !width.is_finite() || width <= 0.0 {
        return limits.min_col_width;
    }
    let columns = width.floor().max(1.0).min(limits.max_col_width as f32) as u16;
    columns.max(limits.min_col_width)
}

pub fn excel_row_height(height: f32, limits: &LayoutLimits) -> u8 {
    if !height.is_finite() || height <= 0.0 {
        return 1;
    }
    let lines = (height / EXCEL_REFERENCE_ROW_HEIGHT_POINTS)
        .round()
        .max(1.0)
        .min(limits.max_row_lines as f32);
    lines as u8
}

#[derive(Clone, Debug, Default)]
pub struct LayoutSnapshot {
    pub column_widths: Vec<u16>,
    pub row_heights: Vec<u8>,
    /// Columns whose size comes from an explicit Excel `customWidth`.
    pub fixed_columns: BTreeSet<u32>,
    /// Rows whose size comes from an explicit Excel `customHeight`.
    pub fixed_rows: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutInvalidation {
    pub rows: BTreeSet<u32>,
    pub columns: BTreeSet<u32>,
}

#[derive(Clone, Debug)]
struct MergeDemand {
    range: CellRange,
    display: String,
    wrap_text: bool,
}

#[derive(Clone, Debug)]
struct WrapDemand {
    row: u32,
    col: u32,
    display: String,
}

pub struct LayoutStatsBuilder {
    limits: LayoutLimits,
    column_demand: Vec<u16>,
    row_demand: Vec<u8>,
    merge_demands: Vec<MergeDemand>,
    wrap_demands: Vec<WrapDemand>,
}

impl LayoutStatsBuilder {
    pub fn new(limits: &LayoutLimits) -> Self {
        Self {
            limits: *limits,
            column_demand: Vec::new(),
            row_demand: Vec::new(),
            merge_demands: Vec::new(),
            wrap_demands: Vec::new(),
        }
    }

    pub fn observe(&mut self, row: u32, col: u32, display: &str, wrap_text: bool) {
        let (width, lines) = text_metrics(display, self.limits.max_col_width);
        if self.column_demand.len() <= col as usize {
            self.column_demand.resize(col as usize + 1, 0);
        }
        if self.row_demand.len() <= row as usize {
            self.row_demand.resize(row as usize + 1, 1);
        }
        self.column_demand[col as usize] = self.column_demand[col as usize].max(width);
        self.row_demand[row as usize] =
            self.row_demand[row as usize].max(lines.min(self.limits.max_row_lines as u8));
        if wrap_text && !display.is_empty() {
            self.wrap_demands.push(WrapDemand {
                row,
                col,
                display: display.to_string(),
            });
        }
    }

    pub fn observe_merge(&mut self, range: CellRange, display: &str, wrap_text: bool) {
        self.merge_demands.push(MergeDemand {
            range,
            display: display.to_string(),
            wrap_text,
        });
    }

    pub fn finish(
        self,
        merges: &MergeIndex,
        excel: Option<&ExcelLayout>,
        used: CellRange,
    ) -> LayoutSnapshot {
        let limits = self.limits;
        let default_column = excel
            .and_then(|layout| layout.default_column_width)
            .map(|width| excel_column_width(width, &limits))
            .unwrap_or(limits.default_col_width)
            .max(limits.default_col_width);
        let default_row = excel
            .and_then(|layout| layout.default_row_height)
            .map(|height| excel_row_height(height, &limits))
            .unwrap_or(1);

        let mut widths: Vec<u16> = self
            .column_demand
            .iter()
            .map(|demand| {
                if *demand == 0 {
                    limits.default_col_width
                } else {
                    (*demand)
                        .max(limits.min_col_width)
                        .min(limits.max_col_width)
                }
            })
            .collect();
        widths.resize(
            (used.end.col as usize + 1).max(widths.len()),
            default_column,
        );

        let mut fixed_columns = BTreeSet::new();
        if let Some(excel) = excel {
            for (&col, &width) in &excel.column_widths {
                let index = col as usize;
                if widths.len() <= index {
                    widths.resize(index + 1, default_column);
                }
                widths[index] = excel_column_width(width, &limits);
                fixed_columns.insert(col);
            }
            if let Some(default) = excel.default_column_width {
                let default = excel_column_width(default, &limits);
                for (col, width) in widths.iter_mut().enumerate() {
                    if !fixed_columns.contains(&(col as u32)) {
                        *width = (*width).max(default);
                    }
                }
            }
        }

        let mut heights: Vec<u8> = self
            .row_demand
            .iter()
            .map(|demand| (*demand).max(1).min(limits.max_row_lines as u8))
            .collect();
        heights.resize((used.end.row as usize + 1).max(heights.len()), 1);

        for demand in &self.merge_demands {
            let cols = demand.range.col_count().max(1);
            let rows = demand.range.row_count().max(1);
            let (width, raw_lines) = text_metrics(&demand.display, limits.max_col_width);
            for col in demand.range.cols() {
                if widths.len() <= col as usize {
                    widths.resize(col as usize + 1, default_column);
                }
            }
            let auto_cols: Vec<u32> = demand
                .range
                .cols()
                .filter(|col| !fixed_columns.contains(col))
                .collect();
            if !auto_cols.is_empty() {
                let current_total = demand
                    .range
                    .cols()
                    .map(|col| widths[col as usize] as u32)
                    .sum::<u32>()
                    .saturating_add(CELL_GUTTER as u32 * cols.saturating_sub(1));
                let deficit = (width as u32).saturating_sub(current_total);
                if deficit > 0 {
                    let per_col = deficit
                        .div_ceil(auto_cols.len() as u32)
                        .min(u16::MAX as u32) as u16;
                    for col in auto_cols {
                        widths[col as usize] = widths[col as usize]
                            .saturating_add(per_col)
                            .min(limits.max_col_width);
                    }
                }
            }
            let total_width: u32 = demand
                .range
                .cols()
                .map(|col| widths.get(col as usize).copied().unwrap_or(default_column) as u32)
                .sum::<u32>()
                .saturating_add(CELL_GUTTER as u32 * cols.saturating_sub(1));
            let lines = if demand.wrap_text {
                wrapped_line_count(&demand.display, total_width.min(u16::MAX as u32) as u16) as u32
            } else {
                raw_lines as u32
            };
            let per_row = lines.div_ceil(rows).max(1) as u8;
            for row in demand.range.rows() {
                if heights.len() <= row as usize {
                    heights.resize(row as usize + 1, 1);
                }
                heights[row as usize] = heights[row as usize]
                    .max(per_row)
                    .min(limits.max_row_lines as u8);
            }
        }
        for demand in &self.wrap_demands {
            let addr = CellAddr::new(demand.row, demand.col);
            if merges.region_at(addr).is_some() {
                continue;
            }
            let width = widths
                .get(demand.col as usize)
                .copied()
                .unwrap_or(default_column);
            let lines = wrapped_line_count(&demand.display, width);
            if heights.len() <= demand.row as usize {
                heights.resize(demand.row as usize + 1, 1);
            }
            heights[demand.row as usize] = heights[demand.row as usize]
                .max(lines)
                .min(limits.max_row_lines as u8);
        }

        let mut fixed_rows = BTreeSet::new();
        if let Some(excel) = excel {
            for (&row, &height) in &excel.row_heights {
                let index = row as usize;
                if heights.len() <= index {
                    heights.resize(index + 1, 1);
                }
                heights[index] = excel_row_height(height, &limits);
                fixed_rows.insert(row);
            }
            for (row, height) in heights.iter_mut().enumerate() {
                if !fixed_rows.contains(&(row as u32)) {
                    *height = (*height).max(default_row);
                }
            }
        }

        LayoutSnapshot {
            column_widths: widths,
            row_heights: heights,
            fixed_columns,
            fixed_rows,
        }
    }
}

pub fn text_metrics(display: &str, max_width: u16) -> (u16, u8) {
    let mut width = 0usize;
    let mut lines = 0u8;
    for line in display.split('\n') {
        width = width.max(UnicodeWidthStr::width(line));
        lines = lines.saturating_add(1);
    }
    (width.min(max_width as usize) as u16, lines.max(1))
}

/// Number of visual lines after wrapping every explicit line at `width`.
pub fn wrapped_line_count(display: &str, width: u16) -> u8 {
    let width = width.max(1) as usize;
    let mut lines = 0u8;
    for raw in display.split('\n') {
        let count = if raw.is_empty() {
            1
        } else {
            textwrap::wrap(raw, Options::new(width)).len().max(1)
        };
        lines = lines.saturating_add(count.min(u8::MAX as usize) as u8);
    }
    lines.max(1)
}

/// Whether a display string is truncated when only `rendered_lines` lines are
/// available at `width`.
fn is_truncated(display: &str, wrap_text: bool, width: u16, rendered_lines: usize) -> bool {
    let expected_lines = if wrap_text {
        wrapped_line_count(display, width) as usize
    } else {
        display.lines().count().max(1)
    };
    expected_lines > rendered_lines
        || (!wrap_text && UnicodeWidthStr::width(display) > width as usize)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Viewport {
    /// Index into the scrollable (non-frozen, visible) row list.
    pub top_visible_row: u32,
    pub left_col: u32,
    /// Number of terminal lines available for data rows, excluding the status
    /// line, sheet tabs, and the grid column header.
    pub height: u16,
    pub width: u16,
}

pub const CELL_GUTTER: u16 = 1;

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderColumn {
    pub col: u32,
    pub x: u16,
    pub width: u16,
}

#[derive(Clone, Debug, Default)]
pub struct RenderPlan {
    pub frozen_rows: Vec<RenderRow>,
    pub scroll_rows: Vec<RenderRow>,
    /// Horizontal slots for every visible column, independent of merged cells.
    /// These are used for the column headers so that a merged title row does
    /// not hide the labels of the columns it spans.
    pub columns: Vec<RenderColumn>,
    pub frozen_cols: u32,
    pub scroll_left_col: u32,
    pub freeze: EffectiveFreeze,
}

#[derive(Clone, Debug, Default)]
pub struct CellMeasure {
    pub addr: CellAddr,
    pub x: u16,
    pub base_width: u16,
    pub width: u16,
    pub lines: Vec<StyledLine>,
    pub style: CellStyle,
    pub overflow: bool,
    pub truncated: bool,
    pub merge_span: Option<CellRange>,
}

#[derive(Clone, Debug, Default)]
pub struct RenderRow {
    pub row: u32,
    pub height: u16,
    pub cells: Vec<RenderCell>,
}

#[derive(Clone, Debug, Default)]
pub struct RenderCell {
    pub addr: CellAddr,
    pub x: u16,
    /// Width used for the cell's text. This includes gutters of empty
    /// neighbours when the text overflows.
    pub width: u16,
    /// Width of the cell itself, excluding overflow. Borders are drawn at
    /// `x + slot_width`.
    pub slot_width: u16,
    pub lines: Vec<StyledLine>,
    pub style: CellStyle,
    pub overflow: bool,
    pub truncated: bool,
    pub selected: bool,
    pub cursor: bool,
    pub search_match: bool,
    pub merge_span: Option<CellRange>,
    /// True when this cell is a slice of a merged range in a row other than
    /// the anchor row. The text lines are already sliced for this row.
    pub merge_continuation: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

#[derive(Clone, Debug, Default)]
pub struct StyledSpan {
    pub text: String,
    pub style: CellStyle,
}

/// Column widths used for one render. The sheet layout keeps the original
/// widths; this table may scale them down to fit the viewport.
#[derive(Clone, Debug, Default)]
pub struct ColumnWidths {
    widths: Vec<u16>,
    limited: bool,
}

/// Result of scaling column widths into an available viewport width.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FitWidths {
    pub widths: Vec<u16>,
    /// True when even one cell per column plus gutters cannot fit.
    pub limited: bool,
}

impl ColumnWidths {
    pub fn from_sheet(sheet: &Sheet) -> Self {
        let default_width = sheet.layout_limits.default_col_width.max(1);
        let end = sheet.used_range.end.col as usize;
        let widths = (0..=end)
            .map(|col| {
                sheet
                    .layout
                    .column_widths
                    .get(col)
                    .copied()
                    .unwrap_or(default_width)
                    .max(1)
            })
            .collect();
        Self {
            widths,
            limited: false,
        }
    }

    pub fn fit(sheet: &Sheet, available: u16) -> Self {
        let base = Self::from_sheet(sheet);
        let fitted = fit_column_widths(&base.widths, available);
        Self {
            widths: fitted.widths,
            limited: fitted.limited,
        }
    }

    pub fn width(&self, col: u32) -> u16 {
        self.widths.get(col as usize).copied().unwrap_or(1).max(1)
    }

    pub fn total_slots(&self) -> u32 {
        self.widths
            .iter()
            .map(|width| *width as u32 + CELL_GUTTER as u32)
            .sum()
    }

    pub fn limited(&self) -> bool {
        self.limited
    }
}

/// Scales `base` column widths down so that the columns and their gutters fit
/// in `available` terminal cells. Columns are never widened and always keep at
/// least one cell. Leftover cells are assigned in column order so the result is
/// deterministic.
pub fn fit_column_widths(base: &[u16], available: u16) -> FitWidths {
    let count = base.len();
    if count == 0 {
        return FitWidths::default();
    }
    let base: Vec<u32> = base.iter().map(|width| (*width).max(1) as u32).collect();
    let base_cells: u32 = base.iter().sum();
    let gutters = count as u32 * CELL_GUTTER as u32;
    if base_cells.saturating_add(gutters) <= available as u32 {
        return FitWidths {
            widths: base.iter().map(|width| *width as u16).collect(),
            limited: false,
        };
    }

    let budget = (available as u32).saturating_sub(gutters);
    if budget < count as u32 {
        return FitWidths {
            widths: vec![1; count],
            limited: true,
        };
    }

    let extra = budget - count as u32;
    let mut widths = Vec::with_capacity(count);
    let mut used = 0u32;
    let mut remainders = Vec::with_capacity(count);
    for (index, width) in base.iter().enumerate() {
        let scaled = width.saturating_mul(extra);
        let whole = scaled / base_cells;
        let remainder = scaled % base_cells;
        let next = (1 + whole).min(*width);
        used = used.saturating_add(next);
        widths.push(next as u16);
        remainders.push((remainder, index));
    }

    // Flooring leaves at most one cell per column unassigned. If a column is
    // already at its original width, skip it and continue down the list.
    let mut remaining = budget.saturating_sub(used);
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    while remaining > 0 {
        let mut assigned = false;
        for (_, index) in &remainders {
            if remaining == 0 {
                break;
            }
            if (widths[*index] as u32) < base[*index] {
                widths[*index] += 1;
                remaining -= 1;
                assigned = true;
            }
        }
        if !assigned {
            break;
        }
    }

    FitWidths {
        widths,
        limited: false,
    }
}

pub struct LayoutEngine;

impl LayoutEngine {
    pub fn compute(sheet: &Sheet, limits: &LayoutLimits) -> LayoutSnapshot {
        let mut builder = LayoutStatsBuilder::new(limits);
        if let SheetData::Eager(cells) = &sheet.data {
            for (addr, cell) in cells {
                if let Some(region) = sheet.merges.region_at(*addr) {
                    if region.anchor == *addr {
                        builder.observe_merge(region.range, &cell.display, cell.wrap_text);
                    }
                    continue;
                }
                builder.observe(addr.row, addr.col, &cell.display, cell.wrap_text);
            }
        }
        builder.finish(&sheet.merges, None, sheet.used_range)
    }

    pub fn effective_freeze(
        sheet: &Sheet,
        viewport: &Viewport,
        widths: &ColumnWidths,
    ) -> EffectiveFreeze {
        if !sheet.freeze.is_active() {
            return EffectiveFreeze::default();
        }
        let used = sheet.used_range;
        let requested_rows = sheet
            .freeze
            .active_rows()
            .min(used.end.row.saturating_add(1));
        let requested_cols = sheet
            .freeze
            .active_cols()
            .min(used.end.col.saturating_add(1));
        let mut rows = 0u32;
        let mut height = 0u16;
        let max_frozen_height = viewport.height.saturating_sub(3);
        for row in 0..requested_rows {
            let next = row_height(&sheet.layout, row);
            if height.saturating_add(next) > max_frozen_height {
                break;
            }
            height = height.saturating_add(next);
            rows += 1;
        }
        let mut cols = 0u32;
        let mut width = 0u16;
        let max_frozen_width = viewport.width.saturating_sub(12);
        for col in 0..requested_cols {
            let next = widths.width(col).saturating_add(CELL_GUTTER);
            if width.saturating_add(next) > max_frozen_width {
                break;
            }
            width = width.saturating_add(next);
            cols += 1;
        }
        EffectiveFreeze {
            rows,
            cols,
            rows_limited: rows < requested_rows,
            cols_limited: cols < requested_cols,
        }
    }

    pub fn plan(
        sheet: &Sheet,
        viewport: &Viewport,
        focus: CellAddr,
        selection: Option<&Selection>,
        search: Option<&SearchIndex>,
        widths: &ColumnWidths,
    ) -> RenderPlan {
        let layout = &sheet.layout;
        let freeze = Self::effective_freeze(sheet, viewport, widths);
        let focus = sheet.merges.anchor_for(focus);
        let mut frozen = Vec::new();
        let mut scroll = Vec::new();
        let mut used_height = 0u16;
        for row in 0..freeze.rows {
            let height = row_height(layout, row);
            if used_height + height > viewport.height {
                break;
            }
            used_height += height;
            frozen.push(render_row(
                sheet,
                viewport,
                selection,
                search,
                focus,
                row,
                freeze.cols,
                widths,
            ));
        }
        let visible = sheet.visibility.visible_rows();
        let scrollable: Vec<u32> = visible
            .iter()
            .copied()
            .filter(|row| *row >= freeze.rows)
            .collect();
        let start = viewport.top_visible_row as usize;
        for row in scrollable.into_iter().skip(start) {
            let height = row_height(layout, row);
            if used_height + height > viewport.height {
                break;
            }
            used_height += height;
            scroll.push(render_row(
                sheet,
                viewport,
                selection,
                search,
                focus,
                row,
                freeze.cols,
                widths,
            ));
        }
        RenderPlan {
            frozen_rows: frozen,
            scroll_rows: scroll,
            columns: plan_columns(sheet, viewport, freeze.cols, widths),
            frozen_cols: freeze.cols,
            scroll_left_col: viewport.left_col,
            freeze,
        }
    }

    pub fn measure_cell(sheet: &Sheet, addr: CellAddr, widths: &ColumnWidths) -> CellMeasure {
        let addr = sheet.merges.anchor_for(addr);
        let layout = &sheet.layout;
        let effective = sheet.effective_cell(addr).unwrap_or_else(|| EffectiveCell {
            value: CellValue::Empty,
            display: String::new(),
            style: CellStyle::default(),
            formula: None,
            formula_status: FormulaStatus::Pending,
        });
        let merge = sheet.merges.region_at(addr);
        let mut style = effective.style;
        if let Some(region) = merge
            && region.anchor == addr
            && let Some(borders) = sheet.merge_borders.get(&addr)
        {
            style.borders = *borders;
        }
        let base_width = if let Some(region) = merge {
            let cols = region.range.col_count().max(1) as u16;
            region
                .range
                .cols()
                .map(|col| widths.width(col))
                .sum::<u16>()
                .saturating_add(cols.saturating_sub(1) * CELL_GUTTER)
        } else {
            widths.width(addr.col)
        };
        let mut width = base_width;
        let mut overflow = false;
        let single_line = !effective.display.contains('\n') && !style.wrap_text;
        if single_line
            && !style.borders.right.is_visible()
            && UnicodeWidthStr::width(effective.display.as_str()) > width as usize
        {
            let mut next = addr.col + 1;
            while next <= sheet.used_range.end.col
                && !sheet.merges.is_blocker(CellAddr::new(addr.row, next))
            {
                let next_cell = sheet.effective_cell(CellAddr::new(addr.row, next));
                if next_cell.as_ref().is_some_and(|cell| !cell.is_empty()) {
                    break;
                }
                let next_borders = next_cell
                    .as_ref()
                    .map(|cell| cell.style.borders)
                    .unwrap_or_default();
                if next_borders.left.is_visible() {
                    break;
                }
                let next_width = widths.width(next);
                width = width.saturating_add(next_width).saturating_add(
                    if next_borders.right.is_visible() {
                        0
                    } else {
                        CELL_GUTTER
                    },
                );
                overflow = true;
                if next_borders.right.is_visible() {
                    break;
                }
                next += 1;
            }
        }
        let height = if let Some(region) = merge {
            merge_total_height(layout, region.range) as usize
        } else {
            row_height(layout, addr.row) as usize
        };
        let mut lines = wrap_display(&effective.display, style.wrap_text, width, height);
        let truncated = is_truncated(&effective.display, style.wrap_text, width, lines.len());
        if truncated && let Some(last) = lines.last_mut() {
            let text = last
                .spans
                .first()
                .map(|span| span.text.clone())
                .unwrap_or_default();
            let mut clipped = text;
            while UnicodeWidthStr::width(clipped.as_str()) + 1 > width as usize
                && !clipped.is_empty()
            {
                clipped.pop();
            }
            clipped.push('…');
            last.spans = vec![StyledSpan {
                text: clipped,
                style,
            }];
        }
        CellMeasure {
            addr,
            x: 0,
            base_width,
            width,
            lines,
            style,
            overflow,
            truncated,
            merge_span: merge.map(|region| region.range),
        }
    }

    pub fn truncated_at(sheet: &Sheet, addr: CellAddr, widths: &ColumnWidths) -> bool {
        Self::measure_cell(sheet, addr, widths).truncated
    }
}

impl LayoutSnapshot {
    pub fn grow_for_cell(
        &mut self,
        addr: CellAddr,
        display: &str,
        wrap_text: bool,
        merge: Option<CellRange>,
        limits: &LayoutLimits,
    ) -> LayoutInvalidation {
        let mut invalidation = LayoutInvalidation::default();
        let cols: Vec<u32> = merge
            .map(|range| range.cols().collect())
            .unwrap_or_else(|| vec![addr.col]);
        let rows: Vec<u32> = merge
            .map(|range| range.rows().collect())
            .unwrap_or_else(|| vec![addr.row]);
        let (width, raw_lines) = text_metrics(display, limits.max_col_width);
        let per_col = width.div_ceil(cols.len() as u16).max(1);
        for col in &cols {
            if self.fixed_columns.contains(col) {
                continue;
            }
            if self.column_widths.len() <= *col as usize {
                self.column_widths
                    .resize(*col as usize + 1, limits.default_col_width);
            }
            let current = self.column_widths[*col as usize];
            let next = current
                .max(per_col)
                .max(limits.min_col_width)
                .min(limits.max_col_width);
            if next != current {
                self.column_widths[*col as usize] = next;
                invalidation.columns.insert(*col);
            }
        }
        let total_width: u32 = cols
            .iter()
            .map(|col| {
                self.column_widths
                    .get(*col as usize)
                    .copied()
                    .unwrap_or(limits.default_col_width) as u32
            })
            .sum::<u32>()
            .saturating_add(CELL_GUTTER as u32 * (cols.len().saturating_sub(1) as u32));
        let lines = if wrap_text {
            wrapped_line_count(display, total_width.min(u16::MAX as u32) as u16) as u32
        } else {
            raw_lines as u32
        };
        let per_row = lines.div_ceil(rows.len() as u32).max(1) as u8;
        for row in &rows {
            if self.fixed_rows.contains(row) {
                continue;
            }
            if self.row_heights.len() <= *row as usize {
                self.row_heights.resize(*row as usize + 1, 1);
            }
            let current = self.row_heights[*row as usize];
            let next = current.max(per_row).min(limits.max_row_lines as u8).max(1);
            if next != current {
                self.row_heights[*row as usize] = next;
                invalidation.rows.insert(*row);
            }
        }
        invalidation
    }
}

fn plan_columns(
    sheet: &Sheet,
    viewport: &Viewport,
    frozen_cols: u32,
    widths: &ColumnWidths,
) -> Vec<RenderColumn> {
    let scroll_start = viewport.left_col.max(frozen_cols);
    let mut columns = Vec::new();
    let mut x = 0u16;
    for col in (0..frozen_cols).chain(scroll_start..=sheet.used_range.end.col) {
        if x >= viewport.width {
            break;
        }
        let width = widths.width(col);
        columns.push(RenderColumn { col, x, width });
        x = x.saturating_add(width).saturating_add(CELL_GUTTER);
    }
    columns
}

fn row_height(layout: &LayoutSnapshot, row: u32) -> u16 {
    layout
        .row_heights
        .get(row as usize)
        .copied()
        .unwrap_or(1)
        .max(1) as u16
}

fn merged_width(widths: &ColumnWidths, range: CellRange) -> u16 {
    let cols = range.col_count().max(1) as u16;
    range
        .cols()
        .map(|col| widths.width(col))
        .fold(0u16, u16::saturating_add)
        .saturating_add(cols.saturating_sub(1) * CELL_GUTTER)
}

fn merge_total_height(layout: &LayoutSnapshot, range: CellRange) -> u16 {
    range
        .rows()
        .map(|row| row_height(layout, row))
        .fold(0u16, u16::saturating_add)
}

fn merge_line_offset(layout: &LayoutSnapshot, range: CellRange, row: u32) -> u16 {
    range
        .rows()
        .take_while(|candidate| *candidate < row)
        .map(|candidate| row_height(layout, candidate))
        .fold(0u16, u16::saturating_add)
}

#[allow(clippy::too_many_arguments)]
fn render_row(
    sheet: &Sheet,
    viewport: &Viewport,
    selection: Option<&Selection>,
    search: Option<&SearchIndex>,
    focus: CellAddr,
    row: u32,
    frozen_cols: u32,
    widths: &ColumnWidths,
) -> RenderRow {
    let layout = &sheet.layout;
    let height = row_height(layout, row);
    let mut cells = Vec::new();
    let mut x = 0u16;
    let mut cols = Vec::new();
    for col in 0..frozen_cols {
        cols.push(col);
    }
    let scroll_start = viewport.left_col.max(frozen_cols);
    for col in scroll_start..=sheet.used_range.end.col {
        cols.push(col);
    }

    for col in cols {
        if x >= viewport.width {
            break;
        }
        let addr = CellAddr::new(row, col);

        if let Some(region) = sheet.merges.region_at(addr) {
            if col != region.range.start.col {
                // The merge was already rendered at its start column. If the
                // start column is horizontally scrolled out, keep the x
                // position of the remaining columns consistent.
                if region.range.start.col < scroll_start {
                    x = x.saturating_add(widths.width(col) + CELL_GUTTER);
                }
                continue;
            }

            let anchor = region.anchor;
            let width = merged_width(widths, region.range);
            let total_height = merge_total_height(layout, region.range) as usize;
            let line_offset = merge_line_offset(layout, region.range, row) as usize;
            let effective = sheet
                .effective_cell(anchor)
                .unwrap_or_else(|| EffectiveCell {
                    value: CellValue::Empty,
                    display: String::new(),
                    style: CellStyle::default(),
                    formula: None,
                    formula_status: FormulaStatus::Pending,
                });
            let mut style = effective.style;
            if let Some(borders) = sheet.merge_borders.get(&anchor) {
                style.borders = *borders;
            }

            let mut lines = wrap_display(
                &effective.display,
                style.wrap_text,
                width,
                total_height.max(1),
            );
            let truncated = is_truncated(&effective.display, style.wrap_text, width, lines.len());
            if truncated && let Some(last) = lines.last_mut() {
                let text = last
                    .spans
                    .first()
                    .map(|span| span.text.clone())
                    .unwrap_or_default();
                let mut clipped = text;
                while UnicodeWidthStr::width(clipped.as_str()) + 1 > width as usize
                    && !clipped.is_empty()
                {
                    clipped.pop();
                }
                clipped.push('…');
                last.spans = vec![StyledSpan {
                    text: clipped,
                    style,
                }];
            }
            let slice_end = line_offset.saturating_add(height as usize).min(lines.len());
            let slice = if line_offset < lines.len() {
                lines[line_offset..slice_end].to_vec()
            } else {
                Vec::new()
            };
            let selected = selection
                .is_some_and(|selection| selection.contains(sheet.used_range.end.col + 1, anchor));
            cells.push(RenderCell {
                addr: anchor,
                x,
                width,
                slot_width: width,
                lines: slice,
                style,
                overflow: false,
                truncated,
                selected,
                cursor: focus == anchor,
                search_match: search.is_some_and(|search| search.is_match(anchor)),
                merge_span: Some(region.range),
                merge_continuation: row != region.range.start.row,
            });
            x = x.saturating_add(width.saturating_add(CELL_GUTTER));
            continue;
        }

        let width = widths.width(col);
        let effective = sheet.effective_cell(addr).unwrap_or_else(|| EffectiveCell {
            value: CellValue::Empty,
            display: String::new(),
            style: CellStyle::default(),
            formula: None,
            formula_status: FormulaStatus::Pending,
        });
        let style = effective.style;
        let mut cell_width = width;
        let mut overflow = false;
        let single_line = !effective.display.contains('\n') && !style.wrap_text;
        if single_line
            && !style.borders.right.is_visible()
            && UnicodeWidthStr::width(effective.display.as_str()) > cell_width as usize
        {
            let mut next = col + 1;
            while next <= sheet.used_range.end.col
                && !sheet.merges.is_blocker(CellAddr::new(row, next))
            {
                let next_cell = sheet.effective_cell(CellAddr::new(row, next));
                if next_cell.as_ref().is_some_and(|cell| !cell.is_empty()) {
                    break;
                }
                let next_borders = next_cell
                    .as_ref()
                    .map(|cell| cell.style.borders)
                    .unwrap_or_default();
                if next_borders.left.is_visible() {
                    break;
                }
                let next_width = widths.width(next);
                cell_width = cell_width.saturating_add(next_width).saturating_add(
                    if next_borders.right.is_visible() {
                        0
                    } else {
                        CELL_GUTTER
                    },
                );
                overflow = true;
                if next_borders.right.is_visible() {
                    break;
                }
                next += 1;
            }
        }
        let max_lines = height as usize;
        let mut lines = wrap_display(&effective.display, style.wrap_text, cell_width, max_lines);
        let truncated = is_truncated(&effective.display, style.wrap_text, cell_width, lines.len());
        if truncated && let Some(last) = lines.last_mut() {
            let text = last
                .spans
                .first()
                .map(|span| span.text.clone())
                .unwrap_or_default();
            let mut clipped = text;
            while UnicodeWidthStr::width(clipped.as_str()) + 1 > cell_width as usize
                && !clipped.is_empty()
            {
                clipped.pop();
            }
            clipped.push('…');
            last.spans = vec![StyledSpan {
                text: clipped,
                style,
            }];
        }
        let selected = selection
            .is_some_and(|selection| selection.contains(sheet.used_range.end.col + 1, addr));
        let cursor = focus == addr;
        cells.push(RenderCell {
            addr,
            x,
            width: cell_width,
            slot_width: width,
            lines,
            style,
            overflow,
            truncated,
            selected,
            cursor,
            search_match: search.is_some_and(|search| search.is_match(addr)),
            merge_span: None,
            merge_continuation: false,
        });
        x = x.saturating_add(width.saturating_add(CELL_GUTTER));
    }
    RenderRow { row, height, cells }
}

fn wrap_display(display: &str, wrap: bool, width: u16, max_lines: usize) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    for raw in display.split('\n') {
        if wrap && width > 0 {
            for line in textwrap::wrap(raw, Options::new(width as usize)) {
                lines.push(StyledLine {
                    spans: vec![StyledSpan {
                        text: line.to_string(),
                        style: CellStyle::default(),
                    }],
                });
            }
        } else {
            lines.push(StyledLine {
                spans: vec![StyledSpan {
                    text: raw.to_string(),
                    style: CellStyle::default(),
                }],
            });
        }
    }
    lines.truncate(max_lines.max(1));
    if lines.is_empty() {
        lines.push(StyledLine::default());
    }
    lines
}

#[allow(dead_code)]
fn _default_visibility(row_count: u32) -> VisibilityMap {
    VisibilityMap::all(row_count)
}

#[allow(dead_code)]
fn _cell_style(cell: &Cell) -> CellStyle {
    cell.style
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(rows: u32, cols: u32) -> CellRange {
        CellRange::new(CellAddr::new(0, 0), CellAddr::new(rows, cols))
    }

    #[test]
    fn excel_column_width_floors_and_clamps() {
        let limits = LayoutLimits::default();
        assert_eq!(excel_column_width(24.710_938, &limits), 24);
        assert_eq!(excel_column_width(8.7109375, &limits), 8);
        assert_eq!(excel_column_width(0.5, &limits), limits.min_col_width);
        assert_eq!(excel_column_width(1_000.0, &limits), limits.max_col_width);
        assert_eq!(excel_column_width(f32::NAN, &limits), limits.min_col_width);
    }

    #[test]
    fn excel_row_height_rounds_points() {
        let limits = LayoutLimits::default();
        assert_eq!(excel_row_height(15.0, &limits), 1);
        assert_eq!(excel_row_height(30.0, &limits), 2);
        assert_eq!(excel_row_height(22.5, &limits), 2);
        assert_eq!(excel_row_height(0.0, &limits), 1);
        assert_eq!(excel_row_height(300.0, &limits), limits.max_row_lines as u8);
        assert_eq!(excel_row_height(f32::NAN, &limits), 1);
    }

    #[test]
    fn explicit_column_width_replaces_auto_width() {
        let limits = LayoutLimits::default();
        let mut builder = LayoutStatsBuilder::new(&limits);
        builder.observe(0, 0, "a very long cell", false);
        let excel = ExcelLayout {
            column_widths: BTreeMap::from([(0, 10.0)]),
            ..ExcelLayout::default()
        };
        let layout = builder.finish(&MergeIndex::empty(), Some(&excel), used(1, 1));
        assert_eq!(layout.column_widths[0], 10);
        assert!(layout.fixed_columns.contains(&0));
    }

    #[test]
    fn unspecified_columns_keep_auto_width() {
        let limits = LayoutLimits::default();
        let mut builder = LayoutStatsBuilder::new(&limits);
        builder.observe(0, 0, "a very long cell", false);
        builder.observe(0, 1, "short", false);
        let layout = builder.finish(&MergeIndex::empty(), None, used(1, 1));
        assert!(layout.column_widths[0] > layout.column_widths[1]);
        assert!(!layout.fixed_columns.contains(&0));
    }

    #[test]
    fn explicit_row_height_replaces_wrapped_height() {
        let limits = LayoutLimits::default();
        let mut builder = LayoutStatsBuilder::new(&limits);
        builder.observe(0, 0, "0123456789", true);
        let excel = ExcelLayout {
            column_widths: BTreeMap::from([(0, 5.0)]),
            row_heights: BTreeMap::from([(0, 15.0)]),
            ..ExcelLayout::default()
        };
        let layout = builder.finish(&MergeIndex::empty(), Some(&excel), used(0, 0));
        assert_eq!(layout.column_widths[0], 5);
        assert_eq!(layout.row_heights[0], 1);
        assert!(layout.fixed_rows.contains(&0));
    }

    #[test]
    fn merge_width_is_not_distributed_to_fixed_columns() {
        let limits = LayoutLimits::default();
        let mut builder = LayoutStatsBuilder::new(&limits);
        let range = CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 1));
        builder.observe_merge(range, "01234567890123456789", false);
        let excel = ExcelLayout {
            column_widths: BTreeMap::from([(0, 4.0)]),
            ..ExcelLayout::default()
        };
        let layout = builder.finish(&MergeIndex::empty(), Some(&excel), used(0, 1));
        assert_eq!(layout.column_widths[0], 4);
        assert!(layout.column_widths[1] > 4);
    }

    #[test]
    fn excel_defaults_are_used_as_minimums() {
        let limits = LayoutLimits::default();
        let mut builder = LayoutStatsBuilder::new(&limits);
        builder.observe(0, 0, "x", false);
        let excel = ExcelLayout {
            default_column_width: Some(12.0),
            default_row_height: Some(30.0),
            ..ExcelLayout::default()
        };
        let layout = builder.finish(&MergeIndex::empty(), Some(&excel), used(0, 0));
        assert_eq!(layout.column_widths[0], 12);
        assert_eq!(layout.row_heights[0], 2);
        assert!(!layout.fixed_columns.contains(&0));
        assert!(!layout.fixed_rows.contains(&0));
    }

    #[test]
    fn grow_for_cell_skips_fixed_columns_and_rows() {
        let limits = LayoutLimits::default();
        let mut layout = LayoutSnapshot {
            column_widths: vec![10, 10],
            row_heights: vec![1, 1],
            fixed_columns: BTreeSet::from([0]),
            fixed_rows: BTreeSet::from([0]),
        };

        layout.grow_for_cell(CellAddr::new(0, 0), &"x".repeat(50), false, None, &limits);
        assert_eq!(layout.column_widths[0], 10);
        assert_eq!(layout.row_heights[0], 1);

        layout.grow_for_cell(
            CellAddr::new(1, 1),
            &format!("{}\nsecond", "x".repeat(50)),
            false,
            None,
            &limits,
        );
        assert!(layout.column_widths[1] > 10);
        assert!(layout.row_heights[1] > 1);
    }

    #[test]
    fn fit_widths_keep_base_when_already_fits() {
        let fitted = fit_column_widths(&[8, 4, 2], 20);
        assert_eq!(fitted.widths, vec![8, 4, 2]);
        assert!(!fitted.limited);
    }

    #[test]
    fn fit_widths_scale_proportionally_and_preserve_order() {
        let fitted = fit_column_widths(&[20, 10, 5], 20);
        assert_eq!(fitted.widths, vec![9, 5, 3]);
        assert_eq!(
            fitted
                .widths
                .iter()
                .map(|width| *width as u32 + CELL_GUTTER as u32)
                .sum::<u32>(),
            20
        );
        assert!(!fitted.limited);
        assert!(fitted.widths[0] > fitted.widths[1]);
        assert!(fitted.widths[1] > fitted.widths[2]);
    }

    #[test]
    fn fit_widths_never_exceed_base_and_keep_one_cell() {
        let fitted = fit_column_widths(&[1, 100, 3], 10);
        assert_eq!(fitted.widths, vec![1, 5, 1]);
        assert!(fitted.widths[1] <= 100);
        assert!(fitted.widths.iter().all(|width| *width >= 1));
        assert!(!fitted.limited);
    }

    #[test]
    fn fit_widths_report_limited_when_even_one_cell_does_not_fit() {
        let fitted = fit_column_widths(&[2, 2, 2], 5);
        assert_eq!(fitted.widths, vec![1, 1, 1]);
        assert!(fitted.limited);
    }

    #[test]
    fn fit_widths_handle_empty_and_single_column() {
        assert_eq!(
            fit_column_widths(&[], 10),
            FitWidths {
                widths: Vec::new(),
                limited: false
            }
        );
        let fitted = fit_column_widths(&[10], 11);
        assert_eq!(fitted.widths, vec![10]);
        assert!(!fitted.limited);
        let fitted = fit_column_widths(&[10], 5);
        assert_eq!(fitted.widths, vec![4]);
        assert!(!fitted.limited);
        let fitted = fit_column_widths(&[10], 1);
        assert_eq!(fitted.widths, vec![1]);
        assert!(fitted.limited);
    }

    #[test]
    fn fit_widths_are_deterministic_for_ties() {
        let first = fit_column_widths(&[5, 5, 5], 8);
        let second = fit_column_widths(&[5, 5, 5], 8);
        assert_eq!(first, second);
        assert_eq!(first.widths.iter().sum::<u16>() + 3, 8);
    }

    #[test]
    fn fit_widths_clamp_zero_base_width_to_one() {
        let fitted = fit_column_widths(&[0, 4], 10);
        assert_eq!(fitted.widths, vec![1, 4]);
        assert!(!fitted.limited);
    }
}
