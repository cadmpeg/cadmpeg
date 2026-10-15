// SPDX-License-Identifier: Apache-2.0
use crate::design::geometry::endpoint_root;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn endpoint_root_compresses_a_deep_chain_without_recursion() {
    let root = 100_000;
    let mut parents: Vec<_> = (1..=root).chain(std::iter::once(root)).collect();
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(endpoint_root(ctx, &mut parents, 0).unwrap(), root);
    });
    assert!(parents.iter().all(|parent| *parent == root));
}

#[test]
fn endpoint_walks_refuse_before_mutating_the_first_link() {
    let original = [1, 2, 3, 3];
    for operation in [
        "f3d sketch endpoint root walk",
        "f3d sketch endpoint path compression",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                let mut parents = original;
                let result = endpoint_root(ctx, &mut parents, 0);
                if result.is_err() {
                    assert_eq!(parents, original);
                }
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}
