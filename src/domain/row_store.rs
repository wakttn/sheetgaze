use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::domain::addr::CellAddr;
use crate::domain::cell::Cell;
use crate::domain::error::LoadError;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub row: u32,
    pub cells: Vec<(u32, Cell)>,
}

impl Row {
    pub fn cell(&self, col: u32) -> Option<&Cell> {
        self.cells
            .iter()
            .find_map(|(cell_col, cell)| (*cell_col == col).then_some(cell))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowRange {
    pub start: u32,
    pub end: u32,
}

impl RowRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

pub struct MemoryRowSource {
    rows: Vec<Row>,
}

impl MemoryRowSource {
    pub fn new(rows: Vec<Row>) -> Self {
        Self { rows }
    }
}

impl RowSource for MemoryRowSource {
    fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }

    fn load_range(&mut self, range: RowRange) -> Result<Vec<Row>, LoadError> {
        Ok(self
            .rows
            .iter()
            .skip(range.start as usize)
            .take(range.len() as usize)
            .cloned()
            .collect())
    }

    fn scan_all(
        &mut self,
        visitor: &mut dyn FnMut(&Row) -> std::ops::ControlFlow<()>,
    ) -> Result<(), LoadError> {
        for row in &self.rows {
            if visitor(row).is_break() {
                break;
            }
        }
        Ok(())
    }
}

pub trait RowSource: Send {
    fn row_count(&self) -> u32;
    fn load_range(&mut self, range: RowRange) -> Result<Vec<Row>, LoadError>;
    fn scan_all(
        &mut self,
        visitor: &mut dyn FnMut(&Row) -> std::ops::ControlFlow<()>,
    ) -> Result<(), LoadError>;
}

#[derive(Clone, Copy, Debug)]
pub struct LazyConfig {
    pub enabled: bool,
    pub threshold_cells: u64,
    pub threshold_bytes: u64,
    pub row_cache_rows: usize,
    pub prefetch_rows: usize,
}

impl Default for LazyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_cells: 200_000,
            threshold_bytes: 16 * 1024 * 1024,
            row_cache_rows: 2_000,
            prefetch_rows: 200,
        }
    }
}

pub struct LazySheet {
    pub source: Box<dyn RowSource>,
    pub cache: RowWindowCache,
    pub row_count: u32,
}

impl LazySheet {
    pub fn new(source: Box<dyn RowSource>, row_cache_rows: usize, prefetch_rows: usize) -> Self {
        let row_count = source.row_count();
        Self {
            source,
            cache: RowWindowCache::new(row_cache_rows, prefetch_rows),
            row_count,
        }
    }

    pub fn ensure_rows(&mut self, range: RowRange) -> Result<(), LoadError> {
        self.cache.ensure(self.source.as_mut(), range)
    }

    pub fn cell(&self, addr: CellAddr) -> Option<&Cell> {
        self.cache.get(addr.row).and_then(|row| row.cell(addr.col))
    }

    pub fn row_loaded(&self, row: u32) -> bool {
        self.cache.get(row).is_some()
    }

    pub fn scan_all(
        &mut self,
        visitor: &mut dyn FnMut(&Row) -> std::ops::ControlFlow<()>,
    ) -> Result<(), LoadError> {
        self.source.scan_all(visitor)
    }
}

pub struct RowWindowCache {
    capacity_rows: usize,
    prefetch_rows: usize,
    rows: HashMap<u32, Arc<Row>>,
    lru: VecDeque<u32>,
}

impl RowWindowCache {
    pub fn new(capacity_rows: usize, prefetch_rows: usize) -> Self {
        Self {
            capacity_rows: capacity_rows.max(1),
            prefetch_rows,
            rows: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    pub fn ensure(&mut self, source: &mut dyn RowSource, range: RowRange) -> Result<(), LoadError> {
        let row_count = source.row_count();
        let start = range
            .start
            .saturating_sub(self.prefetch_rows as u32)
            .min(row_count);
        let end = range
            .end
            .saturating_add(self.prefetch_rows as u32)
            .min(row_count);
        if start >= end {
            return Ok(());
        }
        let missing: Vec<u32> = (start..end)
            .filter(|row| !self.rows.contains_key(row))
            .collect();
        if missing.is_empty() {
            self.touch_range(start..end);
            return Ok(());
        }
        let first = *missing.first().unwrap_or(&start);
        let last = *missing.last().unwrap_or(&end);
        let loaded = source.load_range(RowRange::new(first, last + 1))?;
        for row in loaded {
            let row_number = row.row;
            self.rows.insert(row_number, Arc::new(row));
            self.lru.push_back(row_number);
        }
        self.evict_to_capacity();
        self.touch_range(start..end);
        Ok(())
    }

    pub fn get(&self, row: u32) -> Option<&Row> {
        self.rows.get(&row).map(|row| row.as_ref())
    }

    pub fn resident_rows(&self) -> usize {
        self.rows.len()
    }

    fn touch_range(&mut self, range: std::ops::Range<u32>) {
        for row in range {
            if self.rows.contains_key(&row) {
                self.lru.retain(|value| *value != row);
                self.lru.push_back(row);
            }
        }
    }

    fn evict_to_capacity(&mut self) {
        while self.rows.len() > self.capacity_rows {
            if let Some(row) = self.lru.pop_front() {
                self.rows.remove(&row);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cell::CellValue;
    use crate::domain::style::CellStyle;

    struct MemorySource(Vec<Row>);

    impl RowSource for MemorySource {
        fn row_count(&self) -> u32 {
            self.0.len() as u32
        }

        fn load_range(&mut self, range: RowRange) -> Result<Vec<Row>, LoadError> {
            Ok(self.0[range.start as usize..range.end as usize].to_vec())
        }

        fn scan_all(
            &mut self,
            visitor: &mut dyn FnMut(&Row) -> std::ops::ControlFlow<()>,
        ) -> Result<(), LoadError> {
            for row in &self.0 {
                if visitor(row).is_break() {
                    break;
                }
            }
            Ok(())
        }
    }

    #[test]
    fn prefetch_is_clamped_to_row_count() {
        let rows = (0..3)
            .map(|row| Row {
                row,
                cells: vec![(
                    0,
                    Cell::new(CellValue::Number(row as f64), CellStyle::default()),
                )],
            })
            .collect();
        let mut cache = RowWindowCache::new(10, 8);
        let mut source = MemorySource(rows);
        cache.ensure(&mut source, RowRange::new(2, 3)).unwrap();
        assert!(cache.get(2).is_some());
    }

    #[test]
    fn cache_respects_capacity() {
        let rows = (0..10)
            .map(|row| Row {
                row,
                cells: vec![(
                    0,
                    Cell::new(CellValue::Number(row as f64), CellStyle::default()),
                )],
            })
            .collect();
        let mut cache = RowWindowCache::new(3, 0);
        let mut source = MemorySource(rows);
        cache.ensure(&mut source, RowRange::new(0, 1)).unwrap();
        cache.ensure(&mut source, RowRange::new(5, 6)).unwrap();
        assert!(cache.resident_rows() <= 3);
    }
}
