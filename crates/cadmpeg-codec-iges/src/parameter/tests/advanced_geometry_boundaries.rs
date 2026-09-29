use super::integer_parameter_record;
use super::token_parameter_record;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::ParameterRecord;
use crate::parameter::Token;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

#[test]
fn type402_form21_entity_table_boundary_follows_geometry_blocks() {
    for (geometry_count, expected_start) in [(1_usize, 11_usize), (2, 16)] {
        let association = directory_target(1, 212);
        let dimension = directory_target(5, 216);
        let geometry_1 = directory_target(7, 116);
        let geometry_2 = directory_target(9, 110);
        let mut source = directory_target(3, 402);
        source.form = 21;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &source),
            (5, &dimension),
            (7, &geometry_1),
            (9, &geometry_2),
        ]);
        let mut values = vec![TokenValue::Integer(0); expected_start + 3];
        values[0] = 402.into();
        values[1] = 1.into();
        values[2] = i64::try_from(geometry_count).unwrap().into();
        values[3] = 5.into();
        values[4] = 4.into();
        values[5] = TokenValue::real(0.25);
        for (offset, sequence) in [7_i64, 9].into_iter().take(geometry_count).enumerate() {
            let start = 6 + offset * 5;
            values[start] = sequence.into();
            values[start + 1] = 0.into();
            values[start + 2] = TokenValue::real(
                cadmpeg_core::convert::f64_from_index(offset).expect("test offset is exact"),
            );
            values[start + 3] = TokenValue::real(1.0);
            values[start + 4] = TokenValue::real(2.0);
        }
        values[expected_start] = 1.into();
        values[expected_start + 1] = 1.into();
        values[expected_start + 2] = 0.into();

        let analysis_record = token_parameter_record(3, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&analysis_record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(
                &analysis_record,
                entity_primary_end(&analysis_record, &directory)
            ),
            1,
            "NG={geometry_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "NG={geometry_count}");
        let groups = analysis.groups().expect("Type 402 Form 21 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form21_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let dimension = directory_target(3, 216);
    let geometry = directory_target(7, 116);
    let mut source = directory_target(5, 402);
    source.form = 21;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &dimension),
        (5, &source),
        (7, &geometry),
    ]);
    let values = vec![
        402.into(),
        1.into(),
        1.into(),
        3.into(),
        4.into(),
        TokenValue::real(0.25),
        7.into(),
        0.into(),
        TokenValue::real(0.0),
        TokenValue::real(1.0),
        2.into(),
        1.into(),
        1.into(),
        0.into(),
    ];
    let analysis_record = token_parameter_record(5, values);
    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&analysis_record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(
            &analysis_record,
            entity_primary_end(&analysis_record, &directory)
        ),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 21 table boundary");
    assert_eq!(groups.token_start, 11);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form21_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let dimension = directory_target(3, 216);
    let geometry = directory_target(7, 116);
    let mut source = directory_target(5, 402);
    source.form = 21;
    let directory = BTreeMap::from([
        (1, &association),
        (3, &dimension),
        (5, &source),
        (7, &geometry),
    ]);
    let cases = vec![
        vec![
            402.into(),
            0.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1_i64).into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            TokenValue::String(b"1".to_vec()),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![402.into(), 1.into()],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
        ],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
        ],
        vec![
            402.into(),
            1.into(),
            1.into(),
            3.into(),
            4.into(),
            TokenValue::real(0.25),
            7.into(),
            0.into(),
            TokenValue::real(0.0),
            TokenValue::real(1.0),
            TokenValue::real(2.0),
            1.into(),
        ],
    ];

    for values in cases {
        let analysis_record = token_parameter_record(5, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&analysis_record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(
                &analysis_record,
                entity_primary_end(&analysis_record, &directory)
            ),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type408_fixed_primary_boundary_follows_translation_and_scale() {
    let association = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([(3, &association), (7, &source), (9, &definition)]);
    let record = token_parameter_record(
        7,
        vec![
            408.into(),
            9.into(),
            1.into(),
            2.into(),
            3.into(),
            TokenValue::real(0.5),
            1.into(),
            3.into(),
            0.into(),
        ],
    );

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type408_fixed_primary_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property = directory_target(5, 406);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property),
        (7, &source),
        (9, &definition),
    ]);
    let record = integer_parameter_record(7, &[408, 9, 1, 2, 3, 2, 1, 3, 6, 5, 5, 5, 5, 5, 5]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 408 table boundary");
    assert_eq!(groups.token_start, 6);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5; 6]);
}

