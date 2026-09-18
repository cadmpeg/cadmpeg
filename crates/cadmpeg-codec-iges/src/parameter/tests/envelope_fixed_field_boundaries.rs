use super::{integer_parameter_record, token_parameter_record};
use crate::parameter::{
    analyze_trailing_pointer_groups, entity_primary_end, groups_for_candidate,
    structural_pointer_group_candidates, TokenValue,
};
use crate::test_support::directory_target;
use std::collections::BTreeMap;
#[test]
fn type132_fixed_primary_boundary_follows_fourteen_fields() {
    let association = directory_target(3, 212);
    let mut source = directory_target(7, 132);
    source.form = 0;
    let directory = BTreeMap::from([(3, &association), (7, &source)]);
    let record = token_parameter_record(
        7,
        vec![
            132.into(),
            1.0.into(),
            2.0.into(),
            3.0.into(),
            0.into(),
            101.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 132 table boundary");
    assert_eq!(groups.token_start, 15);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type132_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(9, 132);
    source.form = 0;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (9, &source),
    ]);
    let record = token_parameter_record(
        9,
        vec![
            132.into(),
            1.0.into(),
            2.0.into(),
            3.0.into(),
            0.into(),
            101.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            2.into(),
            1.into(),
            3.into(),
            2.into(),
            5.into(),
            7.into(),
        ],
    );

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(
        generic
            .iter()
            .map(|candidate| candidate.token_start)
            .collect::<Vec<_>>(),
        vec![14, 15]
    );
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 132 table boundary");
    assert_eq!(groups.token_start, 15);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5, 7]);
}

