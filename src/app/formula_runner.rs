use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use tempfile::NamedTempFile;

use crate::domain::formula::{RecalcConfig, RecalcPolicy};
use crate::domain::task::Generation;
use crate::domain::workbook::WorkbookModel;
use crate::io::formula::{self, FormulaHandle, FormulaMessage};

#[derive(Clone, Debug, Default)]
pub struct FormulaStart {
    pub generation: u64,
    pub started: bool,
    pub skipped_reason: Option<String>,
}

#[derive(Default)]
pub struct FormulaRunner {
    handle: Option<FormulaHandle>,
    temp: Option<NamedTempFile>,
}

impl FormulaRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restart(&mut self, workbook: &mut WorkbookModel, config: &RecalcConfig) -> FormulaStart {
        let path = workbook.source.path.clone();
        let file_bytes = workbook.source.fingerprint.len;
        self.restart_with_path(workbook, config, path, file_bytes, None)
    }

    /// Restarts calculation from a temporary patched workbook that contains
    /// unsaved edits. The temporary file is kept alive until the job finishes.
    pub fn restart_with_temp(
        &mut self,
        workbook: &mut WorkbookModel,
        config: &RecalcConfig,
        temp: NamedTempFile,
    ) -> FormulaStart {
        let path = temp.path().to_path_buf();
        let file_bytes = temp
            .as_file()
            .metadata()
            .map(|meta| meta.len())
            .unwrap_or(0);
        self.restart_with_path(workbook, config, path, file_bytes, Some(temp))
    }

    fn restart_with_path(
        &mut self,
        workbook: &mut WorkbookModel,
        config: &RecalcConfig,
        path: PathBuf,
        file_bytes: u64,
        temp: Option<NamedTempFile>,
    ) -> FormulaStart {
        self.cancel();
        let generation = workbook.generation.0.saturating_add(1);
        workbook.generation = Generation(generation);
        let mut formula_cells = Vec::new();
        let mut total_cells = 0u64;
        for sheet in &workbook.sheets {
            total_cells = total_cells
                .saturating_add((sheet.meta.row_count as u64) * (sheet.meta.column_count as u64));
            for addr in &sheet.formula_cells {
                formula_cells.push((sheet.name.clone(), *addr));
            }
        }
        let should_recalculate =
            formula::should_recalculate(config.policy, total_cells, file_bytes, config)
                && !formula_cells.is_empty()
                && config.policy != RecalcPolicy::Never;
        if !should_recalculate {
            return FormulaStart {
                generation,
                started: false,
                skipped_reason: (!formula_cells.is_empty() && config.policy != RecalcPolicy::Never)
                    .then(|| "formula recalculation skipped: size policy".to_string()),
            };
        }
        self.temp = temp;
        self.handle = Some(formula::spawn_recalculation(
            path,
            formula_cells,
            generation,
        ));
        FormulaStart {
            generation,
            started: true,
            skipped_reason: None,
        }
    }

    pub fn poll(&mut self) -> Vec<FormulaMessage> {
        let Some(handle) = &mut self.handle else {
            return Vec::new();
        };
        let mut messages = Vec::new();
        loop {
            match handle.receiver.try_recv() {
                Ok(message) => messages.push(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.handle = None;
                    self.temp = None;
                    break;
                }
            }
        }
        messages
    }

    /// Blocks until the current recalculation finishes and returns its message.
    pub fn wait(&mut self) -> Option<FormulaMessage> {
        let handle = self.handle.take()?;
        let message = handle.receiver.recv().ok();
        self.temp = None;
        message
    }

    pub fn cancel(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.cancel.cancel();
        }
        self.temp = None;
    }
}

impl Drop for FormulaRunner {
    fn drop(&mut self) {
        self.cancel();
    }
}
