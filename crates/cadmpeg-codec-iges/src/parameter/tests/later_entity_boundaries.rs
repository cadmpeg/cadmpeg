use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::analyze_trailing_pointer_groups_with_records_for_global_table;
use crate::parameter::entity_primary_end;
use crate::parameter::entity_primary_end_with_records;
use crate::parameter::groups_for_candidate_with_context;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

#[test]
fn type308_malformed_counts_or_spans_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(11, 308);
    let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);
    let malformed = [
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec().into()),
            (-1_i64).into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec().into()),
            i64::MAX.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec().into()),
            TokenValue::String(b"bad-count".to_vec().into()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec().into()),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec().into()),
            2.into(),
            7.into(),
        ],
    ];
    for values in malformed {
        let analysis_record = token_parameter_record(11, values);
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
fn type302_entity_table_boundary_follows_variable_class_grammar() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);

    for item_counts in [vec![1_usize], vec![2], vec![1, 2]] {
        let expected_start = 2 + item_counts
            .iter()
            .map(|item_count| 3 + item_count)
            .sum::<usize>();
        let mut source = directory_target(11, 302);
        source.form = 5001;
        let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);
        let class_count = i64::try_from(item_counts.len()).expect("test class count fits");
        let mut values: Vec<TokenValue> = vec![302_i64.into(), class_count.into()];
        for item_count in item_counts {
            values.extend([
                1_i64.into(),
                1_i64.into(),
                i64::try_from(item_count)
                    .expect("test item count fits")
                    .into(),
            ]);
            values.extend((0..item_count).map(|_| 1_i64.into()));
        }
        values.extend([1_i64.into(), 1_i64.into(), 1_i64.into(), 5_i64.into()]);

        let analysis_record = token_parameter_record(11, values);
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
        let groups = analysis.groups().expect("Type 302 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
    }
}

#[test]
fn type302_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(11, 302);
    source.form = 5001;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[302, 1, 1, 1, 2, 1, 2, 1, 3, 1, 5]);
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
    assert_eq!(valid_starts, vec![6, 7]);

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
    let groups = analysis.groups().expect("Type 302 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type302_malformed_class_counts_or_spans_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(11, 302);
    source.form = 5001;
    let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);
    let malformed: Vec<Vec<TokenValue>> = vec![
        vec![
            302_i64.into(),
            0_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            (-1_i64).into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            i64::MAX.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            TokenValue::String(b"bad-class-count".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            0_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            TokenValue::String(b"bad-item-count".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            302_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            2_i64.into(),
            1_i64.into(),
        ],
    ];
    for values in malformed {
        let analysis_record = token_parameter_record(11, values);
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
fn type316_entity_table_boundary_follows_unit_entry_count() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);

    for (count, expected_start) in [(1_usize, 5_usize), (2, 8)] {
        let mut source = directory_target(9, 316);
        source.form = 0;
        let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
        let mut values: Vec<TokenValue> = vec![
            316_i64.into(),
            (i64::try_from(count).expect("test count fits i64")).into(),
        ];
        for _ in 0..count {
            values.extend([
                TokenValue::String(b"LENGTH".to_vec().into()),
                TokenValue::String(b"M".to_vec().into()),
                1.0_f64.into(),
            ]);
        }
        values.extend([1_i64.into(), 1_i64.into(), 1_i64.into(), 5_i64.into()]);

        let analysis_record = token_parameter_record(9, values);
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
        let groups = analysis.groups().expect("Type 316 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
    }
}

#[test]
fn type316_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 316);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (9, &source),
    ]);
    let record = token_parameter_record(
        9,
        vec![
            316_i64.into(),
            1_i64.into(),
            TokenValue::String(b"LENGTH".to_vec().into()),
            TokenValue::String(b"M".to_vec().into()),
            2_i64.into(),
            1_i64.into(),
            3_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    );
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
    assert_eq!(valid_starts, vec![4, 5]);

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
    let groups = analysis.groups().expect("Type 316 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type316_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 316);
    source.form = 0;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let malformed: Vec<Vec<TokenValue>> = vec![
        vec![
            316_i64.into(),
            0_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            316_i64.into(),
            (-1_i64).into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            316_i64.into(),
            i64::MAX.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            316_i64.into(),
            TokenValue::String(b"bad-count".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            316_i64.into(),
            1_i64.into(),
            TokenValue::String(b"LENGTH".to_vec().into()),
            TokenValue::String(b"M".to_vec().into()),
        ],
    ];
    for values in malformed {
        let analysis_record = token_parameter_record(9, values);
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
fn type322_entity_table_boundary_follows_form_specific_attribute_values() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);

    for (form, value_counts, value_stride) in [
        (0_i64, vec![0_usize, 2], 0_usize),
        (1, vec![0, 2], 1),
        (2, vec![1, 0], 2),
    ] {
        let expected_start = 4 + value_counts
            .iter()
            .map(|count| 3 + count * value_stride)
            .sum::<usize>();
        let mut source = directory_target(11, 322);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);
        let attribute_count = i64::try_from(value_counts.len()).expect("test count fits");
        let mut values: Vec<TokenValue> = vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            attribute_count.into(),
        ];
        for (attribute_index, value_count) in value_counts.into_iter().enumerate() {
            values.extend([
                i64::try_from(attribute_index + 1)
                    .expect("test attribute type fits")
                    .into(),
                1_i64.into(),
                i64::try_from(value_count)
                    .expect("test value count fits")
                    .into(),
            ]);
            for value_index in 0..value_count * value_stride {
                values.push(
                    i64::try_from(value_index + 1)
                        .expect("test value fits")
                        .into(),
                );
            }
        }
        values.extend([1_i64.into(), 1_i64.into(), 1_i64.into(), 5_i64.into()]);

        let analysis_record = token_parameter_record(11, values);
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
            "form={form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "form={form}");
        let groups = analysis.groups().expect("Type 322 table boundary");
        assert_eq!(groups.token_start, expected_start, "form={form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "form={form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5],
            "form={form}"
        );
    }
}

