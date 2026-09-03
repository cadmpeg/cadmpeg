// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::math::{Point3, Vector3};
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn polygon_constraints_round_trip_and_require_distinct_members() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let sketch = SketchId("synthetic:test:sketch#polygon".into());
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    let members = (0..3)
        .map(|ordinal| SketchEntityId(format!("synthetic:test:polygon-point#{ordinal}")))
        .collect::<Vec<_>>();
    ir.model.sketch_entities.extend(
        members
            .iter()
            .enumerate()
            .map(|(ordinal, id)| SketchEntity {
                id: id.clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(ordinal as f64, 0.0),
                },
            }),
    );
    let constraint = SketchConstraintId("synthetic:test:polygon-constraint#0".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: SketchConstraintDefinition::Polygon {
            entities: members.clone(),
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
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.sketch_constraints,
        ir.model.sketch_constraints
    );

    ir.model.sketch_constraints[0].definition = SketchConstraintDefinition::Polygon {
        entities: vec![members[0].clone(), members[1].clone(), members[0].clone()],
    };
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint.0.as_str())
            && finding.message.contains("three distinct members")
    }));
}

#[test]
fn locus_aware_sketch_constraints_round_trip_and_validate_geometry() {
    use crate::features::{Length, ParameterId};
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
        SketchDistanceMeasurement, SketchDistancePair, SketchEntity, SketchEntityId,
        SketchGeometry, SketchId, SketchLocus, SketchOffsetPair,
    };

    let entity = SketchEntityId("synthetic:test:entity#0".into());
    let parameter = ParameterId("synthetic:test:parameter#0".into());
    let definitions = vec![
        SketchConstraintDefinition::Disabled,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Start(entity.clone()),
                SketchLocus::Center(entity.clone()),
            ],
        },
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Start(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::End(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinition::Offset {
            pairs: vec![SketchOffsetPair {
                source: entity.clone(),
                result: entity.clone(),
                source_reversed: false,
            }],
            distance: Length(2.0),
            parameter: Some(parameter.clone()),
            parameter_factor: Some(-1.0),
        },
        SketchConstraintDefinition::Concentric {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Curvature {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Collinear {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            axis: entity.clone(),
        },
        SketchConstraintDefinition::Radius {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::RepeatedRadius {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::Diameter {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::RepeatedDiameter {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::EqualDistance {
            first: SketchDistancePair {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            },
            second: SketchDistancePair {
                first: SketchLocus::Center(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            },
        },
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::HorizontalLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
        },
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::VerticalLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
        },
        SketchConstraintDefinition::RepeatedDistance {
            measurements: vec![SketchDistanceMeasurement::Horizontal {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            }],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::RepeatedLength {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::ParallelLineSetDistance {
            first: vec![entity.clone()],
            second: vec![entity.clone()],
            parameter,
        },
        SketchConstraintDefinition::SnellsLaw {
            incident: SketchLocus::Start(entity.clone()),
            refracted: SketchLocus::End(entity.clone()),
            interface: entity.clone(),
            parameter: ParameterId("synthetic:test:parameter#0".into()),
        },
        SketchConstraintDefinition::Weight {
            entity: entity.clone(),
            parameter: ParameterId("synthetic:test:parameter#0".into()),
        },
        SketchConstraintDefinition::InternalAlignment {
            helper: entity.clone(),
            parent: entity.clone(),
            alignment: crate::sketches::SketchInternalAlignment::BsplineControlPoint,
            index: Some(2),
        },
        SketchConstraintDefinition::Group {
            elements: vec![SketchLocus::Entity(entity.clone())],
        },
        SketchConstraintDefinition::Text {
            elements: vec![SketchLocus::Entity(entity.clone())],
            text: "R42".into(),
            font: Some("Mono".into()),
            is_text_height: false,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<SketchConstraintDefinition>>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    let sketch = SketchId("synthetic:test:sketch#locus".into());
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: entity.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    });
    let constraint_id = SketchConstraintId("synthetic:test:constraint#locus".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint_id.clone(),
        sketch,
        definition: SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Center(entity.clone()),
                SketchLocus::Start(entity),
            ],
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
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint_id.0.as_str())
            && finding.check == Check::GeometricConsistency
    }));
    ir.model.sketch_entities[0].geometry = SketchGeometry::Native {
        native_kind: "center-bearing-curve".into(),
    };
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint_id.0.as_str())
            && finding.check == Check::GeometricConsistency
    }));
}

