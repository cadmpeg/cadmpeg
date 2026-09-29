use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

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
            TokenValue::real(1.0),
            TokenValue::real(2.0),
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
            TokenValue::real(2.0),
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
            TokenValue::real(1.0),
            TokenValue::real(2.0),
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
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(8),
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
        let groups = analysis.groups().expect("Type 406 Form 6 table boundary");
        assert_eq!(groups.token_start, 7);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));
    assert!(generic.iter().any(|candidate| candidate.token_start == 10));

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
    let groups = analysis.groups().expect("Type 406 Form 6 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3; 6]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form6_truncated_primary_or_group_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 6;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [vec![406, 5, 1, 2, 1, 2], vec![406, 5, 1, 2, 1, 2, 8, 1, 3]] {
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
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
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
                    TokenValue::real(1.5),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::real(0.25),
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
                    TokenValue::real(1.5),
                    TokenValue::Integer(0),
                    TokenValue::Integer(2),
                    TokenValue::Integer(1),
                    TokenValue::real(0.25),
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
                    TokenValue::real(0.25),
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
            let groups = analysis.groups().expect("Type 406 fixed table boundary");
            assert_eq!(groups.token_start, boundary, "Form {form}");
            assert_eq!(
                groups.associations().copied().collect::<Vec<_>>(),
                vec![3; 3],
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
        let generic = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        });
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
        let groups = analysis.groups().expect("Type 406 fixed table boundary");
        assert_eq!(groups.token_start, boundary, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3; 3],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
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
            TokenValue::real(12.0),
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
        let groups = analysis.groups().expect("Type 406 Form 19 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form19_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 19;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 12, 6, 3, 3, 3, 3, 3, 3, 0]);
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 3));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

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
    let groups = analysis.groups().expect("Type 406 Form 19 table boundary");
    assert_eq!(groups.token_start, 3);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3; 6]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
fn type406_form4_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 4;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 2, 1, 0, 1, 3, 0]);

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
    let groups = analysis.groups().expect("Type 406 Form 4 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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
            let groups = analysis.groups().expect("fixed property table boundary");
            assert_eq!(groups.token_start, boundary, "Form {form}");
            assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
            assert!(
                groups.properties().copied().collect::<Vec<_>>().is_empty(),
                "Form {form}"
            );
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
        let generic = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        });
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
        let groups = analysis.groups().expect("fixed property table boundary");
        assert_eq!(groups.token_start, boundary, "Form {form}");
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
                0,
                "Form {form}"
            );
            assert_eq!(analysis.valid_candidate_count(), 0, "Form {form}");
            assert!(analysis.groups().is_none(), "Form {form}");
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
        let groups = analysis.groups().expect("Type 406 Form 32 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}
