use rust_xlsxwriter::{Color, ConditionalFormatCell, ConditionalFormatCellRule, Format, Workbook};
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::row_store::{LazyConfig, RowRange};
use sheetgaze::domain::style::RgbColor;
use sheetgaze::io::xlsx::load_xlsx;

fn load(path: &std::path::Path) -> sheetgaze::domain::workbook::WorkbookModel {
    load_xlsx(
        path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap()
}

#[test]
fn number_formats_are_applied_to_cell_display() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("number-formats.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .write_number_with_format(0, 0, 0.572, &Format::new().set_num_format("0.0%"))
        .unwrap();
    sheet
        .write_number_with_format(
            1,
            0,
            1042.0,
            &Format::new().set_num_format("#,##0\"百万円\""),
        )
        .unwrap();
    sheet
        .write_number_with_format(2, 0, 2400.0, &Format::new().set_num_format("#,##0"))
        .unwrap();
    sheet
        .write_number_with_format(3, 0, 1234.5, &Format::new().set_num_format("#,##0.00"))
        .unwrap();
    sheet
        .write_number_with_format(4, 0, 0.3, &Format::new().set_num_format("0.0%"))
        .unwrap();
    sheet
        .write_number_with_format(5, 0, 1.0, &Format::new().set_num_format("0.0%"))
        .unwrap();
    workbook.save(&path).unwrap();

    let workbook = load(&path);
    let sheet = &workbook.sheets[0];
    let display = |row| {
        sheet
            .cell(CellAddr::new(row, 0))
            .expect("cell")
            .display
            .clone()
    };
    assert_eq!(display(0), "57.2%");
    assert_eq!(display(1), "1,042百万円");
    assert_eq!(display(2), "2,400");
    assert_eq!(display(3), "1,234.50");
    assert_eq!(display(4), "30.0%");
    assert_eq!(display(5), "100.0%");
}

#[test]
fn conditional_formatting_styles_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conditional.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (row, value) in [0.85, 0.7, 0.5].into_iter().enumerate() {
        sheet.write_number(row as u32, 0, value).unwrap();
    }

    let red = Format::new()
        .set_font_color(Color::RGB(0x9C0006))
        .set_background_color(Color::RGB(0xFFC7CE));
    let orange = Format::new()
        .set_font_color(Color::RGB(0x9C6500))
        .set_background_color(Color::RGB(0xFFEB9C));
    let green = Format::new()
        .set_font_color(Color::RGB(0x006100))
        .set_background_color(Color::RGB(0xC6EFCE));

    sheet
        .add_conditional_format(
            0,
            0,
            2,
            0,
            &ConditionalFormatCell::new()
                .set_rule(ConditionalFormatCellRule::GreaterThanOrEqualTo(0.8))
                .set_format(&red),
        )
        .unwrap();
    sheet
        .add_conditional_format(
            0,
            0,
            2,
            0,
            &ConditionalFormatCell::new()
                .set_rule(ConditionalFormatCellRule::Between(0.65, 0.7999))
                .set_format(&orange),
        )
        .unwrap();
    sheet
        .add_conditional_format(
            0,
            0,
            2,
            0,
            &ConditionalFormatCell::new()
                .set_rule(ConditionalFormatCellRule::LessThan(0.65))
                .set_format(&green),
        )
        .unwrap();
    workbook.save(&path).unwrap();

    let workbook = load(&path);
    let sheet = &workbook.sheets[0];
    let style = |row| {
        sheet
            .effective_cell(CellAddr::new(row, 0))
            .expect("cell")
            .style
    };
    assert_eq!(style(0).color, Some(RgbColor::new(0x9C, 0x00, 0x06)));
    assert_eq!(style(0).fill, Some(RgbColor::new(0xFF, 0xC7, 0xCE)));
    assert_eq!(style(1).color, Some(RgbColor::new(0x9C, 0x65, 0x00)));
    assert_eq!(style(1).fill, Some(RgbColor::new(0xFF, 0xEB, 0x9C)));
    assert_eq!(style(2).color, Some(RgbColor::new(0x00, 0x61, 0x00)));
    assert_eq!(style(2).fill, Some(RgbColor::new(0xC6, 0xEF, 0xCE)));
}

#[test]
fn empty_filled_cells_are_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty-fills.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let fill = Format::new().set_background_color(Color::RGB(0xD9E2F3));
    sheet.write_string_with_format(0, 0, "x", &fill).unwrap();
    sheet.write_blank(1, 0, &fill).unwrap();
    sheet.write_blank(2, 2, &fill).unwrap();
    workbook.save(&path).unwrap();

    for lazy in [false, true] {
        let lazy_config = if lazy {
            LazyConfig {
                threshold_cells: 0,
                ..LazyConfig::default()
            }
        } else {
            LazyConfig {
                enabled: false,
                ..LazyConfig::default()
            }
        };
        let mut workbook = load_xlsx(&path, &Default::default(), &lazy_config).unwrap();
        workbook.ensure_rows(0, RowRange::new(0, 3)).unwrap();
        let sheet = &workbook.sheets[0];
        let expected = Some(RgbColor::new(0xD9, 0xE2, 0xF3));
        for addr in [
            CellAddr::new(0, 0),
            CellAddr::new(1, 0),
            CellAddr::new(2, 2),
        ] {
            let cell = sheet.effective_cell(addr).expect("cell should be loaded");
            assert_eq!(cell.style.fill, expected, "missing fill at {addr:?}");
        }
        assert_eq!(sheet.used_range.end, CellAddr::new(2, 2));
    }
}

#[test]
fn solid_cell_fills_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fills.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let format = Format::new()
        .set_font_color(Color::RGB(0x1F3864))
        .set_background_color(Color::RGB(0xD9E2F3));
    sheet
        .write_string_with_format(0, 0, "header", &format)
        .unwrap();
    workbook.save(&path).unwrap();

    let workbook = load(&path);
    let sheet = &workbook.sheets[0];
    let cell = sheet.cell(CellAddr::new(0, 0)).expect("cell");
    assert_eq!(cell.style.fill, Some(RgbColor::new(0xD9, 0xE2, 0xF3)));
}
