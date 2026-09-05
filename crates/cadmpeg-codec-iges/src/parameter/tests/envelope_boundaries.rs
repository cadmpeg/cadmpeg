use super::*;

#[test]
fn fixed_envelope_entity_forms_have_registered_primary_boundaries() {
    const PROBE_TOKEN_COUNT: usize = 512;
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let mut sequence = 1_u32;

    for entity in matrix["entity"].as_array().unwrap() {
        let entity_type = entity["type"].as_integer().unwrap();
        let mut forms = entity["forms"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .map(|value| value.as_integer().unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let implementor_defined = entity["forms"].as_str() == Some("implementor-defined")
            || entity
                .get("implementor_defined")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
        if implementor_defined {
            forms.extend([5001, 9999]);
        }

        for form in forms {
            if matches!(entity_type, 0 | 306 | 422) {
                continue;
            }
            let mut entry = directory_target(sequence, entity_type);
            entry.form = form;
            let directory = BTreeMap::from([(sequence, &entry)]);
            let mut values = vec![TokenValue::Integer(0); PROBE_TOKEN_COUNT];
            values[0] = entity_type.into();
            let record = token_parameter_record(sequence, values);
            let primary_end = entity_primary_end(&record, &directory);
            assert!(
                primary_end.is_some_and(|end| end <= record.tokens.len()),
                "missing primary boundary for Type {entity_type} Form {form}"
            );
            sequence += 2;
        }
    }

    for (entity_type, form) in [(0_i64, 0_i64), (306, 0)] {
        let entry = directory_target(sequence, entity_type);
        let directory = BTreeMap::from([(sequence, &entry)]);
        let record = token_parameter_record(
            sequence,
            vec![entity_type.into(), form.into(), 0_i64.into()],
        );
        assert_eq!(
            entity_primary_end(&record, &directory),
            None,
            "Type {entity_type} Form {form} must use its non-table framing"
        );
        sequence += 2;
    }

    for entity_type in [600_i64, 699, 10_000, 99_999] {
        let entry = directory_target(sequence, entity_type);
        let directory = BTreeMap::from([(sequence, &entry)]);
        let record = token_parameter_record(sequence, vec![entity_type.into(), 0_i64.into()]);
        assert_eq!(
            entity_primary_end(&record, &directory),
            None,
            "macro instance Type {entity_type} must retain its definition-dependent stream"
        );
        sequence += 2;
    }

    let mut definition = directory_target(sequence, 322);
    definition.form = 0;
    let definition_record = token_parameter_record(
        sequence,
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
    let definition_sequence = sequence;
    sequence += 2;
    let mut instance = directory_target(sequence, 422);
    instance.form = 0;
    instance.structure = -i64::from(definition_sequence);
    let instance_record = token_parameter_record(
        sequence,
        vec![
            422_i64.into(),
            7_i64.into(),
            8_i64.into(),
            1_i64.into(),
            1_i64.into(),
            1_i64.into(),
            5_i64.into(),
        ],
    );
    let directory = BTreeMap::from([(definition_sequence, &definition), (sequence, &instance)]);
    let records = BTreeMap::from([
        (definition_sequence, &definition_record),
        (sequence, &instance_record),
    ]);
    assert_eq!(
        entity_primary_end_with_records(&instance_record, &directory, &records),
        Some(3)
    );
}

#[test]
fn blank_parameter_field_is_an_omitted_value() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "00010000",
                parameters: "116,1,2,3,   ;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn a_back_pointer_naming_no_other_entry_defers_to_the_declared_range() {
    for pointer in ["       2", "       0", "      99", "     abc"] {
        let mut bytes = owned_test_file(&[OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;".into(),
        }]);
        let marker = bytes
            .windows(8)
            .position(|window| window == b"P      1")
            .expect("Parameter Data card");
        let card_start = marker - 72;
        bytes[card_start + 64..card_start + 72].copy_from_slice(pointer.as_bytes());

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let losses = &result.report().losses;
        assert_eq!(result.ir().model.points.len(), 1, "{pointer}");
        assert_eq!(losses.len(), 1, "{pointer}: {losses:#?}");
        assert_eq!(losses[0].code, IgesLossCode::CardFramingRecovered.kind());
    }
}

#[test]
fn a_parameter_card_count_disagreement_recovers_from_the_card_census() {
    let entity = OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: format!("116,1,2,3,0;{}", "comment".repeat(12)),
    };
    let canonical = owned_test_file(&[entity]);

    for declared in [1, 3] {
        let mut bytes = canonical.clone();
        let marker = bytes
            .windows(8)
            .position(|window| window == b"D      2")
            .expect("second Directory Entry card");
        let card_start = marker - 72;
        bytes[card_start + 24..card_start + 32]
            .copy_from_slice(format!("{declared:>8}").as_bytes());

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let losses = &result.report().losses;
        assert_eq!(result.ir().model.points.len(), 1, "{declared}");
        assert_eq!(losses.len(), 1, "{declared}: {losses:#?}");
        assert_eq!(losses[0].code, IgesLossCode::CardFramingRecovered.kind());
    }
}

#[test]
fn a_back_pointer_naming_another_entry_quarantines_both_records() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00010000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      2")
        .expect("second Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       1");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let losses = &result.report().losses;
    let code = IgesLossCode::ParameterDataQuarantined.kind();
    assert_eq!(native.arenas["quarantined_parameter_records"].len(), 2);
    assert!(result.ir().model.points.is_empty());
    assert_eq!(losses.len(), 2, "{losses:#?}");
    assert!(losses.iter().all(|loss| loss.code == code));
}

