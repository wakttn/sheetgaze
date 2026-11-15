//! Deterministic xlsx fixture generator.
//!
//! Run with:
//!
//! ```text
//! cargo test --test generate_fixtures -- --ignored --nocapture
//! ```
//!
//! The generated files are written to `tests/fixtures/xlsx/` and are committed
//! as binary fixtures so that the regular test suite does not need to generate
//! them. A fixed document creation date keeps the output byte-for-byte stable
//! across runs.

use std::error::Error;
use std::path::{Path, PathBuf};

use rust_xlsxwriter::{
    Color, DocProperties, ExcelDateTime, FilterCondition, FilterCriteria, Format, FormatBorder,
    Table, Workbook, Worksheet, XlsxError,
};

type FixtureResult = Result<(), Box<dyn Error>>;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx")
}

fn new_workbook() -> Result<Workbook, XlsxError> {
    let mut workbook = Workbook::new();
    let created = ExcelDateTime::from_ymd(2026, 1, 1)?;
    let properties = DocProperties::new().set_creation_datetime(&created);
    workbook.set_properties(&properties);
    Ok(workbook)
}

fn save(workbook: &mut Workbook, dir: &Path, name: &str) -> FixtureResult {
    let path = dir.join(name);
    workbook.save(&path)?;
    println!("wrote {}", path.display());
    Ok(())
}

#[test]
#[ignore = "generates binary xlsx fixtures on demand"]
fn generate_fixtures() -> FixtureResult {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir)?;

    basic_types(&dir)?;
    formulas(&dir)?;
    styles(&dir)?;
    borders(&dir)?;
    merged_newlines(&dir)?;
    merged_tail(&dir)?;
    overflow(&dir)?;
    excel_layout(&dir)?;
    freeze_rows(&dir)?;
    freeze_both(&dir)?;
    freeze_merge_conflict(&dir)?;
    filters_values(&dir)?;
    filters_custom(&dir)?;
    filters_table(&dir)?;
    filter_merge_conflict(&dir)?;
    multi_sheet(&dir)?;
    sparse(&dir)?;
    special_chars(&dir)?;
    large_lazy(&dir)?;
    empty_sheet(&dir)?;
    single_cell(&dir)?;
    hidden_rows_columns(&dir)?;

    println!("fixtures written to {}", dir.display());
    Ok(())
}

fn basic_types(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Basic Types")?;
    sheet.set_column_width(0, 24)?;
    let date_format = Format::new().set_num_format("yyyy-mm-dd");

    sheet.write_string(0, 0, "kind")?;
    sheet.write_string(0, 1, "value")?;

    sheet.write_string(1, 0, "text")?;
    sheet.write_string(1, 1, "Hello, 世界")?;

    sheet.write_string(2, 0, "integer")?;
    sheet.write_number(2, 1, 42)?;

    sheet.write_string(3, 0, "float")?;
    sheet.write_number(3, 1, 2.5)?;

    sheet.write_string(4, 0, "boolean")?;
    sheet.write_boolean(4, 1, true)?;

    let date = ExcelDateTime::from_ymd(2024, 6, 15)?;
    sheet.write_string(5, 0, "date")?;
    sheet.write_datetime_with_format(5, 1, &date, &date_format)?;

    sheet.write_string(6, 0, "empty below")?;
    // A8/B8 are intentionally left unwritten.
    sheet.write_string(7, 0, "after empty")?;

    save(&mut workbook, dir, "basic_types.xlsx")
}

