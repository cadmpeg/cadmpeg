// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn encoder_writes_source_less_curved_sketches() {
    use cadmpeg_ir::features::{
        Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SketchId("synthetic:test:sketch#curves".into());
    let geometries = vec![
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(8.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Arc {
            center: Point2::new(16.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(std::f64::consts::TAU),
        },
        SketchGeometry::Ellipse {
            center: Point2::new(0.0, 8.0),
            major_angle: Angle(0.4),
            major_radius: Length(3.0),
            minor_radius: Length(1.5),
            start_angle: None,
            end_angle: None,
        },
        SketchGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(6.0, 6.0),
                Point2::new(10.0, 10.0),
                Point2::new(6.0, 6.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        },
        SketchGeometry::Line {
            start: Point2::new(6.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(18.0, 0.0),
            end: Point2::new(14.0, 0.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(24.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
        },
        SketchGeometry::Line {
            start: Point2::new(24.0, -2.0),
            end: Point2::new(24.0, 2.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(8.0, 0.0),
            radius: Length(3.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Line {
            start: Point2::new(5.0, 0.0),
            end: Point2::new(11.0, 0.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(40.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        },
        SketchGeometry::Line {
            start: Point2::new(40.0, 2.0),
            end: Point2::new(42.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(30.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(34.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(30.0, 4.0),
        },
        SketchGeometry::Point {
            position: Point2::new(41.0, 1.0),
        },
        SketchGeometry::Circle {
            center: Point2::new(8.0, 2.0),
            radius: Length(2.0),
        },
        SketchGeometry::Line {
            start: Point2::new(50.0, 0.0),
            end: Point2::new(54.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(54.0, 4.0),
            end: Point2::new(50.0, 4.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(52.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Arc {
            center: Point2::new(52.0, 4.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(std::f64::consts::TAU),
        },
        SketchGeometry::Circle {
            center: Point2::new(8.0, 0.0),
            radius: Length(2.0),
        },
        SketchGeometry::Ellipse {
            center: Point2::new(60.0, 0.0),
            major_angle: Angle(0.0),
            major_radius: Length(3.0),
            minor_radius: Length(1.5),
            start_angle: Some(Angle(0.0)),
            end_angle: Some(Angle(std::f64::consts::FRAC_PI_2)),
        },
        SketchGeometry::Line {
            start: Point2::new(60.0, 1.5),
            end: Point2::new(63.0, 0.0),
        },
    ];
    let entity_ids = geometries
        .into_iter()
        .enumerate()
        .map(|(index, geometry)| {
            let id = SketchEntityId(format!("synthetic:test:sketch-entity#curve-{index:02}"));
            ir.model.sketch_entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry,
            });
            id
        })
        .collect::<Vec<_>>();
    let profile = |indices: &[usize]| {
        indices
            .iter()
            .map(|index| SketchEntityUse {
                entity: entity_ids[*index].clone(),
                reversed: false,
            })
            .collect()
    };
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Curves".into()),
        configuration: Some("Main".into()),
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            profile(&[0]),
            profile(&[1, 5]),
            profile(&[2, 6]),
            profile(&[7, 8]),
            profile(&[9, 10]),
            profile(&[11, 12]),
            profile(&[17]),
            profile(&[18, 20]),
            profile(&[19, 21]),
            profile(&[3]),
            profile(&[4]),
            profile(&[22]),
            profile(&[23, 24]),
        ],
        native_ref: None,
    });
    let feature_id = FeatureId("synthetic:test:feature#curves".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Curves".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    let distance_parameter = ParameterId("synthetic:test:parameter#00-distance".into());
    let point_line_parameter = ParameterId("synthetic:test:parameter#01-point-line".into());
    let line_line_parameter = ParameterId("synthetic:test:parameter#02-line-line".into());
    let horizontal_parameter = ParameterId("synthetic:test:parameter#03-horizontal".into());
    let vertical_parameter = ParameterId("synthetic:test:parameter#04-vertical".into());
    let angle_parameter = ParameterId("synthetic:test:parameter#05-angle".into());
    let radius_parameter = ParameterId("synthetic:test:parameter#06-radius".into());
    let diameter_parameter = ParameterId("synthetic:test:parameter#07-diameter".into());
    for (id, ordinal, name, expression, display, value) in [
        (
            distance_parameter.clone(),
            0,
            "D10",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            point_line_parameter.clone(),
            1,
            "D11",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            line_line_parameter.clone(),
            2,
            "D12",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            horizontal_parameter.clone(),
            3,
            "H1",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            vertical_parameter.clone(),
            4,
            "V1",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            angle_parameter.clone(),
            5,
            "A1",
            "90deg",
            None,
            ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2)),
        ),
        (
            radius_parameter.clone(),
            6,
            "R1",
            "R2mm",
            Some(DimensionDisplay::Radius),
            ParameterValue::Length(Length(2.0)),
        ),
        (
            diameter_parameter.clone(),
            7,
            "DIA1",
            "<MOD-DIAM>4mm",
            Some(DimensionDisplay::Diameter),
            ParameterValue::Length(Length(4.0)),
        ),
    ] {
        ir.model.parameters.push(DesignParameter {
            id,
            owner: Some(feature_id.clone()),
            ordinal,
            name: name.into(),
            expression: expression.into(),
            display,
            value: Some(value),
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#arc-angle".into()),
        sketch: sketch_id.clone(),
        definition: SketchConstraintDefinition::ArcAngle {
            entity: entity_ids[1].clone(),
            angle: Angle(std::f64::consts::PI),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#arc-angle-ellipse".into()),
        sketch: sketch_id.clone(),
        definition: SketchConstraintDefinition::EllipseAngle {
            entity: entity_ids[23].clone(),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    for (suffix, definition) in [
        (
            "collinear",
            SketchConstraintDefinition::Collinear {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "concentric",
            SketchConstraintDefinition::Concentric {
                first: entity_ids[1].clone(),
                second: entity_ids[9].clone(),
            },
        ),
        (
            "coradial",
            SketchConstraintDefinition::Coradial {
                first: entity_ids[1].clone(),
                second: entity_ids[22].clone(),
            },
        ),
        (
            "dimension-angle",
            SketchConstraintDefinition::Angle {
                first: entity_ids[5].clone(),
                second: entity_ids[8].clone(),
                parameter: angle_parameter,
            },
        ),
        (
            "dimension-diameter",
            SketchConstraintDefinition::Diameter {
                entity: entity_ids[17].clone(),
                parameter: diameter_parameter,
            },
        ),
        (
            "dimension-horizontal",
            SketchConstraintDefinition::HorizontalDistance {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
                parameter: horizontal_parameter,
            },
        ),
        (
            "dimension-line-line",
            SketchConstraintDefinition::Distance {
                entities: vec![entity_ids[18].clone(), entity_ids[19].clone()],
                parameter: line_line_parameter,
            },
        ),
        (
            "dimension-point-line",
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(entity_ids[15].clone()),
                second: SketchLocus::Entity(entity_ids[5].clone()),
                parameter: point_line_parameter,
            },
        ),
        (
            "dimension-vertical",
            SketchConstraintDefinition::VerticalDistance {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[15].clone()),
                parameter: vertical_parameter,
            },
        ),
        (
            "distance",
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
                parameter: distance_parameter,
            },
        ),
        (
            "equal-arcs",
            SketchConstraintDefinition::Equal {
                first: entity_ids[1].clone(),
                second: entity_ids[2].clone(),
            },
        ),
        (
            "equal-lines",
            SketchConstraintDefinition::Equal {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "horizontal-points",
            SketchConstraintDefinition::HorizontalPoints {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
            },
        ),
        (
            "midpoint",
            SketchConstraintDefinition::Midpoint {
                point: SketchLocus::Entity(entity_ids[16].clone()),
                entity: entity_ids[12].clone(),
            },
        ),
        (
            "parallel",
            SketchConstraintDefinition::Parallel {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "perpendicular",
            SketchConstraintDefinition::Perpendicular {
                first: entity_ids[5].clone(),
                second: entity_ids[8].clone(),
            },
        ),
        (
            "radius",
            SketchConstraintDefinition::Radius {
                entity: entity_ids[0].clone(),
                parameter: radius_parameter,
            },
        ),
        (
            "tangent",
            SketchConstraintDefinition::Tangent {
                first: entity_ids[5].clone(),
                second: entity_ids[17].clone(),
            },
        ),
        (
            "vertical-points",
            SketchConstraintDefinition::VerticalPoints {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[15].clone()),
            },
        ),
    ] {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#{suffix}")),
            sketch: sketch_id.clone(),
            definition,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 29);
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Coradial { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::EllipseAngle { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::DistanceLoci { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Radius { .. }
        )));
    assert!(decoded.ir().model.parameters.iter().any(|parameter| {
        parameter.name == "D10" && parameter.value == Some(ParameterValue::Length(Length(4.0)))
    }));
    assert!(decoded.ir().model.parameters.iter().any(|parameter| {
        parameter.name == "R1" && parameter.display == Some(DimensionDisplay::Radius)
    }));
    for name in ["D11", "D12", "H1", "V1", "A1", "DIA1"] {
        assert!(
            decoded
                .ir()
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.name == name),
            "missing regenerated {name} dimension parameter"
        );
    }
    for expected in ["line-line", "horizontal", "vertical", "angle", "diameter"] {
        assert!(
            decoded
                .ir()
                .model
                .sketch_constraints
                .iter()
                .any(|constraint| matches!(
                    (expected, &constraint.definition),
                    ("line-line", SketchConstraintDefinition::Distance { .. })
                        | (
                            "horizontal",
                            SketchConstraintDefinition::HorizontalDistance { .. }
                        )
                        | (
                            "vertical",
                            SketchConstraintDefinition::VerticalDistance { .. }
                        )
                        | ("angle", SketchConstraintDefinition::Angle { .. })
                        | ("diameter", SketchConstraintDefinition::Diameter { .. })
                )),
            "missing regenerated {expected} dimension"
        );
    }
    let native = sldprt_native(decoded.ir());
    let circle_relation = native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .find(|relation| {
            relation.family == crate::records::FeatureInputRelationFamily::CircleDiameter
                && relation.parameter_scalar_ref.as_deref()
                    == decoded
                        .ir()
                        .model
                        .parameters
                        .iter()
                        .find(|parameter| parameter.name == "DIA1")
                        .and_then(|parameter| parameter.native_ref.as_deref())
        })
        .expect("diameter relation instance");
    let [operand] = circle_relation.operands.as_slice() else {
        panic!("one diameter operand");
    };
    let marker = native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .find(|marker| Some(marker.id.as_str()) == operand.entity_ref.as_deref())
        .expect("resolved diameter marker");
    assert_eq!(marker.kind, crate::records::SketchInputKind::LineOrCircle);
    assert_ne!(marker.local_id, Some(u32::from(operand.entity_index)));
    assert!(native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .flat_map(|relation| &relation.operands)
        .all(|operand| operand.entity_ref.is_some()));
    assert!(native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .flat_map(|relation| &relation.operands)
        .filter(|operand| {
            matches!(
                operand.kind,
                crate::records::FeatureInputOperandKind::D6
                    | crate::records::FeatureInputOperandKind::E1
                    | crate::records::FeatureInputOperandKind::Native(0x8dcb | 0x8dda)
            )
        })
        .any(|operand| {
            native.feature_input_lanes[0]
                .sketch_entities
                .iter()
                .find(|marker| Some(marker.id.as_str()) == operand.entity_ref.as_deref())
                .is_some_and(|marker| marker.local_id != Some(u32::from(operand.entity_index)))
        }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::ArcAngle {
                    angle: Angle(value),
                    ..
                } if (value - std::f64::consts::PI).abs() < 1.0e-12
            )
        }));
    for expected in [
        crate::records::SketchRelationKind::Parallel,
        crate::records::SketchRelationKind::Perpendicular,
        crate::records::SketchRelationKind::Equal,
        crate::records::SketchRelationKind::Collinear,
        crate::records::SketchRelationKind::Concentric,
        crate::records::SketchRelationKind::HorizontalPoints,
        crate::records::SketchRelationKind::VerticalPoints,
        crate::records::SketchRelationKind::Midpoint,
        crate::records::SketchRelationKind::Tangent,
    ] {
        assert!(sldprt_native(decoded.ir())
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .any(|marker| marker.kind == crate::records::SketchInputKind::Relation(expected)));
    }
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Parallel { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Perpendicular { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Collinear { .. }
        )));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Concentric { .. }
        )));
    assert!(
        decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| matches!(
                constraint.definition,
                SketchConstraintDefinition::Equal { .. }
            ))
            .count()
            >= 2
    );
    for definition in [
        "horizontal_points",
        "vertical_points",
        "midpoint",
        "tangent",
    ] {
        assert!(decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                matches!(
                    (&constraint.definition, definition),
                    (
                        SketchConstraintDefinition::HorizontalPoints { .. },
                        "horizontal_points"
                    ) | (
                        SketchConstraintDefinition::VerticalPoints { .. },
                        "vertical_points"
                    ) | (SketchConstraintDefinition::Midpoint { .. }, "midpoint")
                        | (SketchConstraintDefinition::Tangent { .. }, "tangent")
                )
            }));
    }
    assert_eq!(
        decoded
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Circle { .. }))
            .count(),
        3
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
            .count(),
        7
    );
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(entity.geometry, SketchGeometry::Ellipse { .. })));
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(entity.geometry, SketchGeometry::Nurbs { .. })));

    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D10")
        .expect("source distance parameter");
    parameter.expression = "5mm".into();
    parameter.value = Some(ParameterValue::Length(Length(5.0)));
    let error = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("not satisfied by measured geometry"));
}

