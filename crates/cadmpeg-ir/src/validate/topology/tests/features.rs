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
fn zero_count_composite_stage_is_compositionally_invalid() {
    let stages = [
        crate::features::PatternStage {
            pattern: Box::new(PatternKind::Linear {
                direction: None,
                spacing: Length(1.0),
                count: 1,
                second: None,
            }),
            combination: PatternStageCombination::Initialize,
        },
        crate::features::PatternStage {
            pattern: Box::new(PatternKind::Scale {
                center: crate::features::PatternScaleCenter::FirstSeedCentroid,
                final_factor: 2.0,
                count: 0,
            }),
            combination: PatternStageCombination::AlignedSlices,
        },
    ];
    assert!(!composite_composition_is_valid(&stages));
}

#[test]
fn unresolved_composite_count_can_feed_a_cartesian_stage() {
    let stages = [
        crate::features::PatternStage {
            pattern: Box::new(PatternKind::Unresolved { form: None }),
            combination: PatternStageCombination::Initialize,
        },
        crate::features::PatternStage {
            pattern: Box::new(PatternKind::Linear {
                direction: None,
                spacing: Length(1.0),
                count: 2,
                second: None,
            }),
            combination: PatternStageCombination::CartesianProduct,
        },
    ];
    assert!(composite_composition_is_valid(&stages));
}

#[test]
fn historical_body_overlap_ignores_set_ordering_form() {
    use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

    let state = FeatureInputTopologyId("test:input".into());
    let target = BodySelection::Historical {
        state: state.clone(),
        bodies: vec![HistoricalBodyId("test:body:4".into())],
        native: "target".into(),
    };
    let overlapping = BodySelection::HistoricalUnorderedSet {
        state: state.clone(),
        bodies: vec![
            HistoricalBodyId("test:body:2".into()),
            HistoricalBodyId("test:body:4".into()),
        ],
        native: vec!["tool-a".into(), "tool-b".into()],
    };
    let disjoint = BodySelection::HistoricalSet {
        state,
        bodies: vec![HistoricalBodyId("test:body:5".into())],
        native: vec!["tool".into()],
    };

    assert!(body_selections_overlap(&target, &overlapping));
    assert!(!body_selections_overlap(&target, &disjoint));
}

