use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::ParameterRecord;
use crate::parameter::Token;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

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
        TokenValue::real(0.1),
        TokenValue::real(-0.1),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::Integer(3),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

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
    let groups = analysis.groups().expect("Type 406 Form 29 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            TokenValue::real(0.1),
            TokenValue::real(-0.1),
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
            TokenValue::real(0.1),
            TokenValue::real(-0.1),
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
            TokenValue::real(0.1),
            TokenValue::real(-0.1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(8),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::real(0.1),
            TokenValue::real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        })
        .len();
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
            0,
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
        let analysis_record = token_parameter_record(1, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &analysis_record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &analysis_record,
                entity_primary_end(&analysis_record, &directory)
            ),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 406 Form 31 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 8));

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
    let groups = analysis.groups().expect("Type 406 Form 31 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        })
        .len();
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
            0,
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
        let analysis_record = token_parameter_record(1, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &analysis_record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &analysis_record,
                entity_primary_end(&analysis_record, &directory)
            ),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 406 Form 36 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
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
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

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
    let groups = analysis.groups().expect("Type 406 Form 36 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3, 3]
    );
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
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
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        })
        .len();
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
            0,
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
            double_precision_reals: Vec::new(),
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
            "form={form}, N={item_count}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "form={form}, N={item_count}"
        );
        let groups = analysis.groups().expect("Type 184 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        double_precision_reals: Vec::new(),
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
    let groups = analysis.groups().expect("Type 184 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            double_precision_reals: Vec::new(),
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
            double_precision_reals: Vec::new(),
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
            "LC={list_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "LC={list_count}");
        let groups = analysis.groups().expect("Type 412 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        double_precision_reals: Vec::new(),
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
    let groups = analysis.groups().expect("Type 412 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            double_precision_reals: Vec::new(),
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
        double_precision_reals: Vec::new(),
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
            double_precision_reals: Vec::new(),
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
            "LC={list_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "LC={list_count}");
        let groups = analysis.groups().expect("Type 414 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        double_precision_reals: Vec::new(),
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
    let groups = analysis.groups().expect("Type 414 table boundary");
    assert_eq!(groups.token_start, 12);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            double_precision_reals: Vec::new(),
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
        double_precision_reals: Vec::new(),
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
            double_precision_reals: Vec::new(),
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
            "N={placement_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "N={placement_count}");
        let groups = analysis.groups().expect("Type 402 Form 5 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
        double_precision_reals: Vec::new(),
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
    let groups = analysis.groups().expect("Type 402 Form 5 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            double_precision_reals: Vec::new(),
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
        double_precision_reals: Vec::new(),
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
