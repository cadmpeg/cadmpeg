//! Compact pattern face seed projection tests.

use super::super::project_compact_surface_selections;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind, FeatureSource,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternSeed},
    BodySelection, FaceSelection, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureId, FeatureOperation,
};
use std::collections::BTreeMap;

fn pattern_fixture() -> (Vec<Feature>, FeatureHistory, FeatureInputLane) {
    let neutral = |id: &str, native_ref: &str, definition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let features = vec![
        neutral(
            "synthetic:test:id#producer",
            "producer-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        neutral(
            "synthetic:test:id#pattern",
            "pattern-native",
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            }),
        ),
    ];
    let native = |id: &str, source: &str| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source).expect("source identity")),
        ordinal: 0,
        name: id.into(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("producer-native", "10"),
            native("pattern-native", "20"),
        ],
    };
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![FeatureInputSurfaceSelection {
            id: "seed".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            selector: 0,
            kind: FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "name".into(),
            feature_ref: "pattern-native".into(),
            producer_feature_refs: vec!["producer-native".into()],
            terminal_feature_ref: Some("producer-native".into()),
            components: vec![FeatureInputComponentPathEntry {
                instance: Some(0x8020),
                type_signature: signature.into(),
                local_id: Some(7),
            }],
        }],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (features, history, lane)
}

#[test]
fn pattern_surface_seed_uses_generated_face_and_dependency() {
    let (mut features, history, lane) = pattern_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_surface_selections(&ctx, &mut features, &[history], &[lane])
        .expect("pattern projection");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        features[1].evaluation.definition()
    else {
        panic!("expected pattern");
    };
    assert!(
        matches!(seeds.as_slice(), [PatternSeed::Faces(FaceSelection::Generated { faces, native })]
        if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(
            FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"),
            "7".into(), &cadmpeg_test_support::service_decode_context(),
        ).expect("selection reference admission").expect("generated face")]
            && native.as_str() == "sldprt:feature-input:surface-component-ids:7")
    );
    assert_eq!(
        features[1].dependencies.as_slice(),
        &[features[0].id.clone()]
    );
}

#[test]
fn pattern_surface_seed_refuses_collection_limit() {
    let (mut features, _history, lane) = pattern_fixture();
    features.remove(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, &mut features, &[], &[lane])
        .expect_err("pattern seed exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT pattern face seeds")
    );
}
