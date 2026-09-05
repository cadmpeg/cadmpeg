// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::cloned_ref_to_slice_refs)]
use super::*;
use crate::design::constraints::project_sketch_constraints;
use crate::design::dimensions::{exact_atomic_constraint, point_lies_on_sketch_geometry};
use crate::design::geometry::{point_on_sketch_entity, sketch_entity_endpoints};
use crate::records::{
    DesignSketchPlacement, DesignSketchVisibility, SketchCurveIdentity, SketchPoint,
    SketchRelation, SketchRelationKind, SketchRelationMember, SketchRelationOperand,
    SketchRelationReturnMember, SketchText,
};
use cadmpeg_ir::features::Length;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchGeometry,
};
use cadmpeg_ir::sketches::{
    SketchTextHorizontalAlignment as Horizontal, SketchTextVerticalAlignment as Vertical,
};

const EPS_POINT_PROJECTION: f64 = 1.0e-6;

#[test]
fn sketch_text_alignment_ordinals_project_to_named_positions() {
    assert_eq!(
        sketch_text_horizontal_alignment(Some(1)),
        Some(Horizontal::Left)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(2)),
        Some(Horizontal::Right)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(3)),
        Some(Horizontal::Center)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(99)),
        Some(Horizontal::Native(99))
    );
    assert_eq!(sketch_text_vertical_alignment(Some(1)), Some(Vertical::Top));
    assert_eq!(
        sketch_text_vertical_alignment(Some(2)),
        Some(Vertical::Bottom)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(3)),
        Some(Vertical::Middle)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(99)),
        Some(Vertical::Native(99))
    );
    assert_eq!(sketch_text_horizontal_alignment(None), None);
    assert_eq!(sketch_text_vertical_alignment(None), None);
}

#[test]
fn sketch_container_visibility_projects_to_the_neutral_sketch() {
    let placement = DesignSketchPlacement {
        id: "f3d:design:design-sketch-placement#1".into(),
        scope_record_index: None,
        entity_id: "Sketch_201".into(),
        entity_suffix: 201,
        visibility: Some(DesignSketchVisibility {
            stream_ordinal: 1,
            stream_ordinal_offset: 30,
            visible_offset: 35,
            visible: false,
        }),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        frame_length: 34,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "257".into(),
        paired_byte_offset: 34,
        member_run_head: true,
    };

    let (sketches, entities) = project_sketch_design(&[placement], &[], &[], &[], &[], 1.0e-6);
    assert!(entities.is_empty());
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].visible, Some(false));
}

