use crate::app::action::{Action, ClipboardWriteOrigin, Effect, Motion};
use crate::app::paste::PastePlanner;
use crate::app::reload::{ReloadSummary, ReloadTrigger};
use crate::app::structure_flow::{StructureFlow, WorkbookStructureOutcome};
use crate::config::Config;
use crate::domain::addr::{CellAddr, CellRange};
use crate::domain::cell::{Cell, CellRead, CellValue};
use crate::domain::command::{self, ExCommand, SubstituteCommand, SubstituteScope};
use crate::domain::edit::{
    self, EditChange, EditCursor, EditLog, EditOp, EditTransaction, InputKind, InputState,
    StructureEdit,
};
use crate::domain::filter::{FilterSource, FilterState};
use crate::domain::formula::FormulaResults;
use crate::domain::jump::{JumpEntry, JumpList};
use crate::domain::layout::{ColumnWidths, LayoutEngine, RenderPlan, Viewport};
use crate::domain::mode::Mode;
use crate::domain::register::{
    ClipboardBlock, PasteSource, RegisterKind, RegisterLimits, RegisterStore,
};
use crate::domain::reload::WatchMode;
use crate::domain::row_store::RowRange;
use crate::domain::search::{MatchSource, SearchIndex, SearchMatch, SearchQuery};
use crate::domain::selection::{Selection, SelectionMode};
use crate::domain::sheet::Sheet;
use crate::domain::structure::{Axis, StructureOp, StructureWarning};
use crate::domain::task::Generation;
use crate::domain::viewport::ViewportPlanner;
use crate::domain::workbook::{SourceFormat, WorkbookModel};
use crate::io::tsv::TsvCodec;
use crate::ui::view::{
    DetailView, FilterPanelView, HelpView, InputLineView, ScreenView, SearchHighlight,
    SearchPromptView, StatusView,
};

pub struct AppState {
    pub workbook: Option<WorkbookModel>,
    pub focus: CellAddr,
    pub selection: Option<Selection>,
    pub mode: Mode,
    pub fit_width: bool,
    pub viewport: Viewport,
    pub detail_scroll: usize,
    pub search_query: String,
    pub search_index: Option<SearchIndex>,
    pub last_search: Option<SearchQuery>,
    pub status_message: Option<String>,
    pub config: Config,
    pub should_quit: bool,
    pub search_running: bool,
    pub selected_filter: usize,
    pub watch_configured: WatchMode,
    pub watch_effective: WatchMode,
    pub edit_log: EditLog,
    pub input: Option<InputState>,
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,
    pub jump_list: JumpList,
    pub registers: RegisterStore,
    pub pending_recalc: bool,
    pub read_only: bool,
}

impl AppState {
    pub fn new(workbook: WorkbookModel, config: Config) -> Self {
        let active = workbook.active_sheet;
        let focus = CellAddr::new(0, 0);
        let watch_mode = config.watch.mode;
        let read_only = config.cli.read_only
            || workbook.source.format != SourceFormat::Xlsx
            || std::fs::metadata(&workbook.source.path)
                .map(|meta| meta.permissions().readonly())
                .unwrap_or(false);
        Self {
            workbook: Some(workbook),
            focus,
            selection: None,
            mode: Mode::Normal,
            fit_width: config.cli.fit_width,
            viewport: Viewport {
                top_visible_row: 0,
                left_col: 0,
                height: 20,
                width: 80,
            },
            detail_scroll: 0,
            search_query: String::new(),
            search_index: None,
            last_search: None,
            status_message: None,
            config,
            should_quit: false,
            search_running: false,
            selected_filter: 0,
            watch_configured: watch_mode,
            watch_effective: watch_mode,
            edit_log: EditLog::new(),
            input: None,
            command_history: Vec::new(),
            command_history_index: None,
            jump_list: JumpList::new(),
            registers: RegisterStore::default(),
            pending_recalc: false,
            read_only,
        }
        .with_sheet(active)
        .with_focus(focus)
    }

    fn with_sheet(mut self, index: usize) -> Self {
        if let Some(workbook) = &mut self.workbook {
            workbook.active_sheet = index;
        }
        self
    }

    fn with_focus(mut self, focus: CellAddr) -> Self {
        self.focus = focus;
        self
    }

    pub fn set_watch_state(&mut self, configured: WatchMode, effective: WatchMode) {
        self.watch_configured = configured;
        self.watch_effective = effective;
    }

    pub fn is_current_generation(&self, generation: u64) -> bool {
        self.workbook
            .as_ref()
            .is_some_and(|workbook| workbook.generation.0 == generation)
    }

