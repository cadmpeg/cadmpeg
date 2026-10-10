use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::groups_for_candidate_with_context;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

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
        let valid_generic_starts = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        })
        .into_iter()
        .filter(|candidate| {
            crate::test_support::with_service_context(&[], |ctx| {
                groups_for_candidate_with_context(&record, &directory, *candidate, ctx)
                    .expect("test-only trailing pointer allocation")
            })
            .is_some_and(|groups| groups.fully_valid().is_some())
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
            "Type {}",
            source.entity_type
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "Type {}",
            source.entity_type
        );
        let groups = analysis
            .groups()
            .expect("count-defined annotation table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {}",
            source.entity_type
        );
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1, 3],
            "Type {}",
            source.entity_type
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5, 7],
            "Type {}",
            source.entity_type
        );
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
    for count in [TokenValue::real(0.0), TokenValue::Omitted, (-1_i64).into()] {
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
            "sequence {sequence}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "sequence {sequence}");
        assert!(analysis.groups().is_none(), "sequence {sequence}");
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
    wrong_field[7] = TokenValue::String(b"1HX".to_vec().into());

    for form in 1_i64..=12 {
        let mut source = directory_target(1, 214);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association)]);
        for (values, expected_start) in [
            (n1.clone(), 9_usize),
            (n2.clone(), 11_usize),
            (wrong_field.clone(), 9_usize),
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
                1,
                "Form {form}"
            );
            assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
            let groups = analysis.groups().expect("Type 214 table boundary");
            assert_eq!(groups.token_start, expected_start, "Form {form}");
            assert_eq!(
                groups.associations().copied().collect::<Vec<_>>(),
                vec![3, 3, 3],
                "Form {form}"
            );
            assert!(
                groups.properties().copied().collect::<Vec<_>>().is_empty(),
                "Form {form}"
            );
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
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert_eq!(
        generic
            .iter()
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>(),
        vec![6, 9]
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
    let groups = analysis.groups().expect("Type 214 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3, 3, 3]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
    wrong_type[1] = TokenValue::real(1.0);
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
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
    }

    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                TokenValue::Integer(218),
                TokenValue::Integer(3),
                TokenValue::real(5.0),
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
                TokenValue::real(7.0),
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
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis
            .groups()
            .expect("Type 218 boundary with invalid field");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3],
            "Form {form}"
        );
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
        let generic = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        });
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_start));
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == alternative_start));

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
        let groups = analysis.groups().expect("Type 218 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3; 6],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
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
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &integer_parameter_record(1, &values),
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(1, &values),
                entity_primary_end(&integer_parameter_record(1, &values), &directory)
            ),
            0,
            "Form {form}, values={values:?}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            0,
            "Form {form}, values={values:?}"
        );
        assert!(
            analysis.groups().is_none(),
            "Form {form}, values={values:?}"
        );
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
            TokenValue::String(b"5".to_vec().into()),
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
        let groups = analysis.groups().expect("Type 406 Form 1 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
    let groups = analysis.groups().expect("Type 406 Form 1 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5, 7]);
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
            TokenValue::real(1.0),
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
                TokenValue::String(b"MM".to_vec().into()),
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
                TokenValue::String(b"X".to_vec().into()),
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
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis
            .groups()
            .expect("Type 406 drawing property table boundary");
        assert_eq!(groups.token_start, 4, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
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
            crate::test_support::with_service_context(&[], |ctx| {
                structural_pointer_group_candidates_with_context(&record, ctx)
                    .expect("test-only pointer candidate allocation")
            })
            .iter()
            .any(|candidate| candidate.token_start == 3),
            "Form {form} generic candidate"
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
            0,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "Form {form}");
        assert!(analysis.groups().is_none(), "Form {form}");
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
                    TokenValue::real(2.0),
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
                    TokenValue::real(2.0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec().into()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Omitted,
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec().into()),
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(0),
                ],
                vec![
                    TokenValue::Integer(406),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::String(b"MM".to_vec().into()),
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
                "Form {form}"
            );
            assert_eq!(analysis.valid_candidate_count(), 0, "Form {form}");
            assert!(analysis.groups().is_none(), "Form {form}");
        }
    }
}
