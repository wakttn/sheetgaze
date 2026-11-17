use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use rust_xlsxwriter::{Format, FormatBorder, Workbook};
use sheetgaze::app::action::{Action, Motion};
use sheetgaze::app::state::AppState;
use sheetgaze::config::{Config, FillMode};
use sheetgaze::domain::layout::Viewport;
use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::csv::load_csv;
use sheetgaze::io::xlsx::load_xlsx;
use sheetgaze::ui::screen::Screen;
use sheetgaze::ui::theme::Theme;

fn fixture() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sheetgaze-tests-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tui.csv");
    std::fs::write(&path, "Name,Value\nEast,10\nWest,20\n").unwrap();
    path
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx")
}

#[test]
fn renders_grid_and_status() {
    let path = fixture();
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
    state.viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 22,
        width: 73,
    };
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = format!("{buffer:?}");
    assert!(rendered.contains("Name") || rendered.contains("East"));
    assert!(rendered.contains("tui.csv"));
    assert!(
        buffer.content().iter().any(|cell| cell.symbol() == "│"),
        "default vertical gridlines should be visible"
    );
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.modifier.contains(Modifier::UNDERLINED)),
        "default horizontal gridlines should be visible"
    );
}

#[test]
fn renders_insert_line_and_dirty_marker() {
    let path = std::env::temp_dir().join("sheetgaze-tests/edit-line.xlsx");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "hello").unwrap();
    workbook.save(&path).unwrap();
    let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);

    state.apply(Action::EditClear);
    state.apply(Action::EditInput('x'));
    let view = state.view();
    assert_eq!(view.mode, sheetgaze::domain::mode::Mode::Insert);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        rendered.contains("A1 >"),
        "input line should show cell address"
    );
    assert!(rendered.contains("INSERT"));

    state.apply(Action::EditCommit(
        sheetgaze::domain::edit::EditCommit::Stay,
    ));
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains("[+]"), "dirty marker should be visible");
}

#[test]
fn fit_width_renders_the_last_column_and_status() {
    let path = std::env::temp_dir().join("sheetgaze-tests/fit-width-tui.csv");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::new();
    for col in 0..10 {
        if col > 0 {
            text.push(',');
        }
        text.push_str(&format!("column-{col:02}"));
    }
    text.push('\n');
    for col in 0..10 {
        if col > 0 {
            text.push(',');
        }
        if col == 9 {
            text.push('Z');
        } else {
            text.push('1');
        }
    }
    text.push('\n');
    std::fs::write(&path, text).unwrap();
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
    state.set_size(80, 10);
    state.apply(Action::ToggleFitWidth);
    let view = state.view();
    assert_eq!(view.plan.columns.last().unwrap().col, 9);

    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered = format!("{:?}", terminal.backend().buffer());
    assert!(rendered.contains("fit:on"), "status missing: {rendered}");
    assert!(rendered.contains('Z'), "last column missing: {rendered}");
}

#[test]
fn renders_excel_border_styles() {
    let workbook = load_xlsx(
        &fixture_dir().join("borders.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(100, 30);
    state.viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 28,
        width: 93,
    };
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let symbols: Vec<&str> = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(
        symbols.contains(&"┃"),
        "thick/medium Excel borders should use a heavy vertical glyph"
    );
    assert!(
        symbols.contains(&"║"),
        "double Excel borders should use a double vertical glyph"
    );
}

#[test]
fn normal_mode_highlights_focus_cell() {
    let path = fixture();
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
    state.focus = sheetgaze::domain::addr::CellAddr::new(1, 1);
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let cell = terminal
        .backend()
        .buffer()
        .cell((11, 3))
        .expect("focus cell must be rendered");
    assert!(
        cell.modifier.contains(Modifier::REVERSED),
        "normal mode focus must use the cursor style: {:?}",
        cell.modifier
    );
}

#[test]
fn horizontal_focus_scroll_renders_the_focused_column() {
    let path = std::env::temp_dir().join("sheetgaze-tests/tui-wide.csv");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::from("a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y,z\n");
    for row in 0..3 {
        text.push_str(&format!("{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row},{row}\n"));
    }
    std::fs::write(&path, text).unwrap();
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
    state.set_size(40, 10);
    state.apply(Action::Move(Motion::LineEnd));
    let view = state.view();
    assert!(view.viewport.left_col > 0);
    let focus = view.focus;
    assert!(view.plan.scroll_rows.iter().any(|row| {
        row.cells
            .iter()
            .any(|cell| cell.addr == focus && cell.cursor)
    }));
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    assert!(!terminal.backend().to_string().is_empty());
}

#[test]
fn renders_multi_row_merged_text() {
    let workbook = load_xlsx(
        &fixture_dir().join("merged_newlines.xlsx"),
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
    )
    .unwrap();
    let mut state = AppState::new(workbook, Config::default());
    state.set_size(80, 24);
    state.viewport = Viewport {
        top_visible_row: 0,
        left_col: 0,
        height: 22,
        width: 73,
    };
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("複数行"));
    assert!(rendered.contains("複数列"));
    assert!(
        rendered.contains("の結合セル"),
        "the last line of a multi-row merged cell must be rendered"
    );
}

