//! Disjoint-set (union-find) over contiguous integer nodes.
//!
//! Callers map their domain onto `0..len` node indices.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::cell::RefCell;

/// A disjoint-set forest with path compression on representative lookup.
///
/// The parent array is solver scratch: it lives in scoped storage that is
/// released when the forest is dropped.
#[derive(Debug)]
pub(crate) struct UnionFind<'storage> {
    parents: RefCell<Vec<usize>>,
    /// Live parent storage; test forests built without a context hold none.
    storage: Option<ScopedReservation<'storage>>,
}

impl<'storage> UnionFind<'storage> {
    /// Creates `length` singleton sets, admitting one step per parent slot.
    pub(crate) fn charged(
        ctx: &'storage DecodeContext<'_>,
        length: usize,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let parents = storage.with_storage(|| ctx.collect_indexed_vec(length, operation, Ok))?;
        Ok(Self {
            parents: RefCell::new(parents),
            storage: Some(storage),
        })
    }
}

impl UnionFind<'_> {
    /// Creates `length` singleton sets, one per node `0..length`.
    #[cfg(test)]
    pub(crate) fn new(length: usize) -> Self {
        Self {
            parents: RefCell::new((0..length).collect()),
            storage: None,
        }
    }

    /// Copies the forest into its own scoped storage.
    pub(crate) fn clone_charged<'storage>(
        &self,
        ctx: &'storage DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<UnionFind<'storage>, CodecError> {
        let (parents, storage) = ctx.copy_temporary_slice(&self.parents.borrow(), operation)?;
        Ok(UnionFind {
            parents: RefCell::new(parents),
            storage: Some(storage),
        })
    }

    /// Returns the number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.parents.borrow().len()
    }

    /// Appends a new singleton node and returns its index.
    #[cfg(test)]
    pub(crate) fn push(&mut self) -> usize {
        let index = self.parents.borrow().len();
        self.parents.get_mut().push(index);
        index
    }

    /// Appends a new singleton node in the forest's storage and returns its index.
    pub(crate) fn push_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        let index = self.parents.borrow().len();
        match &mut self.storage {
            Some(storage) => {
                ctx.push_scoped_vec(storage, self.parents.get_mut(), index, operation)?
            }
            None => ctx.push_vec(self.parents.get_mut(), index, operation)?,
        }
        Ok(index)
    }

    /// Returns the semantic representative and compresses its parent path.
    /// Compression changes no class membership or selected representative.
    pub(crate) fn find(&self, ctx: &DecodeContext<'_>, node: usize) -> Result<usize, CodecError> {
        let mut parents = self.parents.borrow_mut();
        let mut root = node;
        let mut last_child = node;
        loop {
            ctx.charge_work(1, "catia_union_traversal")?;
            let parent = *parents
                .get(root)
                .ok_or_else(|| CodecError::malformed("union node is outside its forest"))?;
            if parent == root {
                break;
            }
            last_child = root;
            root = parent;
        }
        let mut current = node;
        while current != last_child {
            ctx.charge_work(1, "catia_union_compression")?;
            let parent = parents[current];
            parents[current] = root;
            current = parent;
        }
        Ok(root)
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
            *self
                .parents
                .get_mut()
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
            parents: RefCell::new(self.parents.borrow().clone()),
            storage: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn union_parent_initialization_admits_each_slot_once() {
        use cadmpeg_core::CodecError;

        // Each parent slot is written once with its own index.
        crate::test_support::with_work_limit(2, |ctx| {
            let admitted = UnionFind::charged(ctx, 2, "catia_union_initialization_test")
                .expect("the exact initialization work fits");
            assert_eq!(*admitted.parents.borrow(), [0, 1]);
        });
        crate::test_support::with_work_limit(1, |ctx| {
            let Err(CodecError::ResourceLimit(limit)) =
                UnionFind::charged(ctx, 2, "catia_union_initialization_test")
            else {
                panic!("parent traversal must refuse")
            };
            assert_eq!(limit.operation, "catia_union_initialization_test");
            assert_eq!(limit.used, 1);
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn union_parents_and_nodes_are_scoped_scratch() {
        let parent_bytes = u64::try_from(std::mem::size_of::<usize>()).expect("parent bytes");
        crate::test_support::with_retained_limit(0, |ctx| {
            let mut union =
                UnionFind::charged(ctx, 2, "catia_union_scratch").expect("scoped parents");
            assert_eq!(
                union
                    .push_charged(ctx, "catia_union_scratch")
                    .expect("scoped node"),
                2
            );
        });
        crate::test_support::with_materialized_limit(2 * parent_bytes, |ctx| {
            for _ in 0..64 {
                drop(UnionFind::charged(ctx, 2, "catia_union_scratch").expect("released parents"));
            }
        });
    }

    #[test]
    fn charged_union_parents_refuse_below_node_count() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let union = UnionFind::charged(&ctx, 2, "catia_union_test_parents")
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
            crate::test_support::with_service_context(|ctx| union.find(ctx, 0))
                .expect("service forest traversal"),
            LAST
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| union.find(ctx, 0))
                .expect("service forest traversal"),
            LAST
        );
        assert!(union.parents.borrow().iter().all(|parent| *parent == LAST));
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
            crate::test_support::with_service_context(|ctx| union.find(ctx, separate))
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
        assert_eq!(union.parents.borrow()[0], 1);
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
                empty.find(ctx, 0),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert!(matches!(
                empty.union(ctx, 0, 0),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            let small = UnionFind::new(1);
            let large = UnionFind::new(2);
            let foreign = large.find(ctx, 1).expect("large node");
            assert!(matches!(
                small.find(ctx, foreign),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert_eq!(small.find(ctx, 0).expect("own node"), 0);
        });
    }
    #[test]
    fn flat_representative_lookups_do_not_rewrite_parents() {
        let mut union = UnionFind::new(2);
        crate::test_support::with_service_context(|ctx| {
            union.union(ctx, 0, 1).expect("merge");
        });
        // Each lookup reads the child and representative; neither parent changes.
        crate::test_support::with_work_limit(4, |ctx| {
            assert_eq!(union.find(ctx, 1).expect("first lookup"), 0);
            assert_eq!(union.find(ctx, 1).expect("second lookup"), 0);
            assert!(ctx.resource_refusal().is_none());
        });
    }
    #[test]
    fn readonly_root_scans_compress_an_adversarial_chain() {
        const NODES: usize = 1_024;
        let mut union = UnionFind::new(NODES);
        crate::test_support::with_service_context(|ctx| {
            for node in 1..NODES {
                union.union(ctx, node, node - 1).expect("merge");
            }
        });
        // First scan: N reads and N-2 writes, then 2(N-2)+1 reads.
        // Second scan: 2(N-1)+1 reads. Total: 6N-6.
        crate::test_support::with_work_limit(
            cadmpeg_core::decode::u64_from_index(6 * NODES - 6),
            |ctx| {
                for _ in 0..2 {
                    for node in 0..NODES {
                        assert_eq!(union.find(ctx, node).expect("linear scan"), NODES - 1);
                    }
                }
            },
        );
    }
}