#[test]
fn historical_vertex_selection_requires_input_state_membership() {
    use crate::features::{
        DatumPointConstruction, Feature, FeatureDefinition, FeatureId, FeatureInputTopology,
        VertexSelection,
    };
    use crate::ids::{FeatureInputTopologyId, HistoricalVertexId};
    use crate::schema::EntitySchema;

    let feature_id = FeatureId("test:model:feature#datum-point".into());
    let state_id = FeatureInputTopologyId("test:model:feature-input#datum-point".into());
    let historical_vertex = HistoricalVertexId("test:model:historical-vertex#local".into());
    let mut ir = CadIr::empty();
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            id: state_id.clone(),
            input_of: feature_id.clone(),
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: vec![historical_vertex.clone()],
            native_ref: None,
        });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPoint {
            position: crate::math::Point3::new(1.0, 2.0, 3.0),
            construction: Some(Box::new(DatumPointConstruction::Vertex {
                vertex: VertexSelection::Historical {
                    state: state_id.clone(),
                    vertex: historical_vertex,
                    native: "vertex:local".into(),
                },
            })),
        },
        native_ref: None,
    });

    let mut references = Vec::new();
    ir.model.features[0].visit_references(&mut |reference| references.push(reference.target));
    assert_eq!(references, vec![state_id.0]);

    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));

    let missing = "test:model:historical-vertex#missing";
    let FeatureDefinition::DatumPoint {
        construction: Some(construction),
        ..
    } = &mut ir.model.features[0].definition
    else {
        unreachable!("test feature is a constructed datum point")
    };
    let DatumPointConstruction::Vertex {
        vertex: VertexSelection::Historical { vertex, .. },
    } = construction.as_mut()
    else {
        unreachable!("test datum point uses a historical vertex")
    };
    *vertex = HistoricalVertexId(missing.into());
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
fn three_point_datum_plane_requires_distinct_vertices_from_one_input_topology() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, FeatureInputTopology, VertexSelection,
    };
    use crate::ids::{FeatureInputTopologyId, HistoricalVertexId};

    let feature_id = FeatureId("test:model:feature#three-point-plane".into());
    let first_state = FeatureInputTopologyId("test:model:feature-input#three-point-plane-a".into());
    let second_state =
        FeatureInputTopologyId("test:model:feature-input#three-point-plane-b".into());
    let vertices = [
        HistoricalVertexId("test:model:historical-vertex#1".into()),
        HistoricalVertexId("test:model:historical-vertex#2".into()),
        HistoricalVertexId("test:model:historical-vertex#3".into()),
    ];
    let other_vertex = HistoricalVertexId("test:model:historical-vertex#4".into());
    let historical = |state: &FeatureInputTopologyId, vertex: &HistoricalVertexId, native: &str| {
        VertexSelection::Historical {
            state: state.clone(),
            vertex: vertex.clone(),
            native: native.into(),
        }
    };

    let mut ir = CadIr::empty();
    ir.model.feature_input_topologies.extend([
        FeatureInputTopology {
            id: first_state.clone(),
            input_of: feature_id.clone(),
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: vertices.to_vec(),
            native_ref: None,
        },
        FeatureInputTopology {
            id: second_state.clone(),
            input_of: feature_id.clone(),
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: vec![other_vertex.clone()],
            native_ref: None,
        },
    ]);
    ir.model.features.push(Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumThreePointPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            points: Box::new([
                historical(&first_state, &vertices[0], "native:1"),
                historical(&first_state, &vertices[1], "native:2"),
                historical(&first_state, &vertices[2], "native:3"),
            ]),
        },
        native_ref: None,
    });

    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("three-point datum-plane")));

    let set_third = |ir: &mut CadIr, point| {
        let FeatureDefinition::DatumThreePointPlane { points, .. } =
            &mut ir.model.features[0].definition
        else {
            unreachable!("test feature is a three-point datum plane")
        };
        points[2] = point;
    };
    set_third(
        &mut ir,
        historical(&first_state, &vertices[0], "different-native-identity"),
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.message == "three-point datum plane requires three distinct vertices"
        }));

    set_third(
        &mut ir,
        historical(&second_state, &other_vertex, "native:4"),
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.message == "three-point datum-plane vertices use different input topologies"
        }));
}