#[test]
fn column_headers_cover_columns_spanned_by_a_merged_title() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged-title.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .merge_range(0, 0, 0, 3, "Survey title", &Format::new())
        .unwrap();
    for col in 0..4 {
        sheet.write_string(1, col, format!("value {col}")).unwrap();
    }
    workbook.save(&path).unwrap();

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
    state.set_size(80, 24);
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();

    let header: String = (0..80)
        .map(|x| terminal.backend().buffer().cell((x, 1)).unwrap().symbol())
        .collect();
    for label in ["A", "B", "C", "D"] {
        assert!(
            header.contains(label),
            "column label {label} is missing from the header: {header}"
        );
    }
}

#[test]
fn focus_highlight_is_rendered_on_blank_cells() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blank-focus.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "Top").unwrap();
    sheet.write_string(2, 0, "Bottom").unwrap();
    workbook.save(&path).unwrap();

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
    state.set_size(80, 24);
    state.focus = sheetgaze::domain::addr::CellAddr::new(1, 0);
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();

    let cell = terminal.backend().buffer().cell((6, 3)).unwrap();
    assert!(
        cell.modifier.contains(Modifier::REVERSED),
        "blank focus cell must use the cursor style: {:?}",
        cell.modifier
    );
}

#[test]
fn handles_small_terminal() {
    let path = fixture();
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
    state.set_size(20, 5);
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Terminal too small"));
}

fn render_state(state: &mut AppState, width: u16, height: u16) -> String {
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let theme = Theme::new(sheetgaze::config::ColorMode::Mono);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn write_tall_csv(dir: &Path, rows: usize) -> PathBuf {
    let path = dir.join("tall.csv");
    let mut text = String::from("Name,Value\n");
    for row in 0..rows {
        text.push_str(&format!("row{row},{row}\n"));
    }
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn bottom_motion_renders_the_last_data_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_tall_csv(dir.path(), 60);
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
    state.apply(Action::Move(Motion::Bottom));

    let view = state.view();
    assert_eq!(view.focus.row, 60, "the last data row should be focused");
    let rendered = render_state(&mut state, 80, 24);
    assert!(
        rendered.contains("row59"),
        "the last data row must be visible at the bottom of the viewport"
    );
}

#[test]
fn page_down_keeps_the_focused_row_visible() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_tall_csv(dir.path(), 60);
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
    state.apply(Action::Move(Motion::PageDown));

    let view = state.view();
    assert_eq!(view.focus.row, 21);
    let rendered = render_state(&mut state, 80, 24);
    assert!(
        rendered.contains("row20"),
        "the row focused by PageDown must be visible"
    );
}

#[test]
fn bottom_merged_section_is_rendered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tall-merged.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for row in 0..40u32 {
        sheet.write_string(row, 0, format!("row{row}")).unwrap();
    }
    let wrap = Format::new().set_text_wrap();
    sheet
        .merge_range(
            40,
            0,
            43,
            3,
            "MERGEDTAILSTART\nsecond line\nthird line\nMERGEDTAILEND",
            &wrap,
        )
        .unwrap();
    workbook.save(&path).unwrap();

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
    state.set_size(80, 24);
    state.apply(Action::Move(Motion::Bottom));

    let view = state.view();
    assert_eq!(view.focus.row, 40);
    let rendered = render_state(&mut state, 80, 24);
    for marker in [
        "MERGEDTAILSTART",
        "second line",
        "third line",
        "MERGEDTAILEND",
    ] {
        assert!(
            rendered.contains(marker),
            "the complete merged tail section must be visible at the bottom: missing {marker}"
        );
    }
}

