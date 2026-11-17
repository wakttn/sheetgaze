use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use sheetgaze::app::action::{Action, Motion};
use sheetgaze::app::reload::ReloadTrigger;
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::cell::CellValue;
use sheetgaze::domain::formula::{FormulaCellResult, FormulaResults, FormulaStatus};
use sheetgaze::domain::layout::ColumnWidths;
use sheetgaze::domain::mode::Mode;
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::csv::load_csv;
use sheetgaze::io::xlsx::load_xlsx;

fn unique(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sheetgaze-fit-width-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn wide_csv(columns: usize) -> PathBuf {
    let path = unique(&format!("wide-{columns}.csv"));
    let mut text = String::new();
    for col in 0..columns {
        if col > 0 {
            text.push(',');
        }
        text.push_str(&format!("column-{col:02}"));
    }
    text.push('\n');
    for col in 0..columns {
        if col > 0 {
            text.push(',');
        }
        if col + 1 == columns {
            text.push('Z');
        } else {
            text.push_str(&format!("r0c{col}"));
        }
    }
    text.push('\n');
    std::fs::write(&path, text).unwrap();
    path
}

fn state_for(path: &Path) -> AppState {
    let workbook = load_csv(
        path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap();
    AppState::new(workbook, Config::default())
}

fn slots(widths: &ColumnWidths) -> u32 {
    widths.total_slots()
}

#[test]
fn toggle_fit_width_compresses_columns_and_restores_original() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);

    let base = state.view();
    assert!(
        base.plan
            .columns
            .iter()
            .all(|column| column.col < state.active_sheet().unwrap().used_range.end.col),
        "the original layout should require horizontal scrolling"
    );
    assert!(base.status.fit_label.is_empty());

    let effects = state.apply(Action::ToggleFitWidth);
    assert!(effects.is_empty());
    assert!(state.fit_width);

    let fitted = state.view();
    let sheet = state.active_sheet().unwrap();
    assert_eq!(
        fitted.plan.columns.last().unwrap().col,
        sheet.used_range.end.col,
        "fit width should render the last column"
    );
    assert!(slots(&state.column_widths(sheet)) <= fitted.viewport.width as u32);
    assert_eq!(fitted.status.fit_label, "fit:on");
    let last_cell = fitted
        .plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.addr == CellAddr::new(1, 9))
        .expect("last column should be planned");
    assert!(
        last_cell
            .lines
            .iter()
            .any(|line| line.spans.iter().any(|span| span.text.contains('Z')))
    );

    state.apply(Action::ToggleFitWidth);
    assert!(!state.fit_width);
    let restored = state.view();
    assert!(restored.status.fit_label.is_empty());
    assert!(
        restored
            .plan
            .columns
            .iter()
            .all(|column| column.col < state.active_sheet().unwrap().used_range.end.col),
        "turning fit width off should restore the original scrollable layout"
    );
}

#[test]
fn fit_width_never_mutates_the_base_layout() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    let before = state.active_sheet().unwrap().layout.column_widths.clone();
    state.apply(Action::ToggleFitWidth);
    let _ = state.view();
    let after = state.active_sheet().unwrap().layout.column_widths.clone();
    assert_eq!(before, after, "fit width must not rewrite the base layout");
    let sheet = state.active_sheet().unwrap();
    let base = ColumnWidths::from_sheet(sheet);
    assert_eq!(base.width(0), sheet.layout.column_widths[0]);
}

#[test]
fn fit_width_recomputes_after_resize() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.apply(Action::ToggleFitWidth);
    let narrow = state.view();
    assert!(slots(&state.column_widths(state.active_sheet().unwrap())) <= 33);
    assert_eq!(narrow.status.fit_label, "fit:on");

    state.apply(Action::Resize(80, 24));
    let wide = state.view();
    assert!(slots(&state.column_widths(state.active_sheet().unwrap())) <= 73);
    assert_eq!(wide.status.fit_label, "fit:on");

    state.apply(Action::Resize(40, 10));
    let narrow_again = state.view();
    assert!(slots(&state.column_widths(state.active_sheet().unwrap())) <= 33);
    assert_eq!(narrow_again.status.fit_label, "fit:on");
}

#[test]
fn fit_width_reports_limited_and_keeps_focus_visible() {
    let path = wide_csv(40);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.apply(Action::ToggleFitWidth);
    state.apply(Action::Move(Motion::LineEnd));

    let view = state.view();
    assert_eq!(view.status.fit_label, "fit:limited");
    assert!(view.viewport.left_col > 0);
    let focus = view.focus;
    assert_eq!(focus.col, state.active_sheet().unwrap().used_range.end.col);
    assert!(
        view.plan
            .scroll_rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .any(|cell| cell.addr == focus && cell.cursor),
        "focused cell must remain visible at maximum compression"
    );
}

#[test]
fn fit_width_survives_reload() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.apply(Action::ToggleFitWidth);
    assert!(state.fit_width);

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
    state.replace_workbook(workbook, ReloadTrigger::Manual);
    assert!(state.fit_width);
    let view = state.view();
    assert_eq!(view.status.fit_label, "fit:on");
    assert!(slots(&state.column_widths(state.active_sheet().unwrap())) <= 33);
}

#[test]
fn fit_width_recomputes_after_formula_growth() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.apply(Action::ToggleFitWidth);
    let before = state.active_sheet().unwrap().layout.column_widths[0];
    let generation = state.workbook.as_ref().unwrap().generation.0;
    let sheet_name = state.active_sheet().unwrap().name.clone();
    state.apply_formula_results(FormulaResults {
        generation,
        updates: vec![FormulaCellResult {
            sheet: sheet_name,
            addr: CellAddr::new(0, 0),
            value: CellValue::Text("x".repeat(200)),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(
        state.active_sheet().unwrap().layout.column_widths[0] > before,
        "formula growth should update the base layout"
    );
    let view = state.view();
    assert!(slots(&state.column_widths(state.active_sheet().unwrap())) <= 33);
    assert_eq!(view.status.fit_label, "fit:on");
}

#[test]
fn fit_width_survives_sheet_switch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx/multi_sheet.xlsx");
    let workbook = load_xlsx(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(40, 10);
    state.apply(Action::ToggleFitWidth);
    assert!(state.fit_width);
    state.apply(Action::SwitchSheet(1));
    assert!(state.fit_width);
    assert_eq!(state.view().status.fit_label, "fit:on");
}

#[test]
fn fit_width_keeps_detail_truncation_consistent_with_the_grid() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.focus = CellAddr::new(0, 0);
    state.apply(Action::ToggleFitWidth);
    state.apply(Action::OpenDetail);
    assert_eq!(state.mode, Mode::Detail);

    let view = state.view();
    let planned = view
        .plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.addr == CellAddr::new(0, 0))
        .expect("focus cell must be planned");
    let detail = view.detail.expect("detail view should be present");
    assert_eq!(detail.truncated, planned.truncated);
    assert!(planned.truncated);
}

#[test]
fn fit_width_leaves_visual_selection_and_copy_intact() {
    let path = wide_csv(10);
    let mut state = state_for(&path);
    state.set_size(40, 10);
    state.apply(Action::ToggleVisual);
    state.apply(Action::Extend(Motion::Right));
    let selected = state.selection;
    state.apply(Action::ToggleFitWidth);
    assert_eq!(state.selection, selected);
    assert_eq!(state.mode, Mode::Visual);
}
