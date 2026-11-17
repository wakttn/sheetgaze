use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_xlsxwriter::{FilterCondition, Format, Workbook};
use sheetgaze::app::action::{Action, Effect, Motion};
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::csv::load_csv;
use sheetgaze::io::xlsx::load_xlsx;

fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheetgaze-app-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn unique(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir().join(format!("{}-{}-{name}", std::process::id(), id))
}

fn csv_state() -> AppState {
    let path = unique("app.csv");
    std::fs::write(&path, "Name,Value\nEast,10\nWest,20\nEast,30\n").unwrap();
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
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state
}

fn csv_state_with(content: &str, lazy: bool) -> AppState {
    let path = unique("data-nav.csv");
    std::fs::write(&path, content).unwrap();
    let workbook = load_csv(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: lazy,
            threshold_bytes: 0,
            threshold_cells: 0,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state
}

fn xlsx_state() -> AppState {
    let path = unique("app.xlsx");
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(0, 0, "Name").unwrap();
    worksheet.write_string(0, 1, "Value").unwrap();
    worksheet.write_string(1, 0, "East").unwrap();
    worksheet.write_number(1, 1, 10.0).unwrap();
    worksheet.write_string(2, 0, "West").unwrap();
    worksheet.write_number(2, 1, 20.0).unwrap();
    worksheet.write_string(3, 0, "East").unwrap();
    worksheet.write_number(3, 1, 30.0).unwrap();
    worksheet.write_string(5, 0, "Footer").unwrap();
    worksheet.set_freeze_panes(1, 0).unwrap();
    workbook.save(&path).unwrap();

    let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state
}

#[test]
fn search_moves_between_matches_and_copies() {
    let mut state = csv_state();
    state.apply(Action::SearchOpen);
    for ch in "East".chars() {
        state.apply(Action::SearchInput(ch));
    }
    state.apply(Action::SearchSubmit);
    assert_eq!(state.focus.a1(), "A2");
    state.apply(Action::SearchNext);
    assert_eq!(state.focus.a1(), "A4");
    let effects = state.apply(Action::Copy);
    let Effect::WriteClipboard(text) = effects.into_iter().next().unwrap() else {
        panic!("expected clipboard effect");
    };
    assert_eq!(text, "East");
}

#[test]
fn visual_selection_copies_tsv() {
    let mut state = csv_state();
    state.apply(Action::ToggleVisual);
    state.apply(Action::Extend(Motion::Down));
    state.apply(Action::Extend(Motion::Right));
    let effects = state.apply(Action::Copy);
    let Effect::WriteClipboard(text) = effects.into_iter().next().unwrap() else {
        panic!("expected clipboard effect");
    };
    assert!(text.contains('\t'));
    assert!(text.contains('\r'));
    assert!(text.contains("Name"));
    assert!(text.contains("East"));
}

#[test]
fn data_navigation_skips_empty_rows() {
    for lazy in [false, true] {
        let mut state = csv_state_with(",\n,\nAlpha,1\nBeta,2\n,\n", lazy);
        let (used_end, data_rows) = {
            let sheet = state.active_sheet().unwrap();
            (sheet.used_range.end.row, sheet.data_rows)
        };
        assert_eq!(data_rows, Some((2, 3)), "lazy={lazy}");
        assert_eq!(used_end, 4, "lazy={lazy}");

        state.apply(Action::Move(Motion::DataTop));
        assert_eq!(state.focus.a1(), "A3", "lazy={lazy}");
        state.apply(Action::Move(Motion::DataBottom));
        assert_eq!(state.focus.a1(), "A4", "lazy={lazy}");
        state.apply(Action::Move(Motion::Top));
        assert_eq!(state.focus.a1(), "A1", "lazy={lazy}");
        state.apply(Action::Move(Motion::Bottom));
        assert_eq!(state.focus.a1(), "A5", "lazy={lazy}");
    }
}

#[test]
fn data_navigation_ignores_merge_extended_used_range() {
    let path = unique("data-nav.xlsx");
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(3, 0, "Alpha").unwrap();
    worksheet.write_string(4, 0, "Beta").unwrap();
    worksheet
        .merge_range(4, 1, 7, 1, "Merged", &Format::new())
        .unwrap();
    workbook.save(&path).unwrap();

    let lazy_config = LazyConfig {
        threshold_bytes: 0,
        ..LazyConfig::default()
    };
    for (label, config) in [("eager", LazyConfig::default()), ("lazy", lazy_config)] {
        let workbook = load_xlsx(&path, &Default::default(), &config).unwrap();
        let mut state = AppState::new(workbook, Config::default());
        state.set_size(80, 24);
        let (used_end, data_rows) = {
            let sheet = state.active_sheet().unwrap();
            (sheet.used_range.end.row, sheet.data_rows)
        };
        assert_eq!(data_rows, Some((3, 4)), "{label}");
        assert_eq!(used_end, 7, "{label}");

        state.apply(Action::Move(Motion::DataTop));
        assert_eq!(state.focus.a1(), "A4", "{label}");
        state.apply(Action::Move(Motion::DataBottom));
        assert_eq!(state.focus.a1(), "A5", "{label}");
        state.apply(Action::Move(Motion::Top));
        assert_eq!(state.focus.a1(), "A1", "{label}");
        state.apply(Action::Move(Motion::Bottom));
        assert_eq!(state.focus.a1(), "A8", "{label}");
    }
}

#[test]
fn first_non_blank_skips_empty_and_whitespace_cells() {
    for lazy in [false, true] {
        let mut state = csv_state_with(",,Alpha,1\n   ,Beta,2\n,\n", lazy);

        state.apply(Action::Move(Motion::FirstNonBlank));
        assert_eq!(state.focus.a1(), "C1", "lazy={lazy}");
        assert_eq!(state.focus.row, 0, "lazy={lazy}");

        state.apply(Action::Move(Motion::Down));
        state.apply(Action::Move(Motion::FirstNonBlank));
        assert_eq!(state.focus.a1(), "B2", "lazy={lazy}");

        state.apply(Action::Move(Motion::Down));
        state.apply(Action::Move(Motion::FirstNonBlank));
        assert_eq!(state.focus.a1(), "A3", "lazy={lazy}");
    }
}

#[test]
fn keymap_binds_data_and_used_range_navigation() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sheetgaze::app::keymap::Keymap;
    use sheetgaze::domain::mode::Mode;

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    let mut keymap = Keymap::new();
    assert_eq!(keymap.translate(key('g'), Mode::Normal), None);
    assert_eq!(
        keymap.translate(key('g'), Mode::Normal),
        Some(Action::Move(Motion::DataTop))
    );
    assert_eq!(
        keymap.translate(key('G'), Mode::Normal),
        Some(Action::Move(Motion::DataBottom))
    );
    assert_eq!(
        keymap.translate(key('{'), Mode::Normal),
        Some(Action::Move(Motion::Top))
    );
    assert_eq!(
        keymap.translate(key('}'), Mode::Normal),
        Some(Action::Move(Motion::Bottom))
    );
    assert_eq!(
        keymap.translate(key('^'), Mode::Normal),
        Some(Action::Move(Motion::FirstNonBlank))
    );

    assert_eq!(keymap.translate(key('g'), Mode::Visual), None);
    assert_eq!(
        keymap.translate(key('g'), Mode::Visual),
        Some(Action::Extend(Motion::DataTop))
    );
    assert_eq!(
        keymap.translate(key('G'), Mode::Visual),
        Some(Action::Extend(Motion::DataBottom))
    );
    assert_eq!(
        keymap.translate(key('{'), Mode::Visual),
        Some(Action::Extend(Motion::Top))
    );
    assert_eq!(
        keymap.translate(key('}'), Mode::Visual),
        Some(Action::Extend(Motion::Bottom))
    );
    assert_eq!(
        keymap.translate(key('^'), Mode::Visual),
        Some(Action::Extend(Motion::FirstNonBlank))
    );
    assert_eq!(
        keymap.translate(key('0'), Mode::Visual),
        Some(Action::Extend(Motion::LineStart))
    );
    assert_eq!(
        keymap.translate(key('$'), Mode::Visual),
        Some(Action::Extend(Motion::LineEnd))
    );
    assert_eq!(
        keymap.translate(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            Mode::Visual
        ),
        Some(Action::Extend(Motion::LineStart))
    );
    assert_eq!(
        keymap.translate(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            Mode::Visual
        ),
        Some(Action::Extend(Motion::LineEnd))
    );

    // Leaving visual mode must not leak a pending `g`.
    let mut keymap = Keymap::new();
    assert_eq!(keymap.translate(key('g'), Mode::Visual), None);
    assert_eq!(
        keymap.translate(key('v'), Mode::Visual),
        Some(Action::ToggleVisual)
    );
    assert_eq!(keymap.translate(key('g'), Mode::Normal), None);
}

