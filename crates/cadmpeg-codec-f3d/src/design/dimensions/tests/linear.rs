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
fn dimension_proofs_require_the_evaluated_measurement() {
    const DOCUMENT_LINEAR_TOLERANCE: f64 = 1.0e-6;

    let dimension = |source_kind: &str, unit: &str| {
        parse_design_parameter(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some(unit),
            "d1",
            1.0,
        ))
        .expect("generated dimension parameter is canonical")
    };
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "mm")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Radial Dimension-3", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "deg")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "rad")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Tangent Dimension-2", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Tangent Dimension-2", "deg")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Radius Dimension-2", "native-unit")
    ));

    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let first = entity(
        "generated:point#0",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let second = entity(
        "generated:point#1",
        SketchGeometry::Point {
            position: Point2::new(40.0, 0.0),
        },
    );
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#0".into());
    assert!(crate::design::dimensions::directional_point_dimension(
        &[&first, &second],
        10.0,
        parameter.clone(),
        0.0,
    )
    .is_none());
    assert!(matches!(
        crate::design::dimensions::directional_point_dimension(
            &[&first, &second],
            40.0,
            parameter.clone(),
            0.0,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));
    let rounded = entity(
        "generated:point#rounded",
        SketchGeometry::Point {
            position: Point2::new(40.000_000_5, 0.0),
        },
    );
    assert!(matches!(
        crate::design::dimensions::directional_point_dimension(
            &[&first, &rounded],
            40.0,
            parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));

    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let diagonal = entity(
        "generated:line#diagonal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 10.0),
        },
    );
    assert!(!crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &diagonal,
        2.0,
        0.0,
    ));
    assert!(!crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_6,
    ));
    assert!(crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_4,
    ));
    let vertical = entity(
        "generated:line#vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, -10.0),
            end: Point2::new(0.0, 10.0),
        },
    );
    let offset_point = entity(
        "generated:point#offset",
        SketchGeometry::Point {
            position: Point2::new(2.0, 4.0),
        },
    );
    assert!(crate::design::dimensions::point_line_separation(
        &offset_point,
        &vertical,
        2.0,
        0.0,
    ));
    assert!(crate::design::dimensions::point_line_separation(
        &offset_point,
        &vertical,
        2.000_000_5,
        1.0e-6,
    ));
    assert!(!crate::design::dimensions::point_line_separation(
        &vertical,
        &offset_point,
        3.0,
        1.0e-6,
    ));

    let inner_circle = entity(
        "generated:circle#inner",
        SketchGeometry::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::features::Length(4.0),
        },
    );
    let outer_circle = entity(
        "generated:circle#outer",
        SketchGeometry::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::features::Length(4.25),
        },
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.25,
        0.0,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.5,
        0.0,
    ));
    let displaced_circle = entity(
        "generated:circle#displaced",
        SketchGeometry::Circle {
            center: Point2::new(3.001, -2.0),
            radius: cadmpeg_ir::features::Length(4.25),
        },
    );
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &displaced_circle,
        0.25,
        0.0,
    ));

    let tolerant_center_circle = entity(
        "generated:circle#tolerant-center",
        SketchGeometry::Circle {
            center: Point2::new(3.000_000_5, -2.0),
            radius: cadmpeg_ir::features::Length(4.25),
        },
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_center_circle,
        0.25,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_center_circle,
        0.25,
        0.0,
    ));
    let tolerant_parallel = entity(
        "generated:line#tolerant-parallel",
        SketchGeometry::Line {
            start: Point2::new(0.0, 2.000_000_5),
            end: Point2::new(10.0, 2.000_000_5),
        },
    );
    assert!(crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &tolerant_parallel,
        2.0,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &tolerant_parallel,
        2.0,
        0.0,
    ));
    let tolerant_outer_circle = entity(
        "generated:circle#tolerant-outer",
        SketchGeometry::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::features::Length(4.250_000_5),
        },
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_outer_circle,
        0.25,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_outer_circle,
        0.25,
        0.0,
    ));
}

