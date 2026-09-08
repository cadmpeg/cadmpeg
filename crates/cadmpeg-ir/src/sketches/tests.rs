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
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    };

    let mut ir = unit_cube();
    let sketch = SketchId::mint("synthetic:test:sketch#polygon").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: crate::sketches::SketchProfiles::default(),
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
        definition: crate::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Polygon {
                polygon: crate::sketches::SketchPolygon::try_new(members.clone()).unwrap(),
            },
        )
        .unwrap(),
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
        OffsetParameter, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
        SketchConstraintId, SketchDistanceMeasurement, SketchDistancePair, SketchEntity,
        SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
        SketchOffsetPair,
    };

    let entity = SketchEntityId::mint("synthetic:test:entity#0").unwrap();
    let parameter = ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar");
    let definitions = vec![
        SketchConstraintDefinitionInput::Disabled,
        SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![
                SketchLocus::Start(entity.clone()),
                SketchLocus::Center(entity.clone()),
            ],
        },
        SketchConstraintDefinitionInput::PointOnObject {
            point: SketchLocus::Start(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::End(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinitionInput::Offset {
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
        SketchConstraintDefinitionInput::Concentric {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinitionInput::Curvature {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinitionInput::Collinear {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            axis: entity.clone(),
        },
        SketchConstraintDefinitionInput::Radius {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::RepeatedRadius {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::Diameter {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::RepeatedDiameter {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::EqualDistance {
            first: SketchDistancePair {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            },
            second: SketchDistancePair {
                first: SketchLocus::Center(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            },
        },
        SketchConstraintDefinitionInput::HorizontalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::SameCoordinate {
            relation: crate::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(entity.clone()),
                SketchLocus::End(entity.clone()),
                crate::sketches::SketchCoordinateAxis::V,
            )
            .unwrap(),
        },
        SketchConstraintDefinitionInput::VerticalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::SameCoordinate {
            relation: crate::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(entity.clone()),
                SketchLocus::End(entity.clone()),
                crate::sketches::SketchCoordinateAxis::U,
            )
            .unwrap(),
        },
        SketchConstraintDefinitionInput::RepeatedDistance {
            measurements: vec![SketchDistanceMeasurement::Horizontal {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            }],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::RepeatedLength {
            entities: vec![entity.clone(), entity.clone()],
            parameter: parameter.clone(),
        },
        SketchConstraintDefinitionInput::ParallelLineSetDistance {
            first: vec![entity.clone()],
            second: vec![entity.clone()],
            parameter,
        },
        SketchConstraintDefinitionInput::SnellsLaw {
            incident: SketchLocus::Start(entity.clone()),
            refracted: SketchLocus::End(entity.clone()),
            interface: entity.clone(),
            parameter: ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar"),
        },
        SketchConstraintDefinitionInput::Weight {
            entity: entity.clone(),
            parameter: ParameterId::mint("synthetic:test:parameter#0").expect("identity grammar"),
        },
        SketchConstraintDefinitionInput::InternalAlignment {
            helper: entity.clone(),
            parent: entity.clone(),
            alignment: crate::sketches::SketchInternalAlignment::BsplineControlPoint(2),
        },
        SketchConstraintDefinitionInput::Group {
            elements: vec![SketchLocus::Entity(entity.clone())],
        },
        SketchConstraintDefinitionInput::Text {
            elements: vec![SketchLocus::Entity(entity.clone())],
            text: "R42".into(),
            font: Some("Mono".into()),
            is_text_height: false,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<SketchConstraintDefinitionInput>>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    let sketch = SketchId::mint("synthetic:test:sketch#locus").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: crate::sketches::SketchProfiles::default(),
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
        definition: crate::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci {
                loci: vec![
                    SketchLocus::Center(entity.clone()),
                    SketchLocus::Start(entity),
                ],
            },
        )
        .unwrap(),
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
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
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
            SketchConstraintDefinitionInput::PointCoordinateValues {
                point: SketchLocus::Entity(midpoint.clone()),
                values: [Length(2.0), Length(1.0)],
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-u").unwrap(),
            SketchConstraintDefinitionInput::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::U,
                value: Length(2.0),
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-v").unwrap(),
            SketchConstraintDefinitionInput::MidpointCoordinate {
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
        placement: crate::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: crate::sketches::SketchProfiles::default(),
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
            definition:
                crate::sketches::SketchConstraintDefinition::try_from(definition.clone()).unwrap(),
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
        SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityId,
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
        profiles: vec![SpatialSketchProfile::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            vec![SpatialSketchEntityUse {
                entity: circle.clone(),
                reversed: false,
            }],
        )
        .unwrap()],
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
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::SplineGroup {
                    entities: vec![line.clone(), circle.clone()],
                },
            )
            .unwrap(),
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
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
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
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#offset")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Offset {
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
            )
            .unwrap(),
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
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::ParallelLineSetDistance {
                    first: vec![line.clone(), collinear_line],
                    second: vec![parallel_line.clone()],
                    parameter: distance.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#line-length")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::LineLength {
                    entity: line.clone(),
                    parameter: line_length.clone(),
                },
            )
            .unwrap(),
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
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::RepeatedLineLength {
                    entities: vec![line.clone(), parallel_line.clone()],
                    parameter: line_length,
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#point-surface")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::PointOnSurface {
                    point: surface_point,
                    surface,
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#coincident")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Coincident {
                    first: point.clone(),
                    second: coincident_point.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#symmetric")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Symmetric {
                    first: point.clone(),
                    second: coincident_point,
                    axis: line.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#midpoint")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Midpoint {
                    point: point.clone(),
                    entity: line.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#point-distance")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::PointDistance {
                    first: point.clone(),
                    second: measured_point,
                    parameter: distance.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#direction")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::ParallelToDirection {
                    entity: line.clone(),
                    direction: Vector3::new(
                        1.0 / 3.0f64.sqrt(),
                        1.0 / 3.0f64.sqrt(),
                        1.0 / 3.0f64.sqrt(),
                    ),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#distance")
                .unwrap(),
            sketch: sketch.clone(),
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                    first: line.clone(),
                    second: parallel_line,
                    parameter: distance.clone(),
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:spatial-sketch-constraint#tangent")
                .unwrap(),
            sketch,
            definition: crate::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Tangent {
                    first: line,
                    second: circle,
                },
            )
            .unwrap(),
            native_ref: None,
        });
    ir.finalize();
    assert!(validate_neutral(&ir, Vec::new()).findings.is_empty());
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
    non_curve_offset
        .model
        .spatial_sketch_constraints
        .iter_mut()
        .find(|constraint| constraint.id.0.ends_with("#offset"))
        .expect("spatial offset constraint")
        .definition
        .edit(|definition| {
            let SpatialSketchConstraintDefinitionInput::Offset { sources, .. } = definition else {
                panic!("spatial offset definition");
            };
            sources[0] = point_entity;
        })
        .unwrap();
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
    crate::sketches::SketchPatternDirection::new(axis, crate::features::Length(2.0), None, None)
        .unwrap()
}

#[test]
fn rectangular_pattern_derives_counts_and_indices_on_the_wire() {
    use crate::sketches::{
        SketchConstraintDefinitionInput, SketchEntityId, SketchPatternDistance,
        SketchPatternInstance, SketchRectangularPattern,
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
    let definition = SketchConstraintDefinitionInput::RectangularPattern { pattern };
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
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut split_count = wire.clone();
    split_count["directions"][0]["count"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(split_count).is_err());
    let mut conflicting_distance = wire.clone();
    conflicting_distance["directions"][0]["span_parameter"] =
        serde_json::json!("test:parameter#span");
    assert!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(conflicting_distance).is_err()
    );
    let mut displaced = wire;
    displaced["instances"][1]["indices"] = serde_json::json!([0, 1]);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(displaced).is_err());
}

#[test]
fn circular_pattern_derives_count_and_indices_on_the_wire() {
    use crate::features::Angle;
    use crate::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance, SketchConstraintDefinitionInput,
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
    let definition = SketchConstraintDefinitionInput::CircularPattern { pattern };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["count"], 2);
    assert_eq!(wire["instances"][0]["index"], 0);
    assert_eq!(wire["instances"][1]["index"], 1);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut split_count = wire.clone();
    split_count["count"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(split_count).is_err());
    let mut displaced = wire;
    displaced["instances"][1]["index"] = serde_json::json!(0);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(displaced).is_err());
}

#[test]
fn offset_parameter_keeps_the_paired_factor_wire_shape() {
    use crate::features::{Length, ParameterId};
    use crate::sketches::{
        OffsetParameter, SketchConstraintDefinitionInput, SketchEntityId, SketchOffsetPair,
    };

    let definition = SketchConstraintDefinitionInput::Offset {
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
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut invalid_factor = wire.clone();
    invalid_factor["parameter_factor"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(invalid_factor).is_err());
    let mut split = wire;
    split.as_object_mut().unwrap().remove("parameter_factor");
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(split).is_err());
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
    use crate::sketches::{
        SketchConstraintDefinitionInput, SketchEntityId, SketchInternalAlignment,
    };

    let definition = SketchConstraintDefinitionInput::InternalAlignment {
        helper: SketchEntityId::mint("test:test:sketch-entity#helper").unwrap(),
        parent: SketchEntityId::mint("test:test:sketch-entity#parent").unwrap(),
        alignment: SketchInternalAlignment::BsplineControlPoint(2),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["alignment"], "bspline_control_point");
    assert_eq!(wire["index"], 2);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut missing_index = wire.clone();
    missing_index.as_object_mut().unwrap().remove("index");
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(missing_index).is_err());
    let mut extraneous_index = wire;
    extraneous_index["alignment"] = serde_json::json!("ellipse_focus1");
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(extraneous_index).is_err());
}

