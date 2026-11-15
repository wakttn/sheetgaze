use std::collections::HashMap;

use crate::domain::addr::CellAddr;
use crate::domain::cell::CellValue;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FormulaStatus {
    NotRequested,
    Pending,
    CachedOnly,
    Calculated,
    Error(String),
    Unsupported(String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FormulaInfo {
    pub expression: String,
    pub cached: Option<CellValue>,
    pub evaluated: Option<CellValue>,
    pub status: FormulaStatus,
    pub array_shape: Option<(u32, u32)>,
}

impl FormulaInfo {
    pub fn new(expression: String, cached: Option<CellValue>) -> Self {
        Self {
            expression,
            cached,
            evaluated: None,
            status: FormulaStatus::CachedOnly,
            array_shape: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FormulaCellResult {
    pub sheet: String,
    pub addr: CellAddr,
    pub value: CellValue,
    pub status: FormulaStatus,
}

#[derive(Clone, Debug, Default)]
pub struct FormulaResults {
    pub generation: u64,
    pub updates: Vec<FormulaCellResult>,
}

#[derive(Clone, Debug, Default)]
pub struct FormulaOverlay {
    pub values: HashMap<CellAddr, FormulaCellResult>,
}

impl FormulaOverlay {
    pub fn get(&self, addr: CellAddr) -> Option<&FormulaCellResult> {
        self.values.get(&addr)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecalcPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug)]
pub struct RecalcConfig {
    pub policy: RecalcPolicy,
    pub max_cells: u64,
    pub max_bytes: u64,
}

impl Default for RecalcConfig {
    fn default() -> Self {
        Self {
            policy: RecalcPolicy::Auto,
            max_cells: 500_000,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}
