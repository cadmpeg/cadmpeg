// SPDX-License-Identifier: Apache-2.0
//! Spatial-sketch write-back and semantic-write round-trip pins.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::{collections::BTreeMap, io::Cursor};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};
use cadmpeg_ir::transform::Transform;

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

fn source_less_spatial_line(start: Point3, end: Point3) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let sketch_id = SpatialSketchId::mint("synthetic:test:spatial-sketch#path").unwrap();
    let entity_id =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#line").unwrap();
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Spatial path".into()),
        configuration: Some("0".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            entity_id,
            sketch_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .unwrap(),
        ));
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:feature#spatial-path").expect("identity grammar"),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                sketch: Some(sketch_id),
            }),
        ),
        native_ref: None,
    });
    ir
}

fn check_spatial_point_line_distance_reaches_output(construction: bool, point_construction: bool) {
    use cadmpeg_ir::codec::write::Encoder;
    use cadmpeg_ir::features::{DesignParameter, Dimension, ParameterId, ParameterValue};
    use cadmpeg_ir::sketches::{
        SketchConstraintId, SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput,
    };

    let mut ir = source_less_spatial_line(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0));
    ir.model.spatial_sketch_entities[0].construction = construction;
    let sketch = ir.model.spatial_sketches[0].id.clone();
    let line = ir.model.spatial_sketch_entities[0].id().clone();
    let point = SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#point").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            point.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(3.0, 4.0, 0.0),
            })
            .unwrap(),
        ));
    let parameter = ParameterId::mint("synthetic:test:parameter#distance").unwrap();
    ir.model
        .spatial_sketch_entities
        .last_mut()
        .unwrap()
        .construction = point_construction;
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: parameter.clone(),
        owner: Some(ir.model.features[0].id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "4mm".into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(4.0).unwrap(),
        )),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:constraint#point-line").unwrap(),
            sketch,
            definition: SpatialSketchConstraintDefinitionInput::PointLineDistance {
                point,
                line,
                parameter: Dimension::Parameter(parameter),
            }
            .try_into()
            .unwrap(),
            native_ref: None,
        });
    let other_line =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#other-line").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            other_line.clone(),
            ir.model.spatial_sketches[0].id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(20.0, -5.0, 0.0),
                end: Point3::new(20.0, 5.0, 0.0),
            })
            .unwrap(),
        ));
    let third_parameter = ParameterId::mint("synthetic:test:parameter#third-distance").unwrap();
    ir.model
        .spatial_sketch_entities
        .last_mut()
        .unwrap()
        .construction = construction;
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: third_parameter.clone(),
        owner: Some(ir.model.features[0].id.clone()),
        ordinal: 2,
        name: "D3".into(),
        expression: "17mm".into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(17.0).unwrap(),
        )),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:constraint#other-line").unwrap(),
            sketch: ir.model.spatial_sketches[0].id.clone(),
            definition: SpatialSketchConstraintDefinitionInput::PointLineDistance {
                point: ir.model.spatial_sketch_entities[1].id().clone(),
                line: other_line,
                parameter: Dimension::Parameter(third_parameter),
            }
            .try_into()
            .unwrap(),
            native_ref: None,
        });
    let second_point =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#second-point").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            second_point.clone(),
            ir.model.spatial_sketches[0].id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(3.0, 7.0, 0.0),
            })
            .unwrap(),
        ));
    let second_parameter = ParameterId::mint("synthetic:test:parameter#second-distance").unwrap();
    ir.model
        .spatial_sketch_entities
        .last_mut()
        .unwrap()
        .construction = point_construction;
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: second_parameter.clone(),
        owner: Some(ir.model.features[0].id.clone()),
        ordinal: 1,
        name: "D2".into(),
        expression: "7mm".into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(7.0).unwrap(),
        )),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:constraint#second-point-line").unwrap(),
            sketch: ir.model.spatial_sketches[0].id.clone(),
            definition: SpatialSketchConstraintDefinitionInput::PointLineDistance {
                point: second_point,
                line: ir.model.spatial_sketch_entities[0].id().clone(),
                parameter: Dimension::Parameter(second_parameter),
            }
            .try_into()
            .unwrap(),
            native_ref: None,
        });
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for constraint in &ir.model.spatial_sketch_constraints {
        assert!(
            !plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(constraint.id.as_str())),
            "{:#?}",
            plan.report().losses
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| !lane.relation_instances.is_empty())
        .unwrap();
    assert_eq!(lane.relation_instances.len(), 3);
    let mut line_refs = BTreeMap::new();
    for relation in &lane.relation_instances {
        assert_eq!(
            relation.family,
            crate::records::FeatureInputRelationFamily::PointLineDistance
        );
        assert!(
            relation.operands[0].entity_ref.is_some(),
            "point operand resolves"
        );
        let line_ref = relation.operands[1]
            .entity_ref
            .as_deref()
            .expect("line operand resolves");
        assert!(lane
            .sketch_entities
            .iter()
            .any(|marker| marker.id() == line_ref
                && marker.kind() == crate::records::SketchInputKind::LineOrCircle));
        let scalar = lane
            .scalars
            .iter()
            .find(|scalar| Some(scalar.id.as_str()) == relation.parameter_scalar_ref())
            .unwrap();
        let name = lane
            .names
            .iter()
            .find(|name| name.id == scalar.name)
            .unwrap();
        line_refs.insert(name.value.as_str(), line_ref);
    }
    assert_eq!(line_refs["D1"], line_refs["D2"]);
    assert_ne!(line_refs["D1"], line_refs["D3"]);
    for (position, distance, line_start, line_end) in [
        (
            Point3::new(3.0, 4.0, 0.0),
            4.0,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ),
        (
            Point3::new(3.0, 7.0, 0.0),
            7.0,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ),
        (
            Point3::new(3.0, 4.0, 0.0),
            17.0,
            Point3::new(20.0, -5.0, 0.0),
            Point3::new(20.0, 5.0, 0.0),
        ),
    ] {
        assert!(output.ir().model.spatial_sketch_constraints.iter().any(|constraint| {
            let SpatialSketchConstraintDefinitionInput::PointLineDistance { point, line, parameter } = constraint.definition.kind() else { return false };
            let point_ok = output.ir().model.spatial_sketch_entities.iter().any(|entity| entity.id() == point && entity.construction == point_construction && matches!(entity.geometry.definition(), SpatialSketchGeometryDefinition::Point { position: actual } if actual.get() == position));
            let line_ok = output.ir().model.spatial_sketch_entities.iter().any(|entity| entity.id() == line && entity.construction == construction && matches!(entity.geometry.definition(), SpatialSketchGeometryDefinition::Line { start, end } if start.get() == line_start && end.get() == line_end));
            let value_ok = parameter.parameter().and_then(|id| output.ir().model.parameters.iter().find(|candidate| &candidate.id == id)).and_then(|parameter| parameter.value.as_ref()).is_some_and(|value| matches!(value, ParameterValue::Length(value) if value.get() == distance));
            point_ok && line_ok && value_ok
        }), "{:#?}", output.ir().model.spatial_sketch_constraints);
    }
    for fidelity in [Some(output.source_fidelity()), None] {
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(output.ir(), fidelity),
                TargetRequest::Inherit,
            )
            .unwrap();
        let mut rewritten = Vec::new();
        plan.write_to(&mut rewritten).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            decoded
                .ir()
                .model
                .spatial_sketch_constraints
                .iter()
                .filter(|constraint| matches!(
                    constraint.definition.kind(),
                    SpatialSketchConstraintDefinitionInput::PointLineDistance { .. }
                ))
                .count(),
            3
        );
    }
}

