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
fn analytic_surface_forms_follow_fixed_primary_boundaries() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (entity_type, form, primary, expected_start) in [
        (190_i64, 0_i64, vec![190, 5, 7], 3_usize),
        (190, 1, vec![190, 5, 7, 9], 4),
        (192, 0, vec![192, 5, 7, 2], 4),
        (192, 1, vec![192, 5, 7, 2, 9], 5),
        (194, 0, vec![194, 5, 7, 2, 30], 5),
        (194, 1, vec![194, 5, 7, 2, 30, 9], 6),
        (196, 0, vec![196, 5, 2], 3),
        (196, 1, vec![196, 5, 2, 7, 9], 5),
        (198, 0, vec![198, 5, 7, 4, 1], 5),
        (198, 1, vec![198, 5, 7, 4, 1, 9], 6),
    ] {
        let mut source = directory_target(7, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
        let mut values = primary;
        values.extend([1, 1, 1, 3]);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &integer_parameter_record(7, &values),
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "Type {entity_type} Form {form}"
        );
        let groups = analysis.groups().expect("analytic surface table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![3],
            "Type {entity_type} Form {form}"
        );
    }
}

#[test]
fn analytic_surface_table_boundaries_precede_valid_generic_alternatives() {
    let association_1 = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (entity_type, form, primary, expected_start) in [
        (190_i64, 0_i64, vec![190, 5, 7], 3_usize),
        (190, 1, vec![190, 5, 7, 9], 4),
        (192, 0, vec![192, 5, 7, 2], 4),
        (192, 1, vec![192, 5, 7, 2, 9], 5),
        (194, 0, vec![194, 5, 7, 2, 30], 5),
        (194, 1, vec![194, 5, 7, 2, 30, 9], 6),
        (196, 0, vec![196, 5, 2], 3),
        (196, 1, vec![196, 5, 2, 7, 9], 5),
        (198, 0, vec![198, 5, 7, 4, 1], 5),
        (198, 1, vec![198, 5, 7, 4, 1, 9], 6),
    ] {
        let mut source = directory_target(7, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association_1), (3, &property), (7, &source)]);
        let mut values = primary;
        values.extend([2, 1, 1, 1, 3]);
        let record = integer_parameter_record(7, &values);
        let valid_starts = crate::test_support::with_service_context(&[], |ctx| {
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
        assert_eq!(valid_starts, vec![expected_start, expected_start + 1]);

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
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "Type {entity_type} Form {form}"
        );
        let groups = analysis.groups().expect("analytic surface table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1, 1],
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![3],
            "Type {entity_type} Form {form}"
        );
    }
}

#[test]
fn analytic_surface_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (entity_type, form, primary, expected_start) in [
        (190_i64, 0_i64, vec![190, 5, 7], 3_usize),
        (190, 1, vec![190, 5, 7, 9], 4),
        (192, 0, vec![192, 5, 7, 2], 4),
        (192, 1, vec![192, 5, 7, 2, 9], 5),
        (194, 0, vec![194, 5, 7, 2, 30], 5),
        (194, 1, vec![194, 5, 7, 2, 30, 9], 6),
        (196, 0, vec![196, 5, 2], 3),
        (196, 1, vec![196, 5, 2, 7, 9], 5),
        (198, 0, vec![198, 5, 7, 4, 1], 5),
        (198, 1, vec![198, 5, 7, 4, 1, 9], 6),
    ] {
        let mut source = directory_target(7, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);

        let mut wrong = primary
            .iter()
            .copied()
            .map(TokenValue::from)
            .collect::<Vec<_>>();
        *wrong.last_mut().expect("primary field") = TokenValue::String(b"bad".to_vec().into());
        wrong.extend([1.into(), 1.into(), 1.into(), 3.into()]);
        let analysis_record = token_parameter_record(7, wrong);
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
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis
                .groups()
                .expect("analytic surface wrong-field boundary")
                .token_start,
            expected_start,
            "Type {entity_type} Form {form}"
        );

        for values in [primary[..primary.len() - 1].to_vec(), {
            let mut incomplete = primary;
            incomplete.extend([1, 1, 1]);
            incomplete
        }] {
            let analysis = crate::test_support::with_service_context(&[], |ctx| {
                analyze_trailing_pointer_groups_for_global_table_with_context(
                    &integer_parameter_record(7, &values),
                    &directory,
                    crate::global::GlobalTable::V5Later,
                    ctx,
                )
                .expect("test-only trailing pointer analysis")
            });
            assert_eq!(
                analysis.candidate_count(
                    &integer_parameter_record(7, &values),
                    entity_primary_end(&integer_parameter_record(7, &values), &directory)
                ),
                0,
                "Type {entity_type} Form {form}"
            );
            assert_eq!(
                analysis.valid_candidate_count(),
                0,
                "Type {entity_type} Form {form}"
            );
            assert!(
                analysis.groups().is_none(),
                "Type {entity_type} Form {form}"
            );
        }
    }
}