#[test]
fn coordinate_equation_constraints_round_trip_and_validate_geometry() {
    use crate::features::Length;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
        SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
    };

    let sketch = SketchId("synthetic:test:sketch#coordinate-equations".into());
    let first = SketchEntityId("synthetic:test:coordinate-point#first".into());
    let second = SketchEntityId("synthetic:test:coordinate-point#second".into());
    let midpoint = SketchEntityId("synthetic:test:coordinate-point#midpoint".into());
    let constraints = [
        (
            SketchConstraintId("synthetic:test:constraint#point-coordinates".into()),
            SketchConstraintDefinition::PointCoordinateValues {
                point: SketchLocus::Entity(midpoint.clone()),
                values: [Length(2.0), Length(1.0)],
            },
        ),
        (
            SketchConstraintId("synthetic:test:constraint#mean-u".into()),
            SketchConstraintDefinition::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::U,
                value: Length(2.0),
            },
        ),
        (
            SketchConstraintId("synthetic:test:constraint#mean-v".into()),
            SketchConstraintDefinition::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::V,
                value: Length(1.0),
            },
        ),
    ];
    let mut ir = unit_cube();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.sketch_entities.extend(
        [
            (first.clone(), Point2::new(0.0, 0.0)),
            (second.clone(), Point2::new(4.0, 2.0)),
            (midpoint.clone(), Point2::new(2.0, 1.0)),
        ]
        .into_iter()
        .map(|(id, position)| SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point { position },
        }),
    );
    ir.model
        .sketch_constraints
        .extend(constraints.iter().map(|(id, definition)| SketchConstraint {
            id: id.clone(),
            sketch: sketch.clone(),
            definition: definition.clone(),
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
        }));
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.findings.iter().any(|finding| {
        finding
            .entity
            .as_deref()
            .is_some_and(|entity| entity.starts_with("synthetic:test:constraint#"))
    }));
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.sketch_constraints,
        ir.model.sketch_constraints
    );

    let midpoint_entity = ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id == midpoint)
        .unwrap();
    midpoint_entity.geometry = SketchGeometry::Point {
        position: Point2::new(3.0, 1.0),
    };
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some("synthetic:test:constraint#point-coordinates")
            && finding.check == Check::Counts
    }));
}

#[test]
fn sketch_regions_round_trip_with_explicit_boundary_roles() {
    use crate::features::{ProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};
    use crate::sketches::{SketchEntityId, SketchId};

    let profile = ProfileRef::SketchRegions {
        sketch: SketchId("synthetic:test:sketch#region".into()),
        regions: vec![
            SketchProfileRegion::Loops {
                outer: 2,
                holes: vec![3, 5],
            },
            SketchProfileRegion::Loops {
                outer: 8,
                holes: Vec::new(),
            },
            SketchProfileRegion::Trimmed {
                outer_boundary: vec![SketchProfileBoundaryUse {
                    entity: SketchEntityId("synthetic:test:sketch-entity#curve".into()),
                    parameter_range: [0.25, 0.75],
                    reversed: true,
                }],
                hole_boundaries: Vec::new(),
            },
        ],
    };
    let json = serde_json::to_value(&profile).expect("serialize sketch regions");
    assert_eq!(json["kind"], "sketch_regions");
    assert_eq!(json["value"]["regions"][0]["outer"], 2);
    assert_eq!(
        json["value"]["regions"][0]["holes"],
        serde_json::json!([3, 5])
    );
    assert!(json["value"]["regions"][1].get("holes").is_none());
    assert_eq!(
        json["value"]["regions"][2]["outer_boundary"][0]["parameter_range"],
        serde_json::json!([0.25, 0.75])
    );
    assert_eq!(
        json["value"]["regions"][2]["outer_boundary"][0]["reversed"],
        true
    );
    assert_eq!(
        serde_json::from_value::<ProfileRef>(json).expect("deserialize sketch regions"),
        profile
    );
}

