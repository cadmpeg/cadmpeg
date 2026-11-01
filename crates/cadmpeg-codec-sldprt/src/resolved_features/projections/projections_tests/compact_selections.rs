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
        product_definition: None,
        driving: true,
        id: ParameterId::mint("synthetic:test:id#bound-parameter").expect("identity grammar"),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
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
                sequence: 0,
                feature_ref: None,
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "NativeFeature".into(),
            },
            FeatureInputName {
                sequence: 10,
                feature_ref: None,
                id: "parameter-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 10,
                object_id: None,
                value: "D1".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            sequence: 20,
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
            archive_class: None,
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

#[test]
fn archive_length_class_types_a_raw_scalar_value_as_a_length() {
    use cadmpeg_ir::features::ParameterValue;
    // With no display number the decoder writes the raw SI value (`0.00635`);
    // the archive walk read that value in a `moLengthParameter_c`, so it is
    // 6.35 mm. Without the class it stays a plain real.
    for (class, millimetres) in [
        (
            Some(crate::records::ArchiveParameterClass::Length),
            Some(6.35),
        ),
        (None, None),
    ] {
        let (mut parameters, features, histories, mut lanes) = parameter_scalar_binding_fixture();
        parameters[0].expression = "0.00635".into();
        parameters[0].value = Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.00635).unwrap(),
        ));
        lanes[0].scalars[0].value = cadmpeg_ir::scalar::FiniteReal::new(0.00635).unwrap();
        lanes[0].scalars[0].archive_class = class;
        bind_parameter_scalars(&mut parameters, &features, &histories, &lanes)
            .expect("scalar binding");
        match millimetres {
            Some(mm) => assert!(
                matches!(parameters[0].value, Some(ParameterValue::Length(value))
                    if (value.get() - mm).abs() < 1e-9),
                "{:?}",
                parameters[0].value
            ),
            None => assert!(
                matches!(parameters[0].value, Some(ParameterValue::Real(_))),
                "{:?}",
                parameters[0].value
            ),
        }
    }
}

#[test]
fn tied_object_names_bind_unowned_scalars_to_the_later_history_feature() {
    let (parameters, features, mut histories, mut lanes) = parameter_scalar_binding_fixture();
    let native = histories[0].features[0].clone();
    histories[0].features = ["native-a", "native-b"]
        .map(|id| crate::records::Feature {
            id: id.into(),
            ..native.clone()
        })
        .to_vec();
    lanes[0].scalars[0].feature_ref = None;
    let features = ["native-a", "native-b"].map(|native| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{native}")).expect("identity grammar"),
        native_ref: Some(native.into()),
        ..features[0].clone()
    });
    let parameters = features
        .each_ref()
        .map(|feature| cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId::mint(format!("{}-parameter", feature.id))
                .expect("identity grammar"),
            owner: Some(feature.id.clone()),
            ..parameters[0].clone()
        });
    for _ in 0..32 {
        let mut bound = parameters.clone();
        bind_parameter_scalars(&mut bound, &features, &histories, &lanes).expect("scalar binding");
        assert_eq!(
            bound
                .each_ref()
                .map(|parameter| parameter.native_ref.as_deref()),
            [None, Some("native-scalar")]
        );
    }
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
            sequence: 0,
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
    // D11: the typed driving field is the sole reference/driving fact.
    assert!(!parameters[0].driving);
}

#[test]
fn compact_surface_projection_binds_generated_thread_face_alias() {
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
                standard: None,
                designation: None,
                pitch: None,
                modeled: None,
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
                distance: cadmpeg_ir::Dimension::Literal(
                    Length::new(2.0).expect("offset distance"),
                ),
            },
        ));
    let mut features = [thread, plane];
    project_compact_surface_selections(&mut features, &[], &[]).expect("face alias projection");
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

#[test]
fn compact_generated_edges_match_the_linear_repeat_check() {
    use crate::records::FeatureInputEdgeSelection;
    use cadmpeg_ir::features::GeneratedEdgeRef;

    fn linear(
        selections: &[&FeatureInputEdgeSelection],
        feature_ids_by_native: &std::collections::HashMap<String, FeatureId>,
    ) -> Option<Vec<GeneratedEdgeRef>> {
        let mut generated = Vec::<GeneratedEdgeRef>::new();
        for selection in selections {
            let feature_id = feature_ids_by_native.get(selection.terminal_feature_ref.as_ref()?)?;
            let local_id = super::super::compact_edge_path_value(selection);
            let edge = GeneratedEdgeRef::new(feature_id.clone(), local_id).ok()?;
            if !generated.contains(&edge) {
                generated.push(edge);
            }
        }
        Some(generated)
    }

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % bound).unwrap()
    };
    let feature_ids_by_native = (0..3)
        .map(|native| {
            (
                format!("native-{native}"),
                FeatureId::mint(format!("synthetic:test:id#feature-{native}"))
                    .expect("identity grammar"),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let (mut complete, mut repeated) = (0, 0);
    for _ in 0..256 {
        let selections = (0..next(40))
            .map(|ordinal| FeatureInputEdgeSelection {
                id: format!("edge-{ordinal}"),
                parent: "lane".into(),
                ordinal,
                sequence: ordinal,
                object_name_ref: "name".into(),
                feature_ref: "fillet".into(),
                local_edge_ids: (0..next(3)).map(|_| next(4)).collect(),
                components: Vec::new(),
                references: Vec::new(),
                producer_feature_refs: Vec::new(),
                terminal_feature_ref: (next(32) != 0).then(|| format!("native-{}", next(4))),
            })
            .collect::<Vec<_>>();
        let selections = selections.iter().collect::<Vec<_>>();
        let expected = linear(&selections, &feature_ids_by_native);
        if let Some(edges) = &expected {
            complete += 1;
            repeated += usize::from(edges.len() < selections.len());
        }
        assert_eq!(
            super::super::compact_generated_edges(&selections, &feature_ids_by_native),
            expected
        );
    }
    assert!(complete > 0 && repeated > 0);
}

fn compact_edge_projection_feature() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#fillet").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
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
            sequence: 0,
            id: "body".into(),
            parent: "lane".into(),
            ordinal: 0,
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
    let (mut feature, lane) = compact_body_projection_fixture();
    project_compact_body_selections(std::slice::from_mut(&mut feature), &[lane]);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::KeepSelected,
        }) if bodies.as_slice() == ["3", "4"] && native.as_str() == "sldprt:feature-input:body-ids:3,4"));
}

#[test]
fn compact_body_projection_keeps_duplicate_selection_unresolved() {
    let (mut feature, mut lane) = compact_body_projection_fixture();
    lane.body_selections[0].local_body_ids = vec![3, 3];
    project_compact_body_selections(std::slice::from_mut(&mut feature), &[lane]);
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
        sequence: 0,
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
    use std::collections::HashMap;

    let path = |local_id| {
        vec![FeatureInputComponentPathEntry {
            instance: None,
            type_signature: [0; 12].into(),
            local_id: Some(local_id),
        }]
    };
    let paths = [path(7), path(7), path(9)];
    let selection = draft_face_selection(
        &paths,
        "consumer",
        &[],
        &HashMap::new(),
        &mut cadmpeg_ir::features::DistinctMembers::default(),
    );
    assert_eq!(selection, cadmpeg_ir::features::FaceSelection::Native(
        "sldprt:feature-input:draft-surface-vectors:sldprt:feature-input:surface-component-ids:7;sldprt:feature-input:surface-component-ids:9".into()
    ));
}