#[test]
fn type408_complete_wrong_fields_keep_boundary_and_malformed_spans_do_not_recover() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let source = directory_target(7, 408);
    let definition = directory_target(9, 308);
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (7, &source),
        (9, &definition),
    ]);
    let wrong_fields = token_parameter_record(
        7,
        vec![
            408.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::real(2.0),
            TokenValue::String(b"bad".to_vec()),
            1.into(),
            TokenValue::Omitted,
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&wrong_fields, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    assert_eq!(
        analysis
            .groups()
            .expect("Type 408 table boundary")
            .token_start,
        6
    );

    for values in [vec![408, 9, 1, 2, 3], vec![408, 9, 1, 2, 3, 1, 1, 3]] {
        let analysis =
            crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&integer_parameter_record(7, &values), &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
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
fn type402_form19_entity_table_boundary_follows_segment_blocks() {
    for (block_count, expected_start) in [(1_i64, 8_usize), (2, 14)] {
        let association = directory_target(3, 212);
        let mut source = directory_target(11, 402);
        source.form = 19;
        let directory = BTreeMap::from([(3, &association), (11, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = block_count;
        values[expected_start] = 1;
        values[expected_start + 1] = 3;
        values[expected_start + 2] = 0;

        let analysis =
            crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&integer_parameter_record(11, &values), &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(
                &integer_parameter_record(11, &values),
                entity_primary_end(&integer_parameter_record(11, &values), &directory)
            ),
            1,
            "block_count={block_count}"
        );
        assert_eq!(
            analysis.valid_candidate_count(),
            1,
            "block_count={block_count}"
        );
        let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form19_entity_table_boundary_precedes_valid_generic_alternative() {
    let association_1 = directory_target(1, 212);
    let association_3 = directory_target(3, 212);
    let property_5 = directory_target(5, 406);
    let property_7 = directory_target(7, 406);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([
        (1, &association_1),
        (3, &association_3),
        (5, &property_5),
        (7, &property_7),
        (11, &source),
    ]);
    let record = integer_parameter_record(11, &[402, 1, 9, 0, 0, 0, 0, 2, 1, 3, 2, 5, 7]);

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert_eq!(groups.properties().copied().collect::<Vec<_>>(), vec![5, 7]);
}

#[test]
fn type402_form19_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(3, 212);
    let mut source = directory_target(11, 402);
    source.form = 19;
    let directory = BTreeMap::from([(3, &association), (11, &source)]);

    let wrong_fields = token_parameter_record(
        11,
        vec![
            402.into(),
            1.into(),
            TokenValue::String(b"bad".to_vec()),
            TokenValue::real(0.5),
            0.into(),
            TokenValue::Omitted,
            TokenValue::Omitted,
            2.into(),
            1.into(),
            3.into(),
            0.into(),
        ],
    );
    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&wrong_fields, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&wrong_fields, entity_primary_end(&wrong_fields, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 19 table boundary");
    assert_eq!(groups.token_start, 8);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());

    let malformed = vec![
        token_parameter_record(
            11,
            vec![
                402.into(),
                0.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                (-1_i64).into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                i64::MAX.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                TokenValue::real(1.0),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
            ],
        ),
        token_parameter_record(
            11,
            vec![
                402.into(),
                1.into(),
                9.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                0.into(),
                1.into(),
                3.into(),
            ],
        ),
    ];
    for record in malformed {
        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            0
        );
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_form18_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 10_usize),
        (vec![1, 1, 1, 1, 1, 1], 16),
        (vec![2, 1, 1, 1, 1, 1], 17),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 18;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 2;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
        values[9] = 2;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "counts={counts:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "counts={counts:?}");
        let groups = analysis.groups().expect("Type 402 Form 18 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form18_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 18;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(2)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
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
}

#[test]
fn type402_form20_entity_table_boundary_follows_all_class_lists() {
    for (counts, expected_start) in [
        (vec![0_i64; 6], 9_usize),
        (vec![1, 1, 1, 1, 1, 1], 15),
        (vec![2, 1, 1, 1, 1, 1], 16),
    ] {
        let association = directory_target(1, 212);
        let mut source = directory_target(3, 402);
        source.form = 20;
        let directory = BTreeMap::from([(1, &association), (3, &source)]);
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2..8].copy_from_slice(&counts);
        values[8] = 1;
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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

        let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
        assert_eq!(
            analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
            1,
            "counts={counts:?}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "counts={counts:?}");
        let groups = analysis.groups().expect("Type 402 Form 20 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form20_entity_table_boundary_beats_target_valid_generic_alternative() {
    let target_1 = directory_target(1, 212);
    let target_3 = directory_target(3, 212);
    let mut source = directory_target(5, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &target_1), (3, &target_3), (5, &source)]);
    let values = [402, 1, 0, 0, 0, 0, 0, 1, 1, 2, 1, 3, 0];
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

    let analysis = crate::test_support::with_service_context(&[], |ctx| analyze_trailing_pointer_groups_for_global_table_with_context(&record, &directory, crate::global::GlobalTable::V5Later, ctx).expect("test-only trailing pointer analysis"));
    assert_eq!(
        analysis.candidate_count(&record, entity_primary_end(&record, &directory)),
        1
    );
    assert_eq!(analysis.valid_candidate_count(), 1);
    let groups = analysis.groups().expect("Type 402 Form 20 table boundary");
    assert_eq!(groups.token_start, 10);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![3]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form20_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let mut source = directory_target(3, 402);
    source.form = 20;
    let directory = BTreeMap::from([(1, &association), (3, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
    ];

    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
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
}
