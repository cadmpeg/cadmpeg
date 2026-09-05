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

#[test]
fn three_member_symmetry_states_project_unique_reflection_axis() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let first = entity(
        "generated:point#left",
        SketchGeometry::Point {
            position: Point2::new(-2.0, 3.0),
        },
    );
    let axis_entity = entity(
        "generated:line#axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -5.0),
            end: Point2::new(0.0, 5.0),
        },
    );
    let second = entity(
        "generated:point#right",
        SketchGeometry::Point {
            position: Point2::new(2.0, 3.0),
        },
    );

    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        let definition = exact_atomic_constraint(kind, &[&first, &axis_entity, &second]).unwrap();
        assert!(matches!(
            definition,
            SketchConstraintDefinition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
                axis: ref axis_id,
            } if first_id == first.id()
                && second_id == second.id()
                && axis_id == axis_entity.id()
        ));
    }

    let off_axis = entity(
        "generated:line#off-axis",
        SketchGeometry::Line {
            start: Point2::new(1.0, -5.0),
            end: Point2::new(1.0, 5.0),
        },
    );
    assert!(exact_atomic_constraint(
        SketchConstraintKind::Concentric,
        &[&first, &off_axis, &second],
    )
    .is_none());
    let on_axis = entity(
        "generated:point#on-axis",
        SketchGeometry::Point {
            position: Point2::new(0.0, 3.0),
        },
    );
    for kind in [
        SketchConstraintKind::Concentric,
        SketchConstraintKind::Symmetry,
    ] {
        assert!(exact_atomic_constraint(kind, &[&on_axis, &axis_entity, &on_axis]).is_none());
    }
}

#[test]
fn counted_dimension_groups_resolve_full_circle_symmetry() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let first = entity(
        "generated:circle#first",
        SketchGeometry::Circle {
            center: Point2::new(-3.0, 2.0),
            radius: Length(1.5),
        },
    );
    let axis = entity(
        "generated:line#axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -1.0),
            end: Point2::new(0.0, 4.0),
        },
    );
    let second = entity(
        "generated:circle#second",
        SketchGeometry::Circle {
            center: Point2::new(3.0, 2.0),
            radius: Length(1.5),
        },
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&first, &axis, &second]),
        Some(SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(ref first_id),
            second: SketchLocus::Entity(ref second_id),
            axis: ref axis_id,
        }) if first_id == first.id() && second_id == second.id() && axis_id == axis.id()
    ));

    let mut mismatched = second.clone();
    mismatched.geometry = SketchGeometry::Circle {
        center: Point2::new(3.0, 2.0),
        radius: Length(2.0),
    };
    assert!(exact_counted_dimension_relation(&[&first, &axis, &mismatched]).is_none());
}

#[test]
fn counted_dimension_groups_resolve_bounded_arc_symmetry() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let first = entity(
        "generated:arc#first",
        SketchGeometry::Arc {
            center: Point2::new(-3.0, 2.0),
            radius: Length(1.5),
            start_angle: Angle(-std::f64::consts::FRAC_PI_4),
            end_angle: Angle(std::f64::consts::FRAC_PI_3),
        },
    );
    let axis = entity(
        "generated:line#axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -1.0),
            end: Point2::new(0.0, 4.0),
        },
    );
    let second = entity(
        "generated:arc#second",
        SketchGeometry::Arc {
            center: Point2::new(3.0, 2.0),
            radius: Length(1.5),
            start_angle: Angle(2.0 * std::f64::consts::FRAC_PI_3),
            end_angle: Angle(5.0 * std::f64::consts::FRAC_PI_4),
        },
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&first, &axis, &second]),
        Some(SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(ref first_id),
            second: SketchLocus::Entity(ref second_id),
            axis: ref axis_id,
        }) if first_id == first.id() && second_id == second.id() && axis_id == axis.id()
    ));

    let mut mismatched = second.clone();
    mismatched.geometry = SketchGeometry::Arc {
        center: Point2::new(3.0, 2.0),
        radius: Length(1.5),
        start_angle: Angle(2.0 * std::f64::consts::FRAC_PI_3),
        end_angle: Angle(5.0 * std::f64::consts::FRAC_PI_4 + 0.1),
    };
    assert!(exact_counted_dimension_relation(&[&first, &axis, &mismatched]).is_none());
}

