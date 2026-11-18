use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::action::{Action, Motion};
use crate::domain::edit::{EditCommit, EditCursor};
use crate::domain::mode::Mode;
use crate::domain::register::PasteSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingOperator {
    Delete,
    Yank,
}

#[derive(Default)]
pub struct Keymap {
    pending_g: bool,
    pending_operator: Option<PendingOperator>,
    pending_replace: bool,
    pending_z: bool,
    awaiting_register: bool,
    next_register: Option<PasteSource>,
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn translate(&mut self, key: KeyEvent, mode: Mode) -> Option<Action> {
        if mode == Mode::Insert {
            return self.translate_insert(key);
        }
        if mode == Mode::Command {
            return self.translate_command(key);
        }
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

        if self.awaiting_register {
            self.awaiting_register = false;
            self.next_register = match key.code {
                KeyCode::Char('0') => Some(PasteSource::Yank),
                KeyCode::Char('+') => Some(PasteSource::System),
                KeyCode::Char('"') => Some(PasteSource::Unnamed),
                KeyCode::Esc => None,
                _ => None,
            };
            return None;
        }
        if key.code == KeyCode::Char('"') && !key.modifiers.contains(KeyModifiers::CONTROL) {
            self.awaiting_register = true;
            return None;
        }

        if self.pending_replace {
            self.pending_replace = false;
            return match key.code {
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(Action::ReplaceChar(ch))
                }
                KeyCode::Esc => None,
                _ => None,
            };
        }
        if self.pending_z {
            self.pending_z = false;
            return match key.code {
                KeyCode::Char('Z') => Some(Action::Save {
                    force: false,
                    quit_after: true,
                }),
                KeyCode::Char('Q') => Some(Action::ForceQuit),
                _ => None,
            };
        }
        if let Some(operator) = self.pending_operator {
            if key.code == KeyCode::Esc {
                self.pending_operator = None;
                return None;
            }
            if operator == PendingOperator::Delete {
                self.pending_operator = None;
                if key.code == KeyCode::Char('d') {
                    return Some(Action::CutRow);
                }
                if let Some(motion) = motion_for_key(key) {
                    return Some(Action::CutMotion(motion));
                }
                return None;
            }
            if operator == PendingOperator::Yank {
                self.pending_operator = None;
                if key.code == KeyCode::Char('y') {
                    return Some(Action::YankRow);
                }
                return None;
            }
        }
        if self.pending_g
            && !matches!(
                key.code,
                KeyCode::Char('g')
                    | KeyCode::Char('t')
                    | KeyCode::Char('T')
                    | KeyCode::Char('i')
                    | KeyCode::Char('A')
            )
        {
            self.pending_g = false;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('r') => return Some(Action::Redo),
                KeyCode::Char('s') => {
                    return Some(Action::Save {
                        force: false,
                        quit_after: false,
                    });
                }
                KeyCode::Char('o') => return Some(Action::JumpBack),
                KeyCode::Char('i') => return Some(Action::JumpForward),
                KeyCode::Char('v') => return Some(Action::ToggleBlockVisual),
                KeyCode::Tab => return Some(Action::JumpForward),
                KeyCode::Char('d') => return Some(Action::Move(Motion::HalfDown)),
                KeyCode::Char('u') => return Some(Action::Move(Motion::HalfUp)),
                _ => {}
            }
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
                KeyCode::Char('d') | KeyCode::Char('x') => Some(Action::CutSelection),
                KeyCode::Char('r') => {
                    self.pending_replace = true;
                    None
                }
                KeyCode::Char(':') => Some(Action::CommandOpen),
                KeyCode::Char('K') => Some(Action::OpenDetail),
                KeyCode::Char('u') => Some(Action::Undo),
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
            KeyCode::Char('t') if self.pending_g => {
                self.pending_g = false;
                Some(Action::SwitchSheet(1))
            }
            KeyCode::Char('T') if self.pending_g => {
                self.pending_g = false;
                Some(Action::SwitchSheet(-1))
            }
            KeyCode::Char('i') if self.pending_g => {
                self.pending_g = false;
                Some(Action::InsertColumns {
                    before: true,
                    enter_insert: true,
                    count: 1,
                })
            }
            KeyCode::Char('A') if self.pending_g => {
                self.pending_g = false;
                Some(Action::InsertColumns {
                    before: false,
                    enter_insert: true,
                    count: 1,
                })
            }
            KeyCode::Char('G') => Some(Action::Move(Motion::DataBottom)),
            KeyCode::Char('{') => Some(Action::Move(Motion::Top)),
            KeyCode::Char('}') => Some(Action::Move(Motion::Bottom)),
            KeyCode::Char('0') | KeyCode::Home => Some(Action::Move(Motion::LineStart)),
            KeyCode::Char('$') | KeyCode::End => Some(Action::Move(Motion::LineEnd)),
            KeyCode::Char('^') => Some(Action::Move(Motion::FirstNonBlank)),
            KeyCode::PageUp => Some(Action::Move(Motion::PageUp)),
            KeyCode::PageDown => Some(Action::Move(Motion::PageDown)),
            KeyCode::Char('v') => Some(Action::ToggleVisual),
            KeyCode::Char('V') => Some(Action::ToggleLineVisual),
            KeyCode::Char('C') => Some(Action::ToggleColumnVisual),
            KeyCode::Char('w') => Some(Action::ToggleFitWidth),
            KeyCode::Char('y') => Some(Action::Copy),
            KeyCode::Char('Y') => Some(Action::YankRow),
            KeyCode::Enter => Some(Action::OpenDetail),
            KeyCode::Char('K') => Some(Action::OpenDetail),
            KeyCode::Char('/') => Some(Action::SearchOpen),
            KeyCode::Char('n') => Some(Action::SearchNext),
            KeyCode::Char('N') => Some(Action::SearchPrevious),
            KeyCode::Char('*') => Some(Action::SearchWord(true)),
            KeyCode::Char('#') => Some(Action::SearchWord(false)),
            KeyCode::Char('f') => Some(Action::FilterOpen),
            KeyCode::Char(':') => Some(Action::CommandOpen),
            KeyCode::Char('i') | KeyCode::F(2) => Some(Action::EditStart(EditCursor::Start)),
            KeyCode::Char('a') => Some(Action::EditStart(EditCursor::End)),
            KeyCode::Char('I') => Some(Action::EditStart(EditCursor::Start)),
            KeyCode::Char('A') => Some(Action::EditStart(EditCursor::End)),
            KeyCode::Char('s') => Some(Action::EditClear),
            KeyCode::Char('=') => Some(Action::EditFormula),
            KeyCode::Char('x') => Some(Action::Cut),
            KeyCode::Char('r') => {
                self.pending_replace = true;
                None
            }
            KeyCode::Char('d') => {
                self.pending_operator = Some(PendingOperator::Delete);
                None
            }
            KeyCode::Char('D') => Some(Action::CutMotion(Motion::LineEnd)),
            KeyCode::Char('p') => Some(Action::Paste {
                before: false,
                source: self.next_register.take().unwrap_or(PasteSource::Unnamed),
            }),
            KeyCode::Char('P') => Some(Action::Paste {
                before: true,
                source: self.next_register.take().unwrap_or(PasteSource::Unnamed),
            }),
            KeyCode::Char('o') => Some(Action::InsertRows {
                before: false,
                enter_insert: true,
                count: 1,
            }),
            KeyCode::Char('O') => Some(Action::InsertRows {
                before: true,
                enter_insert: true,
                count: 1,
            }),
            KeyCode::Char('u') => Some(Action::Undo),
            KeyCode::Char('Z') => {
                self.pending_z = true;
                None
            }
            KeyCode::Tab | KeyCode::Char(']') => Some(Action::SwitchSheet(1)),
            KeyCode::BackTab | KeyCode::Char('[') => Some(Action::SwitchSheet(-1)),
            _ => None,
        }
    }

    fn translate_insert(&mut self, key: KeyEvent) -> Option<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Some(Action::EditCancel),
                KeyCode::Char('w') => Some(Action::EditDeleteWord),
                KeyCode::Char('u') => Some(Action::EditDeleteToStart),
                _ => None,
            };
        }
        match key.code {
            KeyCode::Char(ch) => Some(Action::EditInput(ch)),
            KeyCode::Backspace => Some(Action::EditBackspace),
            KeyCode::Delete => Some(Action::EditDelete),
            KeyCode::Left => Some(Action::EditLeft),
            KeyCode::Right => Some(Action::EditRight),
            KeyCode::Home => Some(Action::EditHome),
            KeyCode::End => Some(Action::EditEnd),
            KeyCode::Enter => Some(Action::EditCommit(EditCommit::Down)),
            KeyCode::Tab => Some(Action::EditCommit(EditCommit::Right)),
            KeyCode::BackTab => Some(Action::EditCommit(EditCommit::Left)),
            KeyCode::Esc => Some(Action::EditCommit(EditCommit::Stay)),
            _ => None,
        }
    }

    fn translate_command(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::CommandInput(ch))
            }
            KeyCode::Backspace => Some(Action::CommandBackspace),
            KeyCode::Enter => Some(Action::CommandSubmit),
            KeyCode::Esc => Some(Action::CommandCancel),
            KeyCode::Up => Some(Action::CommandHistory(1)),
            KeyCode::Down => Some(Action::CommandHistory(-1)),
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.pending_g = false;
        self.pending_operator = None;
        self.pending_replace = false;
        self.pending_z = false;
        self.awaiting_register = false;
        self.next_register = None;
    }
}

fn motion_for_key(key: KeyEvent) -> Option<Motion> {
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => Some(Motion::Left),
        KeyCode::Char('j') | KeyCode::Down => Some(Motion::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Motion::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(Motion::Right),
        KeyCode::Char('0') | KeyCode::Home => Some(Motion::LineStart),
        KeyCode::Char('$') | KeyCode::End => Some(Motion::LineEnd),
        KeyCode::Char('^') => Some(Motion::FirstNonBlank),
        KeyCode::Char('G') => Some(Motion::DataBottom),
        KeyCode::Char('{') => Some(Motion::Top),
        KeyCode::Char('}') => Some(Motion::Bottom),
        KeyCode::PageUp => Some(Motion::PageUp),
        KeyCode::PageDown => Some(Motion::PageDown),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Motion::HalfDown)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Motion::HalfUp),
        _ => None,
    }
}