#[test]
fn neutral_features_resolve_sketch_profile_and_path_operands() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        LinearTermination, PathRef, ProfileRef,
    };
    use crate::sketches::SketchId;

    let sketch = SketchId("synthetic:test:sketch#missing".into());
    let definitions = [
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch.clone()),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Sweep {
            section: crate::features::SweepSection::Profile(ProfileRef::Sketch(sketch.clone())),
            sections: Vec::new(),
            path: Some(PathRef::Sketch(sketch.clone())),
            mode: crate::features::SweepMode::Solid {
                op: BooleanOp::NewBody,
            },
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
        id: FeatureId("synthetic:test:feature#sketch-ref".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: definitions[1].clone(),
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
    let feature_id = FeatureId("synthetic:test:feature#invalid".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: Some(feature_id.clone()),
        dependencies: vec![feature_id.clone(), feature_id.clone()],
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: vec![
            FeatureSourceContent::Parameter(ParameterId("synthetic:test:parameter#missing".into())),
            FeatureSourceContent::Feature(feature_id.clone()),
        ],
        outputs: vec![BodyId("synthetic:test:body#missing".into())],
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(vec![FaceId("synthetic:test:face#profile-missing".into())]),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Faces(vec![FaceId(
                            "synthetic:test:face#termination-missing".into(),
                        )]),
                        offset: None,
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#duplicate-order".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Marker".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
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
        "repeats dependency",
        "missing content parameter",
        "content child",
    ] {
        assert!(
            report.findings.iter().any(|finding| {
                finding.entity.as_deref() == Some(feature_id.0.as_str())
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
    let owner = FeatureId("synthetic:test:feature#parameters".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    for (index, name) in ["Width", "Width"].into_iter().enumerate() {
        ir.model.parameters.push(DesignParameter {
            id: ParameterId(format!("synthetic:test:parameter#{index}")),
            owner: Some(owner.clone()),
            ordinal: 0,
            name: name.into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
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
    let owner = FeatureId("synthetic:test:feature#dependency-owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let first = ParameterId("synthetic:test:parameter#first".into());
    let second = ParameterId("synthetic:test:parameter#second".into());
    for (id, ordinal, dependencies) in [
        (first.clone(), 0, vec![second.clone()]),
        (
            second,
            1,
            vec![ParameterId("synthetic:test:parameter#missing".into())],
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
            dependencies,
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
    let owner = FeatureId("synthetic:test:feature#consumer".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let document = ParameterId("synthetic:test:parameter#document".into());
    ir.model.parameters.push(DesignParameter {
        id: document.clone(),
        owner: None,
        ordinal: 0,
        name: "Width".into(),
        expression: "60 mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("synthetic:test:parameter#owned".into()),
        owner: Some(owner),
        ordinal: 0,
        name: "Distance".into(),
        expression: "Width / 2".into(),
        display: None,
        value: None,
        dependencies: vec![document],
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).findings.is_empty());
}

#[test]
fn offset_plane_references_form_an_acyclic_graph_independent_of_list_order() {
    use crate::features::{DatumPlaneReference, Feature, FeatureDefinition, FeatureId, Length};

    let mut ir = unit_cube();
    let principal = FeatureId("synthetic:test:feature#principal".into());
    let feature = |id: &str, ordinal: u64, definition: FeatureDefinition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:feature#offset",
        0,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(principal.clone())),
            distance: Length(5.0),
        },
    ));
    ir.model.features.push(feature(
        principal.0.as_str(),
        1,
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
    ));
    ir.finalize();

    let report = validate_neutral(&ir, Vec::new());
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.message.contains("datum-plane reference cycle")));

    let offset = ir.model.features[0].id.clone();
    ir.model.features[1].definition = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Feature(offset)),
        distance: Length(5.0),
    };
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("datum-plane reference cycle")));
}

#[test]
fn feature_extent_magnitudes_are_validated() {
    use crate::features::{
        Angle, AngularTermination, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature,
        FeatureDefinition, FeatureId, Length, LinearTermination, ProfileRef,
        RevolutionConstruction, RevolveExtent,
    };

    let side = |termination: LinearTermination| ExtrudeSide {
        termination,
        draft: None,
        offset: None,
    };
    for extent in [
        ExtrudeExtent::OneSided {
            side: side(LinearTermination::Blind {
                length: Length(0.0),
            }),
        },
        ExtrudeExtent::TwoSided {
            first: side(LinearTermination::Blind {
                length: Length(1.0),
            }),
            second: side(LinearTermination::Blind {
                length: Length(f64::NAN),
            }),
        },
    ] {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#invalid-extent".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Extrude {
                profile: ProfileRef::Native("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: crate::features::ExtrudeStart::ProfilePlane,
                extent,
                op: BooleanOp::NewBody,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        });
        assert!(validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message == "feature extent magnitude is invalid"));
    }

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#invalid-angle".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                profile: None,
                axis: None,
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: Angle(-1.0) },
                }),
                axis_reference: None,
                solid: None,
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: BooleanOp::NewBody,
        },
        native_ref: None,
    });
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "feature extent magnitude is invalid"));
}

