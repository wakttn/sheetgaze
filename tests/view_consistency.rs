use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use sheetgaze::app::action::{Action, Motion};
use sheetgaze::app::state::AppState;
use sheetgaze::config::Config;
use sheetgaze::domain::addr::CellAddr;
use sheetgaze::domain::cell::CellValue;
use sheetgaze::domain::filter::{FilterState, VisibilityMap};
use sheetgaze::domain::formula::{FormulaCellResult, FormulaResults, FormulaStatus};
use sheetgaze::domain::layout::{LayoutEngine, Viewport};
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::domain::search::{SearchIndex, SearchMatch, SearchQuery};
use sheetgaze::domain::sheet::SheetData;
use sheetgaze::domain::viewport::ViewportPlanner;
use sheetgaze::io::csv::load_csv;
use sheetgaze::io::xlsx::load_xlsx;

fn unique(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sheetgaze-view-consistency-{}-{id}-{name}",
        std::process::id()
    ))
}

fn wide_csv() -> PathBuf {
    let path = unique("wide.csv");
    let mut text = String::new();
    for _ in 0..40 {
        text.push_str("column,");
    }
    text.push_str("last\n");
    for row in 0..4 {
        for col in 0..41 {
            text.push_str(&format!("r{row}c{col},"));
        }
        text.push('\n');
    }
    std::fs::write(&path, text).unwrap();
    path
}

fn load_eager(path: &Path) -> sheetgaze::domain::workbook::WorkbookModel {
    load_csv(
        path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xlsx")
        .join(name)
}

#[test]
fn viewport_follows_focus_horizontally_and_marks_the_cursor() {
    let path = wide_csv();
    let workbook = load_eager(&path);
    let sheet = &workbook.sheets[0];
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 10,
        width: 40,
    };
    let focus = CellAddr::new(1, 35);
    let freeze = LayoutEngine::effective_freeze(sheet, &viewport);
    let viewport_plan = ViewportPlanner::ensure_visible(sheet, &viewport, focus, freeze);
    assert!(
        viewport_plan.left_col > 0,
        "focus at column 35 should scroll horizontally"
    );
    let plan = LayoutEngine::plan(
        sheet,
        &Viewport {
            left_col: viewport_plan.left_col,
            ..viewport
        },
        focus,
        None,
        None,
    );
    let planned_focus = plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.addr == focus);
    let planned_focus = planned_focus.expect("focus cell must be in the render plan");
    assert!(planned_focus.cursor);
}

