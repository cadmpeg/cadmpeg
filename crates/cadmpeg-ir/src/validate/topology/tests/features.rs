// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use crate::examples::unit_cube;
use crate::features::ExtrudeDirection;
use crate::math::{Point3, Vector3};
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

use super::*;

#[test]
fn historical_body_overlap_ignores_set_ordering_form() {
    use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

    let state =
        FeatureInputTopologyId::mint("test:model:entity#test:input").expect("valid identity");
    let target = BodySelection::historical(
        state.clone(),
        vec![HistoricalBodyId::mint("test:body:4").expect("valid identity")],
        "target".into(),
    )
    .unwrap();
    let overlapping = BodySelection::HistoricalUnorderedSet {
        state: state.clone(),
        selection: crate::features::HistoricalUnorderedBodySelection::try_from_parts(
            vec![
                HistoricalBodyId::mint("test:body:2").expect("valid identity"),
                HistoricalBodyId::mint("test:body:4").expect("valid identity"),
            ],
            vec!["tool-a".into(), "tool-b".into()],
        )
        .expect("valid unordered historical body selection"),
    };
    let disjoint = BodySelection::HistoricalSet {
        state,
        members: crate::features::BodyMembers::try_from_parts(
            vec![HistoricalBodyId::mint("test:body:5").expect("valid identity")],
            vec!["tool".into()],
        )
        .expect("valid historical body selection rows"),
    };

    assert!(crate::features::SectionOperands::new(target.clone(), overlapping).is_err());
    assert!(crate::features::SectionOperands::new(target, disjoint).is_ok());
}

#[test]
fn historical_vertex_selection_requires_input_state_membership() {
    use crate::features::{
        DatumPointConstruction, Feature, FeatureDefinition, FeatureId, FeatureInputTopology,
        VertexSelection,
    };
    use crate::ids::{FeatureInputTopologyId, HistoricalVertexId};
    use crate::schema::EntitySchema;

    let feature_id = FeatureId::mint("test:model:feature#datum-point").expect("identity grammar");
    let state_id = FeatureInputTopologyId::mint("test:model:feature-input#datum-point")
        .expect("valid identity");
    let historical_vertex =
        HistoricalVertexId::mint("test:model:historical-vertex#local").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            id: state_id.clone(),
            input_of: feature_id.clone(),
            bodies: (Vec::new()).try_into().unwrap(),
            faces: (Vec::new()).try_into().unwrap(),
            edges: (Vec::new()).try_into().unwrap(),
            vertices: (vec![historical_vertex.clone()]).try_into().unwrap(),
            native_ref: None,
        });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(crate::math::Point3::new(
                    1.0, 2.0, 3.0,
                ))
                .unwrap(),
                construction: Some(Box::new(DatumPointConstruction::Vertex {
                    vertex: VertexSelection::historical(
                        state_id.clone(),
                        historical_vertex,
                        "vertex:local".into(),
                    )
                    .unwrap(),
                })),
            },
        ),
        native_ref: None,
    });

    let mut references = Vec::new();
    ir.model.features[0].visit_references(&mut |reference| references.push(reference.target));
    assert_eq!(references, vec![state_id.as_str()]);

    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));

    let missing = "test:model:historical-vertex#missing";
    ir.model.features[0]
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::DatumPoint {
                construction: Some(construction),
                ..
            } = definition
            else {
                unreachable!("test feature is a constructed datum point")
            };
            let DatumPointConstruction::Vertex {
                vertex: VertexSelection::Historical { vertex, .. },
            } = construction.as_mut()
            else {
                unreachable!("test datum point uses a historical vertex")
            };
            *vertex = HistoricalVertexId::mint(missing).expect("valid identity");
        })
        .unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.entity.as_deref() == Some(feature_id.as_str())
                && finding.message == format!("references missing historical vertex `{missing}`")
        }));
}

#[test]
fn neutral_features_resolve_sketch_profile_and_path_operands() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        LinearTermination, PathRef, ProfileRef,
    };
    use crate::sketches::SketchId;

    let sketch = SketchId::mint("synthetic:test:sketch#missing").unwrap();
    let definitions = [
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch.clone()),
            direction: ExtrudeDirection::ProfileNormal {},
            start: crate::features::ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: crate::scalar::NonZeroLength::new(10.0).unwrap(),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::NewBody,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Sweep {
            shape: crate::features::SweepShape::new(
                crate::features::SweepSection::Profile(
                    ProfileRef::Sketch(sketch.clone()).try_into().unwrap(),
                ),
                Vec::new(),
                crate::features::SweepMode::NewBody,
            )
            .unwrap(),

            path: Some(PathRef::Sketch(sketch.clone())),

            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<[FeatureDefinition; 2]>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#sketch-ref").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(definitions[1].clone()),
        native_ref: None,
    });
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.message.contains("missing sketch"))
            .count(),
        2
    );
}

