use std::path::{Path, PathBuf};

use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::cell::{Cell, CellValue};
use sheetgaze::domain::formula::{FormulaCellResult, FormulaInfo, FormulaResults, FormulaStatus};
use sheetgaze::domain::layout::{ColumnWidths, LayoutEngine, LayoutMode, Viewport};
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::domain::structure::{Axis, StructureEngine, StructureOp};
use sheetgaze::domain::style::CellStyle;
use sheetgaze::domain::workbook::WorkbookModel;
use sheetgaze::io::xlsx::{load_xlsx, load_xlsx_with_mode};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xlsx")
        .join(name)
}

fn eager_config() -> LazyConfig {
    LazyConfig {
        enabled: false,
        ..LazyConfig::default()
    }
}

fn lazy_config() -> LazyConfig {
    LazyConfig {
        enabled: true,
        threshold_cells: 0,
        threshold_bytes: 0,
        row_cache_rows: 64,
        prefetch_rows: 8,
    }
}

fn load(name: &str) -> WorkbookModel {
    load_xlsx(&fixture(name), &Default::default(), &eager_config()).unwrap()
}

fn load_lazy(name: &str) -> WorkbookModel {
    load_xlsx(&fixture(name), &Default::default(), &lazy_config()).unwrap()
}

fn plan_cell(
    plan: &sheetgaze::domain::layout::RenderPlan,
    row: u32,
    col: u32,
) -> &sheetgaze::domain::layout::RenderCell {
    plan.scroll_rows
        .iter()
        .flat_map(|plan_row| plan_row.cells.iter())
        .find(|cell| cell.addr == CellAddr::new(row, col))
        .unwrap_or_else(|| panic!("missing planned cell ({row}, {col})"))
}

#[test]
fn explicit_width_wrap_heights_are_computed_eagerly() {
    let workbook = load("excel_width_wrap.xlsx");
    let sheet = &workbook.sheets[0];

    assert_eq!(&sheet.layout.column_widths[..3], &[6, 8, 10]);
    assert!(sheet.layout.fixed_columns.contains(&0));
    assert!(sheet.layout.fixed_columns.contains(&1));
    assert!(sheet.layout.fixed_columns.contains(&2));

    assert_eq!(sheet.layout.wrap_row_heights.get(&0), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&1), None);
    assert_eq!(sheet.layout.wrap_row_heights.get(&2), Some(&7));
    assert_eq!(sheet.layout.wrap_row_heights.get(&3), Some(&7));
    assert_eq!(sheet.layout.wrap_row_heights.get(&4), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&5), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&7), Some(&10));

    assert_eq!(sheet.layout.row_heights[0], 1);
    assert_eq!(sheet.layout.row_heights[4], 2);
    assert_eq!(sheet.layout.display_row_height(0, true), 3);
    assert_eq!(sheet.layout.display_row_height(0, false), 1);
    assert_eq!(sheet.layout.display_row_height(4, true), 3);
    assert_eq!(sheet.layout.display_row_height(4, false), 2);
}

#[test]
fn explicit_width_wrap_heights_match_between_eager_and_lazy() {
    let eager = load("excel_width_wrap.xlsx");
    let lazy = load_lazy("excel_width_wrap.xlsx");
    let eager_sheet = &eager.sheets[0];
    let lazy_sheet = &lazy.sheets[0];

    assert_eq!(
        eager_sheet.layout.column_widths,
        lazy_sheet.layout.column_widths
    );
    assert_eq!(
        eager_sheet.layout.row_heights,
        lazy_sheet.layout.row_heights
    );
    assert_eq!(
        eager_sheet.layout.wrap_row_heights,
        lazy_sheet.layout.wrap_row_heights
    );
    assert_eq!(
        eager_sheet.layout.fixed_columns,
        lazy_sheet.layout.fixed_columns
    );
}

