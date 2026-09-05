// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use cadmpeg_ir::sketches::SketchOffsetPair;

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;
const TEST_DISTANCE_EPSILON: f64 = 1.0e-9;
const TEST_ANGLE_ROUNDING: f64 = 5.0e-7;

fn offset_loci(rows: &[(u32, u32, u32)]) -> Vec<crate::records::DesignDimensionLocus> {
    rows.iter().map(|&(geometry_record_index, role, returned)| crate::records::DesignDimensionLocus {
        geometry_record_index,
        geometry_reference_offset: 0,
        role,
        role_offset: 0,
        returned: crate::records::Located { value: returned, offset: 0 },
    }).collect()
}

#[test]
fn counted_offset_return_run_pairs_sources_and_results() {
    let entity = |id: &str, start, end| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Line { start, end },
        )
    };
    let bottom = entity(
        "generated:line#bottom",
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 0.0),
    );
    let top = entity(
        "generated:line#top",
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
    let inset_top = entity(
        "generated:line#inset-top",
        Point2::new(2.0, 8.0),
        Point2::new(8.0, 8.0),
    );
    let inset_bottom = entity(
        "generated:line#inset-bottom",
        Point2::new(8.0, 2.0),
        Point2::new(2.0, 2.0),
    );

    let entities = HashMap::from([(1, &bottom), (2, &top), (3, &inset_top), (4, &inset_bottom)]);
    let definition = exact_counted_offset(
        &offset_loci(&[(1, 3, 1), (2, 2, 4), (3, 0, 2), (4, 0, 3)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .expect("counted offset graph");
    let crate::design::dimensions::CountedOffset { pairs, distance } = definition;
    assert_eq!(&pairs[0].source, bottom.id());
    assert_eq!(&pairs[0].result, inset_bottom.id());
    assert_eq!(&pairs[1].source, top.id());
    assert_eq!(&pairs[1].result, inset_top.id());
    assert!((distance.0 - 2.0).abs() <= 1.0e-9);
    assert!(pairs.iter().all(|pair| pair.source_reversed));
}

#[test]
fn counted_offset_accepts_primary_to_generated_identity_partition() {
    let entity = |id: &str, y| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Line {
                start: Point2::new(0.0, y),
                end: Point2::new(8.0, y),
            },
        )
    };
    let source = entity("generated:line#source", 0.0);
    let result = entity("generated:line#result", 2.75);
    let entities = HashMap::from([(1, &source), (2, &result)]);
    let secondary_ids = HashMap::from([(1, 0), (2, 42)]);

    assert!(matches!(
        exact_counted_offset(
            &offset_loci(&[(1, 4, 1), (2, 1, 2)]),
            &entities,
            &secondary_ids,
            1.0e-6,
        ),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance: Length(distance),
        }) if pairs.len() == 1
            && &pairs[0].source == source.id()
            && &pairs[0].result == result.id()
            && (distance - 2.75).abs() <= 1.0e-9
    ));

    let ambiguous_ids = HashMap::from([(1, 0), (2, 0)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 4, 1), (2, 1, 2)]),
        &entities,
        &ambiguous_ids,
        1.0e-6,
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_fitted_nurbs_with_exact_endpoint_frames() {
    let entity = |id: &str, degree, knots, control_points| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights: None,
                periodic: false,
            },
        )
    };
    let source = entity(
        "generated:nurbs#source",
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 3.0),
            Point2::new(10.0, 0.0),
        ],
    );
    let result_start = Point2::new(-1.2, 1.6);
    let result_end = Point2::new(10.0 + 2.0 / 5.0_f64.sqrt(), 4.0 / 5.0_f64.sqrt());
    let result = entity(
        "generated:nurbs#result",
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            result_start,
            Point2::new(result_start.u + 2.0, result_start.v + 1.5),
            Point2::new(result_end.u - 3.0, result_end.v + 1.5),
            result_end,
        ],
    );
    let entities = HashMap::from([(1, &source), (2, &result)]);
    assert!(matches!(
        exact_counted_offset(
            &offset_loci(&[(1, 3, 1), (2, 0, 2)]),
            &entities,
            &HashMap::new(),
            1.0e-6,
        ),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance: Length(distance),
        }) if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: false,
        }] && (distance - 2.0).abs() <= 1.0e-9
    ));

    let mut skewed = result;
    let SketchGeometry::Nurbs { control_points, .. } = &mut skewed.geometry else {
        unreachable!("test result is a NURBS")
    };
    control_points.last_mut().expect("result endpoint").u += 0.01;
    let entities = HashMap::from([(1, &source), (2, &skewed)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 3, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_trimmed_concentric_arcs() {
    let arc = |id: &str, radius| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Arc {
                center: Point2::new(3.0, -4.0),
                radius: Length(radius),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::FRAC_PI_2),
            },
        )
    };
    let source = arc("generated:arc#source", 2.0);
    let mut result = arc("generated:arc#result", 5.0);
    result.geometry = SketchGeometry::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length(5.0),
        start_angle: Angle(0.1),
        end_angle: Angle(1.4),
    };
    let entities = HashMap::from([(1, &source), (2, &result)]);

    let definition = exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .expect("concentric arc offset");
    assert!(matches!(
        definition,
        crate::design::dimensions::CountedOffset {
            pairs,
            distance: Length(distance),
        } if pairs.len() == 1
            && &pairs[0].source == source.id()
            && &pairs[0].result == result.id()
            && pairs[0].source_reversed
            && (distance - 3.0).abs() <= 1.0e-9
    ));

    let mut mismatched = result;
    mismatched.geometry = SketchGeometry::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length(5.0),
        start_angle: Angle(std::f64::consts::PI),
        end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
    };
    let entities = HashMap::from([(1, &source), (2, &mismatched)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_concentric_full_circles() {
    let circle = |id: &str, radius| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Circle {
                center: Point2::new(3.0, -4.0),
                radius: Length(radius),
            },
        )
    };
    let source = circle("generated:circle#source", 5.0);
    let result = circle("generated:circle#result", 3.5);
    let entities = HashMap::from([(1, &source), (2, &result)]);

    assert!(matches!(
        exact_counted_offset(
            &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
            &entities,
            &HashMap::new(),
            TEST_LINEAR_TOLERANCE,
        ),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance: Length(distance),
        }) if pairs.as_slice() == [SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: false,
        }] && (distance - 1.5).abs() <= TEST_DISTANCE_EPSILON
    ));

    let reversed_entities = HashMap::from([(1, &result), (2, &source)]);
    assert!(matches!(
        exact_counted_offset(
            &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
            &reversed_entities,
            &HashMap::new(),
            TEST_LINEAR_TOLERANCE,
        ),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance: Length(distance),
        }) if pairs.as_slice() == [SketchOffsetPair {
            source: result.id().clone(),
            result: source.id().clone(),
            source_reversed: true,
        }] && (distance - 1.5).abs() <= TEST_DISTANCE_EPSILON
    ));

    let mut displaced = result.clone();
    displaced.geometry = SketchGeometry::Circle {
        center: Point2::new(3.0, -3.9),
        radius: Length(3.5),
    };
    let entities = HashMap::from([(1, &source), (2, &displaced)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        TEST_LINEAR_TOLERANCE,
    )
    .is_none());
}

