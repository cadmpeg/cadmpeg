// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::products::{Occurrence, OccurrenceParent, PrototypeReference};
use crate::transform::Transform;

fn occurrence(id: &str, parent: Option<&str>) -> Occurrence {
    Occurrence {
        id: id.try_into().unwrap(), prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| OccurrenceParent::Occurrence { occurrence: parent.try_into().unwrap() }),
        ordinal: 0, transform: Transform::identity(), linked_prototype: None,
        scale: [crate::scalar::FiniteReal::ONE; 3], name: None, visible: None, link: None, native_ref: None,
    }
}

#[test]
fn assembly_validation_preserves_index_work_and_depth_refusals() {
    let rows = [occurrence("test:model:occurrence#one", None)];
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = super::validate(&ctx, &rows) else { panic!("assembly must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn assembly_validation_uses_active_session_depth_in_parent_chains() {
    let rows = [occurrence("test:model:occurrence#child", Some("test:model:occurrence#root")),
        occurrence("test:model:occurrence#root", None)];
    for (limit, caller) in [(1, false), (2, true)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let guard = caller.then(|| ctx.enter_nested("active assembly caller").unwrap());
        let Err(CodecError::ResourceLimit(resource)) = super::validate(&ctx, &rows) else { panic!("parent frame must refuse"); };
        assert_eq!(resource.dimension, ResourceDimension::RecursionDepth);
        drop(guard);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == resource));
    }
}

#[test]
fn assembly_validation_borrows_semantic_errors_and_releases_scopes() {
    let one = occurrence("test:model:occurrence#one", None);
    let inputs = [vec![one.clone(), one],
        vec![occurrence("test:model:occurrence#missing", Some("test:model:occurrence#absent"))],
        vec![occurrence("test:model:occurrence#cycle", Some("test:model:occurrence#cycle"))]];
    for rows in inputs {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 16384;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(!super::validate(&ctx, &rows).unwrap());
        drop(ctx.reserve_scoped(16384, "assembly error scope released").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn assembly_validation_releases_successful_indexes_without_retained_copies() {
    let rows = [occurrence("test:model:occurrence#child", Some("test:model:occurrence#root")),
        occurrence("test:model:occurrence#root", None)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16384;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::validate(&ctx, &rows).unwrap());
    drop(ctx.reserve_scoped(16384, "assembly success scope released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn assembly_identity_collision_compares_full_identity_under_caller_budget() {
    let row = occurrence("test:model:occurrence#stored", None);
    let query = "test:model:occurrence#missing".try_into().unwrap();
    let facts = super::Facts { values: vec![(crate::index::identity_hash("test:model:occurrence#missing"), &row, super::Resolution::Pending)] };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index("test:model:occurrence#missing".len()) + 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(limit) = facts.position(&query, &crate::index::DecodeStorage(&ctx)) else { panic!("collision comparison must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "assembly identity comparison");
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