#[test]
fn counted_dimension_groups_resolve_centered_entities() {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let circle = entity(
        "generated:circle#first",
        SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(3.0),
        },
    );
    let arc = entity(
        "generated:arc#second",
        SketchGeometry::Arc {
            center: Point2::new(1.0, 2.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(1.0),
        },
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &arc]),
        Some(SketchConstraintDefinition::Concentric { first, second })
            if first == circle.id().clone() && second == arc.id().clone()
    ));

    let coradial = entity(
        "generated:circle#coradial",
        SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(3.0),
        },
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &coradial]),
        Some(SketchConstraintDefinition::Coradial { first, second })
            if first == circle.id().clone() && second == coradial.id().clone()
    ));

    let ellipse = entity(
        "generated:ellipse#same-center",
        SketchGeometry::Ellipse {
            center: Point2::new(1.0, 2.0),
            major_angle: Angle(0.25),
            major_radius: Length(4.0),
            minor_radius: Length(1.5),
            bounds: None,
        },
    );
    assert!(matches!(
        exact_counted_dimension_relation(&[&circle, &ellipse]),
        Some(SketchConstraintDefinition::Concentric { first, second })
            if first == circle.id().clone() && second == ellipse.id().clone()
    ));

    let mut displaced = arc.clone();
    displaced.geometry = SketchGeometry::Arc {
        center: Point2::new(1.0, 2.1),
        radius: Length(2.0),
        start_angle: Angle(0.0),
        end_angle: Angle(1.0),
    };
    assert!(exact_counted_dimension_relation(&[&circle, &displaced]).is_none());

    let mut invalid = arc;
    invalid.geometry = SketchGeometry::Arc {
        center: Point2::new(1.0, 2.0),
        radius: Length(0.0),
        start_angle: Angle(0.0),
        end_angle: Angle(1.0),
    };
    assert!(exact_counted_dimension_relation(&[&circle, &invalid]).is_none());
}

#[test]
fn coincident_relation_projects_one_unique_shared_locus_per_member() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let line = entity(
        "generated:line#0",
        SketchGeometry::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(4.0, 2.0),
        },
    );
    let point = entity(
        "generated:point#0",
        SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    );
    assert_eq!(
        crate::design::dimensions::exact_coincident_loci(&[&line, &point]),
        Some(SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                cadmpeg_ir::sketches::SketchLocus::Start(line.id().clone()),
                cadmpeg_ir::sketches::SketchLocus::Entity(point.id().clone()),
            ],
        })
    );

    let degenerate = entity(
        "generated:line#degenerate",
        SketchGeometry::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(1.0, 2.0),
        },
    );
    assert!(crate::design::dimensions::exact_coincident_loci(&[&degenerate, &point]).is_none());
    assert!(crate::design::dimensions::exact_coincident_loci(&[&line, &line]).is_none());
    assert!(exact_atomic_constraint(SketchConstraintKind::Coincident, &[&line, &line]).is_none());
}

#[test]
fn polygon_constraint_requires_three_distinct_resolved_members() {
    let entity = |id: &str| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        )
    };
    let first = entity("generated:point#0");
    let second = entity("generated:point#1");
    let third = entity("generated:point#2");
    assert_eq!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &third]),
        Some(SketchConstraintDefinition::Polygon {
            entities: vec![first.id().clone(), second.id().clone(), third.id().clone()]
        })
    );
    assert!(exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second]).is_none());
    assert!(
        exact_atomic_constraint(SketchConstraintKind::Polygon, &[&first, &second, &first])
            .is_none()
    );
}