#[test]
fn presentation_dimensions_use_direct_operands_with_measurement_proofs() {
    let sketch = SketchId("generated:sketch#presentation".into());
    let entity = |record_index: u32, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId(format!("generated:entity#{record_index}")),
            sketch.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("stream:geometry#{record_index}")))
    };
    let line = entity(
        306,
        SketchGeometry::Line {
            start: Point2::new(90.4875, -17.78),
            end: Point2::new(90.4875, 17.78),
        },
    );
    let circle = entity(
        331,
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::features::Length(11.1125),
        },
    );
    let arc = entity(
        796,
        SketchGeometry::Arc {
            center: Point2::new(60.344_057_626_1, -19.05),
            radius: cadmpeg_ir::features::Length(12.7),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(0.975_682_713_4),
        },
    );
    let outer_arc = entity(
        782,
        SketchGeometry::Arc {
            center: Point2::new(60.344_057_626_1, 19.05),
            radius: cadmpeg_ir::features::Length(12.7),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(0.975_682_713_4),
        },
    );
    let first_point = entity(
        1061,
        SketchGeometry::Point {
            position: Point2::new(11.1125, -11.1125),
        },
    );
    let second_point = entity(
        1075,
        SketchGeometry::Point {
            position: Point2::new(11.1125, -7.3025),
        },
    );
    let entities = [line, circle, arc, outer_arc, first_point, second_point];
    let projected = entities
        .iter()
        .map(|entity| {
            let record_index = entity
                .native_ref
                .as_deref()
                .and_then(|native_ref| native_ref.rsplit_once('#'))
                .and_then(|(_, index)| index.parse::<u32>().ok())
                .expect("synthetic native geometry record");
            (("stream", record_index), entity)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let frame = |operands| crate::records::DesignDimensionPresentationFrame {
        id: "stream:presentation#0".into(),
        byte_offset: 0,
        class_tag: "314".into(),
        record_index: 0,
        frame_length: 0,
        operands,
        presentation_bytes: Vec::new(),
        presentation_byte_offset: 0,
        paired_class_tag: "281".into(),
        paired_byte_offset: 0,
        owner_reference: 0,
        owner_reference_offset: 0,
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    };
    let operand = |record_index| crate::records::DesignDimensionAnnotationOperand {
        geometry_record_index: record_index,
        geometry_reference_offset: 0,
        role: 0,
        role_offset: 0,
    };
    let tangent_span = parse_design_parameter(&parameter_record(
        Some(44),
        "10.16",
        "Tangent Dimension-2",
        Some("cm"),
        "d4",
        10.16,
    ))
    .expect("synthetic tangent dimension");
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition(
            "stream",
            &frame(vec![operand(306), operand(331)]),
            &projected,
            &tangent_span,
            &cadmpeg_ir::features::ParameterId("parameter:d4".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Distance { entities, .. })
            if entities.len() == 2
    ));

    let tangent_radius = parse_design_parameter(&parameter_record(
        Some(45),
        "1.27",
        "Tangent Dimension-2",
        Some("cm"),
        "d16",
        1.27,
    ))
    .expect("synthetic tangent radius dimension");
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition(
            "stream",
            &frame(vec![operand(796)]),
            &projected,
            &tangent_radius,
            &cadmpeg_ir::features::ParameterId("parameter:d16".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Radius { entity, .. })
            if entity.0 == "generated:entity#796"
    ));
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition(
            "stream",
            &frame(vec![operand(782), operand(796)]),
            &projected,
            &tangent_radius,
            &cadmpeg_ir::features::ParameterId("parameter:d16".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Distance { entities, .. })
            if entities.len() == 2
    ));
    let ambiguous_tangent = parse_design_parameter(&parameter_record(
        Some(47),
        "3.81",
        "Tangent Dimension-2",
        Some("cm"),
        "d16_ambiguous",
        3.81,
    ))
    .expect("synthetic ambiguous tangent dimension");
    assert!(
        crate::design::dimensions::presentation_dimension_definition(
            "stream",
            &frame(vec![operand(782), operand(796)]),
            &projected,
            &ambiguous_tangent,
            &cadmpeg_ir::features::ParameterId("parameter:d16_ambiguous".into()),
            1.0e-6,
        )
        .is_none()
    );

    let point_distance = parse_design_parameter(&parameter_record(
        Some(46),
        "0.381",
        "Linear Dimension-2",
        Some("cm"),
        "d32",
        0.381,
    ))
    .expect("synthetic point distance dimension");
    let point_definition = crate::design::dimensions::presentation_dimension_definition(
        "stream",
        &frame(vec![operand(1061), operand(1075)]),
        &projected,
        &point_distance,
        &cadmpeg_ir::features::ParameterId("parameter:d32".into()),
        1.0e-6,
    );
    assert!(matches!(
        point_definition,
        Some(SketchConstraintDefinition::VerticalDistance { .. })
    ));
}

