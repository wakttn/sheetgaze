//! Smoke coverage for the committed xlsx fixtures.
//!
//! These tests do not regenerate files. Regenerate with:
//!
//! ```text
//! cargo test --test generate_fixtures -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use sheetgaze::domain::addr::{CellAddr, CellRange};
use sheetgaze::domain::cell::CellValue;
use sheetgaze::domain::filter::FilterSource;
use sheetgaze::domain::layout::{ColumnWidths, LayoutEngine, LayoutMode, Viewport};
use sheetgaze::domain::row_store::{LazyConfig, RowRange};
use sheetgaze::domain::sheet::SheetData;
use sheetgaze::domain::style::{BorderStyle, RgbColor};
use sheetgaze::domain::workbook::WorkbookModel;
use sheetgaze::io::xlsx::{load_xlsx, load_xlsx_with_mode};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx")
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

fn fixture_files() -> Vec<PathBuf> {
    let dir = fixture_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("missing fixture dir {}: {error}", dir.display()))
        .map(|entry| entry.expect("fixture dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "xlsx"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no xlsx fixtures found in {}",
        dir.display()
    );
    files
}

fn load_eager(name: &str) -> WorkbookModel {
    let path = fixture_dir().join(name);
    load_xlsx(&path, &Default::default(), &eager_config())
        .unwrap_or_else(|error| panic!("eager load failed for {name}: {error}"))
}

#[test]
fn all_fixtures_load_in_eager_and_lazy_modes() {
    for path in fixture_files() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>");

        let eager = load_xlsx(&path, &Default::default(), &eager_config())
            .unwrap_or_else(|error| panic!("eager load failed for {name}: {error}"));
        assert!(
            !eager.sheets.is_empty(),
            "{name}: eager load returned no sheets"
        );

        let lazy = load_xlsx(&path, &Default::default(), &lazy_config())
            .unwrap_or_else(|error| panic!("lazy load failed for {name}: {error}"));
        assert!(
            !lazy.sheets.is_empty(),
            "{name}: lazy load returned no sheets"
        );
        assert!(
            lazy.sheets
                .iter()
                .all(|sheet| matches!(sheet.data, SheetData::Lazy(_))),
            "{name}: not every sheet used lazy storage"
        );

        println!("{name}: {} sheet(s) loaded", eager.sheets.len());
    }
}

#[test]
fn intentional_warning_fixtures_still_load() {
    let freeze = load_eager("freeze_merge_conflict.xlsx");
    let sheet = &freeze.sheets[0];
    assert!(
        sheet.freeze.disabled,
        "freeze intersecting a merge must be disabled"
    );
    assert!(
        !sheet.warnings.is_empty(),
        "freeze merge conflict must leave a warning"
    );

    let filter = load_eager("filter_merge_conflict.xlsx");
    let sheet = &filter.sheets[0];
    assert!(
        sheet.filters.is_empty(),
        "filter intersecting a merge must be dropped"
    );
    assert!(
        !sheet.warnings.is_empty(),
        "filter merge conflict must leave a warning"
    );
}

#[test]
fn basic_types_exposes_values_dates_and_empty_cells() {
    let workbook = load_eager("basic_types.xlsx");
    let sheet = &workbook.sheets[0];

    assert_eq!(
        sheet.effective_cell(CellAddr::new(1, 1)).unwrap().value,
        CellValue::Text("Hello, 世界".to_string())
    );
    assert_eq!(
        sheet.effective_cell(CellAddr::new(2, 1)).unwrap().value,
        CellValue::Number(42.0)
    );
    assert!(matches!(
        sheet.effective_cell(CellAddr::new(3, 1)).unwrap().value,
        CellValue::Number(_)
    ));
    assert_eq!(
        sheet.effective_cell(CellAddr::new(4, 1)).unwrap().value,
        CellValue::Bool(true)
    );
    assert!(matches!(
        sheet.effective_cell(CellAddr::new(5, 1)).unwrap().value,
        CellValue::DateTime(_)
    ));
    let blank = sheet.effective_cell(CellAddr::new(6, 1)).unwrap();
    assert_eq!(blank.value, CellValue::Empty);
    assert!(blank.display.is_empty());
}