#[test]
fn feature_history_rejects_dangling_and_forward_dependencies() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, Feature, FeatureDefinition,
        FeatureId, FeatureSourceContent, LinearTermination, ParameterId, ProfileRef,
    };
    use crate::ids::{BodyId, FaceId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let feature_id = FeatureId::mint("synthetic:test:feature#invalid").expect("identity grammar");
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: (vec![feature_id.clone()]).try_into().unwrap(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: (vec![
            FeatureSourceContent::Parameter(
                ParameterId::mint("synthetic:test:parameter#missing").expect("identity grammar"),
            ),
            FeatureSourceContent::Feature(feature_id.clone()),
        ])
        .try_into()
        .unwrap(),

        evaluation: crate::features::FeatureEvaluation::new(
            FeatureDefinition::Extrude {
                profile: ProfileRef::Faces(vec![FaceId::mint(
                    "synthetic:test:face#profile-missing",
                )
                .expect("valid identity")]),
                direction: ExtrudeDirection::ProfileNormal {},
                start: crate::features::ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Faces(vec![FaceId::mint(
                                "synthetic:test:face#termination-missing",
                            )
                            .expect("valid identity")]),
                            offset: None,
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            vec![BodyId::mint("synthetic:test:body#missing").expect("valid identity")],
        )
        .unwrap(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#duplicate-order").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "Marker".into(),
                parameters: BTreeMap::new(),
            },
        ),
        native_ref: None,
    });
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    for fragment in [
        "does not precede",
        "missing output body",
        "missing profile face",
        "missing termination face",
        "repeats feature ordinal",
        "missing content parameter",
        "content child",
    ] {
        assert!(
            report.findings.iter().any(|finding| {
                finding.entity.as_deref() == Some(feature_id.as_str())
                    && finding.message.contains(fragment)
            }),
            "missing finding containing {fragment:?}"
        );
    }
}

#[test]
fn feature_parameters_require_unique_names_and_ordinals() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner = FeatureId::mint("synthetic:test:feature#parameters").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "Test".into(),
                parameters: BTreeMap::new(),
            },
        ),
        native_ref: None,
    });
    for (index, name) in ["Width", "Width"].into_iter().enumerate() {
        ir.model.parameters.push(DesignParameter {
            id: ParameterId::mint(format!("synthetic:test:parameter#{index}"))
                .expect("identity grammar"),
            owner: Some(owner.clone()),
            ordinal: 0,
            name: name.into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            dependencies: crate::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats parameter name")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats parameter ordinal")));
}

#[test]
fn parameter_dependencies_must_exist_and_precede_consumers() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner =
        FeatureId::mint("synthetic:test:feature#dependency-owner").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "Test".into(),
                parameters: BTreeMap::new(),
            },
        ),
        native_ref: None,
    });
    let first = ParameterId::mint("synthetic:test:parameter#first").expect("identity grammar");
    let second = ParameterId::mint("synthetic:test:parameter#second").expect("identity grammar");
    for (id, ordinal, dependencies) in [
        (first.clone(), 0, vec![second.clone()]),
        (
            second,
            1,
            vec![ParameterId::mint("synthetic:test:parameter#missing").expect("identity grammar")],
        ),
    ] {
        ir.model.parameters.push(DesignParameter {
            id,
            owner: Some(owner.clone()),
            ordinal,
            name: format!("P{ordinal}"),
            expression: String::new(),
            display: None,
            value: None,
            dependencies: (dependencies).try_into().unwrap(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("does not precede its consumer")));
    assert!(findings.iter().any(|finding| {
        finding
            .message
            .contains("parameter dependency `synthetic:test:parameter#missing`")
    }));
}

#[test]
fn document_parameters_can_feed_feature_parameters() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner = FeatureId::mint("synthetic:test:feature#consumer").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "Test".into(),
                parameters: BTreeMap::new(),
            },
        ),
        native_ref: None,
    });
    let document =
        ParameterId::mint("synthetic:test:parameter#document").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        id: document.clone(),
        owner: None,
        ordinal: 0,
        name: "Width".into(),
        expression: "60 mm".into(),
        display: None,
        value: None,
        dependencies: crate::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:parameter#owned").expect("identity grammar"),
        owner: Some(owner),
        ordinal: 0,
        name: "Distance".into(),
        expression: "Width / 2".into(),
        display: None,
        value: None,
        dependencies: (vec![document]).try_into().unwrap(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).findings.is_empty());
}

