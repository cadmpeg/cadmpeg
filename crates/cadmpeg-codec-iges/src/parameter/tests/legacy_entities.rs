use super::*;

#[test]
fn type100_form0_boundary_follows_seven_primary_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 100);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[100, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 100 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type100_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 100);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[100, 0, 0, 0, 1, 0, 1, 2, 1, 3, 1, 5]);
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
    let groups = analysis.groups().expect("Type 100 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type100_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 100);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_field = token_parameter_record(
        9,
        vec![
            100.into(),
            TokenValue::String(b"bad-z".to_vec()),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_field, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 100 boundary").token_start, 8);

    for values in [
        vec![100, 0, 0, 0, 1, 0, 1],
        vec![100, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type104_forms_share_eleven_field_boundary() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    for form in 0..=3 {
        let mut source = directory_target(9, 104);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
        let record =
            integer_parameter_record(9, &[104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 0, 1, 1, 1, 1, 5]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "form {form}");
        let groups = analysis.groups().expect("Type 104 table boundary");
        assert_eq!(groups.token_start, 12, "form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5],
            "form {form}"
        );
    }
}

#[test]
fn type104_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 104);
    source.form = 1;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 3, 1, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 104 candidate")
            .fully_valid()
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![10, 12]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 104 table boundary")
            .token_start,
        12
    );
}

#[test]
fn type104_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 104);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            104.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            0.into(),
            (-1).into(),
            0.into(),
            2.into(),
            0.into(),
            TokenValue::String(b"bad-x".to_vec()),
            TokenValue::String(b"bad-y".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis.groups().expect("Type 104 boundary").token_start,
        12
    );

    for values in [
        vec![104, 1, 0, 1, 0, 0, -1, 0, 2, 0],
        vec![104, 1, 0, 1, 0, 0, -1, 0, 2, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type108_forms_share_nine_field_boundary() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    for form in [-1, 0, 1] {
        let mut source = directory_target(9, 108);
        source.form = form;
        let pointer = if form == 0 { 0 } else { 7 };
        let directory = BTreeMap::from([
            (1, &association),
            (5, &property),
            (7, &boundary),
            (9, &source),
        ]);
        let record =
            integer_parameter_record(9, &[108, 0, 0, 1, 2, pointer, 0, 0, 0, 1, 1, 1, 1, 5]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "form {form}");
        let groups = analysis.groups().expect("Type 108 table boundary");
        assert_eq!(groups.token_start, 10, "form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5],
            "form {form}"
        );
    }
}

#[test]
fn type108_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 108);
    source.form = 1;
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (7, &boundary),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[108, 0, 0, 1, 2, 7, 0, 0, 3, 1, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 108 candidate")
            .fully_valid()
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![8, 10]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 108 table boundary")
            .token_start,
        10
    );
}

#[test]
fn type108_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let boundary = directory_target(7, 100);
    let property = directory_target(5, 406);
    let source = directory_target(9, 108);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &property),
        (7, &boundary),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            108.into(),
            0.into(),
            0.into(),
            1.into(),
            2.into(),
            7.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"bad-z".to_vec()),
            TokenValue::String(b"bad-size".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis.groups().expect("Type 108 boundary").token_start,
        10
    );

    for values in [
        vec![108, 0, 0, 1, 2, 7, 0, 0, 0],
        vec![108, 0, 0, 1, 2, 7, 0, 0, 0, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type312_forms_share_ten_field_boundary() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (form, values) in [
        (0, vec![312, 4, 2, 1, 0, 0, 0, 0, 10, 20, 0, 1, 1, 1, 3]),
        (1, vec![312, 3, 1, 18, 0, 0, 1, 1, 2, -1, 0, 1, 1, 1, 3]),
    ] {
        let mut source = directory_target(5, 312);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
        let record = integer_parameter_record(5, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "form {form}");
        let groups = analysis.groups().expect("Type 312 table boundary");
        assert_eq!(groups.token_start, 11, "form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![3],
            "form {form}"
        );
    }
}

#[test]
fn type312_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property_a = directory_target(3, 406);
    let property_b = directory_target(7, 406);
    let property_c = directory_target(9, 406);
    let mut source = directory_target(5, 312);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &property_a),
        (5, &source),
        (7, &property_b),
        (9, &property_c),
    ]);
    let record =
        integer_parameter_record(5, &[312, 4, 2, 1, 1, 0, 0, 0, 10, 20, 2, 1, 1, 3, 3, 7, 9]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 312 candidate")
            .fully_valid()
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![10, 11]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 312 table boundary");
    assert_eq!(groups.token_start, 11);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(
        groups.properties().copied().collect::<Vec<_>>(),
        vec![3, 7, 9]
    );
}

