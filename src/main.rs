use std::io::{IsTerminal, Write};
use std::path::Path;

use clap::Parser;

use sheetgaze::app::event_loop::{self, EventLoop};
use sheetgaze::app::formula_runner::FormulaRunner;
use sheetgaze::app::reload::{ReloadContext, ReloadHandle};
use sheetgaze::app::state::AppState;
use sheetgaze::app::terminal::TerminalGuard;
use sheetgaze::config::{Cli, Config};
use sheetgaze::domain::reload::WatchMode;
use sheetgaze::io::clipboard::ClipboardService;
use sheetgaze::io::loader::WorkbookLoader;
use sheetgaze::ui::screen::{Screen, SnapshotStyle};
use sheetgaze::ui::theme::Theme;

fn main() {
    let cli = Cli::parse();
    let config = match Config::from_cli(cli) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("sheetgaze: {message}");
            std::process::exit(2);
        }
    };
    let path = Path::new(&config.cli.file);
    let loader = match WorkbookLoader::from_config(path, &config) {
        Ok(loader) => loader,
        Err(error) => {
            eprintln!("sheetgaze: {error}");
            std::process::exit(1);
        }
    };
    let mut workbook = match loader.load() {
        Ok(workbook) => workbook,
        Err(error) => {
            eprintln!("sheetgaze: {error}");
            std::process::exit(1);
        }
    };
    if let Some(sheet) = &config.cli.sheet
        && let Some(index) = workbook.sheets.iter().position(|item| &item.name == sheet)
    {
        workbook.active_sheet = index;
    }
    let formula_config = config.recalc;
    let mut state = AppState::new(workbook, config.clone());
    let mut formula = FormulaRunner::new();
    let start = state
        .workbook
        .as_mut()
        .map(|workbook| formula.restart(workbook, &formula_config));
    if let Some(reason) = start
        .as_ref()
        .and_then(|start| start.skipped_reason.clone())
    {
        state.status_message = Some(reason);
    }
    if config.cli.print {
        let started = start.is_some_and(|start| start.started);
        if started && let Some(message) = formula.wait() {
            event_loop::apply_formula_message(&mut state, message);
        }
        state.set_watch_state(WatchMode::Off, WatchMode::Off);
        let (width, height) = print_size(&config.cli);
        state.set_size_print(width, height);
        let theme = Theme::new_with_fill(config.cli.color, config.cli.fill_mode);
        let style = if config.cli.no_ansi {
            SnapshotStyle::Plain
        } else {
            SnapshotStyle::Ansi
        };
        let rendered = match Screen::snapshot(&state.view(), &theme, width, height, style) {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("sheetgaze: {error}");
                std::process::exit(1);
            }
        };
        if let Err(error) = write_stdout(&rendered) {
            eprintln!("sheetgaze: {error}");
            std::process::exit(1);
        }
        return;
    }
    let reload = match ReloadHandle::spawn(ReloadContext {
        loader: loader.clone(),
        watch: config.watch,
    }) {
        Ok(handle) => Some(handle),
        Err(error) => {
            state.status_message = Some(format!("reload unavailable: {error}"));
            None
        }
    };
    let theme = Theme::new_with_fill(config.cli.color, config.cli.fill_mode);
    let mut clipboard =
        ClipboardService::from_mode(config.cli.clipboard, config.cli.osc52_max_bytes);
    let mut guard = match TerminalGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("sheetgaze: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = EventLoop::run(
        &mut guard,
        &mut state,
        reload,
        &mut formula,
        &mut clipboard,
        &theme,
    ) {
        eprintln!("sheetgaze: {error}");
        std::process::exit(1);
    }
}

/// Resolves the snapshot size from CLI overrides and the terminal, if any.
fn print_size(cli: &Cli) -> (u16, u16) {
    let (terminal_width, terminal_height) = crossterm::terminal::size().unwrap_or((100, 30));
    let width = cli.width.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            terminal_width.max(40)
        } else {
            100
        }
    });
    let height = cli.height.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            terminal_height.max(10)
        } else {
            30
        }
    });
    (width, height)
}

fn write_stdout(text: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
