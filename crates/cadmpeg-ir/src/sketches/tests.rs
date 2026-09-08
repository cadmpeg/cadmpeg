// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::math::{Point3, Vector3};
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

const EPS_SPATIAL_LINE_BOUNDARY: f64 = 1.0e-12;
const EPS_SPATIAL_FRAME_BOUNDARY: f64 = 1.0e-9;

#[test]
fn sketch_entity_ids_are_checked_at_both_construction_boundaries() {
    use crate::math::{Point2, Point3};
    use crate::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchGeometryDefinition, SpatialSketchId,
    };

    let planar = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:sketch-entity#0").unwrap(),
        SketchId::mint("synthetic:test:sketch#0").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    )
    .with_construction(true)
    .with_native_ref(Some("native-planar".into()))
    .with_geometry_ref(Some("native-curve".into()))
    .with_endpoint_refs(vec!["native-point".into()]);
    let planar_wire = serde_json::to_value(&planar).unwrap();
    assert_eq!(
        serde_json::from_value::<SketchEntity>(planar_wire.clone()).unwrap(),
        planar
    );
    assert_eq!(planar_wire["id"], "synthetic:test:sketch-entity#0");
    let mut empty_planar = planar_wire;
    empty_planar["id"] = serde_json::Value::String(String::new());
    assert!(serde_json::from_value::<SketchEntity>(empty_planar)
        .unwrap_err()
        .to_string()
        .contains("identity is invalid"));

    let spatial = SpatialSketchEntity::new(
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#0").unwrap(),
        SpatialSketchId::mint("synthetic:test:spatial-sketch#0").unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
            position: Point3::new(1.0, 2.0, 3.0),
        })
        .unwrap(),
    )
    .with_construction(true)
    .with_native_ref(Some("native-spatial".into()))
    .with_geometry_ref(Some("native-curve".into()))
    .with_endpoint_refs(vec!["native-point".into()]);
    let spatial_wire = serde_json::to_value(&spatial).unwrap();
    assert_eq!(
        serde_json::from_value::<SpatialSketchEntity>(spatial_wire.clone()).unwrap(),
        spatial
    );
    assert_eq!(spatial_wire["id"], "synthetic:test:spatial-sketch-entity#0");
    let mut empty_spatial = spatial_wire;
    empty_spatial["id"] = serde_json::Value::String(String::new());
    assert!(serde_json::from_value::<SpatialSketchEntity>(empty_spatial)
        .unwrap_err()
        .to_string()
        .contains("identity is invalid"));

    for invalid in ["", "x", "a:b#c", "a:b:c#", "a:b:c#d#e", "a:b:c#d e"] {
        assert!(SketchId::mint(invalid).is_err());
        assert!(SketchEntityId::mint(invalid).is_err());
        assert!(SpatialSketchId::mint(invalid).is_err());
        assert!(SpatialSketchEntityId::mint(invalid).is_err());
        assert!(crate::sketches::SketchConstraintId::mint(invalid).is_err());
        let wire = serde_json::to_string(invalid).unwrap();
        assert!(serde_json::from_str::<SketchId>(&wire).is_err());
        assert!(serde_json::from_str::<SketchEntityId>(&wire).is_err());
        assert!(serde_json::from_str::<SpatialSketchId>(&wire).is_err());
        assert!(serde_json::from_str::<SpatialSketchEntityId>(&wire).is_err());
        assert!(serde_json::from_str::<crate::sketches::SketchConstraintId>(&wire).is_err());
    }
}

#[test]
fn polygon_constraints_round_trip_and_require_distinct_members() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    };

    let mut ir = unit_cube();
    let sketch = SketchId::mint("synthetic:test:sketch#polygon").unwrap();
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
        .map(|ordinal| {
            SketchEntityId::mint(format!("synthetic:test:polygon-point#{ordinal}")).unwrap()
        })
        .collect::<Vec<_>>();
    ir.model
        .sketch_entities
        .extend(members.iter().enumerate().map(|(ordinal, id)| {
            SketchEntity::new(
                id.clone(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(ordinal as f64, 0.0),
                })
                .unwrap(),
            )
        }));
    let constraint = SketchConstraintId::mint("synthetic:test:polygon-constraint#0").unwrap();
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: SketchConstraintDefinition::Polygon {
            polygon: crate::sketches::SketchPolygon::try_new(members.clone()).unwrap(),
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
}

