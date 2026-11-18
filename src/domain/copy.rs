use crate::domain::addr::CellRange;
use crate::domain::cell::EffectiveCell;
use crate::domain::error::CopyError;
use crate::domain::filter::VisibilityMap;
use crate::domain::merge::MergeIndex;
use crate::domain::selection::Selection;
use crate::domain::sheet::Sheet;

#[derive(Clone, Copy, Debug)]
pub struct CopyLimits {
    pub max_cells: usize,
}

impl Default for CopyLimits {
    fn default() -> Self {
        Self { max_cells: 200_000 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CopyPayload {
    pub text: String,
    pub cell_count: usize,
}

pub struct CopyEncoder;

impl CopyEncoder {
    pub fn single(cell: &EffectiveCell) -> CopyPayload {
        CopyPayload {
            text: cell.display.clone(),
            cell_count: 1,
        }
    }

    pub fn quote_tsv_field(value: &str) -> String {
        if value.contains('\t')
            || value.contains('\n')
            || value.contains('\r')
            || value.contains('"')
        {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }
}

pub struct CopySession {
    range: CellRange,
    merges: MergeIndex,
    visibility: VisibilityMap,
    limits: CopyLimits,
    buffer: String,
    rows_written: usize,
    cells_written: usize,
}

impl CopySession {
    pub fn start(
        sheet: &Sheet,
        selection: &Selection,
        merges: &MergeIndex,
        visibility: &VisibilityMap,
        limits: &CopyLimits,
    ) -> Result<Self, CopyError> {
        let rows = sheet
            .meta
            .row_count
            .max(sheet.used_range.end.row.saturating_add(1));
        let cols = sheet.used_range.end.col.saturating_add(1);
        let range = selection.range(rows, cols);
        let cells = (range.row_count() as usize).saturating_mul(range.col_count() as usize);
        if cells > limits.max_cells {
            return Err(CopyError::TooLarge {
                cells,
                limit: limits.max_cells,
            });
        }
        Ok(Self {
            range,
            merges: merges.clone(),
            visibility: visibility.clone(),
            limits: *limits,
            buffer: String::new(),
            rows_written: 0,
            cells_written: 0,
        })
    }

    pub fn feed_row(&mut self, row: u32, cells: &[EffectiveCell]) -> Result<(), CopyError> {
        if row < self.range.start.row
            || row > self.range.end.row
            || !self.visibility.is_visible(row)
        {
            return Ok(());
        }
        if self.rows_written > 0 {
            self.buffer.push_str("\r\n");
        }
        let mut first = true;
        for col in self.range.cols() {
            if !first {
                self.buffer.push('\t');
            }
            first = false;
            let index = col.saturating_sub(self.range.start.col) as usize;
            let addr = crate::domain::addr::CellAddr::new(row, col);
            let text = if self.merges.is_covered(addr) {
                String::new()
            } else {
                cells
                    .get(index)
                    .map(|cell| cell.display.clone())
                    .unwrap_or_default()
            };
            self.buffer.push_str(&CopyEncoder::quote_tsv_field(&text));
            self.cells_written += 1;
        }
        self.rows_written += 1;
        if self.cells_written > self.limits.max_cells {
            return Err(CopyError::TooLarge {
                cells: self.cells_written,
                limit: self.limits.max_cells,
            });
        }
        Ok(())
    }

    pub fn finish(self) -> Result<CopyPayload, CopyError> {
        Ok(CopyPayload {
            text: self.buffer,
            cell_count: self.cells_written,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_tsv_fields() {
        assert_eq!(CopyEncoder::quote_tsv_field("plain"), "plain");
        assert_eq!(CopyEncoder::quote_tsv_field("a\tb"), "\"a\tb\"");
        assert_eq!(CopyEncoder::quote_tsv_field("a\"b"), "\"a\"\"b\"");
    }
}