#[test]
fn spatial_counted_offset_projects_source_and_result_sets_without_metric_pairs() {
    let stream = "f3d:synthetic";
    let sketch_id = SpatialSketchId("synthetic:spatial-sketch#offset".into());
    let entity = |record_index, geometry| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId(format!("synthetic:spatial-curve#{record_index}")),
            sketch_id.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("{stream}:sketch-curve#{record_index}")))
    };
    let sources = [
        SpatialSketchGeometry::Line {
            start: Point3::new(-20.0, 5.0, 8.0),
            end: Point3::new(-15.0, 5.0, 8.0),
        },
        SpatialSketchGeometry::Arc {
            center: Point3::new(30.0, -12.0, 9.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            reference_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        },
        SpatialSketchGeometry::Circle {
            center: Point3::new(50.0, 40.0, -7.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length(4.0),
        },
        SpatialSketchGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(70.0, -5.0, 3.0), Point3::new(74.0, -2.0, 6.0)],
            weights: None,
            periodic: false,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(index, geometry)| entity(index as u32 + 1, geometry))
    .collect::<Vec<_>>();
    let results = [
        (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)),
        (Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)),
        (Point3::new(10.0, 10.0, 0.0), Point3::new(0.0, 10.0, 0.0)),
        (Point3::new(0.0, 10.0, 0.0), Point3::new(0.0, 0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| {
        entity(
            index as u32 + 11,
            SpatialSketchGeometry::Line { start, end },
        )
    })
    .collect::<Vec<_>>();
    let profile = SpatialSketchProfile {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        boundary: results
            .iter()
            .map(|entity| SpatialSketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect(),
    };
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![profile],
        native_ref: None,
    };
    let record_index = |entity: &SpatialSketchEntity| {
        entity
            .native_ref
            .as_deref()
            .and_then(|id| id.rsplit_once('#'))
            .and_then(|(_, index)| index.parse().ok())
            .expect("synthetic record index")
    };
    let mut operands = sources
        .iter()
        .map(|entity| SketchNativeOperand {
            native_kind: "curve".into(),
            native_field: Some("locus".into()),
            native_role: Some(1),
            object_index: record_index(entity),
            native_ref: entity.native_ref.clone(),
        })
        .chain(results.iter().map(|entity| SketchNativeOperand {
            native_kind: "curve".into(),
            native_field: Some("locus".into()),
            native_role: Some(0),
            object_index: record_index(entity),
            native_ref: entity.native_ref.clone(),
        }))
        .collect::<Vec<_>>();
    operands.push(SketchNativeOperand {
        native_kind: "record".into(),
        native_field: Some("owner".into()),
        native_role: Some(0),
        object_index: 100,
        native_ref: Some(format!("{stream}:design-entity#100")),
    });
    operands.extend(sources.iter().zip(&results).flat_map(|(source, result)| {
        [source, result].map(|entity| SketchNativeOperand {
            native_kind: "curve".into(),
            native_field: Some("return".into()),
            native_role: None,
            object_index: record_index(entity),
            native_ref: entity.native_ref.clone(),
        })
    }));
    let mut by_record = sources
        .iter()
        .chain(&results)
        .map(|entity| ((stream, record_index(entity)), entity))
        .collect::<HashMap<_, _>>();
    let parameter = ParameterId("synthetic:parameter#offset".into());

    let definition = spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &operands,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .expect("counted spatial offset");
    assert!(matches!(
        definition,
        SpatialSketchConstraintDefinition::Offset {
            sources: actual_sources,
            results: actual_results,
            normal,
            distance: Length(3.0),
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: true,
            }),
        } if actual_sources == sources.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>()
            && actual_results == results.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>()
            && normal == Vector3::new(0.0, 0.0, 1.0)
            && actual_parameter == parameter
    ));
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0),
        &operands,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .is_none());
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &operands,
        &parameter,
        3.0,
        -2.0,
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .is_none());
    let outside_source = entity(
        99,
        SpatialSketchGeometry::Line {
            start: Point3::new(-100.0, 0.0, 0.0),
            end: Point3::new(-90.0, 0.0, 0.0),
        },
    );
    by_record.insert((stream, 99), &outside_source);
    let mut non_permutation = operands.clone();
    let first_return = sources.len() + results.len() + 1;
    non_permutation[first_return].object_index = 99;
    non_permutation[first_return].native_ref = outside_source.native_ref.clone();
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &non_permutation,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .is_none());
    let mut wrong_operand_kind = operands.clone();
    wrong_operand_kind[0].native_kind = "point".into();
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &wrong_operand_kind,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .is_none());
    let mut repeated_boundary = sketch.clone();
    repeated_boundary.profiles[0].boundary[1] = repeated_boundary.profiles[0].boundary[0].clone();
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &operands,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&repeated_boundary),
        &by_record,
    )
    .is_none());
    let mut ambiguous_sketch = sketch.clone();
    ambiguous_sketch.profiles.push(sketch.profiles[0].clone());
    assert!(spatial_counted_offset_dimension_definition(
        "Linear Dimension-1",
        Some(0x20),
        &operands,
        &parameter,
        3.0,
        -3.0,
        &sketch_id,
        std::slice::from_ref(&ambiguous_sketch),
        &by_record,
    )
    .is_none());
}

