use crate::domain::addr::CellAddr;
use crate::domain::cell::{Cell, CellValue};
use crate::domain::formula::{FormulaInfo, FormulaStatus};
use crate::domain::sheet::{Sheet, SheetData};
use crate::domain::structure::{
    FormulaChange, RemovedCells, StructureEngine, StructureOp, StructureSnapshot,
};
use crate::domain::style::CellStyle;
use crate::domain::workbook::WorkbookModel;

const MAX_UNDO_TRANSACTIONS: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCursor {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommit {
    Stay,
    Down,
    Right,
    Left,
}

#[derive(Clone, Debug)]
pub struct EditChange {
    pub sheet: usize,
    pub addr: CellAddr,
    pub before: Option<Cell>,
    pub after: Option<Cell>,
}

impl EditChange {
    pub fn new(sheet: usize, addr: CellAddr, before: Option<Cell>, after: Option<Cell>) -> Self {
        Self {
            sheet,
            addr,
            before,
            after,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StructureEdit {
    pub sheet: usize,
    pub op: StructureOp,
    pub removed: RemovedCells,
    pub before: StructureSnapshot,
    pub after: StructureSnapshot,
    pub formula_before: Vec<FormulaChange>,
    pub formula_after: Vec<FormulaChange>,
}

#[derive(Clone, Debug)]
pub enum EditOp {
    Cell(Box<EditChange>),
    Structure(Box<StructureEdit>),
}

#[derive(Clone, Debug, Default)]
pub struct EditTransaction {
    pub ops: Vec<EditOp>,
}

impl EditTransaction {
    pub fn single(change: EditChange) -> Self {
        Self {
            ops: vec![EditOp::Cell(Box::new(change))],
        }
    }

    pub fn from_ops(ops: Vec<EditOp>) -> Self {
        Self { ops }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn cell_count(&self) -> usize {
        self.ops
            .iter()
            .filter(|op| matches!(op, EditOp::Cell(_)))
            .count()
    }

    pub fn structure_count(&self) -> usize {
        self.ops
            .iter()
            .filter(|op| matches!(op, EditOp::Structure(_)))
            .count()
    }

    pub fn summary(&self) -> String {
        let cells = self.cell_count();
        let structures = self.structure_count();
        match (cells, structures) {
            (0, 0) => "no changes".to_string(),
            (cells, 0) => format!("{cells} cells"),
            (0, structures) => format!("{structures} structure operations"),
            (cells, structures) => format!("{cells} cells, {structures} structure operations"),
        }
    }
}

/// Undo/redo history for in-memory edits. The workbook base data is never
/// modified; each change is applied to the per-sheet edit overlay.
#[derive(Default)]
pub struct EditLog {
    undo: Vec<EditTransaction>,
    redo: Vec<EditTransaction>,
    dirty: bool,
}

impl EditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn transaction_count(&self) -> usize {
        self.undo.len()
    }

    /// Records a transaction that has already been applied to the workbook.
    pub fn record(&mut self, transaction: EditTransaction) {
        if transaction.is_empty() {
            return;
        }
        self.undo.push(transaction);
        if self.undo.len() > MAX_UNDO_TRANSACTIONS {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self, workbook: &mut WorkbookModel) -> Option<EditTransaction> {
        let transaction = self.undo.pop()?;
        apply_changes(workbook, &transaction, false);
        self.redo.push(transaction.clone());
        self.dirty = true;
        Some(transaction)
    }

    pub fn redo(&mut self, workbook: &mut WorkbookModel) -> Option<EditTransaction> {
        let transaction = self.redo.pop()?;
        apply_changes(workbook, &transaction, true);
        self.undo.push(transaction.clone());
        self.dirty = true;
        Some(transaction)
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Called after a successful write. The overlay values stay in the
    /// workbook for display, but undo history is reset because it refers to
    /// the pre-save disk contents.
    pub fn reset_after_save(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
    }

    /// Discards history without touching the workbook. Callers must reload or
    /// discard the workbook when this is used.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
    }
}

pub fn apply_changes(workbook: &mut WorkbookModel, transaction: &EditTransaction, forward: bool) {
    let ops: Box<dyn Iterator<Item = &EditOp>> = if forward {
        Box::new(transaction.ops.iter())
    } else {
        Box::new(transaction.ops.iter().rev())
    };
    for op in ops {
        match op {
            EditOp::Cell(change) => {
                let target = if forward {
                    change.after.clone()
                } else {
                    change.before.clone()
                };
                if let Some(sheet) = workbook.sheets.get_mut(change.sheet) {
                    sheet.set_edit_cell(change.addr, target);
                }
            }
            EditOp::Structure(edit) => {
                if forward {
                    if let Some(sheet) = workbook.sheets.get_mut(edit.sheet) {
                        let _ = StructureEngine::apply(sheet, edit.op);
                        restore_snapshot(sheet, &edit.after);
                    }
                    apply_formula_changes(workbook, &edit.formula_after, true);
                } else {
                    if let Some(sheet) = workbook.sheets.get_mut(edit.sheet) {
                        StructureEngine::invert(sheet, edit.op, &edit.removed);
                        restore_snapshot(sheet, &edit.before);
                    }
                    apply_formula_changes(workbook, &edit.formula_before, false);
                }
            }
        }
    }
}

fn restore_snapshot(sheet: &mut Sheet, snapshot: &StructureSnapshot) {
    sheet.origin_rows = snapshot.origin_rows.clone();
    sheet.origin_cols = snapshot.origin_cols.clone();
    sheet.merges = crate::domain::merge::MergeIndex::new(snapshot.merges.clone());
    sheet.merge_borders = snapshot.merge_borders.clone();
    sheet.filters = snapshot.filters.clone();
    sheet.filter_state = snapshot.filter_state.clone();
    sheet.layout = snapshot.layout.clone();
    sheet.freeze = snapshot.freeze;
    sheet.conditional_formats = snapshot.conditional_formats.clone();
    sheet.used_range = snapshot.used_range;
    sheet.meta = snapshot.meta;
    sheet.data_rows = snapshot.data_rows;
    sheet.formula_cells = snapshot.formula_cells.clone();
    sheet.formula_overlay.values = snapshot.formula_overlay.iter().cloned().collect();
    sheet.formula_rewrites = snapshot.formula_rewrites.clone();
    sheet.structural_dirty = true;
    let _ = sheet.rebuild_visibility();
}

fn apply_formula_changes(workbook: &mut WorkbookModel, changes: &[FormulaChange], forward: bool) {
    for change in changes {
        let value = if forward {
            change.after.clone()
        } else {
            change.before.clone()
        };
        if let Some(sheet) = workbook.sheets.get_mut(change.sheet) {
            match value {
                Some(expression) => {
                    if let Some(cell) = sheet.edits.get_mut(&change.addr) {
                        set_formula_expression(cell, expression.clone());
                    } else if let SheetData::Eager(cells) = &mut sheet.data
                        && let Some(cell) = cells.get_mut(&change.addr)
                    {
                        set_formula_expression(cell, expression.clone());
                    }
                    sheet.formula_rewrites.insert(change.addr, expression);
                }
                None => {
                    if let Some(cell) = sheet.edits.get_mut(&change.addr) {
                        cell.formula = None;
                    } else if let SheetData::Eager(cells) = &mut sheet.data
                        && let Some(cell) = cells.get_mut(&change.addr)
                    {
                        cell.formula = None;
                    }
                    sheet.formula_rewrites.remove(&change.addr);
                }
            }
        }
    }
}

fn set_formula_expression(cell: &mut Cell, expression: String) {
    if let Some(formula) = cell.formula.as_mut() {
        formula.expression = expression;
        formula.status = FormulaStatus::Pending;
    } else {
        cell.formula = Some(FormulaInfo::new(expression, None));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputKind {
    Cell { sheet: usize, addr: CellAddr },
    Command,
}

#[derive(Clone, Debug)]
pub struct InputState {
    pub kind: InputKind,
    pub chars: Vec<char>,
    pub cursor: usize,
    pub original: String,
}

impl InputState {
    pub fn cell(sheet: usize, addr: CellAddr, original: String, cursor: EditCursor) -> Self {
        let chars: Vec<char> = original.chars().collect();
        let cursor = match cursor {
            EditCursor::Start => 0,
            EditCursor::End => chars.len(),
        };
        Self {
            kind: InputKind::Cell { sheet, addr },
            chars,
            cursor,
            original,
        }
    }

    pub fn command() -> Self {
        Self {
            kind: InputKind::Command,
            chars: Vec::new(),
            cursor: 0,
            original: String::new(),
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn set_text(&mut self, text: &str) {
        self.chars = text.chars().collect();
        self.cursor = self.chars.len();
    }

    pub fn insert(&mut self, ch: char) {
        self.chars.insert(self.cursor, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    pub fn delete_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    pub fn delete_to_start(&mut self) {
        while self.cursor > 0 {
            self.backspace();
        }
    }

    pub fn cursor_prefix(&self) -> String {
        self.chars[..self.cursor].iter().collect()
    }

    pub fn cursor_suffix(&self) -> String {
        self.chars[self.cursor..].iter().collect()
    }
}

/// The value shown when a cell is opened in the insert line.
pub fn input_text_for_cell(cell: &Cell) -> String {
    if let Some(formula) = &cell.formula {
        return formula.expression.clone();
    }
    match &cell.raw {
        CellValue::Empty => String::new(),
        CellValue::Text(value) => value.clone(),
        CellValue::Number(value) => format_number(*value),
        CellValue::Bool(value) => {
            if *value {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CellValue::DateTime(value) => value.clone(),
        CellValue::Error(value) => value.clone(),
        CellValue::ArraySummary { value, .. } => match value.as_ref() {
            CellValue::Empty => String::new(),
            CellValue::Text(value) => value.clone(),
            CellValue::Number(value) => format_number(*value),
            CellValue::Bool(value) => value.to_string(),
            CellValue::DateTime(value) => value.clone(),
            CellValue::Error(value) => value.clone(),
            CellValue::ArraySummary { value, .. } => format_number_of(value),
        },
    }
}

fn format_number_of(value: &CellValue) -> String {
    match value {
        CellValue::Number(value) => format_number(*value),
        _ => String::new(),
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let mut text = format!("{value:.15}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Converts user input into a cell, preserving the existing style and number
/// format. A leading `'` forces text; a leading `=` starts a formula.
pub fn parse_cell_input(text: &str, style: CellStyle, number_format: Option<String>) -> Cell {
    if text.is_empty() {
        return Cell::new_with_format(CellValue::Empty, style, number_format);
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return Cell::new_with_format(CellValue::Text(rest.to_string()), style, number_format);
    }
    if text.starts_with('=') {
        let mut cell = Cell::new_with_format(CellValue::Empty, style, number_format);
        let mut formula = FormulaInfo::new(text.to_string(), None);
        formula.status = FormulaStatus::Pending;
        cell.formula = Some(formula);
        return cell;
    }
    if text.eq_ignore_ascii_case("TRUE") {
        return Cell::new_with_format(CellValue::Bool(true), style, number_format);
    }
    if text.eq_ignore_ascii_case("FALSE") {
        return Cell::new_with_format(CellValue::Bool(false), style, number_format);
    }
    if let Ok(value) = text.parse::<f64>()
        && value.is_finite()
    {
        return Cell::new_with_format(CellValue::Number(value), style, number_format);
    }
    Cell::new_with_format(CellValue::Text(text.to_string()), style, number_format)
}

/// Applies a literal replacement to a display string. Returns the new string
/// and the number of occurrences replaced. Supports the usual `\n`, `\t`, `\\`
/// and `&` escapes in the replacement, and vim style `\c` / `\C` in the
/// pattern.
pub fn replace_literal(
    text: &str,
    pattern: &str,
    replacement: &str,
    global: bool,
    case_sensitive: bool,
) -> (String, usize) {
    if pattern.is_empty() {
        return (text.to_string(), 0);
    }
    let pattern = if let Some(rest) = pattern.strip_prefix("\\c") {
        (rest, false)
    } else if let Some(rest) = pattern.strip_prefix("\\C") {
        (rest, true)
    } else {
        (pattern, case_sensitive)
    };
    let (pattern, case_sensitive) = pattern;
    let replacement = unescape_replacement(replacement);
    let mut out = String::new();
    let mut count = 0;
    let haystack = text;
    let mut search_from = 0;
    while search_from <= haystack.len() {
        let found = if case_sensitive {
            haystack[search_from..].find(pattern)
        } else {
            find_case_insensitive(haystack, pattern, search_from)
        };
        let Some(offset) = found else {
            break;
        };
        let start = search_from + offset;
        let end = start + pattern.len();
        if !haystack.is_char_boundary(start) || !haystack.is_char_boundary(end) {
            break;
        }
        out.push_str(&haystack[search_from..start]);
        out.push_str(&replacement);
        count += 1;
        search_from = end;
        if !global {
            break;
        }
    }
    out.push_str(&haystack[search_from..]);
    (out, count)
}

fn find_case_insensitive(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let needle_lower = needle.to_lowercase();
    let needle_chars = needle.chars().count();
    for (offset, _) in haystack[from..].char_indices() {
        let candidate: String = haystack[from + offset..]
            .chars()
            .take(needle_chars)
            .collect();
        if candidate.to_lowercase() == needle_lower {
            return Some(offset);
        }
    }
    None
}

fn unescape_replacement(replacement: &str) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('&') => out.push('&'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Converts a replacement result back into a cell while keeping the original
/// style. Formula cells are never passed to this function.
pub fn cell_from_replacement(original: &Cell, text: &str) -> Cell {
    if let Some(rest) = text.strip_prefix('\'') {
        return Cell::new_with_format(
            CellValue::Text(rest.to_string()),
            original.style,
            original.number_format.clone(),
        );
    }
    if text.starts_with('=') {
        return parse_cell_input(text, original.style, original.number_format.clone());
    }
    match &original.raw {
        CellValue::Number(_) => {
            if let Ok(value) = text.parse::<f64>()
                && value.is_finite()
            {
                return Cell::new_with_format(
                    CellValue::Number(value),
                    original.style,
                    original.number_format.clone(),
                );
            }
        }
        CellValue::Bool(_) => {
            if text.eq_ignore_ascii_case("TRUE") {
                return Cell::new_with_format(
                    CellValue::Bool(true),
                    original.style,
                    original.number_format.clone(),
                );
            }
            if text.eq_ignore_ascii_case("FALSE") {
                return Cell::new_with_format(
                    CellValue::Bool(false),
                    original.style,
                    original.number_format.clone(),
                );
            }
        }
        _ => {}
    }
    Cell::new_with_format(
        CellValue::Text(text.to_string()),
        original.style,
        original.number_format.clone(),
    )
}

/// Captures the raw cell used for undo. Returns `None` when the cell has no
/// stored value and can be revealed by removing the overlay.
pub fn snapshot_cell(sheet: &Sheet, addr: CellAddr) -> Option<Cell> {
    sheet.raw_cell(addr).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formula_text_and_numbers() {
        let style = CellStyle::default();
        let formula = parse_cell_input("=SUM(A1:A2)", style, None);
        assert_eq!(formula.formula.unwrap().expression, "=SUM(A1:A2)");

        let text = parse_cell_input("'=not a formula", style, None);
        assert_eq!(text.raw, CellValue::Text("=not a formula".to_string()));

        let number = parse_cell_input("12.5", style, None);
        assert_eq!(number.raw, CellValue::Number(12.5));

        let boolean = parse_cell_input("true", style, None);
        assert_eq!(boolean.raw, CellValue::Bool(true));

        let plain = parse_cell_input("hello", style, None);
        assert_eq!(plain.raw, CellValue::Text("hello".to_string()));
    }

    #[test]
    fn empty_input_clears_value_but_keeps_format() {
        let cell = parse_cell_input("", CellStyle::default(), Some("0.00".to_string()));
        assert_eq!(cell.raw, CellValue::Empty);
        assert_eq!(cell.number_format.as_deref(), Some("0.00"));
    }

    #[test]
    fn replaces_literally_with_case_folding() {
        let (text, count) = replace_literal("East east EAST", "east", "West", true, false);
        assert_eq!(text, "West West West");
        assert_eq!(count, 3);

        let (text, count) = replace_literal("aaaa", "aa", "b", false, true);
        assert_eq!(text, "baa");
        assert_eq!(count, 1);
    }

    #[test]
    fn replacement_supports_escapes() {
        let (text, count) = replace_literal("a\nb", "a", "x", true, true);
        assert_eq!(text, "x\nb");
        assert_eq!(count, 1);

        let (text, _) = replace_literal("a", "a", "\\n", true, true);
        assert_eq!(text, "\n");
    }
}
