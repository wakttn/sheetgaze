use std::path::{Path, PathBuf};

use rust_xlsxwriter::{Format, Workbook};
use sheetgaze::app::action::Action;
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::xlsx::load_xlsx;
use sheetgaze::io::xlsx_write;

fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheetgaze-structure-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn unique(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir().join(format!("{}-{}-{name}", std::process::id(), id))
}

fn load(path: &Path) -> sheetgaze::domain::workbook::WorkbookModel {
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

fn state(path: &Path) -> AppState {
    let mut state = AppState::new(load(path), Config::default());
    state.set_size(80, 24);
    state.config.cli.clipboard_register = sheetgaze::config::ClipboardRegisterMode::Internal;
    state
}

fn sample(path: &Path) {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "A").unwrap();
    sheet.write_string(1, 0, "B").unwrap();
    sheet.write_string(2, 0, "C").unwrap();
    sheet.write_number(0, 1, 1.0).unwrap();
    sheet.write_number(1, 1, 2.0).unwrap();
    sheet.write_number(2, 1, 3.0).unwrap();
    workbook.save(path).unwrap();
}

fn display(state: &AppState, row: u32, col: u32) -> String {
    state.workbook.as_ref().unwrap().sheets[0]
        .effective_cell(CellAddr::new(row, col))
        .map(|cell| cell.display)
        .unwrap_or_default()
}

