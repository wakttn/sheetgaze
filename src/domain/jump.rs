use crate::domain::addr::CellAddr;

const MAX_JUMP_ENTRIES: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JumpEntry {
    pub sheet: usize,
    pub addr: CellAddr,
}

/// Vim-like jump history for cross-sheet search and `:goto`.
#[derive(Clone, Debug, Default)]
pub struct JumpList {
    entries: Vec<JumpEntry>,
    cursor: usize,
}

impl JumpList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: JumpEntry) {
        if self.entries.get(self.cursor) == Some(&entry) {
            return;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(entry);
        if self.entries.len() > MAX_JUMP_ENTRIES {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len().saturating_sub(1);
    }

    pub fn back(&mut self) -> Option<JumpEntry> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).copied()
    }

    pub fn forward(&mut self) -> Option<JumpEntry> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor).copied()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_back_and_forward() {
        let mut jumps = JumpList::new();
        jumps.push(JumpEntry {
            sheet: 0,
            addr: CellAddr::new(0, 0),
        });
        jumps.push(JumpEntry {
            sheet: 1,
            addr: CellAddr::new(2, 1),
        });
        assert_eq!(
            jumps.back(),
            Some(JumpEntry {
                sheet: 0,
                addr: CellAddr::new(0, 0)
            })
        );
        assert_eq!(jumps.back(), None);
        assert_eq!(
            jumps.forward(),
            Some(JumpEntry {
                sheet: 1,
                addr: CellAddr::new(2, 1)
            })
        );
        assert_eq!(jumps.forward(), None);
    }

    #[test]
    fn drops_the_oldest_entry_at_the_limit() {
        let mut jumps = JumpList::new();
        for row in 0..(MAX_JUMP_ENTRIES + 5) {
            jumps.push(JumpEntry {
                sheet: 0,
                addr: CellAddr::new(row as u32, 0),
            });
        }
        assert_eq!(jumps.len(), MAX_JUMP_ENTRIES);
        let mut first = None;
        while let Some(entry) = jumps.back() {
            first = Some(entry);
        }
        assert_eq!(first.unwrap().addr.row, 5);
    }
}
