// SPDX-License-Identifier: Apache-2.0

use super::super::{operations, unique_recipe_owner};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
const EXTRUDE: &[u8] = b"\xe3protextrude\0Extrude id 7\0";

#[test]
fn recipe_owner_uses_current_consensus_and_preserves_feature_identity() {
    for (payload, expected) in [
        (b"".as_slice(), None),
        (b"\xe3Round id 7\0".as_slice(), None),
        (EXTRUDE, Some(7)),
        (b"\xe3protextrude\0Extrude id 7\0\xe3protextrude\0Extrude id 7\0".as_slice(), Some(7)),
        (b"\xe3protextrude\0Extrude id 7\0\xe3cutextrude\0Extrude id 7\0".as_slice(), None),
        (b"\xe3protextrude\0Extrude id 7\0\xe3protextrude\0Extrude id 3\0".as_slice(), None),
        (b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0".as_slice(), Some(8053)),
        (b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0".as_slice(), None),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(unique_recipe_owner(&ctx, payload).expect("borrowed owner projection"), expected);
        let original = ctx.charge_retained_limit(1, "after recipe owner")
            .expect_err("owner selection retains no operation names or output vector");
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::RetainedBytes, 0, 1));
        assert!(matches!(unique_recipe_owner(&ctx, payload),
            Err(CodecError::ResourceLimit(actual)) if actual == original));

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let records = operations(&ctx, payload).expect("persistent projection remains supported");
        let mut resolved = records.iter().filter(|record| record.recipe.resolved().is_some());
        let first = resolved.next().map(|record| record.feature_id);
        let persistent_owner = if resolved.next().is_none() { first } else { None };
        assert_eq!(persistent_owner, expected);
    }
}

#[test]
fn recipe_owner_empty_route_is_free_and_keeps_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(unique_recipe_owner(&ctx, &[]).expect("no source or output"), None);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after empty recipe owner").expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    assert!(matches!(unique_recipe_owner(&ctx, &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn recipe_owner_grouping_storage_ends_before_the_ambient_parent() {
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "test recipe owner parent").expect("parent");
        let owner = if nested {
            parent.with_storage(|| unique_recipe_owner(&ctx, EXTRUDE))
        } else {
            unique_recipe_owner(&ctx, EXTRUDE)
        }.expect("temporary borrowed consensus storage");
        assert_eq!(owner, Some(7));
        let probe = ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "after recipe owner grouping")
            .expect("all temporary grouping storage ended while parent remains live");
        drop(probe);
        drop(parent);
        let original = ctx.charge_retained_limit(1, "after recipe owner grouping names")
            .expect_err("no retained operation output");
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::RetainedBytes, 0, 1));
        assert!(matches!(unique_recipe_owner(&ctx, &[]),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn consumed_parser_vector_ends_before_current_operation_callback() {
    use super::super::{visit_current_operations, ParsedKind, ParsedOperation, RecipeState};
    use std::mem::{align_of, size_of};

    let state_bytes = size_of::<ParsedOperation<'_, RecipeState>>();
    assert!((2..=1024).contains(&state_bytes), "four-slot minimum vector capacity");
    // One grouped state retains four vector slots. The one-key tree uses the
    // core node bound: 11 key/value lanes, 16 pointer-width metadata lanes,
    // and two alignment widths. The consumed parser vector is no longer live.
    let alignment = align_of::<u32>()
        .max(align_of::<Vec<ParsedOperation<'_, RecipeState>>>())
        .max(align_of::<usize>());
    let group_bytes = u64::try_from(4 * state_bytes
        + 11 * (size_of::<u32>() + size_of::<Vec<ParsedOperation<'_, RecipeState>>>())
        + 16 * size_of::<usize>() + 2 * alignment).expect("group storage bound");
    for nested in [false, true] {
        for overflow in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let parent_bytes = if nested { 37 } else { 0 };
            let mut parent = ctx.reserve_scoped(parent_bytes, "test live grouping parent")
                .expect("parent storage");
            let available = EMPTY_ROOT_MATERIALIZED_BYTES - group_bytes - parent_bytes;
            let mut visits = 0;
            let mut run = || visit_current_operations(&ctx, EXTRUDE, |operation| {
                visits += 1;
                assert_eq!(operation.feature_id, 7);
                assert!(matches!(operation.kind, ParsedKind::Stored("Extrude")));
                assert_eq!(operation.recipe.resolved(), Some(super::super::FeatureRecipe::ProtrudeExtrude));
                let probe = ctx.reserve_scoped(available + u64::from(overflow),
                    "current operation callback storage")?;
                drop(probe);
                Ok(())
            });
            let result = if nested { parent.with_storage(run) } else { run() };
            assert_eq!(visits, 1);
            drop(parent);
            if overflow {
                let original = ctx.resource_refusal().expect("one byte beyond live storage");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::MaterializedBytes, group_bytes + parent_bytes,
                        available + 1, "current operation callback storage"));
                assert!(matches!(unique_recipe_owner(&ctx, &[]),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                result.expect("callback uses the remaining materialized allowance");
                let probe = ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES,
                    "after grouping callback storage").expect("all scoped storage ended");
                drop(probe);
                let original = ctx.charge_retained_limit(1, "after borrowed grouping callback")
                    .expect_err("no retained parser or projection storage");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::RetainedBytes, 0, 1));
            }
        }
    }
}