#[test]
fn offset_plane_references_form_an_acyclic_graph_independent_of_list_order() {
    use crate::{
        features::{DatumPlaneReference, Feature, FeatureDefinition, FeatureId},
        scalar::Length,
    };

    let mut ir = unit_cube();
    let principal = FeatureId::mint("synthetic:test:feature#principal").expect("identity grammar");
    let feature = |id: &str, ordinal: u64, definition: FeatureDefinition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:feature#offset",
        0,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature {
                feature: principal.clone(),
            }),
            distance: Length::new(5.0).unwrap(),
        },
    ));
    ir.model.features.push(feature(
        principal.as_str(),
        1,
        FeatureDefinition::DatumPlane {
            frame: crate::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        },
    ));
    ir.finalize();

    let report = validate_neutral(&ir, Vec::new());
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.message.contains("datum-plane reference cycle")));

    let offset = ir.model.features[0].id.clone();
    ir.model.features[1]
        .evaluation
        .set_definition(FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: offset }),
            distance: Length::new(5.0).unwrap(),
        })
        .unwrap();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("datum-plane reference cycle")));
}

#[test]
fn generated_termination_vertices_require_declared_feature_dependencies() {
    use crate::features::{
        BooleanOp, ConfigurationBodies, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        GeneratedVertexRef, LinearTermination, ProfileRef, VertexSelection,
    };
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let source =
        FeatureId::mint("synthetic:test:feature#0-vertex-source").expect("identity grammar");
    ir.model.features.push(Feature {
        id: source.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            },
        ),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#1-extrude").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("test:profile".into()),
                direction: ExtrudeDirection::ProfileNormal {},
                start: crate::features::ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToVertex {
                            vertex: VertexSelection::generated(
                                GeneratedVertexRef::new(source.clone(), "vertex-0".into()).unwrap(),
                                "test:vertex-selection".into(),
                            )
                            .unwrap(),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
        native_ref: None,
    });

    let message = "generated termination vertex is invalid";
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
    let extrude = ir.model.features[1].id.clone();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:configuration#vertex").expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: None,
        name: "Vertex".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Unresolved,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::from([(
            extrude.clone(),
            ConfigurationFeatureState {
                evaluation: crate::features::ConfigurationEvaluation::Active {
                    outputs: crate::features::DistinctMembers::default(),
                },
                dependencies: crate::features::DistinctMembers::default(),
                definition: ir.model.features[1].evaluation.definition().clone(),
            },
        )]),
        native_ref: None,
    });
    ir.model.features[1].dependencies.insert(source.clone());
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
    let configuration_message = format!(
        "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
        extrude.as_str(),
        source.as_str()
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == configuration_message));

    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&extrude)
        .expect("configured extrude");
    state.dependencies.insert(source);
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
}

#[test]
fn pattern_feature_seeds_must_be_declared_dependencies() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, PatternKind, PatternSeed, PatternTransform,
    };

    let mut ir = unit_cube();
    let seed = FeatureId::mint("synthetic:test:feature#pattern-seed").expect("identity grammar");
    ir.model.features.push(Feature {
        id: seed.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DatumPoint {
                position: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            },
        ),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#pattern").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Pattern {
                seeds: vec![PatternSeed::Feature(seed.clone())],
                pattern: PatternKind::new(PatternTransform::Mirror {
                    plane_origin: Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                })
                .unwrap(),
            },
        ),
        native_ref: None,
    });
    let message = format!(
        "pattern omits seed feature `{}` from its dependencies",
        seed.as_str()
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));

    ir.model.features[1].dependencies.insert(seed);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
}