#[test]
fn type312_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (form, mut values) in [
        (0, vec![312, 4, 2, 1, 0, 0, 0, 0, 10, 20, 0]),
        (1, vec![312, 3, 1, 18, 0, 0, 1, 1, 2, -1, 0]),
    ] {
        let mut source = directory_target(5, 312);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
        let wrong_fields = token_parameter_record(
            5,
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    if index == 4 {
                        TokenValue::String(b"bad-slant".to_vec())
                    } else {
                        TokenValue::Integer(*value)
                    }
                })
                .chain([1, 1, 1, 3].into_iter().map(TokenValue::Integer))
                .collect(),
        );
        let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
        assert_eq!(analysis.candidate_count(), 1, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "form {form}");
        assert_eq!(
            analysis.groups().expect("Type 312 boundary").token_start,
            11
        );

        values.truncate(10);
        let truncated_primary = integer_parameter_record(5, &values);
        let analysis = analyze_trailing_pointer_groups(&truncated_primary, &directory);
        assert_eq!(analysis.candidate_count(), 0, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 0, "form {form}");
        assert!(analysis.groups().is_none(), "form {form}");

        values.push(0);
        values.extend([1, 1, 1]);
        let truncated_group = integer_parameter_record(5, &values);
        let analysis = analyze_trailing_pointer_groups(&truncated_group, &directory);
        assert_eq!(analysis.candidate_count(), 0, "form {form}");
        assert_eq!(analysis.valid_candidate_count(), 0, "form {form}");
        assert!(analysis.groups().is_none(), "form {form}");
    }
}

