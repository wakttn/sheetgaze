use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};

use crate::app::action::{Action, Effect};
use crate::app::formula_runner::FormulaRunner;
use crate::app::keymap::Keymap;
use crate::app::reload::{ReloadHandle, ReloadMessage};
use crate::app::state::AppState;
use crate::app::terminal::TerminalGuard;
use crate::domain::error::AppError;
use crate::io::clipboard::ClipboardService;
use crate::io::formula::FormulaMessage;
use crate::io::watch::WatchNotice;
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
        Effect::Quit => {
            state.should_quit = true;
            true
        }
    }
}