#[test]
fn block_placement_must_be_proper_rigid() {
    use crate::features::{BooleanOp, Feature, FeatureDefinition, FeatureId, Length};

    let mut rotated = crate::transform::Transform::identity();
    rotated.rows[0][0] = 0.0;
    rotated.rows[0][1] = -1.0;
    rotated.rows[1][0] = 1.0;
    rotated.rows[1][1] = 0.0;
    assert!(rotated.is_proper_rigid());

    for placement in [
        {
            let mut placement = crate::transform::Transform::identity();
            placement.rows[0][0] = f64::NAN;
            placement
        },
        {
            let mut placement = crate::transform::Transform::identity();
            placement.rows[3][0] = 1.0;
            placement
        },
        {
            let mut placement = crate::transform::Transform::identity();
            placement.rows[0][0] = 2.0;
            placement
        },
        {
            let mut placement = crate::transform::Transform::identity();
            placement.rows[0][1] = 0.25;
            placement
        },
        {
            let mut placement = crate::transform::Transform::identity();
            placement.rows[0][0] = -1.0;
            placement
        },
    ] {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#invalid-block-placement".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Block {
                dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
                placement: Some(placement),
                op: BooleanOp::NewBody,
            },
            native_ref: None,
        });
        assert!(validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message == "block placement is invalid"));
    }
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
    let source = FeatureId("synthetic:test:feature#0-vertex-source".into());
    ir.model.features.push(Feature {
        id: source.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPoint {
            position: Point3::new(0.0, 0.0, 0.0),
            construction: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#1-extrude".into()),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Native("test:profile".into()),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToVertex {
                        vertex: VertexSelection::Generated {
                            vertex: GeneratedVertexRef {
                                feature: source.clone(),
                                local_id: "vertex-0".into(),
                            },
                            native: "test:vertex-selection".into(),
                        },
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });

    let message = "generated termination vertex is invalid";
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
    let extrude = ir.model.features[1].id.clone();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("synthetic:test:configuration#vertex".into()),
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
                suppressed: false,
                dependencies: Vec::new(),
                outputs: Vec::new(),
                definition: ir.model.features[1].definition.clone(),
            },
        )]),
        native_ref: None,
    });
    ir.model.features[1].dependencies.push(source.clone());
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
    let configuration_message = format!(
        "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
        extrude.0, source.0
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == configuration_message));

    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&extrude)
        .expect("configured extrude");
    state.dependencies.push(source);
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&extrude)
        .expect("configured extrude");
    let FeatureDefinition::Extrude { extent, .. } = &mut state.definition else {
        unreachable!()
    };
    let ExtrudeExtent::OneSided { side } = extent else {
        unreachable!()
    };
    let LinearTermination::ToVertex {
        vertex: VertexSelection::Generated { native, .. },
    } = &mut side.termination
    else {
        unreachable!()
    };
    native.clear();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.message == "configuration generated termination vertex is invalid"
        }));
    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&extrude)
        .expect("configured extrude");
    let FeatureDefinition::Extrude { extent, .. } = &mut state.definition else {
        unreachable!()
    };
    let ExtrudeExtent::OneSided { side } = extent else {
        unreachable!()
    };
    side.termination = LinearTermination::Blind {
        length: crate::features::Length(f64::NAN),
    };
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| { finding.message == "configuration feature extent magnitude is invalid" }));
}

#[test]
fn body_combine_requires_exactly_one_resolved_target() {
    use crate::features::{BodySelection, BooleanOp, Feature, FeatureDefinition, FeatureId};
    use crate::ids::BodyId;

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#invalid-combine-target".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Combine {
            target: BodySelection::Bodies(vec![
                body.clone(),
                BodyId("synthetic:test:body#other-target".into()),
            ]),
            tools: BodySelection::Bodies(vec![body]),
            op: BooleanOp::Join,
            keep_tools: false,
        },
        native_ref: None,
    });
    let findings = validate_neutral(&ir, Vec::new()).findings;
    for message in [
        "body combine target is invalid",
        "body combine operands overlap",
    ] {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}