#[test]
fn same_coordinate_accepts_legacy_relation_tags() {
    use crate::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId,
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
            constraint.definition.kind(),
            &SketchConstraintDefinitionInput::SameCoordinate {
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
    use crate::sketches::SketchConstraintDefinitionInput;
    let angle = SketchConstraintDefinitionInput::AngleDifference {
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
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
        angle
    );

    let equality = SketchConstraintDefinitionInput::ScalarEquality {
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
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
        equality
    );
}

#[test]
fn solver_scalar_class_rejects_a_constraint_slot_mismatch() {
    use crate::sketches::SketchConstraintDefinitionInput;
    for definition in [
        SketchConstraintDefinitionInput::AngleDifference {
            first: 17,
            second: 18,
            difference: 19,
            value: crate::features::Angle(0.5),
        },
        SketchConstraintDefinitionInput::ScalarEquality {
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
                let error = serde_json::from_value::<SketchConstraintDefinitionInput>(malformed)
                    .unwrap_err();
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
    use crate::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchPolygon};

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
        assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(wire).is_err());
    }
    let wire = serde_json::json!({"kind": "polygon", "entities": members});
    let definition = SketchConstraintDefinitionInput::Polygon {
        polygon: SketchPolygon::try_new(members).unwrap(),
    };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
        definition
    );
}