#[test]
fn borders_are_read_for_values_empty_cells_and_merges() {
    let workbook = load_eager("borders.xlsx");
    let sheet = &workbook.sheets[0];

    let header = sheet.effective_cell(CellAddr::new(0, 0)).unwrap();
    assert_eq!(header.style.borders.top.style, BorderStyle::Thin);
    assert_eq!(header.style.borders.bottom.style, BorderStyle::Thin);
    assert_eq!(header.style.borders.left.style, BorderStyle::Thin);
    assert_eq!(header.style.borders.right.style, BorderStyle::Thin);

    let empty = sheet.effective_cell(CellAddr::new(1, 2)).unwrap();
    assert!(empty.value.is_empty());
    assert_eq!(empty.style.borders.left.style, BorderStyle::Thin);
    assert_eq!(empty.style.borders.right.style, BorderStyle::Thin);

    let medium = sheet.effective_cell(CellAddr::new(4, 0)).unwrap();
    assert_eq!(medium.style.borders.left.style, BorderStyle::Medium);
    let double = sheet.effective_cell(CellAddr::new(5, 0)).unwrap();
    assert_eq!(double.style.borders.left.style, BorderStyle::Double);
    let dashed = sheet.effective_cell(CellAddr::new(6, 0)).unwrap();
    assert_eq!(dashed.style.borders.left.style, BorderStyle::Dashed);
    let red = sheet.effective_cell(CellAddr::new(7, 0)).unwrap();
    assert_eq!(red.style.borders.left.style, BorderStyle::Thin);
    assert_eq!(red.style.borders.left.color, Some(RgbColor::new(255, 0, 0)));

    let merged = sheet.merges.region_at(CellAddr::new(5, 2)).unwrap();
    let merged_borders = sheet
        .merge_borders
        .get(&merged.anchor)
        .expect("merged range borders should be aggregated");
    assert!(merged_borders.top.is_visible());
    assert!(merged_borders.bottom.is_visible());
    assert!(merged_borders.left.is_visible());
    assert!(merged_borders.right.is_visible());

    let widths = ColumnWidths::from_sheet(sheet);
    let plan = LayoutEngine::plan(
        sheet,
        &Viewport {
            top_visible_row: 0,
            left_col: 0,
            height: 40,
            width: 120,
        },
        CellAddr::new(0, 0),
        None,
        None,
        &widths,
    );
    let row = plan
        .scroll_rows
        .iter()
        .find(|row| row.row == 10)
        .expect("bordered overflow row should be visible");
    let long = row
        .cells
        .iter()
        .find(|cell| cell.addr == CellAddr::new(10, 0))
        .expect("long cell should be rendered");
    assert!(!long.overflow, "explicit border must stop overflow");
    assert_eq!(long.width, long.slot_width);

    let mut lazy = load_xlsx(
        &fixture_dir().join("borders.xlsx"),
        &Default::default(),
        &lazy_config(),
    )
    .unwrap();
    let sheet = &mut lazy.sheets[0];
    sheet.ensure_rows(RowRange::new(0, 12)).unwrap();
    let empty = sheet.effective_cell(CellAddr::new(1, 2)).unwrap();
    assert!(empty.value.is_empty());
    assert!(empty.style.borders.left.is_visible());
    let merged = sheet.merges.region_at(CellAddr::new(5, 2)).unwrap();
    assert!(sheet.merge_borders.contains_key(&merged.anchor));
}

#[test]
fn merged_range_extends_used_range_and_renders_tail() {
    let workbook = load_eager("merged_tail.xlsx");
    let sheet = &workbook.sheets[0];
    let range = CellRange::new(CellAddr::new(2, 0), CellAddr::new(5, 3));
    assert_eq!(sheet.meta.row_count, 6);
    assert_eq!(sheet.meta.column_count, 4);
    assert_eq!(sheet.used_range.end, CellAddr::new(5, 3));

    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 20,
        width: 80,
    };
    let widths = ColumnWidths::from_sheet(sheet);
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);
    assert!(
        plan.scroll_rows.iter().any(|row| row.row == 5),
        "merged rows past the last valued cell must be planned"
    );
    let slices: Vec<_> = plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.merge_span == Some(range))
        .collect();
    assert_eq!(slices.len(), 4);
    let text: String = slices
        .iter()
        .flat_map(|cell| cell.lines.iter())
        .flat_map(|line| line.spans.iter())
        .map(|span| span.text.as_str())
        .collect();
    assert!(text.contains("必要があります"));

    let mut lazy = load_xlsx(
        &fixture_dir().join("merged_tail.xlsx"),
        &Default::default(),
        &lazy_config(),
    )
    .unwrap();
    let sheet = &mut lazy.sheets[0];
    assert_eq!(sheet.meta.row_count, 6);
    sheet.ensure_rows(RowRange::new(0, 6)).unwrap();
    let widths = ColumnWidths::from_sheet(sheet);
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);
    let slices: Vec<_> = plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.merge_span == Some(range))
        .collect();
    assert_eq!(slices.len(), 4);
}

