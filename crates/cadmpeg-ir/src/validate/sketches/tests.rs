// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::sketch_curve_offset_matches;
use crate::examples::unit_cube;
use crate::features::{Angle, ExtrudeDirection, Length};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::sketches::SketchGeometry;
use crate::validate::validate_neutral;
use crate::CadIr;

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;

#[test]
fn trimmed_concentric_arcs_validate_as_offsets() {
    let arc = |radius, start, end| SketchGeometry::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length(radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    };
    let source = arc(2.0, 0.0, std::f64::consts::FRAC_PI_2);
    let trimmed_result = arc(5.0, 0.1, 1.4);
    let disjoint_result = arc(5.0, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);

    assert!(sketch_curve_offset_matches(
        &source,
        &trimmed_result,
        -3.0,
        1.0e-6,
    ));
    assert!(!sketch_curve_offset_matches(
        &source,
        &disjoint_result,
        -3.0,
        1.0e-6,
    ));
}

#[test]
fn full_concentric_circles_validate_as_offsets() {
    let circle = |radius| SketchGeometry::Circle {
        center: Point2::new(3.0, -4.0),
        radius: Length(radius),
    };
    let source = circle(5.0);
    let result = circle(3.5);
    let displaced = SketchGeometry::Circle {
        center: Point2::new(3.0, -3.9),
        radius: Length(3.5),
    };

    assert!(sketch_curve_offset_matches(
        &source,
        &result,
        1.5,
        TEST_LINEAR_TOLERANCE,
    ));
    assert!(sketch_curve_offset_matches(
        &result,
        &source,
        -1.5,
        TEST_LINEAR_TOLERANCE,
    ));
    assert!(!sketch_curve_offset_matches(
        &source,
        &displaced,
        1.5,
        TEST_LINEAR_TOLERANCE,
    ));
}

#[test]
fn mixed_full_circle_arc_validate_as_offsets() {
    let circle = SketchGeometry::Circle {
        center: Point2::new(3.0, -4.0),
        radius: Length(5.0),
    };
    let arc = SketchGeometry::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length(3.5),
        start_angle: Angle(0.1),
        end_angle: Angle(1.4),
    };
    let displaced = SketchGeometry::Arc {
        center: Point2::new(3.1, -4.0),
        radius: Length(3.5),
        start_angle: Angle(0.1),
        end_angle: Angle(1.4),
    };

    assert!(sketch_curve_offset_matches(
        &circle,
        &arc,
        1.5,
        TEST_LINEAR_TOLERANCE,
    ));
    assert!(sketch_curve_offset_matches(
        &arc,
        &circle,
        -1.5,
        TEST_LINEAR_TOLERANCE,
    ));
    assert!(!sketch_curve_offset_matches(
        &circle,
        &displaced,
        1.5,
        TEST_LINEAR_TOLERANCE,
    ));
}

#[test]
fn malformed_sketch_geometry_and_constraints_are_rejected() {
    use crate::features::Length;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#0".into());
    let circle_id = SketchEntityId("synthetic:test:sketch-entity#0".into());
    let nurbs_id = SketchEntityId("synthetic:test:sketch-entity#1".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 1.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.extend([
        SketchEntity {
            id: circle_id.clone(),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(-1.0),
            },
        },
        SketchEntity {
            id: nurbs_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Nurbs {
                degree: 3,
                knots: vec![0.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0)],
                weights: Some(vec![0.0]),
                periodic: false,
            },
        },
    ]);
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Coincident {
            entities: vec![circle_id],
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
        finding.check == Check::GeometricConsistency
            && finding.entity.as_deref() == Some("synthetic:test:sketch#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Bounds
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#1")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Counts
            && finding.entity.as_deref() == Some("synthetic:test:sketch-constraint#0")
    }));
}