#[test]
fn locus_aware_sketch_constraints_round_trip_and_validate_geometry() {
    use crate::features::{Length, ParameterId};
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        OffsetParameter, Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
        SketchDistanceMeasurement, SketchDistancePair, SketchEntity, SketchEntityId,
        SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus, SketchOffsetPair,
    };

    let entity = SketchEntityId::mint("synthetic:test:entity#0").unwrap();
    let parameter = ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar");
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
            parameter: Some(OffsetParameter {
                id: parameter.clone(),
                negated: true,
            }),
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
        SketchConstraintDefinition::SameCoordinate {
            relation: crate::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(entity.clone()),
                SketchLocus::End(entity.clone()),
                crate::sketches::SketchCoordinateAxis::V,
            )
            .unwrap(),
        },
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::SameCoordinate {
            relation: crate::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(entity.clone()),
                SketchLocus::End(entity.clone()),
                crate::sketches::SketchCoordinateAxis::U,
            )
            .unwrap(),
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
            parameter: ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar"),
        },
        SketchConstraintDefinition::Weight {
            entity: entity.clone(),
            parameter: ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar"),
        },
        SketchConstraintDefinition::InternalAlignment {
            helper: entity.clone(),
            parent: entity.clone(),
            alignment: crate::sketches::SketchInternalAlignment::BsplineControlPoint(2),
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
    let sketch = SketchId::mint("synthetic:test:sketch#locus").unwrap();
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
    ir.model.sketch_entities.push(SketchEntity::new(
        entity.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    let constraint_id = SketchConstraintId::mint("synthetic:test:constraint#locus").unwrap();
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
    ir.model.sketch_entities[0].geometry = SketchGeometry::native("center-bearing-curve".into());
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
        SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchGeometry,
        SketchGeometryDefinition, SketchId, SketchLocus,
    };

    let sketch = SketchId::mint("synthetic:test:sketch#coordinate-equations").unwrap();
    let first = SketchEntityId::mint("synthetic:test:coordinate-point#first").unwrap();
    let second = SketchEntityId::mint("synthetic:test:coordinate-point#second").unwrap();
    let midpoint = SketchEntityId::mint("synthetic:test:coordinate-point#midpoint").unwrap();
    let constraints = [
        (
            SketchConstraintId::mint("synthetic:test:constraint#point-coordinates").unwrap(),
            SketchConstraintDefinition::PointCoordinateValues {
                point: SketchLocus::Entity(midpoint.clone()),
                values: [Length(2.0), Length(1.0)],
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-u").unwrap(),
            SketchConstraintDefinition::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::U,
                value: Length(2.0),
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-v").unwrap(),
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
        .map(|(id, position)| {
            SketchEntity::new(
                id,
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
            )
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
        .find(|entity| entity.id() == &midpoint)
        .unwrap();
    midpoint_entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(3.0, 1.0),
    })
    .unwrap();
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
        sketch: SketchId::mint("synthetic:test:sketch#region").unwrap(),
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
                    entity: SketchEntityId::mint("synthetic:test:sketch-entity#curve").unwrap(),
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
        OffsetParameter, SketchConstraintId, SpatialSketch, SpatialSketchConstraint,
        SpatialSketchConstraintDefinition, SpatialSketchEntity, SpatialSketchEntityId,
        SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
        SpatialSketchId, SpatialSketchProfile,
    };

    let mut ir = unit_cube();
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#one").unwrap();
    let circle =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#circle").unwrap();
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
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            circle.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
                center: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                reference_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: Length(4.0),
            })
            .unwrap(),
        ));
    let parallel_line =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#parallel-line").unwrap();
    ir.model.spatial_sketch_entities.push(
        SpatialSketchEntity::new(
            parallel_line.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(0.0, 2.0f64.sqrt(), -2.0f64.sqrt()),
                end: Point3::new(1.0, 1.0 + 2.0f64.sqrt(), 1.0 - 2.0f64.sqrt()),
            })
            .unwrap(),
        )
        .with_construction(true),
    );
    let collinear_line =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#collinear-line").unwrap();
    ir.model.spatial_sketch_entities.push(
        SpatialSketchEntity::new(
            collinear_line.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(2.0, 2.0, 2.0),
                end: Point3::new(3.0, 3.0, 3.0),
            })
            .unwrap(),
        )
        .with_construction(true),
    );
    let repeated_parallel_line =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#repeated-parallel-line")
            .unwrap();
    ir.model.spatial_sketch_entities.push(
        SpatialSketchEntity::new(
            repeated_parallel_line.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(2.0, 2.0 + 2.0f64.sqrt(), 2.0 - 2.0f64.sqrt()),
                end: Point3::new(3.0, 3.0 + 2.0f64.sqrt(), 3.0 - 2.0f64.sqrt()),
            })
            .unwrap(),
        )
        .with_construction(true),
    );
    let distance =
        ParameterId::mint("synthetic:test:parameter#spatial-distance").expect("identity grammar");
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
    let line_length = ParameterId::mint("synthetic:test:parameter#spatial-line-length")
        .expect("identity grammar");
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
    let surface =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#surface").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            surface.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::NurbsSurface {
                surface: crate::geometry::BsplineSurface::new(
                    1,
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![
                        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                    ],
                )
                .unwrap(),
            })
            .unwrap(),
        ));
    let surface_point =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#surface-point").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            surface_point.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.5, 0.5, 0.0),
            })
            .unwrap(),
        ));
    let line = SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#line").unwrap();
    ir.model.spatial_sketch_entities.push(
        SpatialSketchEntity::new(
            line.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(0.0, 0.0, 0.0),
                end: Point3::new(1.0, 1.0, 1.0),
            })
            .unwrap(),
        )
        .with_construction(true),
    );
    let point = SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#point").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            point.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.5, 0.5, 0.5),
            })
            .unwrap(),
        ));
    let measured_point =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#measured-point").unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            measured_point.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.5, 0.5, 2.5),
            })
            .unwrap(),
        ));
    let coincident_point =
        SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#coincident-point")
            .unwrap();
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            coincident_point.clone(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.5, 0.5, 0.5),
            })
            .unwrap(),
        ));
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#group").unwrap(),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::SplineGroup {
                entities: vec![line.clone(), circle.clone()],
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint(
                "synthetic:test:spatial-sketch-constraint#repeated-parallel-distance",
            )
            .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#offset")
                .unwrap(),
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
                parameter: Some(OffsetParameter {
                    id: distance.clone(),
                    negated: false,
                }),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint(
                "synthetic:test:spatial-sketch-constraint#line-set-distance",
            )
            .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#line-length")
                .unwrap(),
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
            id: SketchConstraintId::mint(
                "synthetic:test:spatial-sketch-constraint#repeated-line-length",
            )
            .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#point-surface")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#coincident")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#symmetric")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#midpoint")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#point-distance")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#direction")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#distance")
                .unwrap(),
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
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#tangent")
                .unwrap(),
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
        .find(|entity| {
            matches!(
                *entity.geometry.definition(),
                SpatialSketchGeometryDefinition::Point { .. }
            )
        })
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
        sketch: SpatialSketchId::mint("synthetic:test:spatial-sketch#0").unwrap(),
        curves: vec![
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#0").unwrap(),
        ],
    };
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);

    let native = PathRef::SpatialSketchSelection {
        sketch: SpatialSketchId::mint("synthetic:test:spatial-sketch#0").unwrap(),
        selections: vec!["native:path-selection#0".into()],
    };
    let json = serde_json::to_string(&native).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), native);
}

