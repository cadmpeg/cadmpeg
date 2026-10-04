use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

#[test]
fn lane_plane_frame_deduplication_preserves_one_frame_and_refusal() {
    let native = |id: &str| crate::records::Feature {
        id: id.into(), parent: "history".into(), xml_tag: "Feature".into(),
        tree_parent: None, source_id: crate::records::FeatureSource::from_value(3),
        ordinal: 0, name: "Top".into(), kind: "Plane".into(), input_class: None,
        suppressed: false, parameters: BTreeMap::new(), dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(), text: None, content: Vec::new(),
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(), part_name: None, properties: BTreeMap::new(),
        content: Vec::new(), configurations: Vec::new(),
        features: vec![native("native-a"), native("native-b")],
    };
    let neutral = |id: &str, native_id: &str| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).unwrap(), ordinal: 0, name: Some("Top".into()), suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(), source_tag: None, source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane: PrincipalPlane::Top })),
        native_ref: Some(native_id.into()),
    };
    let features = [neutral("synthetic:test:id#plane-a", "native-a"),
        neutral("synthetic:test:id#plane-b", "native-b")];
    let lane = crate::records::FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: Vec::new(), classes: Vec::new(),
        names: vec![crate::records::FeatureInputName {
            id: "name".into(), parent: "lane".into(), ordinal: 0, offset: 0,
            object_id: crate::records::ObjectId::from_value(3), value: "Top".into(),
        }],
        scalars: Vec::new(), relation_bindings: Vec::new(), relation_instances: Vec::new(),
        body_selections: Vec::new(), edge_selections: Vec::new(), surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(), references: Vec::new(), sketch_entities: Vec::new(),
    };
    let histories = [history];
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::lane_sketch_plane_frames(ctx, &features, &histories, &lane)
    };
    let result = solve(&cadmpeg_test_support::service_decode_context()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(&3).unwrap().as_tuple(), (
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)));
    crate::test_support::work_refusal_at("deduplicate SLDPRT sketch plane frames", solve);
}

#[test]
fn sketch_plane_frame_cost_counts_fields_and_provenance() {
    use cadmpeg_core::decode::cost::DecodeCost;
    let frame = super::super::SketchPlaneFrame::native((
        Point3::new(1.0, 2.0, 3.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)));
    let ctx = cadmpeg_test_support::service_decode_context();
    // The cost counts nine f64 coordinates and the one-byte axis-source tag.
    assert_eq!(frame.decode_cost(&ctx, "sketch plane frame cost").unwrap(), 73);
}
