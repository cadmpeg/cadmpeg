// SPDX-License-Identifier: Apache-2.0
use crate::examples::unit_cube;
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;
use crate::CadIr;

const EPS_SPATIAL_LINE_BOUNDARY: f64 = 1.0e-12;
const EPS_SPATIAL_FRAME_BOUNDARY: f64 = 1.0e-9;

#[test]
fn spatial_sketch_geometry_round_trips_and_validates() {
    use crate::sketches::{
        OffsetParameter, SketchConstraintId, SpatialSketch, SpatialSketchConstraint,
        SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityId,
        SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
        SpatialSketchId, SpatialSketchProfile,
    };
    use crate::{
        features::{DesignParameter, ParameterId, ParameterValue},
        scalar::Length,
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
                radius: Length::new(4.0).unwrap(),
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
        value: Some(ParameterValue::Length(Length::new(2.0).unwrap())),
        dependencies: crate::features::DistinctMembers::default(),
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
        value: Some(ParameterValue::Length(Length::new(3.0f64.sqrt()).unwrap())),
        dependencies: crate::features::DistinctMembers::default(),
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
                    distance: Length::new(2.0).unwrap(),
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
        .find(|constraint| constraint.id.as_str().ends_with("#offset"))
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
        .value = Some(ParameterValue::Length(Length::new(3.0).unwrap()));
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

    let path = PathRef::spatial_sketch_curves(
        SpatialSketchId::mint("synthetic:test:spatial-sketch#0").unwrap(),
        vec![SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#0").unwrap()],
    )
    .unwrap();
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);

    let native = PathRef::spatial_sketch_selection(
        SpatialSketchId::mint("synthetic:test:spatial-sketch#0").unwrap(),
        vec!["native:path-selection#0".into()],
    )
    .unwrap();
    let json = serde_json::to_string(&native).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), native);
}

#[test]
fn spatial_nurbs_wire_preserves_flat_fields_and_checks_cardinality() {
    use crate::sketches::SpatialSketchGeometry;

    let wire = serde_json::json!({
        "kind": "nurbs",
        "curve": {
            "degree": 1,
            "knots": [0.0, 0.0, 1.0, 1.0],
            "control_points": [{"x": 2.0, "y": 3.0, "z": 4.0}, {"x": 5.0, "y": 6.0, "z": 7.0}],
            "weights": [1.0, 0.5], "periodic": false
        }
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
        invalid["curve"][field] = value;
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
        "kind": "nurbs_surface",
        "surface": {
            "u_degree": 1, "v_degree": 1,
            "u_knots": [0.0, 0.0, 1.0, 1.0], "v_knots": [0.0, 0.0, 1.0, 1.0],
            "control_points": [
                [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 0.0, "y": 1.0, "z": 0.0}],
                [{"x": 1.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 1.0, "z": 0.0}]
            ]
        }
    });
    let geometry: SpatialSketchGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&geometry).unwrap(), wire);
    for field in ["u_knots", "v_knots", "control_points"] {
        let mut invalid = wire.clone();
        invalid["surface"][field].as_array_mut().unwrap().pop();
        assert!(
            serde_json::from_value::<SpatialSketchGeometry>(invalid).is_err(),
            "{field}"
        );
    }
    let mut ragged = wire;
    ragged["surface"]["control_points"][1]
        .as_array_mut()
        .unwrap()
        .pop();
    let error = serde_json::from_value::<SpatialSketchGeometry>(ragged).unwrap_err();
    assert!(error.to_string().contains("control_points row"));
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
        let wire = serde_json::json!({"kind": "nurbs", "curve": curve});
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
    let wire = serde_json::json!({"kind": "nurbs", "curve": &curve});
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
fn spatial_analytic_geometry_checks_separation_frames_and_angles() {
    use crate::scalar::{Angle, Length};
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
            radius: Length::new(0.0).unwrap(),
        },
        Definition::Circle {
            center: origin,
            normal: Vector3::new(0.0, 0.0, 2.0),
            reference_direction,
            radius: Length::new(1.0).unwrap(),
        },
        Definition::Circle {
            center: origin,
            normal,
            reference_direction: normal,
            radius: Length::new(1.0).unwrap(),
        },
        Definition::Arc {
            center: origin,
            normal,
            reference_direction,
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(1.0).unwrap(),
            end_angle: Angle::new(1.0).unwrap(),
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
            radius: Length::new(1.0).unwrap(),
        },
        Definition::Circle {
            center: origin,
            normal,
            reference_direction: Vector3::new(1.0, 0.0, 0.5 * EPS_SPATIAL_FRAME_BOUNDARY),
            radius: Length::new(1.0).unwrap(),
        },
    ] {
        assert!(SpatialSketchGeometry::try_from(definition).is_ok());
    }
}

#[test]
fn spatial_analytic_geometry_preserves_wire_and_rejects_invalid_edits() {
    use crate::scalar::Angle;
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
            *end_angle = Angle::new(-3.0).unwrap();
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
fn spatial_constraint_admission_rejects_local_invalid_states() {
    use super::{
        SpatialSketchConstraintDefinition as Checked,
        SpatialSketchConstraintDefinitionInput as Kind, SpatialSketchEntityId,
        SpatialSketchEntityPair,
    };
    use crate::scalar::Length;
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
        distance: Length::new(1.0).unwrap(),
        parameter: None,
    };
    for distance in [0.0, -1.0] {
        let mut kind = offset.clone();
        if let Kind::Offset {
            distance: value, ..
        } = &mut kind
        {
            *value = Length::new(distance).unwrap();
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

#[test]
fn spatial_native_constraint_admission_requires_kind_and_operands() {
    use super::{
        SpatialSketchConstraintDefinition as Checked,
        SpatialSketchConstraintDefinitionInput as Kind,
    };
    use serde_json::json;

    let wire = json!({
        "kind": "native", "native_kind": "relation",
        "operands": [{"native_kind": "entity", "object_index": 1}]
    });
    let mut value: Checked = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), wire);
    let original = value.clone();
    assert!(value
        .edit(|kind| {
            let Kind::Native { operands, .. } = kind else {
                panic!("native relation")
            };
            operands.clear();
        })
        .is_err());
    assert_eq!(value, original);
    for (field, invalid) in [("native_kind", json!("")), ("operands", json!([]))] {
        let mut rejected = wire.clone();
        rejected[field] = invalid;
        assert!(serde_json::from_value::<Checked>(rejected).is_err());
    }
}
