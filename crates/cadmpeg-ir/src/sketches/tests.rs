// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    SpatialSketchConstraintDefinition, SpatialSketchConstraintDefinitionInput,
    SpatialSketchEntityId, SpatialSketchEntityPair,
};
use crate::examples::unit_cube;
use crate::math::{Point3, Vector3};
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

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
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        OffsetParameter, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
        SketchConstraintId, SketchDistanceMeasurement, SketchDistancePair, SketchEntity,
        SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
        SketchOffsetPair,
    };
    use crate::{features::ParameterId, scalar::Length};

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
            distance: Length::new(2.0).unwrap(),
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
    ir.model.sketch_entities[0].geometry = SketchGeometry::native(
        crate::products::NonEmptyString::new("center-bearing-curve")
            .expect("nonempty source identity"),
    );
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint_id.0.as_str())
            && finding.check == Check::GeometricConsistency
    }));
}

#[test]
fn coordinate_equation_constraints_round_trip_and_validate_geometry() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::scalar::Length;
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
                values: [Length::new(2.0).unwrap(), Length::new(1.0).unwrap()],
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-u").unwrap(),
            SketchConstraintDefinitionInput::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::U,
                value: Length::new(2.0).unwrap(),
            },
        ),
        (
            SketchConstraintId::mint("synthetic:test:constraint#mean-v").unwrap(),
            SketchConstraintDefinitionInput::MidpointCoordinate {
                first: SketchLocus::Entity(first.clone()),
                second: SketchLocus::Entity(second.clone()),
                axis: SketchCoordinateAxis::V,
                value: Length::new(1.0).unwrap(),
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

    let profile = ProfileRef::sketch_regions(
        SketchId::mint("synthetic:test:sketch#region").unwrap(),
        vec![
            SketchProfileRegion::loops(2, vec![3, 5]).unwrap(),
            SketchProfileRegion::loops(8, Vec::new()).unwrap(),
            SketchProfileRegion::trimmed(
                vec![SketchProfileBoundaryUse {
                    entity: SketchEntityId::mint("synthetic:test:sketch-entity#curve").unwrap(),
                    parameter_range: crate::geometry::DirectedParameterRange::new([0.25, 0.75])
                        .unwrap(),
                    reversed: true,
                }],
                Vec::new(),
            )
            .unwrap(),
        ],
    )
    .unwrap();
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

fn pattern_direction(axis: [f64; 2]) -> crate::sketches::SketchPatternDirection {
    crate::sketches::SketchPatternDirection::new(
        axis,
        crate::scalar::Length::new(2.0).unwrap(),
        None,
        None,
    )
    .unwrap()
}

#[test]
fn a_rectangular_pattern_states_its_grid_as_rows() {
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
    assert!(wire["pattern"]["directions"][0].get("count").is_none());
    assert!(wire["pattern"]["directions"][1].get("count").is_none());
    assert_eq!(
        wire["pattern"]["directions"][0]["spacing_parameter"],
        "test:test:parameter#spacing"
    );
    assert!(wire["pattern"]["directions"][0]
        .get("span_parameter")
        .is_none());
    assert!(wire.get("instances").is_none());
    assert!(wire["pattern"].get("instances").is_none());
    assert_eq!(
        wire["pattern"]["rows"],
        serde_json::json!([
            [{ "entities": ["test:test:sketch-entity#0"] }],
            [{ "entities": ["test:test:sketch-entity#1"] }]
        ])
    );
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut restated_count = wire.clone();
    restated_count["pattern"]["directions"][0]
        .as_object_mut()
        .unwrap()
        .insert("count".to_string(), serde_json::json!(2));
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(restated_count)
        .unwrap_err()
        .to_string();
    assert!(error.contains("count"), "{error}");

    let mut restated_indices = wire.clone();
    restated_indices["pattern"]["rows"][0][0]
        .as_object_mut()
        .unwrap()
        .insert("indices".to_string(), serde_json::json!([0, 0]));
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(restated_indices)
        .unwrap_err()
        .to_string();
    assert!(error.contains("indices"), "{error}");

    let mut conflicting_distance = wire.clone();
    conflicting_distance["pattern"]["directions"][0]["span_parameter"] =
        serde_json::json!("test:parameter#span");
    assert!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(conflicting_distance).is_err()
    );

    let mut ragged = wire;
    ragged["pattern"]["rows"][1] = serde_json::json!([]);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(ragged).is_err());
}