fn pattern_direction(axis: [f64; 2]) -> crate::sketches::SketchPatternDirection {
    crate::sketches::SketchPatternDirection {
        direction: axis,
        spacing: crate::features::Length(2.0),
        distance: None,
        count_parameter: None,
    }
}

#[test]
fn rectangular_pattern_derives_counts_and_indices_on_the_wire() {
    use crate::sketches::{
        SketchConstraintDefinition, SketchEntityId, SketchPatternDistance, SketchPatternInstance,
        SketchRectangularPattern,
    };

    let mut first_direction = pattern_direction([1.0, 0.0]);
    first_direction.distance = Some(SketchPatternDistance::Spacing(
        crate::features::ParameterId::mint("test:test:parameter#spacing")
            .expect("identity grammar"),
    ));
    let pattern = SketchRectangularPattern::new(
        [first_direction, pattern_direction([0.0, 1.0])],
        vec![
            vec![SketchPatternInstance {
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#0").unwrap()],
            }],
            vec![SketchPatternInstance {
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#1").unwrap()],
            }],
        ],
    )
    .unwrap();
    let definition = SketchConstraintDefinition::RectangularPattern { pattern };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["directions"][0]["count"], 2);
    assert_eq!(wire["directions"][1]["count"], 1);
    assert_eq!(
        wire["directions"][0]["spacing_parameter"],
        "test:parameter#spacing"
    );
    assert!(wire["directions"][0].get("span_parameter").is_none());
    assert_eq!(wire["instances"][0]["indices"], serde_json::json!([0, 0]));
    assert_eq!(wire["instances"][1]["indices"], serde_json::json!([1, 0]));
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut split_count = wire.clone();
    split_count["directions"][0]["count"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(split_count).is_err());
    let mut conflicting_distance = wire.clone();
    conflicting_distance["directions"][0]["span_parameter"] =
        serde_json::json!("test:parameter#span");
    assert!(serde_json::from_value::<SketchConstraintDefinition>(conflicting_distance).is_err());
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
        SketchEntityId::mint("test:test:sketch-entity#center").unwrap(),
        Angle(1.0),
        None,
        None,
        vec![
            SketchCircularPatternInstance {
                angle: Angle(0.0),
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#0").unwrap()],
            },
            SketchCircularPatternInstance {
                angle: Angle(1.0),
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#1").unwrap()],
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

#[test]
fn offset_parameter_keeps_the_paired_factor_wire_shape() {
    use crate::features::{Length, ParameterId};
    use crate::sketches::{
        OffsetParameter, SketchConstraintDefinition, SketchEntityId, SketchOffsetPair,
    };

    let definition = SketchConstraintDefinition::Offset {
        pairs: vec![SketchOffsetPair {
            source: SketchEntityId::mint("test:test:sketch-entity#source").unwrap(),
            result: SketchEntityId::mint("test:test:sketch-entity#result").unwrap(),
            source_reversed: false,
        }],
        distance: Length(2.0),
        parameter: Some(OffsetParameter {
            id: ParameterId::mint("test:test:parameter#offset").expect("identity grammar"),
            negated: true,
        }),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["parameter"], "test:parameter#offset");
    assert_eq!(wire["parameter_factor"], -1.0);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut invalid_factor = wire.clone();
    invalid_factor["parameter_factor"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<SketchConstraintDefinition>(invalid_factor).is_err());
    let mut split = wire;
    split.as_object_mut().unwrap().remove("parameter_factor");
    assert!(serde_json::from_value::<SketchConstraintDefinition>(split).is_err());
}

#[test]
fn conic_bounds_keep_the_paired_wire_fields() {
    use crate::features::{Angle, Length};
    use crate::math::Point2;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let cases = [
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle(0.25),
            major_radius: Length(4.0),
            minor_radius: Length(2.0),
            bounds: Some([Angle(-0.5), Angle(1.5)]),
        })
        .unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Hyperbola {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle(0.25),
            major_radius: Length(4.0),
            minor_radius: Length(2.0),
            bounds: Some([-0.5, 1.5]),
        })
        .unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
            vertex: Point2::new(1.0, 2.0),
            axis_angle: Angle(0.25),
            focal_length: Length(2.0),
            bounds: Some([-0.5, 1.5]),
        })
        .unwrap(),
    ];

    for geometry in cases {
        let wire = serde_json::to_value(&geometry).unwrap();
        let start_field = if matches!(
            geometry.definition(),
            SketchGeometryDefinition::Ellipse { .. }
        ) {
            "start_angle"
        } else {
            "start_parameter"
        };
        let end_field = if matches!(
            geometry.definition(),
            SketchGeometryDefinition::Ellipse { .. }
        ) {
            "end_angle"
        } else {
            "end_parameter"
        };
        assert_eq!(wire[start_field], -0.5);
        assert_eq!(wire[end_field], 1.5);
        assert_eq!(
            serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap(),
            geometry
        );

        let mut split = wire;
        split.as_object_mut().unwrap().remove(end_field);
        assert!(serde_json::from_value::<SketchGeometry>(split).is_err());
    }
}