#[test]
fn coordinate_locus_distinctness_is_checked_at_admission() {
    use crate::sketches::{
        SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchLocus,
        SketchSameCoordinate,
    };

    let entity = SketchEntityId::mint("test:sketch:entity#0").unwrap();
    let first = SketchLocus::Start(entity.clone());
    for axis in [SketchCoordinateAxis::U, SketchCoordinateAxis::V] {
        assert!(SketchSameCoordinate::try_new(first.clone(), first.clone(), axis).is_err());
        let mut wire = serde_json::json!({"kind": "same_coordinate", "first": first, "second": first, "axis": axis});
        assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).is_err());
        let second = SketchLocus::End(entity.clone());
        wire["second"] = serde_json::to_value(&second).unwrap();
        let definition = SketchConstraintDefinitionInput::SameCoordinate {
            relation: SketchSameCoordinate::try_new(first.clone(), second, axis).unwrap(),
        };
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
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

#[test]
fn spatial_profile_admission_preserves_frame_boundary_and_wire() {
    use crate::sketches::{SpatialSketchEntityId, SpatialSketchEntityUse, SpatialSketchProfile};

    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let u_axis = Vector3::new(1.0, 0.0, 0.0);
    let boundary = vec![SpatialSketchEntityUse {
        entity: SpatialSketchEntityId::mint("synthetic:test:spatial-entity#profile").unwrap(),
        reversed: true,
    }];
    let mut profile =
        SpatialSketchProfile::try_new(origin, normal, u_axis, boundary.clone()).unwrap();
    let wire = serde_json::json!({
        "origin": {"x": 1.0, "y": 2.0, "z": 3.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        "boundary": [{"entity": "synthetic:test:spatial-entity#profile", "reversed": true}]
    });
    assert_eq!(serde_json::to_value(&profile).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SpatialSketchProfile>(wire.clone()).unwrap(),
        profile
    );
    assert!(SpatialSketchProfile::try_new(origin, normal, u_axis, vec![]).is_err());
    assert!(SpatialSketchProfile::try_new(
        origin,
        normal,
        u_axis,
        vec![boundary[0].clone(), boundary[0].clone()]
    )
    .is_err());
    for axis in [
        Vector3::new(0.0, 0.0, 0.0),
        normal,
        Vector3::new(f64::NAN, 0.0, 0.0),
    ] {
        assert!(SpatialSketchProfile::try_new(origin, normal, axis, boundary.clone()).is_err());
    }
    assert!(SpatialSketchProfile::try_new(
        origin,
        normal,
        Vector3::new(1.0 + EPS_SPATIAL_FRAME_BOUNDARY * 0.5, 0.0, 0.0),
        boundary
    )
    .is_ok());
    for (field, invalid) in [
        ("origin", serde_json::json!({"x": null, "y": 0.0, "z": 0.0})),
        ("normal", serde_json::json!({"x": 0.0, "y": 0.0, "z": 2.0})),
        ("u_axis", serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0})),
        ("boundary", serde_json::json!([])),
        (
            "boundary",
            serde_json::json!([wire["boundary"][0], wire["boundary"][0]]),
        ),
    ] {
        let mut invalid_wire = wire.clone();
        invalid_wire[field] = invalid;
        assert!(serde_json::from_value::<SpatialSketchProfile>(invalid_wire).is_err());
    }
    let before = profile.clone();
    assert!(profile
        .set_origin(Point3::new(f64::INFINITY, 0.0, 0.0))
        .is_err());
    assert_eq!(profile, before);
    let moved = Point3::new(4.0, 5.0, 6.0);
    profile.set_origin(moved).unwrap();
    assert_eq!(profile.origin(), moved);
    assert_eq!(profile.boundary(), before.boundary());
}