#[test]
fn multi_row_merged_cells_render_all_lines() {
    let workbook = load_eager("merged_newlines.xlsx");
    let sheet = &workbook.sheets[0];
    let range = CellRange::new(CellAddr::new(2, 0), CellAddr::new(4, 1));

    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 20,
        width: 80,
    };
    let widths = ColumnWidths::from_sheet(sheet);
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);
    let slices: Vec<_> = plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.merge_span == Some(range))
        .collect();
    assert_eq!(slices.len(), 3, "one slice per merged row");
    assert!(!slices[0].merge_continuation);
    assert!(slices[1].merge_continuation);
    assert!(slices[2].merge_continuation);

    let texts: Vec<String> = slices
        .iter()
        .flat_map(|cell| cell.lines.iter())
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert_eq!(texts, ["複数行", "複数列", "の結合セル"]);

    let scrolled = LayoutEngine::plan(
        sheet,
        &Viewport {
            top_visible_row: 3,
            ..viewport
        },
        CellAddr::new(0, 0),
        None,
        None,
        &widths,
    );
    let scrolled_texts: Vec<String> = scrolled
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.merge_span == Some(range))
        .flat_map(|cell| cell.lines.iter())
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert_eq!(
        scrolled_texts,
        ["複数列", "の結合セル"],
        "middle and tail must remain visible when the anchor row is scrolled out"
    );
}

#[test]
fn styles_and_merges_are_read() {
    let styles = load_eager("styles.xlsx");
    let sheet = &styles.sheets[0];
    assert!(
        sheet
            .effective_cell(CellAddr::new(0, 0))
            .unwrap()
            .style
            .bold
    );
    assert_eq!(
        sheet
            .effective_cell(CellAddr::new(1, 0))
            .unwrap()
            .style
            .color,
        Some(RgbColor::new(255, 0, 0))
    );
    assert_eq!(
        sheet
            .effective_cell(CellAddr::new(2, 0))
            .unwrap()
            .style
            .color,
        Some(RgbColor::new(0, 0, 255))
    );
    assert!(
        !sheet
            .effective_cell(CellAddr::new(3, 0))
            .unwrap()
            .style
            .bold
    );

    let merged = load_eager("merged_newlines.xlsx");
    let sheet = &merged.sheets[0];
    assert!(!sheet.merges.regions().is_empty());
    assert_eq!(
        sheet.merges.anchor_for(CellAddr::new(0, 2)),
        CellAddr::new(0, 0)
    );
    assert!(
        sheet
            .effective_cell(CellAddr::new(8, 0))
            .unwrap()
            .display
            .contains('\n')
    );
}

#[test]
fn formulas_include_cross_sheet_errors_and_unsupported_calls() {
    let workbook = load_eager("formulas.xlsx");
    let sheet = &workbook.sheets[0];

    assert_eq!(sheet.formula_cells.len(), 7);
    let cross_sheet = sheet.effective_cell(CellAddr::new(3, 1)).unwrap();
    assert_eq!(cross_sheet.value, CellValue::Number(123.0));
    assert!(
        cross_sheet
            .formula
            .as_ref()
            .is_some_and(|formula| formula.expression.contains("Data Sheet"))
    );

    let error = sheet.effective_cell(CellAddr::new(5, 1)).unwrap();
    assert_eq!(error.value, CellValue::Error("#DIV/0!".to_string()));

    let unsupported = sheet.effective_cell(CellAddr::new(7, 1)).unwrap();
    assert_eq!(unsupported.value, CellValue::Error("#NAME?".to_string()));
}

