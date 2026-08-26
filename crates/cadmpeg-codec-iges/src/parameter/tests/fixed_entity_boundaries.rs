use super::*;

#[test]
fn type124_table_boundary_precedes_valid_generic_alternative() {
    for form in [0_i64, 1, 10, 11, 12] {
        let first_association = directory_target(1, 212);
        let second_association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 124);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &first_association),
            (3, &second_association),
            (7, &property),
            (9, &source),
        ]);
        let record =
            integer_parameter_record(9, &[124, 1, 0, 0, 1, 0, 1, 0, 2, 0, 0, 1, 2, 1, 3, 1, 7]);
        let valid_starts = structural_pointer_group_candidates(&record)
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(valid_starts, vec![12, 13], "Form {form}");

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 124 table boundary");
        assert_eq!(groups.token_start, 13, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert_eq!(groups.properties, vec![7], "Form {form}");
    }
}

#[test]
fn type124_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    for form in [0_i64, 1, 10, 11, 12] {
        let association = directory_target(3, 212);
        let property = directory_target(7, 406);
        let mut source = directory_target(9, 124);
        source.form = form;
        let directory = BTreeMap::from([(3, &association), (7, &property), (9, &source)]);
        let wrong = token_parameter_record(
            9,
            vec![
                124.into(),
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                TokenValue::String(b"bad".to_vec()),
                2.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                1.into(),
                3.into(),
                1.into(),
                7.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        assert_eq!(
            analysis
                .groups
                .expect("Type 124 wrong-field boundary")
                .token_start,
            13,
            "Form {form}"
        );

        for values in [
            vec![124, 1, 0, 0, 1, 0, 1, 0, 2, 0, 0, 1],
            vec![124, 1, 0, 0, 1, 0, 1, 0, 2, 0, 0, 1, 3, 1, 3, 1],
        ] {
            let analysis =
                analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type125_forms_follow_six_primary_fields() {
    for form in 0..=4 {
        let association = directory_target(5, 212);
        let property = directory_target(9, 406);
        let mut source = directory_target(11, 125);
        source.form = form;
        let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
        let record = integer_parameter_record(11, &[125, 10, 20, 3, 4, 0, 0, 1, 5, 1, 9]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 125 table boundary");
        assert_eq!(groups.token_start, 7, "Form {form}");
        assert_eq!(groups.associations, vec![5], "Form {form}");
        assert_eq!(groups.properties, vec![9], "Form {form}");
    }
}

#[test]
fn type118_forms_follow_four_primary_fields() {
    for form in [0_i64, 1] {
        let association = directory_target(5, 212);
        let property = directory_target(9, 406);
        let mut source = directory_target(11, 118);
        source.form = form;
        let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
        let explicit = integer_parameter_record(11, &[118, 3, 7, 0, 0, 1, 5, 1, 9]);
        let omitted = token_parameter_record(
            11,
            vec![
                118.into(),
                3.into(),
                7.into(),
                TokenValue::Omitted,
                TokenValue::Omitted,
                1.into(),
                5.into(),
                1.into(),
                9.into(),
            ],
        );

        for record in [explicit, omitted] {
            let analysis = analyze_trailing_pointer_groups(&record, &directory);
            assert_eq!(analysis.candidate_count, 1, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
            let groups = analysis.groups.expect("Type 118 table boundary");
            assert_eq!(groups.token_start, 5, "Form {form}");
            assert_eq!(groups.associations, vec![5], "Form {form}");
            assert_eq!(groups.properties, vec![9], "Form {form}");
        }
    }
}

#[test]
fn type118_table_boundary_precedes_valid_generic_alternative() {
    for form in [0_i64, 1] {
        let first_association = directory_target(1, 212);
        let second_association = directory_target(5, 212);
        let property = directory_target(9, 406);
        let mut source = directory_target(11, 118);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &first_association),
            (5, &second_association),
            (9, &property),
            (11, &source),
        ]);
        let record = integer_parameter_record(11, &[118, 3, 7, 0, 2, 1, 5, 1, 9]);
        let valid_starts = structural_pointer_group_candidates(&record)
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(valid_starts, vec![4, 5], "Form {form}");

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 118 table boundary");
        assert_eq!(groups.token_start, 5, "Form {form}");
        assert_eq!(groups.associations, vec![5], "Form {form}");
        assert_eq!(groups.properties, vec![9], "Form {form}");
    }
}

#[test]
fn type118_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    for form in [0_i64, 1] {
        let association = directory_target(5, 212);
        let property = directory_target(9, 406);
        let mut source = directory_target(11, 118);
        source.form = form;
        let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
        let wrong = token_parameter_record(
            11,
            vec![
                118.into(),
                3.into(),
                7.into(),
                TokenValue::String(b"bad".to_vec()),
                0.into(),
                1.into(),
                5.into(),
                1.into(),
                9.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        assert_eq!(
            analysis
                .groups
                .expect("Type 118 wrong-field boundary")
                .token_start,
            5,
            "Form {form}"
        );

        for values in [vec![118, 3, 7, 0], vec![118, 3, 7, 0, 0, 1, 5, 1]] {
            let analysis =
                analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
        }
    }
}

#[test]
fn type120_form0_follows_four_primary_fields() {
    let association = directory_target(5, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 120);
    let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
    let record = integer_parameter_record(11, &[120, 3, 7, 0, 1, 1, 5, 1, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 120 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![5]);
    assert_eq!(groups.properties, vec![9]);
}

#[test]
fn type120_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let axis = directory_target(3, 110);
    let second_association = directory_target(5, 212);
    let generatrix = directory_target(7, 110);
    let property = directory_target(9, 406);
    let source = directory_target(11, 120);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &axis),
        (5, &second_association),
        (7, &generatrix),
        (9, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[120, 3, 7, 0, 2, 1, 5, 1, 9]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 120 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![5]);
    assert_eq!(groups.properties, vec![9]);
}

#[test]
fn type120_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(5, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 120);
    let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
    let wrong = token_parameter_record(
        11,
        vec![
            120.into(),
            3.into(),
            7.into(),
            0.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            5.into(),
            1.into(),
            9.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 120 wrong-field boundary")
            .token_start,
        5
    );

    for values in [vec![120, 3, 7, 0], vec![120, 3, 7, 0, 1, 1, 5, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type122_form0_follows_four_primary_fields() {
    let association = directory_target(5, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 122);
    let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
    let record = integer_parameter_record(11, &[122, 3, 0, 1, 1, 1, 5, 1, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 122 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![5]);
    assert_eq!(groups.properties, vec![9]);
}

#[test]
fn type122_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let directrix = directory_target(3, 110);
    let second_association = directory_target(5, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 122);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &directrix),
        (5, &second_association),
        (9, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[122, 3, 0, 1, 2, 1, 5, 1, 9]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 122 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![5]);
    assert_eq!(groups.properties, vec![9]);
}

#[test]
fn type122_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(5, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 122);
    let directory = BTreeMap::from([(5, &association), (9, &property), (11, &source)]);
    let wrong = token_parameter_record(
        11,
        vec![
            122.into(),
            3.into(),
            0.into(),
            1.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            5.into(),
            1.into(),
            9.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 122 wrong-field boundary")
            .token_start,
        5
    );

    for values in [vec![122, 3, 0, 1], vec![122, 3, 0, 1, 1, 1, 5, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type182_form0_follows_four_primary_fields() {
    let tree = directory_target(7, 180);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 182);
    let directory = BTreeMap::from([
        (7, &tree),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[182, 7, 1, 2, 3, 1, 9, 1, 11]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 182 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![9]);
    assert_eq!(groups.properties, vec![11]);
}

#[test]
fn type182_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let tree = directory_target(7, 180);
    let second_association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 182);
    let directory = BTreeMap::from([
        (1, &first_association),
        (7, &tree),
        (9, &second_association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[182, 7, 1, 2, 2, 1, 9, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 182 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![9]);
    assert_eq!(groups.properties, vec![11]);
}

#[test]
fn type182_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let tree = directory_target(7, 180);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 182);
    let directory = BTreeMap::from([
        (7, &tree),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);
    let wrong = token_parameter_record(
        13,
        vec![
            182.into(),
            7.into(),
            1.into(),
            2.into(),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            9.into(),
            1.into(),
            11.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
            .expect("Type 182 wrong-field boundary")
            .token_start,
        5
    );

    for values in [vec![182, 7, 1, 2], vec![182, 7, 1, 2, 3, 1, 9, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type186_form0_follows_void_shell_pairs() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let shell = directory_target(57, 514);
    let property = directory_target(59, 406);
    let source = directory_target(61, 186);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (57, &shell),
        (59, &property),
        (61, &source),
    ]);
    let record = integer_parameter_record(61, &[186, 57, 1, 0, 1, 3, 1, 59]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 186 zero-void boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![59]);
}

#[test]
fn type186_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let shell = directory_target(57, 514);
    let void_shell = directory_target(111, 514);
    let property = directory_target(113, 406);
    let source = directory_target(115, 186);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (57, &shell),
        (111, &void_shell),
        (113, &property),
        (115, &source),
    ]);
    let record = integer_parameter_record(115, &[186, 57, 1, 1, 111, 2, 1, 3, 1, 113]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 186 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![113]);
}

#[test]
fn type186_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let shell = directory_target(57, 514);
    let void_shell = directory_target(111, 514);
    let property = directory_target(113, 406);
    let source = directory_target(115, 186);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (57, &shell),
        (111, &void_shell),
        (113, &property),
        (115, &source),
    ]);
    for wrong_field in [TokenValue::String(b"bad".to_vec()), TokenValue::Real(2.5)] {
        let wrong = token_parameter_record(
            115,
            vec![
                186.into(),
                57.into(),
                1.into(),
                1.into(),
                111.into(),
                wrong_field,
                1.into(),
                3.into(),
                1.into(),
                113.into(),
            ],
        );
        let analysis = analyze_trailing_pointer_groups(&wrong, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        assert_eq!(
            analysis
                .groups
                .expect("Type 186 wrong-field boundary")
                .token_start,
            6
        );
    }

    for record in [
        integer_parameter_record(115, &[186, 57, 1, 1]),
        integer_parameter_record(115, &[186, 57, 1, 1, 111, 0, 1, 3, 1]),
        integer_parameter_record(115, &[186, 57, 1, -1, 1, 3, 1, 113]),
    ] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type_brep_entity_table_boundaries_follow_counted_and_nested_lists() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (entity_type, form, values, expected_start) in [
        (
            502_i64,
            1_i64,
            vec![
                502.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            5_usize,
        ),
        (
            504,
            1,
            vec![
                504.into(),
                1.into(),
                1.into(),
                5.into(),
                1.into(),
                7.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            7,
        ),
        (
            508,
            1,
            vec![
                508.into(),
                1.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                0.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            7,
        ),
        (
            508,
            1,
            vec![
                508.into(),
                1.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                0.into(),
                7.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            9,
        ),
        (
            508,
            1,
            vec![
                508.into(),
                2.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                0.into(),
                7.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                0.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            14,
        ),
        (
            510,
            1,
            vec![
                510.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            5,
        ),
        (
            514,
            1,
            vec![
                514.into(),
                1.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            4,
        ),
        (
            514,
            2,
            vec![
                514.into(),
                1.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(9, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (9, &source)]);
        let record = token_parameter_record(9, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        let groups = analysis.groups.expect("B-rep entity table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.associations,
            vec![1],
            "Type {entity_type} Form {form}"
        );
        assert_eq!(groups.properties, vec![3], "Type {entity_type} Form {form}");
    }
}

#[test]
fn type_brep_table_boundaries_precede_valid_generic_alternatives() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    for (entity_type, form, values, expected_start) in [
        (
            502_i64,
            1_i64,
            vec![
                502.into(),
                1.into(),
                0.into(),
                0.into(),
                2.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            5_usize,
        ),
        (
            504,
            1,
            vec![
                504.into(),
                1.into(),
                1.into(),
                5.into(),
                1.into(),
                7.into(),
                2.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            7,
        ),
        (
            508,
            1,
            vec![
                508.into(),
                1.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                0.into(),
                2.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            9,
        ),
        (
            510,
            1,
            vec![
                510.into(),
                5.into(),
                1.into(),
                2.into(),
                2.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            5,
        ),
        (
            514,
            1,
            vec![
                514.into(),
                1.into(),
                5.into(),
                2.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
            4,
        ),
    ] {
        let mut source = directory_target(9, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (9, &source)]);
        let record = token_parameter_record(9, values);
        let generic_starts = structural_pointer_group_candidates(&record)
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert!(
            generic_starts.contains(&(expected_start - 1)),
            "Type {entity_type} Form {form}: generic starts {generic_starts:?}"
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.groups.expect("B-rep table boundary").token_start,
            expected_start,
            "Type {entity_type} Form {form}"
        );
    }
}

#[test]
fn type_brep_malformed_count_or_nested_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let property = directory_target(3, 406);
    let cases = [
        (
            502_i64,
            1_i64,
            vec![
                502.into(),
                0.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        ),
        (
            504,
            1,
            vec![
                504.into(),
                (-1_i64).into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        ),
        (
            508,
            1,
            vec![
                508.into(),
                1.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                i64::MAX.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        ),
        (
            510,
            1,
            vec![
                510.into(),
                5.into(),
                TokenValue::String(b"bad".to_vec()),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        ),
        (
            514,
            2,
            vec![
                514.into(),
                0.into(),
                5.into(),
                1.into(),
                1.into(),
                1.into(),
                1.into(),
                3.into(),
            ],
        ),
    ];
    for (entity_type, form, values) in cases {
        let mut source = directory_target(9, entity_type);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (9, &source)]);
        let record = token_parameter_record(9, values);
        assert_eq!(
            entity_primary_end(&record, &directory),
            Some(record.tokens.len()),
            "Type {entity_type} Form {form}"
        );
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "Type {entity_type} Form {form}"
        );
        assert!(analysis.groups.is_none(), "Type {entity_type} Form {form}");
    }
}

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
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        let groups = analysis.groups.expect("analytic surface table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.associations,
            vec![1],
            "Type {entity_type} Form {form}"
        );
        assert_eq!(groups.properties, vec![3], "Type {entity_type} Form {form}");
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
        let valid_starts = structural_pointer_group_candidates(&record)
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(valid_starts, vec![expected_start, expected_start + 1]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        let groups = analysis.groups.expect("analytic surface table boundary");
        assert_eq!(
            groups.token_start, expected_start,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            groups.associations,
            vec![1, 1],
            "Type {entity_type} Form {form}"
        );
        assert_eq!(groups.properties, vec![3], "Type {entity_type} Form {form}");
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
        *wrong.last_mut().expect("primary field") = TokenValue::String(b"bad".to_vec());
        wrong.extend([1.into(), 1.into(), 1.into(), 3.into()]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(7, wrong), &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "Type {entity_type} Form {form}"
        );
        assert_eq!(
            analysis
                .groups
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
            let analysis =
                analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
            assert_eq!(
                analysis.candidate_count, 0,
                "Type {entity_type} Form {form}"
            );
            assert_eq!(
                analysis.valid_candidate_count, 0,
                "Type {entity_type} Form {form}"
            );
            assert!(analysis.groups.is_none(), "Type {entity_type} Form {form}");
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
                TokenValue::Real(0.5),
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
                TokenValue::String(b"3".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(5, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 304 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
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
        let generic = structural_pointer_group_candidates(&record);
        let valid_starts = generic
            .into_iter()
            .filter(|candidate| {
                groups_for_candidate(&record, &form_directory, *candidate)
                    .is_some_and(|groups| groups.fully_valid)
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(
            valid_starts,
            if form == 1 { vec![4, 5] } else { vec![7, 8] },
            "Form {form}"
        );

        let analysis = analyze_trailing_pointer_groups(&record, &form_directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 304 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![3], "Form {form}");
        assert_eq!(groups.properties, vec![5, 5, 5, 5, 5, 5], "Form {form}");
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
                TokenValue::String(b"bad-scale".to_vec()),
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
                TokenValue::Real(0.5),
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
                TokenValue::String(b"bad-length".to_vec()),
                1.into(),
                2.into(),
                1.into(),
                2.into(),
                TokenValue::String(b"3".to_vec()),
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
                TokenValue::String(b"3".to_vec()),
                1.into(),
                1.into(),
            ],
            8,
        ),
    ] {
        let mut source = directory_target(5, 304);
        source.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &property), (5, &source)]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(5, wrong_fields), &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        assert_eq!(
            analysis
                .groups
                .expect("Type 304 wrong-field boundary")
                .token_start,
            expected_start,
            "Form {form}"
        );

        for values in [truncated_primary, truncated_group] {
            let analysis =
                analyze_trailing_pointer_groups(&token_parameter_record(5, values), &directory);
            assert_eq!(analysis.candidate_count, 0, "Form {form}");
            assert_eq!(analysis.valid_candidate_count, 0, "Form {form}");
            assert!(analysis.groups.is_none(), "Form {form}");
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
                TokenValue::String(b"A".to_vec()),
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
                TokenValue::String(b"A".to_vec()),
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
                TokenValue::String(b"A".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(5, values), &directory);
        assert_eq!(
            analysis.candidate_count, 1,
            "expected_start={expected_start}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 1,
            "expected_start={expected_start}"
        );
        let groups = analysis.groups.expect("Type 310 nested boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
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
            TokenValue::String(b"MAIN".to_vec()),
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
    let generic = structural_pointer_group_candidates(&record);
    let valid_starts = generic
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid)
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![22, 23]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 310 table boundary");
    assert_eq!(groups.token_start, 23);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5, 5, 5, 5, 5, 5]);
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
            TokenValue::String(b"MAIN".to_vec()),
            (-9).into(),
            TokenValue::String(b"bad-scale".to_vec()),
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
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(
        analysis
            .groups
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
                TokenValue::String(b"MAIN".to_vec()),
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
                TokenValue::String(b"MAIN".to_vec()),
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
                TokenValue::String(b"MAIN".to_vec()),
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
                TokenValue::String(b"MAIN".to_vec()),
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
                TokenValue::String(b"MAIN".to_vec()),
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
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}