#[test]
fn text_frame_curves_are_construction_geometry_not_profiles() {
    let placement = DesignSketchPlacement {
        id: "f3d:BulkStream.dat:placement#0".into(),
        scope_record_index: None,
        entity_id: "Sketch_42".into(),
        entity_suffix: 42,
        visibility: None,
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        frame_length: 0,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "257".into(),
        paired_byte_offset: 0,
        member_run_head: false,
    };
    let curve =
        |record_index, primary_id, start: (f64, f64), end: (f64, f64)| SketchCurveIdentity {
            id: format!("f3d:BulkStream.dat:curve#{record_index}"),
            record_index,
            owner_reference: Some(42),
            class_tag: "375".into(),
            byte_offset: record_index as u64,
            geometry_offset: 0,
            entity_genesis: Some(0),
            primary_id,
            secondary_id: 0,
            geometry: Some(SketchCurveGeometry::Line {
                start: Point3::new(start.0, start.1, 0.0),
                end: Point3::new(end.0, end.1, 0.0),
                direction: Vector3::new(end.0 - start.0, end.1 - start.1, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            }),
        };
    let curves = vec![
        curve(10, 10, (0.0, 0.0), (10.0, 0.0)),
        curve(11, 11, (10.0, 0.0), (10.0, 10.0)),
        curve(12, 12, (10.0, 10.0), (0.0, 10.0)),
        curve(13, 13, (0.0, 10.0), (0.0, 0.0)),
    ];
    let point = SketchPoint {
        id: "f3d:BulkStream.dat:point#14".into(),
        record_index: 14,
        owner_reference: Some(42),
        class_tag: "413".into(),
        byte_offset: 14,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            14,
            crate::records::SketchPointClosure::Selector4State0,
        ),
        paired_reference: 15,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    };
    let text = SketchText {
        id: "f3d:BulkStream.dat:text#20".into(),
        record_index: 20,
        owner_reference: 42,
        class_tag: "376".into(),
        class_version: 0,
        byte_offset: 20,
        entity_genesis: Some(0),
        persistent_id: Some(20),
        base_id: None,
        text: "A".into(),
        font_family: "Arial".into(),
        font_weight: 400,
        height: 10.0,
        color: cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        layout: crate::records::SketchTextLayout::TextexTag {
            width_factor: 1.0,
            horizontal_alignment: Some(1),
            vertical_alignment: Some(1),
            first_reference: None,
            second_reference: None,
            anchor: Some(Point2::new(0.0, 0.0)),
            rotation: Some(0.0),
        },
        raw_bytes: Vec::new(),
    };
    let relation = SketchRelation {
        id: "f3d:BulkStream.dat:relation#30".into(),
        record_index: 30,
        class_tag: "377".into(),
        byte_offset: 30,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: "Sketch_42".into(),
        auxiliary_references: crate::records::ReferenceRun::Unlocated(vec![20]),
        rectangular_counted_reference_count: None,
        members: vec![
            SketchRelationMember::from_index(20),
            SketchRelationMember::from_index(10),
            SketchRelationMember::from_index(11),
            SketchRelationMember::from_index(12),
            SketchRelationMember::from_index(13),
        ],
        owner_reference_offset: 0,
        state: 0x100_0000_0000,
        entity_genesis: Some(0),
        kind: SketchRelationKind::from_pattern(Some(
            crate::records::SketchPatternDefinition::TextFrame { text_reference: 20 },
        )),
        return_members: vec![
            SketchRelationReturnMember::from_index(10),
            SketchRelationReturnMember::from_index(11),
            SketchRelationReturnMember::from_index(12),
            SketchRelationReturnMember::from_index(13),
        ],
        raw_bytes: Vec::new(),
    };

    let (sketches, entities) = project_sketch_design(
        &[placement],
        &[point],
        &curves,
        &[relation],
        &[text],
        EPS_POINT_PROJECTION,
    );
    assert_eq!(sketches.len(), 1);
    assert!(sketches[0].profiles.is_empty());
    assert_eq!(entities.len(), 6);
    assert!(entities
        .iter()
        .filter(|entity| entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":curve#")))
        .all(|entity| entity.construction));
    assert!(entities.iter().any(
        |entity| matches!(entity.geometry, SketchGeometry::Text { .. }) && !entity.construction
    ));
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("f3d:BulkStream.dat:point#14") && !entity.construction
    }));
}

#[test]
fn point_closure_does_not_mark_construction_geometry() {
    let placement = DesignSketchPlacement {
        id: "f3d:BulkStream.dat:placement#0".into(),
        scope_record_index: None,
        entity_id: "Sketch_42".into(),
        entity_suffix: 42,
        visibility: None,
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        frame_length: 0,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "257".into(),
        paired_byte_offset: 0,
        member_run_head: false,
    };
    let point = SketchPoint {
        id: "f3d:BulkStream.dat:point#10".into(),
        record_index: 10,
        owner_reference: Some(42),
        class_tag: "413".into(),
        byte_offset: 10,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            10,
            crate::records::SketchPointClosure::Selector4State0,
        ),
        paired_reference: 11,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: None,
    };
    let standalone_point = SketchPoint {
        id: "f3d:BulkStream.dat:point#11".into(),
        record_index: 11,
        owner_reference: Some(42),
        class_tag: "413".into(),
        byte_offset: 11,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            11,
            crate::records::SketchPointClosure::Selector2State1,
        ),
        paired_reference: 12,
        coordinates: Point2::new(2.0, 0.0),
        depth: 0.0,
        companion: None,
    };
    let curve = SketchCurveIdentity {
        id: "f3d:BulkStream.dat:curve#20".into(),
        record_index: 20,
        owner_reference: Some(42),
        class_tag: "375".into(),
        byte_offset: 20,
        geometry_offset: 0,
        entity_genesis: Some(0),
        primary_id: 20,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(10.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }),
    };

    let (sketches, entities) = project_sketch_design(
        &[placement],
        &[point, standalone_point],
        &[curve],
        &[],
        &[],
        EPS_POINT_PROJECTION,
    );
    assert_eq!(sketches.len(), 1);
    assert!(sketches[0].profiles.is_empty());
    assert_eq!(entities.len(), 3);
    assert!(entities.iter().all(|entity| !entity.construction));
}