#[test]
fn planar_placement_admits_nonunit_perpendicular_axes_at_both_boundaries() {
    use crate::sketches::SketchPlacement;

    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let u_axis = Vector3::new(3.0, 0.0, 0.0);
    let placement = SketchPlacement::try_resolved(origin, normal, u_axis).unwrap();
    assert_eq!(placement.resolved(), Some((origin, normal, u_axis)));
    let wire = serde_json::json!({
        "kind": "resolved",
        "origin": {"x": 1.0, "y": 2.0, "z": 3.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 2.0},
        "u_axis": {"x": 3.0, "y": 0.0, "z": 0.0}
    });
    assert_eq!(serde_json::to_value(placement).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SketchPlacement>(wire.clone()).unwrap(),
        placement
    );
    for axis in [
        Vector3::new(0.0, 0.0, 0.0),
        normal,
        Vector3::new(f64::NAN, 0.0, 0.0),
    ] {
        assert!(SketchPlacement::try_resolved(origin, normal, axis).is_err());
    }
    assert!(
        SketchPlacement::try_resolved(Point3::new(f64::INFINITY, 0.0, 0.0), normal, u_axis)
            .is_err()
    );
    for (field, invalid) in [
        ("origin", serde_json::json!({"x": null, "y": 0.0, "z": 0.0})),
        ("normal", serde_json::json!({"x": 0.0, "y": 0.0, "z": 0.0})),
        ("u_axis", serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0})),
    ] {
        let mut invalid_wire = wire.clone();
        invalid_wire[field] = invalid;
        assert!(serde_json::from_value::<SketchPlacement>(invalid_wire).is_err());
    }
    let unresolved = serde_json::json!({"kind": "unresolved"});
    assert_eq!(
        serde_json::to_value(SketchPlacement::Unresolved).unwrap(),
        unresolved
    );
    assert_eq!(
        serde_json::from_value::<SketchPlacement>(unresolved).unwrap(),
        SketchPlacement::Unresolved
    );
}

#[test]
fn sketch_profile_collection_rejects_empty_chains_and_rolls_back_failed_edits() {
    use crate::sketches::{SketchEntityId, SketchEntityUse, SketchProfiles};

    let usage = SketchEntityUse {
        entity: SketchEntityId::mint("synthetic:test:sketch-entity#profile").unwrap(),
        reversed: false,
    };
    let mut profiles = SketchProfiles::try_from(vec![vec![usage.clone()]]).unwrap();
    let wire = serde_json::json!([[{"entity": "synthetic:test:sketch-entity#profile", "reversed": false}]]);
    assert_eq!(serde_json::to_value(&profiles).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SketchProfiles>(wire).unwrap(),
        profiles
    );
    assert!(SketchProfiles::try_from(vec![vec![]]).is_err());
    assert!(serde_json::from_value::<SketchProfiles>(serde_json::json!([[]])).is_err());
    assert!(
        serde_json::from_value::<SketchProfiles>(serde_json::json!([]))
            .unwrap()
            .is_empty()
    );
    let before = profiles.clone();
    assert!(profiles.try_push(vec![]).is_err());
    assert_eq!(profiles, before);
    assert!(profiles.edit(|chains| chains[0].clear()).is_err());
    assert_eq!(profiles, before);
    profiles
        .edit(|chains| chains.push(vec![usage.clone()]))
        .unwrap();
    assert_eq!(profiles.len(), 2);
    profiles.push_single(usage);
    assert_eq!(profiles.len(), 3);
    let before_filter = profiles.clone();
    let mut calls = 0;
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        profiles.retain_uses(|_| {
            calls += 1;
            assert_ne!(calls, 2, "filter interruption");
            false
        });
    }))
    .is_err());
    assert_eq!(profiles, before_filter);
    profiles.retain_uses(|_| false);
    assert!(profiles.is_empty());
}

