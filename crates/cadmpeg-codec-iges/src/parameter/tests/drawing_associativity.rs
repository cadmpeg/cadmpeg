use super::*;

#[test]
fn type208_and_210_table_boundaries_precede_valid_generic_alternatives() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let source_208 = directory_target(9, 208);
    let source_210 = directory_target(11, 210);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (9, &source_208),
        (11, &source_210),
    ]);

    for (source, values, expected_start) in [
        (
            &source_208,
            vec![
                208.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            7_usize,
        ),
        (
            &source_210,
            vec![
                210.into(),
                1.into(),
                1.into(),
                3.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            4,
        ),
    ] {
        let record = token_parameter_record(source.sequence, values);
        let valid_generic_starts = structural_pointer_group_candidates(&record)
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert!(
            valid_generic_starts.contains(&expected_start),
            "Type {} generic starts {valid_generic_starts:?}",
            source.entity_type
        );
        assert!(
            valid_generic_starts.contains(&(expected_start + 1)),
            "Type {} generic starts {valid_generic_starts:?}",
            source.entity_type
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Type {}", source.entity_type);
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {}",
            source.entity_type
        );
        let groups = analysis
            .groups
            .expect("count-defined annotation table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {}",
            source.entity_type
        );
        assert_eq!(
            groups.associations,
            vec![1, 3],
            "Type {}",
            source.entity_type
        );
        assert_eq!(groups.properties, vec![5, 7], "Type {}", source.entity_type);
    }
}

#[test]
fn type208_and_210_malformed_leader_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source_208 = directory_target(9, 208);
    let source_210 = directory_target(11, 210);
    let directory = BTreeMap::from([
        (1, &association),
        (3, &property),
        (9, &source_208),
        (11, &source_210),
    ]);

    let mut cases = Vec::new();
    for count in [TokenValue::Real(0.0), TokenValue::Omitted, (-1_i64).into()] {
        let mut values: Vec<TokenValue> = vec![
            208.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            2.into(),
            1.into(),
            3.into(),
            1.into(),
            3.into(),
        ];
        values[6] = count;
        cases.push((source_208.sequence, values));
    }
    let mut truncated: Vec<TokenValue> = vec![210.into(), 1.into(), 1.into(), 3.into()];
    truncated.truncate(3);
    cases.push((source_210.sequence, truncated));
    for count in [0.into(), TokenValue::Omitted, (-1_i64).into()] {
        let mut values: Vec<TokenValue> = vec![
            210.into(),
            1.into(),
            1.into(),
            3.into(),
            1.into(),
            1.into(),
            3.into(),
        ];
        values[2] = count;
        cases.push((source_210.sequence, values));
    }

    for (sequence, values) in cases {
        let record = token_parameter_record(sequence, values);
        assert_eq!(
            entity_primary_end(&record, &directory),
            Some(record.tokens.len())
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "sequence {sequence}");
        assert_eq!(analysis.valid_candidate_count, 0, "sequence {sequence}");
        assert!(analysis.groups.is_none(), "sequence {sequence}");
    }
}

#[test]
fn type214_forms_share_count_driven_boundary() {
    let association = directory_target(3, 212);
    let n1 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let n2 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(2),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let mut wrong_field = n1.clone();
    wrong_field[7] = TokenValue::String(b"1HX".to_vec());

    for form in 1_i64..=12 {
        let mut source = directory_target(1, 214);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for (values, expected_start) in [
            (n1.clone(), 9_usize),
            (n2.clone(), 11_usize),
            (wrong_field.clone(), 9_usize),
        ] {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("Type 214 table boundary");
            assert_eq!(groups.token_start, expected_start, "Form {form}");
            assert_eq!(groups.associations, vec![3, 3, 3], "Form {form}");
            assert!(groups.properties.is_empty(), "Form {form}");
        }
    }
}

#[test]
fn type214_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 214);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(214),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(6),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(
        generic
            .iter()
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>(),
        vec![6, 9]
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 214 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations, vec![3, 3, 3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type214_malformed_count_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 214);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let n1 = vec![
        TokenValue::Integer(214),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(6),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let mut wrong_type = n1.clone();
    wrong_type[1] = TokenValue::Real(1.0);
    let mut omitted = n1.clone();
    omitted[1] = TokenValue::Omitted;
    let mut zero = n1.clone();
    zero[1] = TokenValue::Integer(0);
    let mut negative = n1.clone();
    negative[1] = TokenValue::Integer(-1);
    let mut overflowing = n1.clone();
    overflowing[1] = TokenValue::Integer(i64::MAX);
    let mut truncated = n1;
    truncated.truncate(9);

    for values in [wrong_type, omitted, zero, negative, overflowing, truncated] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type218_forms_share_fixed_primary_boundary() {
    let association = directory_target(3, 212);
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Real(5.0),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3_usize,
        ),
        (
            0,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
        ),
        (
            1,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Real(7.0),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            1,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::Integer(5),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis
            .groups
            .expect("Type 218 boundary with invalid field");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
    }
}

#[test]
fn type218_table_boundary_precedes_generic_candidates() {
    let association = directory_target(3, 212);
    for (form, values, expected_start, alternative_start) in [
        (
            0_i64,
            vec![218, 3, 5, 6, 3, 3, 3, 3, 3, 3, 0],
            3_usize,
            6_usize,
        ),
        (1, vec![218, 3, 5, 7, 6, 3, 3, 3, 3, 3, 3, 0], 4, 7),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let record = integer_parameter_record(1, &values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_start));
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == alternative_start));

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3; 6], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type218_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    for (form, values) in [
        (0_i64, vec![218, 3]),
        (0, vec![218, 3, 5, 1, 3]),
        (1, vec![218, 3, 5]),
        (1, vec![218, 3, 5, 7, 1, 3]),
    ] {
        let mut source = directory_target(1, 218);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(1, &values), &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "Form {form}, values={values:?}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "Form {form}, values={values:?}"
        );
        assert!(analysis.groups.is_none(), "Form {form}, values={values:?}");
    }
}