#[test]
fn type322_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(11, 322);
    source.form = 1;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (11, &source),
    ]);
    let record = token_parameter_record(
        11,
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            10_i64.into(),
            1_i64.into(),
            2_i64.into(),
            1_i64.into(),
            2_i64.into(),
            1_i64.into(),
            3_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    );
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
    assert_eq!(valid_starts, vec![8, 9]);

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
    let groups = analysis.groups().expect("Type 322 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type322_malformed_counts_or_spans_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(11, 322);
    source.form = 1;
    let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);
    let malformed: Vec<Vec<TokenValue>> = vec![
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            0_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            (-1_i64).into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            TokenValue::String(b"bad-count".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            i64::MAX.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            (-1_i64).into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            TokenValue::String(b"bad-value-count".to_vec().into()),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            2_i64.into(),
            7_i64.into(),
        ],
    ];
    for values in malformed {
        let analysis_record = token_parameter_record(11, values);
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
fn type422_entity_table_boundary_follows_referenced_definition_shape() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut definition = directory_target(9, 322);
    definition.form = 0;
    let definition_record = token_parameter_record(
        9,
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            10_i64.into(),
            1_i64.into(),
            2_i64.into(),
        ],
    );
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                422_i64.into(),
                7_i64.into(),
                8_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
            3_usize,
        ),
        (
            1_i64,
            vec![
                422_i64.into(),
                2_i64.into(),
                7_i64.into(),
                8_i64.into(),
                9_i64.into(),
                10_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
            6,
        ),
    ] {
        let mut instance = directory_target(11, 422);
        instance.form = form;
        instance.structure = -9;
        let directory = BTreeMap::from([
            (1, &association),
            (5, &property),
            (9, &definition),
            (11, &instance),
        ]);
        let instance_record = token_parameter_record(11, values);
        let records = BTreeMap::from([(9, &definition_record), (11, &instance_record)]);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_with_records_for_global_table(
                &instance_record,
                &directory,
                &records,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &instance_record,
                entity_primary_end_with_records(&instance_record, &directory, &records)
            ),
            1,
            "form={form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "form={form}");
        let groups = analysis.groups().expect("Type 422 table boundary");
        assert_eq!(groups.token_start, expected_start, "form={form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "form={form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5],
            "form={form}"
        );
    }
}

#[test]
fn type422_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let mut definition = directory_target(9, 322);
    definition.form = 0;
    let mut instance = directory_target(11, 422);
    instance.form = 1;
    instance.structure = -9;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (9, &definition),
        (11, &instance),
    ]);
    let definition_record = token_parameter_record(
        9,
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec().into()),
            1_i64.into(),
            1_i64.into(),
            10_i64.into(),
            1_i64.into(),
            2_i64.into(),
        ],
    );
    let record = integer_parameter_record(11, &[422, 1, 7, 2, 1, 3, 1, 5]);
    let records = BTreeMap::from([(9, &definition_record), (11, &record)]);
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
    assert_eq!(valid_starts, vec![3, 4]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_with_records_for_global_table(
            &record,
            &directory,
            &records,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(
            &record,
            entity_primary_end_with_records(&record, &directory, &records)
        ),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 422 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type422_malformed_definition_or_value_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut definition = directory_target(9, 322);
    definition.form = 0;
    let mut instance = directory_target(11, 422);
    instance.form = 1;
    instance.structure = -9;
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (9, &definition),
        (11, &instance),
    ]);
    let definitions = [
        token_parameter_record(
            9,
            vec![
                322_i64.into(),
                TokenValue::String(b"ATTR".to_vec().into()),
                1_i64.into(),
                1_i64.into(),
                10_i64.into(),
                1_i64.into(),
                2_i64.into(),
            ],
        ),
        token_parameter_record(
            9,
            vec![
                322_i64.into(),
                TokenValue::String(b"ATTR".to_vec().into()),
                1_i64.into(),
                1_i64.into(),
                10_i64.into(),
                1_i64.into(),
                TokenValue::String(b"bad-count".to_vec().into()),
            ],
        ),
    ];
    let instances = [
        integer_parameter_record(11, &[422, -1, 7, 2, 1, 1, 1, 5]),
        token_parameter_record(
            11,
            vec![
                422_i64.into(),
                TokenValue::String(b"bad-row-count".to_vec().into()),
                7_i64.into(),
                2_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
        ),
        integer_parameter_record(11, &[422, 1, 7, 1]),
    ];
    for (definition_record, instance_record) in [
        (&definitions[0], &instances[0]),
        (&definitions[0], &instances[1]),
        (&definitions[0], &instances[2]),
        (&definitions[1], &instances[2]),
    ] {
        let records = BTreeMap::from([(9, definition_record), (11, instance_record)]);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_with_records_for_global_table(
                instance_record,
                &directory,
                &records,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                instance_record,
                entity_primary_end_with_records(instance_record, &directory, &records)
            ),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }

    let mut unresolved_instance = instance;
    unresolved_instance.structure = 0;
    let unresolved_record = integer_parameter_record(11, &[422, 1, 7, 2, 1, 1, 1, 5]);
    let records = BTreeMap::from([(9, &definitions[0]), (11, &unresolved_record)]);
    let unresolved_directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (9, &definition),
        (11, &unresolved_instance),
    ]);
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_with_records_for_global_table(
            &unresolved_record,
            &unresolved_directory,
            &records,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(
            &unresolved_record,
            entity_primary_end_with_records(&unresolved_record, &unresolved_directory, &records)
        ),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}
