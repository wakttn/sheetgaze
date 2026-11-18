use crate::config::ColorMode;
use crate::domain::addr::CellAddr;
use crate::domain::filter::{FilterState, VisibilityMap};
use crate::domain::layout::{RenderPlan, Viewport};
use crate::domain::mode::Mode;
use crate::domain::search::SearchIndex;
use crate::domain::selection::{Selection, SelectionMode};

#[derive(Clone, Debug)]
pub struct ScreenView {
    pub mode: Mode,
    pub tabs: Vec<String>,
    pub active_sheet: usize,
    pub focus: CellAddr,
    pub viewport: Viewport,
    pub plan: RenderPlan,
    pub selection: Option<Selection>,
    pub search: Option<SearchHighlight>,
    pub status: StatusView,
    pub detail: Option<DetailView>,
    pub help: Option<HelpView>,
    pub search_prompt: Option<SearchPromptView>,
    pub filter_panel: Option<FilterPanelView>,
    pub input_line: Option<InputLineView>,
}

#[derive(Clone, Debug, Default)]
pub struct InputLineView {
    pub prompt: String,
    pub text: String,
    pub cursor: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SearchHighlight {
    pub matches: Vec<CellAddr>,
    pub current: Option<CellAddr>,
    pub hidden: usize,
}

#[derive(Clone, Debug, Default)]
pub struct StatusView {
    pub file_name: String,
    pub sheet_name: String,
    pub cell_label: String,
    pub mode_label: String,
    pub dirty: bool,
    pub fit_label: String,
    pub selection_count: usize,
    pub selection_kind: Option<SelectionMode>,
    pub formula_label: String,
    pub search_label: String,
    pub filter_label: String,
    pub freeze_label: String,
    pub load_label: String,
    pub reload_label: String,
    pub encoding_label: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DetailView {
    pub address: String,
    pub kind: String,
    pub raw: String,
    pub display: String,
    pub formula: Option<String>,
    pub cached: Option<String>,
    pub evaluated: Option<String>,
    pub status: String,
    pub style: String,
    pub merge: Option<String>,
    pub truncated: bool,
    pub lines: Vec<String>,
    pub scroll: usize,
}

#[derive(Clone, Debug, Default)]
pub struct HelpView {
    pub bindings: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchPromptView {
    pub query: String,
    pub current: usize,
    pub total: usize,
    pub hidden: usize,
    pub searching: bool,
    pub sheet: Option<String>,
    pub cell: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FilterPanelView {
    pub lines: Vec<String>,
    pub selected: usize,
}

#[derive(Clone, Debug)]
pub struct AppViewModel {
    pub color_mode: ColorMode,
    pub sheet_count: usize,
    pub visible_rows: VisibilityMap,
    pub filter_state: FilterState,
    pub search: Option<SearchIndex>,
}