fn formulas(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let summary = workbook.add_worksheet();
    summary.set_name("Summary")?;
    summary.set_column_width(0, 18)?;
    summary.set_column_width(1, 30)?;

    summary.write_string(0, 0, "case")?;
    summary.write_string(0, 1, "formula")?;
    summary.write_string(0, 2, "cached")?;

    summary.write_string(1, 0, "arithmetic")?;
    summary.write_formula(1, 1, "=1+2*3")?;
    summary.set_formula_result(1, 1, "7");

    summary.write_string(2, 0, "SUM")?;
    summary.write_formula(2, 1, "=SUM(A10:A12)")?;
    summary.set_formula_result(2, 1, "60");

    summary.write_string(3, 0, "cross sheet")?;
    summary.write_formula(3, 1, "='Data Sheet'!B2")?;
    summary.set_formula_result(3, 1, "123");

    summary.write_string(4, 0, "IF")?;
    summary.write_formula(4, 1, "=IF(1>0,\"yes\",\"no\")")?;
    summary.set_formula_result(4, 1, "yes");

    summary.write_string(5, 0, "error")?;
    summary.write_formula(5, 1, "=1/0")?;
    summary.set_formula_result(5, 1, "#DIV/0!");

    summary.write_string(6, 0, "circular")?;
    summary.write_formula(6, 1, "=B7")?;
    summary.set_formula_result(6, 1, "0");

    summary.write_string(7, 0, "unsupported fn")?;
    summary.write_formula(7, 1, "=UNSUPPORTED_FN(1,2)")?;
    summary.set_formula_result(7, 1, "#NAME?");

    summary.write_string(8, 0, "SUM inputs")?;
    summary.write_number(9, 0, 10)?;
    summary.write_number(10, 0, 20)?;
    summary.write_number(11, 0, 30)?;

    let data = workbook.add_worksheet();
    data.set_name("Data Sheet")?;
    data.write_string(0, 0, "name")?;
    data.write_number(0, 1, 123)?;
    data.write_string(1, 0, "value")?;

    save(&mut workbook, dir, "formulas.xlsx")
}

fn styles(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Styles")?;
    sheet.set_column_width(0, 24)?;

    let bold = Format::new().set_bold();
    let red = Format::new().set_font_color(Color::Red);
    let blue = Format::new().set_font_color(Color::Blue);
    let bold_red = Format::new().set_bold().set_font_color(Color::Red);

    sheet.write_string_with_format(0, 0, "bold", &bold)?;
    sheet.write_string_with_format(1, 0, "red", &red)?;
    sheet.write_string_with_format(2, 0, "blue", &blue)?;
    sheet.write_string(3, 0, "normal")?;
    sheet.write_string_with_format(4, 0, "bold red", &bold_red)?;
    sheet.write_string(5, 0, "normal after styled")?;

    save(&mut workbook, dir, "styles.xlsx")
}

fn borders(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Borders")?;
    sheet.set_column_width(0, 16)?;
    sheet.set_column_width(1, 10)?;
    sheet.set_column_width(2, 14)?;

    let thin = Format::new().set_border(FormatBorder::Thin);
    let medium = Format::new().set_border(FormatBorder::Medium);
    let double = Format::new().set_border(FormatBorder::Double);
    let dashed = Format::new().set_border(FormatBorder::Dashed);
    let red = Format::new()
        .set_border(FormatBorder::Thin)
        .set_border_color(Color::Red);
    let outline = Format::new()
        .set_border_top(FormatBorder::Thick)
        .set_border_bottom(FormatBorder::Thick)
        .set_border_left(FormatBorder::Thick)
        .set_border_right(FormatBorder::Thick);

    // Bordered table. C2 is intentionally an empty cell with a border.
    sheet.write_string_with_format(0, 0, "Name", &thin)?;
    sheet.write_string_with_format(0, 1, "Value", &thin)?;
    sheet.write_string_with_format(0, 2, "Note", &thin)?;
    sheet.write_string_with_format(1, 0, "East", &thin)?;
    sheet.write_number_with_format(1, 1, 10, &thin)?;
    sheet.write_blank(1, 2, &thin)?;
    sheet.write_string_with_format(2, 0, "West", &thin)?;
    sheet.write_number_with_format(2, 1, 20, &thin)?;
    sheet.write_string_with_format(2, 2, "plain", &thin)?;

    // Individual border styles.
    sheet.write_string_with_format(4, 0, "medium", &medium)?;
    sheet.write_string_with_format(5, 0, "double", &double)?;
    sheet.write_string_with_format(6, 0, "dashed", &dashed)?;
    sheet.write_string_with_format(7, 0, "red", &red)?;
    sheet.write_string_with_format(8, 0, "outline", &outline)?;

    // Border around a merged range.
    sheet.merge_range(4, 1, 5, 2, "merged", &outline)?;

    // A bordered cell must stop text overflow from the cell to its left.
    let long = "this text must not cross the border";
    sheet.write_string(10, 0, long)?;
    sheet.write_string_with_format(10, 1, "blocked", &thin)?;

    save(&mut workbook, dir, "borders.xlsx")
}

