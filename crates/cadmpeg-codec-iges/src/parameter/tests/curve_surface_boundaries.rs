use super::*;

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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count, 1, "Form {form}");
        let groups = analysis.groups.expect("Type 126 table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(groups.associations, vec![1], "Form {form}");
        assert!(groups.properties.is_empty(), "Form {form}");
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

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 126 table boundary");
    assert_eq!(groups.token_start, 24);
    assert_eq!(groups.associations, vec![7]);
    assert!(groups.properties.is_empty());
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "K={k}, M={degree}");
        assert_eq!(analysis.valid_candidate_count, 0, "K={k}, M={degree}");
        assert!(analysis.groups.is_none(), "K={k}, M={degree}");
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 112 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
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

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 112 table boundary");
    assert_eq!(groups.token_start, 31);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 0, "N={segment_count}");
        assert_eq!(analysis.valid_candidate_count, 0, "N={segment_count}");
        assert!(analysis.groups.is_none(), "N={segment_count}");
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
        let mut values = vec![106, interpretation, tuple_count as i64];
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 106 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
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

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 0);
    assert_eq!(analysis.valid_candidate_count, 0);
    assert!(analysis.groups.is_none());
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

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 116 table boundary");
        assert_eq!(groups.token_start, 5);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
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

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 2);
    assert_eq!(analysis.valid_candidate_count, 2);
    assert!(analysis.groups.is_none());
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

    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
        let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
        let source = native.arenas["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(
            source.fields()["association_links"].as_array().unwrap(),
            &[serde_json::json!("iges:entity:directory#7")]
        );
        let group = native.arenas["groups"]
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
        let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
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
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn type114_entity_table_boundary_uses_segment_dimensions() {
    for ((u_segments, v_segments), expected_start) in [((1_i64, 1_i64), 201_usize), ((2, 1), 298)] {
        let association = directory_target(1, 212);
        let surface = directory_target(3, 114);
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 114;
        values[1] = 3;
        values[2] = 1;
        values[3] = u_segments;
        values[4] = v_segments;
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
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 114 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type114_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let surface = directory_target(3, 114);
    let directory = BTreeMap::from([(1, &association), (3, &surface)]);
    let mut values = vec![1_i64; 204];
    values[0] = 114;
    values[1] = 3;
    values[2] = 1;
    values[3] = 1;
    values[4] = 1;
    values[5] = 0;
    values[6] = 1;
    values[7] = 0;
    values[8] = 1;
    values[9] = 193;
    values[203] = 0;
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
        parameter_end: 204,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 114 table boundary");
    assert_eq!(groups.token_start, 201);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type114_malformed_segment_dimensions_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let surface = directory_target(3, 114);
    let directory = BTreeMap::from([(1, &association), (3, &surface)]);
    for (u_segments, v_segments) in [(0_i64, 1_i64), (-1, 1), (1, 0)] {
        let mut values = vec![1_i64; 204];
        values[0] = 114;
        values[1] = 3;
        values[2] = 1;
        values[3] = u_segments;
        values[4] = v_segments;
        values[5] = 0;
        values[6] = 1;
        values[7] = 0;
        values[8] = 1;
        values[9] = 193;
        values[203] = 0;
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
            parameter_end: 204,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "M={u_segments}, N={v_segments}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "M={u_segments}, N={v_segments}"
        );
        assert!(analysis.groups.is_none(), "M={u_segments}, N={v_segments}");
    }
}