#[test]
fn text_placement_keeps_the_paired_wire_fields() {
    use crate::features::{Angle, Length};
    use crate::math::Point2;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition, TextPlacement};

    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: crate::products::NonEmptyString::new("cadmpeg").unwrap(),
        font_family: crate::products::NonEmptyString::new("sans").unwrap(),
        font_weight: crate::sketches::SketchFontWeight::Regular,
        height: Length(4.0),
        width_factor: None,
        placement: Some(TextPlacement {
            anchor: Point2::new(1.0, 2.0),
            rotation: Angle(0.5),
        }),
        horizontal_alignment: None,
        vertical_alignment: None,
    })
    .unwrap();
    let wire = serde_json::to_value(&geometry).unwrap();
    assert_eq!(wire["anchor"], serde_json::json!({ "u": 1.0, "v": 2.0 }));
    assert_eq!(wire["rotation"], 0.5);
    assert_eq!(
        serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap(),
        geometry
    );

    let mut split = wire;
    split.as_object_mut().unwrap().remove("rotation");
    assert!(serde_json::from_value::<SketchGeometry>(split).is_err());
}

#[test]
fn internal_alignment_index_stays_with_bspline_variants() {
    use crate::sketches::{SketchConstraintDefinition, SketchEntityId, SketchInternalAlignment};

    let definition = SketchConstraintDefinition::InternalAlignment {
        helper: SketchEntityId::mint("test:test:sketch-entity#helper").unwrap(),
        parent: SketchEntityId::mint("test:test:sketch-entity#parent").unwrap(),
        alignment: SketchInternalAlignment::BsplineControlPoint(2),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["alignment"], "bspline_control_point");
    assert_eq!(wire["index"], 2);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut missing_index = wire.clone();
    missing_index.as_object_mut().unwrap().remove("index");
    assert!(serde_json::from_value::<SketchConstraintDefinition>(missing_index).is_err());
    let mut extraneous_index = wire;
    extraneous_index["alignment"] = serde_json::json!("ellipse_focus1");
    assert!(serde_json::from_value::<SketchConstraintDefinition>(extraneous_index).is_err());
}

#[test]
fn same_coordinate_accepts_legacy_relation_tags() {
    use crate::sketches::{
        SketchConstraint, SketchConstraintDefinition, SketchCoordinateAxis, SketchEntityId,
        SketchLocus,
    };

    let first = SketchLocus::Entity(SketchEntityId::mint("test:test:sketch-entity#first").unwrap());
    let second =
        SketchLocus::Entity(SketchEntityId::mint("test:test:sketch-entity#second").unwrap());
    for (kind, axis) in [
        ("horizontal_loci", SketchCoordinateAxis::V),
        ("horizontal_points", SketchCoordinateAxis::V),
        ("vertical_loci", SketchCoordinateAxis::U),
        ("vertical_points", SketchCoordinateAxis::U),
    ] {
        let constraint = serde_json::from_value::<SketchConstraint>(serde_json::json!({
            "id": "test:sketch-constraint#axis",
            "sketch": "test:sketch#axis",
            "definition": {
                "kind": kind,
                "first": first,
                "second": second,
            },
        }))
        .unwrap();
        assert_eq!(
            constraint.definition,
            SketchConstraintDefinition::SameCoordinate {
                relation: crate::sketches::SketchSameCoordinate::try_new(
                    first.clone(),
                    second.clone(),
                    axis
                )
                .unwrap()
            }
        );
        let wire = serde_json::to_value(constraint).unwrap();
        assert_eq!(wire["definition"]["kind"], "same_coordinate");
        assert_eq!(
            wire["definition"]["axis"],
            if axis == SketchCoordinateAxis::U {
                "u"
            } else {
                "v"
            }
        );
    }
}