#[test]
fn counted_roles_require_matching_solved_geometry() {
    let line = |id: &str, start, end| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Line { start, end },
        )
    };
    let horizontal = line(
        "generated:line#horizontal",
        Point2::new(-2.0, 3.0),
        Point2::new(5.0, 3.0),
    );
    let vertical = line(
        "generated:line#vertical",
        Point2::new(4.0, -1.0),
        Point2::new(4.0, 8.0),
    );

    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40),
        Some(SketchConstraintDefinition::Horizontal { entity })
            if &entity == horizontal.id()
    ));
    assert!(matches!(
        counted_role_relation(&[&vertical], 0x80),
        Some(SketchConstraintDefinition::Vertical { entity })
            if &entity == vertical.id()
    ));
    assert!(counted_role_relation(&[&horizontal], 0x80).is_none());
    assert!(counted_role_relation(&[&horizontal, &vertical], 0x40).is_none());

    let arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#tangent".into()),
        horizontal.sketch.clone(),
        SketchGeometry::Arc {
            center: Point2::new(-2.0, 2.0),
            radius: Length(1.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(std::f64::consts::PI),
        },
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &horizontal], 0x100),
        Some(SketchConstraintDefinition::Tangent { first, second })
            if &first == arc.id() && &second == horizontal.id()
    ));

    let tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#arc-tangent".into()),
        horizontal.sketch.clone(),
        SketchGeometry::Arc {
            center: Point2::new(-2.0, 5.0),
            radius: Length(2.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(0.0),
        },
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &tangent_arc], 0x100),
        Some(SketchConstraintDefinition::Tangent { first, second })
            if &first == arc.id() && &second == tangent_arc.id()
    ));

    let non_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#arc-not-tangent".into()),
        tangent_arc.sketch.clone(),
        SketchGeometry::Arc {
            center: Point2::new(-1.0, 3.0),
            radius: Length(1.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(2.0 * std::f64::consts::PI),
        },
    );
    assert!(counted_role_relation(&[&arc, &non_tangent_arc], 0x100).is_none());

    let interior_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#arc-interior-tangent".into()),
        tangent_arc.sketch.clone(),
        SketchGeometry::Arc {
            center: Point2::new(-2.0 - 2.0 / 2.0_f64.sqrt(), 2.0 + 2.0 / 2.0_f64.sqrt()),
            radius: Length(1.0),
            start_angle: Angle(-std::f64::consts::FRAC_PI_2),
            end_angle: Angle(0.0),
        },
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &interior_tangent_arc], 0x100),
        Some(SketchConstraintDefinition::Tangent { first, second })
            if &first == arc.id() && &second == interior_tangent_arc.id()
    ));

    let tangent_circle = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:circle#rounded-tangent".into()),
        tangent_arc.sketch.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
        },
    );
    let rounded_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#rounded-tangent".into()),
        tangent_arc.sketch.clone(),
        SketchGeometry::Arc {
            center: Point2::new(2.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(TEST_ANGLE_ROUNDING),
            end_angle: Angle(std::f64::consts::PI),
        },
    );
    assert!(matches!(
        crate::design::dimensions::counted_role_relation_at_tolerance(
            &[&tangent_circle, &rounded_tangent_arc],
            0x100,
            TEST_LINEAR_TOLERANCE,
        ),
        Some(SketchConstraintDefinition::Tangent { first, second })
            if &first == tangent_circle.id() && &second == rounded_tangent_arc.id()
    ));

    let mut equal_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId("generated:arc#equal".into()),
        arc.sketch.clone(),
        arc.geometry.clone(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &equal_arc], 0x800),
        Some(SketchConstraintDefinition::Equal { first, second })
            if &first == arc.id() && &second == equal_arc.id()
    ));
    if let SketchGeometry::Arc { radius, .. } = &mut equal_arc.geometry {
        *radius = Length(2.0);
    }
    assert!(counted_role_relation(&[&arc, &equal_arc], 0x800).is_none());
}

