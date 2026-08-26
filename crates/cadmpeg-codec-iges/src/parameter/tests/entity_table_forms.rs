use super::*;

#[test]
fn type212_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 212);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (5, &property),
        (7, &source),
    ]);
    let record =
        integer_parameter_record(7, &[212, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 3, 1, 5]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![13, 14]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 212 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type212_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 212);
    let directory = BTreeMap::from([(3, &association), (5, &property), (7, &source)]);
    let complete = |count: TokenValue, font: TokenValue, mirror: TokenValue| {
        vec![
            212.into(),
            count,
            1.into(),
            1.into(),
            1.into(),
            font,
            0.into(),
            0.into(),
            mirror,
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"A".to_vec()),
            1.into(),
            3.into(),
            1.into(),
            5.into(),
        ]
    };

    for (font, mirror) in [
        (TokenValue::String(b"bad".to_vec()), TokenValue::Integer(0)),
        (TokenValue::Integer(1), TokenValue::Integer(9)),
    ] {
        let analysis = analyze_trailing_pointer_groups(
            &token_parameter_record(7, complete(1.into(), font, mirror)),
            &directory,
        );
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        assert_eq!(
            analysis
                .groups
                .expect("Type 212 wrong-field boundary")
                .token_start,
            14
        );
    }

    for record in [
        token_parameter_record(7, complete(TokenValue::Real(1.5), 1.into(), 0.into())),
        token_parameter_record(7, complete(TokenValue::Omitted, 1.into(), 0.into())),
        token_parameter_record(7, complete(i64::MAX.into(), 1.into(), 0.into())),
        token_parameter_record(7, complete(1.into(), 1.into(), 0.into())[..4].to_vec()),
    ] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    let mut truncated_group = complete(1.into(), 1.into(), 0.into());
    truncated_group.pop();
    let analysis =
        analyze_trailing_pointer_groups(&token_parameter_record(7, truncated_group), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type213_form0_follows_string_count() {
    let prefix = |count: TokenValue| -> Vec<TokenValue> {
        vec![
            213.into(),
            1.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            count,
        ]
    };
    let text_block = |text: &[u8]| -> Vec<TokenValue> {
        vec![
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            TokenValue::String(Vec::new()),
            TokenValue::Integer(text.len() as i64),
            1.into(),
            1.into(),
            1.into(),
            std::f64::consts::FRAC_PI_2.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(text.to_vec()),
        ]
    };
    let association = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 213);
    let directory = BTreeMap::from([(3, &association), (5, &property), (7, &source)]);

    let cases = [
        (
            {
                let mut values = prefix(1.into());
                values.extend(text_block(b"A"));
                values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
                values
            },
            33_usize,
        ),
        (
            {
                let mut values = prefix(2.into());
                values.extend(text_block(b"A"));
                values.extend(text_block(b"BC"));
                values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
                values
            },
            53_usize,
        ),
    ];

    for (values, expected_start) in cases {
        let record = token_parameter_record(7, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 213 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert_eq!(groups.properties, vec![5]);
    }
}

#[test]
fn type213_table_boundary_precedes_valid_generic_alternative() {
    let prefix = vec![
        213.into(),
        1.into(),
        1.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        1.into(),
    ];
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 213);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (7, &source),
    ]);
    let mut values = prefix;
    values.extend([
        1.into(),
        1.into(),
        1.into(),
        1.into(),
        0.into(),
        1.into(),
        0.into(),
        TokenValue::String(Vec::new()),
        1.into(),
        1.into(),
        1.into(),
        1.into(),
        std::f64::consts::FRAC_PI_2.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        2.into(),
        1.into(),
        3.into(),
        1.into(),
        5.into(),
    ]);
    let record = token_parameter_record(7, values);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![32, 33]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 213 table boundary");
    assert_eq!(groups.token_start, 33);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type213_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let prefix = |count: TokenValue| -> Vec<TokenValue> {
        vec![
            213.into(),
            1.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            count,
        ]
    };
    let complete = |fixed: TokenValue, font: TokenValue| {
        let mut values = prefix(1.into());
        values.extend([
            fixed,
            1.into(),
            1.into(),
            1.into(),
            0.into(),
            font,
            0.into(),
            TokenValue::String(Vec::new()),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            std::f64::consts::FRAC_PI_2.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"A".to_vec()),
            1.into(),
            3.into(),
            1.into(),
            5.into(),
        ]);
        values
    };
    let association = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 213);
    let directory = BTreeMap::from([(3, &association), (5, &property), (7, &source)]);

    for values in [
        complete(9.into(), 1.into()),
        complete(1.into(), TokenValue::String(b"bad".to_vec())),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(7, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        assert_eq!(
            analysis
                .groups
                .expect("Type 213 wrong-field boundary")
                .token_start,
            33
        );
    }

    let text_block = || {
        vec![
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            TokenValue::String(Vec::new()),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            std::f64::consts::FRAC_PI_2.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"A".to_vec()),
        ]
    };
    let mut truncated_primary = prefix(1.into());
    truncated_primary.truncate(4);
    let mut truncated_block = prefix(1.into());
    truncated_block.extend(text_block().into_iter().take(19));
    let mut partial_final_block = prefix(2.into());
    partial_final_block.extend(text_block());
    partial_final_block.extend(text_block().into_iter().take(9));
    let mut truncated_group = complete(1.into(), 1.into());
    truncated_group.pop();
    truncated_group.pop();
    truncated_group.pop();
    for values in [
        {
            let mut values = prefix(TokenValue::Real(1.5));
            values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
            values
        },
        {
            let mut values = prefix(TokenValue::Omitted);
            values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
            values
        },
        {
            let mut values = prefix(0.into());
            values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
            values
        },
        {
            let mut values = prefix((-1_i64).into());
            values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
            values
        },
        {
            let mut values = prefix(i64::MAX.into());
            values.extend([1.into(), 3.into(), 1.into(), 5.into()]);
            values
        },
        truncated_primary,
        truncated_block,
        partial_final_block,
        truncated_group,
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(7, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type228_form0_entity_table_boundary_follows_geometry_and_leader_lists() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(17, 228);
    let directory = BTreeMap::from([(1, &association), (5, &property), (17, &source)]);

    for (values, expected_start) in [
        (vec![228, 9, 1, 7, 0, 1, 1, 1, 5], 5_usize),
        (vec![228, 9, 2, 7, 13, 1, 11, 1, 1, 1, 5], 7),
        (vec![228, 9, 1, 7, 2, 11, 15, 1, 1, 1, 5], 7),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(17, &values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 228 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert_eq!(groups.properties, vec![5]);
    }
}

#[test]
fn type228_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(17, 228);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (17, &source),
    ]);
    let record = integer_parameter_record(17, &[228, 9, 1, 7, 2, 11, 2, 1, 3, 1, 5]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![6, 7]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 228 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type228_malformed_counts_or_spans_do_not_enable_generic_recovery() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(17, 228);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (17, &source),
    ]);
    let malformed: Vec<Vec<TokenValue>> = vec![
        vec![
            228.into(),
            9.into(),
            0.into(),
            1.into(),
            11.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            228.into(),
            9.into(),
            TokenValue::Integer(-1),
            1.into(),
            11.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            228.into(),
            9.into(),
            TokenValue::Real(1.5),
            7.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            228.into(),
            9.into(),
            i64::MAX.into(),
            1.into(),
            11.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![228.into(), 9.into(), 2.into(), 7.into()],
        vec![
            228.into(),
            9.into(),
            1.into(),
            7.into(),
            TokenValue::Real(1.5),
            11.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            228.into(),
            9.into(),
            1.into(),
            7.into(),
            TokenValue::Integer(-1),
            11.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            228.into(),
            9.into(),
            1.into(),
            7.into(),
            2.into(),
            11.into(),
        ],
        vec![
            228.into(),
            9.into(),
            1.into(),
            7.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
    ];

    for values in malformed {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(17, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type410_entity_table_boundaries_follow_view_fields() {
    let cases = [
        (0_i64, vec![410, 1, 1, 0, 0, 0, 0, 0, 0, 1, 3, 0], 9_usize),
        (
            1_i64,
            vec![
                410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 5, 1, 3, 0,
            ],
            23,
        ),
    ];

    for (form, values, expected_start) in cases {
        let association = directory_target(3, 212);
        let mut source = directory_target(9, 410);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let record = integer_parameter_record(9, &values);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 410 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type410_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![410, 1, 1, 0, 0, 0, 0, 0, 2, 1, 3, 2, 5, 7],
            9_usize,
        ),
        (
            1_i64,
            vec![
                410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 2, 1, 3, 2,
                5, 7,
            ],
            23,
        ),
    ] {
        let mut source = directory_target(9, 410);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association_1),
            (3, &association_3),
            (5, &property_5),
            (7, &property_7),
            (9, &source),
        ]);
        let record = integer_parameter_record(9, &values);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 410 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert_eq!(groups.properties, vec![5, 7]);
    }
}