#[test]
fn overflow_plan_represents_overflow_and_truncation() {
    let workbook = load_eager("overflow.xlsx");
    let sheet = &workbook.sheets[0];
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 40,
        width: 200,
    };
    let widths = ColumnWidths::from_sheet(sheet);
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None, &widths);
    let cell = |row: u32, col: u32| {
        plan.scroll_rows
            .iter()
            .flat_map(|plan_row| plan_row.cells.iter())
            .find(|cell| cell.addr == CellAddr::new(row, col))
            .unwrap_or_else(|| panic!("missing planned cell ({row}, {col})"))
    };

    let pure_overflow = cell(5, 0);
    assert!(pure_overflow.overflow);
    assert!(!pure_overflow.truncated);

    let blocked = cell(1, 0);
    assert!(!blocked.overflow);
    assert!(blocked.truncated);

    let merged_blocker = cell(2, 0);
    assert!(merged_blocker.overflow);
    assert!(merged_blocker.truncated);
}

#[test]
fn freeze_fixtures_report_expected_split_sizes() {
    let rows = load_eager("freeze_rows.xlsx");
    let both = load_eager("freeze_both.xlsx");
    let merge = load_eager("freeze_merge_conflict.xlsx");

    for (name, workbook) in [
        ("freeze_rows.xlsx", &rows),
        ("freeze_both.xlsx", &both),
        ("freeze_merge_conflict.xlsx", &merge),
    ] {
        let sheet = &workbook.sheets[0];
        assert!(
            sheet.meta.row_count >= 100,
            "{name}: expected at least 100 rows, got {}",
            sheet.meta.row_count
        );
        assert!(
            sheet.meta.column_count >= 100,
            "{name}: expected at least 100 columns, got {}",
            sheet.meta.column_count
        );
    }

    assert_eq!(rows.sheets[0].freeze.frozen_rows, 2);
    assert_eq!(rows.sheets[0].freeze.frozen_cols, 0);
    assert!(rows.sheets[0].freeze.is_active());

    assert_eq!(both.sheets[0].freeze.frozen_rows, 1);
    assert_eq!(both.sheets[0].freeze.frozen_cols, 1);
    assert!(both.sheets[0].freeze.is_active());
}

#[test]
fn worksheet_filters_hide_rows() {
    let values = load_eager("filters_values.xlsx");
    let sheet = &values.sheets[0];
    assert!(!sheet.filters.is_empty());
    assert!(sheet.visibility.visible_count() < sheet.meta.row_count);
    assert!(sheet.visibility.visible_count() >= 2);

    let custom = load_eager("filters_custom.xlsx");
    let sheet = &custom.sheets[0];
    assert!(!sheet.filters.is_empty());
    assert!(sheet.visibility.visible_count() < sheet.meta.row_count);
    assert!(sheet.visibility.visible_count() >= 2);
}

#[test]
fn multi_sheet_sparse_and_empty_fixtures_have_expected_shapes() {
    let multi = load_eager("multi_sheet.xlsx");
    let names: Vec<&str> = multi
        .sheets
        .iter()
        .map(|sheet| sheet.name.as_str())
        .collect();
    assert_eq!(names, ["概要", "データ", "空シート"]);

    let sparse = load_eager("sparse.xlsx");
    let used = sparse.sheets[0].used_range;
    assert_eq!(used.end, CellAddr::new(99, 26));
    assert_eq!(
        sparse.sheets[0]
            .effective_cell(CellAddr::new(99, 26))
            .unwrap()
            .display,
        "AA100"
    );

    let empty = load_eager("empty_sheet.xlsx");
    assert_eq!(empty.sheets.len(), 1);
    let blank = empty.sheets[0].effective_cell(CellAddr::new(0, 0)).unwrap();
    assert_eq!(blank.value, CellValue::Empty);
    assert!(blank.display.is_empty());

    let single = load_eager("single_cell.xlsx");
    assert_eq!(
        single.sheets[0]
            .effective_cell(CellAddr::new(0, 0))
            .unwrap()
            .display,
        "only"
    );
}