#[test]
fn type314_form0_boundary_follows_optional_color_name_slot() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(5, 314);
    let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
    for name in [TokenValue::String(b"orange".to_vec()), TokenValue::Omitted] {
        let record = token_parameter_record(
            5,
            vec![
                314.into(),
                10.into(),
                20.into(),
                30.into(),
                name,
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 314 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type314_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property_a = directory_target(3, 406);
    let property_b = directory_target(7, 406);
    let property_c = directory_target(9, 406);
    let source = directory_target(5, 314);
    let directory = BTreeMap::from([
        (1, &association),
        (3, &property_a),
        (5, &source),
        (7, &property_b),
        (9, &property_c),
    ]);
    let record = integer_parameter_record(5, &[314, 10, 20, 30, 2, 1, 1, 3, 3, 7, 9]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 314 candidate")
            .fully_valid()
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 314 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(
        groups.properties().copied().collect::<Vec<_>>(),
        vec![3, 7, 9]
    );
}

#[test]
fn type314_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(5, 314);
    let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
    let wrong_field = token_parameter_record(
        5,
        vec![
            314.into(),
            10.into(),
            20.into(),
            30.into(),
            7.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_field, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 314 boundary").token_start, 5);

    for values in [vec![314, 10, 20, 30], vec![314, 10, 20, 30, 1, 1, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(5, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type130_fixed_primary_boundary_follows_fourteen_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 130);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    for values in [
        vec![
            130.into(),
            5.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::Real(0.5),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
        vec![
            130.into(),
            5.into(),
            2.into(),
            0.into(),
            0.into(),
            1.into(),
            TokenValue::Real(0.25),
            TokenValue::Real(0.25),
            TokenValue::Real(0.75),
            TokenValue::Real(0.75),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
        vec![
            130.into(),
            5.into(),
            3.into(),
            9.into(),
            2.into(),
            2.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(7, values), &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 130 table boundary");
        assert_eq!(groups.token_start, 15);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type130_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 130);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record = token_parameter_record(
        7,
        vec![
            130.into(),
            5.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::Real(0.5),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            2.into(),
            1.into(),
            1.into(),
            3.into(),
            3.into(),
            3.into(),
            3.into(),
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
    assert_eq!(valid_starts, vec![14, 15]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 130 table boundary");
    assert_eq!(groups.token_start, 15);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(
        groups.properties().copied().collect::<Vec<_>>(),
        vec![3, 3, 3]
    );
}

#[test]
fn type130_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 130);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            130.into(),
            5.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"bad-distance".to_vec()),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 130 wrong-field boundary")
            .token_start,
        15
    );

    for values in [
        vec![
            130.into(),
            5.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::Real(0.5),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
        ],
        vec![
            130.into(),
            5.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            TokenValue::Real(0.5),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(7, values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type150_form0_boundary_follows_twelve_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 150);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit =
        integer_parameter_record(7, &[150, 2, 3, 4, 1, 2, 3, 1, 0, 0, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            150.into(),
            2.into(),
            3.into(),
            4.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 150 table boundary");
        assert_eq!(groups.token_start, 13);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type150_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 150);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record =
        integer_parameter_record(7, &[150, 2, 3, 4, 1, 2, 3, 1, 0, 0, 0, 0, 2, 1, 1, 1, 3]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![12, 13]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 150 table boundary");
    assert_eq!(groups.token_start, 13);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type150_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 150);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            150.into(),
            2.into(),
            3.into(),
            4.into(),
            TokenValue::String(b"bad".to_vec()),
            2.into(),
            3.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 150 wrong-field boundary")
            .token_start,
        13
    );

    for values in [
        vec![150, 2, 3, 4, 1, 2, 3, 1, 0, 0, 0, 0],
        vec![150, 2, 3, 4, 1, 2, 3, 1, 0, 0, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type152_form0_boundary_follows_thirteen_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 152);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit =
        integer_parameter_record(7, &[152, 4, 3, 2, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            152.into(),
            4.into(),
            3.into(),
            2.into(),
            1.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 152 table boundary");
        assert_eq!(groups.token_start, 14);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type152_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 152);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record =
        integer_parameter_record(7, &[152, 4, 3, 2, 1, 0, 0, 0, 1, 0, 0, 0, 0, 2, 1, 1, 1, 3]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![13, 14]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 152 table boundary");
    assert_eq!(groups.token_start, 14);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type152_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 152);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            152.into(),
            4.into(),
            3.into(),
            2.into(),
            TokenValue::String(b"bad".to_vec()),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 152 wrong-field boundary")
            .token_start,
        14
    );

    for values in [
        vec![152, 4, 3, 2, 1, 0, 0, 0, 1, 0, 0, 0, 0],
        vec![152, 4, 3, 2, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type154_form0_boundary_follows_eight_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 154);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit = integer_parameter_record(7, &[154, 5, 2, 1, 2, 3, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            154.into(),
            5.into(),
            2.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 154 table boundary");
        assert_eq!(groups.token_start, 9);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type154_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 154);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record = integer_parameter_record(7, &[154, 5, 2, 1, 2, 3, 0, 0, 2, 1, 1, 1, 3]);
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
    let groups = analysis.groups().expect("Type 154 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type154_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 154);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            154.into(),
            5.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            2.into(),
            3.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 154 wrong-field boundary")
            .token_start,
        9
    );

    for values in [
        vec![154, 5, 2, 1, 2, 3, 0, 0],
        vec![154, 5, 2, 1, 2, 3, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type156_form0_boundary_follows_nine_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 156);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit = integer_parameter_record(7, &[156, 5, 3, 1, 1, 2, 3, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            156.into(),
            5.into(),
            3.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 156 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type156_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 156);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record = integer_parameter_record(7, &[156, 5, 3, 1, 1, 2, 3, 0, 0, 2, 1, 1, 1, 3]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![9, 10]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 156 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type156_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 156);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            156.into(),
            5.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            1.into(),
            2.into(),
            3.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 156 wrong-field boundary")
            .token_start,
        10
    );

    for values in [
        vec![156, 5, 3, 1, 1, 2, 3, 0, 0],
        vec![156, 5, 3, 1, 1, 2, 3, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type158_form0_boundary_follows_four_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 158);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit = integer_parameter_record(7, &[158, 2, 1, 2, 3, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            158.into(),
            2.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 158 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type158_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 158);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record = integer_parameter_record(7, &[158, 2, 1, 2, 2, 1, 1, 1, 3]);
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
    let groups = analysis.groups().expect("Type 158 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type158_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 158);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            158.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            2.into(),
            3.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 158 wrong-field boundary")
            .token_start,
        5
    );

    for values in [vec![158, 2, 1, 2], vec![158, 2, 1, 2, 3, 1, 1, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type160_form0_boundary_follows_eight_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 160);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit = integer_parameter_record(7, &[160, 4, 1, 1, 2, 3, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            160.into(),
            4.into(),
            1.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 160 table boundary");
        assert_eq!(groups.token_start, 9);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type160_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 160);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record = integer_parameter_record(7, &[160, 4, 1, 1, 2, 3, 0, 0, 2, 1, 1, 1, 3]);
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
    let groups = analysis.groups().expect("Type 160 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type160_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 160);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            160.into(),
            4.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            1.into(),
            2.into(),
            3.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 160 wrong-field boundary")
            .token_start,
        9
    );

    for values in [
        vec![160, 4, 1, 1, 2, 3, 0, 0],
        vec![160, 4, 1, 1, 2, 3, 0, 0, 1, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type168_form0_boundary_follows_twelve_primary_fields_and_defaults() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 168);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let explicit =
        integer_parameter_record(7, &[168, 4, 3, 2, 1, 2, 3, 1, 0, 0, 0, 0, 1, 1, 1, 1, 3]);
    let omitted = token_parameter_record(
        7,
        vec![
            168.into(),
            4.into(),
            3.into(),
            2.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 168 table boundary");
        assert_eq!(groups.token_start, 13);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
    }
}

#[test]
fn type168_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 168);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let record =
        integer_parameter_record(7, &[168, 4, 3, 2, 1, 2, 3, 1, 0, 0, 0, 0, 2, 1, 1, 1, 3]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![12, 13]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 168 table boundary");
    assert_eq!(groups.token_start, 13);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![3]);
}

#[test]
fn type168_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let source = directory_target(7, 168);
    let directory = BTreeMap::from([(1, &association), (3, &property), (7, &source)]);
    let wrong = token_parameter_record(
        7,
        vec![
            168.into(),
            4.into(),
            TokenValue::String(b"bad".to_vec()),
            2.into(),
            1.into(),
            2.into(),
            3.into(),
            1.into(),
            0.into(),
            0.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
            3.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 168 wrong-field boundary")
            .token_start,
        13
    );

    for values in [
        vec![168, 4, 3, 2, 1, 2, 3, 1, 0, 0, 0, 0],
        vec![168, 4, 3, 2, 1, 2, 3, 1, 0, 0, 0, 0, 1, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type162_forms_follow_eight_primary_fields_and_defaults() {
    for form in [0, 1] {
        let association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 162);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
        let explicit = integer_parameter_record(9, &[162, 5, 1, 1, 2, 3, 0, 0, 1, 1, 3, 1, 7]);
        let omitted = token_parameter_record(
            9,
            vec![
                162.into(),
                5.into(),
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                1.into(),
                3.into(),
                1.into(),
                7.into(),
            ],
        );

        for record in [explicit, omitted] {
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count(), 1);
            assert_eq!(analysis.valid_candidate_count(), 1);
            let groups = analysis.groups().expect("Type 162 table boundary");
            assert_eq!(groups.token_start, 9);
            assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
            assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![7]);
        }
    }
}

#[test]
fn type162_table_boundary_precedes_valid_generic_alternatives() {
    for form in [0, 1] {
        let first_association = directory_target(1, 212);
        let second_association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 162);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &first_association),
            (3, &second_association),
            (7, &property),
            (9, &source),
        ]);
        let record = integer_parameter_record(9, &[162, 5, 1, 1, 2, 3, 0, 0, 2, 1, 3, 1, 7]);
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
        let groups = analysis.groups().expect("Type 162 table boundary");
        assert_eq!(groups.token_start, 9);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![7]);
    }
}

#[test]
fn type162_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    for form in [0, 1] {
        let association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 162);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
        let wrong = token_parameter_record(
            9,
            vec![
                162.into(),
                5.into(),
                TokenValue::String(b"bad".to_vec()),
                1.into(),
                2.into(),
                3.into(),
                0.into(),
                0.into(),
                1.into(),
                1.into(),
                3.into(),
                1.into(),
                7.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(
            analysis
                .groups()
                .expect("Type 162 wrong-field boundary")
                .token_start,
            9
        );

        for values in [
            vec![162, 5, 1, 1, 2, 3, 0, 0],
            vec![162, 5, 1, 1, 2, 3, 0, 0, 1, 1, 3, 1],
        ] {
            let analysis =
                analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
            assert_eq!(analysis.candidate_count(), 0);
            assert_eq!(analysis.valid_candidate_count(), 0);
            assert!(analysis.groups().is_none());
        }
    }
}

#[test]
fn type164_form0_boundary_follows_five_primary_fields_and_defaults() {
    let association = directory_target(3, 212);
    let property = directory_target(7, 406);
    let source = directory_target(9, 164);
    let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
    let explicit = integer_parameter_record(9, &[164, 5, 5, 0, 0, 1, 1, 3, 1, 7]);
    let omitted = token_parameter_record(
        9,
        vec![
            164.into(),
            5.into(),
            5.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            TokenValue::Omitted,
            1.into(),
            3.into(),
            1.into(),
            7.into(),
        ],
    );

    for record in [explicit, omitted] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 164 table boundary");
        assert_eq!(groups.token_start, 6);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![7]);
    }
}

#[test]
fn type164_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let property = directory_target(7, 406);
    let source = directory_target(9, 164);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (7, &property),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[164, 5, 5, 0, 0, 2, 1, 3, 1, 7]);
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
    let groups = analysis.groups().expect("Type 164 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![7]);
}

#[test]
fn type164_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let property = directory_target(7, 406);
    let source = directory_target(9, 164);
    let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
    let wrong = token_parameter_record(
        9,
        vec![
            164.into(),
            5.into(),
            TokenValue::String(b"bad".to_vec()),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            3.into(),
            1.into(),
            7.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 164 wrong-field boundary")
            .token_start,
        6
    );

    for values in [vec![164, 5, 5, 0, 0], vec![164, 5, 5, 0, 0, 1, 1, 3, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type124_forms_follow_twelve_primary_fields() {
    for form in [0_i64, 1, 10, 11, 12] {
        let association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 124);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
        let explicit =
            integer_parameter_record(9, &[124, 1, 0, 0, 1, 0, 1, 0, 2, 0, 0, 1, 3, 1, 3, 1, 7]);
        let omitted = token_parameter_record(
            9,
            vec![
                124.into(),
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                TokenValue::Omitted,
                1.into(),
                3.into(),
                1.into(),
                7.into(),
            ],
        );

        for record in [explicit, omitted] {
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count(), 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
            let groups = analysis.groups().expect("Type 124 table boundary");
            assert_eq!(groups.token_start, 13, "Form {form}");
            assert_eq!(
                groups.associations().copied().collect::<Vec<_>>(),
                vec![3],
                "Form {form}"
            );
            assert_eq!(
                groups.properties().copied().collect::<Vec<_>>(),
                vec![7],
                "Form {form}"
            );
        }
    }
}
