use super::*;

#[test]
fn type430_entity_table_boundary_follows_solid_pointer_for_both_forms() {
    let association = directory_target(1, 212);
    let property = directory_target(7, 406);

    for (form, target_type) in [(0_i64, 158_i64), (1, 186)] {
        let target = directory_target(5, target_type);
        let mut source = directory_target(9, 430);
        source.form = form;
        let directory = BTreeMap::from([
            (1, &association),
            (5, &target),
            (7, &property),
            (9, &source),
        ]);
        let analysis = analyze_trailing_pointer_groups(
            &integer_parameter_record(9, &[430, 5, 1, 1, 1, 7]),
            &directory,
        );
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 430 table boundary");
        assert_eq!(groups.token_start, 2, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert_eq!(groups.properties, vec![7], "Form {form}");
    }
}

#[test]
fn type430_entity_table_boundary_suppresses_generic_recovery_for_malformed_span() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let target = directory_target(5, 158);
    let property = directory_target(7, 406);
    let source = directory_target(9, 430);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &target),
        (7, &property),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[430, 5, 3, 1, 3, 1, 1, 1, 7]);
    let generic = structural_pointer_group_candidates(&record);
    let generic_candidate = generic
        .iter()
        .find(|candidate| candidate.token_start == 1)
        .copied()
        .expect("generic recovery candidate");
    assert!(
        groups_for_candidate(&record, &directory, generic_candidate)
            .expect("generic candidate groups")
            .fully_valid
    );

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
}

#[test]
fn type430_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(7, 406);
    let target = directory_target(5, 158);
    let source = directory_target(9, 430);
    let directory = BTreeMap::from([
        (1, &association),
        (5, &target),
        (7, &property),
        (9, &source),
    ]);
    let wrong_fields = token_parameter_record(
        9,
        vec![
            430.into(),
            TokenValue::String(b"BAD".to_vec()),
            1.into(),
            1.into(),
            1.into(),
            7.into(),
        ],
    );
    let omitted_pointer = token_parameter_record(
        9,
        vec![
            430.into(),
            TokenValue::Omitted,
            1.into(),
            1.into(),
            1.into(),
            7.into(),
        ],
    );
    for record in [wrong_fields, omitted_pointer] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        assert_eq!(
            analysis
                .groups
                .expect("Type 430 complete boundary")
                .token_start,
            2
        );
    }

    for record in [
        integer_parameter_record(9, &[430]),
        integer_parameter_record(9, &[430, 5, 1, 1]),
    ] {
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn legacy_type402_primary_boundaries_follow_their_counted_classes() {
    let cases = [
        (
            8,
            vec![
                402.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                TokenValue::String(b"NET".to_vec()),
            ],
            6,
        ),
        (
            10,
            vec![
                402.into(),
                1.into(),
                1.into(),
                5.into(),
                1.into(),
                2.into(),
                1.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
            11,
        ),
        (
            11,
            vec![
                402.into(),
                1.into(),
                2.into(),
                5.into(),
                7.into(),
                42.into(),
            ],
            6,
        ),
    ];
    for (form, values, expected_end) in cases {
        let mut source = directory_target(9, 402);
        source.form = form;
        let directory = BTreeMap::from([(9, &source)]);
        let record = token_parameter_record(9, values);
        assert_eq!(entity_primary_end(&record, &directory), Some(expected_end));
    }
}