#[test]
fn solver_scalar_class_uses_the_numeric_wire_discriminator() {
    use crate::sketches::SketchConstraintDefinition;
    let angle = SketchConstraintDefinition::AngleDifference {
        first: 17,
        second: 18,
        difference: 19,
        value: crate::features::Angle(0.5),
    };
    let wire = serde_json::to_value(&angle).unwrap();
    assert_eq!(
        wire["first"],
        serde_json::json!({"variable_type": 4, "key": 17})
    );
    assert_eq!(
        wire["second"],
        serde_json::json!({"variable_type": 4, "key": 18})
    );
    assert_eq!(
        wire["difference"],
        serde_json::json!({"variable_type": 0, "key": 19})
    );
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire).unwrap(),
        angle
    );

    let equality = SketchConstraintDefinition::ScalarEquality {
        first: 17,
        second: 18,
    };
    let wire = serde_json::to_value(&equality).unwrap();
    assert_eq!(
        wire["first"],
        serde_json::json!({"variable_type": 6, "key": 17})
    );
    assert_eq!(
        wire["second"],
        serde_json::json!({"variable_type": 6, "key": 18})
    );
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire).unwrap(),
        equality
    );
}

#[test]
fn solver_scalar_class_rejects_a_constraint_slot_mismatch() {
    use crate::sketches::SketchConstraintDefinition;
    for definition in [
        SketchConstraintDefinition::AngleDifference {
            first: 17,
            second: 18,
            difference: 19,
            value: crate::features::Angle(0.5),
        },
        SketchConstraintDefinition::ScalarEquality {
            first: 17,
            second: 18,
        },
    ] {
        let wire = serde_json::to_value(&definition).unwrap();
        for slot in ["first", "second", "difference"] {
            let Some(scalar) = wire.get(slot) else {
                continue;
            };
            for wrong_class in [0, 4, 5, 6] {
                if scalar["variable_type"] == wrong_class {
                    continue;
                }
                let mut malformed = wire.clone();
                malformed[slot]["variable_type"] = serde_json::json!(wrong_class);
                let error =
                    serde_json::from_value::<SketchConstraintDefinition>(malformed).unwrap_err();
                assert!(error.to_string().contains("variable_type must be"));
            }
        }
    }
}

#[test]
fn planar_nurbs_wire_preserves_flat_fields_and_checks_cardinality() {
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let wire = serde_json::json!({
        "kind": "nurbs", "degree": 1,
        "knots": [0.0, 0.0, 1.0, 1.0],
        "control_points": [{"u": 2.0, "v": 3.0}, {"u": 4.0, "v": 5.0}],
        "weights": [1.0, 0.5], "periodic": false
    });
    let geometry: SketchGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for (field, value) in [
        ("degree", serde_json::json!(0)),
        ("degree", serde_json::json!(2)),
        ("knots", serde_json::json!([0.0, 1.0])),
        ("control_points", serde_json::json!([{"u": 2.0, "v": 3.0}])),
        ("weights", serde_json::json!([1.0])),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<SketchGeometry>(invalid).is_err(),
            "{field}"
        );
    }
    let mut nonrational = wire;
    nonrational.as_object_mut().unwrap().remove("weights");
    nonrational.as_object_mut().unwrap().remove("periodic");
    let geometry = serde_json::from_value::<SketchGeometry>(nonrational).unwrap();
    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        panic!("NURBS geometry");
    };
    assert!(curve.weights().is_none());
    assert!(!curve.periodic());
}

#[test]
fn spatial_nurbs_wire_preserves_flat_fields_and_checks_cardinality() {
    use crate::sketches::SpatialSketchGeometry;

    let wire = serde_json::json!({
        "kind": "nurbs", "degree": 1,
        "knots": [0.0, 0.0, 1.0, 1.0],
        "control_points": [{"x": 2.0, "y": 3.0, "z": 4.0}, {"x": 5.0, "y": 6.0, "z": 7.0}],
        "weights": [1.0, 0.5], "periodic": false
    });
    let geometry: SpatialSketchGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for (field, value) in [
        ("degree", serde_json::json!(2)),
        ("knots", serde_json::json!([0.0, 1.0])),
        ("control_points", serde_json::json!([])),
        ("weights", serde_json::json!([1.0])),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<SpatialSketchGeometry>(invalid).is_err(),
            "{field}"
        );
    }
}