#[test]
fn grid_plan_wraps_when_spill_width_is_insufficient() {
    let workbook = load("excel_width_wrap.xlsx");
    let sheet = &workbook.sheets[0];
    let widths = ColumnWidths::from_sheet(sheet);
    assert!(widths.xlsx_priority());
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 40,
        width: 200,
    };
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);

    let row = |row: u32| {
        plan.scroll_rows
            .iter()
            .find(|plan_row| plan_row.row == row)
            .unwrap_or_else(|| panic!("missing row {row}"))
    };

    let a1 = plan_cell(&plan, 0, 0);
    assert_eq!(a1.width, 15);
    assert_eq!(a1.slot_width, 6);
    assert!(a1.overflow);
    assert!(!a1.truncated);
    assert_eq!(a1.lines.len(), 3);
    assert_eq!(row(0).height, 3);

    let a2 = plan_cell(&plan, 1, 0);
    assert_eq!(a2.width, 15);
    assert!(a2.overflow);
    assert!(!a2.truncated);
    assert_eq!(a2.lines.len(), 1);
    assert_eq!(row(1).height, 1);

    let a3 = plan_cell(&plan, 2, 0);
    assert_eq!(a3.width, 6);
    assert!(!a3.overflow);
    assert!(!a3.truncated);
    assert_eq!(a3.lines.len(), 7);
    assert_eq!(row(2).height, 7);

    let a4 = plan_cell(&plan, 3, 0);
    assert_eq!(a4.width, 6);
    assert!(!a4.overflow);
    assert_eq!(a4.lines.len(), 7);

    let a5 = plan_cell(&plan, 4, 0);
    assert_eq!(a5.lines.len(), 3);
    assert_eq!(row(4).height, 3);

    let merged = plan_cell(&plan, 5, 0);
    assert_eq!(merged.slot_width, 15);
    assert_eq!(merged.width, 15);
    assert_eq!(merged.lines.len(), 3);
    assert!(!merged.truncated);
    assert_eq!(row(5).height, 3);

    let capped = plan_cell(&plan, 7, 0);
    assert_eq!(capped.lines.len(), 10);
    assert!(capped.truncated);
    assert_eq!(row(7).height, 10);
}

#[test]
fn detail_and_grid_agree_on_wrap_and_truncation() {
    let workbook = load("excel_width_wrap.xlsx");
    let sheet = &workbook.sheets[0];
    let widths = ColumnWidths::from_sheet(sheet);
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 40,
        width: 200,
    };
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);

    for (row, col) in [(0, 0), (1, 0), (2, 0), (5, 0), (7, 0)] {
        let measure = LayoutEngine::measure_cell(sheet, CellAddr::new(row, col), &widths);
        let planned = plan_cell(&plan, row, col);
        assert_eq!(measure.truncated, planned.truncated, "{row}, {col}");
        assert_eq!(measure.lines.len(), planned.lines.len(), "{row}, {col}");
        assert_eq!(measure.width, planned.width, "{row}, {col}");
    }
}

#[test]
fn fit_width_mode_ignores_the_extra_wrap_heights() {
    let workbook = load("excel_width_wrap.xlsx");
    let sheet = &workbook.sheets[0];
    let widths = ColumnWidths::fit(sheet, 200);
    assert!(!widths.xlsx_priority());

    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 40,
        width: 200,
    };
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);
    let row = |row: u32| {
        plan.scroll_rows
            .iter()
            .find(|plan_row| plan_row.row == row)
            .unwrap_or_else(|| panic!("missing row {row}"))
            .height
    };
    assert_eq!(row(0), 1);
    assert_eq!(row(4), 2);
    assert_eq!(row(7), 1);

    let a1 = plan_cell(&plan, 0, 0);
    assert_eq!(a1.lines.len(), 1);
    assert!(a1.truncated);
}

#[test]
fn content_layout_does_not_create_wrap_heights() {
    let workbook = load_xlsx_with_mode(
        &fixture("excel_width_wrap.xlsx"),
        &Default::default(),
        &eager_config(),
        LayoutMode::Content,
    )
    .unwrap();
    let sheet = &workbook.sheets[0];

    assert!(sheet.layout.fixed_columns.is_empty());
    assert!(sheet.layout.wrap_row_heights.is_empty());
    assert!(sheet.layout.row_heights.iter().all(|height| *height == 1));
}

