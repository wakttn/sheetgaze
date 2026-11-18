use std::collections::BTreeMap;

use formualizer::parse::parser::{ASTNode, ASTNodeType, Parser, ReferenceType};

use crate::domain::structure::{Axis, OriginMap, StructureOp, transform_axis_range};

#[derive(Clone, Debug, Default)]
pub struct FormulaContext {
    /// Current sheet name to the origin map for that sheet.
    pub transforms: BTreeMap<String, OriginMap>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteStatus {
    Unchanged,
    Rewritten(String),
    Unsupported(String),
}

pub struct FormulaRefEngine;

impl FormulaRefEngine {
    /// Rewrites formulas in `source_sheet` after a structural operation on
    /// `target_sheet`. Unqualified references are shifted only when the
    /// formula lives on the target sheet; qualified references are shifted
    /// when they point at the target sheet.
    pub fn rewrite_for_op(
        expression: &str,
        source_sheet: &str,
        target_sheet: &str,
        op: StructureOp,
    ) -> RewriteStatus {
        rewrite(expression, |reference| {
            let apply = match &reference {
                ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => {
                    match sheet.as_deref() {
                        Some(name) => name == target_sheet,
                        None => source_sheet == target_sheet,
                    }
                }
                _ => false,
            };
            if !apply {
                return Ok(None);
            }
            rewrite_reference_for_op(reference, op)
        })
    }

    /// Rewrites sheet-qualified references to structurally changed sheets at
    /// save time using the current-to-original origin map.
    pub fn rewrite_for_origin(expression: &str, context: &FormulaContext) -> RewriteStatus {
        rewrite(expression, |reference| {
            let (sheet, _) = match &reference {
                ReferenceType::Cell { sheet, .. } | ReferenceType::Range { sheet, .. } => {
                    (sheet.as_deref(), ())
                }
                _ => return Ok(None),
            };
            let Some(name) = sheet else {
                return Ok(None);
            };
            let Some(origin) = context.transforms.get(name) else {
                return Ok(None);
            };
            rewrite_reference_for_origin(reference, origin)
        })
    }