#[test]
fn encoder_binds_multiple_source_less_sketches_by_object_id() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    for (ordinal, name) in ["Profile", "Profile"].into_iter().enumerate() {
        let sketch_id = SketchId(format!("synthetic:test:sketch#named-{ordinal}"));
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: Some(name.into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, ordinal as f64),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: None,
        });
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId(format!("synthetic:test:sketch-entity#named-{ordinal}")),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(ordinal as f64, ordinal as f64 + 1.0),
            },
        });
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#named-{ordinal}")),
            ordinal: ordinal as u64,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
            },
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketches.len(), 2);
    assert_eq!(
        decoded
            .ir()
            .model
            .sketches
            .iter()
            .filter_map(|sketch| sketch.name.as_deref())
            .collect::<Vec<_>>(),
        ["Profile", "Profile"]
    );
    let bound = decoded
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    assert_ne!(bound[0], bound[1]);
}

#[test]
fn encoder_writes_source_less_native_features() {
    use cadmpeg_ir::features::{
        Angle, BodySelection, BooleanOp, ChamferSpec, EdgeSelection, FaceMotion, FaceSelection,
        Feature, FeatureDefinition, FeatureId, HoleKind, Length, PatternKind, RadiusSpec,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let seed_id = FeatureId("sldprt:model:feature#generated:0".into());
    ir.model.features.push(Feature {
        id: seed_id.clone(),
        ordinal: 0,
        name: Some("Boss".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "BossExtrude".into(),
            parameters: BTreeMap::from([("Depth".into(), "25mm".into())]),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let definitions = vec![
        FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Resolved {
                    edges: vec![ir.model.edges[0].id.clone()],
                    native: "edge-a,edge-b".into(),
                },
                radius: RadiusSpec::Constant {
                    radius: Length(3.0),
                },
                tangency_weight: None,
            }],
        },
        FeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: EdgeSelection::Native("edge-c".into()),
                spec: ChamferSpec::TwoDistances {
                    first: Length(1.0),
                    second: Length(2.0),
                },
            }],
            flip_direction: false,
        },
        FeatureDefinition::Shell {
            bodies: None,
            removed_faces: FaceSelection::Resolved {
                faces: vec![ir.model.faces[0].id.clone()],
                native: "face-a".into(),
            },
            thickness: Some(Length(1.5)),
            outward: Some(true),
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        },
        FeatureDefinition::Draft {
            faces: FaceSelection::Native("face-b".into()),
            neutral_plane: FaceSelection::Native("face-c".into()),
            parting_tool: None,
            pull_direction: Some(Vector3::new(0.0, 0.0, 1.0)),
            pull_plane: None,
            angle: Some(Angle(0.2)),
            outward: Some(false),
        },
        FeatureDefinition::Combine {
            target: BodySelection::Resolved {
                bodies: vec![ir.model.bodies[0].id.clone()],
                native: "body-a".into(),
            },
            tools: BodySelection::Native("body-b,body-c".into()),
            op: BooleanOp::Join,
            keep_tools: false,
        },
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native("face-d".into()),
            heal: true,
        },
        FeatureDefinition::MoveFace {
            faces: FaceSelection::Native("face-e".into()),
            motion: FaceMotion::Rotate {
                axis_origin: Point3::new(1.0, 2.0, 3.0),
                axis_dir: Vector3::new(0.0, 1.0, 0.0),
                angle: Angle(0.4),
            },
        },
        FeatureDefinition::Dome {
            faces: FaceSelection::Native("face-f".into()),
            height: Some(Length(4.0)),
            elliptical: Some(true),
            reverse: Some(false),
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Native("face-g".into())),
            position: None,
            direction: None,
            placements: vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: Point3::new(3.0, 4.0, 5.0),
                direction: Vector3::new(0.0, 0.0, -1.0),
            }],
            kind: HoleKind::Countersink {
                diameter: Length(8.0),
                angle: Angle(1.4),
            },
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(Termination::Blind {
                length: Length(20.0),
            }),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
    ];
    for (index, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#direct-{index}")),
            ordinal: index as u64 + 1,
            name: Some(format!("Direct {index}")),
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
    let patterns = [
        PatternKind::Linear {
            direction: Some(Vector3::new(1.0, 0.0, 0.0)),
            spacing: Length(10.0),
            count: 3,
            second: Some(cadmpeg_ir::features::LinearPatternDirection {
                direction: Vector3::new(0.0, 1.0, 0.0),
                spacing: Length(20.0),
                count: 4,
            }),
        },
        PatternKind::Circular {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_dir: Vector3::new(0.0, 0.0, 1.0),
            angle: Angle(std::f64::consts::TAU),
            count: 6,
        },
        PatternKind::Mirror {
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    for (index, pattern) in patterns.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#pattern-{index}")),
            ordinal: index as u64 + 10,
            name: Some(format!("Pattern {index}")),
            suppressed: Some(false),
            parent: None,
            dependencies: vec![seed_id.clone()],
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: vec![cadmpeg_ir::features::PatternSeed::Feature(seed_id.clone())],
                pattern,
            },
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan.blocks.iter().any(|block| {
        block
            .section
            .as_deref()
            .is_some_and(|section| section.starts_with("Contents/Keywords-"))
    }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(25.0),
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert_eq!(
        sldprt_native(decoded.ir()).feature_histories[0].features[0].xml_tag,
        "Extrusion"
    );
    let native_features = &sldprt_native(decoded.ir()).feature_histories[0].features;
    let source_ids = native_features
        .iter()
        .map(|feature| {
            feature
                .source_id
                .as_deref()
                .expect("generated features have source ids")
                .parse::<u32>()
                .expect("generated feature source ids are numeric")
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(source_ids.len(), native_features.len());
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Fillet { .. })));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                second: Some(cadmpeg_ir::features::LinearPatternDirection {
                    direction: Vector3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0
                    },
                    spacing: Length(20.0),
                    count: 4,
                }),
                ..
            },
            ..
        }
    )));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Chamfer { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Shell { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Draft { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Combine { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::DeleteFace { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::MoveFace { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Dome { .. })));
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Hole { .. })));
    assert_eq!(
        decoded
            .ir()
            .model
            .features
            .iter()
            .filter(|feature| matches!(feature.definition, FeatureDefinition::Pattern { .. }))
            .count(),
        3
    );
}

