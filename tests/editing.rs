use std::path::{Path, PathBuf};

use rust_xlsxwriter::{FilterCondition, Format, Workbook};
use sheetgaze::app::action::{Action, Effect};
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::edit::{EditCommit, EditCursor};
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::xlsx::load_xlsx;
use sheetgaze::io::xlsx_write;

fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheetgaze-editing-tests");
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
    state
}

fn type_text(state: &mut AppState, text: &str) {
    for ch in text.chars() {
        state.apply(Action::EditInput(ch));
    }
}

#[test]
fn edit_undo_redo_and_save_roundtrip() {
    let path = unique("edit-roundtrip.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .write_string_with_format(0, 0, "old", &Format::new().set_bold())
        .unwrap();
    sheet.write_number(1, 1, 10.0).unwrap();
    sheet
        .merge_range(3, 0, 3, 1, "merged", &Format::new())
        .unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::EditClear);
    type_text(&mut state, "new value");
    state.apply(Action::EditCommit(EditCommit::Down));
    let cell = state.workbook.as_ref().unwrap().sheets[0]
        .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
        .unwrap();
    assert_eq!(cell.display, "new value");
    assert!(cell.style.bold, "style must be preserved");
    assert_eq!(state.focus.a1(), "A2");
    assert!(state.is_dirty());

    state.apply(Action::Undo);
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "old"
    );
    state.apply(Action::Redo);
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "new value"
    );

    let effects = state.apply(Action::Save {
        force: false,
        quit_after: false,
    });
    assert!(matches!(effects.as_slice(), [Effect::Save { .. }]));
    let workbook = state.workbook.as_ref().unwrap();
    let outcome = xlsx_write::save_workbook(workbook).unwrap();
    assert_eq!(outcome.cells, 1);
    state.edit_log.mark_saved();
    assert!(!state.is_dirty());

    let reloaded = load(&path);
    let cell = reloaded.sheets[0]
        .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
        .unwrap();
    assert_eq!(cell.display, "new value");
    assert!(cell.style.bold);
    assert_eq!(reloaded.sheets[0].merges.regions().len(), 1);
    assert_eq!(reloaded.sheets[0].used_range.end.row, 3);
}

#[test]
fn formula_input_and_clearing_row() {
    let path = unique("edit-formula.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_number(0, 1, 2.0).unwrap();
    sheet.write_number(0, 2, 3.0).unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = sheetgaze::domain::addr::CellAddr::new(0, 3);
    state.apply(Action::EditFormula);
    type_text(&mut state, "SUM(A1:C1)");
    state.apply(Action::EditCommit(EditCommit::Stay));
    let cell = state.workbook.as_ref().unwrap().sheets[0]
        .raw_cell(sheetgaze::domain::addr::CellAddr::new(0, 3))
        .unwrap();
    assert_eq!(cell.formula.as_ref().unwrap().expression, "=SUM(A1:C1)");

    state.focus = sheetgaze::domain::addr::CellAddr::new(0, 0);
    state.apply(Action::ClearRow);
    assert!(state.is_dirty());
    for col in 0..3 {
        let cell = state.workbook.as_ref().unwrap().sheets[0]
            .raw_cell(sheetgaze::domain::addr::CellAddr::new(0, col))
            .unwrap();
        assert!(cell.raw.is_empty());
    }
    state.apply(Action::Undo);
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .raw_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .raw,
        sheetgaze::domain::cell::CellValue::Number(1.0)
    );
}

#[test]
fn cross_sheet_search_moves_between_sheets() {
    let path = unique("cross-search.xlsx");
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("First").unwrap();
    first.write_string(0, 0, "needle").unwrap();
    let second = workbook.add_worksheet();
    second.set_name("Second").unwrap();
    second.write_string(0, 2, "needle").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::SearchOpen);
    for ch in "needle".chars() {
        state.apply(Action::SearchInput(ch));
    }
    state.apply(Action::SearchSubmit);
    assert_eq!(state.active_sheet_index(), 0);
    assert_eq!(state.focus.a1(), "A1");
    state.apply(Action::SearchNext);
    assert_eq!(state.active_sheet_index(), 1);
    assert_eq!(state.focus.a1(), "C1");
    state.apply(Action::SearchPrevious);
    assert_eq!(state.active_sheet_index(), 0);
    assert_eq!(state.focus.a1(), "A1");
}