#[test]
fn type304_forms_use_fixed_and_counted_boundaries() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (form, values, expected_start) in [
        (
            1_i64,
            vec![
                304.into(),
                1.into(),
                9.into(),
                2.into(),
                TokenValue::real(0.5),
                1.into(),
                1.into(),
                0.into(),
            ],
            5_usize,
        ),
        (
            2,
            vec![
                304.into(),
                2.into(),
                2.into(),
                1.into(),
                TokenValue::String(b"3".to_vec().into()),
                1.into(),
                1.into(),
                0.into(),
            ],
            5,
        ),
    ] {
        let mut source = directory_target(5, 304);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
        let analysis_record = token_parameter_record(5, values);
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
        let groups = analysis.groups().expect("Type 304 table boundary");
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
}

#[test]
fn type304_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let source = directory_target(7, 304);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &source),
    ]);

    for (form, values, expected_start) in [
        (
            1_i64,
            vec![
                304.into(),
                1.into(),
                9.into(),
                2.into(),
                2.into(),
                1.into(),
                3.into(),
                6.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
            ],
            5_usize,
        ),
        (
            2,
            vec![
                304.into(),
                5.into(),
                2.into(),
                1.into(),
                2.into(),
                1.into(),
                2.into(),
                2.into(),
                1.into(),
                3.into(),
                6.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
                5.into(),
            ],
            8,
        ),
    ] {
        let mut form_source = directory_target(7, 304);
        form_source.form = form;
        let mut form_directory = directory.clone();
        form_directory.insert(7, &form_source);
        let record = integer_parameter_record(7, &values);
        let generic = crate::test_support::with_service_context(&[], |ctx| {
            structural_pointer_group_candidates_with_context(&record, ctx)
                .expect("test-only pointer candidate allocation")
        });
        let valid_starts = generic
            .into_iter()
            .filter(|candidate| {
                crate::test_support::with_service_context(&[], |ctx| {
                    groups_for_candidate_with_context(&record, &form_directory, *candidate, ctx)
                        .expect("test-only trailing pointer allocation")
                })
                .is_some_and(|groups| groups.fully_valid().is_some())
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(
            valid_starts,
            if form == 1 { vec![4, 5] } else { vec![7, 8] },
            "Form {form}"
        );

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &form_directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &form_directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 304 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![3],
            "Form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5, 5, 5, 5, 5, 5],
            "Form {form}"
        );
    }
}

#[test]
fn type304_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (form, wrong_fields, truncated_primary, truncated_group, expected_start) in [
        (
            1_i64,
            vec![
                304.into(),
                1.into(),
                9.into(),
                2.into(),
                TokenValue::String(b"bad-scale".to_vec().into()),
                1.into(),
                1.into(),
                0.into(),
            ],
            vec![304.into(), 1.into(), 9.into(), 2.into()],
            vec![
                304.into(),
                1.into(),
                9.into(),
                2.into(),
                TokenValue::real(0.5),
                1.into(),
                1.into(),
            ],
            5_usize,
        ),
        (
            2,
            vec![
                304.into(),
                5.into(),
                TokenValue::String(b"bad-length".to_vec().into()),
                1.into(),
                2.into(),
                1.into(),
                2.into(),
                TokenValue::String(b"3".to_vec().into()),
                1.into(),
                1.into(),
                0.into(),
            ],
            vec![
                304.into(),
                5.into(),
                2.into(),
                1.into(),
                2.into(),
                1.into(),
                2.into(),
            ],
            vec![
                304.into(),
                5.into(),
                2.into(),
                1.into(),
                2.into(),
                1.into(),
                2.into(),
                TokenValue::String(b"3".to_vec().into()),
                1.into(),
                1.into(),
            ],
            8,
        ),
    ] {
        let mut source = directory_target(5, 304);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
        let analysis_record = token_parameter_record(5, wrong_fields);
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
        assert_eq!(
            analysis
                .groups()
                .expect("Type 304 wrong-field boundary")
                .token_start,
            expected_start,
            "Form {form}"
        );

        for values in [truncated_primary, truncated_group] {
            let analysis_record = token_parameter_record(5, values);
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
fn type310_nested_boundary_follows_character_and_motion_counts() {
    let association = directory_target(1, 212);
    let source = directory_target(5, 310);
    let directory = BTreeMap::from([(1, &association), (5, &source)]);
    for (values, expected_start) in [
        (
            vec![
                310.into(),
                1.into(),
                TokenValue::String(b"A".to_vec().into()),
                0.into(),
                10.into(),
                1.into(),
                65.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                1.into(),
                0.into(),
            ],
            10_usize,
        ),
        (
            vec![
                310.into(),
                1.into(),
                TokenValue::String(b"A".to_vec().into()),
                0.into(),
                10.into(),
                1.into(),
                65.into(),
                0.into(),
                0.into(),
                2.into(),
                0.into(),
                1.into(),
                2.into(),
                1.into(),
                3.into(),
                4.into(),
                1.into(),
                1.into(),
                0.into(),
            ],
            16,
        ),
        (
            vec![
                310.into(),
                1.into(),
                TokenValue::String(b"A".to_vec().into()),
                0.into(),
                10.into(),
                2.into(),
                65.into(),
                0.into(),
                0.into(),
                0.into(),
                66.into(),
                8.into(),
                0.into(),
                1.into(),
                0.into(),
                8.into(),
                0.into(),
                1.into(),
                1.into(),
                0.into(),
            ],
            17,
        ),
    ] {
        let analysis_record = token_parameter_record(5, values);
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
            "expected_start={expected_start}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "expected_start={expected_start}"
        );
        let groups = analysis.groups().expect("Type 310 nested boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    }
}

#[test]
fn type310_nested_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let source = directory_target(7, 310);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &source),
    ]);
    let record = token_parameter_record(
        7,
        vec![
            310.into(),
            101.into(),
            TokenValue::String(b"MAIN".to_vec().into()),
            (-9).into(),
            10.into(),
            2.into(),
            65.into(),
            8.into(),
            0.into(),
            1.into(),
            TokenValue::Omitted,
            1.into(),
            2.into(),
            66.into(),
            8.into(),
            0.into(),
            2.into(),
            TokenValue::Omitted,
            0.into(),
            0.into(),
            1.into(),
            8.into(),
            2.into(),
            1.into(),
            3.into(),
            6.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
        ],
    );
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    let valid_starts = generic
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
    assert_eq!(valid_starts, vec![22, 23]);

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
    let groups = analysis.groups().expect("Type 310 table boundary");
    assert_eq!(groups.token_start, 23);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(
        groups.properties().copied().collect::<Vec<_>>(),
        vec![5, 5, 5, 5, 5, 5]
    );
}