    pub fn replace_workbook(
        &mut self,
        mut workbook: WorkbookModel,
        trigger: ReloadTrigger,
    ) -> ReloadSummary {
        let previous_sheet = self
            .workbook
            .as_ref()
            .and_then(|workbook| workbook.active().map(|sheet| sheet.name.clone()));
        let snapshots = self
            .workbook
            .as_ref()
            .map(snapshot_filters)
            .unwrap_or_default();
        let previous_focus = self.focus;
        let previous_selection = self.selection;
        let previous_mode = self.mode;
        let previous_selected_filter = self.selected_filter;
        let previous_detail_scroll = self.detail_scroll;
        let previous_generation = self
            .workbook
            .as_ref()
            .map(|workbook| workbook.generation)
            .unwrap_or_default();
        let search_cleared = self.search_index.take().is_some();

        workbook.generation = previous_generation;

        let index = previous_sheet
            .as_ref()
            .and_then(|name| workbook.sheets.iter().position(|sheet| &sheet.name == name));
        let sheet_fallback = previous_sheet.is_some() && index.is_none();
        workbook.active_sheet = index.unwrap_or(0);
        let filter_reset = apply_filter_snapshots(&mut workbook, &snapshots);

        self.workbook = Some(workbook);
        self.mode = previous_mode;
        self.detail_scroll = previous_detail_scroll;
        self.selected_filter = previous_selected_filter;

        let mut focus_moved = false;
        if let Some(sheet) = self
            .workbook
            .as_ref()
            .and_then(|workbook| workbook.active())
        {
            let end = sheet.used_range.end;
            let mut target =
                CellAddr::new(self.focus.row.min(end.row), self.focus.col.min(end.col));
            target = sheet.merges.anchor_for(target);
            if !sheet.visibility.is_visible(target.row) {
                let visible = sheet.visibility.visible_rows();
                let row = visible
                    .iter()
                    .copied()
                    .find(|row| *row >= target.row)
                    .or_else(|| visible.last().copied())
                    .unwrap_or(target.row);
                if !visible.is_empty() {
                    target.row = row;
                }
            }
            focus_moved = target != previous_focus;
            self.focus = target;

            if let Some(selection) = previous_selection
                && (!sheet.used_range.contains(selection.anchor)
                    || !sheet.used_range.contains(selection.cursor))
            {
                self.selection = None;
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                }
            } else {
                self.selection = previous_selection;
            }
        } else {
            self.selection = None;
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
            }
        }

        self.ensure_focus_loaded();
        match self.mode {
            Mode::Detail if self.detail_view().is_none() => self.mode = Mode::Normal,
            Mode::Filter
                if self
                    .active_sheet()
                    .is_none_or(|sheet| sheet.filters.is_empty()) =>
            {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
        let max_filter = self.filter_line_count().saturating_sub(1);
        self.selected_filter = self.selected_filter.min(max_filter);
        self.search_index = None;
        let edits_discarded = self.edit_log.is_dirty();
        self.edit_log.clear();
        self.jump_list.clear();
        self.input = None;
        self.pending_recalc = false;
        if matches!(self.mode, Mode::Insert | Mode::Command) {
            self.mode = Mode::Normal;
        }
        self.read_only = self.config.cli.read_only
            || self
                .workbook
                .as_ref()
                .is_some_and(|workbook| workbook.source.format != SourceFormat::Xlsx)
            || self
                .workbook
                .as_ref()
                .and_then(|workbook| std::fs::metadata(&workbook.source.path).ok())
                .is_some_and(|meta| meta.permissions().readonly());
        self.ensure_focus_visible();

        let summary = ReloadSummary {
            sheet_fallback,
            filter_reset,
            focus_moved,
            search_cleared,
        };
        let mut message = format!("reloaded ({})", trigger.label());
        if sheet_fallback {
            message.push_str("; previous sheet not found");
        }
        if filter_reset {
            message.push_str("; filter settings reset");
        }
        if search_cleared {
            message.push_str("; search results cleared");
        }
        if edits_discarded {
            message.push_str("; unsaved edits discarded");
        }
        self.status_message = Some(message);
        summary
    }

    /// Updates the viewport for the interactive TUI.
    ///
    /// The terminal rows are shared by the status line, the sheet tabs, and
    /// the grid. The grid itself also reserves one row for its column header,
    /// so only `height - 3` data rows can be shown.
    pub fn set_size(&mut self, width: u16, height: u16) {
        self.set_size_with_chrome(width, height, 3);
    }

    /// Updates the viewport for a print snapshot.
    ///
    /// Print snapshots omit the sheet tabs. The remaining chrome is the status
    /// line and the grid column header, leaving `height - 2` data rows.
    pub fn set_size_print(&mut self, width: u16, height: u16) {
        self.set_size_with_chrome(width, height, 2);
    }

    fn set_size_with_chrome(&mut self, width: u16, height: u16, chrome_rows: u16) {
        self.viewport.width = width.saturating_sub(7).max(10);
        self.viewport.height = height.saturating_sub(chrome_rows).max(3);
    }

    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::EditStart(cursor) => {
                self.start_edit(cursor, None);
                Vec::new()
            }
            Action::EditClear => {
                self.start_edit(EditCursor::Start, Some(String::new()));
                Vec::new()
            }
            Action::EditFormula => {
                self.start_edit(EditCursor::End, Some("=".to_string()));
                Vec::new()
            }
            Action::EditInput(ch) => {
                if let Some(input) = &mut self.input {
                    input.insert(ch);
                }
                Vec::new()
            }
            Action::EditBackspace => {
                if let Some(input) = &mut self.input {
                    input.backspace();
                }
                Vec::new()
            }
            Action::EditDelete => {
                if let Some(input) = &mut self.input {
                    input.delete();
                }
                Vec::new()
            }
            Action::EditLeft => {
                if let Some(input) = &mut self.input {
                    input.left();
                }
                Vec::new()
            }
            Action::EditRight => {
                if let Some(input) = &mut self.input {
                    input.right();
                }
                Vec::new()
            }
            Action::EditHome => {
                if let Some(input) = &mut self.input {
                    input.home();
                }
                Vec::new()
            }
            Action::EditEnd => {
                if let Some(input) = &mut self.input {
                    input.end();
                }
                Vec::new()
            }
            Action::EditDeleteWord => {
                if let Some(input) = &mut self.input {
                    input.delete_word();
                }
                Vec::new()
            }
            Action::EditDeleteToStart => {
                if let Some(input) = &mut self.input {
                    input.delete_to_start();
                }
                Vec::new()
            }
            Action::EditCommit(direction) => {
                self.commit_edit(direction);
                Vec::new()
            }
            Action::EditCancel => {
                self.cancel_edit();
                Vec::new()
            }
            Action::CommandOpen => {
                self.open_command();
                Vec::new()
            }
            Action::CommandInput(ch) => {
                if let Some(input) = &mut self.input {
                    input.insert(ch);
                }
                Vec::new()
            }
            Action::CommandBackspace => {
                if let Some(input) = &mut self.input {
                    input.backspace();
                }
                Vec::new()
            }
            Action::CommandHistory(delta) => {
                self.command_history(delta);
                Vec::new()
            }
            Action::CommandSubmit => self.submit_command(),
            Action::CommandCancel => {
                self.input = None;
                self.mode = Mode::Normal;
                Vec::new()
            }
            Action::Undo => {
                self.undo();
                Vec::new()
            }
            Action::Redo => {
                self.redo();
                Vec::new()
            }
            Action::ClearCell => {
                self.clear_cell_or_selection();
                Vec::new()
            }
            Action::ClearRow => {
                self.clear_row();
                Vec::new()
            }
            Action::ClearMotion(motion) => {
                self.clear_motion(motion);
                Vec::new()
            }
            Action::ReplaceChar(ch) => {
                self.replace_char(ch);
                Vec::new()
            }
            Action::SearchWord(forward) => {
                self.search_word(forward);
                Vec::new()
            }
            Action::JumpBack => {
                if let Some(entry) = self.jump_list.back() {
                    self.jump_to_without_history(entry.sheet, entry.addr);
                }
                Vec::new()
            }
            Action::JumpForward => {
                if let Some(entry) = self.jump_list.forward() {
                    self.jump_to_without_history(entry.sheet, entry.addr);
                }
                Vec::new()
            }
            Action::Save { force, quit_after } => {
                if self.edit_log.is_dirty() {
                    vec![Effect::Save { force, quit_after }]
                } else if quit_after {
                    self.should_quit = true;
                    vec![Effect::Quit]
                } else {
                    self.status_message = Some("no changes to write".to_string());
                    Vec::new()
                }
            }
            Action::ForceQuit => {
                self.should_quit = true;
                vec![Effect::Quit]
            }
            Action::Move(motion) => {
                self.move_focus(motion, false);
                Vec::new()
            }
            Action::Extend(motion) => {
                self.move_focus(motion, true);
                Vec::new()
            }
            Action::ToggleVisual => {
                if self.selection.is_some() {
                    self.selection = None;
                    self.mode = Mode::Normal;
                } else {
                    self.selection = Some(Selection::new(self.focus));
                    self.mode = Mode::Visual;
                }
                Vec::new()
            }
            Action::ToggleLineVisual => {
                self.selection = Some(Selection::row(self.focus));
                self.mode = Mode::Visual;
                Vec::new()
            }
            Action::ToggleColumnVisual => {
                self.selection = Some(Selection::column(self.focus));
                self.mode = Mode::Visual;
                Vec::new()
            }
            Action::ToggleBlockVisual => {
                self.selection = Some(Selection::block(self.focus));
                self.mode = Mode::Visual;
                Vec::new()
            }
            Action::ToggleFitWidth => {
                self.fit_width = !self.fit_width;
                if self.fit_width {
                    self.viewport.left_col = 0;
                }
                self.ensure_focus_visible();
                Vec::new()
            }
            Action::Copy => self.yank_selection(),
            Action::YankRow => self.yank_row(),
            Action::Cut => self.cut_current(),
            Action::CutRow => self.cut_rows(1),
            Action::CutSelection => self.cut_selection(),
            Action::CutMotion(motion) => self.cut_motion(motion),
            Action::Paste { before, source } => self.paste(before, source),
            Action::InsertRows {
                before,
                enter_insert,
                count,
            } => self.insert_rows(before, enter_insert, count),
            Action::InsertColumns {
                before,
                enter_insert,
                count,
            } => self.insert_columns(before, enter_insert, count),
            Action::DeleteRows { before, count } => self.delete_rows(before, count),
            Action::DeleteColumns { before, count } => self.delete_columns(before, count),
            Action::OpenDetail => {
                self.ensure_focus_loaded();
                if self.detail_view().is_some() {
                    self.mode = Mode::Detail;
                    self.detail_scroll = 0;
                } else {
                    self.status_message = Some("row is still loading".to_string());
                }
                Vec::new()
            }
            Action::CloseDetail => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Action::ScrollDetail(delta) => {
                let max = self
                    .detail_view()
                    .map(|detail| detail.lines.len().saturating_sub(1))
                    .unwrap_or(0);
                self.detail_scroll =
                    (self.detail_scroll as i32 + delta).clamp(0, max as i32) as usize;
                Vec::new()
            }
            Action::SwitchSheet(delta) => {
                self.switch_sheet(delta);
                Vec::new()
            }
            Action::SearchOpen => {
                self.mode = Mode::Search;
                self.search_query.clear();
                Vec::new()
            }
            Action::SearchInput(ch) => {
                self.search_query.push(ch);
                Vec::new()
            }
            Action::SearchBackspace => {
                self.search_query.pop();
                Vec::new()
            }
            Action::SearchSubmit => {
                self.submit_search();
                self.mode = Mode::Normal;
                Vec::new()
            }
            Action::SearchCancel => {
                self.mode = Mode::Normal;
                self.search_index = None;
                Vec::new()
            }
            Action::SearchNext => {
                self.next_match(true);
                Vec::new()
            }
            Action::SearchPrevious => {
                self.next_match(false);
                Vec::new()
            }
            Action::FilterOpen => {
                if self
                    .active_sheet()
                    .is_some_and(|sheet| !sheet.filters.is_empty())
                {
                    self.mode = Mode::Filter;
                } else {
                    self.status_message = Some("no saved filters on this sheet".to_string());
                }
                Vec::new()
            }
            Action::FilterMove(delta) => {
                let max = self.filter_line_count().saturating_sub(1) as i32;
                self.selected_filter = (self.selected_filter as i32 + delta).clamp(0, max) as usize;
                Vec::new()
            }
            Action::FilterToggle => {
                self.toggle_filter();
                Vec::new()
            }
            Action::FilterToggleAll => {
                self.toggle_all_filters();
                Vec::new()
            }
            Action::FilterClear => {
                if let Some(sheet) = self.active_sheet_mut() {
                    sheet.filter_state = FilterState::from_definitions(&sheet.filters, true);
                    self.rebuild_visibility();
                }
                Vec::new()
            }
            Action::FilterClose => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Action::Reload => {
                if self.edit_log.is_dirty() {
                    self.status_message =
                        Some("unsaved changes; use :e! to discard or :w to write".to_string());
                    Vec::new()
                } else {
                    self.status_message = Some("reloading (manual)...".to_string());
                    vec![Effect::Reload]
                }
            }
            Action::ReloadForce => {
                self.input = None;
                self.status_message = Some("reloading (manual)...".to_string());
                vec![Effect::Reload]
            }
            Action::HelpOpen => {
                self.mode = Mode::Help;
                Vec::new()
            }
            Action::HelpClose => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Action::Resize(width, height) => {
                self.set_size(width, height);
                self.ensure_focus_visible();
                Vec::new()
            }
            Action::Quit => {
                if self.edit_log.is_dirty() {
                    self.status_message =
                        Some("unsaved changes; use :wq, :q!, or ZQ to discard".to_string());
                    Vec::new()
                } else {
                    self.should_quit = true;
                    vec![Effect::Quit]
                }
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.edit_log.is_dirty()
    }

    pub fn has_structural_changes(&self) -> bool {
        self.workbook
            .as_ref()
            .is_some_and(|workbook| workbook.sheets.iter().any(|sheet| sheet.structural_dirty))
    }

    pub fn normalize_after_save(&mut self) {
        if let Some(workbook) = &mut self.workbook {
            for sheet in &mut workbook.sheets {
                sheet.normalize_after_structural_save();
                sheet.formula_rewrites.clear();
            }
        }
        self.edit_log.reset_after_save();
        self.pending_recalc = false;
    }

    pub fn editing_enabled(&self) -> bool {
        !self.read_only
    }

    fn editing_allowed(&self) -> bool {
        !self.read_only
            && self
                .workbook
                .as_ref()
                .is_some_and(|workbook| workbook.source.format == SourceFormat::Xlsx)
    }

    fn start_edit(&mut self, cursor: EditCursor, initial: Option<String>) {
        if !self.editing_allowed() {
            self.status_message = Some(if self.read_only {
                "read-only workbook".to_string()
            } else {
                "editing is supported for xlsx only".to_string()
            });
            return;
        }
        self.ensure_focus_loaded();
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let sheet_index = self.active_sheet_index();
        let addr = sheet.merges.anchor_for(self.focus);
        let cell = sheet.raw_cell(addr).cloned();
        let original = cell
            .as_ref()
            .map(edit::input_text_for_cell)
            .unwrap_or_default();
        let text = initial.unwrap_or_else(|| original.clone());
        let mut input = InputState::cell(sheet_index, addr, original, cursor);
        input.set_text(&text);
        input.cursor = match cursor {
            EditCursor::Start => 0,
            EditCursor::End => input.chars.len(),
        };
        self.input = Some(input);
        self.mode = Mode::Insert;
    }

    fn cancel_edit(&mut self) {
        self.input = None;
        self.mode = Mode::Normal;
    }

    fn commit_edit(&mut self, direction: crate::domain::edit::EditCommit) {
        let Some(input) = self.input.take() else {
            self.mode = Mode::Normal;
            return;
        };
        self.mode = Mode::Normal;
        let InputKind::Cell { sheet, addr } = input.kind else {
            return;
        };
        let text = input.text();
        if text != input.original {
            self.apply_cell_text(sheet, addr, &text);
        }
        match direction {
            crate::domain::edit::EditCommit::Down => self.move_focus(Motion::Down, false),
            crate::domain::edit::EditCommit::Right => self.move_focus(Motion::Right, false),
            crate::domain::edit::EditCommit::Left => self.move_focus(Motion::Left, false),
            crate::domain::edit::EditCommit::Stay => {}
        }
    }

    fn apply_cell_text(&mut self, sheet_index: usize, addr: CellAddr, text: &str) {
        if !self.editing_allowed() {
            return;
        }
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        let Some(sheet) = workbook.sheets.get_mut(sheet_index) else {
            return;
        };
        let addr = sheet.merges.anchor_for(addr);
        let before = sheet.raw_cell(addr).cloned();
        let style = before.as_ref().map(|cell| cell.style).unwrap_or_default();
        let number_format = before.as_ref().and_then(|cell| cell.number_format.clone());
        let after = edit::parse_cell_input(text, style, number_format);
        sheet.set_edit_cell(addr, Some(after.clone()));
        self.edit_log
            .record(EditTransaction::single(EditChange::new(
                sheet_index,
                addr,
                before,
                Some(after),
            )));
        self.after_edit(&[sheet_index]);
        self.status_message = Some(format!("edited {}", addr.a1()));
    }

    fn after_edit(&mut self, sheets: &[usize]) {
        if let Some(workbook) = &mut self.workbook {
            workbook.generation = Generation(workbook.generation.0.saturating_add(1));
        }
        for &index in sheets {
            if let Some(sheet) = self
                .workbook
                .as_mut()
                .and_then(|workbook| workbook.sheets.get_mut(index))
            {
                let _ = sheet.rebuild_visibility();
            }
        }
        self.rebuild_visibility();
        self.search_index = None;
        self.pending_recalc = self.has_formulas();
        self.ensure_focus_loaded();
        self.ensure_focus_visible();
    }

    fn has_formulas(&self) -> bool {
        self.workbook.as_ref().is_some_and(|workbook| {
            workbook
                .sheets
                .iter()
                .any(|sheet| !sheet.formula_cells.is_empty())
        })
    }

    fn undo(&mut self) {
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        let Some(transaction) = self.edit_log.undo(workbook) else {
            self.status_message = Some("already at the oldest change".to_string());
            return;
        };
        let sheets = transaction_sheets(&transaction);
        for &index in &sheets {
            if let Some(sheet) = workbook.sheets.get_mut(index) {
                let _ = sheet.rebuild_visibility();
            }
        }
        self.rebuild_visibility();
        self.search_index = None;
        self.pending_recalc = self.has_formulas();
        self.ensure_focus_loaded();
        self.ensure_focus_visible();
        self.status_message = Some(format!("undo: {}", transaction.summary()));
    }

    fn redo(&mut self) {
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        let Some(transaction) = self.edit_log.redo(workbook) else {
            self.status_message = Some("already at the newest change".to_string());
            return;
        };
        let sheets = transaction_sheets(&transaction);
        for &index in &sheets {
            if let Some(sheet) = workbook.sheets.get_mut(index) {
                let _ = sheet.rebuild_visibility();
            }
        }
        self.rebuild_visibility();
        self.search_index = None;
        self.pending_recalc = self.has_formulas();
        self.ensure_focus_loaded();
        self.ensure_focus_visible();
        self.status_message = Some(format!("redo: {}", transaction.summary()));
    }

    fn open_command(&mut self) {
        let mut input = InputState::command();
        if self.selection.is_some() {
            input.set_text("'<,'>");
        }
        self.input = Some(input);
        self.command_history_index = None;
        self.mode = Mode::Command;
    }

    fn command_history(&mut self, delta: i32) {
        if self.command_history.is_empty() {
            return;
        }
        let len = self.command_history.len();
        let current = self.command_history_index.unwrap_or(len);
        let next = if delta > 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(len)
        };
        self.command_history_index = if next == len { None } else { Some(next) };
        if let Some(input) = &mut self.input {
            if let Some(index) = self.command_history_index {
                input.set_text(&self.command_history[index].clone());
            } else {
                input.set_text("");
            }
        }
    }

    fn submit_command(&mut self) -> Vec<Effect> {
        let Some(input) = self.input.take() else {
            self.mode = Mode::Normal;
            return Vec::new();
        };
        let text = input.text();
        if !text.trim().is_empty() && self.command_history.last() != Some(&text) {
            self.command_history.push(text.clone());
        }
        self.command_history_index = None;
        self.mode = Mode::Normal;
        match command::parse(&text) {
            Ok(command) => match self.execute_command(command) {
                Ok(effects) => effects,
                Err(message) => {
                    self.status_message = Some(message);
                    Vec::new()
                }
            },
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn execute_command(&mut self, command: ExCommand) -> Result<Vec<Effect>, String> {
        match command {
            ExCommand::Noop => Ok(Vec::new()),
            ExCommand::Write { force, path } => {
                if path.is_some() {
                    return Err("save-as is not supported yet".to_string());
                }
                Ok(vec![Effect::Save {
                    force,
                    quit_after: false,
                }])
            }
            ExCommand::Quit { force } => {
                if self.edit_log.is_dirty() && !force {
                    return Err("unsaved changes; use :q! to discard".to_string());
                }
                self.should_quit = true;
                Ok(vec![Effect::Quit])
            }
            ExCommand::WriteQuit { force } => {
                if !self.edit_log.is_dirty() {
                    self.should_quit = true;
                    return Ok(vec![Effect::Quit]);
                }
                Ok(vec![Effect::Save {
                    force,
                    quit_after: true,
                }])
            }
            ExCommand::Edit { force } => {
                if self.edit_log.is_dirty() && !force {
                    return Err("unsaved changes; use :e! to discard".to_string());
                }
                self.input = None;
                self.status_message = Some("reloading (manual)...".to_string());
                Ok(vec![Effect::Reload])
            }
            ExCommand::Substitute(substitute) => {
                self.apply_substitute(&substitute)?;
                Ok(Vec::new())
            }
            ExCommand::Goto { sheet, addr } => {
                self.goto_command(sheet, addr)?;
                Ok(Vec::new())
            }
            ExCommand::Sheet(name) => {
                let Some(workbook) = &self.workbook else {
                    return Err("no workbook".to_string());
                };
                let Some(index) = workbook.sheets.iter().position(|sheet| sheet.name == name)
                else {
                    return Err(format!("unknown sheet: {name}"));
                };
                self.activate_sheet(index);
                self.status_message = Some(format!("sheet: {name}"));
                Ok(Vec::new())
            }
            ExCommand::InsertRows { before, count } => Ok(self.insert_rows(before, false, count)),
            ExCommand::InsertColumns { before, count } => {
                Ok(self.insert_columns(before, false, count))
            }
            ExCommand::DeleteRows { before, count } => Ok(self.delete_rows(before, count)),
            ExCommand::DeleteColumns { before, count } => Ok(self.delete_columns(before, count)),
            ExCommand::Put { before, source } => Ok(self.paste(before, source)),
            ExCommand::Help => {
                self.mode = Mode::Help;
                Ok(Vec::new())
            }
            ExCommand::Unknown(command) => Err(format!("unknown command: :{command}")),
        }
    }

    fn clear_cell_or_selection(&mut self) {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return;
        }
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let range = self.selection.map_or_else(
            || CellRange::single(self.focus),
            |selection| {
                selection.range(
                    sheet.used_range.end.row.saturating_add(1),
                    sheet.used_range.end.col.saturating_add(1),
                )
            },
        );
        let sheet_index = self.active_sheet_index();
        if self.selection.is_some() {
            self.selection = None;
            self.mode = Mode::Normal;
        }
        match self.clear_range(sheet_index, range) {
            Ok(0) => self.status_message = Some("nothing to clear".to_string()),
            Ok(count) => self.status_message = Some(format!("cleared {count} cells")),
            Err(message) => self.status_message = Some(message),
        }
    }

    fn clear_row(&mut self) {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return;
        }
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let row = self.focus.row;
        let range = CellRange::new(
            CellAddr::new(row, 0),
            CellAddr::new(row, sheet.used_range.end.col),
        );
        let sheet_index = self.active_sheet_index();
        match self.clear_range(sheet_index, range) {
            Ok(0) => self.status_message = Some("nothing to clear".to_string()),
            Ok(count) => {
                self.status_message = Some(format!("cleared row {} ({count} cells)", row + 1))
            }
            Err(message) => self.status_message = Some(message),
        }
    }

    fn clear_motion(&mut self, motion: Motion) {
        if matches!(motion, Motion::FirstNonBlank) {
            self.ensure_focus_loaded();
        }
        let Some(target) = self.motion_target(motion) else {
            return;
        };
        let sheet_index = self.active_sheet_index();
        let range = CellRange::new(self.focus, target);
        match self.clear_range(sheet_index, range) {
            Ok(0) => self.status_message = Some("nothing to clear".to_string()),
            Ok(count) => self.status_message = Some(format!("cleared {count} cells")),
            Err(message) => self.status_message = Some(message),
        }
    }

    fn clear_range(&mut self, sheet_index: usize, range: CellRange) -> Result<usize, String> {
        if !self.editing_allowed() {
            return Err("editing is not available".to_string());
        }
        let Some(workbook) = &mut self.workbook else {
            return Err("no workbook".to_string());
        };
        let Some(sheet) = workbook.sheets.get_mut(sheet_index) else {
            return Err("no sheet".to_string());
        };
        let cells = sheet
            .collect_cells(range, true)
            .map_err(|error| error.to_string())?;
        let mut changes = Vec::new();
        for (addr, cell) in cells {
            if sheet.merges.anchor_for(addr) != addr {
                continue;
            }
            if cell.raw.is_empty() && cell.formula.is_none() {
                continue;
            }
            let after =
                Cell::new_with_format(CellValue::Empty, cell.style, cell.number_format.clone());
            sheet.set_edit_cell(addr, Some(after.clone()));
            changes.push(EditChange::new(sheet_index, addr, Some(cell), Some(after)));
        }
        if changes.is_empty() {
            return Ok(0);
        }
        let count = changes.len();
        self.edit_log.record(EditTransaction::from_ops(
            changes
                .into_iter()
                .map(|change| EditOp::Cell(Box::new(change)))
                .collect(),
        ));
        self.after_edit(&[sheet_index]);
        Ok(count)
    }

    fn replace_char(&mut self, ch: char) {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return;
        }
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let range = self.selection.map_or_else(
            || CellRange::single(self.focus),
            |selection| {
                selection.range(
                    sheet.used_range.end.row.saturating_add(1),
                    sheet.used_range.end.col.saturating_add(1),
                )
            },
        );
        let sheet_index = self.active_sheet_index();
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        let Some(sheet) = workbook.sheets.get_mut(sheet_index) else {
            return;
        };
        let mut changes = Vec::new();
        for row in range.rows() {
            let _ = sheet.ensure_rows(RowRange::new(row, row.saturating_add(1)));
            for col in range.cols() {
                let addr = CellAddr::new(row, col);
                if sheet.merges.anchor_for(addr) != addr {
                    continue;
                }
                let before = sheet.raw_cell(addr).cloned();
                let style = before.as_ref().map(|cell| cell.style).unwrap_or_default();
                let number_format = before.as_ref().and_then(|cell| cell.number_format.clone());
                let after = edit::parse_cell_input(&ch.to_string(), style, number_format);
                sheet.set_edit_cell(addr, Some(after.clone()));
                changes.push(EditChange::new(sheet_index, addr, before, Some(after)));
            }
        }
        if changes.is_empty() {
            return;
        }
        let count = changes.len();
        self.edit_log.record(EditTransaction::from_ops(
            changes
                .into_iter()
                .map(|change| EditOp::Cell(Box::new(change)))
                .collect(),
        ));
        self.selection = None;
        self.mode = Mode::Normal;
        self.after_edit(&[sheet_index]);
        self.status_message = Some(format!("replaced {count} cells"));
    }

    fn search_word(&mut self, forward: bool) {
        self.ensure_focus_loaded();
        let origin_sheet = self.active_sheet_index();
        let origin = self.focus;
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let text = match sheet.read_cell(self.focus) {
            CellRead::Loaded(cell) => cell.display.trim().to_string(),
            _ => String::new(),
        };
        if text.is_empty() {
            self.status_message = Some("current cell is empty".to_string());
            return;
        }
        self.search_query = text.clone();
        let query = SearchQuery::new(text);
        self.last_search = Some(query.clone());
        let Some(mut index) = self.build_search_index(query) else {
            return;
        };
        let visibilities: Vec<_> = self
            .workbook
            .as_ref()
            .map(|workbook| {
                workbook
                    .sheets
                    .iter()
                    .map(|sheet| sheet.visibility.clone())
                    .collect()
            })
            .unwrap_or_default();
        let total = index.matches.len();
        let matched = index.select_visible_from(origin_sheet, origin, forward, &visibilities);
        self.search_index = Some(index);
        if let Some(matched) = matched {
            self.jump_to(matched.sheet, matched.addr);
        } else if total > 0 {
            self.status_message = Some("no visible match".to_string());
        } else {
            self.status_message = Some("no match".to_string());
        }
    }

    fn goto_command(
        &mut self,
        sheet: Option<String>,
        addr: Option<CellAddr>,
    ) -> Result<(), String> {
        let Some(workbook) = &self.workbook else {
            return Err("no workbook".to_string());
        };
        let sheet_index = match sheet {
            Some(name) => workbook
                .sheets
                .iter()
                .position(|sheet| sheet.name == name)
                .ok_or_else(|| format!("unknown sheet: {name}"))?,
            None => workbook.active_sheet,
        };
        let addr = addr.unwrap_or_default();
        self.jump_to(sheet_index, addr);
        Ok(())
    }

    fn jump_to(&mut self, sheet: usize, addr: CellAddr) {
        if sheet == self.active_sheet_index() && addr == self.focus {
            return;
        }
        self.jump_list.push(JumpEntry {
            sheet: self.active_sheet_index(),
            addr: self.focus,
        });
        self.jump_to_without_history(sheet, addr);
        self.jump_list.push(JumpEntry {
            sheet: self.active_sheet_index(),
            addr: self.focus,
        });
    }

    fn jump_to_without_history(&mut self, sheet: usize, addr: CellAddr) {
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        if sheet >= workbook.sheets.len() {
            return;
        }
        let changed_sheet = workbook.active_sheet != sheet;
        workbook.active_sheet = sheet;
        let anchor = workbook
            .sheets
            .get(sheet)
            .map(|sheet| sheet.merges.anchor_for(addr))
            .unwrap_or(addr);
        self.focus = anchor;
        self.selection = None;
        self.mode = Mode::Normal;
        if changed_sheet {
            self.viewport.top_visible_row = 0;
            self.viewport.left_col = 0;
        }
        if let Some(index) = &mut self.search_index {
            index.set_active_sheet(sheet);
        }
        self.ensure_focus_loaded();
        self.rebuild_visibility();
        self.ensure_focus_visible();
    }

    fn activate_sheet(&mut self, index: usize) {
        self.jump_to(index, CellAddr::new(0, 0));
    }

    fn apply_substitute(&mut self, command: &SubstituteCommand) -> Result<(), String> {
        if !self.editing_allowed() {
            return Err("editing is not available".to_string());
        }
        let pattern = if command.pattern.is_empty() {
            self.last_search
                .as_ref()
                .map(|query| query.text.clone())
                .ok_or_else(|| "no previous search pattern".to_string())?
        } else {
            command.pattern.clone()
        };
        let case_sensitive = command
            .case_sensitive
            .unwrap_or_else(|| pattern.chars().any(|ch| ch.is_uppercase()));
        let Some(workbook) = &mut self.workbook else {
            return Err("no workbook".to_string());
        };
        let active_sheet = workbook.active_sheet;
        let mut targets: Vec<(usize, CellRange)> = Vec::new();
        match &command.scope {
            SubstituteScope::CurrentCell => {
                targets.push((active_sheet, CellRange::single(self.focus)));
            }
            SubstituteScope::Selection => {
                let selection = self
                    .selection
                    .ok_or_else(|| "no visual selection".to_string())?;
                let sheet = workbook
                    .sheets
                    .get(active_sheet)
                    .ok_or_else(|| "no sheet".to_string())?;
                targets.push((
                    active_sheet,
                    selection.range(
                        sheet.used_range.end.row.saturating_add(1),
                        sheet.used_range.end.col.saturating_add(1),
                    ),
                ));
            }
            SubstituteScope::Range(range) => {
                targets.push((active_sheet, *range));
            }
            SubstituteScope::Sheet => {
                let sheet = workbook
                    .sheets
                    .get(active_sheet)
                    .ok_or_else(|| "no sheet".to_string())?;
                targets.push((
                    active_sheet,
                    CellRange::new(CellAddr::new(0, 0), sheet.used_range.end),
                ));
            }
            SubstituteScope::Workbook => {
                for (index, sheet) in workbook.sheets.iter().enumerate() {
                    targets.push((
                        index,
                        CellRange::new(CellAddr::new(0, 0), sheet.used_range.end),
                    ));
                }
            }
        }

        let mut changes = Vec::new();
        let mut cells_changed = 0usize;
        let mut occurrences = 0usize;
        let mut skipped_formulas = 0usize;
        for (sheet_index, range) in targets {
            let Some(sheet) = workbook.sheets.get_mut(sheet_index) else {
                continue;
            };
            let cells = sheet
                .collect_cells(range, true)
                .map_err(|error| error.to_string())?;
            for (addr, cell) in cells {
                if sheet.merges.anchor_for(addr) != addr {
                    continue;
                }
                if cell.formula.is_some() {
                    if edit::replace_literal(
                        &cell.display,
                        &pattern,
                        &command.replacement,
                        command.global,
                        case_sensitive,
                    )
                    .1 > 0
                    {
                        skipped_formulas += 1;
                    }
                    continue;
                }
                let (new_text, count) = edit::replace_literal(
                    &cell.display,
                    &pattern,
                    &command.replacement,
                    command.global,
                    case_sensitive,
                );
                if count == 0 {
                    continue;
                }
                occurrences += count;
                if command.count_only {
                    cells_changed += 1;
                    continue;
                }
                let after = edit::cell_from_replacement(&cell, &new_text);
                sheet.set_edit_cell(addr, Some(after.clone()));
                changes.push(EditChange::new(sheet_index, addr, Some(cell), Some(after)));
                cells_changed += 1;
            }
        }

        if command.count_only {
            self.status_message = Some(format!(
                "would replace {occurrences} occurrences in {cells_changed} cells; formulas skipped: {skipped_formulas}"
            ));
            return Ok(());
        }
        if changes.is_empty() {
            self.status_message = Some(format!("no matches; formulas skipped: {skipped_formulas}"));
            return Ok(());
        }
        let sheets: Vec<usize> = {
            let mut sheets: Vec<usize> = changes.iter().map(|change| change.sheet).collect();
            sheets.sort_unstable();
            sheets.dedup();
            sheets
        };
        self.edit_log.record(EditTransaction::from_ops(
            changes
                .into_iter()
                .map(|change| EditOp::Cell(Box::new(change)))
                .collect(),
        ));
        self.selection = None;
        self.mode = Mode::Normal;
        self.after_edit(&sheets);
        self.status_message = Some(format!(
            "replaced {occurrences} occurrences in {cells_changed} cells; formulas skipped: {skipped_formulas}"
        ));
        Ok(())
    }

    pub fn apply_formula_results(&mut self, results: FormulaResults) {
        if !self.is_current_generation(results.generation) {
            return;
        }
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        let summary = workbook.apply_formula_results(results);
        if summary.changed {
            self.rebuild_visibility();
        }
        if self.search_index.is_some() {
            self.status_message =
                Some("formula values updated; search results cleared".to_string());
            self.search_index = None;
        }
    }

    pub fn active_sheet(&self) -> Option<&Sheet> {
        self.workbook
            .as_ref()
            .and_then(|workbook| workbook.active())
    }

    pub fn active_sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.workbook
            .as_mut()
            .and_then(|workbook| workbook.active_mut())
    }

    pub fn active_sheet_index(&self) -> usize {
        self.workbook
            .as_ref()
            .map(|workbook| workbook.active_sheet)
            .unwrap_or(0)
    }

    pub fn column_widths(&self, sheet: &Sheet) -> ColumnWidths {
        if self.fit_width {
            ColumnWidths::fit(sheet, self.viewport.width)
        } else {
            ColumnWidths::from_sheet(sheet)
        }
    }

    fn switch_sheet(&mut self, delta: i32) {
        let Some(workbook) = &mut self.workbook else {
            return;
        };
        if workbook.sheets.is_empty() {
            return;
        }
        let count = workbook.sheets.len() as i32;
        let next = (workbook.active_sheet as i32 + delta).rem_euclid(count);
        workbook.active_sheet = next as usize;
        self.focus = CellAddr::new(0, 0);
        self.selection = None;
        if let Some(index) = &mut self.search_index {
            index.set_active_sheet(next as usize);
        }
        self.viewport.top_visible_row = 0;
        self.viewport.left_col = 0;
        self.rebuild_visibility();
        self.ensure_focus_visible();
    }

    fn move_focus(&mut self, motion: Motion, extend: bool) {
        if matches!(motion, Motion::FirstNonBlank) {
            self.ensure_focus_loaded();
        }
        let Some(target) = self.motion_target(motion) else {
            return;
        };
        self.focus = target;
        if extend {
            if let Some(selection) = &mut self.selection {
                selection.extend(target);
            } else {
                self.selection = Some(Selection::new(target));
                self.mode = Mode::Visual;
            }
        }
        self.ensure_focus_visible();
    }

    /// Computes the destination of a motion without mutating navigation state.
    /// Used both by focus movement and by delete/change operators.
    fn motion_target(&self, motion: Motion) -> Option<CellAddr> {
        let sheet = self.active_sheet()?;
        let current = self.focus;
        let mut row = current.row;
        let mut col = current.col;
        match motion {
            Motion::Up => row = row.saturating_sub(1),
            Motion::Down => row = row.saturating_add(1),
            Motion::Left => col = col.saturating_sub(1),
            Motion::Right => col = col.saturating_add(1),
            Motion::PageUp => row = row.saturating_sub(self.viewport.height as u32),
            Motion::PageDown => row = row.saturating_add(self.viewport.height as u32),
            Motion::HalfUp => row = row.saturating_sub((self.viewport.height / 2) as u32),
            Motion::HalfDown => row = row.saturating_add((self.viewport.height / 2) as u32),
            Motion::Top => row = 0,
            Motion::Bottom => row = sheet.used_range.end.row,
            Motion::DataTop => row = sheet.data_start_row(),
            Motion::DataBottom => row = sheet.data_end_row(),
            Motion::LineStart => col = 0,
            Motion::LineEnd => col = sheet.used_range.end.col,
            Motion::FirstNonBlank => col = sheet.first_non_blank_col(row),
        }
        row = row.min(sheet.used_range.end.row);
        col = col.min(sheet.used_range.end.col);
        let absolute_jump = matches!(
            motion,
            Motion::DataTop | Motion::DataBottom | Motion::FirstNonBlank
        );
        if !absolute_jump && let Some(region) = sheet.merges.region_at(current) {
            match motion {
                Motion::Right | Motion::LineEnd => col = region.range.end.col.saturating_add(1),
                Motion::Left | Motion::LineStart => col = region.range.start.col.saturating_sub(1),
                Motion::Down | Motion::Bottom | Motion::PageDown | Motion::HalfDown => {
                    row = region.range.end.row.saturating_add(1)
                }
                Motion::Up | Motion::Top | Motion::PageUp | Motion::HalfUp => {
                    row = region.range.start.row.saturating_sub(1)
                }
                Motion::DataTop | Motion::DataBottom | Motion::FirstNonBlank => {}
            }
            row = row.min(sheet.used_range.end.row);
            col = col.min(sheet.used_range.end.col);
        }
        let target = CellAddr::new(row, col);
        let anchor = sheet.merges.anchor_for(target);
        let visible = sheet.visibility.visible_rows();
        let target = if visible.is_empty() || visible.contains(&anchor.row) {
            anchor
        } else {
            let next = if anchor.row < current.row {
                visible
                    .iter()
                    .rev()
                    .copied()
                    .find(|row| *row <= anchor.row)
                    .or_else(|| visible.first().copied())
            } else {
                visible
                    .iter()
                    .copied()
                    .find(|row| *row >= anchor.row)
                    .or_else(|| visible.last().copied())
            }
            .unwrap_or(anchor.row);
            CellAddr::new(next, anchor.col)
        };
        Some(target)
    }

    fn ensure_focus_visible(&mut self) {
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let widths = self.column_widths(sheet);
        let freeze = LayoutEngine::effective_freeze(sheet, &self.viewport, &widths);
        let plan =
            ViewportPlanner::ensure_visible(sheet, &self.viewport, self.focus, freeze, &widths);
        self.viewport.top_visible_row = plan.top_visible_row;
        self.viewport.left_col = plan.left_col;
    }

    fn ensure_focus_loaded(&mut self) {
        let index = self.active_sheet_index();
        if let Some(workbook) = &mut self.workbook {
            let _ = workbook.ensure_rows(index, RowRange::new(self.focus.row, self.focus.row + 1));
        }
    }

    fn selection_range(&self) -> Option<CellRange> {
        let sheet = self.active_sheet()?;
        let rows = sheet
            .meta
            .row_count
            .max(sheet.used_range.end.row.saturating_add(1));
        let cols = sheet.used_range.end.col.saturating_add(1);
        self.selection.map(|selection| selection.range(rows, cols))
    }

    fn build_clipboard_block(
        &mut self,
        range: CellRange,
        kind: RegisterKind,
    ) -> Result<ClipboardBlock, String> {
        let index = self.active_sheet_index();
        if let Some(workbook) = &mut self.workbook {
            let _ = workbook.ensure_rows(
                index,
                RowRange::new(range.start.row, range.end.row.saturating_add(1)),
            );
        }
        let limit = self.config.cli.max_copy_cells;
        let cells = (range.row_count() as usize).saturating_mul(range.col_count() as usize);
        if cells > limit {
            return Err(format!(
                "selection is too large: {cells} cells (limit {limit})"
            ));
        }
        let Some(sheet) = self.active_sheet() else {
            return Err("no sheet".to_string());
        };
        let complete = kind == RegisterKind::Cells;
        let mut block = ClipboardBlock::new(kind, range.col_count(), range.row_count(), complete);
        for row in range.rows() {
            for col in range.cols() {
                let addr = CellAddr::new(row, col);
                let cell = if sheet.merges.is_covered(addr) {
                    None
                } else {
                    sheet.cell(addr).cloned()
                };
                if complete {
                    block.insert(
                        row.saturating_sub(range.start.row),
                        col.saturating_sub(range.start.col),
                        cell.unwrap_or_else(empty_cell),
                    );
                } else if let Some(cell) = cell {
                    block.insert(
                        row.saturating_sub(range.start.row),
                        col.saturating_sub(range.start.col),
                        cell,
                    );
                }
            }
        }
        Ok(block)
    }

    fn clipboard_effect(&self, block: ClipboardBlock) -> Vec<Effect> {
        match TsvCodec::encode(&block) {
            Ok(text) => vec![Effect::WriteClipboard {
                text,
                origin: ClipboardWriteOrigin::Register { kind: block.kind },
            }],
            Err(error) => vec![Effect::SetStatus(error.to_string())],
        }
    }

    fn yank_selection(&mut self) -> Vec<Effect> {
        let kind = match self.selection.map(|selection| selection.mode) {
            Some(SelectionMode::Row) => RegisterKind::Rows,
            Some(SelectionMode::Column) => RegisterKind::Columns,
            _ => RegisterKind::Cells,
        };
        let range = self
            .selection_range()
            .unwrap_or_else(|| CellRange::single(self.focus));
        match self.build_clipboard_block(range, kind) {
            Ok(block) => {
                self.registers.set_yank(block.clone());
                self.selection = None;
                self.mode = Mode::Normal;
                self.clipboard_effect(block)
            }
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn yank_row(&mut self) -> Vec<Effect> {
        let Some(sheet) = self.active_sheet() else {
            return Vec::new();
        };
        let range = CellRange::new(
            CellAddr::new(self.focus.row, 0),
            CellAddr::new(self.focus.row, sheet.used_range.end.col),
        );
        match self.build_clipboard_block(range, RegisterKind::Rows) {
            Ok(block) => {
                self.registers.set_yank(block.clone());
                self.clipboard_effect(block)
            }
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn cut_current(&mut self) -> Vec<Effect> {
        if self.selection.is_some() {
            return self.cut_selection();
        }
        let range = CellRange::single(self.focus);
        self.cut_range(range, RegisterKind::Cells, None)
    }

    fn cut_selection(&mut self) -> Vec<Effect> {
        let Some(selection) = self.selection else {
            return self.cut_current();
        };
        let kind = match selection.mode {
            SelectionMode::Row => RegisterKind::Rows,
            SelectionMode::Column => RegisterKind::Columns,
            _ => RegisterKind::Cells,
        };
        let range = self
            .selection_range()
            .unwrap_or_else(|| CellRange::single(self.focus));
        let structure = match selection.mode {
            SelectionMode::Row => Some(StructureOp::delete(
                Axis::Row,
                range.start.row,
                range.row_count(),
            )),
            SelectionMode::Column => Some(StructureOp::delete(
                Axis::Column,
                range.start.col,
                range.col_count(),
            )),
            _ => None,
        };
        self.cut_range(range, kind, structure)
    }

    fn cut_motion(&mut self, motion: Motion) -> Vec<Effect> {
        let Some(target) = self.motion_target(motion) else {
            return Vec::new();
        };
        let range = CellRange::new(self.focus, target);
        self.cut_range(range, RegisterKind::Cells, None)
    }

    fn cut_rows(&mut self, count: u32) -> Vec<Effect> {
        self.cut_rows_at(self.focus.row, count)
    }

    fn cut_rows_at(&mut self, index: u32, count: u32) -> Vec<Effect> {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return Vec::new();
        }
        if count == 0 || count > 1000 {
            self.status_message = Some("count must be between 1 and 1000".to_string());
            return Vec::new();
        }
        let Some(sheet) = self.active_sheet() else {
            return Vec::new();
        };
        if index.saturating_add(count) > sheet.meta.row_count {
            self.status_message = Some("row range is outside the sheet".to_string());
            return Vec::new();
        }
        let last_col = sheet
            .used_range
            .end
            .col
            .min(sheet.meta.column_count.saturating_sub(1));
        let range = CellRange::new(
            CellAddr::new(index, 0),
            CellAddr::new(index.saturating_add(count).saturating_sub(1), last_col),
        );
        self.cut_range(
            range,
            RegisterKind::Rows,
            Some(StructureOp::delete(Axis::Row, index, count)),
        )
    }

    fn cut_columns_at(&mut self, index: u32, count: u32) -> Vec<Effect> {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return Vec::new();
        }
        if count == 0 || count > 1000 {
            self.status_message = Some("count must be between 1 and 1000".to_string());
            return Vec::new();
        }
        let Some(sheet) = self.active_sheet() else {
            return Vec::new();
        };
        if index.saturating_add(count) > sheet.meta.column_count {
            self.status_message = Some("column range is outside the sheet".to_string());
            return Vec::new();
        }
        let last_row = sheet
            .used_range
            .end
            .row
            .min(sheet.meta.row_count.saturating_sub(1));
        let range = CellRange::new(
            CellAddr::new(0, index),
            CellAddr::new(last_row, index.saturating_add(count).saturating_sub(1)),
        );
        self.cut_range(
            range,
            RegisterKind::Columns,
            Some(StructureOp::delete(Axis::Column, index, count)),
        )
    }

    fn cut_range(
        &mut self,
        range: CellRange,
        kind: RegisterKind,
        structure: Option<StructureOp>,
    ) -> Vec<Effect> {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return Vec::new();
        }
        let block = match self.build_clipboard_block(range, kind) {
            Ok(block) => block,
            Err(message) => {
                self.status_message = Some(message);
                return Vec::new();
            }
        };
        self.registers.set_cut(block.clone());
        let sheet_index = self.active_sheet_index();
        let result = match structure {
            Some(op) => self.apply_structure_op(op).map(|_| ()),
            None => self.clear_range(sheet_index, range).map(|_| ()),
        };
        self.selection = None;
        self.mode = Mode::Normal;
        match result {
            Ok(()) => self.clipboard_effect(block),
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn insert_rows(&mut self, before: bool, enter_insert: bool, count: u32) -> Vec<Effect> {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return Vec::new();
        }
        let index = if before {
            self.focus.row
        } else {
            self.focus.row.saturating_add(1)
        };
        match self.apply_structure_op(StructureOp::insert(Axis::Row, index, count)) {
            Ok(_) => {
                self.focus.row = index.min(self.active_meta_rows().saturating_sub(1));
                self.selection = None;
                self.mode = Mode::Normal;
                self.status_message = Some(format!("inserted {count} row(s)"));
                if enter_insert {
                    self.start_edit(EditCursor::Start, None);
                }
                Vec::new()
            }
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn insert_columns(&mut self, before: bool, enter_insert: bool, count: u32) -> Vec<Effect> {
        if !self.editing_allowed() {
            self.status_message = Some("editing is not available".to_string());
            return Vec::new();
        }
        let index = if before {
            self.focus.col
        } else {
            self.focus.col.saturating_add(1)
        };
        match self.apply_structure_op(StructureOp::insert(Axis::Column, index, count)) {
            Ok(_) => {
                self.focus.col = index.min(self.active_meta_cols().saturating_sub(1));
                self.selection = None;
                self.mode = Mode::Normal;
                self.status_message = Some(format!("inserted {count} column(s)"));
                if enter_insert {
                    self.start_edit(EditCursor::Start, None);
                }
                Vec::new()
            }
            Err(message) => {
                self.status_message = Some(message);
                Vec::new()
            }
        }
    }

    fn delete_rows(&mut self, before: bool, count: u32) -> Vec<Effect> {
        if before {
            if self.focus.row < count {
                self.status_message = Some("row range is outside the sheet".to_string());
                return Vec::new();
            }
            self.cut_rows_at(self.focus.row - count, count)
        } else {
            self.cut_rows_at(self.focus.row, count)
        }
    }

    fn delete_columns(&mut self, before: bool, count: u32) -> Vec<Effect> {
        if before {
            if self.focus.col < count {
                self.status_message = Some("column range is outside the sheet".to_string());
                return Vec::new();
            }
            self.cut_columns_at(self.focus.col - count, count)
        } else {
            self.cut_columns_at(self.focus.col, count)
        }
    }

    fn apply_structure_op(&mut self, op: StructureOp) -> Result<WorkbookStructureOutcome, String> {
        if !self.editing_allowed() {
            return Err("editing is not available".to_string());
        }
        let sheet_index = self.active_sheet_index();
        let max_cells = self.config.cli.structural_max_cells;
        let outcome = {
            let Some(workbook) = &mut self.workbook else {
                return Err("no workbook".to_string());
            };
            StructureFlow::apply(workbook, sheet_index, op, max_cells)
                .map_err(|error| error.to_string())?
        };
        let edit = StructureEdit {
            sheet: sheet_index,
            op,
            removed: outcome.structure.removed.clone(),
            before: outcome.structure.before.clone(),
            after: outcome.structure.after.clone(),
            formula_before: outcome.formula_before.clone(),
            formula_after: outcome.formula_after.clone(),
        };
        self.edit_log
            .record(EditTransaction::from_ops(vec![EditOp::Structure(
                Box::new(edit),
            )]));
        self.after_structure_edit(&outcome);
        Ok(outcome)
    }

    fn after_structure_edit(&mut self, outcome: &WorkbookStructureOutcome) {
        self.after_structure_edit_parts(&outcome.affected_sheets, &outcome.warnings);
    }

    fn after_structure_edit_parts(
        &mut self,
        affected_sheets: &[usize],
        warnings: &[StructureWarning],
    ) {
        if let Some(workbook) = &mut self.workbook {
            workbook.generation = Generation(workbook.generation.0.saturating_add(1));
        }
        self.search_index = None;
        self.jump_list.clear();
        self.pending_recalc = self.has_formulas();
        let rows = self.active_meta_rows();
        let cols = self.active_meta_cols();
        self.focus.row = self.focus.row.min(rows.saturating_sub(1));
        self.focus.col = self.focus.col.min(cols.saturating_sub(1));
        if let Some(selection) = &mut self.selection {
            selection.anchor.row = selection.anchor.row.min(rows.saturating_sub(1));
            selection.anchor.col = selection.anchor.col.min(cols.saturating_sub(1));
            selection.cursor.row = selection.cursor.row.min(rows.saturating_sub(1));
            selection.cursor.col = selection.cursor.col.min(cols.saturating_sub(1));
        }
        for &index in affected_sheets {
            if let Some(sheet) = self
                .workbook
                .as_mut()
                .and_then(|workbook| workbook.sheets.get_mut(index))
            {
                let _ = sheet.rebuild_visibility();
            }
        }
        self.rebuild_visibility();
        self.ensure_focus_loaded();
        self.ensure_focus_visible();
        if !warnings.is_empty() {
            self.status_message = Some(format!(
                "structure changed with {} warning(s)",
                warnings.len()
            ));
        }
    }

    fn active_meta_rows(&self) -> u32 {
        self.active_sheet()
            .map(|sheet| sheet.meta.row_count.max(1))
            .unwrap_or(1)
    }

    fn active_meta_cols(&self) -> u32 {
        self.active_sheet()
            .map(|sheet| sheet.meta.column_count.max(1))
            .unwrap_or(1)
    }

    fn paste(&mut self, before: bool, source: PasteSource) -> Vec<Effect> {
        match source {
            PasteSource::System => vec![Effect::ReadClipboard {
                before,
                fallback: None,
            }],
            PasteSource::Yank => match self.registers.get(PasteSource::Yank).cloned() {
                Some(block) => {
                    self.paste_internal_block(block, before);
                    Vec::new()
                }
                None => {
                    self.status_message = Some("nothing to paste".to_string());
                    Vec::new()
                }
            },
            PasteSource::Unnamed => {
                if matches!(
                    self.config.cli.clipboard_register,
                    crate::config::ClipboardRegisterMode::UnnamedPlus
                ) {
                    vec![Effect::ReadClipboard {
                        before,
                        fallback: Some(PasteSource::Unnamed),
                    }]
                } else {
                    match self.registers.get(PasteSource::Unnamed).cloned() {
                        Some(block) => {
                            self.paste_internal_block(block, before);
                            Vec::new()
                        }
                        None => {
                            self.status_message = Some("nothing to paste".to_string());
                            Vec::new()
                        }
                    }
                }
            }
        }
    }

    pub fn paste_clipboard_text(&mut self, text: &str, before: bool) {
        if self.registers.last_text_matches(text)
            && let Some(block) = self.registers.get(PasteSource::Unnamed).cloned()
        {
            self.paste_internal_block(block, before);
            return;
        }
        let limits = RegisterLimits {
            max_cells: self.config.cli.max_copy_cells,
        };
        match TsvCodec::decode(text, &limits) {
            Ok(block) => self.paste_internal_block(block, before),
            Err(error) => self.status_message = Some(format!("paste failed: {error}")),
        }
    }

    pub fn paste_internal_block(&mut self, block: ClipboardBlock, before: bool) {
        if block.is_empty() {
            self.status_message = Some("nothing to paste".to_string());
            return;
        }
        let cell_count = block.cell_count();
        if cell_count > self.config.cli.max_copy_cells {
            self.status_message = Some(format!(
                "selection is too large: {cell_count} cells (limit {})",
                self.config.cli.max_copy_cells
            ));
            return;
        }
        let sheet_index = self.active_sheet_index();
        let plan = {
            let Some(sheet) = self.active_sheet() else {
                return;
            };
            match PastePlanner::plan(sheet, sheet_index, self.focus, before, &block) {
                Ok(plan) => plan,
                Err(error) => {
                    self.status_message = Some(error.to_string());
                    return;
                }
            }
        };
        let mut edits = Vec::new();
        let mut affected = vec![sheet_index];
        let mut warnings = plan.warnings.clone();
        for op in &plan.inserts {
            let outcome = {
                let Some(workbook) = &mut self.workbook else {
                    return;
                };
                match StructureFlow::apply(
                    workbook,
                    sheet_index,
                    *op,
                    self.config.cli.structural_max_cells,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        self.status_message = Some(error.to_string());
                        return;
                    }
                }
            };
            edits.push(EditOp::Structure(Box::new(StructureEdit {
                sheet: sheet_index,
                op: *op,
                removed: outcome.structure.removed.clone(),
                before: outcome.structure.before.clone(),
                after: outcome.structure.after.clone(),
                formula_before: outcome.formula_before.clone(),
                formula_after: outcome.formula_after.clone(),
            })));
            for index in outcome.affected_sheets {
                if !affected.contains(&index) {
                    affected.push(index);
                }
            }
            warnings.extend(outcome.warnings.iter().cloned());
        }
        let mut cell_changes = Vec::new();
        if let Some(sheet) = self
            .workbook
            .as_mut()
            .and_then(|workbook| workbook.sheets.get_mut(sheet_index))
        {
            for write in plan.writes {
                if sheet.merges.is_covered(write.addr) {
                    warnings.push(StructureWarning::SkippedCoveredCell { addr: write.addr });
                    continue;
                }
                if write.addr.row >= crate::domain::structure::EXCEL_MAX_ROWS
                    || write.addr.col >= crate::domain::structure::EXCEL_MAX_COLS
                {
                    continue;
                }
                let before_cell = sheet.raw_cell(write.addr).cloned();
                let after = if write.cell.raw.is_empty() && write.cell.formula.is_none() {
                    match &before_cell {
                        Some(before) => {
                            edit::parse_cell_input("", before.style, before.number_format.clone())
                        }
                        None => write.cell.clone(),
                    }
                } else {
                    write.cell.clone()
                };
                sheet.set_edit_cell(write.addr, Some(after.clone()));
                cell_changes.push(EditChange::new(
                    sheet_index,
                    write.addr,
                    before_cell,
                    Some(after),
                ));
            }
        }
        edits.extend(
            cell_changes
                .into_iter()
                .map(|change| EditOp::Cell(Box::new(change))),
        );
        if edits.is_empty() {
            self.status_message = Some("nothing to paste".to_string());
            return;
        }
        self.edit_log.record(EditTransaction::from_ops(edits));
        self.after_structure_edit_parts(&affected, &warnings);
        self.focus = plan.focus;
        self.focus.row = self
            .focus
            .row
            .min(self.active_meta_rows().saturating_sub(1));
        self.focus.col = self
            .focus
            .col
            .min(self.active_meta_cols().saturating_sub(1));
        self.selection = None;
        self.mode = Mode::Normal;
        self.status_message = Some(format!("pasted {cell_count} cell(s)"));
    }

    pub fn note_clipboard_write(&mut self, text: String, success: bool) {
        if success {
            self.registers.note_write_success(text);
        }
    }

    fn submit_search(&mut self) {
        let text = self.search_query.trim().to_string();
        if text.is_empty() {
            self.search_index = None;
            self.last_search = None;
            return;
        }
        let query = SearchQuery::new(text);
        self.last_search = Some(query.clone());
        self.perform_search(query);
    }

    fn perform_search(&mut self, query: SearchQuery) {
        let Some(index) = self.build_search_index(query) else {
            return;
        };
        let total = index.matches.len();
        let current = index.current_match().cloned();
        self.search_index = Some(index);
        if let Some(matched) = current {
            self.jump_to(matched.sheet, matched.addr);
        } else if total > 0 {
            self.status_message = Some("no visible match".to_string());
        } else {
            self.status_message = Some("no match".to_string());
        }
    }

    fn build_search_index(&mut self, query: SearchQuery) -> Option<SearchIndex> {
        let workbook = self.workbook.as_mut()?;
        let mut matches = Vec::new();
        for (sheet_index, sheet) in workbook.sheets.iter_mut().enumerate() {
            let range = CellRange::new(CellAddr::new(0, 0), sheet.used_range.end);
            let Ok(cells) = sheet.collect_cells(range, false) else {
                continue;
            };
            for (addr, cell) in cells {
                if sheet.merges.anchor_for(addr) != addr {
                    continue;
                }
                let effective = sheet.effective_from_cell(addr, &cell);
                if query.matches(&effective.display) {
                    matches.push(SearchMatch {
                        sheet: sheet_index,
                        addr,
                        source: MatchSource::Display,
                    });
                } else if let Some(formula) = &effective.formula
                    && query.matches(&formula.expression)
                {
                    matches.push(SearchMatch {
                        sheet: sheet_index,
                        addr,
                        source: MatchSource::Formula,
                    });
                }
            }
        }
        matches.sort_by_key(|matched| (matched.sheet, matched.addr.row, matched.addr.col));
        let visibilities: Vec<_> = workbook
            .sheets
            .iter()
            .map(|sheet| sheet.visibility.clone())
            .collect();
        let active_sheet = workbook.active_sheet;
        let mut index = SearchIndex::new(query, matches);
        index.set_active_sheet(active_sheet);
        index.sync_visibility_for(&visibilities);
        Some(index)
    }

    fn next_match(&mut self, forward: bool) {
        let Some(workbook) = &self.workbook else {
            return;
        };
        let visibilities: Vec<_> = workbook
            .sheets
            .iter()
            .map(|sheet| sheet.visibility.clone())
            .collect();
        let matched = {
            let Some(index) = &mut self.search_index else {
                return;
            };
            if forward {
                index.next_visible_in(&visibilities)
            } else {
                index.previous_visible_in(&visibilities)
            }
        };
        if let Some(matched) = matched {
            self.jump_to(matched.sheet, matched.addr);
        }
    }

    fn toggle_filter(&mut self) {
        let selected = self.selected_filter;
        if let Some(sheet) = self.active_sheet_mut() {
            if sheet.filters.is_empty() {
                return;
            }
            if selected == 0 {
                sheet.filter_state.enabled = !sheet.filter_state.enabled;
            } else {
                let column = selected - 1;
                if let Some(enabled) = sheet.filter_state.column_enabled.get_mut(column) {
                    *enabled = !*enabled;
                    sheet.filter_state.enabled = true;
                }
            }
        }
        self.rebuild_visibility();
    }

    fn filter_line_count(&self) -> usize {
        self.active_sheet()
            .map(|sheet| {
                1 + sheet
                    .filters
                    .iter()
                    .map(|filter| filter.columns.len())
                    .sum::<usize>()
            })
            .unwrap_or(1)
    }

    fn toggle_all_filters(&mut self) {
        if let Some(sheet) = self.active_sheet_mut() {
            let value = !sheet.filter_state.enabled;
            sheet.filter_state.enabled = value;
            sheet.filter_state.column_enabled.fill(value);
        }
        self.rebuild_visibility();
    }

    fn rebuild_visibility(&mut self) {
        let mut error_message = None;
        let mut next_focus_row = None;
        if let Some(workbook) = &mut self.workbook {
            let index = workbook.active_sheet;
            if let Some(sheet) = workbook.sheets.get_mut(index) {
                if let Err(error) = sheet.rebuild_visibility() {
                    error_message = Some(format!("filter visibility failed: {error}"));
                }
                if !sheet.visibility.is_visible(self.focus.row) {
                    next_focus_row = sheet
                        .visibility
                        .visible_rows()
                        .iter()
                        .copied()
                        .find(|row| *row >= self.focus.row)
                        .or_else(|| sheet.visibility.visible_rows().last().copied());
                }
            }
        }
        if let Some(row) = next_focus_row {
            self.focus = CellAddr::new(row, self.focus.col);
        }
        if self.search_index.is_some() {
            let active = self.active_sheet_index();
            let visibilities: Vec<_> = self
                .workbook
                .as_ref()
                .map(|workbook| {
                    workbook
                        .sheets
                        .iter()
                        .map(|sheet| sheet.visibility.clone())
                        .collect()
                })
                .unwrap_or_default();
            if let Some(index) = &mut self.search_index {
                index.set_active_sheet(active);
                index.sync_visibility_for(&visibilities);
            }
        }
        if let Some(message) = error_message {
            self.status_message = Some(message);
        }
    }

    fn detail_view(&self) -> Option<DetailView> {
        let sheet = self.active_sheet()?;
        let cell = match sheet.read_cell(self.focus) {
            CellRead::Loaded(cell) => cell,
            _ => return None,
        };
        let raw = format!("{:?}", cell.value);
        let formula = cell
            .formula
            .as_ref()
            .map(|formula| formula.expression.clone());
        let number_format = sheet
            .cell(self.focus)
            .and_then(|cell| cell.number_format.clone());
        let cached = cell
            .formula
            .as_ref()
            .and_then(|formula| formula.cached.as_ref())
            .map(|value| value.display_text_with_format(number_format.as_deref()));
        let evaluated = cell
            .formula
            .as_ref()
            .and_then(|formula| formula.evaluated.as_ref())
            .map(|value| value.display_text_with_format(number_format.as_deref()));
        let merge = sheet
            .merges
            .region_at(self.focus)
            .map(|region| format!("{:?}", region.range));
        Some(DetailView {
            address: self.focus.a1(),
            kind: format!("{:?}", cell.value),
            raw,
            display: cell.display.clone(),
            formula,
            cached,
            evaluated,
            status: format!("{:?}", cell.formula_status),
            style: format!(
                "bold={} color={:?} fill={:?} wrap={} numfmt={:?} borders=t:{:?} r:{:?} b:{:?} l:{:?}",
                cell.style.bold,
                cell.style.color,
                cell.style.fill,
                cell.style.wrap_text,
                number_format,
                cell.style.borders.top.style,
                cell.style.borders.right.style,
                cell.style.borders.bottom.style,
                cell.style.borders.left.style
            ),
            merge,
            truncated: LayoutEngine::truncated_at(sheet, self.focus, &self.column_widths(sheet)),
            lines: cell.display.lines().map(str::to_string).collect(),
            scroll: self.detail_scroll,
        })
    }

    fn help_view(&self) -> HelpView {
        HelpView {
            bindings: vec![
                ("h j k l".to_string(), "move".to_string()),
                ("gg / G".to_string(), "data top / bottom".to_string()),
                ("{ / }".to_string(), "used range top / bottom".to_string()),
                (
                    "0 ^ $".to_string(),
                    "line start / first non-blank / end".to_string(),
                ),
                ("v / V".to_string(), "range / row selection".to_string()),
                ("w".to_string(), "fit columns to screen".to_string()),
                ("y".to_string(), "copy".to_string()),
                (
                    "i a I A s =".to_string(),
                    "edit cell / insert value".to_string(),
                ),
                ("u / Ctrl-r".to_string(), "undo / redo".to_string()),
                (
                    "x r d".to_string(),
                    "clear / replace / delete operator".to_string(),
                ),
                ("K / Enter".to_string(), "cell detail".to_string()),
                (
                    "/ n N * #".to_string(),
                    "workbook search / next / previous / word".to_string(),
                ),
                ("f".to_string(), "filter panel".to_string()),
                (
                    "Ctrl-o / Ctrl-i".to_string(),
                    "jump back / forward".to_string(),
                ),
                (":".to_string(), "command line".to_string()),
                (
                    ":e / :e!".to_string(),
                    "reload file / discard edits".to_string(),
                ),
                ("Tab".to_string(), "next sheet".to_string()),
                (
                    "q / ZZ / ZQ".to_string(),
                    "quit / save quit / discard quit".to_string(),
                ),
            ],
        }
    }

    fn reload_label(&self) -> String {
        match self.watch_configured {
            WatchMode::Off => "watch:off".to_string(),
            WatchMode::Auto => format!("watch:auto/{}", self.watch_effective.label()),
            configured if configured != self.watch_effective => {
                format!("watch:{} (fallback)", self.watch_effective.label())
            }
            configured => format!("watch:{}", configured.label()),
        }
    }

    fn filter_panel_view(&self) -> Option<FilterPanelView> {
        let sheet = self.active_sheet()?;
        let mut lines = Vec::new();
        for filter in &sheet.filters {
            for column in &filter.columns {
                lines.push(format!(
                    "{}: {:?}",
                    crate::domain::addr::column_label(filter.range.start.col + column.col_id),
                    column.criterion
                ));
            }
        }
        if lines.is_empty() {
            lines.push("No saved filters".to_string());
        }
        lines.insert(
            0,
            format!(
                "enabled: {}  visible rows: {} / {}",
                sheet.filter_state.enabled,
                sheet.visibility.visible_count(),
                sheet.meta.row_count
            ),
        );
        Some(FilterPanelView {
            lines,
            selected: self.selected_filter,
        })
    }

    pub fn view(&mut self) -> ScreenView {
        self.ensure_focus_visible();
        let index = self.active_sheet_index();
        if let Some(search) = &mut self.search_index {
            search.set_active_sheet(index);
        }
        let (frozen_rows, start_row) = self
            .active_sheet()
            .map(|sheet| {
                let widths = self.column_widths(sheet);
                let frozen = LayoutEngine::effective_freeze(sheet, &self.viewport, &widths).rows;
                let start = sheet
                    .visibility
                    .visible_rows()
                    .iter()
                    .copied()
                    .filter(|row| *row >= frozen)
                    .nth(self.viewport.top_visible_row as usize)
                    .unwrap_or(frozen);
                (frozen, start)
            })
            .unwrap_or((0, 0));
        if let Some(workbook) = &mut self.workbook {
            if frozen_rows > 0 {
                let _ = workbook.ensure_rows(index, RowRange::new(0, frozen_rows));
            }
            let _ = workbook.ensure_rows(
                index,
                RowRange::new(start_row, start_row + self.viewport.height as u32 + 2),
            );
        }
        let Some(sheet) = self.active_sheet() else {
            return ScreenView {
                mode: self.mode,
                tabs: Vec::new(),
                active_sheet: 0,
                focus: self.focus,
                viewport: self.viewport,
                plan: RenderPlan::default(),
                selection: self.selection,
                search: None,
                status: StatusView::default(),
                detail: None,
                help: None,
                search_prompt: None,
                filter_panel: None,
                input_line: None,
            };
        };
        let widths = self.column_widths(sheet);
        let plan = LayoutEngine::plan(
            sheet,
            &self.viewport,
            self.focus,
            self.selection.as_ref(),
            self.search_index.as_ref(),
            &widths,
        );
        let tabs = self
            .workbook
            .as_ref()
            .map(|workbook| {
                workbook
                    .sheets
                    .iter()
                    .map(|sheet| sheet.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let status = StatusView {
            file_name: self
                .workbook
                .as_ref()
                .map(|workbook| {
                    workbook
                        .source
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("(file)")
                        .to_string()
                })
                .unwrap_or_default(),
            sheet_name: sheet.name.clone(),
            cell_label: self.focus.a1(),
            mode_label: self.mode.label().to_string(),
            dirty: self.edit_log.is_dirty(),
            fit_label: if self.fit_width {
                if widths.limited() {
                    "fit:limited".to_string()
                } else {
                    "fit:on".to_string()
                }
            } else {
                String::new()
            },
            selection_count: self
                .selection
                .map(|selection| {
                    let range = selection.range(
                        sheet.used_range.end.row.saturating_add(1),
                        sheet.used_range.end.col.saturating_add(1),
                    );
                    (range.row_count() as usize) * (range.col_count() as usize)
                })
                .unwrap_or(1),
            selection_kind: self.selection.map(|selection| selection.mode),
            formula_label: if sheet.formula_overlay.values.is_empty() {
                String::new()
            } else {
                format!("calc:{}", sheet.formula_overlay.values.len())
            },
            search_label: self
                .search_index
                .as_ref()
                .map(|index| {
                    let position = index.current.map(|current| current + 1).unwrap_or(0);
                    let context = index
                        .current_match()
                        .map(|matched| {
                            let sheet = self
                                .workbook
                                .as_ref()
                                .and_then(|workbook| workbook.sheets.get(matched.sheet))
                                .map(|sheet| sheet.name.as_str())
                                .unwrap_or("?");
                            format!("{sheet}!{}", matched.addr.a1())
                        })
                        .unwrap_or_default();
                    if context.is_empty() {
                        format!("search:{position}/{}", index.matches.len())
                    } else {
                        format!("search:{position}/{} {context}", index.matches.len())
                    }
                })
                .unwrap_or_default(),
            filter_label: if sheet.filters.is_empty() {
                String::new()
            } else {
                format!(
                    "filter:{}/{}",
                    sheet.visibility.visible_count(),
                    sheet.meta.row_count
                )
            },
            freeze_label: if plan.freeze.rows > 0
                || plan.freeze.cols > 0
                || plan.freeze.rows_limited
                || plan.freeze.cols_limited
            {
                format!(
                    "freeze:{}r/{}c{}{}",
                    plan.freeze.rows,
                    plan.freeze.cols,
                    if plan.freeze.rows_limited {
                        " rows-limited"
                    } else {
                        ""
                    },
                    if plan.freeze.cols_limited {
                        " cols-limited"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            },
            load_label: String::new(),
            reload_label: self.reload_label(),
            encoding_label: self
                .workbook
                .as_ref()
                .and_then(|workbook| workbook.source.encoding.clone()),
            message: self
                .status_message
                .clone()
                .or_else(|| sheet.warnings.first().cloned()),
        };
        let search = self.search_index.as_ref().map(|index| SearchHighlight {
            matches: index
                .matches
                .iter()
                .filter(|matched| matched.sheet == index.active_sheet)
                .map(|matched| matched.addr)
                .collect(),
            current: index
                .current_match()
                .filter(|matched| matched.sheet == index.active_sheet)
                .map(|matched| matched.addr),
            hidden: index.hidden_matches,
        });
        let detail = if self.mode == Mode::Detail {
            self.detail_view()
        } else {
            None
        };
        let help = if self.mode == Mode::Help {
            Some(self.help_view())
        } else {
            None
        };
        let search_prompt = if self.mode == Mode::Search || self.search_index.is_some() {
            let current = self
                .search_index
                .as_ref()
                .and_then(|index| index.current_match());
            let sheet = current.and_then(|matched| {
                self.workbook
                    .as_ref()
                    .and_then(|workbook| workbook.sheets.get(matched.sheet))
                    .map(|sheet| sheet.name.clone())
            });
            Some(SearchPromptView {
                query: self.search_query.clone(),
                current: self
                    .search_index
                    .as_ref()
                    .and_then(|index| index.current)
                    .map(|index| index + 1)
                    .unwrap_or(0),
                total: self
                    .search_index
                    .as_ref()
                    .map(|index| index.matches.len())
                    .unwrap_or(0),
                hidden: self
                    .search_index
                    .as_ref()
                    .map(|index| index.hidden_matches)
                    .unwrap_or(0),
                searching: false,
                sheet,
                cell: current.map(|matched| matched.addr.a1()),
            })
        } else {
            None
        };
        let input_line = self.input.as_ref().map(|input| {
            let prompt = match input.kind {
                InputKind::Cell { addr, .. } => format!("{} > ", addr.a1()),
                InputKind::Command => ":".to_string(),
            };
            InputLineView {
                prompt,
                text: input.text(),
                cursor: input.cursor,
            }
        });
        let filter_panel = if self.mode == Mode::Filter {
            self.filter_panel_view()
        } else {
            None
        };
        ScreenView {
            mode: self.mode,
            tabs,
            active_sheet: index,
            focus: self.focus,
            viewport: self.viewport,
            plan,
            selection: self.selection,
            search,
            status,
            detail,
            help,
            search_prompt,
            filter_panel,
            input_line,
        }
    }
}

#[derive(Clone, Debug)]
struct FilterSnapshot {
    sheet_name: String,
    source: FilterSource,
    enabled: bool,
    columns: Vec<(u32, bool)>,
}

fn empty_cell() -> Cell {
    Cell::new(CellValue::Empty, Default::default())
}

fn transaction_sheets(transaction: &EditTransaction) -> Vec<usize> {
    let mut sheets: Vec<usize> = transaction
        .ops
        .iter()
        .map(|op| match op {
            EditOp::Cell(change) => change.sheet,
            EditOp::Structure(edit) => edit.sheet,
        })
        .collect();
    sheets.sort_unstable();
    sheets.dedup();
    sheets
}

fn snapshot_filters(workbook: &WorkbookModel) -> Vec<FilterSnapshot> {
    let mut snapshots = Vec::new();
    for sheet in &workbook.sheets {
        let mut offset = 0;
        for definition in &sheet.filters {
            let columns = definition
                .columns
                .iter()
                .enumerate()
                .filter_map(|(index, column)| {
                    sheet
                        .filter_state
                        .column_enabled
                        .get(offset + index)
                        .map(|enabled| (column.col_id, *enabled))
                })
                .collect();
            snapshots.push(FilterSnapshot {
                sheet_name: sheet.name.clone(),
                source: definition.source.clone(),
                enabled: sheet.filter_state.enabled,
                columns,
            });
            offset += definition.columns.len();
        }
    }
    snapshots
}

fn apply_filter_snapshots(workbook: &mut WorkbookModel, snapshots: &[FilterSnapshot]) -> bool {
    let mut reset = false;
    for sheet in &mut workbook.sheets {
        if !snapshots
            .iter()
            .any(|snapshot| snapshot.sheet_name == sheet.name)
        {
            continue;
        }
        let defaults = sheet.filter_state.clone();
        let mut sheet_reset = false;
        if let Some(snapshot) = snapshots
            .iter()
            .find(|snapshot| snapshot.sheet_name == sheet.name)
        {
            sheet.filter_state.enabled = snapshot.enabled;
        }
        let mut offset = 0;
        for definition in &sheet.filters {
            let snapshot = snapshots.iter().find(|snapshot| {
                snapshot.sheet_name == sheet.name && snapshot.source == definition.source
            });
            let Some(snapshot) = snapshot else {
                sheet_reset = true;
                offset += definition.columns.len();
                continue;
            };
            for (index, column) in definition.columns.iter().enumerate() {
                match snapshot
                    .columns
                    .iter()
                    .find(|(col_id, _)| *col_id == column.col_id)
                {
                    Some((_, enabled)) => {
                        if let Some(target) =
                            sheet.filter_state.column_enabled.get_mut(offset + index)
                        {
                            *target = *enabled;
                        } else {
                            sheet_reset = true;
                        }
                    }
                    None => sheet_reset = true,
                }
            }
            offset += definition.columns.len();
        }
        if sheet_reset {
            sheet.filter_state = defaults;
            reset = true;
        }
        if sheet.rebuild_visibility().is_err() {
            reset = true;
        }
    }
    reset
}
