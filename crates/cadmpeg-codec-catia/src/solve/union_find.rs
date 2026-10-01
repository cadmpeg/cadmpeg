//! Disjoint-set (union-find) over contiguous integer nodes.
//!
//! Callers map their domain onto `0..len` node indices.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

/// A disjoint-set forest with path compression on `find`.
#[derive(Debug)]
pub(crate) struct UnionFind<'storage> {
    parents: Vec<usize>,
    /// Live parent storage for a temporary search snapshot.
    _storage: Option<ScopedReservation<'storage>>,
}

impl UnionFind<'_> {
    pub(crate) fn charged(
        ctx: &DecodeContext<'_>,
        length: usize,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut parents = ctx.alloc_filled(length, 0usize, operation)?;
        for (node, parent) in parents.iter_mut().enumerate() {
            *parent = node;
        }
        Ok(Self { parents, _storage: None })
    }

    /// Creates `length` singleton sets, one per node `0..length`.
    #[cfg(test)]
    pub(crate) fn new(length: usize) -> Self {
        Self {
            parents: (0..length).collect(),
            _storage: None,
        }
    }

    pub(crate) fn clone_charged<'storage>(
        &self,
        ctx: &'storage DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<UnionFind<'storage>, CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.parents.len()), operation)?;
        let (parents, storage) = ctx.copy_temporary_slice(&self.parents, operation)?;
        Ok(UnionFind { parents, _storage: Some(storage) })
    }

    /// Returns the number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.parents.len()
    }

    /// Appends a new singleton node and returns its index.
    #[cfg(test)]
    pub(crate) fn push(&mut self) -> usize {
        let index = self.parents.len();
        self.parents.push(index);
        index
    }

    pub(crate) fn push_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        let index = self.parents.len();
        ctx.push_vec(&mut self.parents, index, operation)?;
        Ok(index)
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
impl Clone for UnionFind<'_> {
    fn clone(&self) -> Self { Self { parents: self.parents.clone(), _storage: None } }
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
    fn union_clone_refuses_temporary_parent_bytes() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_core::CodecError;

        let union = UnionFind::new(1);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits retained limit");
        assert!(matches!(
            union.clone_charged(&ctx, "catia_union_clone_bytes").map(|copy| copy.len()),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_union_clone_bytes"
        ));
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                union
                    .clone_charged(ctx, "catia_union_clone_bytes")
                    .expect("service budget")
                    .len(),
                1
            );
        });
    }

    #[test]
    fn union_snapshots_release_scoped_bytes_without_retaining_them() {
        let union = UnionFind::new(1);
        crate::test_support::with_retained_limit(0, |ctx| {
            for _ in 0..64 {
                assert_eq!(union.clone_charged(ctx, "catia_union_snapshot").expect("temporary snapshot").len(), 1);
            }
        });
        crate::test_support::with_materialized_limit(u64::try_from(std::mem::size_of::<usize>()).expect("parent bytes"), |ctx| {
            for _ in 0..64 {
                assert_eq!(union.clone_charged(ctx, "catia_union_snapshot").expect("released snapshot bytes").len(), 1);
            }
            let first = union.clone_charged(ctx, "catia_union_snapshot").expect("one live snapshot");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = union.clone_charged(ctx, "catia_union_snapshot").expect_err("two simultaneous snapshots exceed storage") else { panic!("resource refusal required") };
            assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert_eq!(first.len(), 1);
        });
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