#[test]
fn fitted_nurbs_offsets_validate_from_clamped_endpoint_frames() {
    use crate::features::Length;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId, SketchOffsetPair,
    };

    let mut ir = CadIr::empty();
    let sketch = SketchId("synthetic:test:sketch#nurbs-offset".into());
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
    let source = SketchEntityId("synthetic:test:nurbs#source".into());
    let result = SketchEntityId("synthetic:test:nurbs#result".into());
    let result_start = Point2::new(-1.2, 1.6);
    let result_end = Point2::new(10.0 + 2.0 / 5.0_f64.sqrt(), 4.0 / 5.0_f64.sqrt());
    ir.model.sketch_entities.extend([
        SketchEntity {
            id: source.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Nurbs {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                control_points: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(4.0, 3.0),
                    Point2::new(10.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
        },
        SketchEntity {
            id: result.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Nurbs {
                degree: 3,
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                control_points: vec![
                    result_start,
                    Point2::new(result_start.u + 2.0, result_start.v + 1.5),
                    Point2::new(result_end.u - 3.0, result_end.v + 1.5),
                    result_end,
                ],
                weights: None,
                periodic: false,
            },
        },
    ]);
    let constraint = SketchConstraintId("synthetic:test:constraint#nurbs-offset".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: SketchConstraintDefinition::Offset {
            pairs: vec![SketchOffsetPair {
                source: source.clone(),
                result: result.clone(),
                source_reversed: false,
            }],
            distance: Length(2.0),
            parameter: None,
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
    let source_ordinal = ir
        .model
        .sketch_entities
        .iter()
        .position(|entity| entity.id == source)
        .expect("source entity");
    let result_ordinal = ir
        .model
        .sketch_entities
        .iter()
        .position(|entity| entity.id == result)
        .expect("result entity");
    let offset_mismatch = |report: &crate::report::ValidationReport| {
        report.findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(constraint.0.as_str())
                && finding
                    .message
                    .contains("offset pair does not match its oriented distance")
        })
    };
    assert!(!offset_mismatch(&validate_neutral(&ir, Vec::new())));

    {
        let SketchGeometry::Nurbs { control_points, .. } =
            &mut ir.model.sketch_entities[result_ordinal].geometry
        else {
            unreachable!("test result is a NURBS")
        };
        control_points.reverse();
    }
    let reversed_distance = crate::eval::fitted_nurbs_offset_frame_distance(
        &ir.model.sketch_entities[source_ordinal].geometry,
        &ir.model.sketch_entities[result_ordinal].geometry,
        ir.tolerances.linear,
    )
    .expect("reversed fitted offset frame");
    assert!(
        (reversed_distance - 2.0).abs() <= 1.0e-9,
        "reversed fitted offset distance {reversed_distance}"
    );
    assert!(!offset_mismatch(&validate_neutral(&ir, Vec::new())));
    let SketchGeometry::Nurbs { control_points, .. } =
        &mut ir.model.sketch_entities[result_ordinal].geometry
    else {
        unreachable!("test result is a NURBS")
    };
    control_points.reverse();
    control_points.last_mut().expect("result endpoint").u += 0.01;
    assert!(offset_mismatch(&validate_neutral(&ir, Vec::new())));
}

#[test]
fn sketch_profiles_and_constraints_enforce_local_connectivity() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let first_sketch = SketchId("synthetic:test:sketch#first".into());
    let second_sketch = SketchId("synthetic:test:sketch#second".into());
    let first = SketchEntityId("synthetic:test:entity#first".into());
    let disconnected = SketchEntityId("synthetic:test:entity#disconnected".into());
    let foreign = SketchEntityId("synthetic:test:entity#foreign".into());
    let plane = |id: SketchId, profiles| Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };
    ir.model.sketches.extend([
        plane(
            first_sketch.clone(),
            vec![vec![
                SketchEntityUse {
                    entity: first.clone(),
                    reversed: false,
                },
                SketchEntityUse {
                    entity: disconnected.clone(),
                    reversed: false,
                },
            ]],
        ),
        plane(second_sketch.clone(), Vec::new()),
    ]);
    let line = |id, sketch, start, end| SketchEntity {
        id,
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    ir.model.sketch_entities.extend([
        line(
            first.clone(),
            first_sketch.clone(),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line(
            disconnected.clone(),
            first_sketch.clone(),
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.0),
        ),
        line(
            foreign.clone(),
            second_sketch,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
    ]);
    let constraint = SketchConstraintId("synthetic:test:constraint#foreign".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch: first_sketch.clone(),
        definition: SketchConstraintDefinition::Parallel {
            first,
            second: foreign,
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
        finding.entity.as_deref() == Some(first_sketch.0.as_str())
            && finding.message.contains("disconnected consecutive")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint.0.as_str())
            && finding.message.contains("different sketch")
    }));

    let disconnected_geometry = &mut ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id == disconnected)
        .expect("disconnected entity remains present")
        .geometry;
    let SketchGeometry::Line { start, .. } = disconnected_geometry else {
        unreachable!("second entity is a line")
    };
    *start = Point2::new(1.0 + ir.tolerances.linear * 0.5, 0.0);
    let report = validate_neutral(&ir, Vec::new());
    assert!(!report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(first_sketch.0.as_str())
            && finding.message.contains("disconnected consecutive")
    }));
}