#[test]
fn aggregate_offset_relation_projects_ordered_oriented_pairs() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let source_horizontal = entity(
        "generated:line#source-horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let result_horizontal = entity(
        "generated:line#result-horizontal",
        SketchGeometry::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(8.0, -2.0),
        },
    );
    let source_vertical = entity(
        "generated:line#source-vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        },
    );
    let result_vertical = entity(
        "generated:line#result-vertical",
        SketchGeometry::Line {
            start: Point2::new(2.0, 2.0),
            end: Point2::new(2.0, 8.0),
        },
    );
    let curve = |record_index, secondary_id| SketchRelationOperand::Curve {
        record_index,
        primary_id: u64::from(record_index),
        secondary_id,
    };
    let relation = SketchRelation {
        id: "f3d:native:sketch-relation#0".into(),
        record_index: 10,
        class_tag: "300".into(),
        byte_offset: 0,
        state_offset: 100,
        owner_reference: 1,
        owner_entity_id: "0_1".into(),
        auxiliary_references: vec![0],
        auxiliary_reference_offsets: vec![80],
        rectangular_counted_reference_count: None,
        members: crate::records::zip_relation_members(
            vec![1, 2, 3, 4],
            vec![25, 40, 55, 70],
            vec![3, 5, 1, 1],
            Vec::new(),
        )
        .expect("member zip"),
        owner_reference_offset: 90,
        state: 0x20_0000_0000,
        entity_genesis: None,
        kind: crate::records::SketchRelationKind::Unpatterned,
        return_members: crate::records::zip_return_members(
            vec![1, 3, 2, 4],
            vec![120, 131, 142, 153],
            vec![curve(1, 10), curve(3, 30), curve(2, 20), curve(4, 40)],
        )
        .expect("return zip"),
        raw_bytes: Vec::new(),
    };
    let projected = HashMap::from([
        (("native", 1), &source_horizontal),
        (("native", 2), &source_vertical),
        (("native", 3), &result_horizontal),
        (("native", 4), &result_vertical),
    ]);

    let definition = exact_offset_constraint(&relation, "native", &projected).unwrap();
    let SketchConstraintDefinition::Offset {
        pairs,
        distance,
        parameter,
    } = definition
    else {
        panic!("expected neutral offset constraint")
    };
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].source, source_horizontal.id().clone());
    assert_eq!(pairs[0].result, result_horizontal.id().clone());
    assert_eq!(pairs[1].source, source_vertical.id().clone());
    assert_eq!(pairs[1].result, result_vertical.id().clone());
    assert!((distance.0 - 2.0).abs() <= 1.0e-9);
    assert!(pairs[0].source_reversed);
    assert!(!pairs[1].source_reversed);
    assert_eq!(parameter, None);

    let mut repeated_pair = relation;
    repeated_pair.return_members.extend([
        crate::records::SketchRelationReturnMember {
            record_index: 1,
            offset: 0,
            resolved: Some(curve(1, 10)),
        },
        crate::records::SketchRelationReturnMember {
            record_index: 3,
            offset: 0,
            resolved: Some(curve(3, 30)),
        },
    ]);
    assert!(exact_offset_constraint(&repeated_pair, "native", &projected).is_none());
}

