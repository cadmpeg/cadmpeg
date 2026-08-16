// SPDX-License-Identifier: Apache-2.0
//! Tests for scalar equality propagation into equation consumers.

use crate::decode::sketch::{
    resolved_section_coordinates, section_equation_coordinate_equality_rows,
    section_equation_equal_length_constraint_rows,
    section_equation_function_forty_three_axis_distance_values,
    section_equation_function_sixteen_angle_difference_values,
    section_equation_point_on_line_constraint_rows, section_equation_scalar_equalities,
    section_equation_scalar_equality_components,
};
use std::collections::BTreeSet;

fn row(variable_type: u32, key: u32, value: Option<f64>) -> crate::feature::FeatureVariableRow {
    crate::feature::FeatureVariableRow {
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
    }
}

fn definition(
    body: &[u8],
    rows: Vec<crate::feature::FeatureVariableRow>,
) -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: body.to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: u32::try_from(rows.len()).expect("variable count"),
            entity_ref: None,
            rows,
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

fn equation_body(rows: &[(u8, u8, &[u8])]) -> Vec<u8> {
    let mut body = b"eqtn_arr\0\xf2\xf8".to_vec();
    body.push(u8::try_from(rows.len() + 1).expect("equation count"));
    body.extend_from_slice(b"\xf7\x80\x9f\xfb\xe2\xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2");
    for &(equation_id, function_id, arguments) in rows {
        body.extend_from_slice(&[
            equation_id,
            function_id,
            0xf8,
            u8::try_from(arguments.len()).expect("argument count"),
        ]);
        body.extend_from_slice(arguments);
        body.extend_from_slice(b"\xf6\xe2");
    }
    body
}

fn axis_distance_values(definition: &crate::feature::FeatureDefinition) -> Vec<((u32, u32), f64)> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .expect("variables")
        .reconciled_points()
        .1;
    section_equation_function_forty_three_axis_distance_values(
        definition,
        &resolved_section_coordinates(definition),
        &ambiguous_point_ids,
    )
}

#[test]
fn function_forty_three_reconciles_scalar_equality_consumers() {
    let body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x2b\xf8\x08\x00\x01\x02\x03\x04\x05\x06\x07\xf6\xe2\
            \x02\x02\xf8\x02\x06\x08\xf6\xe2";
    let rows = vec![
        row(1, 10, Some(0.0)),
        row(2, 10, Some(0.0)),
        row(1, 11, Some(3.0)),
        row(2, 11, Some(4.0)),
        row(4, 2, Some(0.0)),
        row(5, 0, Some(0.0)),
        row(0, 20, None),
        row(5, 1, Some(0.0)),
        row(0, 21, Some(4.0)),
    ];
    let propagated = definition(body, rows);
    assert_eq!(axis_distance_values(&propagated), vec![((0, 20), 4.0)]);

    let mut conflicting = propagated.clone();
    conflicting.variables.as_mut().expect("variables").rows[6].value = Some(5.0);
    assert!(axis_distance_values(&conflicting).is_empty());

    let invalid_auxiliary_body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x2b\xf8\x08\x00\x01\x02\x03\x04\x05\x06\x07\xf6\xe2\
            \x02\x02\xf8\x02\x06\x08\xf6\xe2\
            \x03\x02\xf8\x02\x07\x09\xf6\xe2";
    let invalid_auxiliary = definition(
        invalid_auxiliary_body,
        vec![
            row(1, 10, Some(0.0)),
            row(2, 10, Some(0.0)),
            row(1, 11, Some(3.0)),
            row(2, 11, Some(4.0)),
            row(4, 2, Some(0.0)),
            row(5, 0, Some(0.0)),
            row(0, 20, None),
            row(5, 1, None),
            row(0, 21, Some(4.0)),
            row(5, 22, Some(1.0)),
        ],
    );
    assert!(axis_distance_values(&invalid_auxiliary).is_empty());
}

#[test]
fn function_sixteen_reconciles_scalar_equality_consumers() {
    let body = b"eqtn_arr\0\xf2\xf8\x06\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
            \x01\x10\xf8\x04\x00\x01\x02\x03\xf6\xe2\
            \x02\x02\xf8\x02\x00\x04\xf6\xe2\
            \x03\x02\xf8\x02\x01\x05\xf6\xe2\
            \x04\x02\xf8\x02\x02\x06\xf6\xe2\
            \x05\x02\xf8\x02\x03\x07\xf6\xe2";
    let propagated = definition(
        body,
        vec![
            row(4, 10, None),
            row(4, 11, None),
            row(0, 20, None),
            row(5, 0, None),
            row(4, 12, Some(2.5)),
            row(4, 13, Some(1.0)),
            row(0, 21, Some(1.5)),
            row(5, 22, Some(0.0)),
        ],
    );
    assert_eq!(
        section_equation_function_sixteen_angle_difference_values(&propagated),
        vec![((0, 20), 1.5)]
    );

    let mut conflicting = propagated.clone();
    let variables = conflicting.variables.as_mut().expect("variables");
    variables.rows[0].value = Some(2.5);
    variables.rows[4].value = Some(3.0);
    assert!(section_equation_function_sixteen_angle_difference_values(&conflicting).is_empty());

    let mut invalid_selector = propagated;
    invalid_selector.variables.as_mut().expect("variables").rows[7].value = Some(1.0);
    assert!(
        section_equation_function_sixteen_angle_difference_values(&invalid_selector).is_empty()
    );
}

