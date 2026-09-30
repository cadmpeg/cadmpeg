//! Resource admission for generated Combine operands.

use super::super::{project_compact_combine_paths, COMPACT_EDGE_VECTOR_MARKER};
use super::{Feature, FeatureHistory, FeatureInputLane, FeatureSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineOperands, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};
use std::collections::BTreeMap;

fn combine_projection_error(policy: DecodePolicy) -> cadmpeg_core::CodecError {
    let feature = |id: &str, source| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: source,
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
    let mut combine = feature("combine", 44);
    combine.properties.insert(
        cadmpeg_core::nonblank_literal!("Target"),
        "sldprt:feature-input:body-path:7:12".into(),
    );
    combine.properties.insert(
        cadmpeg_core::nonblank_literal!("Tools"),
        "sldprt:feature-input:body-path:7:112".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature("target", 42), feature("tools", 43), combine],
    };
    let mut payload = vec![0; 200];
    for (marker, source, local_id) in [(12, 42u32, 6u32), (112, 43, 7)] {
        payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 3, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let entry = marker + 18;
        payload[entry..entry + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
        payload[entry + 4..entry + 16].fill(1);
        payload[entry + 8..entry + 12].copy_from_slice(&source.to_le_bytes());
        payload[entry + 16..entry + 20].copy_from_slice(&local_id.to_le_bytes());
    }
    let lane = FeatureInputLane {
        id: "lane#7".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
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
    };
    let model = |native: &str, ordinal, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(format!("test:model:feature#{native}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        native_ref: Some(native.into()),
        evaluation: FeatureEvaluation::from_definition(definition),
    };
    let producer = || {
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Unresolved,
        })
    };
    let mut features = vec![
        model("target", 0, producer()),
        model("tools", 1, producer()),
        model(
            "combine",
            2,
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .unwrap(),
                op: BooleanKind::Join,
                keep_tools: false,
            }),
        ),
    ];
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&lane.native_payload, &arena, &DecodePolicy::service())
            .unwrap();
    let mut admitted = features.clone();
    project_compact_combine_paths(
        &service,
        &mut admitted,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        admitted[2].evaluation.definition()
    else {
        panic!("Combine feature");
    };
    assert!(
        matches!(operands.target(), BodySelection::Generated { bodies, native }
        if bodies.len() == 1 && bodies[0].feature == admitted[0].id && bodies[0].local_id == "6"
            && native == "sldprt:feature-input:body-path:7:12")
    );
    assert!(
        matches!(operands.tools(), BodySelection::Generated { bodies, native }
        if bodies.len() == 1 && bodies[0].feature == admitted[1].id && bodies[0].local_id == "7"
            && native == "sldprt:feature-input:body-path:7:112")
    );
    assert_eq!(
        admitted[2].dependencies.iter().collect::<Vec<_>>(),
        [&admitted[0].id, &admitted[1].id]
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    project_compact_combine_paths(
        &ctx,
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    )
    .unwrap_err()
}

#[test]
fn combine_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(
        matches!(combine_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn combine_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(
        matches!(combine_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn combine_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(
        matches!(combine_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits)
    );
}