#[test]
fn placed_sketch_projects_signed_normal_and_nonclamped_curves() {
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:native:placement#0".into(),
        scope_record_index: Some(177),
        entity_id: "0_172".into(),
        entity_suffix: 172,
        visibility: None,
        byte_offset: 100,
        class_tag: "356".into(),
        record_index: 185,
        frame_length: 329,
        transform: [
            [0.0, 0.0, 1.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(155),
        paired_class_tag: "259".into(),
        paired_byte_offset: 429,
    };
    let point = SketchPoint {
        id: "f3d:native:point#175".into(),
        record_index: 175,
        owner_reference: Some(172),
        class_tag: "300".into(),
        byte_offset: 400,
        coordinate_offset: 89,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            10,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(2.5, 4.0),
        depth: 0.0,
        companion: None,
    };
    let line = SketchCurveIdentity {
        id: "f3d:native:curve#217".into(),
        record_index: 217,
        owner_reference: Some(172),
        class_tag: "301".into(),
        byte_offset: 500,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 20,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(1.0, 2.0, 0.0),
            end: Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.6, 0.8, 0.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
        }),
    };
    let clockwise_arc = SketchCurveIdentity {
        id: "f3d:native:curve#220".into(),
        record_index: 220,
        owner_reference: Some(172),
        class_tag: "305".into(),
        byte_offset: 800,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 22,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Arc {
            center: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        }),
    };
    let nonclamped_nurbs = SketchCurveIdentity {
        id: "f3d:native:curve#218".into(),
        record_index: 218,
        owner_reference: Some(172),
        class_tag: "303".into(),
        byte_offset: 700,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 21,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Nurbs {
            carrier_reference: None,
            subtype_class_tag: "304".into(),
            subtype_record_index: 219,
            degree: 2,
            fit_tolerance: 1.0e-6,
            scalar_width: 8,
            knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            poles: crate::records::SketchNurbsPoles::Polynomial(vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(4.0, 2.0, 0.0),
            ]),
        }),
    };

    let placements = vec![placement];
    let points = vec![point];
    let curves = vec![line, nonclamped_nurbs, clockwise_arc];
    let (sketches, entities) =
        project_sketch_design(&placements, &points, &curves, &[], &[], 1.0e-6);
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0].resolved_placement(),
        Some((
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert_eq!(entities.len(), 4);
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Point { position } if position == Point2::new(2.5, 4.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Line { start, end }
            if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 6.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SketchGeometry::Arc { start_angle, end_angle, .. }
            if start_angle.0 == 0.0
                && end_angle.0 == -std::f64::consts::FRAC_PI_2
    )));
    let nurbs = entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some("f3d:native:curve#218"))
        .expect("non-clamped NURBS projects");
    let endpoints = sketch_entity_endpoints(nurbs).expect("non-clamped NURBS endpoints");
    assert_eq!(endpoints, [Point2::new(1.0, 0.0), Point2::new(3.0, 2.0)]);
    assert!(point_on_sketch_entity(Point2::new(2.0, 1.0), nurbs, 1.0e-9));
    assert!(point_lies_on_sketch_geometry(
        Point2::new(2.0, 1.0),
        &nurbs.geometry
    ));

    let relation = |record_index, member, _operand| SketchRelation {
        id: format!("f3d:native:relation#{record_index}"),
        record_index,
        class_tag: "302".into(),
        byte_offset: 600,
        state_offset: 70,
        owner_reference: 172,
        owner_entity_id: "0_172".into(),
        auxiliary_references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        rectangular_counted_reference_count: None,
        members: vec![member]
            .into_iter()
            .map(SketchRelationMember::from_index)
            .collect(),
        owner_reference_offset: 55,
        state: 0x40,
        entity_genesis: None,
        kind: SketchRelationKind::Unpatterned,
        return_members: vec![member]
            .into_iter()
            .map(SketchRelationReturnMember::from_index)
            .collect(),
        raw_bytes: Vec::new(),
    };
    let mut curve_point_coincidence = relation(
        702,
        217,
        SketchRelationOperand::Curve {
            record_index: 217,
            primary_id: 20,
            secondary_id: 0,
        },
    );
    curve_point_coincidence.members.push(SketchRelationMember {
        record_index: 175,
        offset: 40,
        relation_ordinal: 0,
        resolved: Some(SketchRelationOperand::Point {
            record_index: 175,
            persistent_id: Some(10),
        }),
    });
    curve_point_coincidence
        .return_members
        .push(SketchRelationReturnMember::from_index(175));
    curve_point_coincidence.state = 1;
    let mut midpoint = curve_point_coincidence.clone();
    midpoint.record_index = 703;
    midpoint.id = "f3d:native:relation#703".into();
    midpoint.state = 0x10;
    let mut curvature = curve_point_coincidence.clone();
    curvature.record_index = 704;
    curvature.id = "f3d:native:relation#704".into();
    curvature.state = 0x200;
    let mut spline_group = relation(
        705,
        218,
        SketchRelationOperand::Curve {
            record_index: 218,
            primary_id: 21,
            secondary_id: 0,
        },
    );
    // Reverse the first run so only the specified semantic run can satisfy the
    // assertion below.
    spline_group.members = [(218, 25), (217, 40)].into_iter().map(|(record_index, offset)| crate::records::SketchRelationMember { record_index, offset, relation_ordinal: 0, resolved: None }).collect();
    spline_group.return_members = [(217, 80), (218, 95)].into_iter().map(|(record_index, offset)| crate::records::SketchRelationReturnMember { record_index, offset, resolved: None }).collect();
    spline_group.state = 0x8000_0000;
    let mut horizontal_point = relation(
        701,
        175,
        SketchRelationOperand::Point {
            record_index: 175,
            persistent_id: Some(10),
        },
    );
    horizontal_point.auxiliary_references = crate::records::ReferenceRun::Unlocated(vec![999]);
    horizontal_point.return_members = vec![
        SketchRelationReturnMember::from_index(175),
        SketchRelationReturnMember::from_index(175),
    ];
    horizontal_point.state = 0x8000_0040;
    let constraints = project_sketch_constraints(
        &placements,
        &[],
        &points,
        &curves,
        &[],
        &[
            relation(
                700,
                217,
                SketchRelationOperand::Curve {
                    record_index: 217,
                    primary_id: 20,
                    secondary_id: 0,
                },
            ),
            horizontal_point,
            curve_point_coincidence,
            midpoint,
            curvature,
            spline_group,
        ],
        &entities,
    );
    assert!(matches!(
        constraints[0].definition,
        SketchConstraintDefinition::Horizontal { .. }
    ));
    assert!(matches!(
        constraints[1].definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            native_state: Some(0x8000_0040),
            native_flags: None,
            ref entities,
            ref operands,
            ..
        } if native_kind == "horizontal+unknown_bits"
            && entities.len() == 3
            && entities.iter().all(|entity| entity == &entities[0])
            && operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_kind.as_str(), operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("member"), "point", 175),
                    (Some("auxiliary"), "record", 999),
                    (Some("return"), "point", 175),
                    (Some("return"), "point", 175),
                ]
    ));
    assert!(matches!(
        constraints[2].definition,
        SketchConstraintDefinition::Coincident { ref entities } if entities.len() == 2
    ));
    assert!(matches!(
        constraints[3].definition,
        SketchConstraintDefinition::Midpoint { .. }
    ));
    assert!(matches!(
        constraints[4].definition,
        SketchConstraintDefinition::Native {
            ref native_kind,
            ref entities,
            ..
        } if native_kind == "curvature" && entities.len() == 4
    ));
    assert!(matches!(
        constraints[5].definition,
        SketchConstraintDefinition::SplineGroup { ref entities }
            if entities == &[
                neutral_sketch_curve_id(&sketches[0].id, 20, 0),
                neutral_sketch_curve_id(&sketches[0].id, 21, 0),
            ]
    ));
    let line = entities
        .iter()
        .find(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .unwrap();
    let point = entities
        .iter()
        .find(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
        .unwrap();
    let other_point = SketchEntity::new(
        SketchEntityId("generated:point#other".into()),
        point.sketch.clone(),
        point.geometry.clone(),
    )
    .with_construction(point.construction)
    .with_native_ref(point.native_ref.clone())
    .with_geometry_ref(point.geometry_ref.clone())
    .with_endpoint_refs(point.endpoint_refs.clone());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Horizontal, &[point, &other_point]),
        Some(SketchConstraintDefinition::SameCoordinate {
            axis: SketchCoordinateAxis::V,
            ..
        })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Vertical, &[point, &other_point]),
        Some(SketchConstraintDefinition::SameCoordinate {
            axis: SketchCoordinateAxis::U,
            ..
        })
    ));
    assert!(exact_atomic_constraint(SketchConstraintKind::Horizontal, &[point, point]).is_none());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Midpoint, &[line, point]),
        Some(SketchConstraintDefinition::Midpoint { .. })
    ));
    for kind in [
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(exact_atomic_constraint(kind, &[line, point]).is_none());
    }
    let other_line = SketchEntity::new(
        SketchEntityId("generated:line#other".into()),
        line.sketch.clone(),
        line.geometry.clone(),
    )
    .with_construction(line.construction)
    .with_native_ref(line.native_ref.clone())
    .with_geometry_ref(line.geometry_ref.clone())
    .with_endpoint_refs(line.endpoint_refs.clone());
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Tangent, &[line, &other_line]),
        Some(SketchConstraintDefinition::Tangent { .. })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Curvature, &[line, &other_line]),
        Some(SketchConstraintDefinition::Curvature { .. })
    ));
    assert!(matches!(
        exact_atomic_constraint(SketchConstraintKind::Equal, &[line, &other_line]),
        Some(SketchConstraintDefinition::Equal { .. })
    ));
    for kind in [
        SketchConstraintKind::Colinear,
        SketchConstraintKind::EqualLength,
        SketchConstraintKind::Parallel,
        SketchConstraintKind::Perpendicular,
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(exact_atomic_constraint(kind, &[line, line]).is_none());
    }
}