#[test]
fn a_circular_pattern_states_its_count_only_as_instances() {
    use crate::scalar::Angle;
    use crate::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance, SketchConstraintDefinitionInput,
        SketchEntityId,
    };

    let pattern = SketchCircularPattern::new(
        SketchEntityId::mint("test:test:sketch-entity#center").unwrap(),
        Angle::new(1.0).unwrap(),
        None,
        None,
        vec![
            SketchCircularPatternInstance {
                angle: Angle::new(0.0).unwrap(),
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#0").unwrap()],
            },
            SketchCircularPatternInstance {
                angle: Angle::new(1.0).unwrap(),
                entities: vec![SketchEntityId::mint("test:test:sketch-entity#1").unwrap()],
            },
        ],
    )
    .unwrap();
    let definition = SketchConstraintDefinitionInput::CircularPattern { pattern };
    let wire = serde_json::to_value(&definition).unwrap();
    assert!(wire["pattern"].get("count").is_none());
    assert!(wire["pattern"]["instances"][0].get("index").is_none());
    assert_eq!(
        wire["pattern"]["instances"],
        serde_json::json!([
            { "angle": 0.0, "entities": ["test:test:sketch-entity#0"] },
            { "angle": 1.0, "entities": ["test:test:sketch-entity#1"] }
        ])
    );
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut restated_count = wire.clone();
    restated_count["pattern"]
        .as_object_mut()
        .unwrap()
        .insert("count".to_string(), serde_json::json!(2));
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(restated_count)
        .unwrap_err()
        .to_string();
    assert!(error.contains("count"), "{error}");

    let mut restated_index = wire.clone();
    restated_index["pattern"]["instances"][0]
        .as_object_mut()
        .unwrap()
        .insert("index".to_string(), serde_json::json!(0));
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(restated_index)
        .unwrap_err()
        .to_string();
    assert!(error.contains("index"), "{error}");

    let mut unseeded = wire;
    unseeded["pattern"]["instances"][0]["angle"] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(unseeded).is_err());
}

#[test]
fn the_offset_parameter_is_one_nested_key_with_an_explicit_sign() {
    use crate::sketches::{
        OffsetParameter, SketchConstraintDefinitionInput, SketchEntityId, SketchOffsetPair,
    };
    use crate::{features::ParameterId, scalar::Length};

    let definition = SketchConstraintDefinitionInput::Offset {
        pairs: vec![SketchOffsetPair {
            source: SketchEntityId::mint("test:test:sketch-entity#source").unwrap(),
            result: SketchEntityId::mint("test:test:sketch-entity#result").unwrap(),
            source_reversed: false,
        }],
        distance: Length::new(2.0).unwrap(),
        parameter: Some(OffsetParameter {
            id: ParameterId::mint("test:test:parameter#offset").expect("identity grammar"),
            negated: true,
        }),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        wire["parameter"],
        serde_json::json!({"id": "test:test:parameter#offset", "negated": true})
    );
    assert!(wire.get("parameter_factor").is_none());
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut half = wire.clone();
    half["parameter"].as_object_mut().unwrap().remove("negated");
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(half).is_err());

    let mut bogus = wire;
    bogus["parameter"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn the_conic_bounds_are_one_nested_pair_or_absent() {
    use crate::math::Point2;
    use crate::scalar::{Angle, Length};
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let cases = [
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle::new(0.25).unwrap(),
            major_radius: Length::new(4.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: Some([Angle::new(-0.5).unwrap(), Angle::new(1.5).unwrap()]),
        })
        .unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Hyperbola {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle::new(0.25).unwrap(),
            major_radius: Length::new(4.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: Some([-0.5, 1.5]),
        })
        .unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
            vertex: Point2::new(1.0, 2.0),
            axis_angle: Angle::new(0.25).unwrap(),
            focal_length: Length::new(2.0).unwrap(),
            bounds: Some([-0.5, 1.5]),
        })
        .unwrap(),
    ];

    for geometry in cases {
        let wire = serde_json::to_value(&geometry).unwrap();
        assert!(matches!(
            geometry.definition(),
            SketchGeometryDefinition::Ellipse { .. }
                | SketchGeometryDefinition::Hyperbola { .. }
                | SketchGeometryDefinition::Parabola { .. }
        ));
        assert_eq!(wire["bounds"], serde_json::json!([-0.5, 1.5]));
        assert!(wire.get("start_angle").is_none());
        assert!(wire.get("start_parameter").is_none());
        assert_eq!(
            serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap(),
            geometry
        );

        let mut half = wire.clone();
        half["bounds"] = serde_json::json!([-0.5]);
        assert!(serde_json::from_value::<SketchGeometry>(half).is_err());

        let mut absent = wire;
        absent.as_object_mut().unwrap().remove("bounds");
        let unbounded = serde_json::from_value::<SketchGeometry>(absent).unwrap();
        assert_ne!(unbounded, geometry);
    }
}