#[test]
fn feature_operand_roles_must_be_disjoint() {
    use crate::features::{
        BodySelection, BodyTrimSide, FaceSelection, Feature, FeatureDefinition, FeatureId, Length,
        RadiusSpec,
    };

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let body_key = body.0.clone();
    let face = ir.model.faces[0].id.clone();
    for (ordinal, definition) in [
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Faces(vec![face.clone()]),
            second_faces: FaceSelection::Faces(vec![face]),
            radius: RadiusSpec::Constant {
                radius: Length(1.0),
            },
        },
        FeatureDefinition::TrimBodies {
            targets: BodySelection::Local {
                bodies: vec![body_key.clone()],
                native: "test:selection#targets".into(),
            },
            tools: BodySelection::Local {
                bodies: vec![body_key.clone()],
                native: "test:selection#tools".into(),
            },
            keep: BodyTrimSide::Forward,
        },
        FeatureDefinition::SectionShape {
            first: BodySelection::Local {
                bodies: vec![body_key.clone()],
                native: "test:selection#first".into(),
            },
            second: BodySelection::Local {
                bodies: vec![body_key.clone()],
                native: "test:selection#second".into(),
            },
            approximate: Some(false),
        },
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Faces(vec![ir.model.faces[0].id.clone()]),
            replacements: FaceSelection::Faces(vec![ir.model.faces[0].id.clone()]),
        },
        FeatureDefinition::SewBodies {
            bodies: BodySelection::Local {
                bodies: vec![body_key],
                native: "test:selection#sew".into(),
            },
            gap_tolerance: None,
        },
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#overlap-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    for message in [
        "face blend supports overlap",
        "body trim operands overlap",
        "section operands overlap",
        "replacement face operands overlap",
        "sew requires at least two bodies",
    ] {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}

#[test]
fn pattern_feature_seeds_must_be_declared_dependencies() {
    use crate::features::{Feature, FeatureDefinition, FeatureId, PatternKind, PatternSeed};

    let mut ir = unit_cube();
    let seed = FeatureId("synthetic:test:feature#pattern-seed".into());
    ir.model.features.push(Feature {
        id: seed.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPoint {
            position: Point3::new(0.0, 0.0, 0.0),
            construction: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#pattern".into()),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Feature(seed.clone())],
            pattern: PatternKind::Mirror {
                plane_origin: Point3::new(0.0, 0.0, 0.0),
                plane_normal: Vector3::new(1.0, 0.0, 0.0),
            },
        },
        native_ref: None,
    });
    let message = format!(
        "pattern omits seed feature `{}` from its dependencies",
        seed.0
    );
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));

    ir.model.features[1].dependencies.push(seed);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == message));
}

