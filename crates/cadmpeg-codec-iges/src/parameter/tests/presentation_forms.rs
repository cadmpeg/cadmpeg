use super::*;

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
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::Integer(4),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 32 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3, 3, 3]);
    assert!(groups.properties.is_empty());
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
            TokenValue::Real(3.0),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
            TokenValue::String(b"20260714.123456".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
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
            TokenValue::String(b"JANE".to_vec()),
            TokenValue::String(b"ENG".to_vec()),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"NO".to_vec()),
            TokenValue::String(b"C".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 33 table boundary");
        assert_eq!(groups.token_start, 4);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(5),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 33 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3; 5]);
    assert!(groups.properties.is_empty());
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
            TokenValue::Real(2.0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Integer(2),
            TokenValue::String(b"C".to_vec()),
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
            TokenValue::String(b"C".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form2_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 2 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form2_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 2;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 3, 0, 1, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].token_start, 4);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[406, 3, 0, 1, 2, 1, 3, 0]);
    record.tokens[1].value = TokenValue::Real(3.0);
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"POWER".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 3 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type406_form3_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 3;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 2, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"POWER".to_vec()),
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
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form8_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for pin_number in [TokenValue::String(b"PA7".to_vec()), TokenValue::Integer(17)] {
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 8 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type406_form8_table_boundary_precedes_generic_candidate() {
    let mut source = directory_target(1, 406);
    source.form = 8;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    let record = integer_parameter_record(1, &[406, 1, 1, 3, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"PA7".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(406), TokenValue::Integer(1)],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form9_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 9;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for first_number in [
        TokenValue::String(b"GENERIC".to_vec()),
        TokenValue::Integer(1),
    ] {
        let record = token_parameter_record(
            1,
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(4),
                first_number,
                TokenValue::String(b"MIL123".to_vec()),
                TokenValue::String(b"VEND42".to_vec()),
                TokenValue::String(b"INT99".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 9 table boundary");
        assert_eq!(groups.token_start, 6);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"GENERIC".to_vec()),
            TokenValue::String(b"MIL123".to_vec()),
            TokenValue::String(b"VEND42".to_vec()),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
        TokenValue::String(b"1".to_vec()),
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 10 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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

    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let record = token_parameter_record(1, values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
                TokenValue::Real(2.5),
                TokenValue::String(b"AWG".to_vec()),
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
                TokenValue::Real(2.5),
                TokenValue::String(b"AWG".to_vec()),
                TokenValue::String(b"ANSI123".to_vec()),
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
                TokenValue::String(b"2HNO".to_vec()),
                TokenValue::String(b"AWG".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            4,
        ),
    ] {
        assert_eq!(values[1], TokenValue::Integer(np));
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
            TokenValue::Real(2.5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let generic =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
        let expected_generic_start = if values[1] == TokenValue::Integer(2) {
            3
        } else {
            4
        };
        assert!(generic
            .iter()
            .any(|candidate| candidate.token_start == expected_generic_start));

        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(2.5),
            TokenValue::String(b"AWG".to_vec()),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

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
                TokenValue::String(b"FLOW".to_vec()),
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
                TokenValue::String(b"FLOW".to_vec()),
                TokenValue::String(b"MOD".to_vec()),
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
                TokenValue::String(b"FLOW".to_vec()),
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
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 14 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
        TokenValue::String(b"FLOW".to_vec()),
        TokenValue::String(b"MOD".to_vec()),
        TokenValue::Integer(5),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let record = token_parameter_record(1, values);
    let generic = structural_pointer_group_candidates(&record);
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 406 Form 14 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations, vec![3; 5]);
    assert!(groups.properties.is_empty());
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
            TokenValue::Real(1.0),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(0),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(-1),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::String(b"MOD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"FLOW".to_vec()),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"FLOW".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
    ] {
        let record = token_parameter_record(1, values);
        let generic_count = structural_pointer_group_candidates(&record).len();
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type406_form15_entity_table_boundary_follows_fixed_values() {
    let mut source = directory_target(1, 406);
    source.form = 15;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for name in [
        TokenValue::String(b"USERNM".to_vec()),
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 15 table boundary");
        assert_eq!(groups.token_start, 3);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 2));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"USERNM".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(1),
            TokenValue::String(b"USERNM".to_vec()),
        ],
        vec![TokenValue::Integer(406), TokenValue::Integer(1)],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec()),
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
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"SIGNAL_T".to_vec()),
                TokenValue::Integer(20),
                TokenValue::String(b"CORE".to_vec()),
                TokenValue::Integer(0),
                TokenValue::String(b"UNDEFINED".to_vec()),
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
                TokenValue::String(b"TOP1".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 24 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
        TokenValue::String(b"TOP1".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 6));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"TOP1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::String(b"SIGNAL_T".to_vec()),
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
            TokenValue::String(b"TOP1".to_vec()),
            TokenValue::Integer(1),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
                TokenValue::String(b"BOARD".to_vec()),
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
                TokenValue::String(b"BOARD".to_vec()),
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
                TokenValue::String(b"BOARD".to_vec()),
                TokenValue::Integer(1),
                TokenValue::String(b"1HX".to_vec()),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            5,
        ),
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 25 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
        TokenValue::String(b"BOARD".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BOARD".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(10),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::String(b"BAD".to_vec()),
            TokenValue::Real(0.7),
            TokenValue::Integer(6),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 26 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
        TokenValue::Real(0.8),
        TokenValue::Real(0.7),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Omitted,
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(3),
            TokenValue::Real(0.8),
            TokenValue::Real(0.7),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
            TokenValue::String(b"MM".to_vec()),
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
            TokenValue::String(b"MM".to_vec()),
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
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(2),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 28 table boundary");
        assert_eq!(groups.token_start, 8);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
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
        TokenValue::String(b"MM".to_vec()),
        TokenValue::Integer(0),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
        TokenValue::Integer(0),
    ];
    let generic = structural_pointer_group_candidates(&token_parameter_record(1, values.clone()));
    assert!(generic.iter().any(|candidate| candidate.token_start == 7));

    let analysis = analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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
            TokenValue::String(b"MM".to_vec()),
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
            TokenValue::String(b"MM".to_vec()),
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
            TokenValue::String(b"MM".to_vec()),
            TokenValue::Integer(0),
        ],
    ] {
        let generic_count =
            structural_pointer_group_candidates(&token_parameter_record(1, values.clone())).len();
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 0, "generic_count={generic_count}");
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
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
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
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
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
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
            TokenValue::String(b"2".to_vec()),
            TokenValue::Integer(2),
            TokenValue::Real(0.1),
            TokenValue::Real(-0.1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(3),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(1, values), &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 406 Form 29 table boundary");
        assert_eq!(groups.token_start, 10);
        assert_eq!(groups.associations, vec![3]);
        assert!(groups.properties.is_empty());
    }
}
