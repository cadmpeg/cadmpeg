// SPDX-License-Identifier: Apache-2.0
use crate::entities::{admit_with_scoped_loss_slots, non_resource_error};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

#[test]
fn entity_scalar_admission_preserves_original_refusal() {
    let entry = crate::test_support::directory_target(1, 104);
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty caller input");
        let mut slots = ctx.reserve_scoped(0, "test entity loss slots").expect("empty loss owner before refusal");
        let mut losses = Vec::new();
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original entity refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original entity refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original entity refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original entity refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original entity refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original entity refusal").map(|_| ()),
                _ => panic!("entity refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 {
            let result = admit_with_scoped_loss_slots(Ok(42_u32), &entry, &mut slots, &mut losses, &ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(Some(42)))),
            }
            assert!(losses.is_empty());
        }
        drop(losses);
        drop(slots);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh scalar admission finishes"),
        }
    }
}

#[test]
fn entity_foreign_resource_error_preserves_original_refusal() {
    let foreign = ResourceLimit::allocation_failed(
        ResourceDimension::Entities, 17, 19, "test foreign entity refusal",
    );
    crate::test_support::with_entry_context(|ctx, original| {
        let expected = original.unwrap_or(foreign);
        let result = non_resource_error(CodecError::ResourceLimit(foreign), ctx);
        assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == expected));
    });
}