#[test]
fn circular_pattern_admission_checks_angles_and_entity_ownership() {
    use crate::features::Angle;
    use crate::sketches::{SketchCircularPattern, SketchCircularPatternInstance, SketchEntityId};

    let center = SketchEntityId::mint("test:test:sketch-entity#center").unwrap();
    let seed = SketchEntityId::mint("test:test:sketch-entity#seed").unwrap();
    let copy = SketchEntityId::mint("test:test:sketch-entity#copy").unwrap();
    let instances = vec![
        SketchCircularPatternInstance {
            angle: Angle(0.0),
            entities: vec![seed.clone()],
        },
        SketchCircularPatternInstance {
            angle: Angle(-1.0),
            entities: vec![copy],
        },
    ];
    let admit = |angle, instances| {
        SketchCircularPattern::new(center.clone(), Angle(angle), None, None, instances)
    };
    let pattern = admit(-1.0, instances.clone()).unwrap();
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(admit(angle, instances.clone()).is_none());
        let mut invalid = instances.clone();
        invalid[1].angle = Angle(angle);
        assert!(admit(-1.0, invalid).is_none());
    }
    let mut invalid = instances.clone();
    invalid[0].angle = Angle(f64::MIN_POSITIVE);
    assert!(admit(-1.0, invalid).is_none());
    for entity in [center, seed] {
        let mut invalid = instances.clone();
        invalid[1].entities[0] = entity;
        assert!(SketchCircularPattern::new(
            pattern.center().clone(),
            Angle(-1.0),
            None,
            None,
            invalid
        )
        .is_none());
    }
    let wire = serde_json::to_value(&pattern).unwrap();
    assert_eq!(
        serde_json::from_value::<SketchCircularPattern>(wire.clone()).unwrap(),
        pattern
    );
    for (path, value) in [
        ("angle", serde_json::json!(f64::MIN_POSITIVE)),
        ("entities", serde_json::json!([pattern.center()])),
    ] {
        let mut invalid = wire.clone();
        invalid["instances"][0][path] = value;
        assert!(serde_json::from_value::<SketchCircularPattern>(invalid).is_err());
    }
    let mut duplicate = wire;
    duplicate["instances"][1]["entities"] = duplicate["instances"][0]["entities"].clone();
    assert!(serde_json::from_value::<SketchCircularPattern>(duplicate).is_err());
}

#[test]
fn rectangular_pattern_admission_checks_directions_and_distinct_members() {
    use crate::features::Length;
    use crate::sketches::{
        SketchEntityId, SketchPatternDirection, SketchPatternInstance, SketchRectangularPattern,
    };

    for direction in [
        [0.0, 0.0],
        [2.0, 0.0],
        [f64::NAN, 1.0],
        [1.0, f64::INFINITY],
    ] {
        assert!(SketchPatternDirection::new(direction, Length(1.0), None, None).is_none());
    }
    for spacing in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SketchPatternDirection::new([1.0, 0.0], Length(spacing), None, None).is_none());
    }
    let first = SketchPatternDirection::new([1.0, 0.0], Length(-2.0), None, None).unwrap();
    let second = SketchPatternDirection::new([0.0, 1.0], Length(0.0), None, None).unwrap();
    let instance = SketchPatternInstance {
        entities: vec![SketchEntityId::mint("test:test:sketch-entity#seed").unwrap()],
    };
    assert!(SketchRectangularPattern::new(
        [first.clone(), first.clone()],
        vec![vec![instance.clone()]]
    )
    .is_none());
    assert!(SketchRectangularPattern::new(
        [first.clone(), second.clone()],
        vec![vec![instance.clone(), instance.clone()]]
    )
    .is_none());
    let duplicate = SketchPatternInstance {
        entities: vec![instance.entities[0].clone(), instance.entities[0].clone()],
    };
    assert!(
        SketchRectangularPattern::new([first.clone(), second.clone()], vec![vec![duplicate]])
            .is_none()
    );
    let pattern = SketchRectangularPattern::new([first, second], vec![vec![instance]]).unwrap();
    let wire = serde_json::to_value(&pattern).unwrap();
    assert_eq!(
        serde_json::from_value::<SketchRectangularPattern>(wire.clone()).unwrap(),
        pattern
    );
    for direction in [[0.0, 0.0], [2.0, 0.0], [0.0, 1.0]] {
        let mut invalid = wire.clone();
        invalid["directions"][0]["direction"] = serde_json::json!(direction);
        assert!(serde_json::from_value::<SketchRectangularPattern>(invalid).is_err());
    }
}