#[test]
fn nonplanar_sketch_curves_project_in_model_space() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;

    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:Design/BulkStream.dat:placement#100".into(),
        scope_record_index: None,
        entity_id: "Sketch_42".into(),
        entity_suffix: 42,
        visibility: None,
        byte_offset: 100,
        class_tag: "300".into(),
        record_index: 100,
        frame_length: 329,
        transform: [
            [0.0, 0.0, 1.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(155),
        paired_class_tag: "259".into(),
        paired_byte_offset: 429,
    };
    let curve = |record_index, primary_id, geometry| SketchCurveIdentity {
        id: format!("f3d:Design/BulkStream.dat:curve#{record_index}"),
        record_index,
        owner_reference: Some(42),
        class_tag: "301".into(),
        byte_offset: u64::from(record_index),
        geometry_offset: 100,
        entity_genesis: None,
        primary_id,
        secondary_id: 0,
        geometry: Some(geometry),
    };
    let mut curves = vec![
        curve(
            101,
            1,
            SketchCurveGeometry::Line {
                start: Point3::new(1.0, 2.0, 3.0),
                end: Point3::new(4.0, 5.0, 6.0),
                direction: Vector3::new(1.0, 1.0, 1.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
            },
        ),
        curve(
            102,
            2,
            SketchCurveGeometry::Arc {
                center: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                reference_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::TAU,
            },
        ),
        curve(
            108,
            7,
            SketchCurveGeometry::Line {
                start: Point3::new(1.0, 2.0, 0.0),
                end: Point3::new(4.0, 2.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
        ),
    ];
    curves.push(SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:curve#103".into(),
        record_index: 103,
        owner_reference: Some(42),
        class_tag: "301".into(),
        byte_offset: 103,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 3,
        secondary_id: 0,
        geometry: None,
    });
    curves.push(curve(
        104,
        4,
        SketchCurveGeometry::Nurbs {
            carrier_reference: None,
            subtype_class_tag: "302".into(),
            subtype_record_index: 104,
            degree: 1,
            fit_tolerance: 1.0e-8,
            scalar_width: 4,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            poles: crate::records::SketchNurbsPoles::Rational(vec![
                crate::records::SketchNurbsPole { point: Point3::new(2.0, 3.0, 4.0), weight: 1.0 },
                crate::records::SketchNurbsPole { point: Point3::new(5.0, 6.0, 7.0), weight: 1.0 },
            ]),
        },
    ));
    let relation = SketchRelation {
        id: "f3d:Design/BulkStream.dat:relation#105".into(),
        record_index: 105,
        class_tag: "303".into(),
        byte_offset: 105,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: "Sketch_42".into(),
        auxiliary_references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        rectangular_counted_reference_count: None,
        // Member run order disagrees with semantic order below.
        members: vec![
            SketchRelationMember::from_index(104),
            SketchRelationMember::from_index(103),
        ],
        owner_reference_offset: 0,
        state: 0x8000_0000,
        entity_genesis: None,
        kind: SketchRelationKind::Unpatterned,
        return_members: vec![
            SketchRelationReturnMember::from_index(103),
            SketchRelationReturnMember::from_index(104),
        ],
        raw_bytes: Vec::new(),
    };
    let point = SketchPoint {
        id: "f3d:Design/BulkStream.dat:point#106".into(),
        record_index: 106,
        owner_reference: Some(42),
        class_tag: "305".into(),
        byte_offset: 106,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            5,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(2.5, 3.5),
        depth: 4.5,
        companion: None,
    };
    let mut midpoint_relation = relation.clone();
    midpoint_relation.id = "f3d:Design/BulkStream.dat:relation#106".into();
    midpoint_relation.record_index = 106;
    midpoint_relation.state = 0x1000;
    midpoint_relation.members = vec![106, 101]
        .into_iter()
        .map(SketchRelationMember::from_index)
        .collect();
    midpoint_relation.return_members = vec![101, 106]
        .into_iter()
        .map(SketchRelationReturnMember::from_index)
        .collect();
    let mut coincident_point = point.clone();
    coincident_point.id = "f3d:Design/BulkStream.dat:point#107".into();
    coincident_point.record_index = 107;
    coincident_point.byte_offset = 107;
    coincident_point.set_persistent_id(Some(6));
    let mut coincident_relation = relation.clone();
    coincident_relation.id = "f3d:Design/BulkStream.dat:relation#107".into();
    coincident_relation.record_index = 107;
    coincident_relation.state = 1;
    coincident_relation.members = vec![106, 107]
        .into_iter()
        .map(SketchRelationMember::from_index)
        .collect();
    coincident_relation.return_members = vec![106, 107]
        .into_iter()
        .map(SketchRelationReturnMember::from_index)
        .collect();
    let mut horizontal_relation = relation.clone();
    horizontal_relation.id = "f3d:Design/BulkStream.dat:relation#108".into();
    horizontal_relation.record_index = 108;
    horizontal_relation.state = 0x40;
    horizontal_relation.members = vec![SketchRelationMember::from_index(108)];
    horizontal_relation.return_members = vec![SketchRelationReturnMember::from_index(108)];
    let surface = SketchSurface {
        id: "f3d:Design/BulkStream.dat:surface#109".into(),
        record_index: 109,
        owner_reference: Some(42),
        class_tag: "306".into(),
        byte_offset: 109,
        entity_genesis: None,
        persistent_id: 8,
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            vec![Point3::new(1.0, 2.0, 0.0), Point3::new(1.0, 5.0, 0.0)],
            vec![Point3::new(4.0, 2.0, 0.0), Point3::new(4.0, 5.0, 0.0)],
        ],
    };
    let mut point_on_surface_relation = relation.clone();
    point_on_surface_relation.id = "f3d:Design/BulkStream.dat:relation#109".into();
    point_on_surface_relation.record_index = 109;
    point_on_surface_relation.state = 1;
    point_on_surface_relation.members = vec![106, 109]
        .into_iter()
        .map(SketchRelationMember::from_index)
        .collect();
    point_on_surface_relation.return_members = vec![106, 109]
        .into_iter()
        .map(SketchRelationReturnMember::from_index)
        .collect();

    let points = [point, coincident_point];
    let relations = [
        relation,
        midpoint_relation,
        coincident_relation,
        horizontal_relation,
        point_on_surface_relation,
    ];
    let (planar_sketches, planar_entities) =
        project_sketch_design(&[placement.clone()], &points, &curves, &[], &[], 1.0e-6);
    assert!(planar_sketches.is_empty());
    assert!(planar_entities.is_empty());
    let surfaces = [surface];
    let (sketches, entities) = project_spatial_sketch_design(
        &[placement.clone()],
        &points,
        &curves,
        &surfaces,
        &relations,
        1.0e-6,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 8);
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(13.0, 21.0, 32.0)
                && end == Point3::new(16.0, 24.0, 35.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(14.0, 22.0, 33.0)
                && end == Point3::new(17.0, 25.0, 36.0)
    )));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == Point3::new(10.0, 21.0, 32.0)
                && end == Point3::new(10.0, 24.0, 32.0)
    )));
    let constraints = project_spatial_sketch_constraints(
        &[placement],
        &relations,
        &points,
        &curves,
        &surfaces,
        &entities,
    );
    assert!(matches!(
        constraints.first(),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::SplineGroup { entities },
            ..
        }) if entities == &[
            crate::ids::neutral_spatial_sketch_curve_id(&sketches[0].id, 3, 0),
            crate::ids::neutral_spatial_sketch_curve_id(&sketches[0].id, 4, 0),
        ]
    ));
    assert!(matches!(
        constraints.get(1),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Midpoint { .. },
            ..
        })
    ));
    assert!(matches!(
        constraints.get(2),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Coincident { .. },
            ..
        })
    ));
    assert!(matches!(
        constraints.get(3),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::ParallelToDirection {
                entity,
                direction,
            },
            ..
        }) if entity == &crate::ids::neutral_spatial_sketch_curve_id(
            &sketches[0].id,
            7,
            0,
        ) && direction == &Vector3::new(0.0, 1.0, 0.0)
    ));
    assert!(matches!(
        constraints.get(4),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraint {
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::PointOnSurface { .. },
            ..
        })
    ));
    assert!(entities.iter().any(|entity| matches!(
        entity.geometry,
        SpatialSketchGeometry::Circle {
            center,
            normal,
            reference_direction,
            radius: Length(2.0),
        } if center == Point3::new(13.0, 21.0, 32.0)
            && normal == Vector3::new(0.0, 1.0, 0.0)
            && reference_direction == Vector3::new(0.0, 0.0, 1.0)
    )));
}