#[test]
fn definition_references_must_be_declared_dependencies_in_every_configuration() {
    use crate::{
        features::{
            BooleanOp, ConfigurationBodies, ConfigurationFeatureState, ConfigurationId,
            DatumPlaneReference, DesignConfiguration, ExtrudeDirection, ExtrudeExtent, ExtrudeSide,
            ExtrudeStart, Feature, FeatureDefinition, FeatureId, GeneratedCurveRef,
            LinearTermination, PatternKind, PatternSeed, PatternTransform, ProfileRef,
        },
        scalar::Length,
    };
    use std::collections::{BTreeMap, HashSet};

    let mut ir = unit_cube();
    let source = FeatureId::mint("synthetic:test:feature#0-source").expect("identity grammar");
    let offset = FeatureId::mint("synthetic:test:feature#1-offset").expect("identity grammar");
    let derived = FeatureId::mint("synthetic:test:feature#2-derived").expect("identity grammar");
    let pattern = FeatureId::mint("synthetic:test:feature#3-pattern").expect("identity grammar");
    let block = FeatureId::mint("synthetic:test:feature#4-block").expect("identity grammar");
    let instance = FeatureId::mint("synthetic:test:feature#5-instance").expect("identity grammar");
    let profile =
        FeatureId::mint("synthetic:test:feature#6-profile-consumer").expect("identity grammar");
    let feature = |id, ordinal, definition| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    ir.model.features = vec![
        feature(
            source.clone(),
            0,
            FeatureDefinition::DatumPlane {
                frame: crate::features::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            },
        ),
        feature(
            offset.clone(),
            1,
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature {
                    feature: source.clone(),
                }),
                distance: Length::new(5.0).unwrap(),
            },
        ),
        feature(
            derived.clone(),
            2,
            FeatureDefinition::DerivedGeometry {
                source: source.clone(),
            },
        ),
        feature(
            pattern.clone(),
            3,
            FeatureDefinition::Pattern {
                seeds: vec![PatternSeed::Feature(source.clone())],
                pattern: PatternKind::new(PatternTransform::Mirror {
                    plane_origin: Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                })
                .unwrap(),
            },
        ),
        feature(
            block.clone(),
            4,
            FeatureDefinition::SketchBlockDefinition { sketch: None },
        ),
        feature(
            instance.clone(),
            5,
            FeatureDefinition::SketchBlockInstance {
                block: Some(block.clone()),
                placement: Some(crate::transform::Transform::identity()),
            },
        ),
        feature(
            profile.clone(),
            6,
            FeatureDefinition::Extrude {
                profile: ProfileRef::generated(
                    vec![GeneratedCurveRef::new(source.clone(), "curve-0".into()).unwrap()],
                    "synthetic:test:profile-selection".into(),
                )
                .unwrap(),
                direction: ExtrudeDirection::ProfileNormal {},
                start: ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: crate::scalar::NonZeroLength::new(5.0).unwrap(),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    ir.model.features[2].dependencies.insert(source.clone());
    ir.model.features[3].dependencies.insert(source.clone());
    ir.model.features[6].dependencies.insert(source.clone());
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:configuration#offset-plane")
            .expect("identity grammar"),
        ordinal: 0,
        active: false,
        source_index: None,
        name: "Offset".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Unresolved,
        parameter_values: BTreeMap::new(),
        feature_states: [
            (offset.clone(), 1),
            (derived.clone(), 2),
            (pattern.clone(), 3),
            (instance.clone(), 5),
            (profile.clone(), 6),
        ]
        .into_iter()
        .map(|(feature, index)| {
            (
                feature,
                ConfigurationFeatureState {
                    evaluation: crate::features::ConfigurationEvaluation::Active {
                        outputs: crate::features::DistinctMembers::default(),
                    },
                    dependencies: crate::features::DistinctMembers::default(),
                    definition: ir.model.features[index].evaluation.definition().clone(),
                },
            )
        })
        .collect(),
        native_ref: None,
    });

    let findings = validate_neutral(&ir, Vec::new())
        .findings
        .into_iter()
        .map(|finding| finding.message)
        .collect::<HashSet<_>>();
    assert!(findings.contains(&format!(
        "offset plane omits reference feature `{}` from its dependencies",
        source.as_str()
    )));
    assert!(findings.contains(&format!(
        "sketch block instance omits block feature `{}` from its dependencies",
        block.as_str()
    )));
    for feature in [&offset, &derived, &pattern, &profile] {
        assert!(findings.contains(&format!(
            "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
            feature.as_str(),
            source.as_str()
        )));
    }
    assert!(findings.contains(&format!(
        "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
        instance.as_str(),
        block.as_str()
    )));

    ir.model.features[1].dependencies.insert(source.clone());
    ir.model.features[5].dependencies.insert(block.clone());
    for feature in [&offset, &derived, &pattern, &profile] {
        ir.model.configurations[0]
            .feature_states
            .get_mut(feature)
            .expect("configuration feature state")
            .dependencies
            .insert(source.clone());
    }
    ir.model.configurations[0]
        .feature_states
        .get_mut(&instance)
        .expect("block-instance state")
        .dependencies
        .insert(block);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:#?}", report.findings);
}