    /// Rewrites defined-name formulas. Only sheet-qualified references are
    /// changed; unqualified references are left untouched.
    pub fn rewrite_defined_name(expression: &str, context: &FormulaContext) -> RewriteStatus {
        Self::rewrite_for_origin(expression, context)
    }
}

fn rewrite<F>(expression: &str, mut mapper: F) -> RewriteStatus
where
    F: FnMut(ReferenceType) -> Result<Option<ReferenceType>, String>,
{
    let mut parser = match Parser::new(expression) {
        Ok(parser) => parser,
        Err(error) => return RewriteStatus::Unsupported(error.to_string()),
    };
    let ast = match parser.parse() {
        Ok(ast) => ast,
        Err(error) => return RewriteStatus::Unsupported(error.to_string()),
    };
    let mut references = Vec::new();
    collect_references(&ast, &mut references);
    let mut replacements = Vec::new();
    for (start, end, reference) in references {
        match mapper(reference) {
            Ok(Some(updated)) => {
                if start >= end || end > expression.len() {
                    return RewriteStatus::Unsupported(
                        "reference span is outside the formula".to_string(),
                    );
                }
                replacements.push((start, end, updated.to_excel_string()));
            }
            Ok(None) => {}
            Err(message) => return RewriteStatus::Unsupported(message),
        }
    }
    if replacements.is_empty() {
        return RewriteStatus::Unchanged;
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    let mut output = expression.to_string();
    for (start, end, text) in replacements.into_iter().rev() {
        output.replace_range(start..end, &text);
    }
    RewriteStatus::Rewritten(output)
}

fn collect_references(node: &ASTNode, out: &mut Vec<(usize, usize, ReferenceType)>) {
    match &node.node_type {
        ASTNodeType::Reference { reference, .. } => {
            if let Some(token) = &node.source_token {
                out.push((token.start, token.end, reference.clone()));
            }
        }
        ASTNodeType::UnaryOp { expr, .. } => collect_references(expr, out),
        ASTNodeType::BinaryOp { left, right, .. } => {
            collect_references(left, out);
            collect_references(right, out);
        }
        ASTNodeType::Function { args, .. } => {
            for argument in args {
                collect_references(argument, out);
            }
        }
        ASTNodeType::Call { callee, args } => {
            collect_references(callee, out);
            for argument in args {
                collect_references(argument, out);
            }
        }
        ASTNodeType::Array(rows) => {
            for row in rows {
                for item in row {
                    collect_references(item, out);
                }
            }
        }
        ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
    }
}

fn rewrite_reference_for_op(
    reference: ReferenceType,
    op: StructureOp,
) -> Result<Option<ReferenceType>, String> {
    match reference {
        ReferenceType::Cell {
            sheet,
            row,
            col,
            row_abs,
            col_abs,
        } => {
            let row0 = row.checked_sub(1).ok_or("invalid row")?;
            let col0 = col.checked_sub(1).ok_or("invalid column")?;
            let (row0, col0) = transform_cell(row0, col0, op)?;
            Ok(Some(ReferenceType::Cell {
                sheet,
                row: row0 + 1,
                col: col0 + 1,
                row_abs,
                col_abs,
            }))
        }
        ReferenceType::Range {
            sheet,
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
        } => {
            let (start_row, end_row) =
                transform_optional_axis(start_row, end_row, op.axis == Axis::Row, op)?;
            let (start_col, end_col) =
                transform_optional_axis(start_col, end_col, op.axis == Axis::Column, op)?;
            Ok(Some(ReferenceType::Range {
                sheet,
                start_row,
                start_col,
                end_row,
                end_col,
                start_row_abs,
                start_col_abs,
                end_row_abs,
                end_col_abs,
            }))
        }
        ReferenceType::Cell3D { .. } | ReferenceType::Range3D { .. } => {
            Err("3D references cannot be shifted safely".to_string())
        }
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            Ok(None)
        }
    }
}

fn transform_cell(row: u32, col: u32, op: StructureOp) -> Result<(u32, u32), String> {
    match op.axis {
        Axis::Row => transform_axis_range(row, row, op.mode, op.index, op.count)
            .map(|(start, _)| (start, col))
            .ok_or_else(|| "reference points at a deleted row".to_string()),
        Axis::Column => transform_axis_range(col, col, op.mode, op.index, op.count)
            .map(|(start, _)| (row, start))
            .ok_or_else(|| "reference points at a deleted column".to_string()),
    }
}

fn transform_optional_axis(
    start: Option<u32>,
    end: Option<u32>,
    active: bool,
    op: StructureOp,
) -> Result<(Option<u32>, Option<u32>), String> {
    match (start, end) {
        (Some(start), Some(end)) => {
            if !active {
                return Ok((Some(start), Some(end)));
            }
            let start = start.checked_sub(1).ok_or("invalid range start")?;
            let end = end.checked_sub(1).ok_or("invalid range end")?;
            let (start, end) = transform_axis_range(start, end, op.mode, op.index, op.count)
                .ok_or_else(|| "reference range was deleted".to_string())?;
            Ok((Some(start + 1), Some(end + 1)))
        }
        (None, None) => Ok((None, None)),
        _ => Err("partially open ranges cannot be shifted safely".to_string()),
    }
}

fn rewrite_reference_for_origin(
    reference: ReferenceType,
    origin: &OriginMap,
) -> Result<Option<ReferenceType>, String> {
    match reference {
        ReferenceType::Cell {
            sheet,
            row,
            col,
            row_abs,
            col_abs,
        } => {
            let row = origin
                .current_row(row.saturating_sub(1))
                .ok_or_else(|| "referenced row was deleted".to_string())?;
            let col = origin
                .current_col(col.saturating_sub(1))
                .ok_or_else(|| "referenced column was deleted".to_string())?;
            Ok(Some(ReferenceType::Cell {
                sheet,
                row: row + 1,
                col: col + 1,
                row_abs,
                col_abs,
            }))
        }
        ReferenceType::Range {
            sheet,
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
        } => {
            let (start_row, end_row) =
                map_optional_origin(start_row, end_row, |value| origin.current_row(value))?;
            let (start_col, end_col) =
                map_optional_origin(start_col, end_col, |value| origin.current_col(value))?;
            Ok(Some(ReferenceType::Range {
                sheet,
                start_row,
                start_col,
                end_row,
                end_col,
                start_row_abs,
                start_col_abs,
                end_row_abs,
                end_col_abs,
            }))
        }
        ReferenceType::Cell3D { .. } | ReferenceType::Range3D { .. } => {
            Err("3D references cannot be shifted safely".to_string())
        }
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            Ok(None)
        }
    }
}

