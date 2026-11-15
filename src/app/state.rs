use crate::app::action::{Action, Effect, Motion};
use crate::app::reload::{ReloadSummary, ReloadTrigger};
use crate::config::Config;
use crate::domain::addr::CellAddr;
use crate::domain::cell::{CellRead, EffectiveCell};
use crate::domain::copy::{CopyEncoder, CopyLimits};
use crate::domain::filter::{FilterSource, FilterState};
use crate::domain::formula::{FormulaOverlay, FormulaResults, FormulaStatus};
use crate::domain::layout::{LayoutEngine, RenderPlan, Viewport};
use crate::domain::mode::Mode;
use crate::domain::reload::WatchMode;
use crate::domain::row_store::RowRange;
use crate::domain::search::{MatchSource, SearchIndex, SearchMatch, SearchQuery};
use crate::domain::selection::Selection;
use crate::domain::sheet::{Sheet, SheetData};
use crate::domain::viewport::ViewportPlanner;
use crate::domain::workbook::WorkbookModel;
use crate::ui::view::{
    DetailView, FilterPanelView, HelpView, ScreenView, SearchHighlight, SearchPromptView,
    StatusView,
};

pub struct AppState {
    pub workbook: Option<WorkbookModel>,
    pub focus: CellAddr,
    pub selection: Option<Selection>,
    pub mode: Mode,
    pub viewport: Viewport,
    pub detail_scroll: usize,
    pub search_query: String,
    pub search_index: Option<SearchIndex>,
    pub status_message: Option<String>,
    pub config: Config,
    pub should_quit: bool,
    pub search_running: bool,
    pub selected_filter: usize,
    pub watch_configured: WatchMode,
    pub watch_effective: WatchMode,
}