#[test]
fn spatial_surface_wire_checks_rectangular_grid_and_full_knots() {
    use crate::sketches::SpatialSketchGeometry;

    let wire = serde_json::json!({
        "kind": "nurbs_surface", "u_degree": 1, "v_degree": 1,
        "u_knots": [0.0, 0.0, 1.0, 1.0], "v_knots": [0.0, 0.0, 1.0, 1.0],
        "control_points": [
            [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 0.0, "y": 1.0, "z": 0.0}],
            [{"x": 1.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 1.0, "z": 0.0}]
        ]
    });
    let geometry: SpatialSketchGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for field in ["u_knots", "v_knots", "control_points"] {
        let mut invalid = wire.clone();
        invalid[field].as_array_mut().unwrap().pop();
        assert!(
            serde_json::from_value::<SpatialSketchGeometry>(invalid).is_err(),
            "{field}"
        );
    }
    let mut ragged = wire;
    ragged["control_points"][1].as_array_mut().unwrap().pop();
    let error = serde_json::from_value::<SpatialSketchGeometry>(ragged).unwrap_err();
    assert!(error.to_string().contains("control_points row"));
}

#[test]
fn native_operand_requires_nonempty_names_and_keeps_the_role_inside_the_field() {
    use crate::sketches::SketchNativeOperand;

    for wire in [
        serde_json::json!({"native_kind": "", "object_index": 0}),
        serde_json::json!({"native_kind": "operand", "object_index": 0,
            "field": {"name": "", "role": 1}}),
        serde_json::json!({"native_kind": "operand", "object_index": 0, "native_role": 1}),
        serde_json::json!({"native_kind": "operand", "object_index": 0, "field": {"role": 1}}),
    ] {
        assert!(serde_json::from_value::<SketchNativeOperand>(wire).is_err());
    }
    for wire in [
        serde_json::json!({"native_kind": "operand", "object_index": 0}),
        serde_json::json!({"native_kind": "operand", "object_index": 0, "field": {"name": "edge"}}),
        serde_json::json!({"native_kind": "operand", "object_index": 0,
            "field": {"name": "edge", "role": 1}}),
    ] {
        let operand: SketchNativeOperand = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(operand).unwrap(), wire);
    }
}

#[test]
fn spatial_nurbs_rejects_general_curve_context_mismatches() {
    use crate::geometry::NurbsCurve;
    use crate::sketches::{SpatialSketchGeometry, SpatialSketchNurbsCurve};

    let negative = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![-1.0, -1.0]),
        false,
    )
    .unwrap();
    let degree_zero = NurbsCurve::new(
        0,
        vec![0.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    for (curve, field) in [(negative, "weights"), (degree_zero, "degree")] {
        assert!(SpatialSketchNurbsCurve::try_from(curve.clone())
            .unwrap_err()
            .contains(field));
        let mut wire = serde_json::to_value(&curve).unwrap();
        wire["kind"] = "nurbs".into();
        assert!(serde_json::from_value::<SpatialSketchGeometry>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn spatial_nurbs_preserves_wire_fields_and_checked_point_edits() {
    use crate::geometry::NurbsCurve;
    use crate::sketches::{
        SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchNurbsCurve,
    };

    let curve = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]),
        false,
    )
    .unwrap();
    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["kind"] = "nurbs".into();
    let mut curve = SpatialSketchNurbsCurve::try_from(curve).unwrap();
    let before = curve.clone();
    assert!(curve
        .edit_control_points(|points| points[0].x = f64::NAN)
        .is_err());
    assert_eq!(curve, before);
    let geometry =
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Nurbs { curve }).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SpatialSketchGeometry>(wire).unwrap(),
        geometry
    );
}

#[test]
fn polygon_membership_is_checked_at_admission() {
    use crate::sketches::{SketchConstraintDefinition, SketchEntityId, SketchPolygon};

    let members = (0..3)
        .map(|index| SketchEntityId::mint(format!("test:sketch:entity#{index}")).unwrap())
        .collect::<Vec<_>>();
    for entities in [
        Vec::new(),
        members[..1].to_vec(),
        members[..2].to_vec(),
        vec![members[0].clone(), members[1].clone(), members[0].clone()],
    ] {
        assert!(SketchPolygon::try_new(entities.clone()).is_err());
        let wire = serde_json::json!({"kind": "polygon", "entities": entities});
        assert!(serde_json::from_value::<SketchConstraintDefinition>(wire).is_err());
    }
    let wire = serde_json::json!({"kind": "polygon", "entities": members});
    let definition = SketchConstraintDefinition::Polygon {
        polygon: SketchPolygon::try_new(members).unwrap(),
    };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinition>(wire).unwrap(),
        definition
    );
}

#[test]
fn coordinate_locus_distinctness_is_checked_at_admission() {
    use crate::sketches::{
        SketchConstraintDefinition, SketchCoordinateAxis, SketchEntityId, SketchLocus,
        SketchSameCoordinate,
    };

    let entity = SketchEntityId::mint("test:sketch:entity#0").unwrap();
    let first = SketchLocus::Start(entity.clone());
    for axis in [SketchCoordinateAxis::U, SketchCoordinateAxis::V] {
        assert!(SketchSameCoordinate::try_new(first.clone(), first.clone(), axis).is_err());
        let mut wire = serde_json::json!({"kind": "same_coordinate", "first": first, "second": first, "axis": axis});
        assert!(serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).is_err());
        let second = SketchLocus::End(entity.clone());
        wire["second"] = serde_json::to_value(&second).unwrap();
        let definition = SketchConstraintDefinition::SameCoordinate {
            relation: SketchSameCoordinate::try_new(first.clone(), second, axis).unwrap(),
        };
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<SketchConstraintDefinition>(wire).unwrap(),
            definition
        );
    }
}