#[test]
fn spatial_sketch_geometry_round_trips_and_validates() {
    use crate::features::{DesignParameter, Length, ParameterId, ParameterValue};
    use crate::sketches::{
        SketchConstraintId, SpatialSketch, SpatialSketchConstraint,
        SpatialSketchConstraintDefinition, SpatialSketchEntity, SpatialSketchEntityId,
        SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId, SpatialSketchProfile,
    };

    let mut ir = unit_cube();
    let sketch = SpatialSketchId("synthetic:test:spatial-sketch#one".into());
    let circle = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#circle".into());
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch.clone(),
        name: Some("3D path".into()),
        configuration: None,
        visible: Some(false),
        profiles: vec![SpatialSketchProfile {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            boundary: vec![SpatialSketchEntityUse {
                entity: circle.clone(),
                reversed: false,
            }],
        }],
        native_ref: None,
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: circle.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length(4.0),
        },
    });
    let parallel_line =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#parallel-line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: parallel_line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(0.0, 2.0f64.sqrt(), -2.0f64.sqrt()),
            end: Point3::new(1.0, 1.0 + 2.0f64.sqrt(), 1.0 - 2.0f64.sqrt()),
        },
    });
    let collinear_line =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#collinear-line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: collinear_line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(2.0, 2.0, 2.0),
            end: Point3::new(3.0, 3.0, 3.0),
        },
    });
    let repeated_parallel_line =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#repeated-parallel-line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: repeated_parallel_line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(2.0, 2.0 + 2.0f64.sqrt(), 2.0 - 2.0f64.sqrt()),
            end: Point3::new(3.0, 3.0 + 2.0f64.sqrt(), 3.0 - 2.0f64.sqrt()),
        },
    });
    let distance = ParameterId("synthetic:test:parameter#spatial-distance".into());
    ir.model.parameters.push(DesignParameter {
        id: distance.clone(),
        owner: None,
        ordinal: 0,
        name: "spatial_distance".into(),
        expression: "2 mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(2.0))),
        dependencies: Vec::new(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    let line_length = ParameterId("synthetic:test:parameter#spatial-line-length".into());
    ir.model.parameters.push(DesignParameter {
        id: line_length.clone(),
        owner: None,
        ordinal: 1,
        name: "spatial_line_length".into(),
        expression: "sqrt(3) mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(3.0f64.sqrt()))),
        dependencies: Vec::new(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    let surface = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#surface".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: surface.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
        },
    });
    let surface_point =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#surface-point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: surface_point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.0),
        },
    });
    let line = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(1.0, 1.0, 1.0),
        },
    });
    let point = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.5),
        },
    });
    let measured_point =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#measured-point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: measured_point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 2.5),
        },
    });
    let coincident_point =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#coincident-point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: coincident_point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.5),
        },
    });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#group".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::SplineGroup {
                entities: vec![line.clone(), circle.clone()],
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId(
                "synthetic:test:spatial-sketch-constraint#repeated-parallel-distance".into(),
            ),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::RepeatedParallelLineDistance {
                pairs: vec![
                    crate::sketches::SpatialSketchEntityPair {
                        first: line.clone(),
                        second: parallel_line.clone(),
                    },
                    crate::sketches::SpatialSketchEntityPair {
                        first: collinear_line.clone(),
                        second: repeated_parallel_line,
                    },
                ],
                parameter: distance.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#offset".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Offset {
                sources: vec![line.clone()],
                results: vec![parallel_line.clone()],
                normal: Vector3::new(
                    -2.0 / 6.0f64.sqrt(),
                    1.0 / 6.0f64.sqrt(),
                    1.0 / 6.0f64.sqrt(),
                ),
                distance: Length(2.0),
                parameter: Some(distance.clone()),
                parameter_factor: Some(1.0),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId(
                "synthetic:test:spatial-sketch-constraint#line-set-distance".into(),
            ),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::ParallelLineSetDistance {
                first: vec![line.clone(), collinear_line],
                second: vec![parallel_line.clone()],
                parameter: distance.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#line-length".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::LineLength {
                entity: line.clone(),
                parameter: line_length.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId(
                "synthetic:test:spatial-sketch-constraint#repeated-line-length".into(),
            ),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::RepeatedLineLength {
                entities: vec![line.clone(), parallel_line.clone()],
                parameter: line_length,
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#point-surface".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::PointOnSurface {
                point: surface_point,
                surface,
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#coincident".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Coincident {
                first: point.clone(),
                second: coincident_point.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#symmetric".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Symmetric {
                first: point.clone(),
                second: coincident_point,
                axis: line.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#midpoint".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Midpoint {
                point: point.clone(),
                entity: line.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId(
                "synthetic:test:spatial-sketch-constraint#point-distance".into(),
            ),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::PointDistance {
                first: point.clone(),
                second: measured_point,
                parameter: distance.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#direction".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::ParallelToDirection {
                entity: line.clone(),
                direction: Vector3::new(
                    1.0 / 3.0f64.sqrt(),
                    1.0 / 3.0f64.sqrt(),
                    1.0 / 3.0f64.sqrt(),
                ),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#distance".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::ParallelLineDistance {
                first: line.clone(),
                second: parallel_line,
                parameter: distance.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#tangent".into()),
            sketch,
            definition: SpatialSketchConstraintDefinition::Tangent {
                first: line,
                second: circle,
            },
            native_ref: None,
        });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).findings.is_empty());
    let mut overlapping_offset = ir.clone();
    let SpatialSketchConstraintDefinition::Offset {
        sources, results, ..
    } = &mut overlapping_offset
        .model
        .spatial_sketch_constraints
        .iter_mut()
        .find(|constraint| constraint.id.0.ends_with("#offset"))
        .expect("spatial offset constraint")
        .definition
    else {
        panic!("spatial offset definition");
    };
    results[0] = sources[0].clone();
    assert!(validate_neutral(&overlapping_offset, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid spatial constraint arity"));
    let mut non_curve_offset = ir.clone();
    let point_entity = non_curve_offset
        .model
        .spatial_sketch_entities
        .iter()
        .find(|entity| matches!(entity.geometry, SpatialSketchGeometry::Point { .. }))
        .expect("spatial point")
        .id
        .clone();
    let SpatialSketchConstraintDefinition::Offset { sources, .. } = &mut non_curve_offset
        .model
        .spatial_sketch_constraints
        .iter_mut()
        .find(|constraint| constraint.id.0.ends_with("#offset"))
        .expect("spatial offset constraint")
        .definition
    else {
        panic!("spatial offset definition");
    };
    sources[0] = point_entity;
    assert!(validate_neutral(&non_curve_offset, Vec::new())
        .findings
        .iter()
        .any(
            |finding| finding.message == "spatial offset source and result members must be curves"
        ));
    let mut invalid_distance = ir.clone();
    invalid_distance
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == distance)
        .expect("spatial distance parameter")
        .value = Some(ParameterValue::Length(Length(3.0)));
    let invalid_distance_findings = validate_neutral(&invalid_distance, Vec::new()).findings;
    assert!(invalid_distance_findings.iter().any(|finding| finding
        .message
        .contains("spatial distance requires parallel lines")));
    assert!(invalid_distance_findings
        .iter()
        .any(|finding| finding.message == "spatial offset distance does not match its parameter"));
    let json = ir.to_canonical_json().expect("serialize spatial sketch");
    let decoded = CadIr::from_json(&json).expect("deserialize spatial sketch");
    assert_eq!(decoded.model.spatial_sketches, ir.model.spatial_sketches);
    assert_eq!(
        decoded.model.spatial_sketch_entities,
        ir.model.spatial_sketch_entities
    );
    assert_eq!(
        decoded.model.spatial_sketch_constraints,
        ir.model.spatial_sketch_constraints
    );
}