#[test]
fn symmetric_parallel_line_dimension_uses_twice_the_carrier_gap() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#symmetric-distance".into()),
            geometry,
        )
    };
    let first = entity(
        "generated:line#first",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 10.0),
        },
    );
    let second = entity(
        "generated:line#second",
        SketchGeometry::Line {
            start: Point2::new(5.0, 2.0),
            end: Point2::new(5.0, 8.0),
        },
    );
    let parameter = parse_design_parameter(&parameter_record(
        Some(44),
        "value",
        "Linear Dimension-3",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("symmetric line-width parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("generated:parameter#symmetric".into());

    assert!(matches!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            &first,
            &second,
            1,
            1,
            &parameter,
            parameter_id.clone(),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Distance { entities, parameter: actual })
            if entities == vec![first.id().clone(), second.id().clone()] && actual == parameter_id
    ));

    let mut direct_parameter = parameter.clone();
    direct_parameter.evaluated_value = 0.5;
    assert!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            &first,
            &second,
            1,
            1,
            &direct_parameter,
            parameter_id.clone(),
            1.0e-6,
        )
        .is_none()
    );
    assert!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            &first,
            &second,
            0,
            1,
            &parameter,
            parameter_id,
            1.0e-6,
        )
        .is_none()
    );
}

#[test]
fn counted_linear_graph_selects_one_parameter_backed_direction() {
    let entity = |id: &str, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            SketchGeometry::Point { position },
        )
    };
    let first = entity("generated:point#first", Point2::new(4.0, 16.0));
    let second = entity("generated:point#second", Point2::new(4.0, 14.0));
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

    let definition =
        directional_point_dimension(&[&first, &second], 2.0, parameter.clone(), 0.0).unwrap();
    assert!(matches!(
        definition,
        SketchConstraintDefinition::VerticalDistance {
            first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
            second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
            parameter: ref parameter_id,
        } if first_id == first.id() && second_id == second.id() && parameter_id == &parameter
    ));
    assert!(directional_point_dimension(&[&first, &second], 3.0, parameter, 0.0).is_none());

    let diagonal = entity("generated:point#diagonal", Point2::new(7.0, 14.0));
    assert!(matches!(
        directional_point_dimension(
            &[&first, &diagonal],
            3.0,
            cadmpeg_ir::features::ParameterId("generated:parameter#horizontal".into()),
            0.0,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));
    let square = entity("generated:point#square", Point2::new(6.0, 18.0));
    assert!(directional_point_dimension(
        &[&first, &square],
        2.0,
        cadmpeg_ir::features::ParameterId("generated:parameter#ambiguous".into()),
        0.0,
    )
    .is_none());
}

#[test]
fn unclassified_two_locus_linear_group_is_parameter_backed_distance() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let point = entity(
        "generated:point#dimension",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let line = entity(
        "generated:line#dimension",
        SketchGeometry::Line {
            start: Point2::new(-10.0, 0.0),
            end: Point2::new(-50.0, 0.0),
        },
    );
    let parameter = cadmpeg_ir::features::ParameterId("generated:parameter#distance".into());

    assert!(exact_counted_dimension_relation(&[&point, &line]).is_none());
    assert!(matches!(
        two_locus_distance_dimension(&[&point, &line], parameter.clone()),
        Some(SketchConstraintDefinition::Distance {
            ref entities,
            parameter: ref actual_parameter,
        }) if entities == &[point.id().clone(), line.id().clone()] && actual_parameter == &parameter
    ));
}

#[test]
fn counted_linear_graph_projects_exact_auxiliary_relations() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("generated:sketch#0".into()),
            geometry,
        )
    };
    let horizontal = entity(
        "generated:line#horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let vertical = entity(
        "generated:line#vertical",
        SketchGeometry::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        },
    );
    let parallel = entity(
        "generated:line#parallel",
        SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(10.0, 2.0),
        },
    );
    let point = entity(
        "generated:point#on-line",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let duplicate_point = entity(
        "generated:point#duplicate",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let arc = entity(
        "generated:arc#bounded",
        SketchGeometry::Arc {
            center: Point2::new(3.0, 0.0),
            radius: cadmpeg_ir::features::Length(1.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        },
    );
    let arc_start = entity(
        "generated:point#arc-start",
        SketchGeometry::Point {
            position: Point2::new(4.0, 0.0),
        },
    );
    let outside_arc = entity(
        "generated:point#outside-arc",
        SketchGeometry::Point {
            position: Point2::new(2.0, 0.0),
        },
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &vertical]),
        Some(SketchConstraintDefinition::Perpendicular { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &parallel]),
        Some(SketchConstraintDefinition::Parallel { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &point]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&point, &duplicate_point]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&arc_start, &arc]),
        Some(SketchConstraintDefinition::Coincident { .. })
    ));
    assert!(exact_counted_dimension_relation(&[&outside_arc, &arc]).is_none());
}