#[test]
fn zero_sentinel_equations_reconcile_scalar_equalities() {
    let function_thirteen = definition(
        &equation_body(&[(1, 13, &[0, 1, 2]), (2, 2, &[2, 3])]),
        vec![
            row(2, 10, Some(4.5)),
            row(2, 11, None),
            row(7, 20, None),
            row(7, 21, Some(0.0)),
        ],
    );
    assert_eq!(
        section_equation_coordinate_equality_rows(&function_thirteen, &BTreeSet::new()).len(),
        1
    );

    let mut conflicting_thirteen = function_thirteen;
    conflicting_thirteen
        .variables
        .as_mut()
        .expect("variables")
        .rows[2]
        .value = Some(1.0);
    assert!(
        section_equation_coordinate_equality_rows(&conflicting_thirteen, &BTreeSet::new())
            .is_empty()
    );

    let function_thirty_three = definition(
        &equation_body(&[(1, 33, &[0, 1, 2, 3, 4, 5, 6, 7, 8]), (2, 2, &[8, 9])]),
        vec![
            row(1, 1, Some(0.0)),
            row(2, 1, Some(0.0)),
            row(1, 2, Some(0.0)),
            row(2, 2, Some(4.0)),
            row(1, 3, Some(0.0)),
            row(2, 3, Some(0.0)),
            row(1, 4, Some(0.0)),
            row(2, 4, Some(4.0)),
            row(7, 30, None),
            row(7, 31, Some(0.0)),
        ],
    );
    assert_eq!(
        section_equation_equal_length_constraint_rows(&function_thirty_three, &BTreeSet::new())
            .len(),
        1
    );

    let mut conflicting_thirty_three = function_thirty_three;
    conflicting_thirty_three
        .variables
        .as_mut()
        .expect("variables")
        .rows[8]
        .value = Some(1.0);
    assert!(section_equation_equal_length_constraint_rows(
        &conflicting_thirty_three,
        &BTreeSet::new()
    )
    .is_empty());

    let function_thirty_five = definition(
        &equation_body(&[
            (1, 35, &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
            (2, 2, &[7, 9]),
            (3, 2, &[8, 9]),
        ]),
        vec![
            row(1, 20, None),
            row(2, 20, Some(165.0)),
            row(1, 18, Some(0.0)),
            row(2, 18, Some(0.0)),
            row(1, 19, Some(0.0)),
            row(2, 19, Some(-100.0)),
            row(4, 2, None),
            row(5, 40, None),
            row(5, 41, None),
            row(5, 42, Some(0.0)),
        ],
    );
    assert_eq!(
        section_equation_point_on_line_constraint_rows(&function_thirty_five, &BTreeSet::new())
            .len(),
        1
    );

    let mut conflicting_thirty_five = function_thirty_five;
    conflicting_thirty_five
        .variables
        .as_mut()
        .expect("variables")
        .rows[7]
        .value = Some(1.0);
    assert!(section_equation_point_on_line_constraint_rows(
        &conflicting_thirty_five,
        &BTreeSet::new()
    )
    .is_empty());
}

#[test]
fn function_five_accepts_a_zero_selector_proved_by_scalar_equality() {
    let definition = definition(
        &equation_body(&[(1, 5, &[0, 1, 2]), (2, 2, &[2, 3])]),
        vec![
            row(6, 10, None),
            row(6, 11, Some(2.0)),
            row(5, 20, None),
            row(5, 21, Some(0.0)),
        ],
    );
    let components = section_equation_scalar_equality_components(&definition);
    assert!(components
        .iter()
        .any(|component| { component == &BTreeSet::from([(6, 10), (6, 11)]) }));
    assert_eq!(
        section_equation_scalar_equalities(&definition).get(&(6, 10)),
        Some(&2.0)
    );

    let mut conflicting_selector = definition;
    conflicting_selector
        .variables
        .as_mut()
        .expect("variables")
        .rows[2]
        .value = Some(1.0);
    assert!(!section_equation_scalar_equalities(&conflicting_selector).contains_key(&(6, 10)));
}
