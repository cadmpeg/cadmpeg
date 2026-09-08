use super::*;

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
            TokenValue::String(b"FIG".to_vec()),
            (-1_i64).into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec()),
            i64::MAX.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec()),
            TokenValue::String(b"bad-count".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
        vec![308.into(), 0.into(), TokenValue::String(b"FIG".to_vec())],
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec()),
            2.into(),
            7.into(),
        ],
    ];
    for values in malformed {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
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

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count(), 1);
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
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![6, 7]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
            TokenValue::String(b"bad-class-count".to_vec()),
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
            TokenValue::String(b"bad-item-count".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
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
        let mut values: Vec<TokenValue> = vec![316_i64.into(), (count as i64).into()];
        for _ in 0..count {
            values.extend([
                TokenValue::String(b"LENGTH".to_vec()),
                TokenValue::String(b"M".to_vec()),
                1.0_f64.into(),
            ]);
        }
        values.extend([1_i64.into(), 1_i64.into(), 1_i64.into(), 5_i64.into()]);

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
        assert_eq!(analysis.candidate_count(), 1);
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
            TokenValue::String(b"LENGTH".to_vec()),
            TokenValue::String(b"M".to_vec()),
            2_i64.into(),
            1_i64.into(),
            3_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    );
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
            TokenValue::String(b"bad-count".to_vec()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            316_i64.into(),
            1_i64.into(),
            TokenValue::String(b"LENGTH".to_vec()),
            TokenValue::String(b"M".to_vec()),
        ],
    ];
    for values in malformed {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
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
            TokenValue::String(b"ATTR".to_vec()),
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

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count(), 1, "form={form}");
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
            TokenValue::String(b"ATTR".to_vec()),
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
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
            TokenValue::String(b"ATTR".to_vec()),
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
            TokenValue::String(b"ATTR".to_vec()),
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
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            TokenValue::String(b"bad-count".to_vec()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            i64::MAX.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec()),
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
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            TokenValue::String(b"bad-value-count".to_vec()),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
        ],
        vec![
            322_i64.into(),
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            2_i64.into(),
            7_i64.into(),
        ],
    ];
    for values in malformed {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
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
            TokenValue::String(b"ATTR".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups_with_records(&instance_record, &directory, &records);
        assert_eq!(analysis.candidate_count(), 1, "form={form}");
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
            TokenValue::String(b"ATTR".to_vec()),
            1_i64.into(),
            1_i64.into(),
            10_i64.into(),
            1_i64.into(),
            2_i64.into(),
        ],
    );
    let record = integer_parameter_record(11, &[422, 1, 7, 2, 1, 3, 1, 5]);
    let records = BTreeMap::from([(9, &definition_record), (11, &record)]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![3, 4]);

    let analysis = analyze_trailing_pointer_groups_with_records(&record, &directory, &records);
    assert_eq!(analysis.candidate_count(), 1);
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
                TokenValue::String(b"ATTR".to_vec()),
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
                TokenValue::String(b"ATTR".to_vec()),
                1_i64.into(),
                1_i64.into(),
                10_i64.into(),
                1_i64.into(),
                TokenValue::String(b"bad-count".to_vec()),
            ],
        ),
    ];
    let instances = [
        integer_parameter_record(11, &[422, -1, 7, 2, 1, 1, 1, 5]),
        token_parameter_record(
            11,
            vec![
                422_i64.into(),
                TokenValue::String(b"bad-row-count".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups_with_records(instance_record, &directory, &records);
        assert_eq!(analysis.candidate_count(), 0);
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
    let analysis = analyze_trailing_pointer_groups_with_records(
        &unresolved_record,
        &unresolved_directory,
        &records,
    );
    assert_eq!(analysis.candidate_count(), 0);
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type404_entity_table_boundary_follows_view_and_annotation_lists() {
    let association = directory_target(1, 212);
    let annotation = directory_target(3, 212);
    let property = directory_target(5, 406);
    let view = directory_target(7, 410);
    for (form, values, expected_start) in [
        (
            0_i64,
            vec![
                404_i64.into(),
                1_i64.into(),
                7_i64.into(),
                10_i64.into(),
                20_i64.into(),
                1_i64.into(),
                3_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
            7_usize,
        ),
        (
            1_i64,
            vec![
                404_i64.into(),
                1_i64.into(),
                7_i64.into(),
                10_i64.into(),
                20_i64.into(),
                TokenValue::Real(0.5),
                1_i64.into(),
                3_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
            8,
        ),
    ] {
        let mut source = directory_target(9, 404);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &annotation),
            (5, &property),
            (7, &view),
            (9, &source),
        ]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
        assert_eq!(analysis.candidate_count(), 1, "form={form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "form={form}");
        let groups = analysis.groups().expect("Type 404 table boundary");
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

    let mut source = directory_target(9, 404);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &annotation),
        (5, &property),
        (7, &view),
        (9, &source),
    ]);
    let analysis = analyze_trailing_pointer_groups(
        &token_parameter_record(
            9,
            vec![
                404_i64.into(),
                TokenValue::Omitted,
                1_i64.into(),
                3_i64.into(),
                1_i64.into(),
                1_i64.into(),
                1_i64.into(),
                5_i64.into(),
            ],
        ),
        &directory,
    );
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis
        .groups()
        .expect("Type 404 omitted-count table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type404_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let annotation = directory_target(2, 212);
    let property = directory_target(5, 406);
    let view = directory_target(7, 410);
    let mut source = directory_target(9, 404);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association_1),
        (2, &annotation),
        (3, &association_3),
        (5, &property),
        (7, &view),
        (9, &source),
    ]);
    let record = token_parameter_record(
        9,
        vec![
            404_i64.into(),
            1_i64.into(),
            7_i64.into(),
            10_i64.into(),
            20_i64.into(),
            1_i64.into(),
            2_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    );
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![6, 7]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 404 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type404_malformed_counts_or_spans_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let view = directory_target(7, 410);
    let mut source = directory_target(9, 404);
    source.form = 1;
    let directory = BTreeMap::from([(1, &association), (5, &property), (7, &view), (9, &source)]);
    let malformed: Vec<Vec<TokenValue>> = vec![
        vec![
            404_i64.into(),
            TokenValue::String(b"bad-view-count".to_vec()),
            7_i64.into(),
            10_i64.into(),
            20_i64.into(),
            TokenValue::Real(0.5),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![
            404_i64.into(),
            (-1_i64).into(),
            7_i64.into(),
            10_i64.into(),
            20_i64.into(),
            TokenValue::Real(0.5),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
        vec![404_i64.into(), 1_i64.into(), 7_i64.into(), 10_i64.into()],
        vec![
            404_i64.into(),
            1_i64.into(),
            7_i64.into(),
            10_i64.into(),
            20_i64.into(),
            1_i64.into(),
        ],
        vec![
            404_i64.into(),
            1_i64.into(),
            7_i64.into(),
            10_i64.into(),
            20_i64.into(),
            1_i64.into(),
            TokenValue::String(b"bad-annotation-count".to_vec()),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    ];
    for values in malformed {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(9, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type141_entity_table_boundary_uses_nested_curve_counts() {
    for (counts, expected_start) in [
        (vec![0_i64], 8_usize),
        (vec![2], 10),
        (vec![0, 0], 11),
        (vec![1, 2], 14),
    ] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 141);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 141;
        values[1] = i64::from(counts.iter().any(|count| *count > 0));
        values[2] = 1;
        values[3] = 1;
        values[4] = i64::try_from(counts.len()).expect("test count fits");
        let mut index = 5;
        for count in counts {
            values[index] = 1;
            values[index + 1] = 1;
            values[index + 2] = count;
            for pcurve_index in 0..usize::try_from(count).expect("test count is nonnegative") {
                values[index + 3 + pcurve_index] = 1;
            }
            index += 3 + usize::try_from(count).expect("test count is nonnegative");
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        assert_eq!(index, expected_start);
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
            parameter_end: expected_start + 3,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 141 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type141_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 141);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [141, 1, 1, 1, 1, 1, 3, 2, 1, 2, 1, 3, 0];
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
    let groups = analysis.groups().expect("Type 141 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type141_malformed_boundary_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 141);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![141, 1, 1, 0, 0, 1, 1, 0],
        vec![141, 1, 1, -1, 0, 1, 1, 0],
        vec![141, 1, 1, 100, 0, 1, 1, 0],
        vec![141, 1, 1, 1, 1, 1, 1, 1, 1, 0],
        vec![141, 1, 1, 1, 1, 1, 1, -1, 1, 1, 0],
    ] {
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
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type142_form0_follows_five_primary_fields() {
    let surface = directory_target(3, 108);
    let model_curve = directory_target(5, 106);
    let parameter_curve = directory_target(7, 106);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 142);
    let directory = BTreeMap::from([
        (3, &surface),
        (5, &model_curve),
        (7, &parameter_curve),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[142, 1, 3, 7, 5, 1, 1, 9, 1, 11]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 142 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type142_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let surface = directory_target(3, 108);
    let model_curve = directory_target(5, 106);
    let parameter_curve = directory_target(7, 106);
    let second_association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 142);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &surface),
        (5, &model_curve),
        (7, &parameter_curve),
        (9, &second_association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[142, 1, 3, 7, 5, 2, 1, 9, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 142 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type142_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let surface = directory_target(3, 108);
    let model_curve = directory_target(5, 106);
    let parameter_curve = directory_target(7, 106);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 142);
    let directory = BTreeMap::from([
        (3, &surface),
        (5, &model_curve),
        (7, &parameter_curve),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);

    for preference in [TokenValue::String(b"bad".to_vec()), TokenValue::Real(2.5)] {
        let wrong = token_parameter_record(
            13,
            vec![
                142.into(),
                1.into(),
                3.into(),
                7.into(),
                5.into(),
                preference,
                1.into(),
                9.into(),
                1.into(),
                11.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(
            analysis
                .groups()
                .expect("Type 142 wrong-field boundary")
                .token_start,
            6
        );
    }

    for values in [vec![142, 1, 3, 7, 5], vec![142, 1, 3, 7, 5, 1, 1, 9, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type208_form0_follows_leader_count() {
    let note = directory_target(3, 212);
    let leader_one = {
        let mut entry = directory_target(5, 214);
        entry.form = 1;
        entry
    };
    let leader_two = {
        let mut entry = directory_target(7, 214);
        entry.form = 1;
        entry
    };
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 208);
    let directory = BTreeMap::from([
        (3, &note),
        (5, &leader_one),
        (7, &leader_two),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);

    for (values, expected_start) in [
        (vec![208, 0, 0, 0, 0, 3, 0, 1, 9, 1, 11], 7_usize),
        (vec![208, 0, 0, 0, 0, 3, 2, 5, 7, 1, 9, 1, 11], 9_usize),
    ] {
        let record = integer_parameter_record(13, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 208 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
    }
}

#[test]
fn type208_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let note = directory_target(3, 212);
    let second_association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 208);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &note),
        (9, &second_association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[208, 0, 0, 0, 0, 3, 0, 2, 1, 9, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![7, 8]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 208 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![1, 9]
    );
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type208_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let note = directory_target(3, 212);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 208);
    let directory = BTreeMap::from([
        (3, &note),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);

    for preference in [TokenValue::String(b"bad".to_vec()), TokenValue::Real(2.5)] {
        let wrong = token_parameter_record(
            13,
            vec![
                208.into(),
                0.into(),
                0.into(),
                0.into(),
                preference,
                3.into(),
                0.into(),
                1.into(),
                9.into(),
                1.into(),
                11.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(
            analysis
                .groups()
                .expect("Type 208 wrong-field boundary")
                .token_start,
            7
        );
    }

    for values in [
        vec![208, 0, 0, 0, 0, 3],
        vec![208, 0, 0, 0, 0, 3, 0, 1, 9, 1],
        vec![208, 0, 0, 0, 0, 3, -1, 1, 9, 1, 11],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type210_form0_follows_positive_leader_count() {
    let note = directory_target(3, 212);
    let leader_one = {
        let mut entry = directory_target(5, 214);
        entry.form = 1;
        entry
    };
    let leader_two = {
        let mut entry = directory_target(7, 214);
        entry.form = 1;
        entry
    };
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 210);
    let directory = BTreeMap::from([
        (3, &note),
        (5, &leader_one),
        (7, &leader_two),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);

    for (values, expected_start) in [
        (vec![210, 3, 1, 5, 1, 9, 1, 11], 4_usize),
        (vec![210, 3, 2, 5, 7, 1, 9, 1, 11], 5_usize),
    ] {
        let record = integer_parameter_record(13, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 210 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
    }
}

#[test]
fn type210_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let note = directory_target(3, 212);
    let second_association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 210);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &note),
        (9, &second_association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[210, 3, 1, 2, 1, 9, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![3, 4]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 210 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type210_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let note = directory_target(3, 212);
    let leader = {
        let mut entry = directory_target(5, 214);
        entry.form = 1;
        entry
    };
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 210);
    let directory = BTreeMap::from([
        (3, &note),
        (5, &leader),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);

    for preference in [TokenValue::String(b"bad".to_vec()), TokenValue::Real(3.5)] {
        let wrong = token_parameter_record(
            13,
            vec![
                210.into(),
                preference,
                1.into(),
                5.into(),
                1.into(),
                9.into(),
                1.into(),
                11.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(
            analysis
                .groups()
                .expect("Type 210 wrong-field boundary")
                .token_start,
            4
        );
    }

    let wrong_count = token_parameter_record(
        13,
        vec![
            210.into(),
            3.into(),
            TokenValue::Real(1.5),
            5.into(),
            1.into(),
            9.into(),
            1.into(),
            11.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_count, &directory);
    assert_eq!(analysis.candidate_count(), 0);
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());

    for values in [
        vec![210, 3, 1, 5, 1, 9, 1],
        vec![210, 3, 0, 1, 9, 1, 11],
        vec![210, 3, -1, 1, 9, 1, 11],
        vec![210, 3],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type212_forms_follow_string_count() {
    let text_block = |text: &[u8]| -> Vec<TokenValue> {
        vec![
            TokenValue::Integer(text.len() as i64),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::String(text.to_vec()),
        ]
    };
    let cases = [
        (
            {
                let mut values = vec![TokenValue::Integer(212), TokenValue::Integer(1)];
                values.extend(text_block(b"A"));
                values.extend([
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(1),
                    TokenValue::Integer(5),
                ]);
                values
            },
            14_usize,
        ),
        (
            {
                let mut values = vec![TokenValue::Integer(212), TokenValue::Integer(2)];
                values.extend(text_block(b"A"));
                values.extend(text_block(b"BC"));
                values.extend([
                    TokenValue::Integer(1),
                    TokenValue::Integer(3),
                    TokenValue::Integer(1),
                    TokenValue::Integer(5),
                ]);
                values
            },
            26_usize,
        ),
    ];

    for form in [0, 1, 2, 3, 4, 5, 6, 7, 8, 100, 101, 102, 105] {
        let association = directory_target(3, 212);
        let property = directory_target(5, 406);
        let mut source = directory_target(7, 212);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (5, &property), (7, &source)]);
        for (values, expected_start) in &cases {
            let record = token_parameter_record(7, values.clone());
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count(), 1);
            assert_eq!(analysis.valid_candidate_count(), 1);
            let groups = analysis.groups().expect("Type 212 table boundary");
            assert_eq!(groups.token_start, *expected_start);
            assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
            assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
        }
    }
}
