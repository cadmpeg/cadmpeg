use super::*;

#[test]
fn type406_form29_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(8),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(2),
        TokenValue::Real(0.1),
        TokenValue::Real(-0.1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 29 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form29_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
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
fn type406_form31_entity_table_boundary_follows_fixed_corners() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 31 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form31_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(8),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 31 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form31_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 31;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
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
fn type406_form36_entity_table_boundary_follows_np_arity() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::String(b"1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(1),
                TokenValue::Integer(5),
            ],
            4,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 36 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert_eq!(groups.properties, vec![5]);
    }
}

#[test]
fn type406_form36_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(5),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 36 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations, vec![3, 3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type406_form36_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 316);
    let mut source = directory_target(1, 406);
    source.form = 36;
    let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
    let cases = [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
        ],
    ];
    for values in cases {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type184_entity_table_boundary_follows_item_and_transform_lists() {
    for (form, item_count, expected_start) in [(0_i64, 1_i64, 4), (0, 2, 6), (1, 3, 8)] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 184);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let item_count = usize::try_from(item_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 184;
        values[1] = i64::try_from(item_count).unwrap();
        for index in 0..item_count {
            values[2 + index] = 1;
            values[2 + item_count + index] = 0;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "form={form}, N={item_count}");
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "form={form}, N={item_count}"
        );
        let groups = analysis.groups.expect("Type 184 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type184_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let target_7 = directory_target(7, 212);
    let source = directory_target(5, 184);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source), (7, &target_7)]);
    let values = [184, 2, 1, 3, 0, 2, 1, 7, 0];
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 184 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![7]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type184_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 184);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![184, 0, 1, 5, 1, 5, 0],
        vec![184, -1, 1, 5, 1, 5, 0],
        vec![184, 100, 1, 5, 1, 5, 0],
        vec![184],
        vec![184, 2, 1, 5, 0],
    ];

    for values in cases {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type412_entity_table_boundary_follows_do_dont_list() {
    for (list_count, expected_start) in [(0_i64, 13_usize), (1, 14), (2, 15)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 412);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let list_count = usize::try_from(list_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 412;
        values[1] = 1;
        values[2] = 1;
        values[6] = 2;
        values[7] = 2;
        values[8] = 1;
        values[9] = 1;
        values[11] = i64::try_from(list_count).unwrap();
        values[12] = 0;
        for index in 0..list_count {
            values[13 + index] = i64::try_from(index + 1).unwrap();
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "LC={list_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "LC={list_count}");
        let groups = analysis.groups.expect("Type 412 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type412_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 412);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 1, 0, 2, 1, 1, 0];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 412 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type412_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 412);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, -1, 0, 1, 5, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 100, 0, 1, 5, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0],
        vec![412, 1, 1, 0, 0, 0, 2, 2, 1, 1, 0, 2, 0, 1],
    ];

    for values in cases {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let mut values = (0..16)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(412);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(2);
    values[7].value = TokenValue::Integer(2);
    values[8].value = TokenValue::Integer(1);
    values[9].value = TokenValue::Integer(1);
    values[11].value = TokenValue::String(b"1".to_vec());
    values[13].value = TokenValue::Integer(1);
    values[14].value = TokenValue::Integer(1);
    values[15].value = TokenValue::Integer(5);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type414_entity_table_boundary_follows_do_dont_list() {
    for (list_count, expected_start) in [(0_i64, 11_usize), (1, 12), (2, 13)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 414);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let list_count = usize::try_from(list_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 414;
        values[1] = 1;
        values[2] = 4;
        values[6] = 8;
        values[7] = 1;
        values[8] = 1;
        values[9] = i64::try_from(list_count).unwrap();
        values[10] = 0;
        for index in 0..list_count {
            values[11 + index] = i64::try_from(index + 1).unwrap();
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "LC={list_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "LC={list_count}");
        let groups = analysis.groups.expect("Type 414 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type414_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 414);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [414, 1, 4, 0, 0, 0, 8, 1, 1, 1, 0, 2, 1, 1, 0];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 414 table boundary");
    assert_eq!(groups.token_start, 12);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type414_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 414);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, -1, 0, 1, 5, 0],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, 100, 0, 1, 5, 0],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1],
        vec![414, 1, 4, 0, 0, 0, 8, 1, 1, 2, 0, 1],
    ];

    for values in cases {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let mut values = (0..14)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(414);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::Integer(4);
    values[6].value = TokenValue::Integer(8);
    values[7].value = TokenValue::Integer(1);
    values[8].value = TokenValue::Integer(1);
    values[9].value = TokenValue::String(b"1".to_vec());
    values[11].value = TokenValue::Integer(1);
    values[12].value = TokenValue::Integer(1);
    values[13].value = TokenValue::Integer(5);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type402_form5_entity_table_boundary_follows_label_placements() {
    for (placement_count, expected_start) in [(1_i64, 9_usize), (2, 16)] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 5;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let placement_count = usize::try_from(placement_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = i64::try_from(placement_count).unwrap();
        for index in 0..placement_count {
            let start = 2 + index * 7;
            values[start] = 1;
            values[start + 4] = 1;
            values[start + 6] = 1;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "N={placement_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N={placement_count}");
        let groups = analysis.groups.expect("Type 402 Form 5 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type402_form5_entity_table_boundary_precedes_valid_generic_alternative() {
    let mut generic_target = directory_target(1, 402);
    generic_target.form = 7;
    let association = directory_target(9, 212);
    let mut source = directory_target(3, 402);
    source.form = 5;
    let directory = BTreeMap::from([(1, &generic_target), (3, &source), (9, &association)]);
    let values = [402, 1, 5, 0, 0, 0, 7, 0, 2, 1, 9, 0];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 402 Form 5 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations, vec![9]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type402_form5_malformed_counts_do_not_enable_generic_recovery() {
    let target = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 5;
    let directory = BTreeMap::from([(1, &target), (3, &source)]);
    let cases = [
        vec![402, 0, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402, -1, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402, 1000, 1, 0, 0, 0, 5, 0, 7, 0, 1, 1, 0],
        vec![402],
        vec![402, 2, 1, 0, 0, 0, 5, 0, 7, 1, 1, 0],
    ];

    for values in cases {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let mut values = (0..13)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(402);
    values[1].value = TokenValue::String(b"1".to_vec());
    values[2].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(5);
    values[8].value = TokenValue::Integer(7);
    values[10].value = TokenValue::Integer(1);
    values[11].value = TokenValue::Integer(1);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type406_form34_and_form35_entity_table_boundary_follows_text_score_ranges() {
    let association = directory_target(1, 212);
    let cases = [
        (34, vec![406, 4, 1, 1, 2, 4, 1, 1, 0], 6),
        (34, vec![406, 7, 2, 1, 2, 4, 2, 1, 3, 1, 1, 0], 9),
        (35, vec![406, 4, 1, 1, 2, 4, 1, 1, 0], 6),
        (35, vec![406, 7, 2, 1, 2, 4, 2, 1, 3, 1, 1, 0], 9),
    ];
    for (form, values, expected_start) in cases {
        let mut source = directory_target(3, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("text-score table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }

    let mut source = directory_target(3, 406);
    source.form = 34;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [406, 4, 1, 1, 1, 2, 1, 1, 0];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Form 34 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![1]);
}

#[test]
fn type406_form34_and_form35_malformed_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 406);
    source.form = 34;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = [
        vec![406, 1, 0, 1, 1, 0],
        vec![406, 1, -1, 1, 1, 0],
        vec![406, 5, 1, 1, 1, 1, 1, 0],
        vec![406, 4, 1, 1, 1],
        vec![406],
    ];
    for values in cases {
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(406),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(4),
                span: 0..0,
            },
            Token {
                value: TokenValue::String(b"1".to_vec()),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        parameter_end: 6,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());

    let mut source = directory_target(3, 406);
    source.form = 35;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let values = [
        Token {
            value: TokenValue::Integer(406),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(4),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(1),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(1),
            span: 0..0,
        },
        Token {
            value: TokenValue::String(b"1".to_vec()),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(2),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(1),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(1),
            span: 0..0,
        },
        Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        },
    ];
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values.into_iter().collect(),
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 35 table boundary").token_start,
        6
    );
}

#[test]
fn type406_form30_entity_table_boundary_follows_fixed_np_and_note_count() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let make_record = |values: Vec<TokenValue>| {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        }
    };
    let cases = [
        (
            vec![406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 0, 0, 1, 5],
            14,
            Vec::new(),
            vec![5],
        ),
        (
            vec![
                406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1, 1, 1, 1, 1, 5,
            ],
            17,
            vec![1],
            vec![5],
        ),
    ];
    for (values, expected_start, associations, properties) in cases {
        let record = make_record(
            values
                .into_iter()
                .map(TokenValue::Integer)
                .collect::<Vec<_>>(),
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 30 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, associations);
        assert_eq!(groups.properties, properties);
    }
}

#[test]
fn type406_form30_complete_counted_span_keeps_boundary_with_wrong_note_type() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(14),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(12),
        TokenValue::Integer(1),
        TokenValue::String(b"bad".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(5),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 30 table boundary").token_start,
        17
    );
}

#[test]
fn type406_form30_malformed_np_or_note_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(5, 316);
    let mut source = directory_target(3, 406);
    source.form = 30;
    let directory = BTreeMap::from([(1, &association), (3, &source), (5, &units)]);
    let integers = |values: &[i64]| {
        values
            .iter()
            .copied()
            .map(TokenValue::Integer)
            .collect::<Vec<_>>()
    };
    let cases = vec![
        integers(&[406, 15, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 0, 0, 1, 5]),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, -1, 0, 1, 5]),
        integers(&[
            406, 15, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1, 1, 1, 3, 1, 5,
        ]),
        vec![
            integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12]),
            vec![TokenValue::String(b"1".to_vec())],
            integers(&[0, 1, 5]),
        ]
        .into_iter()
        .flatten()
        .collect(),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12, 1, 1, 1]),
        integers(&[406, 14, 0, 1, 1, 3, 0, 0, 1, 0, 0, 0, 12]),
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form11_entity_table_boundary_follows_nested_value_counts() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(5),
                TokenValue::Integer(2),
                TokenValue::Integer(0),
                TokenValue::Integer(33),
                TokenValue::Integer(46),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(18),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(2),
                TokenValue::Integer(3),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(100),
                TokenValue::Integer(200),
                TokenValue::Integer(300),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(2),
                TokenValue::Integer(3),
                TokenValue::Integer(4),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            20,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        if expected_start == 20 {
            let generic_valid_candidate_count = structural_pointer_group_candidates(&record)
                .iter()
                .filter_map(|candidate| groups_for_candidate(&record, &directory, *candidate))
                .filter(|groups| groups.fully_valid)
                .count();
            assert_eq!(generic_valid_candidate_count, 2);
        }
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 11 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}

