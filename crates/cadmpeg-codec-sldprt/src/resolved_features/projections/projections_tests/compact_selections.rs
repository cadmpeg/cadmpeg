//! Compact selection projection tests.

use super::super::{
    bind_parameter_scalars, compact_surface_selection_set_value, draft_face_selection,
    project_compact_body_selections, project_compact_surface_selections,
    synthesize_display_relation_parameters,
};
use crate::records::{
    FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind,
};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, FeatureDefinition, FeatureId, FeatureOperation,
    UnresolvedFamily,
};
use std::collections::BTreeMap;

fn parameter_scalar_binding_fixture() -> (
    Vec<cadmpeg_ir::features::DesignParameter>,
    Vec<cadmpeg_ir::features::Feature>,
    Vec<crate::records::FeatureHistory>,
    Vec<FeatureInputLane>,
) {
    use crate::records::{FeatureInputName, FeatureInputScalar, FeatureInputScalarRole};
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};

    let mut feature = compact_edge_projection_feature();
    feature.native_ref = Some("native-feature".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#bound-parameter").expect("identity grammar"),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "native-feature".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "NativeFeature".into(),
            kind: "Feature".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "NativeFeature".into(),
            },
            FeatureInputName {
                id: "parameter-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 10,
                object_id: None,
                value: "D1".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            id: "native-scalar".into(),
            parent: "lane".into(),
            feature_ref: Some("native-feature".into()),
            ordinal: 0,
            offset: 20,
            object_id: 0,
            name: "parameter-name".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
            role: FeatureInputScalarRole::Native,
            operands: Vec::new(),
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (vec![parameter], vec![feature], vec![history], vec![lane])
}

#[test]
fn parameter_scalar_binding_preserves_native_reference_and_value() {
    let (mut parameters, features, histories, lanes) = parameter_scalar_binding_fixture();
    bind_parameter_scalars(&mut parameters, &features, &histories, &lanes).expect("scalar binding");
    assert_eq!(parameters[0].native_ref.as_deref(), Some("native-scalar"));
    assert!(
        matches!(parameters[0].value, Some(cadmpeg_ir::features::ParameterValue::Real(value))
        if value.get() == 2.0)
    );
}

fn display_relation_synthesis_fixture() -> (
    Vec<cadmpeg_ir::features::DesignParameter>,
    Vec<cadmpeg_ir::features::Feature>,
    Vec<FeatureInputLane>,
) {
    use crate::records::{
        FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalarRole,
    };
    let (_, features, _, mut lanes) = parameter_scalar_binding_fixture();
    lanes[0].scalars[0].role = FeatureInputScalarRole::Display;
    lanes[0].scalars[0].value = cadmpeg_ir::scalar::FiniteReal::new(0.012).unwrap();
    lanes[0]
        .relation_instances
        .push(FeatureInputRelationInstance {
            id: "sldprt:feature-input:relation-instance#lane:10".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "native-feature".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["native-scalar".into()],
                None,
                Some("native-scalar".into()),
            )
            .expect("display scalar relation"),
            operands: Vec::new(),
        });
    (Vec::new(), features, lanes)
}

#[test]
fn display_relation_synthesis_preserves_reference_parameter() {
    let (mut parameters, features, lanes) = display_relation_synthesis_fixture();
    synthesize_display_relation_parameters(&mut parameters, &features, &lanes)
        .expect("display relation synthesis");
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].name, "D1@reference");
    assert_eq!(parameters[0].owner.as_ref(), Some(&features[0].id));
    assert_eq!(
        parameters[0]
            .properties
            .get("sldprt_relation_parameter_role"),
        Some(&"reference".into())
    );
}

