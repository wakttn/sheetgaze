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
    pub addr: CellAddr,
    pub source: MatchSource,
}

#[derive(Clone, Debug)]
pub struct SearchIndex {
    pub query: SearchQuery,
    pub matches: Vec<SearchMatch>,
    pub current: Option<usize>,
    pub hidden_matches: usize,
}

impl SearchIndex {
    pub fn new(query: SearchQuery, matches: Vec<SearchMatch>) -> Self {
        let current = (!matches.is_empty()).then_some(0);
        Self {
            query,
            matches,
            current,
            hidden_matches: 0,
        }
    }

    pub fn empty(query: SearchQuery) -> Self {
        Self {
            query,
            matches: Vec::new(),
            current: None,
            hidden_matches: 0,
        }
    }

    pub fn is_match(&self, addr: CellAddr) -> bool {
        self.matches.iter().any(|matched| matched.addr == addr)
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    pub fn sync_visibility(&mut self, visibility: &VisibilityMap) {
        self.hidden_matches = self
            .matches
            .iter()
            .filter(|matched| !visibility.is_visible(matched.addr.row))
            .count();
        let current_visible = self.current.is_some_and(|index| {
            self.matches
                .get(index)
                .is_some_and(|matched| visibility.is_visible(matched.addr.row))
        });
        if !current_visible {
            self.current = self
                .matches
                .iter()
                .position(|matched| visibility.is_visible(matched.addr.row));
        }
    }

    pub fn next_visible(&mut self, visibility: &VisibilityMap) -> Option<SearchMatch> {
        self.step(visibility, true)
    }

    pub fn previous_visible(&mut self, visibility: &VisibilityMap) -> Option<SearchMatch> {
        self.step(visibility, false)
    }

    fn step(&mut self, visibility: &VisibilityMap, forward: bool) -> Option<SearchMatch> {
        if self.matches.is_empty() {
            return None;
        }
        self.hidden_matches = self
            .matches
            .iter()
            .filter(|matched| !visibility.is_visible(matched.addr.row))
            .count();
        let len = self.matches.len();
        let mut index = self.current.unwrap_or(0);
        for _ in 0..len {
            index = if forward {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            };
            if visibility.is_visible(self.matches[index].addr.row) {
                self.current = Some(index);
                return Some(self.matches[index].clone());
            }
        }
        None
    }
}