#[test]
fn semantic_writer_round_trips_flex_operations() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, FlexMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Flex Name="Bend" Type="Flex" id="44" Mode="Bending" Axis="0,1,0"><Dimension Name="Angle">30deg</Dimension></Flex></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Flex { axis, mode } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed flex feature");
        };
        assert_eq!(*axis, Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)));
        assert!(matches!(
            mode,
            FlexMode::Bending { angle }
                if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
        ));
        *mode = FlexMode::Twisting { angle: Angle(0.75) };
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].xml_tag,
        "Flex"
    );
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Flex {
            axis,
            mode: FlexMode::Twisting { angle },
        } if *axis == Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
            && (angle.0 - 0.75).abs() < 1.0e-12
    ));
}

#[test]
fn semantic_writer_round_trips_all_flex_modes() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, FlexMode, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Flex Name="Bend" Type="Flex" id="1" Mode="Bending" Axis="1,0,0"><Dimension Name="Angle">10deg</Dimension></Flex>
            <Flex Name="Twist" Type="Flex" id="2" Mode="Twisting" Axis="0,1,0"><Dimension Name="Angle">20deg</Dimension></Flex>
            <Flex Name="Taper" Type="Flex" id="3" Mode="Tapering" Axis="0,0,1"><Dimension Name="Factor">1.5</Dimension></Flex>
            <Flex Name="Stretch" Type="Flex" id="4" Mode="Stretching" Axis="1,1,0"><Dimension Name="Distance">8mm</Dimension></Flex>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    for feature in &mut decoded.ir_mut().model.features {
        if let FeatureDefinition::Flex { mode, .. } = &mut feature.definition {
            *mode = match feature.name.as_deref().unwrap() {
                "Bend" => FlexMode::Bending { angle: Angle(0.1) },
                "Twist" => FlexMode::Twisting { angle: Angle(0.2) },
                "Taper" => FlexMode::Tapering { factor: 2.0 },
                "Stretch" => FlexMode::Stretching {
                    distance: Length(12.0),
                },
                name => panic!("unexpected flex {name}"),
            };
        } else {
            panic!("untyped flex feature");
        }
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let modes = regenerated
        .ir()
        .model
        .features
        .iter()
        .map(|feature| &feature.definition)
        .collect::<Vec<_>>();
    assert!(
        matches!(modes[0], FeatureDefinition::Flex { mode: FlexMode::Bending { angle }, .. } if (angle.0 - 0.1).abs() < 1.0e-12)
    );
    assert!(
        matches!(modes[1], FeatureDefinition::Flex { mode: FlexMode::Twisting { angle }, .. } if (angle.0 - 0.2).abs() < 1.0e-12)
    );
    assert!(
        matches!(modes[2], FeatureDefinition::Flex { mode: FlexMode::Tapering { factor }, .. } if (*factor - 2.0).abs() < 1.0e-12)
    );
    assert!(
        matches!(modes[3], FeatureDefinition::Flex { mode: FlexMode::Stretching { distance }, .. } if (distance.0 - 12.0).abs() < 1.0e-12)
    );
}

