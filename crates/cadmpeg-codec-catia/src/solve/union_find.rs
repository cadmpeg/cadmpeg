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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)?;
        for (node, parent) in parents.iter_mut().enumerate() {
            *parent = node;
        }
        Ok(Self {
            parents,
            _storage: None,
        })
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(self.parents.len()),
            operation,
        )?;
        let (parents, storage) = ctx.copy_temporary_slice(&self.parents, operation)?;
        Ok(UnionFind {
            parents,
            _storage: Some(storage),
        })
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

    /// Returns the representative of `node`, compressing its admitted path.
    pub(crate) fn find(
        &mut self,
        ctx: &DecodeContext<'_>,
        mut node: usize,
    ) -> Result<usize, CodecError> {
        let root = self.root(ctx, node)?;
        while node != root {
            ctx.charge_work(2, "catia_union_compression")?;
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
    pub(super) fn root(
        &self,
        ctx: &DecodeContext<'_>,
        mut node: usize,
    ) -> Result<usize, CodecError> {
        loop {
            ctx.charge_work(1, "catia_union_traversal")?;
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

    /// Merges the sets containing two nodes admitted against this forest.
    pub(crate) fn union(
        &mut self,
        ctx: &DecodeContext<'_>,
        left: usize,
        right: usize,
    ) -> Result<(), CodecError> {
        let left = self.find(ctx, left)?;
        let right = self.find(ctx, right)?;
        if left != right {
            ctx.charge_work(1, "catia_union_link")?;
            *self
                .parents
                .get_mut(right)
                .ok_or_else(|| CodecError::malformed("union root is outside its forest"))? = left;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Clone for UnionFind<'_> {
    fn clone(&self) -> Self {
        Self {
            parents: self.parents.clone(),
            _storage: None,
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
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, 1))
                .expect("service forest traversal"),
            1
        );

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
                assert_eq!(
                    union
                        .clone_charged(ctx, "catia_union_snapshot")
                        .expect("temporary snapshot")
                        .len(),
                    1
                );
            }
        });
        crate::test_support::with_materialized_limit(
            u64::try_from(std::mem::size_of::<usize>()).expect("parent bytes"),
            |ctx| {
                for _ in 0..64 {
                    assert_eq!(
                        union
                            .clone_charged(ctx, "catia_union_snapshot")
                            .expect("released snapshot bytes")
                            .len(),
                        1
                    );
                }
                let first = union
                    .clone_charged(ctx, "catia_union_snapshot")
                    .expect("one live snapshot");
                let cadmpeg_core::CodecError::ResourceLimit(limit) = union
                    .clone_charged(ctx, "catia_union_snapshot")
                    .expect_err("two simultaneous snapshots exceed storage")
                else {
                    panic!("resource refusal required")
                };
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                );
                assert_eq!(ctx.resource_refusal(), Some(limit));
                assert_eq!(first.len(), 1);
            },
        );
    }

    #[test]
    fn long_chain_compression_preserves_left_root_selection() {
        const LAST: usize = 100_000;
        let mut union = UnionFind::new(LAST + 1);
        for node in 0..LAST {
            crate::test_support::with_service_context(|ctx| union.union(ctx, node + 1, node))
                .expect("service forest traversal");
        }
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.root(ctx, 0))
                .expect("service forest traversal"),
            LAST
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, 0))
                .expect("service forest traversal"),
            LAST
        );
        assert!(union.parents.iter().all(|parent| *parent == LAST));
        let separate = union.push();
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, separate))
                .expect("service forest traversal"),
            separate
        );
        assert_ne!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, 0))
                .expect("service forest traversal"),
            separate
        );
        crate::test_support::with_service_context(|ctx| union.union(ctx, separate, 0))
            .expect("service forest traversal");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, 0))
                .expect("service forest traversal"),
            separate
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, LAST))
                .expect("service forest traversal"),
            separate
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.root(ctx, separate))
                .expect("service forest traversal"),
            separate
        );
    }
    #[test]
    fn union_traversal_refuses_before_walking_a_long_chain() {
        let mut union = UnionFind::new(32);
        crate::test_support::with_service_context(|ctx| {
            for node in 0..31 {
                union.union(ctx, node + 1, node).expect("service merge");
            }
        });
        let refusal = crate::test_support::with_work_limit(4, |ctx| union.find(ctx, 0))
            .expect_err("chain exceeds work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = refusal else {
            panic!("work refusal")
        };
        assert_eq!(limit.operation, "catia_union_traversal");
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::WorkUnits
        );
        assert_eq!(union.parents[0], 1);
    }

    #[test]
    fn union_nodes_are_admitted_against_each_forest() {
        crate::test_support::with_service_context(|ctx| {
            let mut empty = UnionFind::new(0);
            assert!(matches!(
                empty.find(ctx, 0),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert!(matches!(
                empty.root(ctx, 0),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert!(matches!(
                empty.union(ctx, 0, 0),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            let mut small = UnionFind::new(1);
            let large = UnionFind::new(2);
            let foreign = large.root(ctx, 1).expect("large node");
            assert!(matches!(
                small.find(ctx, foreign),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert_eq!(small.root(ctx, 0).expect("own node"), 0);
        });
    }
}