#[test]
fn definition_references_must_be_declared_dependencies_in_every_configuration() {
    use crate::features::{
        BooleanOp, ConfigurationBodies, ConfigurationFeatureState, ConfigurationId,
        DatumPlaneReference, DesignConfiguration, ExtrudeDirection, ExtrudeExtent, ExtrudeSide,
        ExtrudeStart, Feature, FeatureDefinition, FeatureId, GeneratedCurveRef, Length,
        LinearTermination, PatternKind, PatternSeed, ProfileRef,
    };
    use std::collections::{BTreeMap, HashSet};

    let mut ir = unit_cube();
    let source = FeatureId("synthetic:test:feature#0-source".into());
    let offset = FeatureId("synthetic:test:feature#1-offset".into());
    let derived = FeatureId("synthetic:test:feature#2-derived".into());
    let pattern = FeatureId("synthetic:test:feature#3-pattern".into());
    let block = FeatureId("synthetic:test:feature#4-block".into());
    let instance = FeatureId("synthetic:test:feature#5-instance".into());
    let profile = FeatureId("synthetic:test:feature#6-profile-consumer".into());
    let feature = |id, ordinal, definition| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features = vec![
        feature(
            source.clone(),
            0,
            FeatureDefinition::DatumPlane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        feature(
            offset.clone(),
            1,
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature(source.clone())),
                distance: Length(5.0),
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
                pattern: PatternKind::Mirror {
                    plane_origin: Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                },
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
                profile: ProfileRef::Generated {
                    curves: vec![GeneratedCurveRef {
                        feature: source.clone(),
                        local_id: "curve-0".into(),
                    }],
                    native: "synthetic:test:profile-selection".into(),
                },
                direction: ExtrudeDirection::ProfileNormal,
                start: ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: Length(5.0),
                        },
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::NewBody,
                direction_source: None,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    ir.model.features[2].dependencies.push(source.clone());
    ir.model.features[3].dependencies.push(source.clone());
    ir.model.features[6].dependencies.push(source.clone());
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("synthetic:test:configuration#offset-plane".into()),
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
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: ir.model.features[index].definition.clone(),
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
        source.0
    )));
    assert!(findings.contains(&format!(
        "sketch block instance omits block feature `{}` from its dependencies",
        block.0
    )));
    for feature in [&offset, &derived, &pattern, &profile] {
        assert!(findings.contains(&format!(
            "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
            feature.0, source.0
        )));
    }
    assert!(findings.contains(&format!(
        "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
        instance.0, block.0
    )));

    ir.model.features[1].dependencies.push(source.clone());
    ir.model.features[5].dependencies.push(block.clone());
    for feature in [&offset, &derived, &pattern, &profile] {
        ir.model.configurations[0]
            .feature_states
            .get_mut(feature)
            .expect("configuration feature state")
            .dependencies
            .push(source.clone());
    }
    ir.model.configurations[0]
        .feature_states
        .get_mut(&instance)
        .expect("block-instance state")
        .dependencies
        .push(block);
    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&offset)
        .expect("offset-plane state");
    let FeatureDefinition::DatumOffsetPlane { distance, .. } = &mut state.definition else {
        unreachable!()
    };
    *distance = Length(f64::NAN);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| { finding.message == "configuration datum-plane offset is invalid" }));
    let state = ir.model.configurations[0]
        .feature_states
        .get_mut(&offset)
        .expect("offset-plane state");
    let FeatureDefinition::DatumOffsetPlane { distance, .. } = &mut state.definition else {
        unreachable!()
    };
    *distance = Length(5.0);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:#?}", report.findings);
}

#[test]
fn resolved_datum_geometry_must_be_finite_and_coherent() {
    use crate::features::{Feature, FeatureDefinition, FeatureId};

    let definitions = [
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 1.0),
        },
        FeatureDefinition::DatumAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 0.0),
        },
        FeatureDefinition::DatumPoint {
            position: Point3::new(f64::NAN, 0.0, 0.0),
            construction: None,
        },
        FeatureDefinition::DatumPoint {
            position: Point3::new(0.0, 0.0, 0.0),
            construction: Some(Box::new(
                crate::features::DatumPointConstruction::DistanceOnEdge {
                    edge: crate::features::EdgeSelection::Unresolved,
                    fraction: 1.5,
                },
            )),
        },
    ];
    let mut ir = unit_cube();
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#invalid-datum-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    for message in [
        "datum-plane frame is invalid",
        "datum-axis frame is invalid",
        "datum-point position is invalid",
        "datum-point path fraction is invalid",
    ] {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}

#[test]
fn explicit_extrusion_direction_must_be_nonzero() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        LinearTermination, ProfileRef,
    };

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#invalid-extrude-direction".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Native("profile".into()),
            direction: ExtrudeDirection::Explicit(Vector3::new(0.0, 0.0, 0.0)),
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(1.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "extrusion direction is invalid"));
}

#[test]
fn extrusion_side_drafts_are_validated() {
    use crate::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        Length, LinearTermination, ProfileRef,
    };

    let side = |length: f64, draft: Option<Angle>| ExtrudeSide {
        termination: LinearTermination::Blind {
            length: Length(length),
        },
        draft,
        offset: None,
    };
    for (extent, expected_invalid) in [
        (
            ExtrudeExtent::TwoSided {
                first: side(1.0, None),
                second: side(2.0, Some(Angle(0.25))),
            },
            false,
        ),
        (
            ExtrudeExtent::TwoSided {
                first: side(1.0, None),
                second: side(2.0, Some(Angle(f64::NAN))),
            },
            true,
        ),
        (
            ExtrudeExtent::Symmetric {
                side: side(1.0, Some(Angle(std::f64::consts::FRAC_PI_2))),
            },
            true,
        ),
    ] {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#side-draft".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Extrude {
                profile: ProfileRef::Native("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: crate::features::ExtrudeStart::ProfilePlane,
                extent,
                op: BooleanOp::NewBody,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        });
        let has_draft_finding = validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message == "extrusion draft is invalid");
        assert_eq!(has_draft_finding, expected_invalid);
    }
}