#[test]
fn semantic_writer_retains_partial_native_flex_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, FlexForm, FlexMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Flex Name="Axis" Type="Flex" id="1" Mode="Bending" Axis="0,0,0"><Dimension Name="Angle">10deg</Dimension></Flex>
            <Flex Name="Angle" Type="Flex" id="2" Mode="Twisting" Axis="0,1,0"><Dimension Name="Angle">NaNrad</Dimension></Flex>
            <Flex Name="Taper" Type="Flex" id="3" Mode="Tapering" Axis="0,0,1"><Dimension Name="Factor">0</Dimension></Flex>
            <Flex Name="Stretch" Type="Flex" id="4" Mode="Stretching" Axis="1,0,0"><Dimension Name="Distance">infmm</Dimension></Flex>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert_eq!(decoded.ir().model.features.len(), 4);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Flex {
            axis: None,
            mode: FlexMode::Bending { .. },
        }
    ));
    for (index, form) in [FlexForm::Twisting, FlexForm::Tapering, FlexForm::Stretching]
        .into_iter()
        .enumerate()
    {
        assert!(matches!(
            decoded.ir().model.features[index + 1].definition,
            FeatureDefinition::Flex {
                axis: Some(_),
                mode: FlexMode::Unresolved {
                    form: Some(actual),
                    angle: None,
                    factor: None,
                    distance: None,
                },
            } if actual == form
        ));
    }

    for index in 0..4 {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                &detached,
                decoded.source_fidelity(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("unresolved flex construction"));
    }

    for (index, feature) in decoded.ir_mut().model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed flex {}", index + 1));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].properties["Axis"], "0,0,0");
    assert_eq!(native[1].parameters["Angle"], "NaNrad");
    assert_eq!(native[2].parameters["Factor"], "0");
    assert_eq!(native[3].parameters["Distance"], "infmm");
}