#[test]
fn type310_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 310);
    let directory = BTreeMap::from([(3, &association), (5, &property), (7, &source)]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            310.into(),
            101.into(),
            TokenValue::String(b"MAIN".to_vec().into()),
            (-9).into(),
            TokenValue::String(b"bad-scale".to_vec().into()),
            2.into(),
            65.into(),
            8.into(),
            0.into(),
            1.into(),
            TokenValue::Omitted,
            1.into(),
            2.into(),
            66.into(),
            8.into(),
            0.into(),
            2.into(),
            TokenValue::Omitted,
            0.into(),
            0.into(),
            1.into(),
            8.into(),
            0.into(),
            1.into(),
            3.into(),
            6.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
            5.into(),
        ],
    );
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &wrong_fields,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 310 wrong-field boundary")
            .token_start,
        23
    );

    let malformed = [
        token_parameter_record(
            7,
            vec![
                310.into(),
                101.into(),
                TokenValue::String(b"MAIN".to_vec().into()),
                0.into(),
                10.into(),
                0.into(),
                2.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            7,
            vec![
                310.into(),
                101.into(),
                TokenValue::String(b"MAIN".to_vec().into()),
                0.into(),
                10.into(),
                1.into(),
                65.into(),
                8.into(),
                0.into(),
                (-1).into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            7,
            vec![
                310.into(),
                101.into(),
                TokenValue::String(b"MAIN".to_vec().into()),
                0.into(),
                10.into(),
                1.into(),
                65.into(),
                8.into(),
                0.into(),
                i64::MAX.into(),
            ],
        ),
        token_parameter_record(
            7,
            vec![
                310.into(),
                101.into(),
                TokenValue::String(b"MAIN".to_vec().into()),
                0.into(),
                10.into(),
                2.into(),
                65.into(),
                8.into(),
                0.into(),
                1.into(),
                TokenValue::Omitted,
                1.into(),
                2.into(),
                66.into(),
                8.into(),
                0.into(),
                2.into(),
                TokenValue::Omitted,
                0.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            7,
            vec![
                310.into(),
                101.into(),
                TokenValue::String(b"MAIN".to_vec().into()),
                0.into(),
                10.into(),
                2.into(),
                65.into(),
                8.into(),
                0.into(),
                1.into(),
                TokenValue::Omitted,
                1.into(),
                2.into(),
                66.into(),
                8.into(),
                0.into(),
                2.into(),
                TokenValue::Omitted,
                0.into(),
                0.into(),
                1.into(),
                8.into(),
                0.into(),
                1.into(),
                3.into(),
            ],
        ),
    ];
    for record in malformed {
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