#[test]
fn the_text_placement_is_one_nested_key_or_absent() {
    use crate::math::Point2;
    use crate::scalar::{Angle, Length};
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition, TextPlacement};

    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: crate::products::NonEmptyString::new("cadmpeg").unwrap(),
        font_family: crate::products::NonEmptyString::new("sans").unwrap(),
        font_weight: crate::sketches::SketchFontWeight::Regular,
        height: Length::new(4.0).unwrap(),
        width_factor: None,
        placement: Some(TextPlacement {
            anchor: Point2::new(1.0, 2.0),
            rotation: Angle::new(0.5).unwrap(),
        }),
        horizontal_alignment: None,
        vertical_alignment: None,
    })
    .unwrap();
    let wire = serde_json::to_value(&geometry).unwrap();
    assert_eq!(
        wire["placement"],
        serde_json::json!({"anchor": {"u": 1.0, "v": 2.0}, "rotation": 0.5})
    );
    assert!(wire.get("anchor").is_none());
    assert_eq!(
        serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap(),
        geometry
    );

    let mut half = wire.clone();
    half["placement"]
        .as_object_mut()
        .unwrap()
        .remove("rotation");
    assert!(serde_json::from_value::<SketchGeometry>(half).is_err());

    let mut bogus = wire;
    bogus["placement"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<SketchGeometry>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
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
    assert_eq!(wire["alignment"]["alignment"], "bspline_control_point");
    assert_eq!(wire["alignment"]["index"], 2);
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).unwrap(),
        definition
    );

    let mut missing_index = wire.clone();
    missing_index["alignment"]
        .as_object_mut()
        .unwrap()
        .remove("index");
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(missing_index).is_err());

    let mut extraneous_index = wire;
    extraneous_index["alignment"]["alignment"] = serde_json::json!("ellipse_focus1");
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(extraneous_index)
        .unwrap_err()
        .to_string();
    assert!(error.contains("index"), "{error}");
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
            "id": "test:test:sketch-constraint#axis",
            "sketch": "test:test:sketch#axis",
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
fn a_solver_scalar_slot_is_its_key_and_carries_no_class_key() {
    use crate::sketches::SketchConstraintDefinitionInput;
    let angle = SketchConstraintDefinitionInput::AngleDifference {
        first: 17,
        second: 18,
        difference: 19,
        value: crate::scalar::Angle::new(0.5).unwrap(),
    };
    let wire = serde_json::to_value(&angle).unwrap();
    assert_eq!(wire["first"], serde_json::json!(17));
    assert_eq!(wire["second"], serde_json::json!(18));
    assert_eq!(wire["difference"], serde_json::json!(19));
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
        angle
    );

    let equality = SketchConstraintDefinitionInput::ScalarEquality {
        first: 17,
        second: 18,
    };
    let wire = serde_json::to_value(&equality).unwrap();
    assert_eq!(wire["first"], serde_json::json!(17));
    assert_eq!(wire["second"], serde_json::json!(18));
    assert_eq!(
        serde_json::from_value::<SketchConstraintDefinitionInput>(wire).unwrap(),
        equality
    );
}

#[test]
fn a_solver_scalar_slot_rejects_the_deleted_variable_type_key() {
    use crate::sketches::SketchConstraintDefinitionInput;
    let mut wire = serde_json::to_value(SketchConstraintDefinitionInput::ScalarEquality {
        first: 17,
        second: 18,
    })
    .unwrap();
    wire["first"] = serde_json::json!({"variable_type": 6, "key": 17});
    let error = serde_json::from_value::<SketchConstraintDefinitionInput>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("invalid type: map"), "{error}");
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
    use crate::math::Point2;
    use crate::scalar::{Angle, Length};
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
            radius: Length::new(0.0).unwrap(),
        },
        Definition::Ellipse {
            center: point,
            major_angle: Angle::new(0.0).unwrap(),
            major_radius: Length::new(1.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: None,
        },
        Definition::Hyperbola {
            center: point,
            major_angle: Angle::new(0.0).unwrap(),
            major_radius: Length::new(1.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: Some([0.0, f64::INFINITY]),
        },
        Definition::Parabola {
            vertex: point,
            axis_angle: Angle::new(0.0).unwrap(),
            focal_length: Length::new(-1.0).unwrap(),
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
    use crate::scalar::Length;
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
            *radius = Length::new(-1.0).unwrap();
        })
        .is_err());
    assert_eq!(geometry, original);
    geometry
        .edit(|definition| {
            let SketchGeometryDefinition::Arc { radius, .. } = definition else {
                panic!("arc")
            };
            *radius = Length::new(2.0).unwrap();
        })
        .unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap()["radius"], 2.0);
}