#[test]
fn semantic_writer_preserves_native_feature_leaf_text() {
    use crate::records::FeatureContent;
    use cadmpeg_ir::features::FeatureSourceContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><MacroFeature Name="Custom" Type="Macro" id="70">prefix<Dimension Name="A">1</Dimension><Definition Name="Payload" Type="Definition" Language="expr">a &amp; b &lt; c</Definition>suffix<Dimension Name="B">2</Dimension></MacroFeature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native = sldprt_native(decoded.ir());
    let definition = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "Definition")
        .unwrap();
    assert_eq!(definition.text.as_deref(), Some("a & b < c"));
    assert_eq!(definition.properties["Language"], "expr");
    assert!(definition.tree_parent.is_some());
    let macro_feature = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "MacroFeature")
        .unwrap();
    assert_eq!(
        macro_feature.content,
        [
            FeatureContent::Text("prefix".into()),
            FeatureContent::Dimension("A".into()),
            FeatureContent::Feature(definition.id.clone()),
            FeatureContent::Text("suffix".into()),
            FeatureContent::Dimension("B".into()),
        ]
    );
    {
        let mut ir_edit = decoded.ir_mut();
        let neutral_macro = ir_edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.source_tag.as_deref() == Some("MacroFeature"))
            .unwrap();
        assert!(matches!(
            neutral_macro.source_content.as_slice(),
            [
                FeatureSourceContent::Text(prefix),
                FeatureSourceContent::Parameter(_),
                FeatureSourceContent::Feature(_),
                FeatureSourceContent::Text(suffix),
                FeatureSourceContent::Parameter(_),
            ] if prefix == "prefix" && suffix == "suffix"
        ));
        let FeatureSourceContent::Text(prefix) = &mut neutral_macro.source_content[0] else {
            unreachable!()
        };
        *prefix = "lead & more".into();
        let neutral_definition = ir_edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.source_tag.as_deref() == Some("Definition"))
            .unwrap();
        assert_eq!(neutral_definition.source_text.as_deref(), Some("a & b < c"));
        neutral_definition.source_tag = Some("FormulaPayload".into());
        neutral_definition.source_text = Some("x > 1 & y < 2".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let definition = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "FormulaPayload")
        .unwrap();
    assert_eq!(definition.text.as_deref(), Some("x > 1 & y < 2"));
    assert_eq!(definition.properties["Language"], "expr");
    assert!(definition.tree_parent.is_some());
    let neutral_definition = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("FormulaPayload"))
        .unwrap();
    assert_eq!(
        neutral_definition.source_text.as_deref(),
        Some("x > 1 & y < 2")
    );
    let macro_feature = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "MacroFeature")
        .unwrap();
    assert_eq!(
        macro_feature.content,
        [
            FeatureContent::Text("lead & more".into()),
            FeatureContent::Dimension("A".into()),
            FeatureContent::Feature(definition.id.clone()),
            FeatureContent::Text("suffix".into()),
            FeatureContent::Dimension("B".into()),
        ]
    );
}

