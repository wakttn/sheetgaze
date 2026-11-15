use thiserror::Error;

use crate::domain::addr::CellRange;
use crate::domain::merge::MergeIndex;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FreezeWarning {
    #[error("unsupported pane state: {state}")]
    UnsupportedPaneState { sheet: String, state: String },
    #[error("invalid freeze split: {message}")]
    InvalidSplit { sheet: String, message: String },
    #[error("freeze boundary intersects merged cell: {range:?}")]
    MergeBoundary { sheet: String, range: CellRange },
    #[error("freeze area was limited to leave a scrollable region")]
    AreaLimited { sheet: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreezePane {
    pub frozen_rows: u32,
    pub frozen_cols: u32,
    pub disabled: bool,
}

/// Freeze area after validity and viewport limits have been applied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectiveFreeze {
    pub rows: u32,
    pub cols: u32,
    pub rows_limited: bool,
    pub cols_limited: bool,
}

impl EffectiveFreeze {
    pub fn is_active(self) -> bool {
        self.rows > 0 || self.cols > 0
    }
}

impl FreezePane {
    pub fn new(frozen_rows: u32, frozen_cols: u32) -> Self {
        Self {
            frozen_rows,
            frozen_cols,
            disabled: false,
        }
    }

    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !self.disabled && (self.frozen_rows > 0 || self.frozen_cols > 0)
    }

    /// Row count that must be treated as frozen by rendering and navigation.
    pub fn active_rows(&self) -> u32 {
        if self.is_active() {
            self.frozen_rows
        } else {
            0
        }
    }

    /// Column count that must be treated as frozen by rendering and navigation.
    pub fn active_cols(&self) -> u32 {
        if self.is_active() {
            self.frozen_cols
        } else {
            0
        }
    }

    pub fn validate(
        &self,
        used: CellRange,
        merges: &MergeIndex,
        sheet: &str,
    ) -> Result<(), FreezeWarning> {
        if self.frozen_rows > used.end.row + 1 {
            return Err(FreezeWarning::InvalidSplit {
                sheet: sheet.to_string(),
                message: "frozen rows exceed sheet range".to_string(),
            });
        }
        if self.frozen_cols > used.end.col + 1 {
            return Err(FreezeWarning::InvalidSplit {
                sheet: sheet.to_string(),
                message: "frozen columns exceed sheet range".to_string(),
            });
        }
        if self.frozen_rows > 0 || self.frozen_cols > 0 {
            let boundary_rows = if self.frozen_rows > 0 {
                Some(self.frozen_rows - 1)
            } else {
                None
            };
            let boundary_cols = if self.frozen_cols > 0 {
                Some(self.frozen_cols - 1)
            } else {
                None
            };
            for region in merges.regions() {
                let row_cross = boundary_rows
                    .is_some_and(|row| region.range.start.row <= row && region.range.end.row > row);
                let col_cross = boundary_cols
                    .is_some_and(|col| region.range.start.col <= col && region.range.end.col > col);
                if row_cross || col_cross {
                    return Err(FreezeWarning::MergeBoundary {
                        sheet: sheet.to_string(),
                        range: region.range,
                    });
                }
            }
        }
        Ok(())
    }
}