#[test]
fn substitute_sheet_and_workbook() {
    let path = unique("replace.xlsx");
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("One").unwrap();
    first.write_string(0, 0, "East").unwrap();
    first.write_string(1, 0, "East").unwrap();
    let second = workbook.add_worksheet();
    second.set_name("Two").unwrap();
    second.write_string(0, 0, "East").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::CommandOpen);
    for ch in "%s/East/West/g".chars() {
        state.apply(Action::CommandInput(ch));
    }
    state.apply(Action::CommandSubmit);
    let first_sheet = &state.workbook.as_ref().unwrap().sheets[0];
    assert_eq!(
        first_sheet
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "West"
    );
    assert_eq!(
        first_sheet
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(1, 0))
            .unwrap()
            .display,
        "West"
    );
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[1]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "East"
    );

    state.apply(Action::Undo);
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "East"
    );

    state.apply(Action::CommandOpen);
    for ch in "gs/East/West/g".chars() {
        state.apply(Action::CommandInput(ch));
    }
    state.apply(Action::CommandSubmit);
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "West"
    );
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[1]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "West"
    );
}

#[test]
fn goto_switches_sheets_and_cells() {
    let path = unique("goto.xlsx");
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("One").unwrap();
    first.write_string(0, 0, "a").unwrap();
    let second = workbook.add_worksheet();
    second.set_name("Two").unwrap();
    second.write_string(2, 1, "b").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::CommandOpen);
    for ch in "goto Two!B3".chars() {
        state.apply(Action::CommandInput(ch));
    }
    state.apply(Action::CommandSubmit);
    assert_eq!(state.active_sheet_index(), 1);
    assert_eq!(state.focus.a1(), "B3");

    state.apply(Action::JumpBack);
    assert_eq!(state.active_sheet_index(), 0);
    assert_eq!(state.focus.a1(), "A1");
    state.apply(Action::JumpForward);
    assert_eq!(state.active_sheet_index(), 1);
    assert_eq!(state.focus.a1(), "B3");
}

#[test]
fn save_preserves_filters_and_formulas() {
    let path = unique("save-preserves.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "Name").unwrap();
    sheet.write_string(0, 1, "Value").unwrap();
    sheet.write_string(1, 0, "East").unwrap();
    sheet.write_number(1, 1, 10.0).unwrap();
    sheet.write_string(2, 0, "West").unwrap();
    sheet.write_number(2, 1, 20.0).unwrap();
    sheet.write_formula(3, 1, "=SUM(B2:B3)").unwrap();
    sheet.autofilter(0, 0, 3, 1).unwrap();
    sheet
        .filter_column(0, &FilterCondition::new().add_list_filter("East"))
        .unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = sheetgaze::domain::addr::CellAddr::new(1, 0);
    state.apply(Action::EditClear);
    type_text(&mut state, "Changed");
    state.apply(Action::EditCommit(EditCommit::Stay));
    let outcome = xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    assert_eq!(outcome.cells, 1);

    let reloaded = load(&path);
    let sheet = &reloaded.sheets[0];
    assert_eq!(
        sheet
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(1, 0))
            .unwrap()
            .display,
        "Changed"
    );
    assert!(
        sheet
            .raw_cell(sheetgaze::domain::addr::CellAddr::new(3, 1))
            .unwrap()
            .formula
            .is_some()
    );
    assert_eq!(sheet.filters.len(), 1);
    assert_eq!(
        sheet
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "Name"
    );
}

