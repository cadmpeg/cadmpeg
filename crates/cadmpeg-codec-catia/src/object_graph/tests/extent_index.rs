// SPDX-License-Identifier: Apache-2.0
use crate::object_graph::{extent_contains, ExtentIndex};

#[test]
fn extent_prefix_index_matches_strict_containment_and_overlap_predicates() {
    let owners = [(7, 4), (0, 0), (8, 1), (0, 5), (3, 2), (usize::MAX - 2, 4)];
    crate::test_support::with_service_context(|ctx| {
        let (index, _storage) =
            ctx.with_scoped_storage("catia_test_extent_index", || ExtentIndex::new(ctx, owners))?;
        for start in 0..16 {
            for length in 0..8 {
                assert_eq!(
                    index.contains(ctx, start, length)?,
                    owners
                        .iter()
                        .any(|(owner, len)| extent_contains(*owner, *len, start, length))
                );
                assert_eq!(
                    index.overlaps(ctx, start, length)?,
                    owners
                        .iter()
                        .any(|(owner, len)| crate::checked::extents_overlap(
                            start, length, *owner, *len
                        ))
                );
            }
        }
        assert!(!index.contains(ctx, usize::MAX - 1, 3)?);
        assert!(!index.overlaps(ctx, usize::MAX - 1, 3)?);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("interval predicates");
}

#[test]
fn extent_prefix_index_admits_many_disjoint_queries() {
    crate::test_support::with_work_limit(2_000_000, |ctx| {
        let (index, _storage) = ctx.with_scoped_storage("catia_test_extent_index", || {
            ExtentIndex::new(ctx, (0..4096).map(|item| (item * 4, 2)))
        })?;
        for item in 0..4096 {
            assert!(!index.contains(ctx, item * 4 + 3, 1)?);
            assert!(!index.overlaps(ctx, item * 4 + 3, 1)?);
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("binary interval queries fit the allowance");
}