#[test]
fn exact_pair_suppresses_counted_frames_in_its_containing_companion() {
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
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind: "Linear Dimension-4".into(),
        source_kind_offset: 0,

        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "d4".into(),
        name_offset: 0,
        evaluated_value: 0.2,
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
        evaluated_value: 0.2,
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
    let pair = DesignDimensionLocusPair {
        id: format!("{stream}:design-dimension-locus-pair#30"),
        companion_record_index: 99,
        governing_companion_record_index: 22,
        byte_offset: 30,
        class_tag: "277".into(),
        record_index: 30,
        frame_length: 100,
        opaque_index: 0,
        opaque_index_offset: 65,
        first_geometry_record_index: 40,
        first_geometry_reference_offset: 70,
        first_role: 7,
        first_role_offset: 80,
        second_geometry_record_index: 41,
        second_geometry_reference_offset: 85,
        second_role: 8,
        second_role_offset: 95,
        paired_class_tag: "273".into(),
        paired_byte_offset: 130,
    };
    let group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#140"),
        companion_record_index: 99,
        byte_offset: 140,
        class_tag: "277".into(),
        record_index: 31,
        frame_length: 100,
        loci: vec![DesignDimensionLocus {
            geometry_record_index: 40,
            geometry_reference_offset: 170,
            role: 0,
            role_offset: 180,
        }],
        owner_reference: 100,
        owner_reference_offset: 185,
        owner_role: 0,
        owner_role_offset: 195,
        state: 0,
        state_offset: 199,
        constraint_kinds: Vec::new(),
        unknown_constraint_bits: 0,
        return_members: vec![40],
        return_member_offsets: vec![210],
        next_class_tag: "273".into(),
        next_record_index: 32,
        next_byte_offset: 240,
    };
    let point = |record_index, y| SketchPoint {
        id: format!("{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            u64::from(record_index),
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, y),
        depth: 0.0,
        companion: None,
    };
    let points = [point(40, 0.0), point(41, 2.0)];
    let sketch = neutral_sketch_id(&placement);
    let entities = points
        .iter()
        .map(|point| {
            SketchEntity::new(
                SketchEntityId(format!("point-{}", point.record_index)),
                sketch.clone(),
                SketchGeometry::Point {
                    position: point.coordinates,
                },
            )
            .with_native_ref(Some(point.id.clone()))
        })
        .collect::<Vec<_>>();

    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );

    assert_eq!(constraints.len(), 1);
    assert!(matches!(
        constraints[0].definition,
        SketchConstraintDefinition::VerticalDistance { .. }
    ));

    let spatial_sketch = SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    let spatial_entities = points
        .iter()
        .map(|point| {
            cadmpeg_ir::sketches::SpatialSketchEntity::new(
                cadmpeg_ir::sketches::SpatialSketchEntityId(format!(
                    "spatial-point-{}",
                    point.record_index
                )),
                spatial_sketch.id.clone(),
                cadmpeg_ir::sketches::SpatialSketchGeometry::Point {
                    position: Point3::new(0.0, point.coordinates.v, 0.0),
                },
            )
            .with_native_ref(Some(point.id.clone()))
        })
        .collect::<Vec<_>>();
    assert!(project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        std::slice::from_ref(&spatial_sketch),
    )
    .is_empty());
    let spatial_constraints = project_spatial_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &spatial_entities,
    );
    assert_eq!(spatial_constraints.len(), 1, "{spatial_constraints:#?}");
    assert!(matches!(
        &spatial_constraints[0],
        cadmpeg_ir::sketches::SpatialSketchConstraint {
            sketch: actual_sketch,
            definition: SpatialSketchConstraintDefinition::PointDistance {
                first,
                second,
                parameter: actual_parameter,
            },
            ..
        } if actual_sketch == &spatial_sketch.id
            && actual_parameter == &neutral_parameter_id_parts(stream, 20)
            && first == spatial_entities[0].id()
            && second == spatial_entities[1].id()
    ));

    let axis_record = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#42"),
        record_index: 42,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 42,
        secondary_id: 0,
        geometry: None,
    };
    let axis_entity = cadmpeg_ir::sketches::SpatialSketchEntity::new(
        cadmpeg_ir::sketches::SpatialSketchEntityId("spatial-axis".into()),
        spatial_sketch.id.clone(),
        cadmpeg_ir::sketches::SpatialSketchGeometry::Line {
            start: Point3::new(-1.0, 1.0, 0.0),
            end: Point3::new(1.0, 1.0, 0.0),
        },
    )
    .with_construction(true)
    .with_native_ref(Some(axis_record.id.clone()));
    let symmetry_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#31"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 31,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                geometry_record_index: 42,
                geometry_reference_offset: 0,
                role: 5,
                role_offset: 0,
            },
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
        owner_role: 0x400,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        constraint_kinds: Vec::new(),
        unknown_constraint_bits: 0,
        return_members: vec![40, 41, 42],
        return_member_offsets: vec![0, 0, 0],
        next_class_tag: "273".into(),
        next_record_index: 32,
        next_byte_offset: 0,
    };
    let mut symmetry_parameter = parameter.clone();
    symmetry_parameter.source_kind = "Linear Dimension-6".into();
    let mut symmetry_entities = spatial_entities.clone();
    symmetry_entities.push(axis_entity.clone());
    let symmetry_constraints = project_spatial_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&symmetry_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&symmetry_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: std::slice::from_ref(&axis_record),
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &symmetry_entities,
    );
    assert_eq!(symmetry_constraints.len(), 2, "{symmetry_constraints:#?}");
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        &constraint.definition,
        SpatialSketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } if first == spatial_entities[0].id()
            && second == spatial_entities[1].id()
            && axis == axis_entity.id()
    )));
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        constraint.definition,
        SpatialSketchConstraintDefinition::Native {
            parameter: Some(_),
            ..
        }
    )));

    let mut zero_parameter = parameter;
    zero_parameter.evaluated_value = 0.0;
    let mut duplicate_pair = pair.clone();
    duplicate_pair.second_geometry_record_index = duplicate_pair.first_geometry_record_index;
    let duplicate = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&duplicate_pair),
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(duplicate.len(), 1);
    assert!(matches!(
        duplicate[0].definition,
        SketchConstraintDefinition::Native { ref operands, .. }
            if operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_role, operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("first_locus"), Some(7), 40),
                    (Some("second_locus"), Some(8), 40),
                ]
    ));

    let mut group_owner = owner.clone();
    group_owner.companion_record_index = group.companion_record_index;
    let combined = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: &[owner, group_owner.clone()],
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(combined.len(), 2);

    let grouped = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert!(matches!(
        grouped.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native {
                native_state: Some(0),
                native_flags: None,
                operands,
                ..
            },
            ..
        }] if operands.iter().map(|operand| (operand.native_field.as_deref(), operand.native_role, operand.object_index)).collect::<Vec<_>>()
            == [
                (Some("locus"), Some(0), 40),
                (Some("owner"), Some(0), 100),
                (Some("return"), None, 40),
            ]
    ));

    let mut indirect_group = group;
    indirect_group.owner_reference = 999;
    let indirect = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&indirect_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(indirect.len(), 1);
    assert_eq!(indirect[0].sketch, sketch);
}

#[test]
fn repeated_linear_dimension_requires_disjoint_measurement_pairs() {
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchDistanceMeasurement as Measurement,
        SketchEntityId, SketchLocus,
    };

    let entity = |name: &str| SketchEntityId(format!("generated:{name}"));
    let parameter = ParameterId("generated:distance".into());
    let horizontal = |first: &str, second: &str| Definition::HorizontalDistance {
        first: SketchLocus::Entity(entity(first)),
        second: SketchLocus::Entity(entity(second)),
        parameter: parameter.clone(),
    };
    let candidates = vec![horizontal("a", "b"), horizontal("c", "d")];
    let Definition::RepeatedDistance {
        measurements,
        parameter: actual,
    } = repeated_linear_dimension(&candidates, parameter.clone()).unwrap()
    else {
        panic!("expected repeated distance")
    };
    assert_eq!(actual, parameter);
    assert!(matches!(
        measurements.as_slice(),
        [
            Measurement::Horizontal { .. },
            Measurement::Horizontal { .. }
        ]
    ));

    let ambiguous = vec![horizontal("a", "b"), horizontal("a", "c")];
    assert!(repeated_linear_dimension(&ambiguous, parameter).is_none());
}
