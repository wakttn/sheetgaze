use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

use formualizer::workbook::{NativeXlsxAdapter, SpreadsheetReader};
use formualizer::{LiteralValue, LoadStrategy, Workbook, WorkbookConfig};

use crate::domain::addr::CellAddr;
use crate::domain::cell::CellValue;
use crate::domain::error::FormulaError;
use crate::domain::formula::{
    FormulaCellResult, FormulaResults, FormulaStatus, RecalcConfig, RecalcPolicy,
};
use crate::domain::task::CancelToken;

#[derive(Clone, Debug)]
pub enum FormulaMessage {
    Results(FormulaResults),
    Failed { generation: u64, message: String },
}

pub struct FormulaHandle {
    pub receiver: Receiver<FormulaMessage>,
    pub cancel: CancelToken,
    _thread: JoinHandle<()>,
}

pub fn spawn_recalculation(
    path: PathBuf,
    job_cells: Vec<(String, CellAddr)>,
    generation: u64,
) -> FormulaHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = CancelToken::new();
    let job_cancel = cancel.clone();
    let thread = std::thread::spawn(move || {
        let message = match recalculate(&path, &job_cells, &job_cancel, generation) {
            Ok(results) => FormulaMessage::Results(results),
            Err(error) => FormulaMessage::Failed {
                generation,
                message: error.to_string(),
            },
        };
        let _ = tx.send(message);
    });
    FormulaHandle {
        receiver: rx,
        cancel,
        _thread: thread,
    }
}

pub fn should_recalculate(
    policy: RecalcPolicy,
    total_cells: u64,
    file_bytes: u64,
    config: &RecalcConfig,
) -> bool {
    match policy {
        RecalcPolicy::Always => true,
        RecalcPolicy::Never => false,
        RecalcPolicy::Auto => total_cells <= config.max_cells && file_bytes <= config.max_bytes,
    }
}

pub fn recalculate(
    path: &Path,
    formula_cells: &[(String, CellAddr)],
    cancel: &CancelToken,
    generation: u64,
) -> Result<FormulaResults, FormulaError> {
    if formula_cells.is_empty() {
        return Ok(FormulaResults {
            generation,
            updates: Vec::new(),
        });
    }
    let reader = NativeXlsxAdapter::open_path(path)
        .map_err(|error| FormulaError::Engine(error.to_string()))?;
    let mut workbook =
        Workbook::from_reader(reader, LoadStrategy::EagerAll, WorkbookConfig::ephemeral())
            .map_err(|error| FormulaError::Engine(error.to_string()))?;
    workbook
        .evaluate_all()
        .map_err(|error| FormulaError::Engine(error.to_string()))?;
    let mut updates = Vec::new();
    for (sheet, addr) in formula_cells {
        if cancel.is_cancelled() {
            return Err(FormulaError::Cancelled);
        }
        let value = workbook.get_value(sheet, addr.row + 1, addr.col + 1);
        let (value, status) = match value {
            Some(LiteralValue::Int(value)) => {
                (CellValue::Number(value as f64), FormulaStatus::Calculated)
            }
            Some(LiteralValue::Number(value)) => {
                (CellValue::Number(value), FormulaStatus::Calculated)
            }
            Some(LiteralValue::Text(value)) => (CellValue::Text(value), FormulaStatus::Calculated),
            Some(LiteralValue::Boolean(value)) => {
                (CellValue::Bool(value), FormulaStatus::Calculated)
            }
            Some(LiteralValue::Date(value)) => (
                CellValue::DateTime(value.to_string()),
                FormulaStatus::Calculated,
            ),
            Some(LiteralValue::DateTime(value)) => (
                CellValue::DateTime(value.to_string()),
                FormulaStatus::Calculated,
            ),
            Some(LiteralValue::Time(value)) => (
                CellValue::DateTime(value.to_string()),
                FormulaStatus::Calculated,
            ),
            Some(LiteralValue::Duration(value)) => (
                CellValue::DateTime(value.to_string()),
                FormulaStatus::Calculated,
            ),
            Some(LiteralValue::Error(error)) => (
                CellValue::Error(error.to_string()),
                FormulaStatus::Error(error.to_string()),
            ),
            Some(LiteralValue::Array(values)) => {
                let first = values
                    .first()
                    .and_then(|row| row.first())
                    .cloned()
                    .unwrap_or(LiteralValue::Empty);
                let converted = literal_to_cell(first);
                (
                    CellValue::ArraySummary {
                        rows: values.len() as u32,
                        cols: values.first().map(|row| row.len()).unwrap_or(0) as u32,
                        value: Box::new(converted),
                    },
                    FormulaStatus::Calculated,
                )
            }
            Some(LiteralValue::Empty) | Some(LiteralValue::Pending) | None => {
                (CellValue::Empty, FormulaStatus::CachedOnly)
            }
        };
        updates.push(FormulaCellResult {
            sheet: sheet.clone(),
            addr: *addr,
            value,
            status,
        });
    }
    Ok(FormulaResults {
        generation,
        updates,
    })
}

fn literal_to_cell(value: LiteralValue) -> CellValue {
    match value {
        LiteralValue::Int(value) => CellValue::Number(value as f64),
        LiteralValue::Number(value) => CellValue::Number(value),
        LiteralValue::Text(value) => CellValue::Text(value),
        LiteralValue::Boolean(value) => CellValue::Bool(value),
        LiteralValue::Date(value) => CellValue::DateTime(value.to_string()),
        LiteralValue::DateTime(value) => CellValue::DateTime(value.to_string()),
        LiteralValue::Time(value) => CellValue::DateTime(value.to_string()),
        LiteralValue::Duration(value) => CellValue::DateTime(value.to_string()),
        LiteralValue::Error(error) => CellValue::Error(error.to_string()),
        LiteralValue::Array(values) => {
            let rows = values.len() as u32;
            let cols = values.first().map(|row| row.len()).unwrap_or(0) as u32;
            let first = values
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .map(literal_to_cell)
                .unwrap_or(CellValue::Empty);
            CellValue::ArraySummary {
                rows,
                cols,
                value: Box::new(first),
            }
        }
        LiteralValue::Empty | LiteralValue::Pending => CellValue::Empty,
    }
}