#[test]
fn source_less_spatial_point_line_distance_reaches_output() {
    check_spatial_point_line_distance_reaches_output(false, false);
}

#[test]
fn source_less_spatial_construction_line_distance_reaches_output() {
    check_spatial_point_line_distance_reaches_output(true, false);
}

#[test]
fn source_less_spatial_construction_point_distance_reaches_output() {
    check_spatial_point_line_distance_reaches_output(true, true);
}

#[test]
fn inactive_configuration_retains_its_spatial_sketch() {
    use cadmpeg_ir::codec::write::Encoder;
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, DistinctMembers,
    };

    let mut ir = source_less_spatial_line(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
    let feature = ir.model.features[0].id.clone();
    let alternate_id = SpatialSketchId::mint("synthetic:test:spatial-sketch#alternate").unwrap();
    ir.model.spatial_sketches.push(SpatialSketch {
        id: alternate_id.clone(),
        name: Some("Alternate path".into()),
        configuration: Some("1".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    let start = Point3::new(0.0, 2.0, 0.0);
    let end = Point3::new(1.0, 2.0, 0.0);
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#alternate").unwrap(),
            alternate_id.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .unwrap(),
        ));
    let bodies = DistinctMembers::try_from(
        ir.model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 0,
            source_index: Some(index),
            name: Some(format!("Configuration {index}")),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(bodies.clone()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].feature_states.insert(
        feature.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(FeatureDefinition::Operation(
                FeatureOperation::SpatialSketch {
                    sketch: Some(alternate_id),
                },
            )),
        },
    );
    ir.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(feature.as_str())
            && loss.message.contains("structural override")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| lane.configuration.as_deref() == Some("1"))
        .unwrap();
    for point in [start, end] {
        assert!(
            (0..lane.native_payload.len().saturating_sub(23)).any(|offset| {
                cadmpeg_core::decode::View::f64_le_at(&lane.native_payload, offset) == Some(point.x)
                    && cadmpeg_core::decode::View::f64_le_at(&lane.native_payload, offset + 8)
                        == Some(point.y)
                    && cadmpeg_core::decode::View::f64_le_at(&lane.native_payload, offset + 16)
                        == Some(point.z)
            })
        );
    }
    let owners = output
        .ir()
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. })
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 1);
    let configuration = output
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
        .unwrap();
    let Some(ConfigurationFeatureState::Definition { definition }) =
        configuration.feature_states.get(&owners[0].id)
    else {
        panic!(
            "alternate state has its owner: {:#?}",
            configuration.feature_states
        )
    };
    let FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
        sketch: Some(sketch),
    }) = definition.as_ref()
    else {
        panic!("alternate state has a spatial sketch")
    };
    assert!(output.ir().model.spatial_sketch_entities.iter().any(|entity| &entity.sketch == sketch && matches!(*entity.geometry.definition(), SpatialSketchGeometryDefinition::Line { start: actual_start, end: actual_end } if actual_start == start && actual_end == end)));
}