#[test]
fn editing_a_blocking_neighbour_wraps_the_cell_to_its_left() {
    let mut workbook = load("excel_width_wrap.xlsx");
    let sheet = &mut workbook.sheets[0];
    assert_eq!(sheet.layout.display_row_height(1, true), 1);

    sheet.set_edit_cell(
        CellAddr::new(1, 1),
        Some(Cell::new(
            CellValue::Text("x".to_string()),
            CellStyle::default(),
        )),
    );
    assert_eq!(sheet.layout.display_row_height(1, true), 2);
}

#[test]
fn editing_a_fixed_column_grows_the_row_height() {
    let mut workbook = load("excel_width_wrap.xlsx");
    let sheet = &mut workbook.sheets[0];
    let long = "0123456789".repeat(4);

    sheet.set_edit_cell(
        CellAddr::new(9, 0),
        Some(Cell::new(
            CellValue::Text(long.clone()),
            CellStyle::default(),
        )),
    );
    assert!(sheet.layout.display_row_height(9, true) >= 2);

    // The edited cell itself is wrapped in the next grid plan.
    let widths = ColumnWidths::from_sheet(sheet);
    let measure = LayoutEngine::measure_cell(sheet, CellAddr::new(9, 0), &widths);
    assert_eq!(
        measure.lines.len(),
        sheet.layout.display_row_height(9, true) as usize
    );
    assert!(!measure.truncated);
}

#[test]
fn structural_row_changes_shift_wrap_heights() {
    let mut workbook = load("excel_width_wrap.xlsx");
    let sheet = &mut workbook.sheets[0];

    StructureEngine::apply(sheet, StructureOp::insert(Axis::Row, 0, 1)).unwrap();
    assert_eq!(sheet.layout.wrap_row_heights.get(&1), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&3), Some(&7));

    StructureEngine::apply(sheet, StructureOp::delete(Axis::Row, 0, 1)).unwrap();
    assert_eq!(sheet.layout.wrap_row_heights.get(&0), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&1), None);
    assert_eq!(sheet.layout.wrap_row_heights.get(&2), Some(&7));
}

#[test]
fn structural_column_changes_recompute_wrap_heights() {
    let mut workbook = load("excel_width_wrap.xlsx");
    let sheet = &mut workbook.sheets[0];

    StructureEngine::apply(sheet, StructureOp::delete(Axis::Column, 1, 1)).unwrap();
    assert_eq!(sheet.layout.wrap_row_heights.get(&0), Some(&7));
    assert_eq!(sheet.layout.wrap_row_heights.get(&1), Some(&2));
    assert_eq!(sheet.layout.wrap_row_heights.get(&3), Some(&3));
    assert_eq!(sheet.layout.wrap_row_heights.get(&4), Some(&7));
}

#[test]
fn formula_results_refresh_wrap_heights() {
    let mut workbook = load("excel_width_wrap.xlsx");
    let sheet_name = workbook.sheets[0].name.clone();
    let addr = CellAddr::new(10, 0);
    {
        let sheet = &mut workbook.sheets[0];
        let mut cell = Cell::new(CellValue::Text("x".to_string()), CellStyle::default());
        cell.formula = Some(FormulaInfo::new(
            "=REPT(\"0123456789\",4)".to_string(),
            Some(CellValue::Text("x".to_string())),
        ));
        sheet.set_edit_cell(addr, Some(cell));
        assert_eq!(sheet.layout.display_row_height(addr.row, true), 1);
    }

    let long = "0123456789".repeat(4);
    workbook.apply_formula_results(FormulaResults {
        generation: 1,
        updates: vec![FormulaCellResult {
            sheet: sheet_name,
            addr,
            value: CellValue::Text(long),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(workbook.sheets[0].layout.display_row_height(addr.row, true) >= 2);
}
