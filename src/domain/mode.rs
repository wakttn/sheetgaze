#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Visual,
    Detail,
    Help,
    Search,
    Filter,
}
