//! Disjoint-set (union-find) over contiguous integer nodes.
//!
//! Callers map their domain onto `0..len` node indices.

use cadmpeg_core::CodecError;

/// A disjoint-set forest with path compression on `find`.
#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    parents: Vec<usize>,
}

impl UnionFind {
    /// Creates `length` singleton sets, one per node `0..length`.
    pub(crate) fn new(length: usize) -> Self {
        Self {
            parents: (0..length).collect(),
        }
    }

    /// Returns the number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.parents.len()
    }

    /// Appends a new singleton node and returns its index.
    pub(crate) fn push(&mut self) -> usize {
        let index = self.parents.len();
        self.parents.push(index);
        index
    }

    /// Returns the representative of `node`, compressing its path.
    pub(crate) fn find(&mut self, mut node: usize) -> Result<usize, CodecError> {
        let root = self.root(node)?;
        while node != root {
            let slot = self
                .parents
                .get_mut(node)
                .ok_or_else(|| CodecError::malformed("union node is outside its forest"))?;
            let parent = *slot;
            *slot = root;
            node = parent;
        }
        Ok(root)
    }

    /// Returns the representative without changing the forest.
    pub(super) fn root(&self, mut node: usize) -> Result<usize, CodecError> {
        loop {
            let parent = *self
                .parents
                .get(node)
                .ok_or_else(|| CodecError::malformed("union node is outside its forest"))?;
            if parent == node {
                return Ok(node);
            }
            node = parent;
        }
    }

    /// Merges the sets containing two nodes of this forest.
    pub(crate) fn union(&mut self, left: usize, right: usize) -> Result<(), CodecError> {
        let left = self.find(left)?;
        let right = self.find(right)?;
        if left != right {
            *self
                .parents
                .get_mut(right)
                .ok_or_else(|| CodecError::malformed("union root is outside its forest"))? = left;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn long_chain_compression_preserves_left_root_selection() {
        const LAST: usize = 100_000;
        let mut union = UnionFind::new(LAST + 1);
        for node in 0..LAST {
            union.union(node + 1, node).expect("forest traversal");
        }
        assert_eq!(union.root(0).expect("forest traversal"), LAST);
        assert_eq!(union.find(0).expect("forest traversal"), LAST);
        assert!(union.parents.iter().all(|parent| *parent == LAST));
        let separate = union.push();
        assert_eq!(union.find(separate).expect("forest traversal"), separate);
        assert_ne!(union.find(0).expect("forest traversal"), separate);
        union.union(separate, 0).expect("forest traversal");
        assert_eq!(union.find(0).expect("forest traversal"), separate);
        assert_eq!(union.find(LAST).expect("forest traversal"), separate);
        assert_eq!(union.root(separate).expect("forest traversal"), separate);
    }

    #[test]
    fn union_nodes_are_admitted_against_each_forest() {
        let mut empty = UnionFind::new(0);
        assert!(matches!(
            empty.find(0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(matches!(
            empty.root(0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(matches!(
            empty.union(0, 0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        let mut small = UnionFind::new(1);
        let large = UnionFind::new(2);
        let foreign = large.root(1).expect("large node");
        assert!(matches!(
            small.find(foreign),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert_eq!(small.root(0).expect("own node"), 0);
    }
}