#[test]
fn constraint_admission_rejects_local_arity_and_distinctness_on_every_route() {
    use crate::features::{Length, ParameterId};
    use crate::sketches::{
        SketchConstraintDefinition, SketchConstraintDefinitionInput as Kind,
        SketchDistanceMeasurement, SketchEntityId, SketchLocus, SketchOffsetPair,
    };

    let a = SketchEntityId::mint("test:test:sketch-entity#a").unwrap();
    let b = SketchEntityId::mint("test:test:sketch-entity#b").unwrap();
    let parameter = ParameterId::mint("test:test:parameter#distance").unwrap();
    let pair = SketchOffsetPair {
        source: a.clone(),
        result: b.clone(),
        source_reversed: false,
    };
    let invalid = vec![
        Kind::Coincident {
            entities: vec![a.clone()],
        },
        Kind::SplineGroup {
            entities: vec![a.clone()],
        },
        Kind::CoincidentLoci {
            loci: vec![SketchLocus::Start(a.clone())],
        },
        Kind::Distance {
            entities: vec![],
            parameter: parameter.clone(),
        },
        Kind::TextFrame {
            text: a.clone(),
            frame: vec![],
        },
        Kind::TextFrame {
            text: a.clone(),
            frame: vec![a.clone()],
        },
        Kind::TextPath {
            text: a.clone(),
            path: b.clone(),
            glyph_transforms: vec![],
        },
        Kind::TextPath {
            text: a.clone(),
            path: a.clone(),
            glyph_transforms: vec![crate::transform::Transform::identity()],
        },
        Kind::ScalarEquality {
            first: 1,
            second: 1,
        },
        Kind::ProjectedCopy {
            source: a.clone(),
            result: a.clone(),
        },
        Kind::RepeatedDistance {
            measurements: vec![],
            parameter: parameter.clone(),
        },
        Kind::RepeatedDistance {
            measurements: vec![SketchDistanceMeasurement::Distance {
                first: SketchLocus::Start(a.clone()),
                second: SketchLocus::End(a.clone()),
            }],
            parameter: parameter.clone(),
        },
        Kind::RepeatedLength {
            entities: vec![a.clone()],
            parameter: parameter.clone(),
        },
        Kind::RepeatedRadius {
            entities: vec![a.clone(), a.clone()],
            parameter: parameter.clone(),
        },
        Kind::RepeatedDiameter {
            entities: vec![a.clone(), a.clone()],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![],
            second: vec![b.clone()],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![a.clone()],
            second: vec![b.clone()],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![a.clone(), b.clone()],
            second: vec![b.clone()],
            parameter,
        },
        Kind::Offset {
            pairs: vec![],
            distance: Length(1.0),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: a.clone(),
                source_reversed: false,
            }],
            distance: Length(1.0),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![pair.clone(), pair],
            distance: Length(1.0),
            parameter: None,
        },
        Kind::Group { elements: vec![] },
        Kind::Text {
            elements: vec![],
            text: "text".into(),
            font: None,
            is_text_height: false,
        },
        Kind::Native {
            native_kind: String::new(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::default(),
            entities: vec![a],
            parameter: None,
            operands: vec![],
        },
        Kind::Native {
            native_kind: "native".into(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::default(),
            entities: vec![],
            parameter: None,
            operands: vec![],
        },
    ];
    let mut admitted = SketchConstraintDefinition::try_from(Kind::Disabled).unwrap();
    let original = admitted.clone();
    for kind in invalid {
        let wire = serde_json::to_value(&kind).unwrap();
        assert!(
            SketchConstraintDefinition::try_from(kind.clone()).is_err(),
            "{kind:?}"
        );
        assert!(
            serde_json::from_value::<SketchConstraintDefinition>(wire).is_err(),
            "{kind:?}"
        );
        assert!(admitted.edit(|current| *current = kind).is_err());
        assert_eq!(admitted, original);
    }
}

