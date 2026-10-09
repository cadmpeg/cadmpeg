// SPDX-License-Identifier: Apache-2.0

use super::super::{inline_recipe_resolution, operations, operation_states,
    reference_names, FeatureRecipe, RecipeState};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn inline_recipe_windows_charge_present_comparisons_and_stop_at_conflict() {
    for length in 0..=14usize {
        let record = vec![0xff; length];
        // The four NUL-terminated names have lengths 12, 11, 12, 11.
        let total = 2 * length.saturating_sub(11) + 2 * length.saturating_sub(10);
        check_windows(&record, total as u64, RecipeState::None);
    }
    // A 12-byte name gives window counts 1, 2, 1, 2 in stored family order.
    check_windows(b"protextrude\0", 6,
        RecipeState::Resolved(FeatureRecipe::ProtrudeExtrude));
    // The first family sees its second occurrence at offset12 and stops
    // immediately: 13 comparisons, with no remaining family traversal.
    check_windows(b"protextrude\0protextrude\0", 13,
        RecipeState::Conflicting { candidate: None });
}

fn check_windows(record: &[u8], total: u64, expected: RecipeState) {
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = inline_recipe_resolution(&ctx, record);
        if cap == total {
            assert_eq!(result.expect("exact present-window work"), expected);
            assert_eq!(ctx.resource_refusal(), None);
            let refusal = ctx.charge_work_limit(1, "after inline recipe windows").expect_err("exact cap");
            assert_eq!((refusal.dimension, refusal.used, refusal.additional),
                (ResourceDimension::WorkUnits, total, 1));
        } else {
            let original = ctx.resource_refusal().expect("present window refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, "creo inline recipe scan"));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(inline_recipe_resolution(&ctx, record),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
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
            results.push(inline_recipe_resolution(&ctx, record).map(|recipe| recipe == RecipeState::None));
        }
        results.push(operation_states(&ctx, &[]).map(|states| states.is_empty()));
        results.push(operations(&ctx, &[]).map(|operations| operations.is_empty()));
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("free fixed route"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed operation returns").expect_err("zero cap");
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