#[test]
fn single_curve_annotation_projects_parameterized_offset() {
    let stream = "f3d:Design/BulkStream.dat";
    let sketch = SketchId("generated:sketch#offset".into());
    let source_curve_id = format!("{stream}:sketch-curve#10");
    let result_curve_id = format!("{stream}:sketch-curve#11");
    let curve = |id: String, record_index, primary_id, secondary_id| SketchCurveIdentity {
        id,
        record_index,
        owner_reference: Some(100),
        class_tag: "262".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id,
        secondary_id,
        geometry: None,
    };
    let source_curve = curve(source_curve_id.clone(), 10, 20, 0);
    let result_curve = curve(result_curve_id.clone(), 11, 21, 7);
    let entity = |id: &str, native_ref: String, start, end| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
        .with_native_ref(Some(native_ref))
    };
    let source = entity(
        "source",
        source_curve_id,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let result = entity(
        "result",
        result_curve_id,
        Point2::new(0.0, -2.0),
        Point2::new(10.0, -2.0),
    );
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#12"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 12,
        family_discriminator: Some(crate::records::Located { value: 6, offset: 0 }),
        source_ordinal: 0,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Dimension,
            Some(13),
        ),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-2".into(),
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "mm".into(), offset: Some(0) }),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let frame = DesignDimensionAnnotationFrame {
        id: format!("{stream}:design-dimension-annotation-frame#14"),
        companion_record_index: Some(15),
        governing_companion_record_index: 15,
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 14,
        frame_length: 100,
        operands: vec![
            DesignDimensionAnnotationOperand {
                geometry_record_index: 0,
                geometry_reference_offset: 0,
                role: 3,
                role_offset: 0,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: 10,
                geometry_reference_offset: 0,
                role: 2,
                role_offset: 0,
            },
        ],
        entity_genesis: 0x80,
        annotation_bytes: Vec::new(),
        annotation_byte_offset: 0,
        governing_owner_record_index: 13,
        governing_owner_reference_offset: 0,
        return_members: vec![10],
        return_member_offsets: vec![0],
        paired_class_tag: "256".into(),
        paired_byte_offset: 0,
        owner_reference: 100,
        owner_reference_offset: 0,
    };
    let parameter_id = ParameterId("generated:parameter#offset".into());
    let projected = HashMap::from([((stream, 10), &source), ((stream, 11), &result)]);

    let definition = crate::design::dimensions::annotation_offset_dimension_definition(
        &frame,
        &parameter,
        &parameter_id,
        stream,
        &[source_curve.clone(), result_curve.clone()],
        &projected,
        1.0e-6,
    )
    .expect("single-curve annotation offset");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::Offset {
            pairs,
            distance: Length(distance),
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: false,
            }),
        } if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: true,
        }] && (distance - 2.0).abs() <= 1.0e-9
            && actual_parameter == parameter_id
    ));

    let explicit_frame = DesignDimensionAnnotationFrame {
        operands: vec![
            DesignDimensionAnnotationOperand {
                geometry_record_index: 0,
                geometry_reference_offset: 0,
                role: 3,
                role_offset: 0,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: 11,
                geometry_reference_offset: 0,
                role: 1,
                role_offset: 0,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: 10,
                geometry_reference_offset: 0,
                role: 2,
                role_offset: 0,
            },
        ],
        return_members: vec![10, 11],
        return_member_offsets: vec![0, 0],
        ..frame.clone()
    };
    let explicit_definition = crate::design::dimensions::annotation_offset_dimension_definition(
        &explicit_frame,
        &parameter,
        &parameter_id,
        stream,
        &[source_curve.clone(), result_curve.clone()],
        &projected,
        1.0e-6,
    )
    .expect("explicit two-curve annotation offset");
    assert!(matches!(
        explicit_definition,
        SketchConstraintDefinition::Offset {
            pairs,
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: false,
            }),
            ..
        } if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: true,
        }] && actual_parameter == parameter_id
    ));

    let duplicate_curve_id = format!("{stream}:sketch-curve#12");
    let duplicate_curve = curve(duplicate_curve_id.clone(), 12, 22, 8);
    let duplicate = entity(
        "duplicate",
        duplicate_curve_id,
        Point2::new(2.0, -2.0),
        Point2::new(8.0, -2.0),
    );
    let projected_with_duplicate = HashMap::from([
        ((stream, 10), &source),
        ((stream, 11), &result),
        ((stream, 12), &duplicate),
    ]);
    assert!(
        crate::design::dimensions::annotation_offset_dimension_definition(
            &frame,
            &parameter,
            &parameter_id,
            stream,
            &[
                curve(format!("{stream}:sketch-curve#10"), 10, 20, 0),
                curve(format!("{stream}:sketch-curve#11"), 11, 21, 7),
                duplicate_curve
            ],
            &projected_with_duplicate,
            1.0e-6,
        )
        .is_none()
    );
}