#[test]
fn large_lazy_fixture_loads_rows_beyond_the_cache_capacity() {
    // Use no prefetch here so that requesting the final rows stays within the
    // source's exclusive row range. A prefetch beyond EOF currently makes the
    // spill row source return "row N is out of range"; see the fixture report.
    let config = LazyConfig {
        prefetch_rows: 0,
        ..lazy_config()
    };
    let path = fixture_dir().join("large_lazy.xlsx");
    let mut workbook = load_xlsx(&path, &Default::default(), &config)
        .expect("large_lazy.xlsx must load in lazy mode");
    let sheet = &workbook.sheets[0];
    assert!(matches!(sheet.data, SheetData::Lazy(_)));
    assert!(sheet.meta.row_count >= 6_000);

    let row_count = sheet.meta.row_count;
    workbook
        .ensure_rows(0, RowRange::new(row_count - 5, row_count))
        .expect("load the last rows through the lazy row source");
    let last_row = row_count - 1;
    let cell = workbook.sheets[0]
        .effective_cell(CellAddr::new(last_row, 0))
        .expect("last row must be readable after ensure_rows");
    assert_eq!(cell.value, CellValue::Number(last_row as f64));
}

#[test]
fn table_filter_is_read_from_table_xml() {
    let workbook = load_eager("filters_table.xlsx");
    let sheet = &workbook.sheets[0];
    assert!(!sheet.filters.is_empty(), "table autoFilter was not parsed");
    assert!(
        sheet
            .filters
            .iter()
            .any(|filter| matches!(&filter.source, FilterSource::Table { .. }))
    );
}

#[test]
fn excel_layout_fixture_uses_explicit_widths_and_heights() {
    let workbook = load_eager("excel_layout.xlsx");
    let sheet = &workbook.sheets[0];

    assert_eq!(sheet.layout.column_widths[0], 24);
    assert_eq!(sheet.layout.column_widths[1], 6);
    assert_eq!(sheet.layout.column_widths[2], 12);
    assert_eq!(sheet.layout.row_heights[0], 2);
    assert_eq!(sheet.layout.row_heights[1], 3);
    assert_eq!(sheet.layout.row_heights[2], 1);
    assert!(sheet.layout.fixed_columns.contains(&0));
    assert!(sheet.layout.fixed_columns.contains(&1));
    assert!(sheet.layout.fixed_columns.contains(&2));
    assert!(sheet.layout.fixed_rows.contains(&0));
    assert!(sheet.layout.fixed_rows.contains(&1));
    assert!(sheet.layout.fixed_rows.contains(&2));

    let widths = ColumnWidths::from_sheet(sheet);
    let wrap = LayoutEngine::measure_cell(sheet, CellAddr::new(2, 1), &widths);
    assert!(
        wrap.truncated,
        "an explicit one-line row height must truncate wrapped text"
    );
}

#[test]
fn excel_layout_is_identical_in_eager_and_lazy_modes() {
    let path = fixture_dir().join("excel_layout.xlsx");
    let eager = load_xlsx_with_mode(
        &path,
        &Default::default(),
        &eager_config(),
        LayoutMode::Excel,
    )
    .unwrap();
    let lazy = load_xlsx_with_mode(
        &path,
        &Default::default(),
        &lazy_config(),
        LayoutMode::Excel,
    )
    .unwrap();

    for (eager_sheet, lazy_sheet) in eager.sheets.iter().zip(&lazy.sheets) {
        assert_eq!(
            eager_sheet.layout.column_widths,
            lazy_sheet.layout.column_widths
        );
        assert_eq!(
            eager_sheet.layout.row_heights,
            lazy_sheet.layout.row_heights
        );
        assert_eq!(
            eager_sheet.layout.fixed_columns,
            lazy_sheet.layout.fixed_columns
        );
        assert_eq!(eager_sheet.layout.fixed_rows, lazy_sheet.layout.fixed_rows);
    }
}

#[test]
fn content_layout_mode_ignores_excel_sizes() {
    let path = fixture_dir().join("excel_layout.xlsx");
    let workbook = load_xlsx_with_mode(
        &path,
        &Default::default(),
        &eager_config(),
        LayoutMode::Content,
    )
    .unwrap();
    let sheet = &workbook.sheets[0];

    assert!(sheet.layout.fixed_columns.is_empty());
    assert!(sheet.layout.fixed_rows.is_empty());
    assert!(
        sheet.layout.column_widths[0] < 24,
        "content mode should auto-fit instead of using the Excel width"
    );
}
