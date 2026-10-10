//! Detached spatial binding releases its temporary relation range table.

use super::super::bind_detached_spatial_relation_objects;
use crate::records::{FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashSet;

// Each class-reference Vec grows to four slots. Both remain live when the
// output range Vec grows to four slots; the singleton stable sorts need no scratch.
const CLASS_BYTES: u64 =
    8 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&FeatureInputClass>());
const RANGE_BYTES: u64 =
    4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(u64, u64)>());
const PEAK_BYTES: u64 = CLASS_BYTES + RANGE_BYTES;

fn lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "sldprt:feature-input:config-objects#spatial-ranges".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: [
            (10, "moRelMgr_c"),
            (20, "sg3DPlaneHandle"),
            (40, "suObList"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (offset, name))| FeatureInputClass {
            id: format!("class-{index}"),
            parent: "sldprt:feature-input:config-objects#spatial-ranges".into(),
            ordinal: u32::try_from(index).unwrap(),
            offset,
            name: name.into(),
        })
        .collect(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

#[test]
fn detached_spatial_range_workspace_releases_unbound_ranges() {
    let mut lane = lane();
    let represented = HashSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = PEAK_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        assert!(
            bind_detached_spatial_relation_objects(&ctx, &[], &represented, &mut lane)
                .unwrap()
                .is_empty()
        );
        ctx.reserve_scoped(PEAK_BYTES, "reuse detached spatial range workspace")
            .unwrap();
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn detached_spatial_range_workspace_preserves_original_refusal() {
    let mut lane = lane();
    let represented = HashSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = PEAK_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) =
        bind_detached_spatial_relation_objects(&ctx, &[], &represented, &mut lane).unwrap_err()
    else {
        panic!("expected detached spatial range refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT spatial relation ranges");
    assert_eq!(limit.used, CLASS_BYTES);
    assert_eq!(limit.additional, RANGE_BYTES);
    assert_eq!(limit.limit, PEAK_BYTES - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) =
        bind_detached_spatial_relation_objects(&ctx, &[], &represented, &mut lane).unwrap_err()
    else {
        panic!("expected sticky detached spatial range refusal");
    };
    assert_eq!(repeated, limit);
}

#[test]
fn spatial_plane_class_comparison_has_its_own_work_boundary() {
    const OPERATION: &str = "compare SLDPRT spatial plane class name";
    let lane = lane();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::super::spatial_relation_manager_ranges_charged(&ctx, &lane).unwrap(),
        [(10, 40)]
    );
    ctx.finish_session().unwrap();
    let error = crate::test_support::work_refusal_at(OPERATION, |ctx| {
        super::super::spatial_relation_manager_ranges_charged(ctx, &lane)
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == OPERATION && limit.additional > 1));
}
