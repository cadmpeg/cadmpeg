//! Disjoint-set (union-find) over contiguous integer nodes.
//!
//! Callers map their domain onto `0..len` node indices.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// A disjoint-set forest with path compression on `find`.
#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    parents: Vec<usize>,
}

impl UnionFind {
    pub(crate) fn charged(
        ctx: &DecodeContext<'_>,
        length: usize,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut parents = ctx.alloc_filled(length, 0usize, operation)?;
        for (node, parent) in parents.iter_mut().enumerate() {
            *parent = node;
        }
        Ok(Self { parents })
    }

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

    /// Returns the representative of `node`, compressing the path to it.
    pub(crate) fn find(&mut self, mut node: usize) -> usize {
        let root = self.root(node);
        while node != root {
            let parent = self.parents[node];
            self.parents[node] = root;
            node = parent;
        }
        root
    }

    /// Returns the representative of `node` without mutating the forest.
    pub(super) fn root(&self, mut node: usize) -> usize {
        while self.parents[node] != node {
            node = self.parents[node];
        }
        node
    }

    /// Merges the sets containing `left` and `right`.
    pub(crate) fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn charged_union_parents_refuse_below_node_count() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let mut union = UnionFind::charged(&ctx, 2, "catia_union_test_parents")
            .expect("service resource budget");
        assert_eq!(union.find(1), 1);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        assert!(
            matches!(UnionFind::charged(&ctx, 2, "catia_union_test_parents"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "catia_union_test_parents")
        );
    }

    #[test]
    fn long_chain_compression_preserves_left_root_selection() {
        const LAST: usize = 100_000;
        let mut union = UnionFind::new(LAST + 1);
        for node in 0..LAST {
            union.union(node + 1, node);
        }
        assert_eq!(union.root(0), LAST);
        assert_eq!(union.find(0), LAST);
        assert!(union.parents.iter().all(|parent| *parent == LAST));
        let separate = union.push();
        assert_eq!(union.find(separate), separate);
        assert_ne!(union.find(0), separate);
        union.union(separate, 0);
        assert_eq!(union.find(0), separate);
        assert_eq!(union.find(LAST), separate);
        assert_eq!(union.root(separate), separate);
    }
}
