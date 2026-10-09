use super::super::{LaneFrameIndex, SketchPlaneFrame};
use crate::records::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane};
use std::collections::BTreeMap;

fn fixture(second: PrincipalPlane, claimed: bool) -> (Vec<cadmpeg_ir::features::Feature>, [FeatureHistory; 1], FeatureInputLane) {
    let native = |id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: if claimed { FeatureSource::from_value(3) } else { None },
        ordinal: 0,
        name: "Top".into(),
        kind: "Plane".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let neutral = |id: &str, native_id: &str, plane| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: Some("Top".into()),
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }),
        ),
        native_ref: Some(native_id.into()),
    };
    let features = vec![
        neutral("synthetic:test:id#plane-a", "native-a", PrincipalPlane::Top),
        neutral("synthetic:test:id#plane-b", "native-b", second),
    ];
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("native-a"), native("native-b")],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: ObjectId::from_value(3),
            value: "Top".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (features, histories, lane)
}

fn one_frame_storage() -> u64 {
    // Core's first HashMap reservation has four buckets and sixteen trailing
    // control bytes, with max(alignment, 16)-1 bytes of padding.
    let bucket = std::mem::size_of::<(u32, SketchPlaneFrame)>();
    let padding = std::mem::align_of::<(u32, SketchPlaneFrame)>().max(16) - 1;
    u64::try_from(4 * bucket + 4 + 16 + padding).unwrap()
}

#[test]
fn duplicate_lane_plane_candidates_retain_only_the_selected_frame() {
    let (features, histories, lane) = fixture(PrincipalPlane::Top, false);
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let index = LaneFrameIndex::new(&source_ctx, &features, &histories).unwrap();
    assert!(index.entries.is_empty());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = one_frame_storage();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let names = crate::resolved_features::scalars::ObjectNames::new(&ctx, &lane).unwrap();
    let frames = index.lane_frames(&ctx, &histories, &names).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames.get(&3).unwrap().as_tuple(), crate::resolved_features::compact_reference_planes::principal_sketch_frame(PrincipalPlane::Top));
}

#[test]
fn ambiguous_lane_plane_candidates_retain_no_output() {
    let (features, histories, lane) = fixture(PrincipalPlane::Front, false);
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let index = LaneFrameIndex::new(&source_ctx, &features, &histories).unwrap();
    assert!(index.entries.is_empty());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let names = crate::resolved_features::scalars::ObjectNames::new(&ctx, &lane).unwrap();
    assert!(index.lane_frames(&ctx, &histories, &names).unwrap().is_empty());
}

#[test]
fn claimed_lane_plane_keeps_existing_frame_without_retaining_candidates() {
    let (features, histories, lane) = fixture(PrincipalPlane::Top, true);
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let index = LaneFrameIndex::new(&source_ctx, &features, &histories).unwrap();
    assert_eq!(index.entries.len(), 2);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = one_frame_storage();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let names = crate::resolved_features::scalars::ObjectNames::new(&ctx, &lane).unwrap();
    let frames = index.lane_frames(&ctx, &histories, &names).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames.get(&3).unwrap().as_tuple(), crate::resolved_features::compact_reference_planes::principal_sketch_frame(PrincipalPlane::Top));
}

#[test]
fn lane_plane_candidate_storage_refusal_preserves_original_fuse() {
    let (features, histories, lane) = fixture(PrincipalPlane::Top, false);
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let index = LaneFrameIndex::new(&source_ctx, &features, &histories).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let names = crate::resolved_features::scalars::ObjectNames::new(&ctx, &lane).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "index SLDPRT lane sketch plane candidates",
        None,
    );
    let error = index.lane_frames(&ctx, &histories, &names).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("candidate storage refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT lane sketch plane candidates");
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    drop(probe);
    let replay = index.lane_frames(&ctx, &histories, &names).unwrap_err();
    let CodecError::ResourceLimit(replayed) = replay else { panic!("sticky candidate storage refusal"); };
    assert_eq!(replayed, limit);
}
