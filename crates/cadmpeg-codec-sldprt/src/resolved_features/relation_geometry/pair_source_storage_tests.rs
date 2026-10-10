//! Consumed child and indexed pair shells expire before the next query phase.

use super::{declared_entity_handle_pairs, DIMENSIONED_HANDLE_OPERATION};
use crate::records::{FeatureInputClass, FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

// Three references fit the core's first four-slot vector. The returned pair
// vector also starts with four slots. At indexed construction the destination
// pair shell, indexed reference roster and new indexed pair shell coexist.
const ROSTER_BYTES: u64 = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&SketchInputEntity>());
const PAIR_BYTES: u64 = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<[&SketchInputEntity; 2]>());
const QUERY_BYTES: u64 = PAIR_BYTES + ROSTER_BYTES + PAIR_BYTES;

fn fixture() -> FeatureInputLane {
    let marker = |id: &str, offset, kind, object_index, local_id| {
        let mut value =
            SketchInputEntity::new(id, "lane", u32::try_from(offset).unwrap(), offset, kind);
        value.feature_ref = Some("feature".into());
        value.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
        value.with_test_identity(Some(object_index), Some(local_id))
    };
    let mut lane = super::ownership_tests::relation_lane();
    lane.relation_instances.clear();
    lane.sketch_entities = vec![
        marker("arc", 0, SketchInputKind::Arc, 0, 1),
        marker("child", 10, SketchInputKind::Point, 1, 2),
        marker("indexed", 30, SketchInputKind::Point, 2, 3),
    ];
    lane.classes = vec![FeatureInputClass {
        id: "arc-class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 20,
        name: "sgArcHandle".into(),
    }];
    lane
}

#[test]
fn relation_pair_sources_reuse_storage_and_preserve_ordered_borrowed_pairs() {
    let lane = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        let storage;
        let pairs;
        (pairs, storage) = ctx
            .with_scoped_storage("relation pair query output", || {
                declared_entity_handle_pairs(&ctx, &lane, "feature")
            })
            .unwrap();
        assert_eq!(pairs.len(), 2);
        for (pair, indices) in pairs.iter().zip([[0, 1], [1, 2]]) {
            assert!(std::ptr::eq(pair[0], &raw const lane.sketch_entities[indices[0]]));
            assert!(std::ptr::eq(pair[1], &raw const lane.sketch_entities[indices[1]]));
        }
        // Only the actual result shell remains. Consumed input shells and
        // construction rosters must have released their reservations.
        ctx.reserve_scoped(
            QUERY_BYTES - PAIR_BYTES,
            "reuse consumed relation pair sources",
        )
        .unwrap();
        drop(pairs);
        drop(storage);
        ctx.reserve_scoped(QUERY_BYTES, "reuse complete relation pair query")
            .unwrap();
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn relation_pair_sources_preserve_feature_filter_without_storage() {
    let lane = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        let storage;
        let pairs;
        (pairs, storage) = ctx
            .with_scoped_storage("filtered relation pair query", || {
                declared_entity_handle_pairs(&ctx, &lane, "other-feature")
            })
            .unwrap();
        assert!(pairs.is_empty());
        assert_eq!(pairs.capacity(), 0);
        drop(pairs);
        drop(storage);
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn relation_pair_sources_preserve_original_refusal() {
    let lane = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let run = || {
        ctx.with_scoped_storage("relation pair query output", || {
            declared_entity_handle_pairs(&ctx, &lane, "feature")
        })
    };
    let Err(error) = run() else { panic!("expected indexed pair source storage refusal") };
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected original resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, DIMENSIONED_HANDLE_OPERATION);
    assert_eq!(limit.used, PAIR_BYTES + ROSTER_BYTES);
    assert_eq!(limit.additional, PAIR_BYTES);
    assert_eq!(limit.limit, QUERY_BYTES - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let Err(repeated) = run() else { panic!("expected sticky pair source refusal") };
    let CodecError::ResourceLimit(repeated) = repeated else {
        panic!("expected original sticky resource refusal");
    };
    assert_eq!(repeated, limit);
}
