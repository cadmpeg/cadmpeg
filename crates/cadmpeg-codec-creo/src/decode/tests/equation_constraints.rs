// SPDX-License-Identifier: Apache-2.0
//! Unit tests for section-equation constraint transfer.

use crate::decode::sketch_transfer::{
    section_equation_axis_distance_constraints, section_equation_equal_distance_constraints,
    section_equation_function_forty_two_midpoint_coordinate_constraints,
    section_equation_function_six_distance_constraints,
    section_equation_function_thirty_one_point_coordinate_constraints,
    section_equation_native_constraints, section_equation_point_on_line_constraints,
    section_equation_polar_distance_constraints, section_equation_radius_dimension_constraints,
    section_equation_unsigned_distance_constraints,
};
use cadmpeg_ir::features::{Angle, Length, ParameterId};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchDistancePair, SketchEntityId, SketchLocus,
};
use std::collections::BTreeSet;

#[test]
fn equation_native_fallback_retains_untyped_row_slots_and_activity() {
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x04\xf8\x03\xf6\x02\x03\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_native_constraints(&definition, &sketch, &BTreeSet::new());
    assert_eq!(constraints.len(), 1);
    let (constraint, offset) = &constraints[0];
    assert_eq!(*offset, 28);
    assert_eq!(
        constraint.id.0,
        "creo:featdefs:sketch_constraint#40:equation:offset:28"
    );
    assert_eq!(constraint.active, Some(true));
    let SketchConstraintDefinition::Native {
        native_kind,
        native_state,
        native_properties,
        operands,
        ..
    } = &constraint.definition
    else {
        panic!("equation fallback must be native");
    };
    assert_eq!(native_kind, "creo:equation:4");
    assert_eq!(*native_state, Some(1));
    assert_eq!(native_properties["equation_id"], "1");
    assert_eq!(native_properties["function_id"], "4");
    assert_eq!(native_properties["explicit_argument_count"], "3");
    assert_eq!(native_properties["argument_slots"], "0:null,1:2,2:3");
    assert_eq!(native_properties["null_argument_ordinals"], "0");
    assert_eq!(operands.len(), 3);
    assert_eq!(operands[0].native_kind, "eqtn_arr");
    assert_eq!(operands[0].object_index, 1);
    assert_eq!(operands[1].native_field.as_deref(), Some("arguments[1]"));
    assert_eq!(operands[1].object_index, 2);
    assert_eq!(operands[2].native_field.as_deref(), Some("arguments[2]"));
    assert_eq!(operands[2].object_index, 3);
    assert!(
        section_equation_native_constraints(&definition, &sketch, &BTreeSet::from([28]),)
            .is_empty()
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints =
        section_equation_native_constraints(&disabled, &sketch, &BTreeSet::new());
    assert_eq!(disabled_constraints[0].0.active, Some(false));
    let SketchConstraintDefinition::Native { native_state, .. } =
        &disabled_constraints[0].0.definition
    else {
        panic!("equation fallback must be native");
    };
    assert_eq!(*native_state, Some(0));
}

#[test]
fn equation_function_two_emits_radius_dimension_constraint() {
    let variable =
        |variable_type, key, value, dimension_driven| crate::feature::FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: dimension_driven,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven,
            offset: 0,
        };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x02\xf8\x02\x00\x01\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                variable(3, 42, None, true),
                variable(0, 0, Some(5.0), false),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: Vec::new(),
            circle_rows: vec![crate::feature::FeatureCircleSegment {
                center_id: 11,
                radius_ref: 42,
                external_id: 13,
                offset: 13,
            }],
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureDimension {
                dimension_type: 3,
                value: Some(5.0),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 100,
                references: None,
                offset: 0,
            }],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_radius_dimension_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].1, 28);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1:radius:13"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::Radius {
            entity: SketchEntityId("creo:featdefs:sketch_entity#40:13".into()),
            parameter: ParameterId("creo:featdefs:parameter#40:100".into()),
        }
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_radius_dimension_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_zero_emits_polar_distance_constraint() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x00\xf8\x06\x00\x01\x02\x03\x04\x05\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 6,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0)),
                variable(2, 1, Some(0.0)),
                variable(1, 2, Some(0.0)),
                variable(2, 2, Some(2.0)),
                variable(3, 9, Some(2.0)),
                variable(6, 10, Some(std::f64::consts::FRAC_PI_2)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [1, 3]), line(11, [2, 4])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_polar_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].1, 28);
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::PolarDistance {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:11".into(),)),
            distance: Length(2.0),
            angle: Some(Angle(std::f64::consts::FRAC_PI_2)),
            distance_parameter: None,
        }
    );

    let mut propagated_polar = definition.clone();
    propagated_polar.body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x00\xf8\x06\x00\x01\x02\x03\x04\x05\xf6\xe2\
            \x02\x05\xf8\x03\x05\x06\x07\xf6\xe2"
        .to_vec();
    let variables = propagated_polar.variables.as_mut().expect("variables");
    variables.declared_count = 8;
    variables.rows[2].value = None;
    variables.rows[3].value = None;
    variables.rows[5].value = None;
    variables
        .rows
        .push(variable(6, 11, Some(std::f64::consts::FRAC_PI_2)));
    variables.rows.push(variable(5, 0, Some(0.0)));
    let propagated_constraints =
        section_equation_polar_distance_constraints(&propagated_polar, &sketch);
    assert_eq!(propagated_constraints.len(), 1);
    assert_eq!(
        propagated_constraints[0].0.definition,
        SketchConstraintDefinition::PolarDistance {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:11".into(),)),
            distance: Length(2.0),
            angle: Some(Angle(std::f64::consts::FRAC_PI_2)),
            distance_parameter: None,
        }
    );
    let mut conflicting_equality_polar = propagated_polar.clone();
    conflicting_equality_polar
        .variables
        .as_mut()
        .expect("variables")
        .rows[5]
        .value = Some(0.0);
    assert!(
        section_equation_polar_distance_constraints(&conflicting_equality_polar, &sketch)
            .is_empty()
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_polar_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_six_emits_fixed_distance_constraint() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x06\xf8\x05\x00\x01\x02\x03\x04\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 5,
            entity_ref: None,
            rows: vec![
                variable(1, 10, Some(0.0)),
                variable(2, 10, Some(0.0)),
                variable(1, 11, Some(3.0)),
                variable(2, 11, Some(4.0)),
                variable(3, 20, Some(5.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [10, 12]), line(11, [11, 13])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_function_six_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].1, 28);
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::DistanceLociValue {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into())),
            second: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:11".into())),
            distance: Length(5.0),
            parameter: None,
        }
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints =
        section_equation_function_six_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_functions_thirty_one_and_forty_two_emit_coordinate_constraints() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let function_forty_two = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x2a\xf8\x03\x00\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![
                variable(1, 10, Some(0.0)),
                variable(1, 11, Some(4.0)),
                variable(6, 20, Some(2.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(30, [10, 12]), line(31, [11, 13])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let midpoint_constraints = section_equation_function_forty_two_midpoint_coordinate_constraints(
        &function_forty_two,
        &sketch,
    );
    assert_eq!(midpoint_constraints.len(), 1);
    assert_eq!(midpoint_constraints[0].1, 28);
    assert_eq!(midpoint_constraints[0].0.active, Some(true));
    assert_eq!(
        midpoint_constraints[0].0.definition,
        SketchConstraintDefinition::MidpointCoordinate {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:30".into())),
            second: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:31".into())),
            axis: cadmpeg_ir::sketches::SketchCoordinateAxis::U,
            value: Length(2.0),
        }
    );

    let mut propagated_midpoint = function_forty_two.clone();
    propagated_midpoint.body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x2a\xf8\x03\x00\x01\x02\xf6\xe2\
            \x02\x05\xf8\x03\x02\x03\x04\xf6\xe2"
        .to_vec();
    let variables = propagated_midpoint.variables.as_mut().expect("variables");
    variables.declared_count = 5;
    variables.rows[2].value = None;
    variables.rows.push(variable(6, 21, Some(2.0)));
    variables.rows.push(variable(5, 0, Some(0.0)));
    let propagated_constraints =
        section_equation_function_forty_two_midpoint_coordinate_constraints(
            &propagated_midpoint,
            &sketch,
        );
    assert_eq!(propagated_constraints.len(), 1);
    assert_eq!(
        propagated_constraints[0].0.definition,
        SketchConstraintDefinition::MidpointCoordinate {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:30".into())),
            second: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:31".into())),
            axis: cadmpeg_ir::sketches::SketchCoordinateAxis::U,
            value: Length(2.0),
        }
    );
    let mut conflicting_equality_midpoint = propagated_midpoint.clone();
    let variables = conflicting_equality_midpoint
        .variables
        .as_mut()
        .expect("variables");
    variables.rows[2].value = Some(2.0);
    variables.rows[3].value = Some(3.0);
    assert!(
        section_equation_function_forty_two_midpoint_coordinate_constraints(
            &conflicting_equality_midpoint,
            &sketch,
        )
        .is_empty()
    );
    let mut conflicting_midpoint = function_forty_two;
    conflicting_midpoint
        .variables
        .as_mut()
        .expect("variables")
        .rows[2]
        .value = Some(3.0);
    assert!(
        section_equation_function_forty_two_midpoint_coordinate_constraints(
            &conflicting_midpoint,
            &sketch,
        )
        .is_empty()
    );

    let function_thirty_one = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x1f\xf8\x04\x00\x01\x02\x03\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 4,
            entity_ref: None,
            rows: vec![
                variable(1, 10, Some(2.0)),
                variable(2, 10, Some(1.0)),
                variable(6, 20, Some(2.0)),
                variable(6, 21, Some(1.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(30, [10, 12])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let point_constraints = section_equation_function_thirty_one_point_coordinate_constraints(
        &function_thirty_one,
        &sketch,
    );
    assert_eq!(point_constraints.len(), 1);
    assert_eq!(point_constraints[0].1, 28);
    assert_eq!(point_constraints[0].0.active, Some(true));
    assert_eq!(
        point_constraints[0].0.definition,
        SketchConstraintDefinition::PointCoordinateValues {
            point: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:30".into())),
            values: [Length(2.0), Length(1.0)],
        }
    );

    let mut propagated_point = function_thirty_one.clone();
    propagated_point.body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x1f\xf8\x04\x00\x01\x02\x03\xf6\xe2\
            \x02\x05\xf8\x03\x02\x04\x05\xf6\xe2"
        .to_vec();
    let variables = propagated_point.variables.as_mut().expect("variables");
    variables.declared_count = 6;
    variables.rows[2].value = None;
    variables.rows.push(variable(6, 22, Some(2.0)));
    variables.rows.push(variable(5, 0, Some(0.0)));
    let propagated_constraints = section_equation_function_thirty_one_point_coordinate_constraints(
        &propagated_point,
        &sketch,
    );
    assert_eq!(propagated_constraints.len(), 1);
    assert_eq!(
        propagated_constraints[0].0.definition,
        SketchConstraintDefinition::PointCoordinateValues {
            point: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:30".into())),
            values: [Length(2.0), Length(1.0)],
        }
    );
    let mut conflicting_equality_point = propagated_point.clone();
    let variables = conflicting_equality_point
        .variables
        .as_mut()
        .expect("variables");
    variables.rows[2].value = Some(2.0);
    variables.rows[4].value = Some(3.0);
    assert!(
        section_equation_function_thirty_one_point_coordinate_constraints(
            &conflicting_equality_point,
            &sketch,
        )
        .is_empty()
    );
    let mut conflicting_point = function_thirty_one;
    conflicting_point
        .variables
        .as_mut()
        .expect("variables")
        .rows[0]
        .value = Some(3.0);
    assert!(
        section_equation_function_thirty_one_point_coordinate_constraints(
            &conflicting_point,
            &sketch,
        )
        .is_empty()
    );
}

#[test]
fn equation_function_thirty_three_emits_equal_distance_pairs() {
    let row = |variable_type, key| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value: Some(0.0),
        value_body: Vec::new(),
        guess: Some(0.0),
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x21\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 1),
                row(2, 1),
                row(1, 2),
                row(2, 2),
                row(1, 3),
                row(2, 3),
                row(1, 4),
                row(2, 4),
                row(7, 0),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [1, 2]), line(11, [3, 4])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_equal_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::EqualDistance {
            first: SketchDistancePair {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:10".into(),
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:10".into(),
                )),
            },
            second: SketchDistancePair {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:11".into(),
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:11".into(),
                )),
            },
        }
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_equal_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_thirty_five_emits_point_on_line() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let segment = |kind, external_id, point_ids| crate::feature::FeatureSegment {
        kind,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x23\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 3, None),
                row(2, 3, None),
                row(1, 1, Some(0.0)),
                row(2, 1, Some(0.0)),
                row(1, 2, Some(10.0)),
                row(2, 2, Some(0.0)),
                row(4, 0, Some(0.0)),
                row(5, 0, Some(0.0)),
                row(5, 1, Some(0.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![
                segment(crate::feature::FeatureSegmentKind::Line, 10, [1, 2]),
                segment(crate::feature::FeatureSegmentKind::Point, 12, [3, 3]),
            ],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_point_on_line_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId("creo:featdefs:sketch_entity#40:12".into(),)),
            entity: SketchEntityId("creo:featdefs:sketch_entity#40:10".into()),
        }
    );

    let mut reference_line_definition = definition.clone();
    let segments = reference_line_definition
        .segments
        .as_mut()
        .expect("segments");
    segments
        .rows
        .retain(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point);
    segments.reference_line_rows = vec![crate::feature::FeatureReferenceLineSegment {
        directions: [None; 3],
        point_ids: [Some(1), Some(2)],
        vertical_horizontal: None,
        external_id: 10,
        offset: 10,
    }];
    let reference_line_constraints =
        section_equation_point_on_line_constraints(&reference_line_definition, &sketch);
    assert_eq!(reference_line_constraints.len(), 1);
    assert_eq!(
        reference_line_constraints[0].0.definition,
        constraints[0].0.definition
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_point_on_line_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_three_emits_parameterized_coordinate_distance() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let dimension = crate::feature::FeatureDimension {
        dimension_type: 1,
        value: Some(10.0),
        value_body: Vec::new(),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: None,
        auxiliary_body: Vec::new(),
        external_id: 27,
        references: None,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x03\xf8\x03\x00\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0)),
                variable(1, 2, Some(10.0)),
                variable(0, 0, Some(10.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 10,
                body: Vec::new(),
                offset: 10,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![dimension],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_unsigned_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::End(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            parameter: ParameterId("creo:featdefs:parameter#40:27".into()),
        }
    );

    let mut vertical = definition.clone();
    let variables = vertical.variables.as_mut().expect("variables");
    variables.rows[0].variable_type = 2;
    variables.rows[1].variable_type = 2;
    assert!(matches!(
        section_equation_unsigned_distance_constraints(&vertical, &sketch)[0]
            .0
            .definition,
        SketchConstraintDefinition::VerticalDistance { .. }
    ));

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_unsigned_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_forty_three_emits_parameterized_axis_distance() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let definition = |second: [f64; 2], dimension_value| crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x2b\xf8\x08\x00\x01\x02\x03\x04\x05\x06\x07\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 8,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0)),
                variable(2, 1, Some(0.0)),
                variable(1, 2, Some(second[0])),
                variable(2, 2, Some(second[1])),
                variable(4, 0, Some(0.0)),
                variable(5, 0, Some(0.0)),
                variable(0, 0, Some(10.0)),
                variable(5, 1, Some(0.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 10,
                body: Vec::new(),
                offset: 10,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureDimension {
                dimension_type: 1,
                value: Some(dimension_value),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 27,
                references: None,
                offset: 0,
            }],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let horizontal =
        section_equation_axis_distance_constraints(&definition([10.0, 0.0], 10.0), &sketch);
    assert_eq!(horizontal.len(), 1);
    assert_eq!(horizontal[0].0.active, Some(true));
    assert_eq!(
        horizontal[0].0.definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into())),
            second: SketchLocus::End(SketchEntityId("creo:featdefs:sketch_entity#40:10".into())),
            parameter: ParameterId("creo:featdefs:parameter#40:27".into()),
        }
    );

    let vertical =
        section_equation_axis_distance_constraints(&definition([0.0, 10.0], 10.0), &sketch);
    assert!(matches!(
        vertical.first().map(|constraint| &constraint.0.definition),
        Some(SketchConstraintDefinition::VerticalDistance { .. })
    ));

    let mut missing = definition([10.0, 0.0], 10.0);
    let distance = &mut missing.variables.as_mut().expect("variables").rows[6];
    distance.value = None;
    distance.guess = None;
    distance.guess_dimension_driven = true;
    distance.dimension_driven = true;
    assert_eq!(
        section_equation_axis_distance_constraints(&missing, &sketch).len(),
        1
    );

    assert!(
        section_equation_axis_distance_constraints(&definition([10.0, 0.0], 9.0), &sketch,)
            .is_empty()
    );

    let mut disabled = definition([10.0, 0.0], 10.0);
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_axis_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}
