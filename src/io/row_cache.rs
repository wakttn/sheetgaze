use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use tempfile::NamedTempFile;

use crate::domain::error::LoadError;
use crate::domain::row_store::{Row, RowRange, RowSource};

pub struct SpillRowCache {
    file: NamedTempFile,
    offsets: Vec<u64>,
}

impl SpillRowCache {
    pub fn new() -> Result<Self, LoadError> {
        let file = NamedTempFile::new().map_err(|error| LoadError::RowCache {
            path: PathBuf::from("<temp>"),
            message: error.to_string(),
        })?;
        Ok(Self {
            file,
            offsets: Vec::new(),
        })
    }

    pub fn write_row(&mut self, row: &Row) -> Result<(), LoadError> {
        let bytes = postcard::to_allocvec(row).map_err(|error| LoadError::RowCache {
            path: PathBuf::from("<temp>"),
            message: error.to_string(),
        })?;
        let offset =
            self.file
                .as_file_mut()
                .stream_position()
                .map_err(|error| LoadError::RowCache {
                    path: PathBuf::from("<temp>"),
                    message: error.to_string(),
                })?;
        self.offsets.push(offset);
        let len = bytes.len() as u32;
        self.file
            .as_file_mut()
            .write_all(&len.to_le_bytes())
            .and_then(|_| self.file.as_file_mut().write_all(&bytes))
            .map_err(|error| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: error.to_string(),
            })?;
        Ok(())
    }

    pub fn row_count(&self) -> u32 {
        self.offsets.len() as u32
    }

    pub fn finish(self) -> Result<SpillRowSource, LoadError> {
        self.file
            .as_file()
            .sync_all()
            .map_err(|error| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: error.to_string(),
            })?;
        Ok(SpillRowSource {
            file: self.file,
            offsets: self.offsets,
        })
    }
}

pub struct SpillRowSource {
    file: NamedTempFile,
    offsets: Vec<u64>,
}

impl SpillRowSource {
    fn read_row(&mut self, index: u32) -> Result<Row, LoadError> {
        let offset = *self
            .offsets
            .get(index as usize)
            .ok_or_else(|| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: format!("row {index} is out of range"),
            })?;
        self.file
            .as_file_mut()
            .seek(SeekFrom::Start(offset))
            .map_err(|error| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: error.to_string(),
            })?;
        let mut len = [0u8; 4];
        self.file
            .as_file_mut()
            .read_exact(&mut len)
            .map_err(|error| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: error.to_string(),
            })?;
        let mut bytes = vec![0u8; u32::from_le_bytes(len) as usize];
        self.file
            .as_file_mut()
            .read_exact(&mut bytes)
            .map_err(|error| LoadError::RowCache {
                path: PathBuf::from("<temp>"),
                message: error.to_string(),
            })?;
        postcard::from_bytes(&bytes).map_err(|error| LoadError::RowCache {
            path: PathBuf::from("<temp>"),
            message: error.to_string(),
        })
    }
}

impl RowSource for SpillRowSource {
    fn row_count(&self) -> u32 {
        self.offsets.len() as u32
    }

    fn load_range(&mut self, range: RowRange) -> Result<Vec<Row>, LoadError> {
        let mut rows = Vec::with_capacity(range.len() as usize);
        for row in range.start..range.end {
            rows.push(self.read_row(row)?);
        }
        Ok(rows)
    }

    fn scan_all(
        &mut self,
        visitor: &mut dyn FnMut(&Row) -> std::ops::ControlFlow<()>,
    ) -> Result<(), LoadError> {
        for row in 0..self.row_count() {
            let loaded = self.read_row(row)?;
            if visitor(&loaded).is_break() {
                break;
            }
        }
        Ok(())
    }
}