#[test]
fn offset_parameter_factor_preserves_curve_direction() {
    assert_eq!(offset_parameter_factor(2.0, 2.0), Some(1.0));
    assert_eq!(offset_parameter_factor(2.0, -2.0), Some(-1.0));
    assert_eq!(offset_parameter_factor(-2.0, 2.0), None);
    assert_eq!(offset_parameter_factor(2.0, 3.0), None);
    assert_eq!(offset_parameter_factor(f64::NAN, 2.0), None);
}

#[test]
fn paired_dimensions_bind_geometry_with_stream_local_record_indices() {
    let placement = |stream: &str, suffix| DesignSketchPlacement {
        member_run_head: false,
        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: format!("0_{suffix}"),
        entity_suffix: suffix,
        visibility: None,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let owner = |stream: &str| DesignParameterOwner {
        id: format!("f3d:{stream}:design-parameter-owner#0"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "305".into(),
        record_index: 9,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 1.0,
        evaluated_value_offset: 40,
        parameter_record_index: 11,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 12,
    };
    let pair = |stream: &str| DesignDimensionLocusPair {
        id: format!("f3d:{stream}:design-dimension-locus-pair#0"),
        companion_record_index: 12,
        governing_companion_record_index: 12,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 13,
        frame_length: 100,
        opaque_index: 0,
        opaque_index_offset: 35,
        first_geometry_record_index: 20,
        first_geometry_reference_offset: 40,
        first_role: 0,
        first_role_offset: 50,
        second_geometry_record_index: 21,
        second_geometry_reference_offset: 55,
        second_role: 0,
        second_role_offset: 65,
        paired_class_tag: "273".into(),
        paired_byte_offset: 100,
    };
    let point = |stream: &str, record_index| SketchPoint {
        id: format!("f3d:{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: None,
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            u64::from(record_index),
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    };
    let mut points = vec![
        point("A", 20),
        point("A", 21),
        point("B", 20),
        point("B", 21),
    ];

    bind_dimension_loci(
        &[placement("A", 100), placement("B", 200)],
        &[owner("A"), owner("B")],
        &[pair("A"), pair("B")],
        &[],
        &[],
        &[],
        &mut points,
        &mut [],
    )
    .unwrap();
    assert_eq!(
        points
            .iter()
            .map(|point| point.owner_reference)
            .collect::<Vec<_>>(),
        [Some(100), Some(100), Some(200), Some(200)]
    );
}