#[test]
fn type410_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 410);
    let directory = BTreeMap::from([(3, &association), (9, &source)]);

    let wrong_form0 = token_parameter_record(
        9,
        vec![
            410.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_form0, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 410 Form 0 boundary")
            .token_start,
        9
    );

    for values in [
        vec![410, 1, 1, 0, 0, 0, 0, 0],
        vec![410, 1, 1, 0, 0, 0, 0, 0, 0, 1, 3],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }

    source.form = 1;
    let directory = BTreeMap::from([(3, &association), (9, &source)]);
    let wrong_form1 = token_parameter_record(
        9,
        vec![
            410.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(1.5),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            10.into(),
            0.into(),
            1.into(),
            0.into(),
            5.into(),
            TokenValue::Real(-2.0),
            TokenValue::Real(2.0),
            TokenValue::Real(-1.0),
            TokenValue::Real(1.0),
            3.into(),
            TokenValue::Real(-5.0),
            TokenValue::Real(5.0),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_form1, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 410 Form 1 boundary")
            .token_start,
        23
    );

    for values in [
        vec![
            410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5,
        ],
        vec![
            410, 2, 1, 0, 0, 1, 0, 0, 0, 0, 0, 10, 0, 1, 0, 5, -2, 2, -1, 1, 3, -5, 5, 1, 3,
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type416_entity_table_boundaries_follow_external_reference_fields() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 416);
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"ONE".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
        ),
        (
            2_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
        ),
        (
            3_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
        ),
        (
            4_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"LIBRARY".to_vec()),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let record = token_parameter_record(9, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 416 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
    }
}

#[test]
fn type416_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(9, 416);

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                416.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            2,
        ),
        (
            2_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"FILE01".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3,
        ),
        (
            3_i64,
            vec![
                416.into(),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            2,
        ),
        (
            4_i64,
            vec![
                TokenValue::Integer(416),
                TokenValue::String(b"LIBRARY".to_vec()),
                2.into(),
                1.into(),
                3.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
            3,
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association_1),
            (3, &association_3),
            (5, &property_5),
            (7, &property_7),
            (9, &source),
        ]);
        let record = token_parameter_record(9, values);
        let generic = structural_pointer_group_candidates(&record);
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start != expected_start));
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 416 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert_eq!(groups.properties, vec![5, 7], "Form {form}");
    }
}