#[test]
fn planar_text_numeric_fields_are_checked_on_every_admission_route() {
    use crate::scalar::Length;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};

    let wire = serde_json::json!({
        "kind": "text", "text": "A", "font_family": "Arial", "font_weight": 400,
        "height": 2.0, "width_factor": 1.0,
        "placement": {"anchor": {"u": 0.0, "v": 0.0}, "rotation": -1.0}
    });
    let geometry = serde_json::from_value::<SketchGeometry>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for field in ["height", "width_factor"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<SketchGeometry>(invalid).is_err());
    }
    let mut invalid_rotation = wire.clone();
    invalid_rotation["placement"]["rotation"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<SketchGeometry>(invalid_rotation).is_err());
    for field in 0..3 {
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
            0 => *height = Length::new(0.0).unwrap(),
            1 => *width_factor = Some(f64::NAN),
            _ => placement.anchor.u = f64::INFINITY,
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
    use crate::scalar::Angle;
    use crate::sketches::{SketchCircularPattern, SketchCircularPatternInstance, SketchEntityId};

    let center = SketchEntityId::mint("test:test:sketch-entity#center").unwrap();
    let seed = SketchEntityId::mint("test:test:sketch-entity#seed").unwrap();
    let copy = SketchEntityId::mint("test:test:sketch-entity#copy").unwrap();
    let instances = vec![
        SketchCircularPatternInstance {
            angle: Angle::new(0.0).unwrap(),
            entities: vec![seed.clone()],
        },
        SketchCircularPatternInstance {
            angle: Angle::new(-1.0).unwrap(),
            entities: vec![copy],
        },
    ];
    let admit = |angle, instances| {
        SketchCircularPattern::new(
            center.clone(),
            Angle::new(angle).unwrap(),
            None,
            None,
            instances,
        )
    };
    let pattern = admit(-1.0, instances.clone()).unwrap();
    let mut invalid = instances.clone();
    invalid[0].angle = Angle::new(f64::MIN_POSITIVE).unwrap();
    assert!(admit(-1.0, invalid).is_none());
    for entity in [center, seed] {
        let mut invalid = instances.clone();
        invalid[1].entities[0] = entity;
        assert!(SketchCircularPattern::new(
            pattern.center().clone(),
            Angle::new(-1.0).unwrap(),
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
    use crate::scalar::Length;
    use crate::sketches::{
        SketchEntityId, SketchPatternDirection, SketchPatternInstance, SketchRectangularPattern,
    };

    for direction in [
        [0.0, 0.0],
        [2.0, 0.0],
        [f64::NAN, 1.0],
        [1.0, f64::INFINITY],
    ] {
        assert!(
            SketchPatternDirection::new(direction, Length::new(1.0).unwrap(), None, None).is_none()
        );
    }
    let first =
        SketchPatternDirection::new([1.0, 0.0], Length::new(-2.0).unwrap(), None, None).unwrap();
    let second =
        SketchPatternDirection::new([0.0, 1.0], Length::new(0.0).unwrap(), None, None).unwrap();
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
    use crate::sketches::{
        SketchConstraintDefinition, SketchConstraintDefinitionInput as Kind,
        SketchDistanceMeasurement, SketchEntityId, SketchLocus, SketchOffsetPair,
    };
    use crate::{features::ParameterId, scalar::Length};

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
            distance: Length::new(1.0).unwrap(),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: a.clone(),
                source_reversed: false,
            }],
            distance: Length::new(1.0).unwrap(),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![pair.clone(), pair],
            distance: Length::new(1.0).unwrap(),
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
            native_kind: crate::products::NonEmptyString::new("native").unwrap(),
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
    use crate::scalar::{Angle, Length};
    use crate::sketches::{
        SketchConstraintDefinition, SketchConstraintDefinitionInput as Kind, SketchEntityId,
        SketchLabelValue, SketchLocus, SketchOffsetPair,
    };

    let a = SketchEntityId::mint("test:test:sketch-entity#a").unwrap();
    let b = SketchEntityId::mint("test:test:sketch-entity#b").unwrap();
    let first = SketchLocus::Start(a.clone());
    let second = SketchLocus::End(b.clone());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SketchLabelValue::try_from(value).is_err());
    }

    for kind in [
        Kind::DistanceLociValue {
            first: first.clone(),
            second: second.clone(),
            distance: Length::new(-1.0).unwrap(),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: b.clone(),
                source_reversed: false,
            }],
            distance: Length::new(0.0).unwrap(),
            parameter: None,
        },
        Kind::Offset {
            pairs: vec![SketchOffsetPair {
                source: a.clone(),
                result: b.clone(),
                source_reversed: false,
            }],
            distance: Length::new(-1.0).unwrap(),
            parameter: None,
        },
        Kind::AngleDifference {
            first: 1,
            second: 2,
            difference: 3,
            value: Angle::new(std::f64::consts::TAU).unwrap(),
        },
    ] {
        let wire = serde_json::to_value(&kind).unwrap();
        assert!(SketchConstraintDefinition::try_from(kind).is_err());
        assert!(serde_json::from_value::<SketchConstraintDefinition>(wire).is_err());
    }
    for value in [-1.0, std::f64::consts::TAU] {
        assert!(SketchConstraintDefinition::try_from(Kind::AngleDifference {
            first: 1,
            second: 2,
            difference: 3,
            value: Angle::new(value).unwrap()
        })
        .is_err());
    }
    for (distance, angle, valid) in [
        (-1.0, None, false),
        (0.0, None, true),
        (0.0, Some(Angle::new(0.0).unwrap()), false),
        (super::EPS_POLAR_DISTANCE_ZERO, None, true),
        (
            super::EPS_POLAR_DISTANCE_ZERO,
            Some(Angle::new(0.0).unwrap()),
            false,
        ),
        (2.0 * super::EPS_POLAR_DISTANCE_ZERO, None, false),
        (
            2.0 * super::EPS_POLAR_DISTANCE_ZERO,
            Some(Angle::new(-1.0).unwrap()),
            true,
        ),
    ] {
        let kind = Kind::PolarDistance {
            first: first.clone(),
            second: second.clone(),
            distance: Length::new(distance).unwrap(),
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
            value: Angle::new(value).unwrap(),
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
fn native_and_external_geometry_identity_admission_preserves_wire() {
    use super::{SketchGeometry, SpatialSketchGeometry};
    for (wire, field) in [
        (
            serde_json::json!({"kind": "native", "native_kind": "line"}),
            "native_kind",
        ),
        (
            serde_json::json!({"kind": "external_reference", "object": "Sketch001"}),
            "object",
        ),
    ] {
        let value: SketchGeometry =
            serde_json::from_value(wire.clone()).expect("nonempty identity");
        assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        let mut invalid = wire;
        invalid[field] = serde_json::json!("");
        let error = serde_json::from_value::<SketchGeometry>(invalid).expect_err("empty identity");
        assert!(error.to_string().contains(field));
    }
    let wire = serde_json::json!({"kind": "native", "native_kind": "curve"});
    let value: SpatialSketchGeometry =
        serde_json::from_value(wire.clone()).expect("nonempty native_kind");
    assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
    let error = serde_json::from_value::<SpatialSketchGeometry>(
        serde_json::json!({"kind": "native", "native_kind": ""}),
    )
    .expect_err("empty native_kind");
    assert!(error.to_string().contains("native_kind"));
}

#[test]
fn native_constraint_kind_rejects_empty_text_at_input_admission() {
    use crate::sketches::{SketchConstraintDefinition, SketchConstraintDefinitionInput};
    let mut wire = serde_json::json!({
        "kind": "native", "native_kind": "source", "entities": ["test:sketch:entity#0"]
    });
    let admitted = serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    wire["native_kind"] = "".into();
    assert!(serde_json::from_value::<SketchConstraintDefinitionInput>(wire.clone()).is_err());
    assert!(serde_json::from_value::<SketchConstraintDefinition>(wire).is_err());
}

mod spatial;