#[test]
fn cross_sheet_search_highlight_is_scoped_to_active_sheet() {
    let path = unique("cross-search-highlight.xlsx");
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("First").unwrap();
    first.write_string(0, 0, "hit").unwrap();
    let second = workbook.add_worksheet();
    second.set_name("Second").unwrap();
    second.write_string(0, 0, "hit").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::SearchOpen);
    for ch in "hit".chars() {
        state.apply(Action::SearchInput(ch));
    }
    state.apply(Action::SearchSubmit);
    let view = state.view();
    assert_eq!(view.search.as_ref().unwrap().matches.len(), 1);
    assert_eq!(state.focus.a1(), "A1");
    assert_eq!(state.active_sheet_index(), 0);

    state.apply(Action::SearchNext);
    let view = state.view();
    assert_eq!(state.active_sheet_index(), 1);
    assert_eq!(view.search.as_ref().unwrap().matches.len(), 1);
    assert_eq!(view.search.as_ref().unwrap().current, Some(state.focus));
}

#[test]
fn unsaved_edits_feed_formula_recalculation() {
    let path = unique("recalc.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_formula(0, 1, "=A1+1").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::EditClear);
    type_text(&mut state, "5");
    state.apply(Action::EditCommit(EditCommit::Stay));

    let temp = xlsx_write::write_temp_workbook(state.workbook.as_ref().unwrap()).unwrap();
    let results = sheetgaze::io::formula::recalculate(
        temp.path(),
        &[(
            "Sheet1".to_string(),
            sheetgaze::domain::addr::CellAddr::new(0, 1),
        )],
        &sheetgaze::domain::task::CancelToken::new(),
        1,
    )
    .unwrap();
    assert_eq!(results.updates.len(), 1);
    assert_eq!(
        results.updates[0].value,
        sheetgaze::domain::cell::CellValue::Number(6.0)
    );
}

#[test]
fn save_keeps_untouched_zip_entries() {
    let path = unique("zip-preserve.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "a").unwrap();
    sheet.write_string(0, 1, "b").unwrap();
    workbook.save(&path).unwrap();

    let names = |path: &Path| {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let before = names(&path);

    let mut state = state(&path);
    state.apply(Action::EditClear);
    type_text(&mut state, "changed");
    state.apply(Action::EditCommit(EditCommit::Stay));
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    let after = names(&path);
    for name in &before {
        assert!(after.contains(name), "missing zip entry: {name}");
    }
}

#[test]
fn lazy_sheet_edit_and_save() {
    let path = unique("lazy-edit.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "first").unwrap();
    sheet.write_string(50, 0, "far").unwrap();
    workbook.save(&path).unwrap();

    let workbook = load_xlsx(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: true,
            threshold_bytes: 0,
            threshold_cells: 0,
            row_cache_rows: 8,
            prefetch_rows: 0,
        },
    )
    .unwrap();
    assert!(matches!(
        workbook.sheets[0].data,
        sheetgaze::domain::sheet::SheetData::Lazy(_)
    ));
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state.focus = sheetgaze::domain::addr::CellAddr::new(50, 0);
    state.apply(Action::EditClear);
    type_text(&mut state, "changed");
    state.apply(Action::EditCommit(EditCommit::Stay));
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(50, 0))
            .unwrap()
            .display,
        "changed"
    );
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();
    let reloaded = load(&path);
    assert_eq!(
        reloaded.sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(50, 0))
            .unwrap()
            .display,
        "changed"
    );
}

#[test]
fn substitute_flags_count_only_and_case_folding() {
    let path = unique("replace-flags.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "East").unwrap();
    sheet.write_string(1, 0, "east").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::CommandOpen);
    for ch in "%s/east/West/n".chars() {
        state.apply(Action::CommandInput(ch));
    }
    state.apply(Action::CommandSubmit);
    assert!(!state.is_dirty());
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "East"
    );

    state.apply(Action::CommandOpen);
    for ch in "%s/east/West/gi".chars() {
        state.apply(Action::CommandInput(ch));
    }
    state.apply(Action::CommandSubmit);
    assert!(state.is_dirty());
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
            .unwrap()
            .display,
        "West"
    );
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(1, 0))
            .unwrap()
            .display,
        "West"
    );
}

