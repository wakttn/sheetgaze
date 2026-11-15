use std::collections::HashMap;

use crate::domain::addr::{CellAddr, CellRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeRegion {
    pub anchor: CellAddr,
    pub range: CellRange,
}

#[derive(Clone, Debug, Default)]
pub struct MergeIndex {
    regions: Vec<MergeRegion>,
    anchors: HashMap<CellAddr, usize>,
    rows: HashMap<u32, Vec<usize>>,
}

impl MergeIndex {
    pub fn new(regions: Vec<MergeRegion>) -> Self {
        let mut index = Self::default();
        for mut region in regions {
            if region.range.start != region.anchor {
                region.anchor = region.range.start;
            }
            if index
                .regions
                .iter()
                .any(|existing| existing.range.intersects(region.range))
            {
                continue;
            }
            let id = index.regions.len();
            for row in region.range.rows() {
                index.rows.entry(row).or_default().push(id);
            }
            index.anchors.insert(region.anchor, id);
            index.regions.push(region);
        }
        index
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn regions(&self) -> &[MergeRegion] {
        &self.regions
    }

    pub fn region_at(&self, addr: CellAddr) -> Option<&MergeRegion> {
        self.rows
            .get(&addr.row)?
            .iter()
            .map(|id| &self.regions[*id])
            .find(|region| region.range.contains(addr))
    }

    pub fn anchor_for(&self, addr: CellAddr) -> CellAddr {
        self.region_at(addr).map_or(addr, |region| region.anchor)
    }

    pub fn is_covered(&self, addr: CellAddr) -> bool {
        self.region_at(addr)
            .is_some_and(|region| region.anchor != addr)
    }

    pub fn is_blocker(&self, addr: CellAddr) -> bool {
        self.region_at(addr).is_some()
    }

    pub fn regions_overlapping_row(&self, row: u32) -> impl Iterator<Item = &MergeRegion> {
        self.rows
            .get(&row)
            .into_iter()
            .flatten()
            .map(|id| &self.regions[*id])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_anchor() {
        let region = MergeRegion {
            anchor: CellAddr::new(1, 1),
            range: CellRange::new(CellAddr::new(1, 1), CellAddr::new(2, 2)),
        };
        let index = MergeIndex::new(vec![region]);
        assert_eq!(index.anchor_for(CellAddr::new(2, 2)), CellAddr::new(1, 1));
        assert!(index.is_covered(CellAddr::new(2, 1)));
        assert!(!index.is_covered(CellAddr::new(1, 1)));
    }
}
