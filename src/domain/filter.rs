use std::collections::HashSet;

use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, EffectiveCell};
use crate::domain::error::FilterError;
use crate::domain::formula::{FormulaOverlay, FormulaStatus};
use crate::domain::merge::MergeIndex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterSource {
    Worksheet,
    Table { name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilterColumnDef {
    pub col_id: u32,
    pub criterion: FilterCriterion,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterCriterion {
    ValueList {
        values: Vec<String>,
        include_blank: bool,
    },
    Custom {
        and: bool,
        conditions: Vec<CustomCondition>,
    },
    Top10 {
        top: bool,
        percent: bool,
        value: f64,
    },
    Unsupported {
        kind: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomCondition {
    pub operator: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct FilterDefinition {
    pub source: FilterSource,
    pub range: CellRange,
    pub header_row: u32,
    pub data_range: CellRange,
    pub columns: Vec<FilterColumnDef>,
}

impl FilterDefinition {
    pub fn has_criteria(&self) -> bool {
        self.columns
            .iter()
            .any(|column| !matches!(column.criterion, FilterCriterion::Unsupported { .. }))
    }

    pub fn validate(&self, used: CellRange, merges: &MergeIndex) -> Result<(), FilterError> {
        if !used.contains(self.range.start) || !used.contains(self.range.end) {
            return Err(FilterError::InconsistentRange {
                sheet: String::new(),
                message: "filter range is outside the used range".to_string(),
            });
        }
        for region in merges.regions() {
            if region.range.intersects(self.range) {
                return Err(FilterError::InconsistentRange {
                    sheet: String::new(),
                    message: "filter range intersects a merged cell".to_string(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct FilterState {
    pub enabled: bool,
    pub column_enabled: Vec<bool>,
}

impl FilterState {
    pub fn from_definitions(defs: &[FilterDefinition], no_filter: bool) -> Self {
        let enabled = !no_filter && defs.iter().any(FilterDefinition::has_criteria);
        let columns = defs
            .iter()
            .flat_map(|def| def.columns.iter().map(|_| !no_filter))
            .collect();
        Self {
            enabled,
            column_enabled: columns,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FilterMasks {
    pub row_count: u32,
    pub columns: Vec<Vec<bool>>,
    pub warnings: Vec<FilterWarning>,
}

#[derive(Clone, Debug, Default)]
pub struct FilterValue {
    pub display: String,
    pub numeric: Option<f64>,
}

impl FilterValue {
    pub fn from_effective(cell: &EffectiveCell) -> Self {
        Self {
            display: cell.display.clone(),
            numeric: cell.value.numeric(),
        }
    }

    pub fn from_cell(addr: CellAddr, cell: &Cell, overlay: &FormulaOverlay) -> Self {
        if let Some(result) = overlay.get(addr) {
            return Self {
                display: result
                    .value
                    .display_text_with_format(cell.number_format.as_deref()),
                numeric: result.value.numeric(),
            };
        }
        let status = cell
            .formula
            .as_ref()
            .map(|formula| formula.status.clone())
            .unwrap_or(FormulaStatus::NotRequested);
        Self::from_effective(&cell.effective(status))
    }
}

struct BuilderColumn {
    actual_col: u32,
    header_row: u32,
    data_range: CellRange,
    criterion: FilterCriterion,
    mask: Vec<bool>,
    top10_values: Option<Vec<(u32, f64)>>,
}

pub struct FilterMaskBuilder {
    row_count: u32,
    columns: Vec<BuilderColumn>,
    warnings: Vec<FilterWarning>,
}

impl FilterMaskBuilder {
    pub fn new(defs: &[FilterDefinition], row_count: u32) -> Self {
        let mut columns = Vec::new();
        let mut warnings = Vec::new();
        for def in defs {
            for column in &def.columns {
                if let FilterCriterion::Unsupported { kind } = &column.criterion {
                    warnings.push(FilterWarning::Unsupported {
                        column: column.col_id,
                        kind: kind.clone(),
                    });
                }
                let top10_values =
                    matches!(column.criterion, FilterCriterion::Top10 { .. }).then(Vec::new);
                columns.push(BuilderColumn {
                    actual_col: def.range.start.col + column.col_id,
                    header_row: def.header_row,
                    data_range: def.data_range,
                    criterion: column.criterion.clone(),
                    mask: vec![true; row_count as usize],
                    top10_values,
                });
            }
        }
        Self {
            row_count,
            columns,
            warnings,
        }
    }

    pub fn observe_row(&mut self, row: u32, values: &[FilterValue]) {
        for (column, value) in self.columns.iter_mut().zip(values) {
            if row == column.header_row
                || !column
                    .data_range
                    .contains(CellAddr::new(row, column.actual_col))
            {
                continue;
            }
            match &column.criterion {
                FilterCriterion::Unsupported { .. } => {}
                FilterCriterion::Top10 { .. } => {
                    if let Some(numeric) = value.numeric
                        && let Some(values) = column.top10_values.as_mut()
                    {
                        values.push((row, numeric));
                    }
                }
                criterion => {
                    if !criterion_value_matches(criterion, Some(value))
                        && let Some(slot) = column.mask.get_mut(row as usize)
                    {
                        *slot = false;
                    }
                }
            }
        }
    }

    pub fn finish(mut self) -> FilterMasks {
        for column in &mut self.columns {
            let FilterCriterion::Top10 {
                top,
                percent,
                value,
            } = &column.criterion
            else {
                continue;
            };
            let Some(mut values) = column.top10_values.take() else {
                continue;
            };
            if values.is_empty() {
                continue;
            }
            values.sort_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let count = if *percent {
                ((values.len() as f64 * *value / 100.0).ceil() as usize).max(1)
            } else {
                (*value).max(1.0) as usize
            };
            let keep = if *top {
                values
                    .iter()
                    .rev()
                    .take(count)
                    .map(|(row, _)| *row)
                    .collect::<HashSet<_>>()
            } else {
                values
                    .iter()
                    .take(count)
                    .map(|(row, _)| *row)
                    .collect::<HashSet<_>>()
            };
            for row in column.data_range.rows() {
                if let Some(slot) = column.mask.get_mut(row as usize) {
                    *slot = keep.contains(&row);
                }
            }
        }
        let columns = self.columns.into_iter().map(|column| column.mask).collect();
        FilterMasks {
            row_count: self.row_count,
            columns,
            warnings: self.warnings,
        }
    }
}

pub fn filter_column_indices(defs: &[FilterDefinition]) -> Vec<u32> {
    defs.iter()
        .flat_map(|def| {
            def.columns
                .iter()
                .map(move |column| def.range.start.col + column.col_id)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterWarning {
    Unsupported { column: u32, kind: String },
    InconsistentRange { sheet: String, message: String },
}

#[derive(Clone, Debug)]
pub struct VisibilityMap {
    visible: Vec<bool>,
    visible_rows: Vec<u32>,
}

impl VisibilityMap {
    pub fn all(row_count: u32) -> Self {
        Self {
            visible: vec![true; row_count as usize],
            visible_rows: (0..row_count).collect(),
        }
    }

    pub fn from_mask(mask: Vec<bool>) -> Self {
        let visible_rows = mask
            .iter()
            .enumerate()
            .filter_map(|(row, visible)| (*visible).then_some(row as u32))
            .collect();
        Self {
            visible: mask,
            visible_rows,
        }
    }

    pub fn is_visible(&self, row: u32) -> bool {
        self.visible.get(row as usize).copied().unwrap_or(false)
    }

    pub fn visible_rows(&self) -> &[u32] {
        &self.visible_rows
    }

    pub fn visible_count(&self) -> u32 {
        self.visible_rows.len() as u32
    }

    pub fn visible_row_at(&self, index: usize) -> Option<u32> {
        self.visible_rows.get(index).copied()
    }

    pub fn force_visible(&mut self, start: u32, end: u32) {
        let end = end.min(self.visible.len() as u32);
        for row in start..end {
            if let Some(slot) = self.visible.get_mut(row as usize) {
                *slot = true;
            }
        }
        self.visible_rows = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(row, visible)| (*visible).then_some(row as u32))
            .collect();
    }
}

pub struct FilterEngine;

impl FilterEngine {
    pub fn combine(
        defs: &[FilterDefinition],
        masks: &FilterMasks,
        state: &FilterState,
        row_count: u32,
    ) -> VisibilityMap {
        if !state.enabled || defs.is_empty() {
            return VisibilityMap::all(row_count);
        }
        let mut visible = vec![true; row_count as usize];
        let mut column_index = 0usize;
        for def in defs {
            for _ in &def.columns {
                let enabled = state
                    .column_enabled
                    .get(column_index)
                    .copied()
                    .unwrap_or(true);
                let mask_index = column_index;
                column_index += 1;
                if !enabled {
                    continue;
                }
                let Some(mask) = masks.columns.get(mask_index) else {
                    continue;
                };
                for (row, matched) in mask.iter().enumerate().take(row_count as usize) {
                    if !*matched {
                        visible[row] = false;
                    }
                }
            }
        }
        VisibilityMap::from_mask(visible)
    }
}

fn criterion_value_matches(criterion: &FilterCriterion, value: Option<&FilterValue>) -> bool {
    let (text, numeric) = value
        .map(|value| (value.display.clone(), value.numeric))
        .unwrap_or_default();
    match criterion {
        FilterCriterion::ValueList {
            values,
            include_blank,
        } => {
            if text.is_empty() && *include_blank {
                return true;
            }
            values.iter().any(|value| value.eq_ignore_ascii_case(&text))
        }
        FilterCriterion::Custom { and, conditions } => {
            let results = conditions
                .iter()
                .map(|condition| custom_matches(condition, &text, numeric));
            if *and {
                results.into_iter().all(|matched| matched)
            } else {
                results.into_iter().any(|matched| matched)
            }
        }
        FilterCriterion::Top10 { .. } => true,
        FilterCriterion::Unsupported { .. } => true,
    }
}

fn custom_matches(condition: &CustomCondition, text: &str, numeric: Option<f64>) -> bool {
    let target = condition.value.trim();
    let target_number = target.parse::<f64>().ok();
    let cmp_text = text.to_lowercase();
    let target_text = target.to_lowercase();
    let wildcard = target_text.contains('*') || target_text.contains('?');
    let text_equal = if wildcard {
        wildcard_match(&cmp_text, &target_text)
    } else {
        cmp_text == target_text
    };
    let numeric_cmp = numeric.zip(target_number);
    match condition.operator.as_str() {
        "equal" => {
            if let Some((value, target)) = numeric_cmp {
                (value - target).abs() < f64::EPSILON
            } else {
                text_equal
            }
        }
        "notEqual" => {
            if let Some((value, target)) = numeric_cmp {
                (value - target).abs() >= f64::EPSILON
            } else {
                !text_equal
            }
        }
        "lessThan" => numeric_cmp.is_some_and(|(value, target)| value < target),
        "lessThanOrEqual" => numeric_cmp.is_some_and(|(value, target)| value <= target),
        "greaterThan" => numeric_cmp.is_some_and(|(value, target)| value > target),
        "greaterThanOrEqual" => numeric_cmp.is_some_and(|(value, target)| value >= target),
        _ => true,
    }
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            ti += 1;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::addr::CellAddr;

    #[test]
    fn wildcard() {
        assert!(wildcard_match("hello world", "hello*"));
        assert!(wildcard_match("hello world", "*world"));
        assert!(wildcard_match("hello world", "h?llo*"));
        assert!(!wildcard_match("hello", "world*"));
    }

    #[test]
    fn top10_keeps_highest_values() {
        let def = FilterDefinition {
            source: FilterSource::Worksheet,
            range: CellRange::new(CellAddr::new(0, 0), CellAddr::new(5, 1)),
            header_row: 0,
            data_range: CellRange::new(CellAddr::new(1, 0), CellAddr::new(5, 1)),
            columns: vec![FilterColumnDef {
                col_id: 1,
                criterion: FilterCriterion::Top10 {
                    top: true,
                    percent: false,
                    value: 2.0,
                },
            }],
        };
        let state = FilterState {
            enabled: true,
            column_enabled: vec![true],
        };
        let mut builder = FilterMaskBuilder::new(std::slice::from_ref(&def), 6);
        for row in 0..6 {
            let value = if row == 0 {
                FilterValue::default()
            } else {
                FilterValue {
                    display: format!("{row}"),
                    numeric: Some(row as f64),
                }
            };
            builder.observe_row(row, &[value]);
        }
        let masks = builder.finish();
        let visibility = FilterEngine::combine(std::slice::from_ref(&def), &masks, &state, 6);
        assert!(!visibility.is_visible(1));
        assert!(!visibility.is_visible(3));
        assert!(visibility.is_visible(4));
        assert!(visibility.is_visible(5));
    }

    #[test]
    fn visibility_all() {
        let map = VisibilityMap::all(5);
        assert_eq!(map.visible_count(), 5);
        assert!(map.is_visible(4));
    }
}
