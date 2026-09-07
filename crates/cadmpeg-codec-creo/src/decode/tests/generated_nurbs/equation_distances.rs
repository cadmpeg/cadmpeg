// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn equation_function_six_derives_positive_point_distance() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition = |radius| crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: None,
        },
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
                row(1, 10, Some(0.0)),
                row(2, 10, Some(0.0)),
                row(1, 11, Some(3.0)),
                row(2, 11, Some(4.0)),
                row(3, 20, radius),
            ],
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
    };

    assert_eq!(
        resolved_section_scalar_values(&definition(None)).get(&(3, 20)),
        Some(&5.0)
    );
    assert_eq!(
        resolved_section_radii(&definition(None)).get(&20),
        Some(&5.0)
    );
    assert!(!resolved_section_scalar_values(&definition(Some(6.0))).contains_key(&(3, 20)));
    assert_eq!(
        resolved_section_radii(&definition(Some(6.0))).get(&20),
        Some(&6.0)
    );

    let mut stored_without_coordinates = definition(Some(5.0));
    for row in &mut stored_without_coordinates
        .variables
        .as_mut()
        .expect("variables")
        .rows[..4]
    {
        row.value = None;
        row.guess = None;
    }
    assert!(!resolved_section_scalar_values(&stored_without_coordinates).contains_key(&(3, 20)));
}

#[test]
fn equation_function_forty_three_derives_unique_axis_distance_scalar() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: value.is_none(),
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: value.is_none(),
        offset: 0,
    };
    let definition =
        |first: [f64; 2], second: [f64; 2], distance| crate::feature::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(40),
                owner_feature_id: None,
            },
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
                    row(1, 10, Some(first[0])),
                    row(2, 10, Some(first[1])),
                    row(1, 11, Some(second[0])),
                    row(2, 11, Some(second[1])),
                    row(4, 2, Some(0.0)),
                    row(5, 0, Some(0.0)),
                    row(0, 20, distance),
                    row(5, 1, Some(0.0)),
                ],
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
        };

    assert_eq!(
        resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 0.0], None)).get(&(0, 20)),
        Some(&3.0)
    );
    assert_eq!(
        resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], Some(4.0)))
            .get(&(0, 20)),
        Some(&4.0)
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], None))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 4.0], Some(5.0)))
            .contains_key(&(0, 20))
    );
    assert!(
        !resolved_section_scalar_values(&definition([0.0, 0.0], [3.0, 3.0], Some(3.0)))
            .contains_key(&(0, 20))
    );

    let mut invalid_auxiliary = definition([0.0, 0.0], [3.0, 0.0], None);
    invalid_auxiliary
        .variables
        .as_mut()
        .expect("variables")
        .rows[5]
        .value = Some(1.0);
    assert!(!resolved_section_scalar_values(&invalid_auxiliary).contains_key(&(0, 20)));
}

#[test]
fn equation_function_three_solves_unique_unsigned_coordinate_distance() {
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
    let dimension = |value| crate::feature::FeatureDimension {
        dimension_type: 1,
        value: crate::feature::definitions::DimensionValue::Resolved(value),
        value_body: Vec::new(),
        direction_byte: 0,
        auxiliary_value: None,
        auxiliary_body: Vec::new(),
        external_id: 0,
        references: None,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: None,
        },
        body: b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x03\xf8\x03\x00\x01\x03\xf6\xe2\
                \x02\x03\xf8\x03\x01\x02\x04\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 5,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0), false),
                variable(1, 2, None, true),
                variable(1, 3, Some(10.0), false),
                variable(0, 0, Some(5.0), false),
                variable(0, 1, Some(5.0), false),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![dimension(5.0), dimension(5.0)],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&2),
        Some(&[Some(5.0), None])
    );

    let mut dimension_driven = definition.clone();
    let dimension_scalar = &mut dimension_driven.variables.as_mut().expect("variables").rows[3];
    dimension_scalar.value = None;
    dimension_scalar.guess = None;
    dimension_scalar.guess_dimension_driven = true;
    dimension_scalar.dimension_driven = true;
    assert_eq!(
        resolved_section_coordinates(&dimension_driven).get(&2),
        Some(&[Some(5.0), None])
    );
    assert_eq!(
        resolved_section_scalar_values(&dimension_driven).get(&(0, 0)),
        Some(&5.0)
    );

    let mut missing_inline = definition.clone();
    let missing_scalar = &mut missing_inline.variables.as_mut().expect("variables").rows[3];
    missing_scalar.value = None;
    missing_scalar.guess = None;
    assert!(!resolved_section_scalar_values(&missing_inline).contains_key(&(0, 0)));

    let equation_id = crate::feature::equation_table(&definition.body, 0, definition.body.len())
        .expect("equation table")
        .rows
        .iter()
        .find(|equation| equation.function_id == 3)
        .expect("function-three equation")
        .equation_id;
    let mut disabled_equation = definition.clone();
    disabled_equation.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
            header: crate::feature::definitions::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 901,
                offset: 900,
            },
            rows: vec![crate::feature::FeatureSkamp {
                id: 900,
                kind: 0,
                flags: 0,
                status: 0,
                items: Vec::new(),
                offset: 900,
            }],
        }),
        triples: Some(crate::feature::definitions::SolverSubtable::Declared {
            header: crate::feature::definitions::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 903,
                offset: 902,
            },
            rows: vec![crate::feature::FeatureRelationTriple {
                relation_id: None,
                equation_id: Some(equation_id),
                skamp_id: Some(900),
                offset: 902,
            }],
        }),
        offset: 899,
    });
    assert!(!resolved_section_coordinates(&disabled_equation).contains_key(&2));

    let mut mismatched = definition;
    mismatched
        .dimensions
        .as_mut()
        .expect("dimension table")
        .rows[0]
        .value = crate::feature::definitions::DimensionValue::Resolved(6.0);
    assert!(!resolved_section_coordinates(&mismatched).contains_key(&2));
}

#[test]
fn equation_function_thirty_three_solves_unique_equal_line_length_coordinate() {
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
    let definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(40),
            owner_feature_id: None,
        },
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
                row(1, 1, Some(0.0)),
                row(2, 1, Some(0.0)),
                row(1, 2, Some(0.0)),
                row(2, 2, Some(4.0)),
                row(1, 3, Some(0.0)),
                row(2, 3, Some(0.0)),
                row(1, 4, None),
                row(2, 4, Some(4.0)),
                row(7, 5, Some(0.0)),
            ],
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
    };

    assert_eq!(
        resolved_section_coordinates(&definition).get(&4),
        Some(&[Some(0.0), Some(4.0)])
    );

    let mut ambiguous = definition.clone();
    ambiguous.variables.as_mut().expect("variables").rows[7].value = Some(0.0);
    assert_eq!(
        resolved_section_coordinates(&ambiguous).get(&4),
        Some(&[None, Some(0.0)])
    );

    let mut nonzero_auxiliary = definition;
    nonzero_auxiliary
        .variables
        .as_mut()
        .expect("variables")
        .rows[8]
        .value = Some(1.0);
    assert_eq!(
        resolved_section_coordinates(&nonzero_auxiliary).get(&4),
        Some(&[None, Some(4.0)])
    );
}
