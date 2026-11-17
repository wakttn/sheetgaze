#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Visual,
    Insert,
    Command,
    Detail,
    Help,
    Search,
    Filter,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Visual => "VISUAL",
            Self::Insert => "INSERT",
            Self::Command => "COMMAND",
            Self::Detail => "DETAIL",
            Self::Help => "HELP",
            Self::Search => "SEARCH",
            Self::Filter => "FILTER",
        }
    }

    pub fn is_editing(self) -> bool {
        matches!(self, Self::Insert | Self::Command)
    }
}