#[test]
fn mixed_circle_arc_offset_uses_concentric_radius_difference() {
    let circle = SketchGeometry::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length(20.0),
    };
    let arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(22.0),
        start_angle: Angle(-0.2),
        end_angle: Angle(0.1),
    };
    let distance = crate::design::dimensions::sketch_curve_offset(&circle, &arc)
        .expect("concentric circle-to-arc offset");
    assert!((distance + 2.0).abs() <= 1.0e-9);

    let clockwise_arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(22.0),
        start_angle: Angle(0.1),
        end_angle: Angle(-0.2),
    };
    let distance = crate::design::dimensions::sketch_curve_offset(&clockwise_arc, &circle)
        .expect("concentric arc-to-circle offset");
    assert!((distance + 2.0).abs() <= 1.0e-9);

    let displaced_arc = SketchGeometry::Arc {
        center: Point2::new(1.0e-6, 0.0),
        radius: Length(22.0),
        start_angle: Angle(0.1),
        end_angle: Angle(-0.2),
    };
    assert!(crate::design::dimensions::sketch_curve_offset(&circle, &displaced_arc).is_none());
}

#[test]
fn angular_point_operand_selects_unique_incident_line_by_value() {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let point = entity(
        "generated:point#vertex",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let explicit = entity(
        "generated:line#explicit",
        SketchGeometry::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(2.0, 2.0),
        },
    );
    let diagonal = entity(
        "generated:line#diagonal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 2.0),
        },
    );
    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let projected = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
    ]);

    let lines = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(lines, (diagonal.id().clone(), explicit.id().clone()));
    let supplementary = indirect_angular_lines(
        "native",
        &[&point, &explicit],
        3.0 * std::f64::consts::FRAC_PI_4,
        &projected,
    )
    .unwrap();
    assert_eq!(supplementary, lines);
    let duplicate_diagonal = entity(
        "generated:line#duplicate-diagonal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 4.0),
        },
    );
    let projected_with_duplicate = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
        (("native", 5), &duplicate_diagonal),
    ]);
    assert!(indirect_angular_lines(
        "native",
        &[&point, &explicit],
        std::f64::consts::FRAC_PI_4,
        &projected_with_duplicate,
    )
    .is_none());
}

