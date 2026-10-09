// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
enum Route { Positive, Negative, Type, NegativeType, AnyOf, Any }

fn exercise(route: Route) {
    for populated in [false, true] {
        let directory = if populated { vec![directory_target(1, 116)] } else { Vec::new() };
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
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            // The real empty resolver exists before the caller refuses. No synthetic
            // context or forged reservation replaces its original session.
            let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
            let original = dimension.map(|dimension| {
                let result = match dimension {
                    ResourceDimension::WorkUnits => ctx.charge_work(1, "test original reference refusal"),
                    ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original reference refusal"),
                    ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original reference refusal").map(|_| ()),
                    ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original reference refusal"),
                    ResourceDimension::Entities => ctx.charge_entities(1, "test original reference refusal"),
                    ResourceDimension::RecursionDepth => ctx.enter_nested("test original reference refusal").map(|_| ()),
                    _ => panic!("reference refusal dimension"),
                };
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("original refusal"); };
                assert_eq!((first.dimension, first.limit, first.used, first.additional), (dimension, 0, 0, 1));
                first
            });
            for _ in 0..64 {
                let result = match route {
                    Route::Positive => resolver.resolve(1, usize::MAX, 0,
                        ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry), |_| panic!("null predicate")),
                    Route::Negative => resolver.resolve_negative(1, usize::MAX, 0,
                        ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry), |_| panic!("null predicate")),
                    Route::Type => resolver.resolve_type(1, usize::MAX, 0, 116, &[0, 1]),
                    Route::NegativeType => resolver.resolve_negative_type(1, usize::MAX, 0, 116, &[0, 1]),
                    Route::AnyOf => resolver.resolve_any_of(1, usize::MAX, 0, (116, 212, &[310, 312]), |_| panic!("null predicate")),
                    Route::Any => resolver.resolve_any(1, usize::MAX, 0),
                };
                match original {
                    Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                    None => assert_eq!(result.unwrap(), None),
                }
                assert!(resolver.edges.borrow().is_empty());
                if let Some(first) = original {
                    // Non-null invalid and valid targets also stop before predicates
                    // or reference-edge creation once the caller has refused.
                    for pointer in [i64::MIN, -1, 1, 2, i64::MAX] {
                        let result = resolver.resolve(1, 0, pointer,
                            ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry), |_| panic!("refused predicate"));
                        assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
                    }
                    assert!(resolver.edges.borrow().is_empty());
                }
            }
            drop(resolver);
            match original {
                Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
                None => ctx.finish_session().unwrap(),
            }
        }
    }
}

#[test]
fn positive_null_reference_preserves_original_refusal() { exercise(Route::Positive); }
#[test]
fn negative_null_reference_preserves_original_refusal() { exercise(Route::Negative); }
#[test]
fn typed_null_reference_preserves_original_refusal() { exercise(Route::Type); }
#[test]
fn negative_typed_null_reference_preserves_original_refusal() { exercise(Route::NegativeType); }
#[test]
fn any_of_null_reference_preserves_original_refusal() { exercise(Route::AnyOf); }
#[test]
fn any_null_reference_preserves_original_refusal() { exercise(Route::Any); }

#[test]
fn fixed_native_reference_copy_preserves_original_refusal() {
    for expected in [
        ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
        ReferenceExpectation::Type { entity_type: 116, forms: Vec::new() },
        ReferenceExpectation::AnyOf { first: 116, second: 212, rest: Vec::new() },
    ] {
        let edge = ReferenceEdge { origin: ReferenceOrigin::Parameter { index: 2 },
            raw_pointer: 1, resolution: Resolution::Resolved(1), expected };
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
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test actual native edge storage").unwrap();
            let original = dimension.map(|dimension| {
                let result = match dimension {
                    ResourceDimension::WorkUnits => ctx.charge_work(1, "test original native edge refusal"),
                    ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original native edge refusal"),
                    ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original native edge refusal").map(|_| ()),
                    ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original native edge refusal"),
                    ResourceDimension::Entities => ctx.charge_entities(1, "test original native edge refusal"),
                    ResourceDimension::RecursionDepth => ctx.enter_nested("test original native edge refusal").map(|_| ()),
                    _ => panic!("native edge refusal dimension"),
                };
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("original refusal"); };
                assert_eq!((first.dimension, first.limit, first.used, first.additional), (dimension, 0, 0, 1));
                first
            });
            for _ in 0..64 {
                let result = edge.copy_for_native(&ctx, &mut storage);
                match original {
                    Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                    None => assert_eq!(result.unwrap(), edge),
                }
            }
            drop(storage);
            match original {
                Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
                None => ctx.finish_session().unwrap(),
            }
        }
    }
}
