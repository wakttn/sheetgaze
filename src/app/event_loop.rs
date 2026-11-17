use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};

use crate::app::action::{Action, Effect};
use crate::app::formula_runner::FormulaRunner;
use crate::app::keymap::Keymap;
use crate::app::reload::{ReloadHandle, ReloadMessage, ReloadTrigger};
use crate::app::state::AppState;
use crate::app::terminal::TerminalGuard;
use crate::domain::error::AppError;
use crate::io::clipboard::ClipboardService;
use crate::io::formula::FormulaMessage;
use crate::io::loader::fingerprint;
use crate::io::watch::WatchNotice;
use crate::io::xlsx_write;
use crate::ui::screen::Screen;
use crate::ui::theme::Theme;

pub struct EventLoop;

impl EventLoop {
    pub fn run(
        guard: &mut TerminalGuard,
        state: &mut AppState,
        mut reload: Option<ReloadHandle>,
        formula: &mut FormulaRunner,
        clipboard: &mut ClipboardService,
        theme: &Theme,
    ) -> Result<(), AppError> {
        let mut keymap = Keymap::new();
        // Redraw only when state actually changes. The event poll below still
        // paces worker checks, but an idle loop should not repaint the screen.
        let mut needs_redraw = true;
        loop {
            if needs_redraw {
                let size = guard
                    .terminal
                    .size()
                    .map_err(|error| AppError::Terminal(error.to_string()))?;
                state.set_size(size.width, size.height);
                let view = state.view();
                guard
                    .terminal
                    .draw(|frame| Screen::render(frame, &view, theme))
                    .map_err(|error| AppError::Terminal(error.to_string()))?;
                needs_redraw = false;
            }
            if state.should_quit {
                break;
            }
            let mut state_changed = false;
            if event::poll(Duration::from_millis(80))
                .map_err(|error| AppError::Terminal(error.to_string()))?
            {
                match event::read().map_err(|error| AppError::Terminal(error.to_string()))? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if let Some(action) = keymap.translate(key, state.mode) {
                            state_changed = true;
                            for effect in state.apply(action) {
                                if handle_effect(effect, state, clipboard, reload.as_ref()) {
                                    break;
                                }
                            }
                        }
                    }
                    Event::Resize(width, height) => {
                        let _ = state.apply(Action::Resize(width, height));
                        state_changed = true;
                    }
                    _ => {}
                }
            }
            if poll_formula(state, formula) {
                state_changed = true;
            }
            if state.pending_recalc {
                state.pending_recalc = false;
                if let Some(workbook) = state.workbook.as_ref() {
                    match xlsx_write::write_temp_workbook(workbook) {
                        Ok(temp) => {
                            let start = state.workbook.as_mut().map(|workbook| {
                                formula.restart_with_temp(workbook, &state.config.recalc, temp)
                            });
                            if let Some(reason) = start.and_then(|start| start.skipped_reason) {
                                state.status_message = Some(reason);
                            }
                            state_changed = true;
                        }
                        Err(error) => {
                            state.status_message = Some(format!("recalculation skipped: {error}"));
                        }
                    }
                }
            }
            if let Some(handle) = &reload {
                loop {
                    match handle.events.try_recv() {
                        Ok(message) => {
                            apply_reload_message(state, formula, message);
                            state_changed = true;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            reload = None;
                            break;
                        }
                    }
                }
            }
            needs_redraw |= state_changed;
        }
        guard.restore();
        Ok(())
    }
}

fn apply_reload_message(state: &mut AppState, formula: &mut FormulaRunner, message: ReloadMessage) {
    match message {
        ReloadMessage::Started { trigger } => {
            state.status_message = Some(format!("reloading ({})...", trigger.label()));
        }
        ReloadMessage::Loaded { workbook, trigger } => {
            if trigger == ReloadTrigger::Watch && state.is_dirty() {
                state.status_message = Some(
                    "external change ignored while editing; use :e! to discard or :w! to overwrite"
                        .to_string(),
                );
                return;
            }
            let _summary = state.replace_workbook(workbook, trigger);
            let skipped = state
                .workbook
                .as_mut()
                .map(|workbook| formula.restart(workbook, &state.config.recalc))
                .and_then(|start| start.skipped_reason);
            if let Some(reason) = skipped {
                let message = state
                    .status_message
                    .take()
                    .map(|message| format!("{message}; {reason}"))
                    .unwrap_or(reason);
                state.status_message = Some(message);
            }
        }
        ReloadMessage::Failed {
            trigger,
            attempt,
            message,
            notify,
            retry_in,
        } => {
            if notify {
                state.status_message = Some(format!(
                    "reload failed ({}, attempt {}): {message}; retrying in {}ms",
                    trigger.label(),
                    attempt,
                    retry_in.as_millis()
                ));
            }
        }
        ReloadMessage::WatchNotice(notice) => match notice {
            WatchNotice::Fallback { from, reason } => {
                state.status_message = Some(format!(
                    "watch fallback: {} -> poll ({reason})",
                    from.label()
                ));
            }
            WatchNotice::Unavailable(reason) => {
                state.status_message = Some(format!("watch unavailable: {reason}"));
            }
        },
        ReloadMessage::WatchState {
            configured,
            effective,
        } => {
            state.set_watch_state(configured, effective);
        }
    }
}