#[test]
fn visual_mode_extends_to_line_bounds_and_first_non_blank() {
    let mut state = csv_state_with(",,Alpha,1\n", false);
    state.apply(Action::ToggleVisual);
    state.apply(Action::Extend(Motion::FirstNonBlank));
    assert_eq!(state.focus.a1(), "C1");
    state.apply(Action::Extend(Motion::LineEnd));
    assert_eq!(state.focus.a1(), "D1");
    state.apply(Action::Extend(Motion::LineStart));
    assert_eq!(state.focus.a1(), "A1");
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Visual);
}

#[test]
fn visual_mode_extends_to_data_rows() {
    let mut state = csv_state_with(",\n,\nAlpha,1\nBeta,2\n,\n", false);
    state.apply(Action::ToggleVisual);
    state.apply(Action::Extend(Motion::DataTop));
    assert_eq!(state.focus.a1(), "A3");
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Visual);
    state.apply(Action::Extend(Motion::DataBottom));
    assert_eq!(state.focus.a1(), "A4");
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Visual);
}

#[test]
fn keymap_binds_vim_editing_keys() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sheetgaze::app::keymap::Keymap;
    use sheetgaze::domain::edit::{EditCommit, EditCursor};
    use sheetgaze::domain::mode::Mode;

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }
    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    let mut keymap = Keymap::new();
    assert_eq!(
        keymap.translate(key('i'), Mode::Normal),
        Some(Action::EditStart(EditCursor::Start))
    );
    assert_eq!(
        keymap.translate(key(':'), Mode::Normal),
        Some(Action::CommandOpen)
    );
    assert_eq!(
        keymap.translate(key('K'), Mode::Normal),
        Some(Action::OpenDetail)
    );
    assert_eq!(keymap.translate(key('d'), Mode::Normal), None);
    assert_eq!(
        keymap.translate(key('d'), Mode::Normal),
        Some(Action::ClearRow)
    );
    assert_eq!(keymap.translate(key('r'), Mode::Normal), None);
    assert_eq!(
        keymap.translate(key('x'), Mode::Normal),
        Some(Action::ReplaceChar('x'))
    );
    assert_eq!(
        keymap.translate(ctrl('r'), Mode::Normal),
        Some(Action::Redo)
    );
    assert_eq!(keymap.translate(key('u'), Mode::Normal), Some(Action::Undo));
    assert_eq!(
        keymap.translate(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            Mode::Insert
        ),
        Some(Action::EditCommit(EditCommit::Stay))
    );
    assert_eq!(
        keymap.translate(ctrl('c'), Mode::Insert),
        Some(Action::EditCancel)
    );
    assert_eq!(keymap.translate(key('Z'), Mode::Normal), None);
    assert_eq!(
        keymap.translate(key('Z'), Mode::Normal),
        Some(Action::Save {
            force: false,
            quit_after: true
        })
    );
}

