use super::*;

#[test]
fn line_angles_retain_shallow_and_near_opposite_angles() {
    const ANGLE: f64 = 1e-8;
    let first = [[0.0, 0.0], [1.0, 0.0]];
    let second = [[0.0, 0.0], [ANGLE.cos(), ANGLE.sin()]];
    assert!(
        (super::super::line_line_angle(first, second).unwrap() - ANGLE).abs() <= f64::EPSILON * ANGLE
    );
    let opposite = [[0.0, 0.0], [-ANGLE.cos(), ANGLE.sin()]];
    assert!(
        (super::super::line_line_angle(first, opposite).unwrap() - (std::f64::consts::PI - ANGLE))
            .abs()
            <= f64::EPSILON * std::f64::consts::PI
    );
}

#[test]
fn numerical_audit_line_angles_are_finite_for_large_directions() {
    let x = [[0.0, 0.0], [1.0e200, 0.0]];
    let y = [[0.0, 0.0], [0.0, 1.0e200]];
    assert_eq!(super::super::line_line_angle(x, x), Some(0.0));
    assert_eq!(
        super::super::line_line_angle(x, y),
        Some(std::f64::consts::FRAC_PI_2)
    );
    assert_eq!(super::super::dynamic_line_line_angle(x, x), Some(0.0));
}

fn sketch_marker(
    id: &str,
    ordinal: u32,
    offset: u64,
    kind: crate::records::SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> crate::records::SketchInputEntity {
    let mut marker = crate::records::SketchInputEntity::new(id, "lane", ordinal, offset, kind);
    marker.feature_ref = Some("sketch".into());
    marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
    marker
}

fn dynamic_point_markers() -> Vec<crate::records::SketchInputEntity> {
    vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
    ]
}

#[test]
fn dynamic_point_relation_uses_unique_geometry_match_across_address_tiers() {
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        )],
    );
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_preserves_explicit_driving_operand_reference() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    driving.operands[0].entity_ref = Some("p1".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_falls_back_to_unique_geometry_after_address_miss() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[3, 1],
        1.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p0"), Some("p2")]
    );
}

#[test]
fn relation_instance_keeps_first_scalar_operands_when_grouping_display_and_driver() {
    let mut display = dynamic_scalar(
        20,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    display.role = FeatureInputScalarRole::Display;
    display.operands[0].entity_ref = Some("p1".into());
    let mut driving = dynamic_scalar(
        30,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![display, driving.clone()],
    );
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one grouped relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id)
    );
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_display_relation_uses_display_value_without_driver() {
    let mut display = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    display.role = FeatureInputScalarRole::Display;
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![display]);
    lane.sketch_entities = dynamic_point_markers();

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation.parameter_scalar_ref().is_none());
    assert_eq!(
        relation
            .operands
            .iter()
            .map(|operand| operand.entity_ref.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("p1"), Some("p2")]
    );
}

#[test]
fn dynamic_point_relation_with_ambiguous_geometry_stays_unbound() {
    let mut lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            1.0,
        )],
    );
    lane.sketch_entities = dynamic_point_markers();
    lane.sketch_entities[3].coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}

#[test]
fn dynamic_point_line_relation_resolves_solver_line_by_exact_distance() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        1.0,
    );
    driving.operands[1].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgPntLineDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "relation",
            0,
            1,
            crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Distance,
            ),
            None,
        ),
        sketch_marker(
            "p0",
            1,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            2,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            3,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 1.0]),
        ),
        sketch_marker(
            "p3",
            4,
            40,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert_eq!(relation.operands[0].entity_ref.as_deref(), Some("p0"));
    assert!(relation.operands[1].entity_ref.is_none());
}

#[test]
fn dynamic_line_relation_validates_solver_line_pair() {
    let mut driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        1.0,
    );
    driving.operands[0].entity_ref = Some("p0".into());
    let mut lane = lane(vec![class(10, "sgLLDist")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "p0",
            0,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "p1",
            1,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "p2",
            2,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 1.0]),
        ),
        sketch_marker(
            "p3",
            3,
            40,
            crate::records::SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}

#[test]
fn dynamic_angle_accepts_reversed_solver_line_direction() {
    let driving = dynamic_scalar(
        40,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[0, 1],
        std::f64::consts::FRAC_PI_4,
    );
    let mut lane = lane(vec![class(10, "sgAnglDim")], vec![driving]);
    lane.sketch_entities = vec![
        sketch_marker(
            "first-start",
            0,
            10,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "first-end",
            1,
            20,
            crate::records::SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
        sketch_marker(
            "second-start",
            2,
            30,
            crate::records::SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        sketch_marker(
            "second-end",
            3,
            40,
            crate::records::SketchInputKind::Point,
            Some([-1.0, 1.0]),
        ),
    ];

    let instances = relation_instances(
        &cadmpeg_test_support::service_decode_context(),
        &sketch_history(),
        &lane,
    )
    .unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one dynamically tagged angle relation");
    };
    assert!(relation
        .operands
        .iter()
        .all(|operand| operand.entity_ref.is_none()));
}
