use crate::domain::addr::CellAddr;
use crate::domain::freeze::EffectiveFreeze;
use crate::domain::layout::{CELL_GUTTER, LayoutLimits, LayoutSnapshot, Viewport};
use crate::domain::sheet::Sheet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportPlan {
    pub top_visible_row: u32,
    pub left_col: u32,
}

pub struct ViewportPlanner;

impl ViewportPlanner {
    pub fn ensure_visible(
        sheet: &Sheet,
        viewport: &Viewport,
        focus: CellAddr,
        freeze: EffectiveFreeze,
    ) -> ViewportPlan {
        let focus = sheet.merges.anchor_for(focus);
        let top_visible_row = Self::vertical(sheet, viewport, focus, freeze);
        let left_col = Self::horizontal(sheet, viewport, focus, freeze);
        ViewportPlan {
            top_visible_row,
            left_col,
        }
    }

    fn vertical(
        sheet: &Sheet,
        viewport: &Viewport,
        focus: CellAddr,
        freeze: EffectiveFreeze,
    ) -> u32 {
        if focus.row < freeze.rows {
            return 0;
        }
        let visible = sheet.visibility.visible_rows();
        if visible.is_empty() {
            return 0;
        }
        let scrollable: Vec<u32> = visible
            .iter()
            .copied()
            .filter(|row| *row >= freeze.rows)
            .collect();
        if scrollable.is_empty() {
            return 0;
        }
        let fallback = scrollable
            .iter()
            .position(|row| *row >= focus.row)
            .unwrap_or_else(|| scrollable.len().saturating_sub(1));
        // A merged focus cell is rendered from its anchor row, but its text
        // spans every row in the merge. Make the whole range visible when it
        // fits; otherwise keep the anchor visible and let the range scroll.
        let span = sheet
            .merges
            .region_at(focus)
            .map(|region| region.range)
            .unwrap_or_else(|| crate::domain::addr::CellRange::single(focus));
        let first = scrollable
            .iter()
            .position(|row| *row >= span.start.row)
            .unwrap_or(fallback);
        let last = scrollable
            .iter()
            .rposition(|row| *row <= span.end.row)
            .filter(|last| *last >= first)
            .unwrap_or(first);
        let frozen_height: u32 = (0..freeze.rows)
            .map(|row| row_height(sheet, row))
            .sum::<u32>()
            .min(viewport.height as u32);
        let available = (viewport.height as u32)
            .saturating_sub(frozen_height)
            .max(1) as usize;
        let span_height: u32 = (first..=last)
            .map(|index| row_height(sheet, scrollable[index]))
            .sum();
        let target_end = if span_height as usize <= available {
            last
        } else {
            first
        };
        let max_top = scrollable.len().saturating_sub(1);
        let mut top = (viewport.top_visible_row as usize).min(max_top);

        // Scroll up if the target starts above the current viewport.
        if top > first {
            top = first;
        }
        // Scroll down until the target end fits, accounting for rows that
        // occupy more than one terminal line.
        if target_end >= top {
            let used: u32 = (top..=target_end)
                .map(|index| row_height(sheet, scrollable[index]))
                .sum();
            if used > available as u32 {
                let mut candidate = target_end;
                let mut height = row_height(sheet, scrollable[candidate]);
                while candidate > 0 {
                    let previous = row_height(sheet, scrollable[candidate - 1]);
                    if height + previous > available as u32 {
                        break;
                    }
                    height += previous;
                    candidate -= 1;
                }
                top = candidate;
            }
        }
        top as u32
    }

    fn horizontal(
        sheet: &Sheet,
        viewport: &Viewport,
        focus: CellAddr,
        freeze: EffectiveFreeze,
    ) -> u32 {
        let used_end = sheet.used_range.end.col;
        let frozen_cols = freeze.cols.min(used_end.saturating_add(1));
        let mut left = viewport.left_col.max(frozen_cols).min(used_end);
        if focus.col < frozen_cols {
            return left;
        }
        let span = sheet
            .merges
            .region_at(focus)
            .map(|region| region.range)
            .unwrap_or_else(|| crate::domain::addr::CellRange::single(focus));
        let span_start = span.start.col.max(frozen_cols);
        let span_end = span.end.col.max(span_start);
        if left > span_start {
            left = span_start;
        }
        if left < frozen_cols {
            left = frozen_cols;
        }
        let frozen_width: u32 = (0..frozen_cols)
            .map(|col| slot_width(sheet, col) as u32)
            .sum();
        let available = (viewport.width as u32)
            .saturating_sub(frozen_width.min(viewport.width as u32))
            .max(1);
        let mut width = Self::range_width(sheet, left, span_end);
        while left < span_start && width > available {
            width = width.saturating_sub(slot_width(sheet, left) as u32);
            left += 1;
        }
        left.min(used_end)
    }

    fn range_width(sheet: &Sheet, start: u32, end: u32) -> u32 {
        (start..=end).map(|col| slot_width(sheet, col) as u32).sum()
    }
}

fn slot_width(sheet: &Sheet, col: u32) -> u16 {
    let limits = sheet.layout_limits;
    column_width(&sheet.layout, col, &limits).saturating_add(CELL_GUTTER)
}

fn row_height(sheet: &Sheet, row: u32) -> u32 {
    sheet
        .layout
        .row_heights
        .get(row as usize)
        .copied()
        .unwrap_or(1)
        .max(1) as u32
}

fn column_width(layout: &LayoutSnapshot, col: u32, limits: &LayoutLimits) -> u16 {
    layout
        .column_widths
        .get(col as usize)
        .copied()
        .unwrap_or(limits.default_col_width)
        .max(1)
}