fn poll_formula(state: &mut AppState, formula: &mut FormulaRunner) -> bool {
    let mut changed = false;
    for message in formula.poll() {
        apply_formula_message(state, message);
        changed = true;
    }
    changed
}

/// Applies one formula worker message to the app state.
pub fn apply_formula_message(state: &mut AppState, message: FormulaMessage) {
    match message {
        FormulaMessage::Results(results) => {
            if state.is_current_generation(results.generation) {
                state.apply_formula_results(results);
            }
        }
        FormulaMessage::Failed {
            generation,
            message,
        } => {
            if state.is_current_generation(generation) {
                state.status_message = Some(format!("recalculation failed: {message}"));
            }
        }
    }
}

fn handle_effect(
    effect: Effect,
    state: &mut AppState,
    clipboard: &mut ClipboardService,
    reload: Option<&ReloadHandle>,
) -> bool {
    match effect {
        Effect::WriteClipboard(text) => {
            match clipboard.write(&text) {
                Ok(result) => {
                    state.status_message =
                        Some(format!("copied {} bytes via {}", result.bytes, result.sink));
                }
                Err(error) => {
                    state.status_message = Some(format!("copy failed: {error}"));
                }
            }
            false
        }
        Effect::SetStatus(message) => {
            state.status_message = Some(message);
            false
        }
        Effect::Reload => {
            if let Some(handle) = reload {
                handle.reload_now();
            } else {
                state.status_message = Some("reload is not available".to_string());
            }
            false
        }
        Effect::Save { force, quit_after } => {
            if !state.edit_log.is_dirty() {
                if quit_after {
                    state.should_quit = true;
                    return true;
                }
                state.status_message = Some("no changes to write".to_string());
                return false;
            }
            let Some(workbook) = &mut state.workbook else {
                state.status_message = Some("no workbook to save".to_string());
                return false;
            };
            let path = workbook.source.path.clone();
            if !force && fingerprint(&path) != workbook.source.fingerprint {
                state.status_message =
                    Some("file changed on disk; use :w! to overwrite".to_string());
                return false;
            }
            match xlsx_write::save_workbook(workbook) {
                Ok(outcome) => {
                    state.edit_log.reset_after_save();
                    state.pending_recalc = false;
                    let stamp = fingerprint(&path);
                    if let Some(workbook) = &mut state.workbook {
                        workbook.source.fingerprint = stamp;
                    }
                    if let Some(handle) = reload {
                        handle.set_baseline(stamp);
                    }
                    state.status_message = Some(format!(
                        "wrote {} cells to {}",
                        outcome.cells,
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("(file)")
                    ));
                    if quit_after {
                        state.should_quit = true;
                        return true;
                    }
                }
                Err(error) => {
                    state.status_message = Some(format!("save failed: {error}"));
                }
            }
            false
        }
        Effect::Quit => {
            state.should_quit = true;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rust_xlsxwriter::Workbook;

    use super::*;
    use crate::app::action::Action;
    use crate::config::{ClipboardMode, Config};
    use crate::domain::edit::EditCommit;
    use crate::domain::row_store::LazyConfig;
    use crate::io::xlsx::load_xlsx;

    fn unique(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join("sheetgaze-event-loop-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{name}", std::process::id(), id))
    }

    #[test]
    fn save_effect_writes_edits_and_clears_dirty() {
        let path = unique("save-effect.xlsx");
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "before").unwrap();
        workbook.save(&path).unwrap();

        let workbook = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
        let mut state = AppState::new(workbook, Config::default());
        state.set_size(80, 24);
        state.apply(Action::EditClear);
        state.apply(Action::EditInput('a'));
        state.apply(Action::EditCommit(EditCommit::Stay));
        assert!(state.is_dirty());

        let mut clipboard = ClipboardService::from_mode(ClipboardMode::None, 1024);
        assert!(!handle_effect(
            Effect::Save {
                force: false,
                quit_after: false
            },
            &mut state,
            &mut clipboard,
            None
        ));
        assert!(!state.is_dirty());
        assert!(!state.should_quit);

        let reloaded = load_xlsx(&path, &Default::default(), &LazyConfig::default()).unwrap();
        assert_eq!(
            reloaded.sheets[0]
                .effective_cell(crate::domain::addr::CellAddr::new(0, 0))
                .unwrap()
                .display,
            "a"
        );
    }
}