#[test]
fn planar_geometry_admission_checks_each_numeric_family() {
    use crate::features::{Angle, Length};
    use crate::math::Point2;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition as Definition};

    let point = Point2::new(0.0, 0.0);
    let bad_point = Point2::new(f64::INFINITY, 0.0);
    for definition in [
        Definition::Point {
            position: bad_point,
        },
        Definition::Line {
            start: point,
            end: bad_point,
        },
        Definition::ReferenceLine {
            origin: point,
            direction: Point2::new(f64::EPSILON, 0.0),
        },
        Definition::Circle {
            center: point,
            radius: Length(0.0),
        },
        Definition::Arc {
            center: point,
            radius: Length(1.0),
            start_angle: Angle(f64::NAN),
            end_angle: Angle(0.0),
        },
        Definition::Ellipse {
            center: point,
            major_angle: Angle(0.0),
            major_radius: Length(1.0),
            minor_radius: Length(2.0),
            bounds: None,
        },
        Definition::Hyperbola {
            center: point,
            major_angle: Angle(0.0),
            major_radius: Length(1.0),
            minor_radius: Length(2.0),
            bounds: Some([0.0, f64::INFINITY]),
        },
        Definition::Parabola {
            vertex: point,
            axis_angle: Angle(0.0),
            focal_length: Length(-1.0),
            bounds: None,
        },
    ] {
        assert!(SketchGeometry::try_from(definition).is_err());
    }
    for wire in [
        serde_json::json!({"kind":"point","position":{"u":null,"v":0.0}}),
        serde_json::json!({"kind":"line","start":{"u":0.0,"v":0.0},"end":{"u":0.0,"v":null}}),
        serde_json::json!({"kind":"reference_line","origin":{"u":0.0,"v":0.0},"direction":{"u":0.0,"v":0.0}}),
        serde_json::json!({"kind":"circle","center":{"u":0.0,"v":0.0},"radius":-1.0}),
        serde_json::json!({"kind":"arc","center":{"u":0.0,"v":0.0},"radius":0.0,"start_angle":0.0,"end_angle":0.0}),
        serde_json::json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},"major_angle":0.0,"major_radius":1.0,"minor_radius":2.0}),
        serde_json::json!({"kind":"hyperbola","center":{"u":0.0,"v":0.0},"major_angle":0.0,"major_radius":0.0,"minor_radius":2.0}),
        serde_json::json!({"kind":"parabola","vertex":{"u":0.0,"v":0.0},"axis_angle":0.0,"focal_length":-1.0}),
    ] {
        assert!(serde_json::from_value::<SketchGeometry>(wire).is_err());
    }
}

#[test]
fn planar_geometry_preserves_wire_and_failed_edits_preserve_geometry() {
    use crate::features::Length;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let wire = serde_json::json!({
        "kind": "arc", "center": {"u": 0.0, "v": 0.0}, "radius": 1.0,
        "start_angle": -2.0, "end_angle": -2.0
    });
    let mut geometry = serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    let original = geometry.clone();
    assert!(geometry
        .edit(|definition| {
            let SketchGeometryDefinition::Arc { radius, .. } = definition else {
                panic!("arc")
            };
            *radius = Length(-1.0);
        })
        .is_err());
    assert_eq!(geometry, original);
    geometry
        .edit(|definition| {
            let SketchGeometryDefinition::Arc { radius, .. } = definition else {
                panic!("arc")
            };
            *radius = Length(2.0);
        })
        .unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap()["radius"], 2.0);
}

#[test]
fn planar_text_numeric_fields_are_checked_on_every_admission_route() {
    use crate::features::Length;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let wire = serde_json::json!({
        "kind": "text", "text": "A", "font_family": "Arial", "font_weight": 400,
        "height": 2.0, "width_factor": 1.0, "anchor": {"u": 0.0, "v": 0.0}, "rotation": -1.0
    });
    let geometry = serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for field in ["height", "width_factor", "rotation"] {
        let mut invalid = wire.clone();
        invalid[field] = if field == "rotation" {
            serde_json::Value::Null
        } else {
            serde_json::json!(0.0)
        };
        assert!(serde_json::from_value::<SketchGeometry>(invalid).is_err());
    }
    for field in 0..4 {
        let mut definition = geometry.clone().into_definition();
        let SketchGeometryDefinition::Text {
            height,
            width_factor,
            placement: Some(placement),
            ..
        } = &mut definition
        else {
            panic!("placed text")
        };
        match field {
            0 => *height = Length(f64::INFINITY),
            1 => *width_factor = Some(f64::NAN),
            2 => placement.anchor.u = f64::INFINITY,
            _ => placement.rotation.0 = f64::NAN,
        }
        assert!(SketchGeometry::try_from(definition).is_err());
    }
}

