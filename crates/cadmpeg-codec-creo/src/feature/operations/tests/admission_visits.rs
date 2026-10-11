// SPDX-License-Identifier: Apache-2.0

use super::super::{inline_recipe_resolution, FeatureRecipe, RecipeState};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn inline_recipe_windows_charge_present_comparisons_and_stop_at_conflict() {
    for length in 0..=14usize {
        let record = vec![0xff; length];
        // The four NUL-terminated names have lengths 12, 11, 12, 11.
        let total = 2 * record.windows(12).len() + 2 * record.windows(11).len();
        check_windows(
            &record,
            u64::try_from(total).expect("fixture value fits u64"),
            RecipeState::None,
        );
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
                    Ok(())
                }
                Err(CodecError::ResourceLimit(original)) => {
                    assert_eq!(ctx.resource_refusal(), Some(original));
                    assert_eq!(original.additional, 1);
                    assert_eq!(original.operation, "creo inline recipe scan");
                    Err(CodecError::ResourceLimit(original))
                }
                Err(error) => Err(error),
            }
        },
    );
}