#[test]
fn generated_body_selection_must_name_a_declared_producer_result() {
    use crate::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureResultTopology,
        GeneratedBodyRef,
    };
    use crate::ids::FeatureResultTopologyId;

    let mut ir = CadIr::empty();
    let producer = FeatureId::mint("synthetic:test:feature#0-producer").expect("identity grammar");
    ir.model.features.push(Feature {
        id: producer.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Native {
                kind: "producer".into(),
                parameters: BTreeMap::default(),
            },
        ),
        native_ref: None,
    });
    ir.model.feature_result_topologies.push(
        FeatureResultTopology::new(
            FeatureResultTopologyId::mint("synthetic:test:feature-result-topology#producer")
                .expect("valid identity"),
            producer.clone(),
            vec!["body#declared".into()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap(),
    );
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#1-consumer").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: (vec![producer.clone()]).try_into().unwrap(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::generated(
                    vec![GeneratedBodyRef {
                        feature: producer,
                        local_id: "body#declared".to_owned().try_into().unwrap(),
                    }],
                    "synthetic:native-selection#0".into(),
                )
                .unwrap(),
            },
        ),
        native_ref: None,
    });

    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    ir.model.features[1]
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::BaseFeature {
                bodies: BodySelection::Generated { bodies, .. },
            } = definition
            else {
                panic!("test consumer must retain its generated body selection");
            };
            *bodies =
                vec![
                    GeneratedBodyRef::new(bodies[0].feature.clone(), "body#undeclared".into())
                        .unwrap(),
                ]
                .try_into()
                .unwrap();
        })
        .unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "generated body selection is invalid"));
}

#[test]
fn reference_images_require_valid_assets_and_plane_placements() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::features::{Feature, FeatureDefinition, FeatureId};
    use crate::math::Point2;

    let asset_id = AssetId::mint("synthetic:test:asset#reference-image").expect("identity grammar");
    let feature_id =
        FeatureId::mint("synthetic:test:feature#reference-image").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.assets.push(
        Asset::try_new(
            asset_id.clone(),
            Some("reference.png".into()),
            Some("image/png".into()),
            AssetContent::Embedded {
                data: crate::assets::AssetData::new(vec![1, 2, 3]).expect("nonempty asset data"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::ReferenceImage {
                asset: asset_id,
                visible: true,
                mirror_u: false,
                mirror_v: false,
                frame: crate::features::FeatureUnitPlaneFrame::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
                bounds: crate::features::FeatureImageBounds::new([
                    Point2::new(-10.0, -5.0),
                    Point2::new(10.0, 5.0),
                ])
                .unwrap(),
                opacity: Some(crate::scalar::Fraction::new(0.75).unwrap()),
            },
        ),
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    assert_eq!(
        serde_json::to_value(&ir.model.assets[0]).unwrap()["content"]["data"],
        "AQID"
    );

    ir.model.assets.clear();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(feature_id.as_str())
            && finding.message.contains("reference-image asset")
    }));
}

#[test]
fn decals_require_valid_assets_faces_and_opacity() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::features::{DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureId};

    let asset_id = AssetId::mint("synthetic:test:asset#decal").expect("identity grammar");
    let feature_id = FeatureId::mint("synthetic:test:feature#decal").expect("identity grammar");
    let mut ir = unit_cube();
    let face_id = ir.model.faces[0].id.clone();
    ir.model.assets.push(
        Asset::try_new(
            asset_id.clone(),
            Some("decal.png".into()),
            Some("image/png".into()),
            AssetContent::Embedded {
                data: crate::assets::AssetData::new(vec![1, 2, 3]).expect("nonempty asset data"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(FeatureDefinition::Decal {
            asset: asset_id,
            faces: FaceSelection::Faces(vec![face_id]),
            mapping: DecalMapping::FitToFaces,
            opacity: Some(crate::scalar::Fraction::new(0.75).unwrap()),
        }),
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
}