#[test]
fn dirty_workbook_refuses_quit_without_force() {
    let mut state = xlsx_state();
    state.apply(Action::EditStart(
        sheetgaze::domain::edit::EditCursor::Start,
    ));
    state.apply(Action::EditInput('x'));
    state.apply(Action::EditCommit(
        sheetgaze::domain::edit::EditCommit::Stay,
    ));
    assert!(state.is_dirty());
    assert!(state.apply(Action::Quit).is_empty());
    assert!(!state.should_quit);
    assert!(matches!(
        state.apply(Action::ForceQuit).as_slice(),
        [Effect::Quit]
    ));
    assert!(state.should_quit);
}

#[test]
fn detail_view_contains_cell_fields() {
    let mut state = csv_state();
    state.apply(Action::OpenDetail);
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Detail);
    let view = state.view();
    let detail = view.detail.expect("detail view");
    assert_eq!(detail.address, "A1");
    assert!(detail.display.contains("Name"));
}

#[test]
fn freeze_view_contains_frozen_row() {
    let mut state = xlsx_state();
    let view = state.view();
    assert!(!view.plan.frozen_rows.is_empty());
    assert_eq!(view.plan.frozen_rows[0].row, 0);
}

#[test]
fn moving_past_a_merge_at_the_used_range_edge_keeps_the_focus_visible() {
    let path = unique("merge-edge.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .merge_range(0, 0, 0, 7, "Survey title", &Format::new())
        .unwrap();
    sheet
        .merge_range(2, 4, 7, 7, "Merged details", &Format::new())
        .unwrap();
    sheet.write_string(9, 0, "Footer").unwrap();
    workbook.save(&path).unwrap();

    let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state.focus = sheetgaze::domain::addr::CellAddr::new(2, 4);

    state.apply(Action::Move(Motion::Right));

    assert_eq!(state.focus.a1(), "E3");
    let view = state.view();
    assert!(
        view.plan
            .scroll_rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .any(|cell| cell.cursor),
        "the focus cell must remain in the render plan"
    );
}

#[test]
fn filter_toggle_updates_visibility() {
    // Load a filtered workbook directly so the test can assert both states.
    let path = unique("filter-state.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "Name").unwrap();
    sheet.write_string(0, 1, "Value").unwrap();
    sheet.write_string(1, 0, "East").unwrap();
    sheet.write_number(1, 1, 10.0).unwrap();
    sheet.write_string(2, 0, "West").unwrap();
    sheet.write_number(2, 1, 20.0).unwrap();
    sheet.write_string(3, 0, "East").unwrap();
    sheet.write_number(3, 1, 30.0).unwrap();
    sheet.autofilter(0, 0, 3, 1).unwrap();
    sheet
        .filter_column(0, &FilterCondition::new().add_list_filter("East"))
        .unwrap();
    workbook.save(&path).unwrap();
    let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
    let mut state = AppState::new(workbook, Config::default());
    assert!(state.active_sheet().unwrap().visibility.is_visible(1));
    assert!(!state.active_sheet().unwrap().visibility.is_visible(2));
    state.apply(Action::FilterToggle);
    assert!(state.active_sheet().unwrap().visibility.is_visible(2));
}

#[test]
fn style_metadata_is_preserved() {
    let path = unique("style.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .write_string_with_format(0, 0, "Bold", &Format::new().set_bold())
        .unwrap();
    workbook.save(&path).unwrap();
    let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
    let cell = workbook.sheets[0]
        .effective_cell(sheetgaze::domain::addr::CellAddr::new(0, 0))
        .unwrap();
    assert!(cell.style.bold);
}
