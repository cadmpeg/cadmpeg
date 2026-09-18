use super::{directory_target, token_parameter_record};
use crate::loss::IgesLossCode;
use crate::parameter::{
    analyze_trailing_pointer_groups, entity_primary_end, entity_primary_end_with_records,
    ParameterRecord, Token, TokenValue,
};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::Codec;
use std::collections::BTreeMap;
use std::io::Cursor;
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
    assert_eq!(native.arenas()["quarantined_parameter_records"].len(), 2);
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

    let entity = &result.ir().native.namespace("iges").unwrap().arenas()["entities"][0];
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
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &BTreeMap::new())),
        1
    );
    let groups = analysis.groups().expect("unique structural group");
    assert!(groups.clone().fully_valid().is_none());
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
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("unique valid group");
    assert_eq!(groups.token_start, 3);
    assert_eq!(
        groups.associations().copied().collect::<Vec<_>>(),
        vec![3, 1]
    );
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    assert_eq!(groups.association_pointers.len(), 2);
}
