use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::action::{Action, Motion};
use crate::domain::mode::Mode;

#[derive(Default)]
pub struct Keymap {
    pending_g: bool,
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn translate(&mut self, key: KeyEvent, mode: Mode) -> Option<Action> {
        if mode == Mode::Search {
            return match key.code {
                KeyCode::Char(ch) => Some(Action::SearchInput(ch)),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Enter => Some(Action::SearchSubmit),
                KeyCode::Esc => Some(Action::SearchCancel),
                _ => None,
            };
        }
        if mode == Mode::Filter {
            return match key.code {
                KeyCode::Char('j') | KeyCode::Down => Some(Action::FilterMove(1)),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::FilterMove(-1)),
                KeyCode::Char(' ') => Some(Action::FilterToggle),
                KeyCode::Char('a') => Some(Action::FilterToggleAll),
                KeyCode::Char('c') => Some(Action::FilterClear),
                KeyCode::Esc | KeyCode::Char('f') | KeyCode::Char('q') => Some(Action::FilterClose),
                _ => None,
            };
        }
        if mode == Mode::Detail {
            return match key.code {
                KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDetail(1)),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollDetail(-1)),
                KeyCode::Char('y') => Some(Action::Copy),
                KeyCode::Esc | KeyCode::Char('q') => Some(Action::CloseDetail),
                _ => None,
            };
        }
        if mode == Mode::Help {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => Some(Action::HelpClose),
                _ => None,
            };
        }
        if self.pending_g && !matches!(key.code, KeyCode::Char('g')) {
            self.pending_g = false;
        }
        if mode == Mode::Visual {
            return match key.code {
                KeyCode::Char('h') | KeyCode::Left => Some(Action::Extend(Motion::Left)),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::Extend(Motion::Down)),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::Extend(Motion::Up)),
                KeyCode::Char('l') | KeyCode::Right => Some(Action::Extend(Motion::Right)),
                KeyCode::Char('g') => {
                    if self.pending_g {
                        self.pending_g = false;
                        Some(Action::Extend(Motion::DataTop))
                    } else {
                        self.pending_g = true;
                        None
                    }
                }
                KeyCode::Char('G') => Some(Action::Extend(Motion::DataBottom)),
                KeyCode::Char('{') => Some(Action::Extend(Motion::Top)),
                KeyCode::Char('}') => Some(Action::Extend(Motion::Bottom)),
                KeyCode::Char('0') | KeyCode::Home => Some(Action::Extend(Motion::LineStart)),
                KeyCode::Char('$') | KeyCode::End => Some(Action::Extend(Motion::LineEnd)),
                KeyCode::Char('^') => Some(Action::Extend(Motion::FirstNonBlank)),
                KeyCode::Char('y') => Some(Action::Copy),
                KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('V') => {
                    Some(Action::ToggleVisual)
                }
                _ => None,
            };
        }
        match key.code {
            KeyCode::Esc => Some(Action::SearchCancel),
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('?') => Some(Action::HelpOpen),
            KeyCode::Char('h') | KeyCode::Left => Some(Action::Move(Motion::Left)),
            KeyCode::Char('j') | KeyCode::Down => Some(Action::Move(Motion::Down)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::Move(Motion::Up)),
            KeyCode::Char('l') | KeyCode::Right => Some(Action::Move(Motion::Right)),
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.pending_g = false;
                    Some(Action::Move(Motion::DataTop))
                } else {
                    self.pending_g = true;
                    None
                }
            }
            KeyCode::Char('G') => Some(Action::Move(Motion::DataBottom)),
            KeyCode::Char('{') => Some(Action::Move(Motion::Top)),
            KeyCode::Char('}') => Some(Action::Move(Motion::Bottom)),
            KeyCode::Char('0') | KeyCode::Home => Some(Action::Move(Motion::LineStart)),
            KeyCode::Char('$') | KeyCode::End => Some(Action::Move(Motion::LineEnd)),
            KeyCode::Char('^') => Some(Action::Move(Motion::FirstNonBlank)),
            KeyCode::PageUp => Some(Action::Move(Motion::PageUp)),
            KeyCode::PageDown => Some(Action::Move(Motion::PageDown)),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Move(Motion::HalfDown))
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Move(Motion::HalfUp))
            }
            KeyCode::Char('v') => Some(Action::ToggleVisual),
            KeyCode::Char('V') => Some(Action::ToggleLineVisual),
            KeyCode::Char('y') => Some(Action::Copy),
            KeyCode::Enter | KeyCode::Char('d') => Some(Action::OpenDetail),
            KeyCode::Char('/') => Some(Action::SearchOpen),
            KeyCode::Char('n') => Some(Action::SearchNext),
            KeyCode::Char('N') => Some(Action::SearchPrevious),
            KeyCode::Char('f') => Some(Action::FilterOpen),
            KeyCode::Char('r') => Some(Action::Reload),
            KeyCode::Tab | KeyCode::Char(']') => Some(Action::SwitchSheet(1)),
            KeyCode::BackTab | KeyCode::Char('[') => Some(Action::SwitchSheet(-1)),
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.pending_g = false;
    }
}
