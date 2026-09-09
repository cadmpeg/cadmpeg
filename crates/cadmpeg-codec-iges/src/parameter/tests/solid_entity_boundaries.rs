use super::*;

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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![12, 13]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![13, 14]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![9, 10]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![12, 13]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0
        );
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
            assert_eq!(
                analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
                1
            );
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
                    .is_some_and(|groups| groups.fully_valid().is_some())
            })
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>();
        assert_eq!(valid_starts, vec![8, 9]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
        assert_eq!(
            analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
            1
        );
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
            assert_eq!(
                analysis.candidate_count(
                    &integer_parameter_record(9, &values),
                    entity_primary_end(&integer_parameter_record(9, &values), &directory)
                ),
                0
            );
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
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
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
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
    assert_eq!(
        analysis.candidate_count(&wrong, entity_primary_end(&wrong, &directory)),
        1
    );
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
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(9, &values),
                entity_primary_end(&integer_parameter_record(9, &values), &directory)
            ),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}