#[test]
fn retained_spatial_line_endpoint_edits_round_trip() {
    let mut first_encoding = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(
            &source_less_spatial_line(Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)),
            None,
        ),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut first_encoding))
    .expect("source-less spatial line should encode");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(first_encoding), &DecodeOptions::default())
        .expect("encoded spatial line should decode");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let replacement_start = Point3::new(-7.5, 8.25, 9.0);
    let replacement_end = Point3::new(10.0, -11.5, 12.75);
    decoded.ir_mut().model.spatial_sketch_entities[0].geometry =
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: replacement_start,
            end: replacement_end,
        })
        .unwrap();

    let mut second_encoding = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut second_encoding))
    .expect("edited retained spatial line should encode");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(second_encoding), &DecodeOptions::default())
        .expect("edited retained spatial line should decode")
        .into_parts()
        .0;

    assert!(
        matches!(*regenerated.model.spatial_sketch_entities[0].geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start, end }
                if start == replacement_start && end == replacement_end
        )
    );
}

#[test]
fn mutated_semantic_write_round_trips() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let expected_z = decoded.ir().model.points[0].position().get().z;
    let expected_bodies = decoded.ir().model.bodies.len();
    let expected_faces = decoded.ir().model.faces.len();

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .expect("mutated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert_eq!(round_trip.ir().model.bodies.len(), expected_bodies);
    assert_eq!(round_trip.ir().model.faces.len(), expected_faces);
    assert!(
        floats_agree(
            round_trip.ir().model.points[0].position().get().z,
            expected_z
        ),
        "mutated z drifted: got {} expected {}",
        round_trip.ir().model.points[0].position().get().z,
        expected_z
    );
}