fn merged_tail(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Merged Tail")?;
    sheet.set_column_width(0, 18)?;
    sheet.set_column_width(1, 18)?;
    sheet.set_column_width(2, 18)?;
    sheet.set_column_width(3, 18)?;
    sheet.write_string(0, 0, "merged tail")?;
    let wrap = Format::new().set_text_wrap();
    let paragraph = "結合範囲がシートの最終行を超えて存在するケースの確認用テキストです。アンカー行だけでなく、結合範囲全体の行に文章が分配され、最終行まで表示される必要があります。";
    sheet.merge_range(2, 0, 5, 3, paragraph, &wrap)?;
    save(&mut workbook, dir, "merged_tail.xlsx")
}

fn merged_newlines(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Merged")?;

    let plain = Format::new();
    let wrap = Format::new().set_text_wrap();

    sheet.merge_range(0, 0, 0, 2, "単一の行の結合", &plain)?;
    sheet.merge_range(2, 0, 4, 1, "複数行\n複数列\nの結合セル", &wrap)?;
    sheet.merge_range(6, 0, 6, 3, "複数列の結合", &plain)?;

    sheet.write_string_with_format(8, 0, "改行\nを含む\n文字列", &wrap)?;
    sheet.write_string(10, 0, "全角テキストと絵文字 😀 🚀")?;
    sheet.write_string(11, 0, "結合と改行\n😀\n全角")?;
    sheet.merge_range(13, 0, 14, 2, "結合内の\n改行と全角", &wrap)?;

    save(&mut workbook, dir, "merged_newlines.xlsx")
}

fn excel_layout(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Excel Layout")?;
    sheet.set_column_width(0, 24)?;
    sheet.set_column_width(1, 6)?;
    sheet.set_column_width(2, 12)?;
    sheet.set_row_height(0, 30)?;
    sheet.set_row_height(1, 45)?;
    sheet.set_row_height(2, 18)?;

    let wrap = Format::new().set_text_wrap();
    let long = "0123456789".repeat(8);

    sheet.write_string(0, 0, "explicit width 24")?;
    sheet.write_string(0, 1, "narrow")?;
    sheet.write_string(0, 2, "height 30")?;
    sheet.write_string(1, 0, "row height 45")?;
    sheet.write_string(1, 2, "third")?;
    sheet.write_string_with_format(2, 1, &long, &wrap)?;
    sheet.write_string(3, 3, "auto width")?;
    sheet.write_string(3, 4, "auto content that can overflow")?;
    sheet.merge_range(5, 3, 5, 4, "merged into unspecified columns", &wrap)?;

    let defaults = workbook.add_worksheet();
    defaults.set_name("Defaults")?;
    defaults.set_default_row_height(30);
    defaults.write_string(0, 0, "default row height")?;

    save(&mut workbook, dir, "excel_layout.xlsx")
}

fn overflow(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Overflow")?;

    let long = "0123456789".repeat(10);
    let plain = Format::new();

    sheet.write_string(0, 0, &long)?;
    sheet.write_string(0, 5, "end")?;

    sheet.write_string(1, 0, &long)?;
    sheet.write_string(1, 1, "blocked")?;
    sheet.write_string(1, 5, "end")?;

    sheet.write_string(2, 0, &long)?;
    sheet.merge_range(2, 2, 2, 3, "merged blocker", &plain)?;
    sheet.write_string(2, 5, "end")?;

    sheet.write_string(3, 0, "short")?;
    sheet.write_string(3, 1, "neighbor")?;

    sheet.write_string_with_format(4, 0, &long, &Format::new().set_text_wrap())?;
    sheet.write_string(4, 5, "end")?;

    // 80 characters overflows into four empty 8-column cells without needing
    // truncation (65 + 8 + 8 + 8 + 8 = 97 displayed columns).
    let medium = "abcdefghij".repeat(8);
    sheet.write_string(5, 0, &medium)?;
    sheet.write_string(5, 5, "end")?;

    save(&mut workbook, dir, "overflow.xlsx")
}