#[test]
fn decode_uses_type114_entity_boundary_for_form7_association() {
    let mut values = vec!["114", "3", "1", "1", "1", "0", "1", "0", "1", "193"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    values.extend((0..191).map(|_| "1".to_owned()));
    values.extend(["1", "1", "0"].map(str::to_owned));
    let parameters = format!("{};", values.join(","));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 114,
                    form: 0,
                    label: "SPLSURF".into(),
                    status: "00000000",
                    parameters,
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
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn type128_entity_table_boundary_uses_surface_indices() {
    for ((k1, k2, m1, m2), expected_start) in
        [((1_i64, 1_i64, 1_i64, 1_i64), 38_usize), ((2, 1, 1, 0), 46)]
    {
        let association = directory_target(1, 212);
        let mut surface = directory_target(3, 128);
        surface.form = 9;
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 128;
        values[1] = k1;
        values[2] = k2;
        values[3] = m1;
        values[4] = m2;
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
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 128 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type128_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut surface = directory_target(5, 128);
    surface.form = 0;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &surface)]);
    let values = [
        128, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 3, 4, 0, 1, 3, 4, 21, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 0,
    ];
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
        parameter_end: 41,
        comment: Vec::new(),
    };

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 128 table boundary");
    assert_eq!(groups.token_start, 38);
    assert_eq!(groups.associations, vec![1]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type128_malformed_indices_do_not_enable_generic_recovery() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut surface = directory_target(5, 128);
    surface.form = 0;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &surface)]);
    for (k1, k2, m1, m2) in [(0_i64, 1_i64, 1_i64, 1_i64), (-1, 1, 0, 0), (1, 0, 2, 0)] {
        let mut values = [
            128, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 3, 4, 0, 1, 3, 4, 21, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 0,
        ];
        values[1] = k1;
        values[2] = k2;
        values[3] = m1;
        values[4] = m2;
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
            parameter_end: 41,
            comment: Vec::new(),
        };

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(
            analysis.candidate_count, 0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert_eq!(
            analysis.valid_candidate_count, 0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert!(
            analysis.groups.is_none(),
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
    }
}

#[test]
fn decode_uses_type128_entity_boundary_for_form7_association() {
    let values = [
        "128", "1", "1", "1", "1", "1", "1", "0", "0", "0", "0", "1", "3", "4", "0", "1", "3", "4",
        "21", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "3",
        "1", "3", "1", "1", "0",
    ]
    .join(",");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET1".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET3".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
                },
                OwnedTestEntity {
                    entity_type: 128,
                    form: 0,
                    label: "NURBS".into(),
                    status: "00000000",
                    parameters: format!("{values};"),
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
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
}

#[test]
fn type144_entity_table_boundary_uses_inner_boundary_count() {
    for (inner_count, expected_start) in [(0_i64, 5_usize), (1, 6)] {
        let association = directory_target(1, 212);
        let surface = directory_target(3, 144);
        let directory = BTreeMap::from([(1, &association), (3, &surface)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 144;
        values[1] = 1;
        values[2] = i64::from(inner_count > 0);
        values[3] = inner_count;
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
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 144 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type144_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 144);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [144, 1, 1, 1, 3, 2, 1, 3, 0];
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
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 144 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type144_malformed_inner_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 144);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![144, 1, 0, -1, 0, 1, 1, 0],
        vec![144, 1, 0, 100, 0, 1, 1, 0],
        vec![144, 1, 0],
    ] {
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
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn decode_uses_type144_entity_boundary_for_form0_association() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 108,
                    form: 0,
                    label: "PLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "OUTMODEL".into(),
                    status: "00010000",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "OUTPCURV".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "OUTBOUND".into(),
                    status: "00010000",
                    parameters: "142,0,1,5,3,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "INMODEL".into(),
                    status: "00010000",
                    parameters: "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;"
                        .into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "INPCURV".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;"
                        .into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "INBOUND".into(),
                    status: "00010000",
                    parameters: "142,0,1,11,9,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 144,
                    form: 0,
                    label: "TRIMMED".into(),
                    status: "00000000",
                    parameters: "144,1,1,1,7,13,1,17,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET1".into(),
                    status: "00010100",
                    parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
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
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.faces.len(), 1);
    let source = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#15")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#17")]
    );
}

#[test]
fn type143_entity_table_boundary_uses_boundary_count() {
    for (boundary_count, expected_start) in [(0_i64, 4_usize), (1, 5), (2, 6)] {
        let association = directory_target(1, 212);
        let source = directory_target(3, 143);
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 143;
        values[1] = 1;
        values[2] = 1;
        values[3] = boundary_count;
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
        assert_eq!(analysis.candidate_count, 1);
        assert_eq!(analysis.valid_candidate_count, 1);
        let groups = analysis.groups.expect("Type 143 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations, vec![1]);
        assert!(groups.properties.is_empty());
    }
}

#[test]
fn type143_entity_table_boundary_precedes_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let source = directory_target(5, 143);
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [143, 1, 1, 1, 2, 1, 3, 0];
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
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 143 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations, vec![3]);
    assert!(groups.properties.is_empty());
}

