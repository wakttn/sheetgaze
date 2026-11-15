use std::path::{Path, PathBuf};

use crate::config::{Config, LazyMode};
use crate::domain::error::LoadError;
use crate::domain::filter::VisibilityMap;
use crate::domain::layout::{LayoutLimits, LayoutMode};
use crate::domain::row_store::LazyConfig;
use crate::domain::workbook::{FileFingerprint, SourceFormat, WorkbookModel};

use super::csv::load_csv;
use super::xlsx::load_xlsx_with_mode;

#[derive(Clone, Debug)]
pub struct LoadRequest {
    pub path: PathBuf,
    pub format: SourceFormat,
    pub layout: LayoutLimits,
    pub layout_mode: LayoutMode,
    pub lazy: LazyConfig,
    pub encoding: Option<String>,
    pub no_filter: bool,
}

impl LoadRequest {
    pub fn from_config(path: &Path, config: &Config) -> Result<Self, LoadError> {
        Ok(Self {
            path: path.to_path_buf(),
            format: detect_format(path)?,
            layout: config.layout,
            layout_mode: config.layout_mode,
            lazy: lazy_config(config),
            encoding: config.cli.encoding.clone(),
            no_filter: config.cli.no_filter,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WorkbookLoader {
    request: LoadRequest,
}

impl WorkbookLoader {
    pub fn new(request: LoadRequest) -> Self {
        Self { request }
    }

    pub fn from_config(path: &Path, config: &Config) -> Result<Self, LoadError> {
        Ok(Self::new(LoadRequest::from_config(path, config)?))
    }

    pub fn load(&self) -> Result<WorkbookModel, LoadError> {
        let mut workbook = match self.request.format {
            SourceFormat::Xlsx => load_xlsx_with_mode(
                &self.request.path,
                &self.request.layout,
                &self.request.lazy,
                self.request.layout_mode,
            )?,
            SourceFormat::Csv => load_csv(
                &self.request.path,
                &self.request.layout,
                &self.request.lazy,
                self.request.encoding.as_deref(),
            )?,
        };
        if self.request.no_filter {
            for sheet in &mut workbook.sheets {
                sheet.filter_state.enabled = false;
                sheet.filter_state.column_enabled.fill(false);
                sheet.visibility = VisibilityMap::all(sheet.meta.row_count);
            }
        }
        workbook.source.fingerprint = fingerprint(&self.request.path);
        Ok(workbook)
    }

    pub fn fingerprint(&self) -> FileFingerprint {
        fingerprint(&self.request.path)
    }

    pub fn path(&self) -> &Path {
        &self.request.path
    }

    pub fn format(&self) -> SourceFormat {
        self.request.format
    }
}

pub fn detect_format(path: &Path) -> Result<SourceFormat, LoadError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xlsx" | "xlsm" | "xlam" => Ok(SourceFormat::Xlsx),
        "csv" => Ok(SourceFormat::Csv),
        _ => Err(LoadError::UnsupportedFormat {
            path: path.to_path_buf(),
        }),
    }
}

pub fn lazy_config(config: &Config) -> LazyConfig {
    let mut lazy = config.lazy;
    match config.cli.lazy {
        LazyMode::Always => {
            lazy.threshold_cells = 0;
            lazy.threshold_bytes = 0;
        }
        LazyMode::Never => lazy.enabled = false,
        LazyMode::Auto => {}
    }
    lazy
}

pub fn fingerprint(path: &Path) -> FileFingerprint {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn detects_supported_formats() {
        assert_eq!(
            detect_format(&PathBuf::from("book.xlsx")).unwrap(),
            SourceFormat::Xlsx
        );
        assert_eq!(
            detect_format(&PathBuf::from("book.XLSM")).unwrap(),
            SourceFormat::Xlsx
        );
        assert_eq!(
            detect_format(&PathBuf::from("data.csv")).unwrap(),
            SourceFormat::Csv
        );
        assert!(detect_format(&PathBuf::from("data.txt")).is_err());
    }

    #[test]
    fn lazy_mode_overrides_thresholds() {
        let mut config = Config::default();
        config.cli.lazy = LazyMode::Always;
        let lazy = lazy_config(&config);
        assert_eq!(lazy.threshold_cells, 0);
        assert_eq!(lazy.threshold_bytes, 0);

        config.cli.lazy = LazyMode::Never;
        assert!(!lazy_config(&config).enabled);
    }

    #[test]
    fn missing_file_fingerprint_is_absent() {
        let stamp = fingerprint(Path::new("/definitely/not/a/real/file.xlsx"));
        assert!(!stamp.present);
        assert_eq!(stamp.len, 0);
    }

    #[test]
    fn load_request_preserves_layout_mode() {
        let config = Config {
            layout_mode: LayoutMode::Content,
            ..Config::default()
        };
        let request = LoadRequest::from_config(Path::new("book.xlsx"), &config).unwrap();
        assert_eq!(request.layout_mode, LayoutMode::Content);

        let config = Config::default();
        let request = LoadRequest::from_config(Path::new("book.xlsx"), &config).unwrap();
        assert_eq!(request.layout_mode, LayoutMode::Excel);
    }
}