#[test]
fn type406_form11_complete_nested_span_keeps_boundary_with_invalid_value() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(4),
        TokenValue::Integer(5),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::String(b"bad".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis.groups.expect("Form 11 table boundary").token_start,
        6
    );
}

#[test]
fn type406_form11_malformed_nested_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(5),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(33),
            TokenValue::Integer(46),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(5),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(5),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(33),
            TokenValue::Integer(46),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(9),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(10),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(33),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form12_entity_table_boundary_follows_name_count() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::String(b"BASE.IGS".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"BASE.IGS".to_vec()),
                TokenValue::String(b"DETAIL.IGS".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            4,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 12 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}

#[test]
fn type406_form12_table_boundary_beats_generic_alternatives() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let tokens = [
        TokenValue::Integer(406),
        TokenValue::Integer(2),
        TokenValue::String(b"BASE.IGS".to_vec()),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
    ]
    .into_iter()
    .map(|value| Token { value, span: 0..0 })
    .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
    };
    let generic_valid_candidate_count = structural_pointer_group_candidates(&record)
        .iter()
        .filter_map(|candidate| groups_for_candidate(&record, &directory, *candidate))
        .filter(|groups| groups.fully_valid)
        .count();
    assert_eq!(generic_valid_candidate_count, 2);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Form 12 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form12_malformed_count_or_name_list_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 12;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        vec![TokenValue::Integer(406)],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"BASE.IGS".to_vec()),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"BASE.IGS".to_vec()),
            TokenValue::String(b"EXTRA.IGS".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form27_entity_table_boundary_follows_np_and_value_pair_count() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(4),
                TokenValue::String(b"PROPTEST".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(17),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            6,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(6),
                TokenValue::String(b"PROPTEST".to_vec()),
                TokenValue::Integer(2),
                TokenValue::Integer(1),
                TokenValue::Integer(17),
                TokenValue::Integer(3),
                TokenValue::String(b"HELLO".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
            ],
            8,
        ),
    ];
    for (values, expected_start) in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
        };
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Form 27 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![3]);
    }
}