#[test]
fn compact_surface_projection_binds_generated_thread_face_alias() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, GeneratedFaceRef};
    use cadmpeg_ir::scalar::Length;

    let mut thread = compact_edge_projection_feature();
    thread.native_ref = Some("native-thread".into());
    let producer = thread.id.clone();
    let generated =
        GeneratedFaceRef::new(producer.clone(), "face-1".into()).expect("generated face identity");
    thread
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::CosmeticThread {
                face: FaceSelection::generated(vec![generated], "native-face".into())
                    .expect("generated face selection"),
                diameter: None,
                extent: None,
            },
        ));
    let mut plane = compact_edge_projection_feature();
    plane.id = FeatureId::mint("synthetic:test:id#face-alias-plane").expect("identity grammar");
    plane.native_ref = Some("native-plane".into());
    plane.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceFeature"),
        "native-thread".into(),
    );
    plane
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumOffsetPlane {
                reference: None,
                distance: Length::new(2.0).expect("offset distance"),
            },
        ));
    let mut features = [thread, plane];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_surface_selections(&ctx, &mut features, &[], &[])
        .expect("face alias projection");
    assert!(matches!(features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face {
                face: FaceSelection::Generated { faces, native }
            }), ..
        }) if faces.len() == 1
            && faces[0].feature == producer
            && faces[0].local_id.as_str() == "face-1"
            && native.as_str() == "native-face"));
    assert!(features[1].dependencies.contains(&producer));
}

fn compact_edge_projection_feature() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#fillet").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Fillet,
            }),
        ),
        native_ref: Some("fillet".into()),
    }
}

fn compact_body_projection_fixture() -> (cadmpeg_ir::features::Feature, FeatureInputLane) {
    let mut feature = compact_edge_projection_feature();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }));
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: vec![FeatureInputBodySelection {
            id: "body".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_name_ref: "name".into(),
            feature_ref: "fillet".into(),
            local_body_ids: vec![3, 4],
            body_state_ids: Vec::new(),
            mode: Some(BodyRetentionMode::KeepSelected),
        }],
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (feature, lane)
}

#[test]
fn compact_body_projection_preserves_selection_and_retention_mode() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("body projection");
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::KeepSelected,
        }) if bodies.as_slice() == ["3", "4"] && native.as_str() == "sldprt:feature-input:body-ids:3,4"));
}

#[test]
fn compact_body_projection_keeps_duplicate_selection_unresolved() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, mut lane) = compact_body_projection_fixture();
    lane.body_selections[0].local_body_ids = vec![3, 3];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("duplicate is a semantic no-match");
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        })
    ));
}

fn full_round_selection() -> FeatureInputSurfaceSelection {
    FeatureInputSurfaceSelection {
        id: "surface".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        kind: FeatureInputSurfaceSelectionKind::Component,
        object_name_ref: "name".into(),
        feature_ref: "fillet".into(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
        components: Vec::new(),
    }
}

#[test]
fn surface_selection_set_preserves_order_and_deduplicates_native_paths() {
    let mut first = full_round_selection();
    first.components = vec![FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12].into(),
        local_id: Some(7),
    }];
    let duplicate = first.clone();
    let mut second = first.clone();
    second.components[0].local_id = Some(9);
    let value = compact_surface_selection_set_value(&[&first, &duplicate, &second]);
    assert_eq!(value, "sldprt:feature-input:surface-selection-vectors:sldprt:feature-input:surface-component-ids:7;sldprt:feature-input:surface-component-ids:9");
    assert_eq!(
        compact_surface_selection_set_value(&[&first, &duplicate]),
        "sldprt:feature-input:surface-component-ids:7"
    );
    assert_eq!(
        compact_surface_selection_set_value(&[]),
        "sldprt:feature-input:surface-selection-vectors:"
    );
}

#[test]
fn draft_face_selection_preserves_native_path_order_and_deduplicates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::HashMap;

    let path = |local_id| {
        vec![FeatureInputComponentPathEntry {
            instance: None,
            type_signature: [0; 12].into(),
            local_id: Some(local_id),
        }]
    };
    let paths = [path(7), path(7), path(9)];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let selection = draft_face_selection(
        &ctx,
        &paths,
        "consumer",
        &[],
        &HashMap::new(),
        &mut cadmpeg_ir::features::DistinctMembers::default(),
    )
    .expect("draft native path selection");
    assert_eq!(selection, cadmpeg_ir::features::FaceSelection::Native(
        "sldprt:feature-input:draft-surface-vectors:sldprt:feature-input:surface-component-ids:7;sldprt:feature-input:surface-component-ids:9".into()
    ));
}
