use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::groups_for_candidate_with_context;
use crate::parameter::structural_pointer_group_candidates_with_context;
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 114 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 114 table boundary");
    assert_eq!(groups.token_start, 201);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0,
            "M={u_segments}, N={v_segments}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            0,
            "M={u_segments}, N={v_segments}"
        );
        assert!(
            analysis.groups().is_none(),
            "M={u_segments}, N={v_segments}"
        );
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 128 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 128 table boundary");
    assert_eq!(groups.token_start, 38);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            0,
            "K1={k1}, K2={k2}, M1={m1}, M2={m2}"
        );
        assert!(
            analysis.groups().is_none(),
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
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 144 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 144 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
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
    let source = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1
        );
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("Type 143 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 143 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type140_form0_follows_five_primary_fields() {
    let association = directory_target(1, 212);
    let property = directory_target(5, 406);
    let source = directory_target(9, 140);
    let directory = BTreeMap::from([(1, &association), (5, &property), (9, &source)]);
    let record = integer_parameter_record(9, &[140, 0, 0, 1, 2, 3, 1, 1, 1, 5]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 140 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
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
    let valid_starts = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    })
    .into_iter()
    .filter(|candidate| {
        crate::test_support::with_service_context(&[], |ctx| {
            groups_for_candidate_with_context(&record, &directory, *candidate, ctx)
                .expect("test-only trailing pointer allocation")
        })
        .is_some_and(|groups| groups.fully_valid().is_some())
    })
    .map(|candidate| candidate.token_start)
    .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 140 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
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
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &wrong_field,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&wrong_field, entity_primary_end(&wrong_field, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(analysis.groups().expect("Type 140 boundary").token_start, 6);

    for values in [vec![140, 0, 0, 1, 2], vec![140, 0, 0, 1, 2, 3, 1, 1, 1]] {
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &integer_parameter_record(9, &values),
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
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
        let analysis_record = token_parameter_record(11, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &analysis_record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert_eq!(
            analysis.candidate_count(
                &analysis_record,
                entity_primary_end(&analysis_record, &directory)
            ),
            1,
            "N={member_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "N={member_count}");
        let groups = analysis.groups().expect("Type 308 table boundary");
        assert_eq!(groups.token_start, expected_start, "N={member_count}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "N={member_count}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            vec![5],
            "N={member_count}"
        );
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
    let valid_starts = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    })
    .into_iter()
    .filter(|candidate| {
        crate::test_support::with_service_context(&[], |ctx| {
            groups_for_candidate_with_context(&record, &directory, *candidate, ctx)
                .expect("test-only trailing pointer allocation")
        })
        .is_some_and(|groups| groups.fully_valid().is_some())
    })
    .map(|candidate| candidate.token_start)
    .collect::<Vec<_>>();
    assert_eq!(valid_starts, vec![5, 6]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &record,
            &directory,
            crate::global::GlobalTable::V5Later,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 308 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5]);
}