#[test]
fn sketch_constraint_native_ref_must_resolve() {
    let mut ir = unit_cube();
    let id =
        crate::sketches::SketchConstraintId("synthetic:test:sketch-constraint#native-ref".into());
    ir.model
        .sketch_constraints
        .push(crate::sketches::SketchConstraint {
            id: id.clone(),
            sketch: crate::sketches::SketchId("synthetic:test:sketch#missing".into()),
            definition: crate::sketches::SketchConstraintDefinition::Native {
                native_kind: "test".into(),
                native_state: None,
                native_flags: Some(0x4000),
                native_properties: std::collections::BTreeMap::from([(
                    "mode".to_string(),
                    "7".to_string(),
                )]),
                entities: Vec::new(),
                parameter: None,
                operands: vec![crate::sketches::SketchNativeOperand {
                    native_kind: "test".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 0,
                    native_ref: Some("native:missing-operand#0".into()),
                }],
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
            native_ref: Some("native:missing-relation#0".into()),
        });

    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks
                && finding.entity.as_deref() == Some(id.0.as_str())
                && finding.message.contains("native:missing-relation#0")
        }));
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks
                && finding.entity.as_deref() == Some(id.0.as_str())
                && finding.message.contains("native:missing-operand#0")
        }));
    let serialized = serde_json::to_string(&ir).unwrap();
    let round_trip = CadIr::from_json(&serialized).unwrap();
    assert!(matches!(
        round_trip.model.sketch_constraints[0].definition,
        crate::sketches::SketchConstraintDefinition::Native {
            native_flags: Some(0x4000),
            ..
        }
    ));
    let crate::sketches::SketchConstraintDefinition::Native {
        native_properties, ..
    } = &round_trip.model.sketch_constraints[0].definition
    else {
        unreachable!("test constraint is native")
    };
    assert_eq!(native_properties.get("mode").map(String::as_str), Some("7"));
    let mut legacy = serde_json::from_str::<serde_json::Value>(&serialized).unwrap();
    legacy["model"]["sketch_constraints"][0]["definition"]
        .as_object_mut()
        .unwrap()
        .remove("native_properties");
    let legacy = CadIr::from_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
    let crate::sketches::SketchConstraintDefinition::Native {
        native_properties, ..
    } = &legacy.model.sketch_constraints[0].definition
    else {
        unreachable!("test constraint is native")
    };
    assert!(native_properties.is_empty());
    let crate::sketches::SketchConstraintDefinition::Native { operands, .. } =
        &mut ir.model.sketch_constraints[0].definition
    else {
        unreachable!("test constraint is native")
    };
    operands[0].native_role = Some(7);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::Counts && finding.entity.as_deref() == Some(id.0.as_str())
        }));
}

#[test]
fn sketch_feature_ownership_and_order_are_validated() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, Termination,
    };
    use crate::sketches::{Sketch, SketchId};

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#ordered".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
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
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#consumer".into()),
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
            profile: ProfileRef::Sketch(sketch_id.clone()),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
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
    for (ordinal, suffix) in [(1, "owner"), (2, "duplicate-owner")] {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#{suffix}")),
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
            definition: FeatureDefinition::Sketch {
                sketch: Some(sketch_id.clone()),
            },
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| finding
        .message
        .contains("does not precede its profile consumer")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("has multiple owning features")));
}

#[test]
fn sketch_profile_subselections_are_bounds_checked() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, SketchProfileRegion, Termination,
    };
    use crate::sketches::{Sketch, SketchEntityId, SketchId};

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#selection".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
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
    let feature = |suffix: &str, ordinal, profile| Feature {
        id: FeatureId(format!("synthetic:test:feature#{suffix}")),
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
        definition: FeatureDefinition::Extrude {
            profile,
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
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
    };
    ir.model.features.push(feature(
        "invalid-profile-index",
        1,
        ProfileRef::SketchProfiles {
            sketch: sketch_id.clone(),
            profiles: vec![0, 0],
        },
    ));
    ir.model.features.push(feature(
        "invalid-region",
        2,
        ProfileRef::SketchRegions {
            sketch: sketch_id.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 0,
                holes: vec![0, 0],
            }],
        },
    ));
    let selected_entity = SketchEntityId("synthetic:test:entity#missing".into());
    ir.model.features.push(feature(
        "repeated-profile-entity",
        3,
        ProfileRef::SketchEntities {
            sketch: sketch_id.clone(),
            entities: vec![selected_entity.clone(), selected_entity],
        },
    ));
    ir.model.features.push(feature(
        "empty-native-selection",
        4,
        ProfileRef::SketchSelection {
            sketch: sketch_id,
            selections: Vec::new(),
        },
    ));

    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.message == "sketch profile indices are empty, repeated, or out of range"
    }));
    assert!(
        findings
            .iter()
            .any(|finding| finding.message
                == "native sketch profile selections are empty or repeated")
    );
    assert!(findings.iter().any(|finding| {
        finding.message
            == "sketch regions have empty, repeated, invalid, or out-of-range boundaries"
    }));
    assert!(findings.iter().any(|finding| {
        finding.message
            == "sketch profile entities are empty, repeated, missing, or owned by another sketch"
    }));
}

#[test]
fn spatial_sketch_feature_owns_spatial_geometry() {
    use crate::features::{Feature, FeatureDefinition, FeatureId};
    use crate::sketches::{SpatialSketch, SpatialSketchId};

    let mut ir = unit_cube();
    let sketch_id = SpatialSketchId("synthetic:test:spatial-sketch#owned".into());
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-sketch".into()),
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
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        },
        native_ref: None,
    });

    assert!(validate_neutral(&ir, Vec::new()).findings.is_empty());
    let mut duplicate = ir.model.features.last().expect("spatial owner").clone();
    duplicate.id = FeatureId("synthetic:test:feature#duplicate-spatial-sketch".into());
    duplicate.ordinal = 1;
    ir.model.features.push(duplicate);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("has multiple owning features")));
}