#[test]
fn type132_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let mut source = directory_target(7, 132);
    source.form = 0;
    let directory = BTreeMap::from([(3, &association), (7, &source)]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            132.into(),
            TokenValue::String(b"bad".to_vec()),
            2.into(),
            3.into(),
            0.into(),
            999.into(),
            1.into(),
            TokenValue::String(b"C1".to_vec()),
            0.into(),
            TokenValue::String(b"PORT".to_vec()),
            0.into(),
            42.into(),
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 132 table boundary")
            .token_start,
        15
    );

    for values in [
        vec![132, 1, 2, 3, 0, 101, 1, 0, 0, 0, 0, 42, 1, 0],
        vec![132, 1, 2, 3, 0, 101, 1, 0, 0, 0, 0, 42, 1, 0, 0, 1, 3],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(7, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(7, &values),
                entity_primary_end(&integer_parameter_record(7, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type202_form0_boundary_follows_eight_primary_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 202);
    source.form = 0;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[202, 1, 0, 0, 0, 0, 2, 3, 5, 1, 1, 1, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 202 table boundary");
    assert_eq!(groups.token_start, 9);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn type202_form0_boundary_precedes_generic_candidate() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let mut source = directory_target(9, 202);
    source.form = 0;
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[202, 1, 0, 0, 0, 0, 2, 3, 2, 1, 1, 1, 5]);

    let generic = structural_pointer_group_candidates(&record);
    let mut valid_starts = Vec::new();
    for candidate in generic {
        if groups_for_candidate(&record, &directory, candidate)
            .expect("generic Type 202 candidate")
            .fully_valid()
            .is_some()
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 202 table boundary")
            .token_start,
        9
    );
}

#[test]
fn type202_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 202);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            202.into(),
            1.into(),
            0.into(),
            0.into(),
            TokenValue::String(b"bad-x".to_vec()),
            TokenValue::Omitted,
            2.into(),
            TokenValue::String(b"bad-leader".to_vec()),
            5.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 202 boundary").token_start, 9);

    for values in [
        vec![202, 1, 0, 0, 0, 0, 2, 3],
        vec![202, 1, 0, 0, 0, 0, 2, 3, 5, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(9, &values),
                entity_primary_end(&integer_parameter_record(9, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type204_form0_follows_seven_fixed_primary_fields() {
    let association = directory_target(3, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 204);
    let directory = BTreeMap::from([(3, &association), (11, &property), (13, &source)]);
    let record = integer_parameter_record(13, &[204, 1, 5, 0, 7, 9, 0, 0, 1, 3, 1, 11]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 204 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type204_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 204);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[204, 1, 5, 0, 7, 9, 0, 2, 1, 3, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![7, 8]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 204 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type204_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 204);
    let directory = BTreeMap::from([(3, &association), (11, &property), (13, &source)]);
    let wrong_fields = token_parameter_record(
        13,
        vec![
            204.into(),
            TokenValue::Real(1.5),
            5.into(),
            TokenValue::Omitted,
            7.into(),
            9.into(),
            0.into(),
            0.into(),
            1.into(),
            3.into(),
            1.into(),
            11.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 204 boundary").token_start, 8);

    for values in [
        vec![204, 1, 5, 0, 7, 9, 0],
        vec![204, 1, 5, 0, 7, 9, 0, 0, 1, 3, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(13, &values),
                entity_primary_end(&integer_parameter_record(13, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type206_form0_follows_five_fixed_primary_fields() {
    let association = directory_target(3, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 206);
    let directory = BTreeMap::from([(3, &association), (9, &property), (11, &source)]);
    let record = integer_parameter_record(11, &[206, 1, 5, 0, 10, 20, 1, 3, 1, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 206 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![9]);
}

#[test]
fn type206_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let second_association = directory_target(3, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 206);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (9, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[206, 1, 5, 0, 10, 2, 1, 3, 1, 9]);
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
    let groups = analysis.groups().expect("Type 206 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![9]);
}

#[test]
fn type206_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(3, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 206);
    let directory = BTreeMap::from([(3, &association), (9, &property), (11, &source)]);
    let wrong_fields = token_parameter_record(
        11,
        vec![
            206.into(),
            TokenValue::Real(1.5),
            5.into(),
            0.into(),
            10.into(),
            20.into(),
            1.into(),
            3.into(),
            1.into(),
            9.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 206 boundary").token_start, 6);

    for values in [vec![206, 1, 5, 0, 10], vec![206, 1, 5, 0, 10, 20, 1, 3, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(11, &values),
                entity_primary_end(&integer_parameter_record(11, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type216_forms_share_five_fixed_primary_field_boundary() {
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);
    for form in 0..=2 {
        let mut source = directory_target(11, 216);
        source.form = form;
        let directory = BTreeMap::from([(7, &association), (9, &property), (11, &source)]);
        let record = integer_parameter_record(11, &[216, 1, 3, 5, 0, 0, 1, 7, 1, 9]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "form {form}");
        let groups = analysis.groups().expect("Type 216 table boundary");
        assert_eq!(groups.token_start, 6, "form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![7],
            "form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![9],
            "form {form}"
        );
    }
}

#[test]
fn type216_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);
    let mut source = directory_target(11, 216);
    source.form = 2;
    let directory = BTreeMap::from([
        (1, &first_association),
        (7, &association),
        (9, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[216, 1, 3, 5, 0, 2, 1, 7, 1, 9]);
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
    let groups = analysis.groups().expect("Type 216 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![9]);
}

#[test]
fn type216_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);
    let mut source = directory_target(11, 216);
    source.form = 1;
    let directory = BTreeMap::from([(7, &association), (9, &property), (11, &source)]);
    let wrong_fields = token_parameter_record(
        11,
        vec![
            216.into(),
            TokenValue::Real(1.5),
            3.into(),
            5.into(),
            0.into(),
            TokenValue::Omitted,
            1.into(),
            7.into(),
            1.into(),
            9.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_fields, &directory);
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 216 boundary").token_start, 6);

    for values in [vec![216, 1, 3, 5, 0], vec![216, 1, 3, 5, 0, 0, 1, 7, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(11, &values),
                entity_primary_end(&integer_parameter_record(11, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type220_form0_follows_three_fixed_primary_fields() {
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 220);
    let directory = BTreeMap::from([(9, &association), (11, &property), (13, &source)]);
    let record = integer_parameter_record(13, &[220, 1, 3, 0, 1, 9, 1, 11]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 220 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type220_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 220);
    let directory = BTreeMap::from([
        (1, &first_association),
        (9, &association),
        (11, &property),
        (13, &source),
    ]);
    let record = integer_parameter_record(13, &[220, 1, 3, 2, 1, 9, 1, 11]);
    let valid_starts = structural_pointer_group_candidates(&record)
        .into_iter()
        .filter(|candidate| {
            groups_for_candidate(&record, &directory, *candidate)
                .is_some_and(|groups| groups.fully_valid().is_some())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![3, 4]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 220 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![11]);
}

#[test]
fn type220_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(9, 212);
    let property = directory_target(11, 406);
    let source = directory_target(13, 220);
    let directory = BTreeMap::from([(9, &association), (11, &property), (13, &source)]);
    let wrong_field = token_parameter_record(
        13,
        vec![
            220.into(),
            TokenValue::Real(1.5),
            3.into(),
            5.into(),
            1.into(),
            9.into(),
            1.into(),
            11.into(),
        ],
    );
    let wrong_value = integer_parameter_record(13, &[220, 1, 3, 99, 1, 9, 1, 11]);
    for record in [&wrong_field, &wrong_value] {
        let analysis = analyze_trailing_pointer_groups(record, &directory);
        assert_eq!(
            analysis.candidate_count(record, entity_primary_end(record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(analysis.groups().expect("Type 220 boundary").token_start, 4);
    }

    for values in [vec![220, 1, 3], vec![220, 1, 3, 5, 1, 9, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(13, &values),
                entity_primary_end(&integer_parameter_record(13, &values), &directory)
            ),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }
}

#[test]
fn type222_forms_follow_fixed_primary_fields() {
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);

    for (form, values, expected_start) in [
        (0, vec![222, 1, 3, 10, 20, 1, 7, 1, 9], 5_usize),
        (1, vec![222, 1, 3, 10, 20, 0, 1, 7, 1, 9], 6_usize),
        (1, vec![222, 1, 3, 10, 20, 5, 1, 7, 1, 9], 6_usize),
    ] {
        let mut source = directory_target(11, 222);
        source.form = form;
        let directory = BTreeMap::from([(7, &association), (9, &property), (11, &source)]);
        let record = integer_parameter_record(11, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 222 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![7],
            "Form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![9],
            "Form {form}"
        );
    }
}

#[test]
fn type222_form0_table_boundary_precedes_valid_generic_alternative() {
    let first_association = directory_target(1, 212);
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);
    let source = directory_target(11, 222);
    let directory = BTreeMap::from([
        (1, &first_association),
        (7, &association),
        (9, &property),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[222, 1, 3, 10, 2, 1, 7, 1, 9]);
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
    let groups = analysis.groups().expect("Type 222 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![9]);
}

#[test]
fn type222_complete_wrong_fields_keep_boundaries_and_truncated_spans_do_not_recover() {
    let association = directory_target(7, 212);
    let property = directory_target(9, 406);

    for (form, records, expected_start) in [
        (
            0,
            vec![
                token_parameter_record(
                    11,
                    vec![
                        222.into(),
                        TokenValue::Real(1.5),
                        3.into(),
                        10.into(),
                        20.into(),
                        1.into(),
                        7.into(),
                        1.into(),
                        9.into(),
                    ],
                ),
                integer_parameter_record(11, &[222, 1, 99, 10, 20, 1, 7, 1, 9]),
            ],
            5_usize,
        ),
        (
            1,
            vec![
                token_parameter_record(
                    11,
                    vec![
                        222.into(),
                        1.into(),
                        3.into(),
                        10.into(),
                        20.into(),
                        TokenValue::Real(5.5),
                        1.into(),
                        7.into(),
                        1.into(),
                        9.into(),
                    ],
                ),
                integer_parameter_record(11, &[222, 1, 3, 10, 20, 99, 1, 7, 1, 9]),
            ],
            6_usize,
        ),
    ] {
        let mut source = directory_target(11, 222);
        source.form = form;
        let directory = BTreeMap::from([(7, &association), (9, &property), (11, &source)]);
        for record in &records {
            let analysis = analyze_trailing_pointer_groups(record, &directory);
            assert_eq!(
                analysis.candidate_count(record, entity_primary_end(record, &directory)),
                1,
                "Form {form}"
            );
            assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
            assert_eq!(
                analysis
                    .groups()
                    .expect("Type 222 fixed boundary")
                    .token_start,
                expected_start,
                "Form {form}"
            );
        }
    }

    for (form, values) in [
        (0, vec![222, 1, 3]),
        (0, vec![222, 1, 3, 10, 20, 1, 7, 1]),
        (1, vec![222, 1, 3, 10, 20]),
        (1, vec![222, 1, 3, 10, 20, 0, 1, 7, 1]),
    ] {
        let mut source = directory_target(11, 222);
        source.form = form;
        let directory = BTreeMap::from([(7, &association), (9, &property), (11, &source)]);
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(11, &values),
                entity_primary_end(&integer_parameter_record(11, &values), &directory)
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
