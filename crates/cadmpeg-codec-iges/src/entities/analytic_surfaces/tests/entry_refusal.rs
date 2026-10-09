// SPDX-License-Identifier: Apache-2.0
use crate::entities::analytic_surfaces::{admit_analytic, form_reference_direction,
    required_direction, AnalyticDirectionError};
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

fn empty_record() -> ParameterRecord {
    ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new())
}

#[test]
fn analytic_missing_required_pointer_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = required_direction(&record, 1, "axis", &[], &[], ctx);
        match original {
            Some(first) => {
                let Err(error) = result else { panic!("original refusal"); };
                assert!(matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            None => assert!(matches!(result, Err(AnalyticDirectionError::MissingPointer("axis")))),
        }
    });
}

#[test]
fn analytic_optional_reference_direction_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = form_reference_direction(0, &record, 1, "axis", Transform::identity(), (&[], &[]), ctx);
        match original {
            Some(first) => {
                let Err(error) = result else { panic!("original refusal"); };
                assert!(matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            None => assert!(matches!(result, Ok(None))),
        }
    });
}

#[test]
fn analytic_fixed_success_preserves_original_refusal() {
    let entry = crate::test_support::directory_target(1, 190);
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
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut slots = ctx.reserve_scoped(0, "analytic test slots").expect("empty owner");
        let mut losses = Vec::new();
        let original = dimension.map(|dimension| {
            let result = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "analytic original refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "analytic original refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "analytic original refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "analytic original refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "analytic original refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("analytic original refusal").map(|_| ()),
                _ => panic!("test dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 {
            let result = admit_analytic(Ok(42_u32), &entry, &mut losses, &mut slots, &ctx);
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
            None => ctx.finish_session().expect("fixed admission finishes"),
        }
    }
}
