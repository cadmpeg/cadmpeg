use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

#[test]
fn type406_form14_entity_table_boundary_follows_string_list() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::String(b"FLOW".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"FLOW".to_vec().into()),
                TokenValue::String(b"MOD".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"FLOW".to_vec().into()),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                TokenValue::Integer(7),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            3,
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
        let groups = analysis.groups().expect("Type 406 Form 14 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form14_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(2),
        TokenValue::String(b"FLOW".to_vec().into()),
        TokenValue::String(b"MOD".to_vec().into()),
        TokenValue::Integer(5),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
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
    let groups = analysis.groups().expect("Type 406 Form 14 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3; 5]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form14_malformed_count_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 14;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::real(1.0),
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::String(b"MOD".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"FLOW".to_vec().into()),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"FLOW".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
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
fn type406_form15_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for name in [
        TokenValue::String(b"USERNM".to_vec().into()),
        TokenValue::Integer(1),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                name,
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
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
        let groups = analysis.groups().expect("Type 406 Form 15 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form15_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(
            &token_parameter_record(1, values.clone()),
            ctx,
        )
        .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

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

#[test]
fn type406_form15_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"USERNM".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"USERNM".to_vec().into()),
        ],
        vec![TokenValue::Integer(406), TokenValue::Integer(1)],
    ] {
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        })
        .len();
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form24_entity_table_boundary_follows_definition_lists() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::String(b"TOP1".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(9),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::String(b"TOP1".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec().into()),
                TokenValue::Integer(20),
                TokenValue::String(b"CORE".to_vec().into()),
                TokenValue::Integer(0),
                TokenValue::String(b"UNDEFINED".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            11,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::String(b"TOP1".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
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
        let groups = analysis.groups().expect("Type 406 Form 24 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form24_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(5),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::String(b"TOP1".to_vec().into()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(
            &token_parameter_record(1, values.clone()),
            ctx,
        )
        .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

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

#[test]
fn type406_form24_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 24;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"TOP1".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::String(b"SIGNAL_T".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::String(b"TOP1".to_vec().into()),
            TokenValue::Integer(1),
        ],
    ] {
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        })
        .len();
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form25_entity_table_boundary_follows_level_lists() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_start) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::String(b"BOARD".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(10),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(5),
                TokenValue::String(b"BOARD".to_vec().into()),
                TokenValue::Integer(3),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(30),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::String(b"BOARD".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::String(b"1HX".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
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
        let groups = analysis.groups().expect("Type 406 Form 25 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form25_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(3),
        TokenValue::String(b"BOARD".to_vec().into()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(
            &token_parameter_record(1, values.clone()),
            ctx,
        )
        .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

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

#[test]
fn type406_form25_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 25;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"BOARD".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"BOARD".to_vec().into()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec().into()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
        ],
    ] {
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        })
        .len();
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form26_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::real(0.8),
            TokenValue::real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BAD".to_vec().into()),
            TokenValue::real(0.7),
            TokenValue::Integer(6),
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
        let groups = analysis.groups().expect("Type 406 Form 26 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form26_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(3),
        TokenValue::real(0.8),
        TokenValue::real(0.7),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(
            &token_parameter_record(1, values.clone()),
            ctx,
        )
        .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

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

#[test]
fn type406_form26_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 26;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::real(0.8),
            TokenValue::real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::real(0.8),
            TokenValue::real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::real(0.8),
            TokenValue::real(0.7),
        ],
    ] {
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        })
        .len();
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form28_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec().into()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Omitted,
            TokenValue::String(b"MM".to_vec().into()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(9),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec().into()),
            TokenValue::Integer(2),
            TokenValue::Integer(3),
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
        let groups = analysis.groups().expect("Type 406 Form 28 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form28_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(6),
        TokenValue::Integer(0),
        TokenValue::Integer(2),
        TokenValue::Integer(1),
        TokenValue::String(b"MM".to_vec().into()),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(
            &token_parameter_record(1, values.clone()),
            ctx,
        )
        .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

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

#[test]
fn type406_form28_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 28;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec().into()),
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
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec().into()),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::String(b"MM".to_vec().into()),
            TokenValue::Integer(0),
        ],
    ] {
        let generic_count = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        })
        .len();
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form29_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 29;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
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
            TokenValue::Omitted,
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
            TokenValue::String(b"2".to_vec().into()),
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
        let groups = analysis.groups().expect("Type 406 Form 29 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}