fn map_optional_origin<F>(
    start: Option<u32>,
    end: Option<u32>,
    mut map: F,
) -> Result<(Option<u32>, Option<u32>), String>
where
    F: FnMut(u32) -> Option<u32>,
{
    match (start, end) {
        (Some(start), Some(end)) => {
            let start = map(start.saturating_sub(1))
                .ok_or_else(|| "referenced range start was deleted".to_string())?;
            let end = map(end.saturating_sub(1))
                .ok_or_else(|| "referenced range end was deleted".to_string())?;
            Ok((Some(start + 1), Some(end + 1)))
        }
        (None, None) => Ok((None, None)),
        _ => Err("partially open ranges cannot be shifted safely".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::structure::{Axis, StructureMode, StructureOp};

    fn op(axis: Axis, mode: StructureMode, index: u32, count: u32) -> StructureOp {
        StructureOp {
            axis,
            mode,
            index,
            count,
        }
    }

    #[test]
    fn shifts_cells_and_ranges_after_row_insert() {
        let op = op(Axis::Row, StructureMode::Insert, 1, 1);
        assert_eq!(
            FormulaRefEngine::rewrite_for_op("=A1+B2", "Sheet1", "Sheet1", op),
            RewriteStatus::Rewritten("=A1+B3".to_string())
        );
        assert_eq!(
            FormulaRefEngine::rewrite_for_op("=SUM(A1:A3)", "Sheet1", "Sheet1", op),
            RewriteStatus::Rewritten("=SUM(A1:A4)".to_string())
        );
    }

    #[test]
    fn preserves_absolute_markers_and_sheet_names() {
        let op = op(Axis::Row, StructureMode::Insert, 1, 1);
        assert_eq!(
            FormulaRefEngine::rewrite_for_op("='Sheet2'!$A$2+$B$2", "Sheet1", "Sheet2", op),
            RewriteStatus::Rewritten("=Sheet2!$A$3+$B$2".to_string())
        );
    }

    #[test]
    fn leaves_unqualified_refs_on_other_sheets_unchanged() {
        let op = op(Axis::Row, StructureMode::Insert, 1, 1);
        assert_eq!(
            FormulaRefEngine::rewrite_for_op("=A2+Sheet2!A2", "Sheet1", "Sheet2", op),
            RewriteStatus::Rewritten("=A2+Sheet2!A3".to_string())
        );
    }

    #[test]
    fn shifts_columns_and_whole_column_ranges() {
        let op = op(Axis::Column, StructureMode::Insert, 1, 1);
        assert_eq!(
            FormulaRefEngine::rewrite_for_op("=B1+SUM(A:A)", "Sheet1", "Sheet1", op),
            RewriteStatus::Rewritten("=C1+SUM(A:A)".to_string())
        );
    }

    #[test]
    fn maps_original_coordinates_at_save_time() {
        let context = FormulaContext {
            transforms: BTreeMap::from([(
                "Sheet1".to_string(),
                OriginMap {
                    origin_rows: vec![Some(0), None, Some(1)],
                    origin_cols: vec![Some(0), Some(1)],
                },
            )]),
        };
        assert_eq!(
            FormulaRefEngine::rewrite_for_origin("=Sheet1!A2", &context),
            RewriteStatus::Rewritten("=Sheet1!A3".to_string())
        );
    }

    #[test]
    fn reports_parse_errors() {
        assert!(matches!(
            FormulaRefEngine::rewrite_for_op(
                "=SUM(",
                "S",
                "S",
                op(Axis::Row, StructureMode::Insert, 0, 1)
            ),
            RewriteStatus::Unsupported(_)
        ));
    }
}
