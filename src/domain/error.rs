use std::path::PathBuf;

use thiserror::Error;

use crate::domain::formula::FormulaStatus;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("cannot read {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("unsupported file format: {path}")]
    UnsupportedFormat { path: PathBuf },
    #[error("xlsx error in {path}: {message}")]
    Xlsx { path: PathBuf, message: String },
    #[error("csv error in {path}: {message}")]
    Csv { path: PathBuf, message: String },
    #[error("invalid text encoding in {path}")]
    InvalidUtf8 { path: PathBuf },
    #[error("workbook is empty: {path}")]
    EmptyWorkbook { path: PathBuf },
    #[error("row cache error in {path}: {message}")]
    RowCache { path: PathBuf, message: String },
    #[error("sheet error in {sheet}: {message}")]
    SheetIndex { sheet: String, message: String },
}

#[derive(Debug, Error)]
pub enum EncodingError {
    #[error("unknown encoding label: {0}")]
    UnknownLabel(String),
    #[error("could not decode {label} at byte {offset}")]
    DecodeFailed { label: String, offset: u64 },
    #[error("encoding io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("metadata parse error in {sheet}: {message}")]
    Parse { sheet: String, message: String },
    #[error("metadata io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("metadata zip error: {0}")]
    Zip(String),
    #[error("metadata xml error: {0}")]
    Xml(String),
}

#[derive(Debug, Error, Clone)]
pub enum CopyError {
    #[error("copy range is too large: {cells} cells (limit {limit})")]
    TooLarge { cells: usize, limit: usize },
    #[error("copy source error in {sheet}: {message}")]
    Source { sheet: String, message: String },
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("no clipboard sink is available")]
    Unavailable,
    #[error("clipboard write failed: {0}")]
    Write(String),
    #[error("clipboard read is not available in this environment")]
    ReadUnavailable,
    #[error("OSC 52 payload is too large: {bytes} bytes (limit {limit})")]
    Osc52PayloadTooLarge { bytes: usize, limit: usize },
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search failed: {0}")]
    Failed(String),
    #[error("search was cancelled")]
    Cancelled,
}

#[derive(Debug, Error, Clone)]
pub enum FilterError {
    #[error("filter parse error in {sheet}: {message}")]
    Parse { sheet: String, message: String },
    #[error("unsupported filter condition in column {column}: {kind}")]
    Unsupported { column: u32, kind: String },
    #[error("inconsistent filter range in {sheet}: {message}")]
    InconsistentRange { sheet: String, message: String },
}

#[derive(Debug, Error)]
pub enum FormulaError {
    #[error("formula engine error: {0}")]
    Engine(String),
    #[error("formula job was cancelled")]
    Cancelled,
    #[error("formula cell {addr} has status {status:?}")]
    Cell { addr: String, status: FormulaStatus },
}

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("editing is only supported for xlsx files")]
    UnsupportedFormat,
    #[error("no changes to save")]
    NoChanges,
    #[error("cannot write read-only file: {path}")]
    ReadOnly { path: PathBuf },
    #[error("cannot write {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("xlsx zip error: {0}")]
    Zip(String),
    #[error("xlsx xml error: {0}")]
    Xml(String),
    #[error("unsupported formula edit in {sheet}!{addr}: {message}")]
    UnsupportedFormula {
        sheet: String,
        addr: String,
        message: String,
    },
    #[error("unsupported structure in {sheet}: {message}")]
    UnsupportedStructure { sheet: String, message: String },
    #[error("file changed on disk; use :w! to overwrite")]
    Stale,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("{0}")]
    Load(#[from] LoadError),
    #[error("{0}")]
    Formula(#[from] FormulaError),
    #[error("{0}")]
    Clipboard(#[from] ClipboardError),
    #[error("{0}")]
    Search(#[from] SearchError),
    #[error("{0}")]
    Filter(#[from] FilterError),
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("application error: {0}")]
    Other(String),
}
