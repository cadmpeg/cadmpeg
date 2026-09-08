use super::*;

#[test]
fn type406_form27_complete_counted_span_keeps_boundary_with_invalid_value_type() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(4),
        TokenValue::String(b"PROPTEST".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(7),
        TokenValue::Integer(17),
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Form 27 table boundary")
            .token_start,
        6
    );
}

#[test]
fn type406_form27_malformed_np_or_value_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
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
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form6_entity_table_boundary_follows_view_list() {
    for (visible_count, expected_start) in [(0_i64, 4_usize), (1, 5), (2, 6)] {
        let association = directory_target(1, 212);
        let view = directory_target(3, 410);
        let mut source = directory_target(5, 402);
        source.form = 6;
        let visible_1 = directory_target(7, 212);
        let visible_2 = directory_target(9, 212);
        let directory = BTreeMap::from([
            (1, &association),
            (3, &view),
            (5, &source),
            (7, &visible_1),
            (9, &visible_2),
        ]);
        let visible_count = usize::try_from(visible_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2] = i64::try_from(visible_count).unwrap();
        values[3] = 3;
        for (offset, sequence) in [7_i64, 9].into_iter().take(visible_count).enumerate() {
            values[4 + offset] = sequence;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
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
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "N1={visible_count}");
        assert_eq!(analysis.valid_candidate_count(), 1, "N1={visible_count}");
        let groups = analysis.groups().expect("Type 402 Form 6 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form6_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source)]);
    let values = [402, 1, 1, 3, 2, 1, 1, 0];
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 6 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form6_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let visible = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source), (7, &visible)]);
    let cases = [
        vec![402, 0, 1, 3, 7, 1, 1, 0],
        vec![402, 1, -1, 3, 7, 1, 1, 0],
        vec![402, 1, 1000, 3, 7, 1, 1, 0],
        vec![402, 1],
        vec![402, 1, 2, 3, 7],
    ];

    for values in cases {
        let parameter_end = values.len();
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
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }

    let mut values = (0..8)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(402);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::String(b"1".to_vec());
    values[3].value = TokenValue::Integer(3);
    values[4].value = TokenValue::Integer(7);
    values[5].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(1);
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values,
        parameter_end: 8,
        comment: Vec::new(),
    };
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 0);
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type402_form16_entity_table_boundary_follows_entity_count() {
    for (entity_count, expected_start) in [(1_i64, 5_usize), (2, 6)] {
        let association = directory_target(1, 212);
        let transform = directory_target(3, 124);
        let entity_1 = directory_target(7, 116);
        let entity_2 = directory_target(9, 116);
        let mut source = directory_target(5, 402);
        source.form = 16;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &transform),
            (5, &source),
            (7, &entity_1),
            (9, &entity_2),
        ]);
        let entity_count = usize::try_from(entity_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2] = i64::try_from(entity_count).unwrap();
        values[3] = 3;
        for (offset, sequence) in [7_i64, 9].into_iter().take(entity_count).enumerate() {
            values[4 + offset] = sequence;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: values.len(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "N={entity_count}");
        assert_eq!(analysis.valid_candidate_count(), 1, "N={entity_count}");
        let groups = analysis.groups().expect("Type 402 Form 16 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form16_accepts_explicit_empty_pointer_groups() {
    let transform = directory_target(3, 124);
    let member = directory_target(341, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(3, &transform), (5, &source), (341, &member)]);
    let record = integer_parameter_record(5, &[402, 1, 1, 3, 341, 0, 0]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis
        .groups()
        .expect("Type 402 Form 16 empty pointer groups");
    assert_eq!(groups.token_start, 5);
    assert!(groups
        .associations()
        .copied()
        .collect::<Vec<_>>()
        .is_empty());
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    assert!(groups.fully_valid());
}

#[test]
fn type402_form16_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let transform = directory_target(3, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(1, &association), (3, &transform), (5, &source)]);
    let values = [402, 1, 1, 3, 2, 1, 1, 0];
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        comment: Vec::new(),
    };
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 16 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form16_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let transform = directory_target(3, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(1, &association), (3, &transform), (5, &source)]);
    let cases = [
        vec![
            402.into(),
            0.into(),
            1.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            (-1).into(),
            1.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1).into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            i64::MAX.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            TokenValue::Real(1.0),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![402.into(), 1.into()],
        vec![402.into(), 1.into(), 1.into(), 3.into()],
        vec![402.into(), 1.into(), 1.into(), 3.into(), 7.into(), 1.into()],
    ];

    for values in cases {
        let record = token_parameter_record(5, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_view_visibility_forms_follow_counted_view_blocks() {
    let cases = [
        (3, vec![402.into(), 1.into(), 0.into(), 3.into()], 4),
        (
            3,
            vec![402.into(), 1.into(), TokenValue::Omitted, 3.into()],
            4,
        ),
        (
            3,
            vec![402.into(), 1.into(), 2.into(), 3.into(), 5.into(), 7.into()],
            6,
        ),
        (
            4,
            vec![
                402.into(),
                1.into(),
                0.into(),
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
            ],
            8,
        ),
        (
            4,
            vec![
                402.into(),
                1.into(),
                TokenValue::Omitted,
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
            ],
            8,
        ),
        (
            4,
            vec![
                402.into(),
                2.into(),
                1.into(),
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
                5.into(),
                7.into(),
                1.into(),
                0.into(),
                7.into(),
            ],
            14,
        ),
    ];
    for (form, values, expected_end) in cases {
        let mut source = directory_target(9, 402);
        source.form = form;
        let directory = BTreeMap::from([(9, &source)]);
        let record = token_parameter_record(9, values);
        assert_eq!(entity_primary_end(&record, &directory), Some(expected_end));
    }
}

#[test]
fn type402_view_visibility_entity_count_requirement_follows_dialect() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(9, 402);
    source.form = 3;
    let directory = BTreeMap::from([(1, &association), (3, &view), (9, &source)]);
    let omitted_count = token_parameter_record(
        9,
        vec![
            402.into(),
            1.into(),
            TokenValue::Omitted,
            3.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
    );

    assert_eq!(
        entity_primary_end_for_global_table(&omitted_count, &directory, GlobalTable::V4_0),
        Some(omitted_count.tokens.len())
    );
    let v4_analysis = analyze_trailing_pointer_groups_for_global_table(
        &omitted_count,
        &directory,
        GlobalTable::V4_0,
    );
    assert!(v4_analysis.groups().is_none());

    for global_table in [GlobalTable::V5_0, GlobalTable::V5Later] {
        assert_eq!(
            entity_primary_end_for_global_table(&omitted_count, &directory, global_table),
            Some(4),
            "global_table={global_table:?}"
        );
        let analysis = analyze_trailing_pointer_groups_for_global_table(
            &omitted_count,
            &directory,
            global_table,
        );
        let groups = analysis
            .groups()
            .expect("optional Type 402 visible-entity list");
        assert_eq!(groups.token_start, 4, "global_table={global_table:?}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "global_table={global_table:?}"
        );
    }

    let explicit_zero = token_parameter_record(
        9,
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
    );
    assert_eq!(
        entity_primary_end_for_global_table(&explicit_zero, &directory, GlobalTable::V4_0),
        Some(4)
    );
    let analysis = analyze_trailing_pointer_groups_for_global_table(
        &explicit_zero,
        &directory,
        GlobalTable::V4_0,
    );
    let groups = analysis
        .groups()
        .expect("explicit V4 Type 402 entity count");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn type402_view_visibility_malformed_counts_do_not_enable_generic_recovery() {
    let mut source = directory_target(9, 402);
    source.form = 4;
    let directory = BTreeMap::from([(9, &source)]);
    for values in [
        vec![
            402.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1_i64).into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
        vec![402.into(), 1.into(), 1.into(), 1.into(), 1.into()],
    ] {
        let record = token_parameter_record(9, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_external_reference_index_entity_table_boundary_follows_entry_pairs() {
    for form in [2, 12] {
        for (entry_count, expected_start) in [(1_usize, 4_usize), (2, 6)] {
            let internal_1 = directory_target(1, 116);
            let internal_2 = directory_target(5, 110);
            let association = directory_target(7, 212);
            let mut source = directory_target(3, 402);
            source.form = form;
            let directory = BTreeMap::from([
                (1, &internal_1),
                (3, &source),
                (5, &internal_2),
                (7, &association),
            ]);
            let mut tokens = (0..expected_start + 3)
                .map(|_| Token {
                    value: TokenValue::Integer(0),
                    span: 0..0,
                })
                .collect::<Vec<_>>();
            tokens[0].value = TokenValue::Integer(402);
            tokens[1].value = TokenValue::Integer(i64::try_from(entry_count).unwrap());
            for (offset, sequence) in [1_i64, 5].into_iter().take(entry_count).enumerate() {
                let start = 2 + offset * 2;
                tokens[start].value = TokenValue::String(format!("REF{}", offset + 1).into_bytes());
                tokens[start + 1].value = TokenValue::Integer(sequence);
            }
            tokens[expected_start].value = TokenValue::Integer(1);
            tokens[expected_start + 1].value = TokenValue::Integer(7);
            tokens[expected_start + 2].value = TokenValue::Integer(0);
            let parameter_end = tokens.len();
            let record = ParameterRecord {
                directory_sequence: 3,
                line_range: 1..2,
                bytes: Vec::new(),
                tokens,
                parameter_end,
                comment: Vec::new(),
            };

            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count(), 1, "N={entry_count}");
            assert_eq!(analysis.valid_candidate_count(), 1, "N={entry_count}");
            let groups = analysis
                .groups()
                .expect("Type 402 external-reference table boundary");
            assert_eq!(groups.token_start, expected_start);
            assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
            assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
        }
    }
}

#[test]
fn type402_external_reference_index_malformed_counts_or_pairs_do_not_enable_generic_recovery() {
    let cases = [
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(0),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(-1),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(i64::MAX),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::String(b"1".to_vec()),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
    ];

    for form in [2, 12] {
        let target = directory_target(1, 116);
        let association = directory_target(7, 212);
        let mut source = directory_target(3, 402);
        source.form = form;
        let directory = BTreeMap::from([(1, &target), (3, &source), (7, &association)]);
        for values in cases.iter().cloned() {
            let tokens = values
                .into_iter()
                .map(|value| Token { value, span: 0..0 })
                .collect::<Vec<_>>();
            let parameter_end = tokens.len();
            let record = ParameterRecord {
                directory_sequence: 3,
                line_range: 1..2,
                bytes: Vec::new(),
                tokens,
                parameter_end,
                comment: Vec::new(),
            };

            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count(), 0);
            assert_eq!(analysis.valid_candidate_count(), 0);
            assert!(analysis.groups().is_none());
        }
    }
}

#[test]
fn type402_form13_entity_table_boundary_follows_geometry_list() {
    for (geometry_count, expected_start) in [(1_usize, 5_usize), (2, 6)] {
        let dimension = directory_target(1, 216);
        let geometry_1 = directory_target(5, 116);
        let geometry_2 = directory_target(7, 110);
        let association = directory_target(9, 212);
        let mut source = directory_target(3, 402);
        source.form = 13;
        let directory = BTreeMap::from([
            (1, &dimension),
            (3, &source),
            (5, &geometry_1),
            (7, &geometry_2),
            (9, &association),
        ]);
        let mut tokens = (0..expected_start + 3)
            .map(|_| Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            })
            .collect::<Vec<_>>();
        tokens[0].value = TokenValue::Integer(402);
        tokens[1].value = TokenValue::Integer(1);
        tokens[2].value = TokenValue::Integer(i64::try_from(geometry_count).unwrap());
        tokens[3].value = TokenValue::Integer(1);
        tokens[4].value = TokenValue::Integer(5);
        if geometry_count == 2 {
            tokens[5].value = TokenValue::Integer(7);
        }
        tokens[expected_start].value = TokenValue::Integer(1);
        tokens[expected_start + 1].value = TokenValue::Integer(9);
        tokens[expected_start + 2].value = TokenValue::Integer(0);
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "NG={geometry_count}");
        assert_eq!(analysis.valid_candidate_count(), 1, "NG={geometry_count}");
        let groups = analysis.groups().expect("Type 402 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form13_malformed_fields_do_not_enable_generic_recovery() {
    let dimension = directory_target(1, 216);
    let geometry = directory_target(5, 116);
    let association = directory_target(9, 212);
    let mut source = directory_target(3, 402);
    source.form = 13;
    let directory = BTreeMap::from([
        (1, &dimension),
        (3, &source),
        (5, &geometry),
        (9, &association),
    ]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form21_entity_table_boundary_follows_geometry_blocks() {
    for (geometry_count, expected_start) in [(1_usize, 11_usize), (2, 16)] {
        let association = directory_target(1, 212);
        let dimension = directory_target(5, 216);
        let geometry_1 = directory_target(7, 116);
        let geometry_2 = directory_target(9, 110);
        let mut source = directory_target(3, 402);
        source.form = 21;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &source),
            (5, &dimension),
            (7, &geometry_1),
            (9, &geometry_2),
        ]);
        let mut values = vec![TokenValue::Integer(0); expected_start + 3];
        values[0] = 402.into();
        values[1] = 1.into();
        values[2] = i64::try_from(geometry_count).unwrap().into();
        values[3] = 5.into();
        values[4] = 4.into();
        values[5] = TokenValue::Real(0.25);
        for (offset, sequence) in [7_i64, 9].into_iter().take(geometry_count).enumerate() {
            let start = 6 + offset * 5;
            values[start] = sequence.into();
            values[start + 1] = 0.into();
            values[start + 2] = TokenValue::Real(offset as f64);
            values[start + 3] = TokenValue::Real(1.0);
            values[start + 4] = TokenValue::Real(2.0);
        }
        values[expected_start] = 1.into();
        values[expected_start + 1] = 1.into();
        values[expected_start + 2] = 0.into();

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(3, values), &directory);
        assert_eq!(analysis.candidate_count(), 1, "NG={geometry_count}");
        assert_eq!(analysis.valid_candidate_count(), 1, "NG={geometry_count}");
        let groups = analysis.groups().expect("Type 402 Form 21 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form21_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let dimension = directory_target(3, 216);
    let geometry = directory_target(7, 116);
    let mut source = directory_target(5, 402);
    source.form = 21;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &dimension),
        (5, &source),
        (7, &geometry),
    ]);
    let values = vec![
        402.into(),
        1.into(),
        1.into(),
        3.into(),
        4.into(),
        TokenValue::Real(0.25),
        7.into(),
        0.into(),
        TokenValue::Real(0.0),
        TokenValue::Real(1.0),
        2.into(),
        1.into(),
        1.into(),
        0.into(),
    ];
    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(5, values), &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 21 table boundary");
    assert_eq!(groups.token_start, 11);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form21_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let dimension = directory_target(3, 216);
    let geometry = directory_target(7, 116);
    let mut source = directory_target(5, 402);
    source.form = 21;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &dimension),
        (5, &source),
        (7, &geometry),
    ]);
    let cases = vec![
        vec![
            402.into(),
            0.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1_i64).into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            TokenValue::String(b"1".to_vec()),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![402.into(), 1.into()],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
        ],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
        ],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::Real(0.25),
            7.into(),
            0.into(),
            TokenValue::Real(0.0),
            TokenValue::Real(1.0),
            TokenValue::Real(2.0),
            1.into(),
        ],
    ];

    for values in cases {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(5, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type408_fixed_primary_boundary_follows_translation_and_scale() {
    let association = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([(3, &association), (7, &source), (9, &definition)]);
    let record = token_parameter_record(
        7,
        vec![
            408.into(),
            9.into(),
            1.into(),
            2.into(),
            3.into(),
            TokenValue::Real(0.5),
            1.into(),
            3.into(),
            0.into(),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type408_fixed_primary_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (7, &source),
        (9, &definition),
    ]);
    let record = integer_parameter_record(7, &[408, 9, 1, 2, 3, 2, 1, 3, 6, 5, 5, 5, 5, 5, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5; 6]);
}

#[test]
fn type408_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (7, &source),
        (9, &definition),
    ]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            408.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(2.0),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            TokenValue::Omitted,
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 408 table boundary")
            .token_start,
        6
    );

    for values in [vec![408, 9, 1, 2, 3], vec![408, 9, 1, 2, 3, 1, 1, 3]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form19_entity_table_boundary_follows_segment_blocks() {
    for (block_count, expected_start) in [(1_i64, 8_usize), (2, 14)] {
        let association = directory_target(3, 212);
        let mut source = directory_target(11, 402);
        source.form = 19;
        let directory = BTreeMap::from([(3, &association), (11, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = block_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 3;
        values[expected_start + 2] = 0;

        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count(), 1, "block_count={block_count}");
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "block_count={block_count}"
        );
        let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form19_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[402, 1, 9, 0, 0, 0, 0, 2, 1, 3, 2, 5, 7]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5, 7]);
}

#[test]
fn type402_form19_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([(3, &association), (11, &source)]);

    let wrong_fields = token_parameter_record(
        11,
        vec![
            402.into(),
            1.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::Real(0.5),
            0.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            2.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());

    let malformed = vec![
        token_parameter_record(
            11,
            vec![
                402.into(),
                0.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                (-1_i64).into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                i64::MAX.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                TokenValue::Real(1.0),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
            ],
        ),
    ];
    for record in malformed {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form18_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 10_usize),
        (vec![1, 1, 1, 1, 1, 1], 16),
        (vec![2, 1, 1, 1, 1, 1], 17),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 18;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 2;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
        values[9] = 2;
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
        assert_eq!(analysis.candidate_count(), 1, "counts={counts:?}");
        assert_eq!(analysis.valid_candidate_count(), 1, "counts={counts:?}");
        let groups = analysis.groups().expect("Type 402 Form 18 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form18_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 18;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(2)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
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
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form20_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 9_usize),
        (vec![1, 1, 1, 1, 1, 1], 15),
        (vec![2, 1, 1, 1, 1, 1], 16),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 20;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
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
        assert_eq!(analysis.candidate_count(), 1, "counts={counts:?}");
        assert_eq!(analysis.valid_candidate_count(), 1, "counts={counts:?}");
        let groups = analysis.groups().expect("Type 402 Form 20 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form20_entity_table_boundary_beats_target_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut source = directory_target(5, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [402, 1, 0, 0, 0, 0, 0, 1, 1, 2, 1, 3, 0];
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 20 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form20_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}