#[test]
fn generated_body_selection_must_name_a_declared_producer_result() {
    use crate::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureResultTopology,
        GeneratedBodyRef,
    };
    use crate::ids::FeatureResultTopologyId;

    let mut ir = CadIr::empty();
    let producer = FeatureId("synthetic:test:feature#0-producer".into());
    ir.model.features.push(Feature {
        id: producer.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "producer".into(),
            parameters: BTreeMap::default(),
            properties: BTreeMap::default(),
        },
        native_ref: None,
    });
    ir.model
        .feature_result_topologies
        .push(FeatureResultTopology {
            id: FeatureResultTopologyId("synthetic:test:feature-result-topology#producer".into()),
            output_of: producer.clone(),
            bodies: vec!["body#declared".into()],
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
            native_ref: None,
        });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#1-consumer".into()),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: vec![producer.clone()],
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Generated {
                bodies: vec![GeneratedBodyRef {
                    feature: producer,
                    local_id: "body#declared".into(),
                }],
                native: "synthetic:native-selection#0".into(),
            },
        },
        native_ref: None,
    });

    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let FeatureDefinition::BaseFeature {
        bodies: BodySelection::Generated { bodies, .. },
    } = &mut ir.model.features[1].definition
    else {
        panic!("test consumer must retain its generated body selection");
    };
    bodies[0].local_id = "body#undeclared".into();
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

    let asset_id = AssetId("synthetic:test:asset#reference-image".into());
    let feature_id = FeatureId("synthetic:test:feature#reference-image".into());
    let mut ir = CadIr::empty();
    ir.model.assets.push(Asset {
        id: asset_id.clone(),
        name: Some("reference.png".into()),
        media_type: Some("image/png".into()),
        content: AssetContent::Embedded {
            data: vec![1, 2, 3],
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::ReferenceImage {
            asset: asset_id,
            visible: true,
            mirror_u: false,
            mirror_v: false,
            origin: Point3::new(0.0, 0.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            v_axis: Vector3::new(0.0, 1.0, 0.0),
            bounds: [Point2::new(-10.0, -5.0), Point2::new(10.0, 5.0)],
            opacity: Some(0.75),
        },
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
        finding.entity.as_deref() == Some(feature_id.0.as_str())
            && finding.message.contains("reference-image asset")
    }));

    let FeatureDefinition::ReferenceImage { ref mut v_axis, .. } = ir.model.features[0].definition
    else {
        unreachable!();
    };
    *v_axis = Vector3::new(1.0, 0.0, 0.0);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(feature_id.0.as_str())
            && finding.message == "reference-image placement is invalid"
    }));
}

#[test]
fn decals_require_valid_assets_faces_and_opacity() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::features::{DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureId};

    let asset_id = AssetId("synthetic:test:asset#decal".into());
    let feature_id = FeatureId("synthetic:test:feature#decal".into());
    let mut ir = unit_cube();
    let face_id = ir.model.faces[0].id.clone();
    ir.model.assets.push(Asset {
        id: asset_id.clone(),
        name: Some("decal.png".into()),
        media_type: Some("image/png".into()),
        content: AssetContent::Embedded {
            data: vec![1, 2, 3],
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Decal {
            asset: asset_id,
            faces: FaceSelection::Faces(vec![face_id]),
            mapping: DecalMapping::FitToFaces,
            opacity: Some(0.75),
        },
        native_ref: None,
    });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).is_ok());

    let FeatureDefinition::Decal {
        ref mut opacity, ..
    } = ir.model.features.last_mut().unwrap().definition
    else {
        unreachable!();
    };
    *opacity = Some(2.0);
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(feature_id.0.as_str())
            && finding.message == "decal opacity is invalid"
    }));
}
