#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    HalfUp,
    HalfDown,
    Top,
    Bottom,
    DataTop,
    DataBottom,
    LineStart,
    LineEnd,
    FirstNonBlank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Move(Motion),
    Extend(Motion),
    ToggleVisual,
    ToggleLineVisual,
    Copy,
    OpenDetail,
    CloseDetail,
    ScrollDetail(i32),
    SwitchSheet(i32),
    SearchOpen,
    SearchInput(char),
    SearchBackspace,
    SearchSubmit,
    SearchCancel,
    SearchNext,
    SearchPrevious,
    FilterOpen,
    FilterMove(i32),
    FilterToggle,
    FilterToggleAll,
    FilterClear,
    FilterClose,
    Reload,
    HelpOpen,
    HelpClose,
    Resize(u16, u16),
    Quit,
}

#[derive(Clone, Debug)]
pub enum Effect {
    WriteClipboard(String),
    SetStatus(String),
    Reload,
    Quit,
}