#[test]
fn insert_row_shifts_cells_and_saves() {
    let path = unique("insert-row.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.focus = CellAddr::new(1, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    assert_eq!(state.workbook.as_ref().unwrap().sheets[0].meta.row_count, 4);
    assert_eq!(display(&state, 0, 0), "A");
    assert_eq!(display(&state, 1, 0), "B");
    assert_eq!(display(&state, 2, 0), "");
    assert_eq!(display(&state, 3, 0), "C");

    let outcome = xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    assert_eq!(outcome.cells, 0);
    state.normalize_after_save();
    let reloaded = load(&path);
    assert_eq!(
        reloaded.sheets[0]
            .effective_cell(CellAddr::new(3, 0))
            .unwrap()
            .display,
        "C"
    );
}

#[test]
fn cut_row_then_paste_before_restores_it() {
    let path = unique("cut-paste-row.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.focus = CellAddr::new(1, 0);
    let effects = state.apply(Action::CutRow);
    assert!(matches!(
        effects.as_slice(),
        [sheetgaze::app::action::Effect::WriteClipboard { .. }]
    ));
    assert_eq!(state.workbook.as_ref().unwrap().sheets[0].meta.row_count, 2);
    assert_eq!(display(&state, 1, 0), "C");

    let block = state
        .registers
        .get(sheetgaze::domain::register::PasteSource::Unnamed)
        .cloned()
        .unwrap();
    state.focus = CellAddr::new(1, 0);
    state.paste_internal_block(block, true);
    assert_eq!(state.workbook.as_ref().unwrap().sheets[0].meta.row_count, 3);
    assert_eq!(display(&state, 1, 0), "B");
    assert_eq!(display(&state, 2, 0), "C");

    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();
    let reloaded = load(&path);
    assert_eq!(
        reloaded.sheets[0]
            .effective_cell(CellAddr::new(1, 0))
            .unwrap()
            .display,
        "B"
    );
}

#[test]
fn cut_column_and_paste_restores_it() {
    let path = unique("cut-paste-col.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.focus = CellAddr::new(0, 1);
    state.apply(Action::ToggleColumnVisual);
    let effects = state.apply(Action::CutSelection);
    assert!(matches!(
        effects.as_slice(),
        [sheetgaze::app::action::Effect::WriteClipboard { .. }]
    ));
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0].meta.column_count,
        1
    );

    let block = state
        .registers
        .get(sheetgaze::domain::register::PasteSource::Unnamed)
        .cloned()
        .unwrap();
    state.focus = CellAddr::new(0, 0);
    state.paste_internal_block(block, false);
    assert_eq!(display(&state, 0, 1), "1");
    assert_eq!(display(&state, 2, 1), "3");
}

#[test]
fn insert_row_updates_formula_references() {
    let path = unique("formula-insert.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_number(1, 0, 2.0).unwrap();
    sheet.write_formula(0, 1, "=A2").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = CellAddr::new(0, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    state.workbook.as_mut().unwrap().sheets[0]
        .formula_overlay
        .values
        .clear();
    let formula = state.workbook.as_ref().unwrap().sheets[0]
        .effective_cell(CellAddr::new(0, 1))
        .unwrap();
    assert_eq!(formula.formula.unwrap().expression, "=A3");

    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();
    let reloaded = load(&path);
    let formula = reloaded.sheets[0]
        .raw_cell(CellAddr::new(0, 1))
        .unwrap()
        .formula
        .as_ref()
        .unwrap();
    assert_eq!(formula.expression, "=A3");
}

#[test]
fn undo_redo_structure_and_cells() {
    let path = unique("undo-structure.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.focus = CellAddr::new(1, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    assert_eq!(display(&state, 2, 0), "");
    state.apply(Action::Undo);
    assert_eq!(state.workbook.as_ref().unwrap().sheets[0].meta.row_count, 3);
    assert_eq!(display(&state, 1, 0), "B");
    state.apply(Action::Redo);
    assert_eq!(state.workbook.as_ref().unwrap().sheets[0].meta.row_count, 4);
    assert_eq!(display(&state, 2, 0), "");
}

#[test]
fn cross_sheet_formula_is_rewritten_on_save() {
    let path = unique("cross-sheet.xlsx");
    let mut workbook = Workbook::new();
    let sheet1 = workbook.add_worksheet();
    sheet1.write_number(0, 0, 1.0).unwrap();
    sheet1.write_number(1, 0, 2.0).unwrap();
    let sheet2 = workbook.add_worksheet();
    sheet2.write_formula(0, 0, "=Sheet1!A2").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = CellAddr::new(0, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();

    let reloaded = load(&path);
    let formula = reloaded.sheets[1]
        .raw_cell(CellAddr::new(0, 0))
        .unwrap()
        .formula
        .as_ref()
        .unwrap();
    assert_eq!(formula.expression, "=Sheet1!A3");
}

#[test]
fn lazy_cross_sheet_formula_is_rewritten_on_save() {
    let path = unique("lazy-cross-sheet.xlsx");
    let mut workbook = Workbook::new();
    let sheet1 = workbook.add_worksheet();
    sheet1.write_number(0, 0, 1.0).unwrap();
    sheet1.write_number(1, 0, 2.0).unwrap();
    let sheet2 = workbook.add_worksheet();
    sheet2.write_formula(0, 0, "=Sheet1!A2").unwrap();
    workbook.save(&path).unwrap();

    let workbook = load_xlsx(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: true,
            threshold_cells: 0,
            threshold_bytes: 0,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    assert!(matches!(
        workbook.sheets[1].data,
        sheetgaze::domain::sheet::SheetData::Lazy(_)
    ));
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state.focus = CellAddr::new(0, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();

    let reloaded = load(&path);
    let formula = reloaded.sheets[1]
        .raw_cell(CellAddr::new(0, 0))
        .unwrap()
        .formula
        .as_ref()
        .unwrap();
    assert_eq!(formula.expression, "=Sheet1!A3");
}

#[test]
fn defined_name_is_rewritten_on_save() {
    let path = unique("defined-name.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_number(1, 0, 2.0).unwrap();
    workbook.define_name("Second", "=Sheet1!$A$2").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = CellAddr::new(0, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();

    let file = std::fs::File::open(&path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut xml = String::new();
    use std::io::Read;
    archive
        .by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Sheet1!$A$3"), "workbook.xml was: {xml}");
}

#[test]
fn clipboard_rich_roundtrip_preserves_formula() {
    let path = unique("clipboard-rich.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_formula(0, 1, "=A1+1").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = CellAddr::new(0, 1);
    let effects = state.apply(Action::Copy);
    let sheetgaze::app::action::Effect::WriteClipboard { text, .. } =
        effects.into_iter().next().unwrap()
    else {
        panic!("expected clipboard write");
    };
    state.note_clipboard_write(text.clone(), true);
    state.focus = CellAddr::new(1, 1);
    state.paste_clipboard_text(&text, false);
    let cell = state.workbook.as_ref().unwrap().sheets[0]
        .effective_cell(CellAddr::new(1, 1))
        .unwrap();
    assert!(cell.formula.is_some(), "rich paste should keep the formula");
}

#[test]
fn clipboard_external_tsv_is_parsed_as_values() {
    let path = unique("clipboard-tsv.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.focus = CellAddr::new(1, 0);
    state.paste_clipboard_text("x\ty\r\nz\t2", false);
    assert_eq!(display(&state, 1, 0), "x");
    assert_eq!(display(&state, 1, 1), "y");
    assert_eq!(display(&state, 2, 0), "z");
    assert_eq!(display(&state, 2, 1), "2");
}

#[test]
fn unnamedplus_paste_reads_system_clipboard() {
    let path = unique("clipboard-effect.xlsx");
    sample(&path);
    let mut state = state(&path);
    state.config.cli.clipboard_register = sheetgaze::config::ClipboardRegisterMode::UnnamedPlus;
    let effects = state.apply(Action::Paste {
        before: false,
        source: sheetgaze::domain::register::PasteSource::Unnamed,
    });
    assert!(matches!(
        effects.as_slice(),
        [sheetgaze::app::action::Effect::ReadClipboard {
            fallback: Some(sheetgaze::domain::register::PasteSource::Unnamed),
            ..
        }]
    ));
}

#[test]
fn structural_save_preserves_merge_filter_and_freeze() {
    let path = unique("preserve.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .merge_range(0, 0, 0, 1, "merged", &Format::new().set_bold())
        .unwrap();
    sheet.write_string(1, 0, "head").unwrap();
    sheet.write_number(1, 1, 1.0).unwrap();
    sheet.write_number(2, 0, 2.0).unwrap();
    sheet.write_number(2, 1, 2.0).unwrap();
    sheet.autofilter(1, 0, 2, 1).unwrap();
    sheet.set_freeze_panes(1, 0).unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = CellAddr::new(1, 0);
    state.apply(Action::InsertRows {
        before: false,
        enter_insert: false,
        count: 1,
    });
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    state.normalize_after_save();

    let reloaded = load(&path);
    let sheet = &reloaded.sheets[0];
    assert_eq!(sheet.merges.regions().len(), 1);
    assert_eq!(sheet.filters.len(), 1);
    assert_eq!(sheet.freeze.frozen_rows, 1);
    assert_eq!(
        sheet.effective_cell(CellAddr::new(0, 0)).unwrap().display,
        "merged"
    );
}
