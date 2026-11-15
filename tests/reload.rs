use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sheetgaze::app::action::Action;
use sheetgaze::app::formula_runner::FormulaRunner;
use sheetgaze::app::reload::{ReloadContext, ReloadHandle, ReloadMessage, ReloadTrigger};
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::cell::{CellRead, CellValue};
use sheetgaze::domain::formula::{FormulaCellResult, FormulaResults, FormulaStatus};
use sheetgaze::domain::reload::{WatchConfig, WatchMode};
use sheetgaze::domain::selection::Selection;
use sheetgaze::io::loader::WorkbookLoader;
use sheetgaze::ui::screen::Screen;
use sheetgaze::ui::theme::Theme;

fn unique(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sheetgaze-reload-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn fast_watch(mode: WatchMode) -> WatchConfig {
    WatchConfig {
        mode,
        poll_interval: Duration::from_millis(100),
        debounce: Duration::from_millis(50),
        retry: Duration::from_millis(50),
        native_safety: Duration::from_millis(200),
        failure_notice_interval: 2,
    }
}

fn loader(path: &Path) -> WorkbookLoader {
    WorkbookLoader::from_config(path, &Config::default()).unwrap()
}

fn wait_loaded(
    handle: &ReloadHandle,
    timeout: Duration,
) -> sheetgaze::domain::workbook::WorkbookModel {
    let start = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        assert!(!remaining.is_zero(), "timed out waiting for reload");
        match handle.events.recv_timeout(remaining) {
            Ok(ReloadMessage::Loaded { workbook, .. }) => return workbook,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for reload"),
            Err(RecvTimeoutError::Disconnected) => panic!("reload worker disconnected"),
        }
    }
}

fn wait_failed(handle: &ReloadHandle, timeout: Duration) -> (u32, bool, String) {
    let start = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        assert!(!remaining.is_zero(), "timed out waiting for failure");
        match handle.events.recv_timeout(remaining) {
            Ok(ReloadMessage::Failed {
                attempt,
                message,
                notify,
                ..
            }) => return (attempt, notify, message),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for failure"),
            Err(RecvTimeoutError::Disconnected) => panic!("reload worker disconnected"),
        }
    }
}

fn cell_text(workbook: &sheetgaze::domain::workbook::WorkbookModel, row: u32, col: u32) -> String {
    match workbook.read_cell(0, CellAddr::new(row, col)) {
        CellRead::Loaded(cell) => cell.display,
        CellRead::Pending | CellRead::Missing => String::new(),
    }
}

#[test]
fn poll_detects_file_change_and_reloads() {
    let path = unique("watch.csv");
    std::fs::write(&path, "name,value\nEast,10\n").unwrap();
    let handle = ReloadHandle::spawn(ReloadContext {
        loader: loader(&path),
        watch: fast_watch(WatchMode::Poll),
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(1_000));
    std::fs::write(&path, "name,value\nEast,10\nWest,20\n").unwrap();
    let workbook = wait_loaded(&handle, Duration::from_secs(5));
    assert_eq!(cell_text(&workbook, 2, 0), "West");
    assert_eq!(cell_text(&workbook, 2, 1), "20");
}

#[test]
fn manual_reload_works_when_watch_is_off() {
    let path = unique("manual.csv");
    std::fs::write(&path, "a,b\n1,2\n").unwrap();
    let handle = ReloadHandle::spawn(ReloadContext {
        loader: loader(&path),
        watch: fast_watch(WatchMode::Off),
    })
    .unwrap();
    std::fs::write(&path, "a,b\n3,4\n").unwrap();
    handle.reload_now();
    let workbook = wait_loaded(&handle, Duration::from_secs(2));
    assert_eq!(cell_text(&workbook, 1, 0), "3");
}

#[test]
fn failed_load_keeps_worker_alive_and_recovers() {
    let path = unique("book.xlsx");
    write_xlsx(&path, 10.0);
    let handle = ReloadHandle::spawn(ReloadContext {
        loader: loader(&path),
        watch: fast_watch(WatchMode::Poll),
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(1_000));
    std::fs::write(&path, b"not a valid xlsx").unwrap();
    let (attempt, notify, _message) = wait_failed(&handle, Duration::from_secs(5));
    assert!(attempt >= 1);
    assert!(notify);

    write_xlsx(&path, 42.0);
    let workbook = wait_loaded(&handle, Duration::from_secs(5));
    assert_eq!(cell_text(&workbook, 1, 1), "42");
}

fn write_xlsx(path: &Path, value: f64) {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "value").unwrap();
    sheet.write_number(1, 1, value).unwrap();
    workbook.save(path).unwrap();
}

#[test]
fn replace_workbook_clamps_focus_and_preserves_selection_in_range() {
    let old_path = unique("old.csv");
    let new_path = unique("new.csv");
    std::fs::write(&old_path, "a,b\n1,2\n3,4\n5,6\n7,8\n9,10\n").unwrap();
    std::fs::write(&new_path, "a,b\n1,2\n3,4\n").unwrap();
    let old = loader(&old_path).load().unwrap();
    let new = loader(&new_path).load().unwrap();
    let mut state = AppState::new(old, Config::default());
    state.focus = CellAddr::new(4, 1);
    state.selection = Some(Selection::new(CellAddr::new(1, 1)));
    state.mode = sheetgaze::domain::mode::Mode::Visual;
    let summary = state.replace_workbook(new, ReloadTrigger::Manual);
    assert!(summary.focus_moved);
    assert_eq!(state.focus.row, 2);
    assert_eq!(state.focus.col, 1);
    assert!(state.selection.is_some());
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Visual);
    assert!(state.status_message.unwrap().contains("manual"));
}