#[test]
fn bottom_row_is_visible_with_frozen_multi_line_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen-tall.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_column_width(0, 12).unwrap();
    let wrap = Format::new().set_text_wrap();
    for row in 0..40u32 {
        sheet
            .write_string_with_format(row, 0, format!("row{row} wrapped content"), &wrap)
            .unwrap();
    }
    sheet.set_freeze_panes(3, 0).unwrap();
    workbook.save(&path).unwrap();

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
    state.set_size(80, 24);
    state.apply(Action::Move(Motion::Bottom));

    let view = state.view();
    assert_eq!(view.focus.row, 39);
    let rendered = render_state(&mut state, 80, 24);
    assert!(
        rendered.contains("row39"),
        "the last row must be visible even with frozen, multi-line rows above it"
    );
}

#[test]
fn fill_border_mode_draws_a_colored_outline_without_a_background() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fill-border.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let format = Format::new().set_background_color(rust_xlsxwriter::Color::RGB(0xD9E2F3));
    sheet.write_string_with_format(0, 0, "x", &format).unwrap();
    workbook.save(&path).unwrap();

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
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let theme = Theme::new_with_fill(sheetgaze::config::ColorMode::Truecolor, FillMode::Border);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let buffer = terminal.backend().buffer();

    let fill = Color::Rgb(0xD9, 0xE2, 0xF3);
    let right_border = buffer.cell((9, 2)).unwrap();
    assert_eq!(right_border.symbol(), "│");
    assert_eq!(right_border.fg, fill);

    let text_cell = buffer.cell((6, 2)).unwrap();
    assert_eq!(
        text_cell.bg,
        Color::Reset,
        "border mode must not paint the background"
    );

    let bottom_border = buffer.cell((7, 2)).unwrap();
    assert!(
        bottom_border.modifier.contains(Modifier::UNDERLINED),
        "the fill should be drawn as an underline"
    );
    assert_eq!(bottom_border.underline_color, fill);
}

#[test]
fn fill_background_mode_paints_whole_cell_slots_and_empty_filled_cells() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fill-background.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let fill = Format::new().set_background_color(rust_xlsxwriter::Color::RGB(0xD9E2F3));
    sheet.write_string_with_format(0, 0, "x", &fill).unwrap();
    sheet.write_string_with_format(0, 1, "y", &fill).unwrap();
    sheet.write_blank(1, 0, &fill).unwrap();
    sheet.write_blank(2, 2, &fill).unwrap();
    workbook.save(&path).unwrap();

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
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let theme = Theme::new_with_fill(
        sheetgaze::config::ColorMode::Truecolor,
        FillMode::Background,
    );
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let fill = Color::Rgb(0xD9, 0xE2, 0xF3);
    let data_x = 6;
    let slot = |col: u32| {
        let column = view
            .plan
            .columns
            .iter()
            .find(|column| column.col == col)
            .expect("visible column");
        (data_x + column.x, column.width)
    };

    for (col, y) in [(0, 2), (1, 2), (0, 3), (2, 4)] {
        let (x, width) = slot(col);
        for offset in 0..width {
            assert_eq!(
                buffer.cell((x + offset, y)).unwrap().bg,
                fill,
                "fill must cover the whole slot at ({}, {y})",
                x + offset
            );
        }
        assert_eq!(
            buffer.cell((x + width, y)).unwrap().bg,
            Color::Reset,
            "the cell fill must not paint over the cell boundary"
        );
    }
}

#[test]
fn fill_border_mode_recolors_explicit_borders() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fill-border-explicit.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let format = Format::new()
        .set_background_color(rust_xlsxwriter::Color::RGB(0xFFC7CE))
        .set_border(FormatBorder::Thin)
        .set_border_color(rust_xlsxwriter::Color::RGB(0xFF0000));
    sheet.write_string_with_format(0, 0, "x", &format).unwrap();
    workbook.save(&path).unwrap();

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
    let view = state.view();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let theme = Theme::new_with_fill(sheetgaze::config::ColorMode::Truecolor, FillMode::Border);
    terminal
        .draw(|frame| Screen::render(frame, &view, &theme))
        .unwrap();
    let buffer = terminal.backend().buffer();

    let right_border = buffer.cell((9, 2)).unwrap();
    assert_eq!(
        right_border.fg,
        Color::Rgb(0xFF, 0xC7, 0xCE),
        "the fill color should win over the explicit red border"
    );
}
