use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::ParameterRecord;
use crate::parameter::Token;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::Codec;
use std::collections::BTreeMap;
use std::io::Cursor;

#[test]
fn type126_entity_table_boundary_uses_k_and_degree() {
    for (form, k, degree) in [(0_i64, 0_i64, 0_i64), (0, 1, 1), (3, 2, 1), (5, 3, 2)] {
        let association = directory_target(1, 402);
        let mut curve = directory_target(3, 126);
        curve.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &curve)]);
        let expected_start =
            18 + usize::try_from(k).unwrap() * 5 + usize::try_from(degree).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 126;
        values[1] = k;
        values[2] = degree;
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "Form {form}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis.groups().expect("Type 126 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "Form {form}"
        );
        assert!(
            groups.properties().copied().collect::<Vec<_>>().is_empty(),
            "Form {form}"
        );
    }
}

#[test]
fn type126_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_21 = directory_target(21, 402);
    let target_23 = directory_target(23, 402);
    let curve = directory_target(25, 126);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (21, &target_21),
        (23, &target_23),
        (25, &curve),
    ]);
    let record = ParameterRecord {
        directory_sequence: 25,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [
            126, 1, 1, 0, 0, 1, 0, 18, 21, 23, 23, 21, 21, 21, 21, 21, 23, 21, 21, 21, 23, 1, 1, 1,
            1, 7, 0,
        ]
        .into_iter()
        .map(|value| Token {
            value: TokenValue::Integer(value),
            span: 0..0,
        })
        .collect(),
        parameter_end: 27,
        comment: Vec::new(),
    };

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 126 table boundary");
    assert_eq!(groups.token_start, 24);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type126_malformed_k_or_degree_does_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_21 = directory_target(21, 402);
    let target_23 = directory_target(23, 402);
    let mut curve = directory_target(25, 126);
    curve.form = 0;
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (21, &target_21),
        (23, &target_23),
        (25, &curve),
    ]);
    for (k, degree) in [(0_i64, 1_i64), (-1, 0), (1, -1)] {
        let mut values = [
            126, 1, 1, 0, 0, 1, 0, 18, 21, 23, 23, 21, 21, 21, 21, 21, 23, 21, 21, 21, 23, 1, 1, 1,
            1, 7, 0,
        ];
        values[1] = k;
        values[2] = degree;
        let record = ParameterRecord {
            directory_sequence: 25,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 27,
            comment: Vec::new(),
        };

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0,
            "K={k}, M={degree}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "K={k}, M={degree}");
        assert!(analysis.groups().is_none(), "K={k}, M={degree}");
    }
}

#[test]
fn type112_entity_table_boundary_uses_segment_count() {
    for (segment_count, expected_start) in [(1_i64, 31_usize), (2, 44)] {
        let association = directory_target(1, 402);
        let spline = directory_target(3, 112);
        let directory = BTreeMap::from([(1, &association), (3, &spline)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 112;
        values[1] = 3;
        values[2] = 0;
        values[3] = 3;
        values[4] = segment_count;
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 112 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type112_entity_table_boundary_precedes_valid_generic_alternatives() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_15 = directory_target(15, 402);
    let target_17 = directory_target(17, 402);
    let target_39 = directory_target(39, 402);
    let spline = directory_target(41, 112);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (15, &target_15),
        (17, &target_17),
        (39, &target_39),
        (41, &spline),
    ]);
    let record = ParameterRecord {
        directory_sequence: 41,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [
            112, 3, 0, 3, 1, 1, 3, 25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 39, 17, 7, 1, 15, 17, 7, 1,
            15, 17, 7, 1, 1, 1, 0,
        ]
        .into_iter()
        .map(|value| Token {
            value: TokenValue::Integer(value),
            span: 0..0,
        })
        .collect(),
        parameter_end: 34,
        comment: Vec::new(),
    };

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 112 table boundary");
    assert_eq!(groups.token_start, 31);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type112_malformed_segment_count_does_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 402);
    let target_7 = directory_target(7, 402);
    let target_15 = directory_target(15, 402);
    let target_17 = directory_target(17, 402);
    let target_39 = directory_target(39, 402);
    let spline = directory_target(41, 112);
    let directory = BTreeMap::from([
        (1, &target_1),
        (7, &target_7),
        (15, &target_15),
        (17, &target_17),
        (39, &target_39),
        (41, &spline),
    ]);
    for segment_count in [0_i64, -1] {
        let mut values = [
            112, 3, 0, 3, 1, 1, 3, 25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 39, 17, 7, 1, 15, 17, 7, 1,
            15, 17, 7, 1, 1, 1, 0,
        ];
        values[4] = segment_count;
        let record = ParameterRecord {
            directory_sequence: 41,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: 34,
            comment: Vec::new(),
        };

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0,
            "N={segment_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 0, "N={segment_count}");
        assert!(analysis.groups().is_none(), "N={segment_count}");
    }
}

