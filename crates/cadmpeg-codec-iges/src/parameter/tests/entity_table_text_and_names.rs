use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::groups_for_candidate_with_context;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::ParameterRecord;
use crate::parameter::Token;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("text-score table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
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
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Form 34 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());

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
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Form 35 table boundary")
            .token_start,
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Form 30 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            associations
        );
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), properties);
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Form 30 table boundary")
            .token_start,
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
            let generic_valid_candidate_count =
                crate::test_support::with_service_context(&[], |ctx| {
                    structural_pointer_group_candidates_with_context(&record, ctx)
                        .expect("test-only pointer candidate allocation")
                })
                .iter()
                .filter_map(|candidate| {
                    crate::test_support::with_service_context(&[], |ctx| {
                        groups_for_candidate_with_context(&record, &directory, *candidate, ctx)
                            .expect("test-only trailing pointer allocation")
                    })
                })
                .filter_map(super::super::TrailingPointerGroups::fully_valid)
                .count();
            assert_eq!(generic_valid_candidate_count, 2);
        }
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Form 11 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Form 11 table boundary")
            .token_start,
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Form 12 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
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
    let generic_valid_candidate_count = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    })
    .iter()
    .filter_map(|candidate| {
        crate::test_support::with_service_context(&[], |ctx| {
            groups_for_candidate_with_context(&record, &directory, *candidate, ctx)
                .expect("test-only trailing pointer allocation")
        })
    })
    .filter_map(super::super::TrailingPointerGroups::fully_valid)
    .count();
    assert_eq!(generic_valid_candidate_count, 2);

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Form 12 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Form 27 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}