#[test]
fn constraint_admission_checks_scalar_bounds_and_polar_angle_presence() {
    use crate::features::{Angle, Length};
    use crate::sketches::{
        SketchConstraintDefinition, SketchConstraintDefinitionInput as Kind, SketchCoordinateAxis,
        SketchEntityId, SketchLabelValue, SketchLocus, SketchOffsetPair,
    };

    let a = SketchEntityId::mint("test:test:sketch-entity#a").unwrap();
    let b = SketchEntityId::mint("test:test:sketch-entity#b").unwrap();
    let first = SketchLocus::Start(a.clone());
    let second = SketchLocus::End(b.clone());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SketchLabelValue::try_from(value).is_err());
        for kind in [
            Kind::DistanceLociValue {
                first: first.clone(),
                second: second.clone(),
                distance: Length(value),
                parameter: None,
            },
            Kind::PointCoordinateValues {
                point: first.clone(),
                values: [Length(0.0), Length(value)],
            },
            Kind::MidpointCoordinate {
                first: first.clone(),
                second: second.clone(),
                axis: SketchCoordinateAxis::U,
                value: Length(value),
            },
            Kind::Offset {
                pairs: vec![SketchOffsetPair {
                    source: a.clone(),
                    result: b.clone(),
                    source_reversed: false,
                }],
                distance: Length(value),
                parameter: None,
            },
            Kind::PolarDistance {
                first: first.clone(),
                second: second.clone(),
                distance: Length(1.0),
                angle: Some(Angle(value)),
                distance_parameter: None,
            },
        ] {
            assert!(SketchConstraintDefinition::try_from(kind).is_err());
        }
    }
    for kind in [
        Kind::DistanceLociValue {
            first: first.clone(),
            second: second.clone(),
            distance: Length(-1.0),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: b.clone(),
                source_reversed: false,
            }],
            distance: Length(0.0),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: b.clone(),
                source_reversed: false,
            }],
            distance: Length(-1.0),
            parameter: None,
        },
        Kind::AngleDifference {
            first: 1,
            second: 2,
            difference: 3,
            value: Angle(std::f64::consts::TAU),
        },
    ] {
        let wire = serde_json::to_value(&kind).unwrap();
        assert!(SketchConstraintDefinition::try_from(kind).is_err());
        assert!(serde_json::from_value::<SketchConstraintDefinition>(wire).is_err());
    }
    for value in [-1.0, std::f64::consts::TAU, f64::NAN, f64::INFINITY] {
        assert!(SketchConstraintDefinition::try_from(Kind::AngleDifference {
            first: 1,
            second: 2,
            difference: 3,
            value: Angle(value)
        })
        .is_err());
    }
    for (distance, angle, valid) in [
        (-1.0, None, false),
        (f64::INFINITY, Some(Angle(0.0)), false),
        (0.0, None, true),
        (0.0, Some(Angle(0.0)), false),
        (super::EPS_POLAR_DISTANCE_ZERO, None, true),
        (super::EPS_POLAR_DISTANCE_ZERO, Some(Angle(0.0)), false),
        (2.0 * super::EPS_POLAR_DISTANCE_ZERO, None, false),
        (
            2.0 * super::EPS_POLAR_DISTANCE_ZERO,
            Some(Angle(-1.0)),
            true,
        ),
    ] {
        let kind = Kind::PolarDistance {
            first: first.clone(),
            second: second.clone(),
            distance: Length(distance),
            angle,
            distance_parameter: None,
        };
        assert_eq!(SketchConstraintDefinition::try_from(kind).is_ok(), valid);
    }
    for value in [0.0, std::f64::consts::PI] {
        let definition = SketchConstraintDefinition::try_from(Kind::AngleDifference {
            first: 1,
            second: 2,
            difference: 3,
            value: Angle(value),
        })
        .unwrap();
        assert_eq!(
            serde_json::from_value::<SketchConstraintDefinition>(
                serde_json::to_value(&definition).unwrap()
            )
            .unwrap(),
            definition
        );
    }
    assert_eq!(SketchLabelValue::try_from(-1.0).unwrap().get(), -1.0);
}