#[test]
fn spatial_sketch_paths_round_trip_through_json() {
    use crate::features::PathRef;
    use crate::sketches::{SpatialSketchEntityId, SpatialSketchId};

    let path = PathRef::SpatialSketchCurves {
        sketch: SpatialSketchId("synthetic:test:spatial-sketch#0".into()),
        curves: vec![SpatialSketchEntityId(
            "synthetic:test:spatial-sketch-entity#0".into(),
        )],
    };
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);

    let native = PathRef::SpatialSketchSelection {
        sketch: SpatialSketchId("synthetic:test:spatial-sketch#0".into()),
        selections: vec!["native:path-selection#0".into()],
    };
    let json = serde_json::to_string(&native).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), native);
}

fn pattern_direction(axis: [f64; 2]) -> crate::sketches::SketchPatternDirection {
    crate::sketches::SketchPatternDirection {
        direction: axis,
        spacing: crate::features::Length(2.0),
        spacing_parameter: None,
        span_parameter: None,
        count_parameter: None,
    }
}

#[test]
fn rectangular_pattern_derives_counts_and_indices_on_the_wire() {
    use crate::sketches::{
        SketchConstraintDefinition, SketchEntityId, SketchPatternInstance, SketchRectangularPattern,
    };

    let pattern = SketchRectangularPattern::new(
        [pattern_direction([1.0, 0.0]), pattern_direction([0.0, 1.0])],
        vec![
            vec![SketchPatternInstance {
                entities: vec![SketchEntityId("test:sketch-entity#0".into())],
            }],
            vec![SketchPatternInstance {
                entities: vec![SketchEntityId("test:sketch-entity#1".into())],
            }],
        ],
    )
    .unwrap();
    let definition = SketchConstraintDefinition::RectangularPattern { pattern };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["directions"][0]["count"], 2);
    assert_eq!(wire["directions"][1]["count"], 1);
    assert_eq!(wire["instances"][0]["indices"], serde_json::json!([0, 0]));
    assert_eq!(wire["instances"][1]["indices"], serde_json::json!([1, 0]));
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut split_count = wire.clone();
    split_count["directions"][0]["count"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(split_count).is_err());
    let mut displaced = wire;
    displaced["instances"][1]["indices"] = serde_json::json!([0, 1]);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(displaced).is_err());
}

#[test]
fn circular_pattern_derives_count_and_indices_on_the_wire() {
    use crate::features::Angle;
    use crate::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance, SketchConstraintDefinition,
        SketchEntityId,
    };

    let pattern = SketchCircularPattern::new(
        SketchEntityId("test:sketch-entity#center".into()),
        Angle(1.0),
        None,
        None,
        vec![
            SketchCircularPatternInstance {
                angle: Angle(0.0),
                entities: vec![SketchEntityId("test:sketch-entity#0".into())],
            },
            SketchCircularPatternInstance {
                angle: Angle(1.0),
                entities: vec![SketchEntityId("test:sketch-entity#1".into())],
            },
        ],
    )
    .unwrap();
    let definition = SketchConstraintDefinition::CircularPattern { pattern };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["count"], 2);
    assert_eq!(wire["instances"][0]["index"], 0);
    assert_eq!(wire["instances"][1]["index"], 1);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut split_count = wire.clone();
    split_count["count"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(split_count).is_err());
    let mut displaced = wire;
    displaced["instances"][1]["index"] = serde_json::json!(0);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(displaced).is_err());
}