#[test]
fn replace_workbook_clears_search_and_out_of_range_selection() {
    let old_path = unique("search-old.csv");
    let new_path = unique("search-new.csv");
    std::fs::write(&old_path, "a,b\n1,2\n3,4\n5,6\n").unwrap();
    std::fs::write(&new_path, "a,b\n1,2\n").unwrap();
    let old = loader(&old_path).load().unwrap();
    let new = loader(&new_path).load().unwrap();
    let mut state = AppState::new(old, Config::default());
    state.selection = Some(Selection::new(CellAddr::new(5, 0)));
    state.mode = sheetgaze::domain::mode::Mode::Visual;
    state.apply(Action::SearchOpen);
    for ch in "a".chars() {
        state.apply(Action::SearchInput(ch));
    }
    state.apply(Action::SearchSubmit);
    let summary = state.replace_workbook(new, ReloadTrigger::Watch);
    assert!(summary.search_cleared);
    assert!(state.search_index.is_none());
    assert!(state.selection.is_none());
    assert_eq!(state.mode, sheetgaze::domain::mode::Mode::Normal);
}

#[test]
fn replace_workbook_restores_filter_toggles() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx/filters_values.xlsx");
    let first = loader(&path).load().unwrap();
    let second = loader(&path).load().unwrap();
    let mut state = AppState::new(first, Config::default());
    {
        let sheet = &mut state.workbook.as_mut().unwrap().sheets[0];
        if let Some(column) = sheet.filter_state.column_enabled.first_mut() {
            *column = false;
        }
        sheet.filter_state.enabled = true;
        sheet.rebuild_visibility().unwrap();
    }
    let expected = state.workbook.as_ref().unwrap().sheets[0]
        .filter_state
        .clone();
    let summary = state.replace_workbook(second, ReloadTrigger::Manual);
    assert!(!summary.filter_reset);
    let actual = &state.workbook.as_ref().unwrap().sheets[0].filter_state;
    assert_eq!(actual.enabled, expected.enabled);
    assert_eq!(actual.column_enabled, expected.column_enabled);
}

#[test]
fn stale_formula_results_are_ignored_after_reload() {
    let old_path = unique("formula-old.csv");
    let new_path = unique("formula-new.csv");
    std::fs::write(&old_path, "a\n1\n").unwrap();
    std::fs::write(&new_path, "a\n2\n").unwrap();
    let mut state = AppState::new(loader(&old_path).load().unwrap(), Config::default());
    let mut runner = FormulaRunner::new();
    let start = runner.restart(state.workbook.as_mut().unwrap(), &Config::default().recalc);
    let stale_generation = start.generation;
    let sheet_name = state
        .workbook
        .as_ref()
        .unwrap()
        .active()
        .unwrap()
        .name
        .clone();
    state.apply_formula_results(FormulaResults {
        generation: stale_generation,
        updates: vec![FormulaCellResult {
            sheet: sheet_name,
            addr: CellAddr::new(0, 0),
            value: CellValue::Text("stale".to_string()),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(
        state.workbook.as_ref().unwrap().sheets[0]
            .formula_overlay
            .values
            .contains_key(&CellAddr::new(0, 0))
    );

    state.replace_workbook(loader(&new_path).load().unwrap(), ReloadTrigger::Watch);
    let next = runner.restart(state.workbook.as_mut().unwrap(), &Config::default().recalc);
    assert!(next.generation > stale_generation);
    state.apply_formula_results(FormulaResults {
        generation: stale_generation,
        updates: vec![FormulaCellResult {
            sheet: state
                .workbook
                .as_ref()
                .unwrap()
                .active()
                .unwrap()
                .name
                .clone(),
            addr: CellAddr::new(0, 0),
            value: CellValue::Text("stale".to_string()),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(
        state.workbook.as_ref().unwrap().sheets[0]
            .formula_overlay
            .values
            .is_empty()
    );
}

#[test]
fn status_and_help_expose_reload() {
    let path = unique("ui.csv");
    std::fs::write(&path, "a\n1\n").unwrap();
    let mut state = AppState::new(loader(&path).load().unwrap(), Config::default());
    state.set_size(100, 30);
    assert_eq!(state.view().status.reload_label, "watch:poll");
    state.apply(Action::HelpOpen);
    let view = state.view();
    assert!(
        view.help
            .as_ref()
            .unwrap()
            .bindings
            .iter()
            .any(|(key, _)| key == "r")
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered = format!("{:?}", terminal.backend().buffer());
    assert!(rendered.contains("watch:poll"));
    assert!(rendered.contains("reload file") || rendered.contains("Reload"));
}