#[test]
fn type406_form1_entity_table_boundary_follows_level_list() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"5".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 1 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form1_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let property_a = directory_target(5, 406);
    let property_b = directory_target(7, 406);
    let directory = BTreeMap::from([
        (1, &source),
        (3, &association),
        (5, &property_a),
        (7, &property_b),
    ]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(2),
        TokenValue::Integer(5),
        TokenValue::Integer(7),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 1 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type406_form1_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 1;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Real(1.0),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Integer(5),
            TokenValue::Integer(6),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_drawing_properties_share_fixed_primary_boundary() {
    let association = directory_target(3, 212);
    for (form, values) in [
        (
            16_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            17_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::String(b"MM".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            16,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"X".to_vec()),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
        (
            17,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis
            .groups
            .expect("Type 406 drawing property table boundary");
        assert_eq!(groups.token_start, 4, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type406_drawing_property_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let property_a = directory_target(5, 406);
    let property_b = directory_target(7, 406);
    for (form, values) in [
        (
            16_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(2),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
            ],
        ),
        (
            17_i64,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(2),
                TokenValue::Integer(5),
                TokenValue::Integer(7),
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &source),
            (3, &association),
            (5, &property_a),
            (7, &property_b),
        ]);
        let record = token_parameter_record(1, values);
        assert!(
            structural_pointer_group_candidates(&record)
                .iter()
                .any(|candidate| candidate.token_start == 3),
            "Form {form} generic candidate"
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
        assert!(analysis.groups.is_none(), "Form {form}");
    }
}

#[test]
fn type406_drawing_property_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    for (form, cases) in [
        (
            16_i64,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Real(2.0),
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(0),
                    TokenValue::Integer(10),
                    TokenValue::Integer(20),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(2),
                    TokenValue::Integer(10),
                ],
            ],
        ),
        (
            17_i64,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Real(2.0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(2),
                    TokenValue::Integer(2),
                ],
            ],
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let record = token_parameter_record(1, values);
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type406_form6_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 6;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::Integer(1),
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 6 table boundary");
        assert_eq!(groups.token_start, 7);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form6_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 6;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![406, 5, 1, 2, 1, 2, 8, 6, 3, 3, 3, 3, 3, 3, 0];
    let record = integer_parameter_record(1, &values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));
    assert!(generic.iter().any(|candidate| candidate.token_start == 10));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 6 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations, vec![3; 6]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form6_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 6;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [vec![406, 5, 1, 2, 1, 2], vec![406, 5, 1, 2, 1, 2, 8, 1, 3]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(1, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type406_forms5_and7_entity_table_boundaries_follow_fixed_values() {
    let association = directory_target(3, 212);
    for (form, boundary, cases) in [
        (
            5_i64,
            7,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(5),
                    TokenValue::Real(1.5),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::Real(0.25),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(4),
                    TokenValue::Integer(1),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::Integer(0),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Real(1.5),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::Real(0.25),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(5),
                    TokenValue::Omitted,
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::Real(0.25),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
            ],
        ),
        (
            7,
            3,
            vec![
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(1),
                    TokenValue::String(b"REF".to_vec()),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(2),
                    TokenValue::String(b"REF".to_vec()),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::String(b"REF".to_vec()),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(1),
                    TokenValue::Integer(4),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("Type 406 fixed table boundary");
            assert_eq!(groups.token_start, boundary, "Form {form}");
            assert_eq!(groups.associations, vec![3; 3], "Form {form}");
            assert!(groups.properties.is_empty(), "Form {form}");
        }
    }
}

#[test]
fn type406_forms5_and7_table_boundaries_precede_generic_candidates() {
    let association = directory_target(3, 212);
    for (form, boundary, alternate, values) in [
        (5_i64, 7, 6, vec![406, 5, 1, 0, 2, 1, 4, 3, 3, 3, 3, 0]),
        (7, 3, 2, vec![406, 1, 4, 3, 3, 3, 3, 0]),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let record = integer_parameter_record(1, &values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == boundary),
            "Form {form} fixed candidate"
        );
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == alternate),
            "Form {form} generic candidate"
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 406 fixed table boundary");
        assert_eq!(groups.token_start, boundary, "Form {form}");
        assert_eq!(groups.associations, vec![3; 3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type406_forms5_and7_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    for (form, cases) in [
        (
            5_i64,
            vec![vec![406, 5, 1, 0, 2, 1], vec![406, 5, 1, 0, 2, 1, 0, 3, 3]],
        ),
        (7, vec![vec![406, 1, 1], vec![406, 1, 1, 3, 3]]),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&integer_parameter_record(1, &values), &directory);
            assert_eq!(
                analysis.candidate_count, 0,
                "Form {form}, values={values:?}"
            );
            assert_eq!(
                analysis.valid_candidate_count, 0,
                "Form {form}, values={values:?}"
            );
            assert!(analysis.groups.is_none(), "Form {form}, values={values:?}");
        }
    }
}

#[test]
fn type406_form19_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            406.into(),
            1.into(),
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into(), 0.into()],
        vec![
            406.into(),
            1.into(),
            TokenValue::Real(12.0),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![
            406.into(),
            2.into(),
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
        vec![
            406.into(),
            TokenValue::Omitted,
            12.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 19 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form19_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 12, 6, 3, 3, 3, 3, 3, 3, 0]);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 3));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 19 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3; 6]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form19_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![406.into(), 1.into(), 12.into()],
        vec![406.into(), 1.into(), 12.into(), 1.into(), 3.into()],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form4_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 4;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 2, 1, 0, 1, 3, 0]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 4 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_fixed_property_forms_follow_table_boundaries() {
    let association = directory_target(3, 212);
    for (form, boundary, cases) in [
        (
            18_i64,
            3,
            vec![
                vec![
                    406.into(),
                    1.into(),
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    2.into(),
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    25.0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            20,
            3,
            vec![
                vec![406.into(), 1.into(), 1.into(), 1.into(), 3.into(), 0.into()],
                vec![406.into(), 2.into(), 1.into(), 1.into(), 3.into(), 0.into()],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    1.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            21,
            3,
            vec![
                vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into(), 0.into()],
                vec![406.into(), 2.into(), 0.into(), 1.into(), 3.into(), 0.into()],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    0.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            22,
            11,
            vec![
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    8.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    TokenValue::Omitted,
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
        (
            23,
            4,
            vec![
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    1.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    TokenValue::Omitted,
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                    0.into(),
                ],
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::Omitted,
                    1.into(),
                    3.into(),
                    0.into(),
                ],
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("fixed property table boundary");
            assert_eq!(groups.token_start, boundary, "Form {form}");
            assert_eq!(groups.associations, vec![3]);
            assert!(groups.properties.is_empty(), "Form {form}");
        }
    }
}

#[test]
fn type406_fixed_property_table_precedes_generic_candidates() {
    let association = directory_target(3, 212);
    for (form, boundary, values, alternate) in [
        (18_i64, 3, vec![406, 1, 25, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (20, 3, vec![406, 1, 1, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (21, 3, vec![406, 1, 0, 6, 3, 3, 3, 3, 3, 3, 0], 6),
        (
            22,
            11,
            vec![406, 9, 1, 1, 1, 10, 20, 1, 2, 3, 4, 6, 3, 3, 3, 3, 3, 3, 0],
            14,
        ),
        (23, 4, vec![406, 2, 3, 4, 6, 3, 3, 3, 3, 3, 3, 0], 7),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        let record = integer_parameter_record(1, &values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == boundary),
            "Form {form} fixed candidate"
        );
        assert!(
            generic
                .iter()
                .any(|candidate| candidate.token_start == alternate),
            "Form {form} generic candidate"
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("fixed property table boundary");
        assert_eq!(groups.token_start, boundary, "Form {form}");
        assert_eq!(groups.associations, vec![3; 6], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type406_fixed_property_truncation_suppresses_generic_recovery() {
    let association = directory_target(3, 212);
    for (form, cases) in [
        (
            18_i64,
            vec![
                vec![406.into(), 1.into(), 25.0.into()],
                vec![406.into(), 1.into(), 25.0.into(), 1.into(), 3.into()],
            ],
        ),
        (
            20,
            vec![
                vec![406.into(), 1.into(), 1.into()],
                vec![406.into(), 1.into(), 1.into(), 1.into(), 3.into()],
            ],
        ),
        (
            21,
            vec![
                vec![406.into(), 1.into(), 0.into()],
                vec![406.into(), 1.into(), 0.into(), 1.into(), 3.into()],
            ],
        ),
        (
            22,
            vec![
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                ],
                vec![
                    406.into(),
                    9.into(),
                    1.into(),
                    1.into(),
                    1.into(),
                    10.0.into(),
                    20.0.into(),
                    1.5.into(),
                    2.5.into(),
                    3.into(),
                    4.into(),
                    1.into(),
                    3.into(),
                ],
            ],
        ),
        (
            23,
            vec![
                vec![406.into(), 2.into(), 3.into()],
                vec![
                    406.into(),
                    2.into(),
                    3.into(),
                    TokenValue::String(b"DIPS".to_vec()),
                    1.into(),
                    3.into(),
                ],
            ],
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for values in cases {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type406_form32_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::Integer(1),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::Integer(4),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 32 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}