#[test]
fn disabled_freeze_pane_is_not_rendered_as_frozen() {
    let workbook = load_xlsx(
        &fixture("freeze_merge_conflict.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let sheet = &workbook.sheets[0];
    assert!(sheet.freeze.disabled);
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 20,
        width: 80,
    };
    let freeze = LayoutEngine::effective_freeze(sheet, &viewport);
    assert_eq!(freeze.rows, 0);
    assert_eq!(freeze.cols, 0);
    let plan = LayoutEngine::plan(sheet, &viewport, CellAddr::new(0, 0), None, None);
    assert!(plan.frozen_rows.is_empty());
}

#[test]
fn lazy_filter_toggle_reuses_masks_and_restores_visibility() {
    let workbook = load_xlsx(
        &fixture("filters_values.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: true,
            threshold_cells: 0,
            threshold_bytes: 0,
            row_cache_rows: 16,
            prefetch_rows: 0,
        },
    )
    .unwrap();
    let mut sheet = workbook.sheets.into_iter().next().unwrap();
    assert!(matches!(sheet.data, SheetData::Lazy(_)));
    let filtered = sheet.visibility.visible_count();
    assert!(filtered < sheet.meta.row_count);

    sheet.filter_state.enabled = false;
    sheet.rebuild_visibility().unwrap();
    assert_eq!(sheet.visibility.visible_count(), sheet.meta.row_count);

    sheet.filter_state = FilterState::from_definitions(&sheet.filters, false);
    sheet.filter_state.enabled = true;
    sheet.rebuild_visibility().unwrap();
    assert_eq!(sheet.visibility.visible_count(), filtered);
}

#[test]
fn horizontal_scroll_keeps_frozen_columns_without_duplicates() {
    let workbook = load_xlsx(
        &fixture("freeze_both.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let sheet = &workbook.sheets[0];
    let viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 10,
        width: 40,
    };
    let focus = CellAddr::new(5, 10);
    let freeze = LayoutEngine::effective_freeze(sheet, &viewport);
    let plan = ViewportPlanner::ensure_visible(sheet, &viewport, focus, freeze);
    let plan = LayoutEngine::plan(
        sheet,
        &Viewport {
            left_col: plan.left_col,
            ..viewport
        },
        focus,
        None,
        None,
    );
    assert_eq!(plan.frozen_cols, 1);
    let row = plan
        .scroll_rows
        .iter()
        .find(|row| row.row == 5)
        .expect("focused row must be planned");
    let mut addresses = std::collections::HashSet::new();
    for cell in &row.cells {
        assert!(addresses.insert(cell.addr), "cell was drawn twice");
    }
}

#[test]
fn limited_freeze_area_reports_a_status_and_keeps_scroll_rows() {
    let workbook = load_xlsx(
        &fixture("freeze_rows.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 4);
    let view = state.view();
    assert!(view.plan.freeze.rows_limited);
    assert!(view.status.freeze_label.contains("limited"));
    assert!(!view.plan.scroll_rows.is_empty());
}

#[test]
fn app_state_toggles_filters_on_a_lazy_sheet() {
    let workbook = load_xlsx(
        &fixture("filters_values.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: true,
            threshold_cells: 0,
            threshold_bytes: 0,
            row_cache_rows: 16,
            prefetch_rows: 0,
        },
    )
    .unwrap();
    let filtered = workbook.sheets[0].visibility.visible_count();
    let mut state = AppState::new(workbook, Config::default());
    assert!(matches!(
        state.active_sheet().unwrap().data,
        SheetData::Lazy(_)
    ));
    assert_eq!(
        state.active_sheet().unwrap().visibility.visible_count(),
        filtered
    );
    state.apply(Action::FilterToggle);
    assert_eq!(
        state.active_sheet().unwrap().visibility.visible_count(),
        state.active_sheet().unwrap().meta.row_count
    );
    state.apply(Action::FilterToggle);
    assert_eq!(
        state.active_sheet().unwrap().visibility.visible_count(),
        filtered
    );
}

#[test]
fn wrap_text_expands_row_height_and_detail_truncation() {
    let workbook = load_xlsx(
        &fixture("overflow.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let sheet = &workbook.sheets[0];
    let wrap_addr = CellAddr::new(4, 0);
    assert!(
        sheet.layout.row_heights[wrap_addr.row as usize] > 1,
        "wrap_text content should increase row height"
    );
    let measure = LayoutEngine::measure_cell(sheet, &sheet.layout_limits, wrap_addr);
    assert!(measure.lines.len() > 1);
    assert_eq!(
        measure.truncated,
        LayoutEngine::truncated_at(sheet, &sheet.layout_limits, wrap_addr)
    );
}

#[test]
fn formula_result_grows_layout_and_invalidates_filter_masks() {
    let path = wide_csv();
    let mut workbook = load_eager(&path);
    let name = workbook.sheets[0].name.clone();
    let before = workbook.sheets[0]
        .layout
        .column_widths
        .first()
        .copied()
        .unwrap_or(0);
    let result = FormulaResults {
        generation: 1,
        updates: vec![FormulaCellResult {
            sheet: name,
            addr: CellAddr::new(0, 0),
            value: CellValue::Text("x".repeat(90)),
            status: FormulaStatus::Calculated,
        }],
    };
    let summary = workbook.apply_formula_results(result);
    assert!(summary.changed);
    assert!(summary.layout_changed);
    assert!(
        workbook.sheets[0].layout.column_widths[0] > before,
        "calculated text should grow the column width"
    );
}

#[test]
fn formula_result_updates_filter_visibility() {
    let mut workbook = load_xlsx(
        &fixture("filters_values.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    assert!(!workbook.sheets[0].visibility.is_visible(3));
    let name = workbook.sheets[0].name.clone();
    let summary = workbook.apply_formula_results(FormulaResults {
        generation: 1,
        updates: vec![FormulaCellResult {
            sheet: name,
            addr: CellAddr::new(3, 0),
            value: CellValue::Text("East".to_string()),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(summary.filter_masks_dirty);
    workbook.sheets[0].rebuild_visibility().unwrap();
    assert!(workbook.sheets[0].visibility.is_visible(3));
}

#[test]
fn formula_result_marks_filter_masks_dirty() {
    let mut workbook = load_xlsx(
        &fixture("filters_values.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let name = workbook.sheets[0].name.clone();
    let summary = workbook.apply_formula_results(FormulaResults {
        generation: 1,
        updates: vec![FormulaCellResult {
            sheet: name,
            addr: CellAddr::new(1, 0),
            value: CellValue::Text("changed".to_string()),
            status: FormulaStatus::Calculated,
        }],
    });
    assert!(summary.changed);
    assert!(summary.filter_masks_dirty);
    assert!(workbook.sheets[0].filter_masks_dirty);
}

#[test]
fn search_syncs_to_the_first_visible_match() {
    let mut visibility = VisibilityMap::from_mask(vec![false, true, true]);
    let mut index = SearchIndex::new(
        SearchQuery::new("hit".to_string()),
        vec![
            SearchMatch {
                addr: CellAddr::new(0, 0),
                source: sheetgaze::domain::search::MatchSource::Display,
            },
            SearchMatch {
                addr: CellAddr::new(1, 0),
                source: sheetgaze::domain::search::MatchSource::Display,
            },
        ],
    );
    index.sync_visibility(&visibility);
    assert_eq!(index.current_match().unwrap().addr, CellAddr::new(1, 0));
    assert_eq!(index.hidden_matches, 1);

    visibility = VisibilityMap::from_mask(vec![false, false, false]);
    index.sync_visibility(&visibility);
    assert!(index.current_match().is_none());
    assert_eq!(index.hidden_matches, 2);
}

#[test]
fn app_state_keeps_focus_visible_and_shows_cursor() {
    let path = wide_csv();
    let mut state = AppState::new(load_eager(&path), Config::default());
    state.set_size(40, 10);
    state.apply(Action::Move(Motion::LineEnd));
    let view = state.view();
    assert!(view.viewport.left_col > 0);
    let focus = view.focus;
    let planned = view
        .plan
        .scroll_rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.addr == focus)
        .expect("focused cell must remain in the viewport");
    assert!(planned.cursor);
}