#[test]
fn counted_angular_group_projects_unique_point_selected_line() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: "0_100".into(),
        entity_suffix: 100,
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
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#20"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 20,
        family_discriminator: Some(crate::records::Located { value: 0, offset: 0 }),
        source_ordinal: 4,
        owner: crate::records::DesignParameterOwnerKind::from_kind(
            DesignParameterKind::Dimension,
            Some(21),
        ),
        expression: "1.0471975512 rad".into(),
        expression_offset: 0,
        source_kind: "Angular Dimension-4".into(),
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "rad".into(), offset: Some(0) }),
        name: "d4".into(),
        name_offset: 0,
        evaluated_value: std::f64::consts::FRAC_PI_3,
        evaluated_value_offset: 0,
    };
    let owner = DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "292".into(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: std::f64::consts::FRAC_PI_3,
        evaluated_value_offset: 0,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    };
    let companion = DesignParameterCompanion {
        id: format!("{stream}:design-parameter-companion#22"),
        byte_offset: 0,
        class_tag: "408".into(),
        record_index: 22,
        owner_record_index: 21,
        timestamp_micros: 1,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    let group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#30"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 30,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 1,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        constraint_kinds: vec![SketchConstraintKind::Coincident],
        unknown_constraint_bits: 0,
        return_members: vec![40, 41],
        return_member_offsets: vec![0, 0],
        next_class_tag: "273".into(),
        next_record_index: 31,
        next_byte_offset: 100,
    };
    let point = SketchPoint {
        id: format!("{stream}:sketch-point#40"),
        record_index: 40,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            40,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    };
    let curve = |record_index: u32, start: Point2, end: Point2| {
        let delta_u = end.u - start.u;
        let delta_v = end.v - start.v;
        let length = delta_u.hypot(delta_v);
        SketchCurveIdentity {
            id: format!("{stream}:sketch-curve#{record_index}"),
            record_index,
            owner_reference: Some(100),
            class_tag: "301".into(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: u64::from(record_index),
            secondary_id: 0,
            geometry: Some(SketchCurveGeometry::Line {
                start: Point3::new(start.u, start.v, 0.0),
                end: Point3::new(end.u, end.v, 0.0),
                direction: Vector3::new(delta_u / length, delta_v / length, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            }),
        }
    };
    let explicit = curve(41, Point2::new(0.0, 0.0), Point2::new(2.0, 0.0));
    let candidate = curve(42, Point2::new(0.0, 0.0), Point2::new(1.0, 3.0f64.sqrt()));
    let sketch = neutral_sketch_id(&placement);
    let point_entity = SketchEntity::new(
        SketchEntityId("generated:point#40".into()),
        sketch.clone(),
        SketchGeometry::Point {
            position: point.coordinates,
        },
    )
    .with_native_ref(Some(point.id.clone()));
    let explicit_entity = SketchEntity::new(
        SketchEntityId("generated:line#41".into()),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    )
    .with_native_ref(Some(explicit.id.clone()));
    let candidate_entity = SketchEntity::new(
        SketchEntityId("generated:line#42".into()),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 3.0f64.sqrt()),
        },
    )
    .with_native_ref(Some(candidate.id.clone()));
    let entities = vec![point_entity, explicit_entity, candidate_entity];
    let curves = vec![explicit, candidate];
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: std::slice::from_ref(&point),
            curves: &curves,
            entities: &entities,
        },
        &[],
    );

    assert_eq!(constraints.len(), 1);
    assert!(matches!(
        &constraints[0].definition,
        SketchConstraintDefinition::Angle {
            first,
            second,
            parameter: actual_parameter,
        } if first == entities[2].id()
            && second == entities[1].id()
            && actual_parameter == &neutral_parameter_id_parts(stream, 20)
    ));
}

#[test]
fn parallel_group_binds_one_common_axis_angle() {
    let line = |id: &str, end: Point2| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end,
            },
        )
    };
    let first = line("generated:line#first", Point2::new(1.0, 1.0));
    let second = line("generated:line#second", Point2::new(-2.0, -2.0));
    let mismatch = line("generated:line#mismatch", Point2::new(1.0, 0.0));
    let crossed = line("generated:line#crossed", Point2::new(1.0, -1.0));
    let parameter = parse_design_parameter(&parameter_record(
        Some(44),
        "45 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_4,
    ))
    .expect("generated angular dimension is canonical");
    let parameter_id = ParameterId("generated:parameter#axis-angle".into());

    assert!(matches!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &second],
            &parameter,
            &parameter_id,
        ),
        Some(SketchConstraintDefinition::AngleToAxis {
            entity,
            axis: SketchAxis::Horizontal,
            parameter,
        }) if entity == first.id().clone() && parameter == parameter_id
    ));
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &mismatch],
            &parameter,
            &parameter_id,
        )
        .is_none()
    );
    assert!(
        crate::design::dimensions::parallel_group_axis_angle_definition(
            &[&first, &crossed],
            &parameter,
            &parameter_id,
        )
        .is_none()
    );
}