#[test]
fn formula_runner_recalculates_from_unsaved_edits() {
    let path = unique("runner-recalc.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 2.0).unwrap();
    sheet.write_formula(0, 1, "=A1*3").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::EditClear);
    type_text(&mut state, "4");
    state.apply(Action::EditCommit(EditCommit::Stay));
    let temp = xlsx_write::write_temp_workbook(state.workbook.as_ref().unwrap()).unwrap();
    let mut runner = sheetgaze::app::formula_runner::FormulaRunner::new();
    let start = runner.restart_with_temp(
        state.workbook.as_mut().unwrap(),
        &Config::default().recalc,
        temp,
    );
    assert!(start.started);
    let message = runner.wait().unwrap();
    let sheetgaze::io::formula::FormulaMessage::Results(results) = message else {
        panic!("expected formula results");
    };
    assert_eq!(
        results.updates[0].value,
        sheetgaze::domain::cell::CellValue::Number(12.0)
    );
}

#[test]
fn saved_formula_keeps_the_calculated_cache() {
    let path = unique("save-formula-cache.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 2.0).unwrap();
    sheet.write_formula(0, 1, "=A1*3").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::EditClear);
    type_text(&mut state, "4");
    state.apply(Action::EditCommit(EditCommit::Stay));
    let temp = xlsx_write::write_temp_workbook(state.workbook.as_ref().unwrap()).unwrap();
    let results = sheetgaze::io::formula::recalculate(
        temp.path(),
        &[(
            "Sheet1".to_string(),
            sheetgaze::domain::addr::CellAddr::new(0, 1),
        )],
        &sheetgaze::domain::task::CancelToken::new(),
        1,
    )
    .unwrap();
    state
        .workbook
        .as_mut()
        .unwrap()
        .apply_formula_results(results);
    xlsx_write::save_workbook(state.workbook.as_ref().unwrap()).unwrap();

    let reloaded = load(&path);
    assert_eq!(
        reloaded.sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 1))
            .unwrap()
            .display,
        "12"
    );
}

#[test]
fn stale_formula_results_do_not_overwrite_a_cleared_formula() {
    let path = unique("stale-formula-edit.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_number(0, 0, 1.0).unwrap();
    sheet.write_formula(0, 1, "=A1+1").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.focus = sheetgaze::domain::addr::CellAddr::new(0, 1);
    state.apply(Action::EditClear);
    type_text(&mut state, "99");
    state.apply(Action::EditCommit(EditCommit::Stay));
    let generation = state.workbook.as_ref().unwrap().generation.0;
    state.apply_formula_results(sheetgaze::domain::formula::FormulaResults {
        generation: generation.saturating_sub(1),
        updates: vec![sheetgaze::domain::formula::FormulaCellResult {
            sheet: "Sheet1".to_string(),
            addr: sheetgaze::domain::addr::CellAddr::new(0, 1),
            value: sheetgaze::domain::cell::CellValue::Number(2.0),
            status: sheetgaze::domain::formula::FormulaStatus::Calculated,
        }],
    });
    assert!(
        state.workbook.as_ref().unwrap().sheets[0]
            .formula_overlay
            .values
            .is_empty()
    );
    assert_eq!(
        state.workbook.as_ref().unwrap().sheets[0]
            .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 1))
            .unwrap()
            .display,
        "99"
    );
}

#[test]
fn search_word_crosses_sheets() {
    let path = unique("search-word.xlsx");
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("First").unwrap();
    first.write_string(0, 0, "hit").unwrap();
    first.write_string(1, 0, "hit").unwrap();
    let second = workbook.add_worksheet();
    second.set_name("Second").unwrap();
    second.write_string(2, 0, "hit").unwrap();
    workbook.save(&path).unwrap();

    let mut state = state(&path);
    state.apply(Action::SearchWord(true));
    assert_eq!(state.active_sheet_index(), 0);
    assert_eq!(state.focus.a1(), "A2");
    state.apply(Action::SearchWord(true));
    assert_eq!(state.active_sheet_index(), 1);
    assert_eq!(state.focus.a1(), "A3");
}

#[test]
fn csv_is_read_only_for_editing() {
    let path = unique("read-only.csv");
    std::fs::write(&path, "a,b\n1,2\n").unwrap();
    let workbook = sheetgaze::io::csv::load_csv(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    assert!(state.read_only);
    state.apply(Action::EditStart(EditCursor::Start));
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Normal);
}
