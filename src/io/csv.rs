use std::collections::HashMap;
use std::path::Path;

use crate::config::LazyMode;
use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, CellValue};
use crate::domain::error::LoadError;
use crate::domain::filter::{FilterMasks, VisibilityMap};
use crate::domain::formula::FormulaOverlay;
use crate::domain::freeze::FreezePane;
use crate::domain::layout::{LayoutLimits, LayoutStatsBuilder};
use crate::domain::merge::MergeIndex;
use crate::domain::row_store::{LazyConfig, LazySheet, Row, RowRange, RowSource};
use crate::domain::sheet::{DataRowsBuilder, Sheet, SheetData, SheetMeta};
use crate::domain::style::CellStyle;
use crate::domain::workbook::{FileFingerprint, SourceFormat, SourceInfo, WorkbookModel};

use super::encoding::read_text;
use super::row_cache::{SpillRowCache, SpillRowSource};

pub fn load_csv(
    path: &Path,
    layout_limits: &LayoutLimits,
    lazy_config: &LazyConfig,
    encoding_override: Option<&str>,
) -> Result<WorkbookModel, LoadError> {
    let (text, encoding) = read_text(path, encoding_override).map_err(|error| LoadError::Csv {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let file_bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut cache = SpillRowCache::new()?;
    let mut eager = HashMap::new();
    let mut stats = LayoutStatsBuilder::new(layout_limits);
    let mut max_row = 0u32;
    let mut max_col = 0u32;
    let mut total_cells = 0u64;
    let mut row_count = 0u32;
    let mut data_rows = DataRowsBuilder::default();
    let mut collect_eager = !lazy_config.enabled || file_bytes <= lazy_config.threshold_bytes;
    for record in reader.records() {
        let record = record.map_err(|error| LoadError::Csv {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let mut cells = Vec::new();
        for (col, field) in record.iter().enumerate() {
            let col = col as u32;
            let cell = Cell::new(CellValue::Text(field.to_string()), CellStyle::default());
            stats.observe(row_count, col, &cell.display, false);
            cells.push((col, cell.clone()));
            if collect_eager {
                eager.insert(CellAddr::new(row_count, col), cell);
            }
            max_col = max_col.max(col);
        }
        data_rows.observe_row(row_count, cells.iter().any(|(_, cell)| cell.has_content()));
        max_row = max_row.max(row_count);
        total_cells += cells.len() as u64;
        if lazy_config.enabled && collect_eager && total_cells > lazy_config.threshold_cells {
            collect_eager = false;
            eager.clear();
        }
        cache.write_row(&Row {
            row: row_count,
            cells,
        })?;
        row_count += 1;
    }
    let source = cache.finish()?;
    let row_count = row_count.max(1);
    let column_count = max_col + 1;
    let used_range = CellRange::new(CellAddr::new(0, 0), CellAddr::new(max_row, max_col));
    let layout = stats.finish(&MergeIndex::empty(), None, used_range);
    let lazy = lazy_config.enabled
        && (file_bytes > lazy_config.threshold_bytes || total_cells > lazy_config.threshold_cells);
    let data = if lazy {
        SheetData::Lazy(LazySheet::new(
            Box::new(source) as Box<dyn RowSource>,
            lazy_config.row_cache_rows,
            lazy_config.prefetch_rows,
        ))
    } else {
        SheetData::Eager(eager)
    };
    let sheet = Sheet {
        name: path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("CSV")
            .to_string(),
        meta: SheetMeta {
            row_count,
            column_count,
        },
        used_range,
        data_rows: data_rows.finish(),
        data,
        edits: HashMap::new(),
        merges: MergeIndex::empty(),
        merge_borders: HashMap::new(),
        layout,
        layout_limits: *layout_limits,
        freeze: FreezePane::none(),
        conditional_formats: Vec::new(),
        filters: Vec::new(),
        filter_masks: FilterMasks::default(),
        filter_masks_dirty: false,
        filter_state: Default::default(),
        visibility: VisibilityMap::all(row_count),
        formula_overlay: FormulaOverlay::default(),
        formula_cells: Vec::new(),
        warnings: if encoding.had_errors {
            vec![format!(
                "encoding {} had invalid byte sequences; replacement characters were used",
                encoding.label()
            )]
        } else {
            Vec::new()
        },
    };
    WorkbookModel::new(
        SourceInfo {
            path: path.to_path_buf(),
            format: SourceFormat::Csv,
            fingerprint: fingerprint(path),
            encoding: Some(encoding.label().to_string()),
            lazy,
        },
        vec![sheet],
    )
}

pub fn lazy_mode_from_cli(mode: LazyMode, enabled: bool) -> LazyConfig {
    LazyConfig {
        enabled: enabled && !matches!(mode, LazyMode::Never),
        ..LazyConfig::default()
    }
}

fn fingerprint(path: &Path) -> FileFingerprint {
    match std::fs::metadata(path) {
        Ok(meta) => FileFingerprint {
            len: meta.len(),
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or(0),
            present: true,
        },
        Err(_) => FileFingerprint::default(),
    }
}

#[allow(dead_code)]
fn _row_range(row: u32) -> RowRange {
    RowRange::new(row, row + 1)
}

#[allow(dead_code)]
fn _source_row_count(source: &SpillRowSource) -> u32 {
    source.row_count()
}