#[test]
fn type416_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(9, 416);

    for (form, wrong_values, expected_start, truncated_primary, truncated_group) in [
        (
            0_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                TokenValue::String(b"NAME".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3_usize,
            vec![416.into(), TokenValue::String(b"FILE01".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"NAME".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            1_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
            vec![416.into()],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            2_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
            vec![416.into(), TokenValue::String(b"FILE01".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                TokenValue::String(b"LOG".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            3_i64,
            vec![
                416.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            2,
            vec![416.into()],
            vec![
                416.into(),
                TokenValue::String(b"FILE01".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
        (
            4_i64,
            vec![
                416.into(),
                TokenValue::String(b"LIB".to_vec()),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                3.into(),
                0.into(),
            ],
            3,
            vec![416.into(), TokenValue::String(b"LIBRARY".to_vec())],
            vec![
                416.into(),
                TokenValue::String(b"LIBRARY".to_vec()),
                TokenValue::String(b"NAT".to_vec()),
                1.into(),
                3.into(),
            ],
        ),
    ] {
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (9, &source)]);
        let wrong = token_parameter_record(9, wrong_values);
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        assert_eq!(
            analysis
                .groups
                .expect("Type 416 complete boundary")
                .token_start,
            expected_start,
            "Form {form}"
        );

        for values in [truncated_primary, truncated_group] {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type420_entity_table_boundary_follows_connect_point_count() {
    let association = directory_target(1, 212);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([(1, &association), (9, &source)]);

    for (connect_count, expected_start) in [(0_i64, 12_usize), (1, 13), (2, 14)] {
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 420;
        values[11] = connect_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 1, "NC={connect_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "NC={connect_count}");
        let groups = analysis.groups.expect("Type 420 table boundary");
        assert_eq!(groups.token_start, expected_start, "NC={connect_count}");
        assert_eq!(groups.associations, vec![1], "NC={connect_count}");
        assert!(groups.properties.is_empty(), "NC={connect_count}");
    }
}

#[test]
fn type420_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let values = [420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0, 2, 1, 3, 2, 5, 7];
    let record = integer_parameter_record(9, &values);
    assert!(structural_pointer_group_candidates(&record)
        .iter()
        .any(|candidate| candidate.token_start == 13));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 420 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 7]);
}

#[test]
fn type420_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let source = directory_target(9, 420);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            420.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"BAD".to_vec()),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"R".to_vec()),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            2.into(),
            5.into(),
            7.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 420 complete primary boundary")
            .token_start,
        12
    );

    let malformed = [
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, -1, 1, 1, 2, 5, 7]),
        token_parameter_record(
            9,
            vec![
                420.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                TokenValue::String(b"BAD".to_vec()),
                1.into(),
                1.into(),
                2.into(),
                5.into(),
                7.into(),
            ],
        ),
        token_parameter_record(
            9,
            vec![
                420.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::String(b"R".to_vec()),
                0.into(),
            ],
        ),
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1]),
        integer_parameter_record(9, &[420, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 2, 5]),
    ];
    for record in malformed {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}