#[test]
fn spatial_constraint_admission_rejects_local_invalid_states() {
    use super::{
        SpatialSketchConstraintDefinition as Checked,
        SpatialSketchConstraintDefinitionInput as Kind, SpatialSketchEntityId,
        SpatialSketchEntityPair,
    };
    use crate::features::Length;
    let id =
        |name: &str| SpatialSketchEntityId::mint(format!("test:entity:spatial#{name}")).unwrap();
    let a = id("a");
    let b = id("b");
    let c = id("c");
    let parameter = crate::features::ParameterId::mint("test:parameter:length#p").unwrap();
    let mut invalid = vec![
        Kind::Coincident {
            first: a.clone(),
            second: a.clone(),
        },
        Kind::Symmetric {
            first: a.clone(),
            second: b.clone(),
            axis: a.clone(),
        },
        Kind::PointOnSurface {
            point: a.clone(),
            surface: a.clone(),
        },
        Kind::Midpoint {
            point: a.clone(),
            entity: a.clone(),
        },
        Kind::Tangent {
            first: a.clone(),
            second: a.clone(),
        },
        Kind::PointDistance {
            first: a.clone(),
            second: a.clone(),
            parameter: parameter.clone(),
        },
        Kind::PointLineDistance {
            point: a.clone(),
            line: a.clone(),
            parameter: parameter.clone(),
        },
        Kind::ParallelLineDistance {
            first: a.clone(),
            second: a.clone(),
            parameter: parameter.clone(),
        },
        Kind::RepeatedLineLength {
            entities: vec![a.clone()],
            parameter: parameter.clone(),
        },
        Kind::RepeatedLineLength {
            entities: vec![a.clone(), a.clone()],
            parameter: parameter.clone(),
        },
        Kind::SplineGroup { entities: vec![] },
        Kind::SplineGroup {
            entities: vec![a.clone(), a.clone()],
        },
        Kind::RepeatedParallelLineDistance {
            pairs: vec![SpatialSketchEntityPair {
                first: a.clone(),
                second: b.clone(),
            }],
            parameter: parameter.clone(),
        },
        Kind::RepeatedParallelLineDistance {
            pairs: vec![
                SpatialSketchEntityPair {
                    first: a.clone(),
                    second: b.clone(),
                },
                SpatialSketchEntityPair {
                    first: b.clone(),
                    second: c.clone(),
                },
            ],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![],
            second: vec![a.clone(), b.clone()],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![a.clone()],
            second: vec![b.clone()],
            parameter: parameter.clone(),
        },
        Kind::ParallelLineSetDistance {
            first: vec![a.clone(), b.clone()],
            second: vec![a.clone()],
            parameter,
        },
    ];
    let offset = Kind::Offset {
        sources: vec![a.clone()],
        results: vec![b.clone()],
        normal: Vector3::new(0.0, 0.0, 1.0),
        distance: Length(1.0),
        parameter: None,
    };
    for distance in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        let mut kind = offset.clone();
        if let Kind::Offset {
            distance: value, ..
        } = &mut kind
        {
            *value = Length(distance);
        }
        invalid.push(kind);
    }
    for normal in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 0.0, 0.0),
    ] {
        invalid.push(Kind::ParallelToDirection {
            entity: a.clone(),
            direction: normal,
        });
        let mut kind = offset.clone();
        if let Kind::Offset { normal: value, .. } = &mut kind {
            *value = normal;
        }
        invalid.push(kind);
    }
    for (sources, results) in [
        (vec![], vec![b.clone()]),
        (vec![a.clone()], vec![]),
        (vec![a.clone()], vec![a.clone()]),
    ] {
        let mut kind = offset.clone();
        if let Kind::Offset {
            sources: s,
            results: r,
            ..
        } = &mut kind
        {
            *s = sources;
            *r = results;
        }
        invalid.push(kind);
    }
    let mut checked = Checked::try_from(offset.clone()).unwrap();
    let wire = serde_json::to_value(&checked).unwrap();
    assert_eq!(wire, serde_json::to_value(&offset).unwrap());
    assert_eq!(serde_json::from_value::<Checked>(wire).unwrap(), checked);
    for kind in invalid {
        assert!(Checked::try_from(kind.clone()).is_err(), "{kind:?}");
        assert!(serde_json::from_value::<Checked>(serde_json::to_value(&kind).unwrap()).is_err());
        let before = checked.clone();
        assert!(checked.edit(|value| *value = kind).is_err());
        assert_eq!(checked, before);
    }
    assert!(Checked::try_from(Kind::ParallelToDirection {
        entity: a,
        direction: Vector3::new(0.0, 1.0, 0.0)
    })
    .is_ok());
}
