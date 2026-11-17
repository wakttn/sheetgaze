use crate::domain::addr::CellAddr;
use crate::domain::filter::VisibilityMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
}

impl SearchQuery {
    pub fn new(text: String) -> Self {
        let case_sensitive = text.chars().any(|ch| ch.is_uppercase());
        Self {
            text,
            case_sensitive,
        }
    }

    pub fn matches(&self, candidate: &str) -> bool {
        if self.case_sensitive {
            candidate.contains(&self.text)
        } else {
            candidate.to_lowercase().contains(&self.text.to_lowercase())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchSource {
    Display,
    Formula,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub sheet: usize,
    pub addr: CellAddr,
    pub source: MatchSource,
}

#[derive(Clone, Debug)]
pub struct SearchIndex {
    pub query: SearchQuery,
    pub matches: Vec<SearchMatch>,
    pub current: Option<usize>,
    pub hidden_matches: usize,
    /// Sheet whose grid is currently displayed. Only matches on this sheet are
    /// highlighted.
    pub active_sheet: usize,
}

impl SearchIndex {
    pub fn new(query: SearchQuery, matches: Vec<SearchMatch>) -> Self {
        let current = (!matches.is_empty()).then_some(0);
        Self {
            query,
            matches,
            current,
            hidden_matches: 0,
            active_sheet: 0,
        }
    }

    pub fn empty(query: SearchQuery) -> Self {
        Self {
            query,
            matches: Vec::new(),
            current: None,
            hidden_matches: 0,
            active_sheet: 0,
        }
    }

    pub fn set_active_sheet(&mut self, sheet: usize) {
        self.active_sheet = sheet;
    }

    pub fn is_match(&self, addr: CellAddr) -> bool {
        self.matches
            .iter()
            .any(|matched| matched.sheet == self.active_sheet && matched.addr == addr)
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    pub fn sync_visibility(&mut self, visibility: &VisibilityMap) {
        self.sync_visibility_for(std::slice::from_ref(visibility));
    }

    pub fn sync_visibility_for(&mut self, visibilities: &[VisibilityMap]) {
        self.hidden_matches = self
            .matches
            .iter()
            .filter(|matched| !is_visible(visibilities, matched.sheet, matched.addr.row))
            .count();
        let current_visible = self.current.is_some_and(|index| {
            self.matches
                .get(index)
                .is_some_and(|matched| is_visible(visibilities, matched.sheet, matched.addr.row))
        });
        if !current_visible {
            self.current = self
                .matches
                .iter()
                .position(|matched| is_visible(visibilities, matched.sheet, matched.addr.row));
        }
    }

    pub fn next_visible(&mut self, visibility: &VisibilityMap) -> Option<SearchMatch> {
        self.step_in(std::slice::from_ref(visibility), true)
    }

    pub fn previous_visible(&mut self, visibility: &VisibilityMap) -> Option<SearchMatch> {
        self.step_in(std::slice::from_ref(visibility), false)
    }

    pub fn next_visible_in(&mut self, visibilities: &[VisibilityMap]) -> Option<SearchMatch> {
        self.step_in(visibilities, true)
    }

    pub fn previous_visible_in(&mut self, visibilities: &[VisibilityMap]) -> Option<SearchMatch> {
        self.step_in(visibilities, false)
    }

    /// Selects the next (or previous) visible match relative to an arbitrary
    /// workbook position, wrapping around when no match remains in that
    /// direction. Used by `*` / `#`.
    pub fn select_visible_from(
        &mut self,
        sheet: usize,
        addr: CellAddr,
        forward: bool,
        visibilities: &[VisibilityMap],
    ) -> Option<SearchMatch> {
        self.hidden_matches = self
            .matches
            .iter()
            .filter(|matched| !is_visible(visibilities, matched.sheet, matched.addr.row))
            .count();
        let start = (sheet, addr.row, addr.col);
        let mut candidate = None;
        if forward {
            for (index, matched) in self.matches.iter().enumerate() {
                if !is_visible(visibilities, matched.sheet, matched.addr.row) {
                    continue;
                }
                if (matched.sheet, matched.addr.row, matched.addr.col) > start {
                    candidate = Some(index);
                    break;
                }
            }
            if candidate.is_none() {
                candidate = self
                    .matches
                    .iter()
                    .position(|matched| is_visible(visibilities, matched.sheet, matched.addr.row));
            }
        } else {
            for (index, matched) in self.matches.iter().enumerate() {
                if !is_visible(visibilities, matched.sheet, matched.addr.row) {
                    continue;
                }
                if (matched.sheet, matched.addr.row, matched.addr.col) < start {
                    candidate = Some(index);
                }
            }
            if candidate.is_none() {
                candidate = self
                    .matches
                    .iter()
                    .rposition(|matched| is_visible(visibilities, matched.sheet, matched.addr.row));
            }
        }
        self.current = candidate;
        candidate.map(|index| self.matches[index].clone())
    }

    fn step_in(&mut self, visibilities: &[VisibilityMap], forward: bool) -> Option<SearchMatch> {
        if self.matches.is_empty() {
            return None;
        }
        self.hidden_matches = self
            .matches
            .iter()
            .filter(|matched| !is_visible(visibilities, matched.sheet, matched.addr.row))
            .count();
        let len = self.matches.len();
        let mut index = self.current.unwrap_or(0);
        for _ in 0..len {
            index = if forward {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            };
            let matched = &self.matches[index];
            if is_visible(visibilities, matched.sheet, matched.addr.row) {
                self.current = Some(index);
                return Some(matched.clone());
            }
        }
        None
    }
}

fn is_visible(visibilities: &[VisibilityMap], sheet: usize, row: u32) -> bool {
    visibilities
        .get(sheet)
        .map(|visibility| visibility.is_visible(row))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_first_visible_match_across_sheets() {
        let mut index = SearchIndex::new(
            SearchQuery::new("hit".to_string()),
            vec![
                SearchMatch {
                    sheet: 0,
                    addr: CellAddr::new(0, 0),
                    source: MatchSource::Display,
                },
                SearchMatch {
                    sheet: 1,
                    addr: CellAddr::new(2, 1),
                    source: MatchSource::Display,
                },
            ],
        );
        let visibilities = [
            VisibilityMap::from_mask(vec![false]),
            VisibilityMap::from_mask(vec![true, true, true]),
        ];
        index.sync_visibility_for(&visibilities);
        assert_eq!(index.current_match().unwrap().sheet, 1);
        assert_eq!(index.hidden_matches, 1);
        let next = index.next_visible_in(&visibilities).unwrap();
        assert_eq!(next.sheet, 1);
        assert_eq!(next.addr, CellAddr::new(2, 1));
    }
}
