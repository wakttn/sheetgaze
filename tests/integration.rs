use std::path::PathBuf;

use rust_xlsxwriter::{FilterCondition, Format, Workbook};
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::copy::CopyLimits;
use sheetgaze::domain::formula::RecalcPolicy;
use sheetgaze::domain::row_store::{LazyConfig, RowRange};
use sheetgaze::domain::selection::Selection;
use sheetgaze::domain::sheet::SheetData;
use sheetgaze::domain::workbook::SourceFormat;
use sheetgaze::io::csv::load_csv;
use sheetgaze::io::xlsx::load_xlsx;

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheetgaze-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_xlsx(path: &PathBuf) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(0, 0, "Name").unwrap();
    worksheet.write_string(0, 1, "Value").unwrap();
    worksheet.write_string(0, 2, "Note").unwrap();
    worksheet.write_string(1, 0, "East").unwrap();
    worksheet.write_number(1, 1, 10.0).unwrap();
    worksheet.write_string(1, 2, "line1\nline2").unwrap();
    worksheet.write_string(2, 0, "West").unwrap();
    worksheet.write_number(2, 1, 20.0).unwrap();
    worksheet.write_string(3, 0, "East").unwrap();
    worksheet.write_number(3, 1, 30.0).unwrap();
    worksheet.write_formula(1, 3, "=SUM(B2:B4)").unwrap();
    worksheet
        .merge_range(5, 0, 6, 1, "Merged", &Format::new().set_bold())
        .unwrap();
    worksheet.autofilter(0, 0, 3, 2).unwrap();
    worksheet
        .filter_column(0, &FilterCondition::new().add_list_filter("East"))
        .unwrap();
    worksheet.set_freeze_panes(1, 0).unwrap();
    workbook.save(path).unwrap();
}

#[test]
fn loads_xlsx_values_styles_merges_freeze_and_filters() {
    let path = fixture_dir().join("basic.xlsx");
    create_xlsx(&path);
    let config = LazyConfig {
        enabled: false,
        ..LazyConfig::default()
    };
    let workbook = load_xlsx(&path, &Default::default(), &config).unwrap();
    assert_eq!(workbook.source.format, SourceFormat::Xlsx);
    assert_eq!(workbook.sheets.len(), 1);
    let sheet = &workbook.sheets[0];
    assert_eq!(sheet.freeze.frozen_rows, 1);
    assert!(!sheet.filters.is_empty());
    assert!(sheet.visibility.is_visible(1));
    assert!(!sheet.visibility.is_visible(2));
    assert!(sheet.visibility.is_visible(3));
    let cell = sheet.effective_cell(CellAddr::new(1, 2)).unwrap();
    assert!(cell.display.contains('\n'));
    assert!(sheet.merges.region_at(CellAddr::new(6, 1)).is_some());
    assert_eq!(
        sheet.merges.anchor_for(CellAddr::new(6, 1)),
        CellAddr::new(5, 0)
    );
}

#[test]
fn loads_lazy_rows_and_effective_values() {
    let path = fixture_dir().join("lazy.xlsx");
    create_xlsx(&path);
    let config = LazyConfig {
        enabled: true,
        threshold_cells: 0,
        threshold_bytes: 0,
        row_cache_rows: 2,
        prefetch_rows: 0,
    };
    let mut workbook = load_xlsx(&path, &Default::default(), &config).unwrap();
    assert!(matches!(workbook.sheets[0].data, SheetData::Lazy(_)));
    assert!(!workbook.sheets[0].visibility.is_visible(2));
    assert!(
        workbook.sheets[0]
            .formula_cells
            .contains(&CellAddr::new(1, 3))
    );
    workbook.ensure_rows(0, RowRange::new(1, 2)).unwrap();
    let cell = workbook.sheets[0]
        .effective_cell(CellAddr::new(1, 1))
        .unwrap();
    assert_eq!(cell.display, "10");
}

#[test]
fn detects_csv_encoding_and_preserves_text() {
    let path = fixture_dir().join("shift_jis.csv");
    let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode("名前,値\n東,10\n");
    std::fs::write(&path, encoded.as_ref()).unwrap();
    let workbook = load_csv(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(workbook.source.format, SourceFormat::Csv);
    assert!(workbook.source.encoding.is_some());
    let cell = workbook.sheets[0]
        .effective_cell(CellAddr::new(1, 0))
        .unwrap();
    assert_eq!(cell.display, "東");
}

#[test]
fn formula_recalculation_computes_sum() {
    let path = fixture_dir().join("formula.xlsx");
    create_xlsx(&path);
    let results = sheetgaze::io::formula::recalculate(
        &path,
        &[("Sheet1".to_string(), CellAddr::new(1, 3))],
        &sheetgaze::domain::task::CancelToken::new(),
        1,
    )
    .unwrap();
    assert_eq!(results.updates.len(), 1);
    assert_eq!(results.updates[0].value.display_text(), "60");
}

#[test]
fn policy_auto_skips_large_workbooks() {
    assert!(!sheetgaze::io::formula::should_recalculate(
        RecalcPolicy::Auto,
        1_000_000,
        1024,
        &Default::default()
    ));
    assert!(sheetgaze::io::formula::should_recalculate(
        RecalcPolicy::Always,
        1_000_000,
        1024,
        &Default::default()
    ));
}

#[test]
fn lazy_copy_streams_beyond_cache_capacity() {
    let path = fixture_dir().join("lazy-copy.xlsx");
    create_xlsx(&path);
    let config = LazyConfig {
        enabled: true,
        threshold_cells: 0,
        threshold_bytes: 0,
        row_cache_rows: 1,
        prefetch_rows: 0,
    };
    let mut workbook = load_xlsx(&path, &Default::default(), &config).unwrap();
    let mut selection = Selection::new(CellAddr::new(1, 0));
    selection.extend(CellAddr::new(3, 1));
    let payload = workbook
        .copy_selection(0, &selection, &CopyLimits::default())
        .unwrap();
    assert_eq!(payload.text.matches("East").count(), 2);
    assert!(!payload.text.contains("West"));
    assert_eq!(workbook.sheets[0].formula_cells.len(), 1);
}

#[test]
fn lazy_formula_overlay_is_applied_on_row_load() {
    let path = fixture_dir().join("lazy-formula.xlsx");
    create_xlsx(&path);
    let config = LazyConfig {
        enabled: true,
        threshold_cells: 0,
        threshold_bytes: 0,
        row_cache_rows: 2,
        prefetch_rows: 0,
    };
    let mut workbook = load_xlsx(&path, &Default::default(), &config).unwrap();
    let cells = workbook.sheets[0]
        .formula_cells
        .iter()
        .map(|addr| ("Sheet1".to_string(), *addr))
        .collect::<Vec<_>>();
    let results = sheetgaze::io::formula::recalculate(
        &path,
        &cells,
        &sheetgaze::domain::task::CancelToken::new(),
        1,
    )
    .unwrap();
    workbook.apply_formula_results(results);
    workbook.ensure_rows(0, RowRange::new(1, 2)).unwrap();
    let cell = workbook.sheets[0]
        .effective_cell(CellAddr::new(1, 3))
        .unwrap();
    assert_eq!(cell.display, "60");
}