#[test]
fn type106_entity_table_boundary_uses_interpretation_width() {
    let cases = [
        (1_i64, 1_i64, 2_usize, 2_usize),
        (2, 2, 2, 3),
        (3, 3, 2, 6),
        (11, 1, 2, 2),
        (12, 2, 2, 3),
        (13, 3, 2, 6),
        (20, 1, 2, 2),
        (21, 1, 2, 2),
        (31, 1, 2, 2),
        (32, 1, 2, 2),
        (33, 1, 2, 2),
        (34, 1, 2, 2),
        (35, 1, 2, 2),
        (36, 1, 2, 2),
        (37, 1, 2, 2),
        (38, 1, 2, 2),
        (40, 1, 3, 2),
        (63, 1, 2, 2),
    ];

    for (form, interpretation, tuple_count, tuple_width) in cases {
        let mut values = vec![
            106,
            interpretation,
            i64::try_from(tuple_count).expect("test tuple count fits i64"),
        ];
        if interpretation == 1 {
            values.push(0);
        }
        values.extend(std::iter::repeat_n(0, tuple_count * tuple_width));
        values.extend([1, 1, 0]);
        let expected_start = match interpretation {
            1 => 4 + tuple_count * 2,
            2 => 3 + tuple_count * 3,
            3 => 3 + tuple_count * 6,
            _ => unreachable!(),
        };
        let association = directory_target(1, 402);
        let mut copious = directory_target(3, 106);
        copious.form = form;
        let directory = BTreeMap::from([(1, &association), (3, &copious)]);
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 106 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type106_form_interpretation_mismatch_does_not_enable_generic_recovery() {
    let association = directory_target(1, 402);
    let mut copious = directory_target(3, 106);
    copious.form = 11;
    let directory = BTreeMap::from([(1, &association), (3, &copious)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 2, 1, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 6,
        comment: Vec::new(),
    };

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type106_form63_rejects_nonplanar_interpretation_for_boundary_recovery() {
    let association = directory_target(1, 402);
    let mut copious = directory_target(3, 106);
    copious.form = 63;
    let directory = BTreeMap::from([(1, &association), (3, &copious)]);
    let record = ParameterRecord {
        directory_sequence: 3,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [106, 2, 2, 0, 0, 0, 0, 0, 0, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 12,
        comment: Vec::new(),
    };

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        0
    );
    assert_eq!(analysis.valid_candidate_count(), 0);
    assert!(analysis.groups().is_none());
}

#[test]
fn type116_entity_table_boundary_keeps_defaulted_display_pointer_slot() {
    let association = directory_target(1, 402);
    let point = directory_target(3, 116);
    let directory = BTreeMap::from([(1, &association), (3, &point)]);
    let token_values = [
        vec![116, 1, 2, 3, 0, 1, 1, 0]
            .into_iter()
            .map(TokenValue::Integer)
            .collect::<Vec<_>>(),
        vec![
            TokenValue::Integer(116),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(3),
            TokenValue::Omitted,
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];

    for values in token_values {
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token { value, span: 0..0 })
                .collect(),
            parameter_end: 8,
            comment: Vec::new(),
        };

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 116 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn multiple_valid_trailing_pointer_group_boundaries_are_ambiguous() {
    let association = directory_target(1, 402);
    let directory = BTreeMap::from([(1, &association)]);
    let record = ParameterRecord {
        directory_sequence: 1,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: [116, 0, 0, 2, 1, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: 7,
        comment: Vec::new(),
    };

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        2
    );
    assert_eq!(analysis.valid_candidate_count(), 2);
    assert!(analysis.groups().is_none());
}

#[test]
fn decode_uses_type116_boundary_without_assigning_malformed_groups() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "BAD".into(),
                    status: "00000000",
                    parameters: "116,4,3,3,3,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 0,
                    label: "ASSOC".into(),
                    status: "00000000",
                    parameters: "402;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));

    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(source.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(source.fields()["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(source.fields()["parameters"].as_array().unwrap().len(), 7);
}

#[test]
fn decode_uses_type116_entity_boundary_for_explicit_and_omitted_display_pointer() {
    for parameters in ["116,1,2,3,0,1,1,0;", "116,1,2,3,,1,1,0;"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "GROUP".into(),
                        status: "00000000",
                        parameters: "402,1,3;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 116,
                        form: 0,
                        label: "POINT".into(),
                        status: "00000000",
                        parameters: parameters.into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
        let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#3")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#1")]
        );
    }
}

#[test]
fn decode_uses_type102_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,7;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINEA".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINEB".into(),
                    status: "00010000",
                    parameters: "110,1,0,0,2,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "COMPOS".into(),
                    status: "00000000",
                    parameters: "102,2,3,5,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn decode_uses_type106_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 11,
                    label: "PATH".into(),
                    status: "00000000",
                    parameters: "106,1,2,0,0,0,1,0,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn decode_uses_type123_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 123,
                    form: 0,
                    label: "DIRECT".into(),
                    status: "00010000",
                    parameters: "123,0,0,2,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn decode_uses_type110_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUPA".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUPB".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "LINE".into(),
                    status: "00010000",
                    parameters: "110,7,3,3,1,3,3,1,3,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#3")]
    );
}

#[test]
fn decode_uses_type402_entity_boundary_for_group_forms() {
    for form in [1_i64, 7, 14, 15] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 402,
                        form,
                        label: "GROUP".into(),
                        status: "00000000",
                        parameters: "402,2,3,5,1,7,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 116,
                        form: 0,
                        label: "MEMBER1".into(),
                        status: "00000000",
                        parameters: "116,0,0,0,0,1,1,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "MEMBER2".into(),
                        status: "00000000",
                        parameters: "402,1,3,1,1,0;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "ASSOC".into(),
                        status: "00000000",
                        parameters: "402,1,5;".into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(
            result.report().losses.is_empty(),
            "Form {form}: {:#?}",
            result.report().losses
        );
        let native = result.ir().native.namespace("iges").unwrap();
        let source = native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#7")]
        );
        let group = native.arenas()["groups"]
            .iter()
            .find(|record| record.fields()["source_entity"] == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(group.fields()["declared_member_count"], 2);
        assert_eq!(
            group.fields()["members"].as_array().unwrap(),
            &[
                serde_json::json!("iges:entity:directory#3"),
                serde_json::json!("iges:entity:directory#5"),
            ]
        );
        assert_eq!(group.fields()["ordered"], matches!(form, 14 | 15));
        assert_eq!(
            group.fields()["back_pointers_required"],
            matches!(form, 1 | 14)
        );
    }
}

#[test]
fn decode_uses_type126_entity_boundary_for_form7_association() {
    for form in 0_i64..=5 {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 7,
                        label: "GROUP".into(),
                        status: "00000000",
                        parameters: "402,1,3;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 126,
                        form,
                        label: "NURBS".into(),
                        status: "00010000",
                        parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1,1,1,0;"
                            .into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(result.report().losses.is_empty(), "Form {form}");
        assert_eq!(result.ir().model.curves.len(), 1, "Form {form}");
        let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#3")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#1")],
            "Form {form}"
        );
    }
}

#[test]
fn decode_uses_type112_entity_boundary_for_form7_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 112,
                    form: 0,
                    label: "SPLINE".into(),
                    status: "00010000",
                    parameters:
                        "112,3,0,3,1,1,3,25,1,1,1,1,1,1,1,1,1,1,1,39,17,7,1,15,17,7,1,15,17,7,1,1,1,0;"
                            .into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ParameterBoundaryAmbiguous.kind()));
    assert_eq!(result.ir().model.curves.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}
