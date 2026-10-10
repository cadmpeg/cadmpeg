// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn nonnull_references_propagate_each_resource_refusal() {
    for populated in [false, true] {
        let directory = if populated { vec![directory_target(1, 116)] } else { Vec::new() };
        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes,
            ResourceDimension::Entities, ResourceDimension::RecursionDepth] {
            let operation = "test original reference refusal";
            let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                policy.limits.max_collection_items = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_entities = 0;
                policy.limits.max_recursion_depth = 0;
                match dimension {
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    ResourceDimension::Entities => policy.limits.max_entities = cap,
                    ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
                    _ => panic!("reference refusal dimension"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let resolver = ParameterResolver::new(&directory, &ctx)?;
                let refused = match dimension {
                    ResourceDimension::WorkUnits => ctx.charge_work(1, operation),
                    ResourceDimension::CollectionItems => ctx.charge_collection_items(1, operation),
                    ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, operation).map(|_| ()),
                    ResourceDimension::RetainedBytes => ctx.charge_retained(1, operation),
                    ResourceDimension::Entities => ctx.charge_entities(1, operation),
                    ResourceDimension::RecursionDepth => ctx.enter_nested(operation).map(|_| ()),
                    _ => panic!("reference refusal dimension"),
                };
                let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
                for _ in 0..64 {
                    for pointer in [i64::MIN, -1, 1, 2, i64::MAX] {
                        let result = resolver.resolve(1, 0, pointer,
                            ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
                            |_| panic!("refused predicate"));
                        assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
                    }
                    assert!(resolver.edges.borrow().is_empty());
                }
                drop(resolver);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
                Err::<(), CodecError>(CodecError::ResourceLimit(first))
            });
            let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal"); };
            assert_eq!((limit.dimension, limit.operation, limit.used, limit.additional), (dimension, operation, 0, 1));
        }
    }
}