const FREEZE_DATA_ROWS: u32 = 100;
const FREEZE_DATA_COLS: u16 = 100;

fn write_freeze_data(sheet: &mut Worksheet, rows: u32, cols: u16) -> Result<(), XlsxError> {
    sheet.write_string(0, 0, "row")?;
    for col in 1..=cols {
        sheet.write_string(0, col, format!("C{col}"))?;
    }
    for row in 1..=rows {
        sheet.write_string(row, 0, format!("R{row}"))?;
        for col in 1..=cols {
            sheet.write_string(row, col, format!("R{row}C{col}"))?;
        }
    }
    Ok(())
}

fn freeze_rows(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Freeze Rows")?;
    write_freeze_data(sheet, FREEZE_DATA_ROWS, FREEZE_DATA_COLS)?;
    sheet.set_freeze_panes(2, 0)?;
    save(&mut workbook, dir, "freeze_rows.xlsx")
}

fn freeze_both(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Freeze Both")?;
    write_freeze_data(sheet, FREEZE_DATA_ROWS, FREEZE_DATA_COLS)?;
    sheet.set_freeze_panes(1, 1)?;
    save(&mut workbook, dir, "freeze_both.xlsx")
}

fn freeze_merge_conflict(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Freeze Merge")?;
    write_freeze_data(sheet, FREEZE_DATA_ROWS, FREEZE_DATA_COLS)?;
    // The freeze boundary after row 2 intersects this merge (rows 2-3).
    sheet.merge_range(1, 0, 2, 1, "crosses freeze boundary", &Format::new())?;
    sheet.set_freeze_panes(2, 0)?;
    save(&mut workbook, dir, "freeze_merge_conflict.xlsx")
}

fn write_filter_data(sheet: &mut Worksheet, rows: u32) -> Result<(), XlsxError> {
    sheet.write_string(0, 0, "Region")?;
    sheet.write_string(0, 1, "Sales")?;
    let regions = ["East", "West", "North", "South"];
    for row in 1..=rows {
        let region = regions[(row as usize - 1) % regions.len()];
        sheet.write_string(row, 0, region)?;
        sheet.write_number(row, 1, row as f64 * 5.0)?;
    }
    Ok(())
}

fn filters_values(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Value List")?;
    write_filter_data(sheet, 20)?;
    sheet.autofilter(0, 0, 20, 1)?;
    sheet.filter_column(
        0,
        &FilterCondition::new()
            .add_list_filter("East")
            .add_list_filter("West"),
    )?;
    save(&mut workbook, dir, "filters_values.xlsx")
}

fn filters_custom(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Custom")?;
    write_filter_data(sheet, 20)?;
    sheet.autofilter(0, 0, 20, 1)?;
    sheet.filter_column(
        1,
        &FilterCondition::new().add_custom_filter(FilterCriteria::GreaterThan, 10.0),
    )?;
    save(&mut workbook, dir, "filters_custom.xlsx")
}

fn filters_table(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Table Filter")?;
    sheet.write_string(0, 0, "Region")?;
    sheet.write_string(0, 1, "Product")?;
    sheet.write_string(0, 2, "Sales")?;
    let regions = ["East", "West", "North", "South"];
    let products = ["Widget", "Gadget", "Gizmo"];
    for row in 1..=15usize {
        sheet.write_string(row as u32, 0, regions[(row - 1) % regions.len()])?;
        sheet.write_string(row as u32, 1, products[(row - 1) % products.len()])?;
        sheet.write_number(row as u32, 2, row as f64 * 100.0)?;
    }
    let table = Table::new();
    sheet.add_table(0, 0, 15, 2, &table)?;
    save(&mut workbook, dir, "filters_table.xlsx")
}

