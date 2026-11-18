use std::collections::BTreeMap;

use crate::domain::addr::CellAddr;
use crate::domain::formula::{FormulaInfo, FormulaStatus};
use crate::domain::sheet::{Sheet, SheetData};
use crate::domain::structure::{
    FormulaChange, StructureEngine, StructureError, StructureOp, StructureOutcome, StructureWarning,
};
use crate::domain::workbook::WorkbookModel;
use crate::io::formula_ref::{FormulaRefEngine, RewriteStatus};

pub struct WorkbookStructureOutcome {
    pub structure: StructureOutcome,
    pub affected_sheets: Vec<usize>,
    pub formula_before: Vec<FormulaChange>,
    pub formula_after: Vec<FormulaChange>,
    pub warnings: Vec<StructureWarning>,
    pub recalc_required: bool,
}

pub struct StructureFlow;

impl StructureFlow {
    pub fn apply(
        workbook: &mut WorkbookModel,
        sheet_index: usize,
        op: StructureOp,
        max_cells: u64,
    ) -> Result<WorkbookStructureOutcome, StructureError> {
        let target_name = workbook
            .sheets
            .get(sheet_index)
            .map(|sheet| sheet.name.clone())
            .ok_or_else(|| StructureError::Sheet("sheet index out of bounds".to_string()))?;

        {
            let sheet = workbook
                .sheets
                .get_mut(sheet_index)
                .ok_or_else(|| StructureError::Sheet("sheet index out of bounds".to_string()))?;
            StructureEngine::materialize(sheet, max_cells)?;
        }

        // Validate every formula that will be touched before mutating anything.
        let mut formula_before = Vec::new();
        for (index, sheet) in workbook.sheets.iter().enumerate() {
            if !matches!(sheet.data, SheetData::Eager(_)) {
                continue;
            }
            for (addr, expression) in formulas(sheet) {
                match FormulaRefEngine::rewrite_for_op(&expression, &sheet.name, &target_name, op) {
                    RewriteStatus::Unchanged => {}
                    RewriteStatus::Rewritten(updated) => {
                        formula_before.push(FormulaChange {
                            sheet: index,
                            addr,
                            before: Some(expression),
                            after: Some(updated),
                        });
                    }
                    RewriteStatus::Unsupported(message) => {
                        return Err(StructureError::UnsupportedFormula {
                            sheet: sheet.name.clone(),
                            addr,
                            message,
                        });
                    }
                }
            }
        }

        let structure = {
            let sheet = workbook
                .sheets
                .get_mut(sheet_index)
                .ok_or_else(|| StructureError::Sheet("sheet index out of bounds".to_string()))?;
            StructureEngine::apply(sheet, op)?
        };

        let mut formula_after = Vec::new();
        let mut affected_sheets = vec![sheet_index];
        for index in 0..workbook.sheets.len() {
            if !matches!(workbook.sheets[index].data, SheetData::Eager(_)) {
                continue;
            }
            let source_name = workbook.sheets[index].name.clone();
            let entries = formulas(&workbook.sheets[index]);
            let mut changed = false;
            for (addr, expression) in entries {
                match FormulaRefEngine::rewrite_for_op(&expression, &source_name, &target_name, op)
                {
                    RewriteStatus::Unchanged => {}
                    RewriteStatus::Rewritten(updated) => {
                        set_formula_expression(&mut workbook.sheets[index], addr, updated.clone());
                        workbook.sheets[index]
                            .formula_rewrites
                            .insert(addr, updated.clone());
                        formula_after.push(FormulaChange {
                            sheet: index,
                            addr,
                            before: Some(expression),
                            after: Some(updated),
                        });
                        changed = true;
                    }
                    RewriteStatus::Unsupported(_) => {}
                }
            }
            if changed && !affected_sheets.contains(&index) {
                affected_sheets.push(index);
            }
        }

        let warnings = structure.warnings.clone();
        Ok(WorkbookStructureOutcome {
            structure,
            affected_sheets,
            formula_before,
            formula_after,
            warnings,
            recalc_required: true,
        })
    }
}

fn formulas(sheet: &Sheet) -> BTreeMap<CellAddr, String> {
    let mut values = BTreeMap::new();
    if let SheetData::Eager(cells) = &sheet.data {
        for (addr, cell) in cells {
            if let Some(formula) = &cell.formula {
                values.insert(*addr, formula.expression.clone());
            }
        }
    }
    for (addr, cell) in &sheet.edits {
        if let Some(formula) = &cell.formula {
            values.insert(*addr, formula.expression.clone());
        } else {
            values.remove(addr);
        }
    }
    values
}

fn set_formula_expression(sheet: &mut Sheet, addr: CellAddr, expression: String) {
    if let Some(cell) = sheet.edits.get_mut(&addr) {
        set_cell_formula(cell, expression);
        return;
    }
    if let SheetData::Eager(cells) = &mut sheet.data
        && let Some(cell) = cells.get_mut(&addr)
    {
        set_cell_formula(cell, expression);
    }
}

fn set_cell_formula(cell: &mut crate::domain::cell::Cell, expression: String) {
    if let Some(formula) = cell.formula.as_mut() {
        formula.expression = expression;
        formula.status = FormulaStatus::Pending;
    } else {
        cell.formula = Some(FormulaInfo::new(expression, None));
    }
}