#[test]
fn type143_malformed_boundary_counts_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let source = directory_target(3, 143);
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    for values in [
        vec![143, 1, 1, -1, 0, 1, 1, 0],
        vec![143, 1, 1, 100, 0, 1, 1, 0],
        vec![143, 1, 1],
    ] {
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
        assert_eq!(analysis.candidate_count, 0);
        assert_eq!(analysis.valid_candidate_count, 0);
        assert!(analysis.groups.is_none());
    }
}

#[test]
fn type140_form0_follows_five_primary_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 140);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[140, 0, 0, 1, 2, 3, 1, 1, 1, 5]);

    let analysis = analyze_trailing_pointer_groups(&record, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    let groups = analysis.groups.expect("Type 140 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![1]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type140_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 140);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (9, &source),
    ]);
    let record = integer_parameter_record(9, &[140, 0, 0, 1, 2, 2, 1, 3, 1, 5]);
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
    let groups = analysis.groups.expect("Type 140 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5]);
}

#[test]
fn type140_complete_wrong_fields_keep_boundary_and_truncated_spans_do_not_recover() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 140);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let wrong_field = token_parameter_record(
        9,
        vec![
            140.into(),
            TokenValue::String(b"bad-nx".to_vec()),
            0.into(),
            1.into(),
            2.into(),
            3.into(),
            1.into(),
            1.into(),
            1.into(),
            5.into(),
        ],
    );
    let analysis = analyze_trailing_pointer_groups(&wrong_field, &directory);
    assert_eq!(analysis.candidate_count, 1);
    assert_eq!(analysis.valid_candidate_count, 1);
    assert_eq!(analysis.groups.expect("Type 140 boundary").token_start, 6);

    for values in [vec![140, 0, 0, 1, 2], vec![140, 0, 0, 1, 2, 3, 1, 1, 1]] {
        let analysis =
            analyze_trailing_pointer_groups(&integer_parameter_record(9, &values), &directory);
        assert_eq!(analysis.candidate_count, 0, "values={values:?}");
        assert_eq!(analysis.valid_candidate_count, 0, "values={values:?}");
        assert!(analysis.groups.is_none(), "values={values:?}");
    }
}

#[test]
fn type308_form0_entity_table_boundary_follows_member_count() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(11, 308);
    let directory = BTreeMap::from([(1, &association), (5, &property), (11, &source)]);

    for (members, expected_start) in [(Vec::new(), 4_usize), (vec![7_i64], 5), (vec![7, 9], 6)] {
        let member_count = i64::try_from(members.len()).expect("test member count fits");
        let mut values = vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec()),
            member_count.into(),
        ];
        values.extend(members.into_iter().map(TokenValue::from));
        values.extend([1.into(), 1.into(), 1.into(), 5.into()]);
        let analysis =
            analyze_trailing_pointer_groups(&token_parameter_record(11, values), &directory);
        assert_eq!(analysis.candidate_count, 1, "N={member_count}");
        assert_eq!(analysis.valid_candidate_count, 1, "N={member_count}");
        let groups = analysis.groups.expect("Type 308 table boundary");
        assert_eq!(groups.token_start, expected_start, "N={member_count}");
        assert_eq!(groups.associations, vec![1], "N={member_count}");
        assert_eq!(groups.properties, vec![5], "N={member_count}");
    }
}

#[test]
fn type308_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(11, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (11, &source),
    ]);
    let record = token_parameter_record(
        11,
        vec![
            308.into(),
            0.into(),
            TokenValue::String(b"FIG".to_vec()),
            2.into(),
            7.into(),
            2.into(),
            1.into(),
            3.into(),
            1.into(),
            5.into(),
        ],
    );
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
    let groups = analysis.groups.expect("Type 308 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations, vec![3]);
    assert_eq!(groups.properties, vec![5]);
}
