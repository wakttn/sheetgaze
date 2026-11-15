use std::fmt;

/// Zero-based cell coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellAddr {
    pub row: u32,
    pub col: u32,
}

impl CellAddr {
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }

    pub fn a1(self) -> String {
        format!("{}{}", column_label(self.col), self.row + 1)
    }

    pub fn from_a1(value: &str) -> Option<Self> {
        let mut letters = String::new();
        let mut digits = String::new();
        for ch in value.chars() {
            if ch.is_ascii_alphabetic() && digits.is_empty() {
                letters.push(ch.to_ascii_uppercase());
            } else if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                return None;
            }
        }
        if letters.is_empty() || digits.is_empty() {
            return None;
        }
        let col = column_index(&letters)?;
        let row: u32 = digits.parse().ok()?;
        if row == 0 {
            return None;
        }
        Some(Self::new(row - 1, col))
    }
}

impl fmt::Display for CellAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.a1())
    }
}

/// Inclusive rectangular range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CellRange {
    pub start: CellAddr,
    pub end: CellAddr,
}

impl CellRange {
    pub fn new(a: CellAddr, b: CellAddr) -> Self {
        Self {
            start: CellAddr::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellAddr::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    pub fn single(addr: CellAddr) -> Self {
        Self {
            start: addr,
            end: addr,
        }
    }

    pub fn contains(self, addr: CellAddr) -> bool {
        addr.row >= self.start.row
            && addr.row <= self.end.row
            && addr.col >= self.start.col
            && addr.col <= self.end.col
    }

    pub fn row_count(self) -> u32 {
        self.end.row.saturating_sub(self.start.row) + 1
    }

    pub fn col_count(self) -> u32 {
        self.end.col.saturating_sub(self.start.col) + 1
    }

    pub fn intersects(self, other: Self) -> bool {
        self.start.row <= other.end.row
            && self.end.row >= other.start.row
            && self.start.col <= other.end.col
            && self.end.col >= other.start.col
    }

    pub fn rows(self) -> impl Iterator<Item = u32> {
        self.start.row..=self.end.row
    }

    pub fn cols(self) -> impl Iterator<Item = u32> {
        self.start.col..=self.end.col
    }
}

pub fn column_label(mut col: u32) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out
}

pub fn column_index(label: &str) -> Option<u32> {
    let mut value: u32 = 0;
    for ch in label.chars() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        value = value.checked_mul(26)?;
        value = value.checked_add((ch.to_ascii_uppercase() as u8 - b'A') as u32 + 1)?;
    }
    value.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_roundtrip() {
        let addr = CellAddr::new(2, 27);
        assert_eq!(addr.a1(), "AB3");
        assert_eq!(CellAddr::from_a1("AB3"), Some(addr));
    }

    #[test]
    fn range_normalizes() {
        let range = CellRange::new(CellAddr::new(5, 3), CellAddr::new(1, 1));
        assert_eq!(range.start, CellAddr::new(1, 1));
        assert_eq!(range.end, CellAddr::new(5, 3));
        assert!(range.contains(CellAddr::new(3, 2)));
        assert!(!range.contains(CellAddr::new(0, 2)));
    }
}
