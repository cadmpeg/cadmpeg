use super::{directory_target, integer_parameter_record};
use crate::parameter::{
    analyze_trailing_pointer_groups, entity_primary_end, structural_pointer_group_candidates,
    ParameterRecord, Token, TokenValue,
};
use std::collections::BTreeMap;
#[test]
fn type123_entity_table_boundary_precedes_a_valid_generic_alternative() {
    let mut association = directory_target(1, 402);
    association.form = 7;
    let direction = directory_target(3, 123);
    let directory = BTreeMap::from([(1, &association), (3, &direction)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [123, 0, 0, 2, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 123 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type110_entity_table_boundary_precedes_valid_generic_alternatives() {
    let first_association = directory_target(1, 402);
    let second_association = directory_target(3, 402);
    let line = directory_target(5, 110);
    let directory = BTreeMap::from([
        (1, &first_association),
        (3, &second_association),
        (5, &line),
    ]);
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [110, 7, 3, 3, 1, 3, 3, 1, 3, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 10,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 110 table boundary");
    assert_eq!(groups.token_start, 7);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type102_entity_table_boundary_follows_declared_child_count() {
    let association = directory_target(1, 402);
    let first_child = directory_target(3, 110);
    let second_child = directory_target(5, 110);
    let composite = directory_target(7, 102);
    let directory = BTreeMap::from([
        (1, &association),
        (3, &first_child),
        (5, &second_child),
        (7, &composite),
    ]);
    let record = ParameterRecord {
        directory_sequence: 7,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [102, 2, 3, 5, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 102 table boundary");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type102_malformed_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 402);
    let composite = directory_target(3, 102);
    let directory = BTreeMap::from([(1, &association), (3, &composite)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(102),
                span: 0..0,
            },
            Token {
                value: TokenValue::Omitted,
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type402_group_entity_table_boundary_precedes_valid_generic_alternatives() {
    for form in [1_i64, 7, 14, 15] {
        let mut group = directory_target(1, 402);
        group.form = form;
        let member = directory_target(3, 116);
        let second_member = directory_target(5, 402);
        let trailing_association = directory_target(7, 402);
        let directory = BTreeMap::from([
            (1, &group),
            (3, &member),
            (5, &second_member),
            (7, &trailing_association),
        ]);
        let record = ParameterRecord {
            directory_sequence: 1,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: [402, 2, 3, 5, 1, 7, 0]
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 7,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 402 table boundary");
        assert_eq!(groups.token_start, 4, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![7],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
    }
}

#[test]
fn type402_malformed_member_count_does_not_enable_generic_recovery() {
    let mut group = directory_target(1, 402);
    group.form = 7;
    let member = directory_target(3, 116);
    let second_member = directory_target(5, 402);
    let trailing_association = directory_target(7, 402);
    let directory = BTreeMap::from([
        (1, &group),
        (3, &member),
        (5, &second_member),
        (7, &trailing_association),
    ]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: vec![
            Token {
                value: TokenValue::Integer(402),
                span: 0..0,
            },
            Token {
                value: TokenValue::Omitted,
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(3),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(5),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(1),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(7),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type402_single_parent_boundary_follows_child_count() {
    for (values, expected_start) in [
        (vec![402, 1, 1, 3, 5, 1, 9, 0], 5),
        (vec![402, 1, 2, 3, 5, 7, 1, 9, 0], 6),
    ] {
        let mut source = directory_target(1, 402);
        source.form = 9;
        let parent = directory_target(3, 212);
        let child = directory_target(5, 212);
        let second_child = directory_target(7, 212);
        let trailing_association = directory_target(9, 212);
        let directory = BTreeMap::from([
            (1, &source),
            (3, &parent),
            (5, &child),
            (7, &second_child),
            (9, &trailing_association),
        ]);
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 1,
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 402 Form 9 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_single_parent_boundary_precedes_valid_generic_alternative() {
    let mut source = directory_target(5, 402);
    source.form = 9;
    let parent = directory_target(1, 212);
    let child = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &parent), (3, &child), (5, &source)]);
    let values = [402, 1, 2, 1, 1, 2, 1, 3, 0];
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
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 9 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_single_parent_malformed_counts_do_not_enable_generic_recovery() {
    let mut source = directory_target(5, 402);
    source.form = 9;
    let target = directory_target(1, 212);
    let directory = BTreeMap::from([(1, &target), (5, &source)]);
    let cases = [
        vec![402, 0, 1, 1, 1, 1, 1, 0],
        vec![402, 1, 0, 1, 1, 1, 1, 0],
        vec![402, 1, -1, 1, 1, 1, 1, 0],
        vec![402, 1, 100, 1, 1, 1, 1, 0],
        vec![402, 1, 1],
    ];

    for values in cases {
        let parameter_end = values.len();
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
            parameter_end,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type230_entity_table_boundary_follows_island_count() {
    for (island_count, expected_start) in [(0_i64, 9), (1, 10), (2, 11)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 230);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 230;
        values[1] = 1;
        values[2] = 2;
        values[8] = island_count;
        for index in 0..usize::try_from(island_count).unwrap() {
            values[9 + index] = 1;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "N={island_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "N={island_count}");
        let groups = analysis.groups().expect("Type 230 table boundary");
        assert_eq!(groups.token_start, expected_start, "N={island_count}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "N={island_count}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "N={island_count}"
        );
    }
}

#[test]
fn type230_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 230);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [230, 7, 2, 0, 0, 0, 1, 0, 1, 2, 1, 3, 0];
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
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 230 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type230_malformed_island_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 230);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![230, 1, 2, 0, 0, 0, 1, 0, -1, 1, 5, 0],
        vec![230, 1, 2, 0, 0, 0, 1, 0, 100, 1, 5, 0],
        vec![230, 1, 2],
        vec![230, 1, 2, 0, 0, 0, 1, 0, 2, 1],
    ];

    for values in cases {
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type320_entity_table_boundary_follows_member_and_connect_counts() {
    for (member_count, connect_count, expected_start) in
        [(0_i64, 0_i64, 8), (1, 0, 9), (0, 1, 9), (2, 1, 11)]
    {
        let association = directory_target(1, 212);
        let member = directory_target(3, 132);
        let connect_point = directory_target(5, 132);
        let mut source = directory_target(7, 320);
        source.form = 0;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &member),
            (5, &connect_point),
            (7, &source),
        ]);
        let member_count = usize::try_from(member_count).unwrap();
        let connect_count = usize::try_from(connect_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 320;
        values[3] = i64::try_from(member_count).unwrap();
        for index in 0..member_count {
            values[4 + index] = 3;
        }
        values[4 + member_count] = 0;
        values[5 + member_count] = 0;
        values[6 + member_count] = 0;
        values[7 + member_count] = i64::try_from(connect_count).unwrap();
        for index in 0..connect_count {
            values[8 + member_count + index] = 5;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let parameter_end = values.len();
        let record = ParameterRecord {
            directory_sequence: 7,
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "NA={member_count}, NC={connect_count}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "NA={member_count}, NC={connect_count}"
        );
        let groups = analysis.groups().expect("Type 320 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type320_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 320);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [320, 0, 0, 1, 1, 0, 0, 0, 1, 2, 1, 3, 0];
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
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 320 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type320_malformed_counts_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_5 = directory_target(5, 212);
    let source = directory_target(3, 320);
    let directory = BTreeMap::from([(1, &target_1), (3, &source), (5, &target_5)]);
    let cases = [
        vec![320, 0, 0, -1, 1, 0, 0, 1, 5, 1, 5, 0],
        vec![320, 0, 0, 100, 1, 0, 0, 1, 5, 1, 5, 0],
        vec![320, 0, 0, 1, 1, 0, 0],
        vec![320, 0, 0, 1, 1, 0, 0, -1, 1, 5, 0],
        vec![320, 0, 0, 1, 1, 0, 0, 2, 5],
    ];

    for values in cases {
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
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type180_forms_share_postorder_boundary() {
    for form in [0_i64, 1] {
        let mut source = directory_target(1, 180);
        source.form = form;
        let operand = directory_target(3, if form == 1 { 186 } else { 158 });
        let association = directory_target(7, 212);
        let directory = BTreeMap::from([(1, &source), (3, &operand), (7, &association)]);
        let record = integer_parameter_record(1, &[180, 3, -3, -5, 1, 1, 7, 0]);

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 180 table boundary");
        assert_eq!(groups.token_start, 5, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![7],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
    }
}

#[test]
fn type180_table_boundary_precedes_generic_candidate() {
    let source = directory_target(1, 180);
    let association = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &source), (7, &association)]);
    let record = integer_parameter_record(1, &[180, 5, -3, -5, 1, -3, 1, 7, 0]);

    let generic = structural_pointer_group_candidates(&record);
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].token_start, 6);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type180_malformed_length_or_terms_do_not_enable_generic_recovery() {
    let source = directory_target(1, 180);
    let association = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &source), (7, &association)]);
    for values in [
        vec![180, 2, -3, -5, 1, 7, 0],
        vec![180, 0, -3, -5, 1, 7, 0],
        vec![180, -1, -3, -5, 1, 7, 0],
        vec![180, 5, -3, -5, 1],
    ] {
        let record = integer_parameter_record(1, &values);
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0,
            "values={values:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[180, 5, -3, -5, 1, 7, 0]);
    record.tokens[1].value = TokenValue::Real(5.0);
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}