#[test]
fn semantic_writer_removes_deleted_history_records() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Keep" SourceIndex="0"/><Configuration Name="Delete" SourceIndex="1"/><Feature Name="Keep" Type="Custom" id="80"/><Feature Name="Delete" Type="Custom" id="81"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded
        .ir_mut()
        .model
        .features
        .retain(|feature| feature.name.as_deref() == Some("Keep"));
    decoded
        .ir_mut()
        .model
        .configurations
        .retain(|configuration| configuration.name == "Keep");

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.features.len(), 1);
    assert_eq!(
        regenerated.ir().model.features[0].name.as_deref(),
        Some("Keep")
    );
    assert_eq!(regenerated.ir().model.configurations.len(), 1);
    assert_eq!(regenerated.ir().model.configurations[0].name, "Keep");
}

#[test]
fn semantic_writer_reorders_nested_history_records() {
    use crate::records::FeatureContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Folder Name="Parent" Type="Folder" id="90">prefix<Item Name="A" Type="Custom" id="91"/>middle<Item Name="B" Type="Custom" id="92"/></Folder></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    for feature in &mut decoded.ir_mut().model.features {
        match feature.name.as_deref() {
            Some("A") => feature.ordinal = 2,
            Some("B") => feature.ordinal = 1,
            _ => {}
        }
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let history = &native.feature_histories[0];
    let parent = history
        .features
        .iter()
        .find(|feature| feature.name == "Parent")
        .unwrap();
    let child_names = parent
        .content
        .iter()
        .filter_map(|item| match item {
            FeatureContent::Feature(id) => history
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .map(|feature| feature.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(child_names, ["B", "A"]);
    assert!(matches!(
        parent.content[0],
        FeatureContent::Text(ref text) if text == "prefix"
    ));
    assert!(matches!(
        parent.content[2],
        FeatureContent::Text(ref text) if text == "middle"
    ));
}