#[test]
fn bake_transform_is_applied_and_output_stays_valid() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let original_x = decoded.ir().model.points[0].position().get().x;
    decoded.ir_mut().model.bodies[0].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .expect("translated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written translated triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(
        floats_agree(
            round_trip.ir().model.points[0].position().get().x,
            original_x + 10.0
        ),
        "baked translation drifted: got {} expected {}",
        round_trip.ir().model.points[0].position().get().x,
        original_x + 10.0
    );
}

#[test]
fn unowned_planar_sketch_does_not_gain_a_feature() {
    use cadmpeg_ir::codec::write::Encoder;
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/authored_nested_circle.sldprt"
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.features.is_empty());
    assert_eq!(decoded.ir().model.sketch_entities.len(), 1);
    let plan = SldprtCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.is_empty());
    assert_eq!(output.ir().model.sketch_entities.len(), 1);
    assert_eq!(
        output.ir().model.sketch_entities[0].geometry,
        decoded.ir().model.sketch_entities[0].geometry
    );
}

#[test]
fn solid_region_retains_multiple_shells() {
    use cadmpeg_ir::codec::write::Encoder;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let region = ir.model.regions[0].id.clone();
    let second = serde_json::to_string(&cadmpeg_ir::examples::unit_cube().unwrap())
        .unwrap()
        .replace("synthetic:cube:", "synthetic:secondcube:");
    let mut other: cadmpeg_ir::CadIr = serde_json::from_str(&second).unwrap();
    ir.model.regions[0]
        .shells
        .extend(other.model.regions[0].shells.clone());
    for shell in &mut other.model.shells {
        shell.region = region.clone();
    }
    macro_rules! append {
        ($($arena:ident),* $(,)?) => {$(ir.model.$arena.extend(other.model.$arena);)*};
    }
    append!(shells, faces, loops, coedges, edges, vertices, points, surfaces, curves);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.regions[0].shells.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 12);
}

#[test]
fn solid_region_retains_seven_shells() {
    use cadmpeg_ir::codec::write::Encoder;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let region = ir.model.regions[0].id.clone();
    for index in 1..7 {
        let source = serde_json::to_string(&cadmpeg_ir::examples::unit_cube().unwrap())
            .unwrap()
            .replace("synthetic:cube:", &format!("synthetic:cube{index}:"));
        let mut other: cadmpeg_ir::CadIr = serde_json::from_str(&source).unwrap();
        ir.model.regions[0]
            .shells
            .extend(other.model.regions[0].shells.clone());
        for shell in &mut other.model.shells {
            shell.region = region.clone();
        }
        macro_rules! append {
            ($($arena:ident),* $(,)?) => {$(ir.model.$arena.extend(other.model.$arena);)*};
        }
        append!(shells, faces, loops, coedges, edges, vertices, points, surfaces, curves);
    }
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.regions[0].shells.len(), 7);
    assert_eq!(result.ir().model.faces.len(), 42);
}

#[test]
fn sheet_region_retains_multiple_shells() {
    use cadmpeg_ir::codec::write::Encoder;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
    let region = ir.model.regions[0].id.clone();
    let source = serde_json::to_string(&cadmpeg_ir::examples::unit_cube().unwrap())
        .unwrap()
        .replace("synthetic:cube:", "synthetic:secondcube:");
    let mut other: cadmpeg_ir::CadIr = serde_json::from_str(&source).unwrap();
    ir.model.regions[0]
        .shells
        .extend(other.model.regions[0].shells.clone());
    for shell in &mut other.model.shells {
        shell.region = region.clone();
    }
    macro_rules! append {
        ($($arena:ident),* $(,)?) => {$(ir.model.$arena.extend(other.model.$arena);)*};
    }
    append!(shells, faces, loops, coedges, edges, vertices, points, surfaces, curves);
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.regions[0].shells.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 12);
}
