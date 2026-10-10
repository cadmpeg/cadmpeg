// SPDX-License-Identifier: Apache-2.0

use super::super::{
    inline_recipe_resolution, operation_states, operations, reference_names, FeatureRecipe,
    RecipeState,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn inline_recipe_windows_charge_present_comparisons_and_stop_at_conflict() {
    for length in 0..=14usize {
        let record = vec![0xff; length];
        // The four NUL-terminated names have lengths 12, 11, 12, 11.
        let total = 2 * record.windows(12).len() + 2 * record.windows(11).len();
        check_windows(&record, total as u64, RecipeState::None);
    }
    // A 12-byte name gives window counts 1, 2, 1, 2 in stored family order.
    check_windows(
        b"protextrude\0",
        6,
        RecipeState::Resolved(FeatureRecipe::ProtrudeExtrude),
    );
    // The first family sees its second occurrence at offset12 and stops
    // immediately: 13 comparisons, with no remaining family traversal.
    check_windows(
        b"protextrude\0protextrude\0",
        13,
        RecipeState::Conflicting { candidate: None },
    );
}

fn check_windows(record: &[u8], total: u64, expected: RecipeState) {
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &vec!["creo inline recipe scan"; usize::try_from(total).expect("fixture visits")],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            match inline_recipe_resolution(&ctx, record) {
                Ok(value) => {
                    assert_eq!(ctx.resource_refusal(), None);
                    assert_eq!(value, expected);
                    let refusal = ctx
                        .charge_work_limit(1, "measure inline recipe windows")
                        .expect_err("measurement");
                    assert_eq!((refusal.used, refusal.additional), (total, 1));
                    let result = inline_recipe_resolution(&ctx, record);
                    assert!(matches!(result, Err(CodecError::ResourceLimit(actual))
                        if actual == refusal));
                    Ok(())
                }
                Err(CodecError::ResourceLimit(original)) => {
                    assert_eq!(ctx.resource_refusal(), Some(original));
                    assert_eq!(original.additional, 1);
                    assert_eq!(original.operation, "creo inline recipe scan");
                    let result = inline_recipe_resolution(&ctx, record);
                    assert!(matches!(result, Err(CodecError::ResourceLimit(actual))
                        if actual == original));
                    Err(CodecError::ResourceLimit(original))
                }
                Err(error) => Err(error),
            }
        },
    );
}

#[test]
fn operation_fixed_returns_are_free_and_preserve_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let check = |refused| {
        let mut results = Vec::new();
        for payload in [&[][..], &[0xf8][..], &[0xf8, 0xf8][..]] {
            results.push(reference_names(&ctx, payload).map(|names| names.is_empty()));
        }
        for record in [&[][..], &[0xff][..], &[0xff; 10][..]] {
            results.push(
                inline_recipe_resolution(&ctx, record).map(|recipe| recipe == RecipeState::None),
            );
        }
        results.push(operation_states(&ctx, &[]).map(|states| states.is_empty()));
        results.push(operations(&ctx, &[]).map(|operations| operations.is_empty()));
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(result.expect("free fixed route"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after fixed operation returns")
        .expect_err("zero cap");
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn derived_operation_materialization_is_free_and_preserves_original_refusal() {
    use super::super::{OperationKind, OperationName, ParsedKind, ParsedOperation};

    for (kind, expected) in [
        (ParsedKind::Native, OperationKind::Native),
        (
            ParsedKind::Recipe(FeatureRecipe::ProtrudeExtrude),
            OperationKind::Extrude,
        ),
        (
            ParsedKind::Recipe(FeatureRecipe::CutExtrude),
            OperationKind::Extrude,
        ),
        (
            ParsedKind::Recipe(FeatureRecipe::ProtrudeRevolve),
            OperationKind::Revolve,
        ),
        (
            ParsedKind::Recipe(FeatureRecipe::CutRevolve),
            OperationKind::Revolve,
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || {
            ParsedOperation {
                feature_id: 7,
                kind,
                name: None,
                recipe: RecipeState::None,
                display_state_conflict: false,
                depdb: None,
                offset: 11,
                state_offset: 11,
            }
            .materialize(&ctx)
        };
        let operation = run().expect("fixed derived projection needs no resources");
        assert_eq!(operation.kind, expected);
        assert_eq!(operation.name, OperationName::Derived);
        assert_eq!(operation.feature_id, 7);
        assert_eq!(operation.recipe, RecipeState::None);
        assert!(!operation.display_state_conflict);
        assert_eq!(operation.depdb, None);
        assert_eq!((operation.offset, operation.state_offset), (11, 11));
        assert_eq!(ctx.resource_refusal(), None);
        let original = ctx
            .charge_work_limit(1, "after derived materialization")
            .expect_err("zero Work cap");
        assert_eq!(
            (original.dimension, original.used, original.additional),
            (ResourceDimension::WorkUnits, 0, 1)
        );
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