fn filter_merge_conflict(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Filter Merge")?;
    write_filter_data(sheet, 20)?;
    sheet.autofilter(0, 0, 20, 1)?;
    sheet.filter_column(0, &FilterCondition::new().add_list_filter("East"))?;
    // Excel normally prevents this; the fixture verifies that the loader
    // rejects the overlapping filter and keeps the sheet readable.
    sheet.merge_range(3, 0, 4, 1, "inside filter range", &Format::new())?;
    save(&mut workbook, dir, "filter_merge_conflict.xlsx")
}

fn multi_sheet(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;

    let summary = workbook.add_worksheet();
    summary.set_name("概要")?;
    summary.write_string(0, 0, "項目")?;
    summary.write_string(0, 1, "値")?;
    summary.write_string(1, 0, "データ")?;
    summary.write_formula(1, 1, "='データ'!B2")?;
    summary.set_formula_result(1, 1, "42");
    summary.write_string(2, 0, "説明")?;
    summary.write_string(2, 1, "日本語シート名の確認")?;

    let data = workbook.add_worksheet();
    data.set_name("データ")?;
    data.write_string(0, 0, "名前")?;
    data.write_number(0, 1, 42)?;
    data.write_string(1, 0, "東京")?;
    data.write_string(2, 0, "大阪")?;

    let empty = workbook.add_worksheet();
    empty.set_name("空シート")?;

    save(&mut workbook, dir, "multi_sheet.xlsx")
}

fn sparse(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Sparse")?;
    sheet.write_string(0, 0, "A1")?;
    sheet.write_number(49, 25, 50.0)?;
    sheet.write_string(99, 26, "AA100")?;
    save(&mut workbook, dir, "sparse.xlsx")
}

fn special_chars(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Special")?;
    let wrap = Format::new().set_text_wrap();

    sheet.write_string(0, 0, "tab\tseparated\tvalue")?;
    sheet.write_string(1, 0, "He said \"hello\" and 'bye'")?;
    sheet.write_string(2, 0, "line 1\nline 2\nline 3")?;
    sheet.write_string(3, 0, "日本語のテキスト")?;
    sheet.write_string(4, 0, "emoji 😀 🚀 ✨")?;
    sheet.write_string_with_format(5, 0, "複合\t\"quote\"\n日本語 😀", &wrap)?;
    sheet.write_string(6, 0, "ゼロ幅\u{200b}スペース")?;

    save(&mut workbook, dir, "special_chars.xlsx")
}

fn large_lazy(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Large")?;

    let headers = [
        "id", "name", "category", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9",
    ];
    for (col, header) in headers.iter().enumerate() {
        sheet.write_string(0, col as u16, *header)?;
    }

    let categories = ["alpha", "beta", "gamma", "delta"];
    const ROWS: u32 = 6_000;
    for row in 1..=ROWS {
        sheet.write_number(row, 0, row as f64)?;
        sheet.write_string(row, 1, format!("name-{row:05}"))?;
        sheet.write_string(row, 2, categories[(row as usize) % categories.len()])?;
        for col in 3..12u16 {
            sheet.write_number(row, col, ((row as u64 * col as u64) % 10_000) as f64 + 0.5)?;
        }
    }

    save(&mut workbook, dir, "large_lazy.xlsx")
}

fn empty_sheet(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Empty")?;
    save(&mut workbook, dir, "empty_sheet.xlsx")
}

fn single_cell(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Single")?;
    sheet.write_string(0, 0, "only")?;
    save(&mut workbook, dir, "single_cell.xlsx")
}

fn hidden_rows_columns(dir: &Path) -> FixtureResult {
    let mut workbook = new_workbook()?;
    let sheet = workbook.add_worksheet();
    sheet.set_name("Hidden")?;
    sheet.write_string(0, 0, "visible header")?;
    sheet.write_string(0, 1, "hidden column B")?;
    sheet.write_string(0, 2, "visible C")?;
    for row in 1..=12u32 {
        sheet.write_number(row, 0, row as f64)?;
        sheet.write_string(row, 1, format!("hidden-{row}"))?;
        sheet.write_number(row, 2, row as f64 * 10.0)?;
    }
    sheet.set_row_hidden(1)?;
    sheet.set_row_hidden(3)?;
    sheet.set_column_hidden(1)?;
    save(&mut workbook, dir, "hidden_or_very_hidden.xlsx")
}