#[test]
fn parameter_card_count_includes_comment_card_payload() {
    let comment = "comment".repeat(12);
    let parameters = format!("116,1,2,3,0;{comment}");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "POINT".into(),
                status: "00010000",
                parameters,
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let entity = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];
    let fields = entity.fields();
    assert_eq!(fields["parameter_line_count"], 2);
    let retained_comment = fields["comment"].as_array().unwrap();
    assert_eq!(retained_comment.len(), 128 - "116,1,2,3,0;".len());
    let prefix = retained_comment
        .iter()
        .take(comment.len())
        .map(|value| value.as_u64().unwrap().try_into().unwrap())
        .collect::<Vec<u8>>();
    assert_eq!(prefix, comment.as_bytes());
}

#[test]
fn trailing_pointer_boundary_search_stays_linear_for_ambiguous_suffixes() {
    let token_count: usize = 4096;
    let mut tokens = (0..token_count)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    for index in (1..token_count.saturating_sub(2)).step_by(2) {
        tokens[index].value = TokenValue::Integer(0);
        tokens[index + 1].value = TokenValue::Integer((token_count - index - 3) as i64);
    }
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens,
        parameter_end: token_count,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &BTreeMap::new());
    assert!(analysis.groups().is_none());
}

#[test]
fn field_defaults_do_not_cross_the_selected_parameter_boundary() {
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 1, 2, 1, 9, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 4,
        comment: Vec::new(),
    };

    assert_eq!(record.integer_or(3, 7), Some(1));
    assert_eq!(record.integer_or(4, 7), None);
    assert_eq!(record.number_or(4, 7.0), None);
    assert_eq!(record.string_or_empty(4), None);
    assert_eq!(record.integer_or(99, 7), Some(7));
}

#[test]
fn unique_invalid_trailing_pointer_group_remains_visible() {
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 1, 99, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 4,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &BTreeMap::new());
    assert_eq!(analysis.candidate_count(), 1);
    let groups = analysis.groups().expect("unique structural group");
    assert!(!groups.fully_valid());
    assert_eq!(groups.association_pointers[0].raw_pointer, 99);
    assert!(groups
        .associations()
        .copied()
        .collect::<Vec<_>>()
        .is_empty());
}

#[test]
fn unique_valid_trailing_pointer_group_boundary_wins() {
    let first_association = directory_target(1, 402);
    let second_association = directory_target(3, 402);
    let directory = BTreeMap::from([(1, &first_association), (3, &second_association)]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 0, 0, 2, 3, 1, 0]
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("unique valid group");
    assert!(groups.fully_valid());
    assert_eq!(groups.token_start, 3);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3, 1]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    assert_eq!(groups.association_pointers.len(), 2);
}

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
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 0);
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
        assert_eq!(analysis.candidate_count(), 1, "Form {form}");
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
    assert_eq!(analysis.candidate_count(), 0);
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
        assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
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
        assert_eq!(analysis.candidate_count(), 0);
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
        assert_eq!(analysis.candidate_count(), 1, "N={island_count}");
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
    assert_eq!(analysis.candidate_count(), 1);
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
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

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
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
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
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
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
            analysis.candidate_count(),
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
    assert_eq!(analysis.candidate_count(), 1);
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
        assert_eq!(analysis.candidate_count(), 0);
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
        assert_eq!(analysis.candidate_count(), 1, "Form {form}");
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
    assert_eq!(analysis.candidate_count(), 0);
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
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count(), 0, "values={values:?}");
        assert!(analysis.groups().is_none(), "values={values:?}");
    }

    let mut record = integer_parameter_record(1, &[180, 5, -3, -5, 1, 7, 0]);
    record.tokens[1].value = TokenValue::Real(5.0);
    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 0);
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
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
    assert_eq!(analysis.candidate_count(), 1);
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
        {
            valid_starts.push(candidate.token_start);
        }
    }
    assert_eq!(valid_starts, vec![8, 9]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 202 boundary").token_start, 9);

    for values in [
        vec![202, 1, 0, 0, 0, 0, 2, 3],
        vec![202, 1, 0, 0, 0, 0, 2, 3, 5, 1, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
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
    assert_eq!(analysis.candidate_count(), 1);
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
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![7, 8]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 204 boundary").token_start, 8);

    for values in [
        vec![204, 1, 5, 0, 7, 9, 0],
        vec![204, 1, 5, 0, 7, 9, 0, 0, 1, 3, 1],
    ] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
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
    assert_eq!(analysis.candidate_count(), 1);
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
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 206 boundary").token_start, 6);

    for values in [vec![206, 1, 5, 0, 10], vec![206, 1, 5, 0, 10, 20, 1, 3, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
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
        assert_eq!(analysis.candidate_count(), 1, "form {form}");
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
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
    assert_eq!(analysis.candidate_count(), 1);
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 216 boundary").token_start, 6);

    for values in [vec![216, 1, 3, 5, 0], vec![216, 1, 3, 5, 0, 0, 1, 7, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(11, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
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
    assert_eq!(analysis.candidate_count(), 1);
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
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![3, 4]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        assert_eq!(analysis.groups().expect("Type 220 boundary").token_start, 4);
    }

    for values in [vec![220, 1, 3], vec![220, 1, 3, 5, 1, 9, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(13, &values), &directory);
        assert_eq!(analysis.candidate_count(), 0, "values={values:?}");
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
        assert_eq!(analysis.candidate_count(), 1, "Form {form}");
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
                .is_some_and(|groups| groups.fully_valid())
        })
        .map(|candidate| candidate.token_start)
        .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![4, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count(), 1);
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
            assert_eq!(analysis.candidate_count(), 1, "Form {form}");
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
            analysis.candidate_count(),
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
