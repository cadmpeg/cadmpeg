use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

#[test]
fn type406_form32_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
            TokenValue::Integer(4),
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
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

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
    let groups = analysis.groups().expect("Type 406 Form 32 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3, 3, 3]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form32_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 32;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::real(3.0),
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
            TokenValue::String(b"20260714.123456".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
            TokenValue::String(b"20260714.123456".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
            TokenValue::String(b"20260714.123456".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
            TokenValue::String(b"20260714.123456".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"JANE".to_vec().into()),
            TokenValue::String(b"ENG".to_vec().into()),
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
fn type406_form33_entity_table_boundary_follows_fixed_values() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"NO".to_vec().into()),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Omitted,
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
        let groups = analysis.groups().expect("Type 406 Form 33 table boundary");
        assert_eq!(groups.token_start, 4);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form33_table_boundary_precedes_generic_candidate() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(5),
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
    let groups = analysis.groups().expect("Type 406 Form 33 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3; 5]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form33_malformed_np_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(1, 406);
    source.form = 33;
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::real(2.0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec().into()),
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
fn type406_form2_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);

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
    let groups = analysis.groups().expect("Type 406 Form 2 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form2_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 1, 3, 0]);

    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].token_start, 4);

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
fn type406_form2_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![406, 2, 0, 1, 1, 3, 0],
        vec![406, 4, 0, 1, 2, 3, 0],
        vec![406, 3, 0, 1],
    ] {
        let record = integer_parameter_record(1, &values);
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
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);
    record.tokens[1].value = TokenValue::real(3.0);
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
fn type406_form3_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
            TokenValue::String(b"POWER".to_vec().into()),
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
    let groups = analysis.groups().expect("Type 406 Form 3 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type406_form3_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 2, 1, 3, 0]);

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
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type406_form3_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::String(b"POWER".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form8_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for pin_number in [
        TokenValue::String(b"PA7".to_vec().into()),
        TokenValue::Integer(17),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(1),
                pin_number,
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
        let groups = analysis.groups().expect("Type 406 Form 8 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form8_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 1, 3, 0]);

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
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type406_form8_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PA7".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form9_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for first_number in [
        TokenValue::String(b"GENERIC".to_vec().into()),
        TokenValue::Integer(1),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(4),
                first_number,
                TokenValue::String(b"MIL123".to_vec().into()),
                TokenValue::String(b"VEND42".to_vec().into()),
                TokenValue::String(b"INT99".to_vec().into()),
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
        let groups = analysis.groups().expect("Type 406 Form 9 table boundary");
        assert_eq!(groups.token_start, 6);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form9_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"GENERIC".to_vec().into()),
            TokenValue::String(b"MIL123".to_vec().into()),
            TokenValue::String(b"VEND42".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

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
fn type406_form9_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"GENERIC".to_vec().into()),
            TokenValue::String(b"MIL123".to_vec().into()),
            TokenValue::String(b"VEND42".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"GENERIC".to_vec().into()),
            TokenValue::String(b"MIL123".to_vec().into()),
            TokenValue::String(b"VEND42".to_vec().into()),
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form10_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for first_value in [
        TokenValue::Integer(1),
        TokenValue::Integer(2),
        TokenValue::String(b"1".to_vec().into()),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(6),
                first_value,
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
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
        let groups = analysis.groups().expect("Type 406 Form 10 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form10_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = token_parameter_record(
        1,
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

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
fn type406_form10_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 10;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
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
            "generic_count={generic_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type406_form13_entity_table_boundary_follows_conditional_values() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (np, values, expected_start) in [
        (
            2,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::real(2.5),
                TokenValue::String(b"AWG".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
        (
            3,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(3),
                TokenValue::real(2.5),
                TokenValue::String(b"AWG".to_vec().into()),
                TokenValue::String(b"ANSI123".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
        (
            2,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::String(b"2HNO".to_vec().into()),
                TokenValue::String(b"AWG".to_vec().into()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        assert_eq!(values[1], TokenValue::Integer(np));
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
        let groups = analysis.groups().expect("Type 406 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type406_form13_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::real(2.5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::real(2.5),
            TokenValue::String(b"AWG".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let generic = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(
                &token_parameter_record(1, values.clone()),
                ctx,
            )
            .expect("test-only pointer candidate allocation")
        });
        let expected_generic_start = if values[1] == TokenValue::Integer(2) {
            3
        } else {
            4
        };
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_generic_start));

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
fn type406_form13_malformed_np_or_span_does_not_enable_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 13;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for values in [
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::real(2.5),
            TokenValue::String(b"AWG".to_vec().into()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::real(2.5),
            TokenValue::String(b"AWG".to_vec().into()),
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
