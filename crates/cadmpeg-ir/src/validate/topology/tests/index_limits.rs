// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn parameter_fixture() -> crate::CadIr {
    let mut ir = crate::CadIr::empty();
    ir.model.parameters.push(serde_json::from_value(serde_json::json!({
        "id": "test:model:parameter#value", "name": "Length", "expression": "10"
    })).unwrap());
    ir
}

#[test]
fn topology_reference_indexes_preserve_scoped_slot_and_work_refusals() {
    let ir = parameter_fixture();
    let index = crate::index::ModelIndex::new(&ir, crate::index::StandardIndex);
    for feature_only in [false, true] {
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let result = if feature_only {
                super::super::check_feature_references(&ctx, &ir, &index, &mut findings)
            } else {
                super::super::check_references(&ctx, &ir, &index, &mut findings)
            };
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("reference index must refuse"); };
            assert_eq!(original.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
    }
}

#[test]
fn topology_reference_indexes_borrow_text_and_release_temporary_storage() {
    let ir = parameter_fixture();
    let index = crate::index::ModelIndex::new(&ir, crate::index::StandardIndex);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::super::check_references(&ctx, &ir, &index, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(65536, "topology reference scratch released").unwrap());
    ctx.finish_session().unwrap();
}