impl AppState {
    pub fn new(workbook: WorkbookModel, config: Config) -> Self {
        let active = workbook.active_sheet;
        let focus = CellAddr::new(0, 0);
        let watch_mode = config.watch.mode;
        Self {
            workbook: Some(workbook),
            focus,
            selection: None,
            mode: Mode::Normal,
            viewport: Viewport {
                top_visible_row: 0,
                left_col: 0,
                height: 20,
                width: 80,
            },
            detail_scroll: 0,
            search_query: String::new(),
            search_index: None,
            status_message: None,
            config,
            should_quit: false,
            search_running: false,
            selected_filter: 0,
            watch_configured: watch_mode,
            watch_effective: watch_mode,
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
            Action::Copy => match self.copy_payload() {
                Ok(payload) => vec![Effect::WriteClipboard(payload.text)],
                Err(message) => {
                    self.status_message = Some(message);
                    Vec::new()
                }
            },
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
                self.should_quit = true;
                vec![Effect::Quit]
            }
        }
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
        self.search_index = None;
        self.viewport.top_visible_row = 0;
        self.viewport.left_col = 0;
        self.rebuild_visibility();
        self.ensure_focus_visible();
    }

    fn move_focus(&mut self, motion: Motion, extend: bool) {
        if matches!(motion, Motion::FirstNonBlank) {
            self.ensure_focus_loaded();
        }
        let Some(sheet) = self.active_sheet() else {
            return;
        };
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
        // Absolute jumps already resolve any merge at the target below.
        // Escaping from the current merge is only meaningful for relative moves.
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
            // Moving past a merge that touches the edge of the used range must
            // not place the cursor outside the rendered grid. Otherwise the
            // focus cell disappears and no highlight is shown.
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

    fn ensure_focus_visible(&mut self) {
        let Some(sheet) = self.active_sheet() else {
            return;
        };
        let freeze = LayoutEngine::effective_freeze(sheet, &self.viewport);
        let plan = ViewportPlanner::ensure_visible(sheet, &self.viewport, self.focus, freeze);
        self.viewport.top_visible_row = plan.top_visible_row;
        self.viewport.left_col = plan.left_col;
    }

    fn ensure_focus_loaded(&mut self) {
        let index = self.active_sheet_index();
        if let Some(workbook) = &mut self.workbook {
            let _ = workbook.ensure_rows(index, RowRange::new(self.focus.row, self.focus.row + 1));
        }
    }

    fn copy_payload(&mut self) -> Result<crate::domain::copy::CopyPayload, String> {
        let Some(selection) = self.selection else {
            self.ensure_focus_loaded();
            let Some(sheet) = self.active_sheet() else {
                return Err("no sheet".to_string());
            };
            let cell = sheet.read_cell(self.focus);
            return match cell {
                CellRead::Loaded(cell) => Ok(CopyEncoder::single(&cell)),
                CellRead::Pending => Err("row is still loading".to_string()),
                CellRead::Missing => Err("cell is outside the sheet".to_string()),
            };
        };
        let limits = CopyLimits {
            max_cells: self.config.cli.max_copy_cells,
        };
        let index = self.active_sheet_index();
        let Some(workbook) = &mut self.workbook else {
            return Err("no sheet".to_string());
        };
        workbook
            .copy_selection(index, &selection, &limits)
            .map_err(|error| error.to_string())
    }

    fn submit_search(&mut self) {
        let text = self.search_query.trim().to_string();
        if text.is_empty() {
            self.search_index = None;
            return;
        }
        let query = SearchQuery::new(text);
        let mut matches = Vec::new();
        if let Some(sheet) = self.active_sheet_mut() {
            match &mut sheet.data {
                SheetData::Eager(cells) => {
                    let overlay = sheet.formula_overlay.clone();
                    for (addr, cell) in cells {
                        let effective = effective_from_parts(&overlay, *addr, cell);
                        if query.matches(&effective.display) {
                            matches.push(SearchMatch {
                                addr: *addr,
                                source: MatchSource::Display,
                            });
                        } else if let Some(formula) = &effective.formula
                            && query.matches(&formula.expression)
                        {
                            matches.push(SearchMatch {
                                addr: *addr,
                                source: MatchSource::Formula,
                            });
                        }
                    }
                }
                SheetData::Lazy(lazy) => {
                    let overlay = sheet.formula_overlay.clone();
                    let _ = lazy.scan_all(&mut |row| {
                        for (col, cell) in &row.cells {
                            let addr = CellAddr::new(row.row, *col);
                            let effective = if let Some(result) = overlay.get(addr) {
                                EffectiveCell {
                                    value: result.value.clone(),
                                    display: result
                                        .value
                                        .display_text_with_format(cell.number_format.as_deref()),
                                    style: cell.style,
                                    formula: cell.formula.clone(),
                                    formula_status: result.status.clone(),
                                }
                            } else {
                                cell.effective(
                                    cell.formula
                                        .as_ref()
                                        .map(|formula| formula.status.clone())
                                        .unwrap_or(FormulaStatus::NotRequested),
                                )
                            };
                            if query.matches(&effective.display) {
                                matches.push(SearchMatch {
                                    addr,
                                    source: MatchSource::Display,
                                });
                            } else if let Some(formula) = &effective.formula
                                && query.matches(&formula.expression)
                            {
                                matches.push(SearchMatch {
                                    addr,
                                    source: MatchSource::Formula,
                                });
                            }
                        }
                        std::ops::ControlFlow::Continue(())
                    });
                }
            }
        }
        matches.sort_by_key(|matched| (matched.addr.row, matched.addr.col));
        let visibility = self.active_sheet().map(|sheet| sheet.visibility.clone());
        let mut index = SearchIndex::new(query, matches);
        if let Some(visibility) = &visibility {
            index.sync_visibility(visibility);
        }
        let total = index.matches.len();
        let current = index.current_match().cloned();
        self.search_index = Some(index);
        if let Some(matched) = current {
            self.focus = matched.addr;
            self.ensure_focus_visible();
        } else if total > 0 {
            self.status_message = Some("no visible match".to_string());
        } else {
            self.status_message = Some("no match".to_string());
        }
    }

    fn next_match(&mut self, forward: bool) {
        let Some(visibility) = self.active_sheet().map(|sheet| sheet.visibility.clone()) else {
            return;
        };
        let Some(index) = &mut self.search_index else {
            return;
        };
        let matched = if forward {
            index.next_visible(&visibility)
        } else {
            index.previous_visible(&visibility)
        };
        if let Some(matched) = matched {
            self.focus = matched.addr;
            self.ensure_focus_visible();
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
            for column in &mut sheet.filter_state.column_enabled {
                *column = value;
            }
        }
        self.rebuild_visibility();
    }

    fn rebuild_visibility(&mut self) {
        let mut error_message = None;
        let mut next_focus_row = None;
        let mut visibility = None;
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
                visibility = Some(sheet.visibility.clone());
            }
        }
        if let Some(row) = next_focus_row {
            self.focus = CellAddr::new(row, self.focus.col);
        }
        if let Some(visibility) = visibility
            && let Some(index) = &mut self.search_index
        {
            index.sync_visibility(&visibility);
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
            truncated: LayoutEngine::truncated_at(sheet, &sheet.layout_limits, self.focus),
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
                ("y".to_string(), "copy".to_string()),
                ("Enter / d".to_string(), "cell detail".to_string()),
                ("/ n N".to_string(), "search / next / previous".to_string()),
                ("f".to_string(), "filter panel".to_string()),
                ("r".to_string(), "reload file".to_string()),
                ("Tab".to_string(), "next sheet".to_string()),
                ("q".to_string(), "quit".to_string()),
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
        let (frozen_rows, start_row) = self
            .active_sheet()
            .map(|sheet| {
                let frozen = LayoutEngine::effective_freeze(sheet, &self.viewport).rows;
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
            };
        };
        let plan = LayoutEngine::plan(
            sheet,
            &self.viewport,
            self.focus,
            self.selection.as_ref(),
            self.search_index.as_ref(),
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
            mode_label: format!("{:?}", self.mode),
            selection_count: self
                .selection
                .map(|selection| {
                    let range = selection.range(sheet.used_range.end.col + 1);
                    (range.row_count() as usize) * (range.col_count() as usize)
                })
                .unwrap_or(1),
            formula_label: if sheet.formula_overlay.values.is_empty() {
                String::new()
            } else {
                format!("calc:{}", sheet.formula_overlay.values.len())
            },
            search_label: self
                .search_index
                .as_ref()
                .map(|index| {
                    format!(
                        "search:{}/{}",
                        index.current.map(|current| current + 1).unwrap_or(0),
                        index.matches.len()
                    )
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
            matches: index.matches.iter().map(|matched| matched.addr).collect(),
            current: index.current_match().map(|matched| matched.addr),
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
            })
        } else {
            None
        };
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

fn effective_from_parts(
    overlay: &FormulaOverlay,
    addr: CellAddr,
    cell: &crate::domain::cell::Cell,
) -> EffectiveCell {
    if let Some(result) = overlay.get(addr) {
        let mut formula = cell.formula.clone();
        if let Some(info) = formula.as_mut() {
            info.evaluated = Some(result.value.clone());
            info.status = result.status.clone();
        }
        return EffectiveCell {
            value: result.value.clone(),
            display: result
                .value
                .display_text_with_format(cell.number_format.as_deref()),
            style: cell.style,
            formula,
            formula_status: result.status.clone(),
        };
    }
    let status = cell
        .formula
        .as_ref()
        .map(|formula| formula.status.clone())
        .unwrap_or(FormulaStatus::NotRequested);
    cell.effective(status)
}