#[test]
fn sketch_text_style_admits_only_nonempty_names_and_three_integer_weights() {
    use crate::sketches::{SketchFontWeight, SketchGeometry};

    for (raw, weight) in [
        (400, SketchFontWeight::Regular),
        (500, SketchFontWeight::Medium),
        (750, SketchFontWeight::Bold),
    ] {
        assert_eq!(SketchFontWeight::try_from(raw).unwrap(), weight);
        assert_eq!(i32::from(weight), raw);
        assert_eq!(serde_json::to_value(weight).unwrap(), raw);
        assert_eq!(
            serde_json::from_value::<SketchFontWeight>(serde_json::json!(raw)).unwrap(),
            weight
        );
    }
    for raw in [-1, 0, 399, 401, 499, 501, 700, 749, 751, i32::MAX] {
        assert!(SketchFontWeight::try_from(raw).is_err());
        assert!(serde_json::from_value::<SketchFontWeight>(serde_json::json!(raw)).is_err());
    }
    let wire = serde_json::json!({"kind":"text", "text":" ", "font_family":" ", "font_weight":500, "height":1.0});
    let geometry = serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for field in ["text", "font_family"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!("");
        assert!(serde_json::from_value::<SketchGeometry>(invalid).is_err());
    }
}

#[test]
fn spatial_analytic_geometry_checks_separation_frames_and_angles() {
    use crate::features::{Angle, Length};
    use crate::sketches::{SpatialSketchGeometry, SpatialSketchGeometryDefinition as Definition};

    let origin = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let reference_direction = Vector3::new(1.0, 0.0, 0.0);
    for definition in [
        Definition::Point {
            position: Point3::new(0.0, f64::NAN, 0.0),
        },
        Definition::Line {
            start: origin,
            end: origin,
        },
        Definition::Line {
            start: origin,
            end: Point3::new(EPS_SPATIAL_LINE_BOUNDARY, 0.0, 0.0),
        },
        Definition::Circle {
            center: origin,
            normal,
            reference_direction,
            radius: Length(0.0),
        },
        Definition::Circle {
            center: origin,
            normal: Vector3::new(0.0, 0.0, 2.0),
            reference_direction,
            radius: Length(1.0),
        },
        Definition::Circle {
            center: origin,
            normal,
            reference_direction: normal,
            radius: Length(1.0),
        },
        Definition::Arc {
            center: origin,
            normal,
            reference_direction,
            radius: Length(1.0),
            start_angle: Angle(1.0),
            end_angle: Angle(1.0),
        },
        Definition::Arc {
            center: origin,
            normal,
            reference_direction,
            radius: Length(1.0),
            start_angle: Angle(f64::INFINITY),
            end_angle: Angle(1.0),
        },
    ] {
        assert!(SpatialSketchGeometry::try_from(definition).is_err());
    }
    for definition in [
        Definition::Line {
            start: origin,
            end: Point3::new(2.0 * EPS_SPATIAL_LINE_BOUNDARY, 0.0, 0.0),
        },
        Definition::Line {
            start: Point3::new(-f64::MAX, 0.0, 0.0),
            end: Point3::new(f64::MAX, 0.0, 0.0),
        },
        Definition::Circle {
            center: origin,
            normal: Vector3::new(0.0, 0.0, 1.0 + 0.5 * EPS_SPATIAL_FRAME_BOUNDARY),
            reference_direction,
            radius: Length(1.0),
        },
        Definition::Circle {
            center: origin,
            normal,
            reference_direction: Vector3::new(1.0, 0.0, 0.5 * EPS_SPATIAL_FRAME_BOUNDARY),
            radius: Length(1.0),
        },
    ] {
        assert!(SpatialSketchGeometry::try_from(definition).is_ok());
    }
}

#[test]
fn spatial_analytic_geometry_preserves_wire_and_rejects_invalid_edits() {
    use crate::features::Angle;
    use crate::sketches::{SpatialSketchGeometry, SpatialSketchGeometryDefinition};

    let wire = serde_json::json!({
        "kind":"arc", "center":{"x":1.0,"y":2.0,"z":3.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0}, "reference_direction":{"x":1.0,"y":0.0,"z":0.0},
        "radius":2.0, "start_angle":-1.0, "end_angle":-2.0
    });
    let mut geometry = serde_json::from_value::<SpatialSketchGeometry>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for (field, value) in [
        ("radius", serde_json::json!(-1.0)),
        ("normal", serde_json::json!({"x":0.0,"y":0.0,"z":2.0})),
        (
            "reference_direction",
            serde_json::json!({"x":0.0,"y":0.0,"z":1.0}),
        ),
        ("end_angle", serde_json::json!(-1.0)),
        ("start_angle", serde_json::Value::Null),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<SpatialSketchGeometry>(invalid).is_err());
    }
    let before = geometry.clone();
    assert!(geometry
        .edit(|definition| {
            let SpatialSketchGeometryDefinition::Arc {
                start_angle,
                end_angle,
                ..
            } = definition
            else {
                panic!("arc")
            };
            *end_angle = *start_angle;
        })
        .is_err());
    assert_eq!(geometry, before);
    geometry
        .edit(|definition| {
            let SpatialSketchGeometryDefinition::Arc { end_angle, .. } = definition else {
                panic!("arc")
            };
            *end_angle = Angle(-3.0);
        })
        .unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap()["end_angle"], -3.0);
}
