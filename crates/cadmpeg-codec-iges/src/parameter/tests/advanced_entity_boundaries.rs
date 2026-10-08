use super::integer_parameter_record;
use super::token_parameter_record;
use crate::global::GlobalTable;
use crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context;
use crate::parameter::entity_primary_end;
use crate::parameter::entity_primary_end_for_global_table;
use crate::parameter::structural_pointer_group_candidates_with_context;
use crate::parameter::ParameterRecord;
use crate::parameter::Token;
use crate::parameter::TokenValue;
use crate::test_support::directory_target;
use std::collections::BTreeMap;

#[test]
fn type406_form27_complete_counted_span_keeps_boundary_with_invalid_value_type() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let values = vec![
        TokenValue::Integer(406),
        TokenValue::Integer(4),
        TokenValue::String(b"PROPTEST".to_vec()),
        TokenValue::Integer(1),
        TokenValue::Integer(7),
        TokenValue::Integer(17),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(3),
    ];
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect::<Vec<_>>();
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: tokens.len(),
        tokens,
        comment: Vec::new(),
        double_precision_reals: Vec::new(),
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
    assert_eq!(
        analysis
            .groups()
            .expect("Form 27 table boundary")
            .token_start,
        6
    );
}

#[test]
fn type406_form27_malformed_np_or_value_count_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let units = directory_target(3, 316);
    let mut source = directory_target(5, 406);
    source.form = 27;
    let directory = BTreeMap::from([(1, &association), (3, &units), (5, &source)]);
    let cases = vec![
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(5),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(2),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(17),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ],
        vec![
            TokenValue::Integer(406),
            TokenValue::Integer(4),
            TokenValue::String(b"PROPTEST".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ],
    ];
    for values in cases {
        let tokens = values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect::<Vec<_>>();
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: tokens.len(),
            tokens,
            comment: Vec::new(),
            double_precision_reals: Vec::new(),
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
fn type402_form6_entity_table_boundary_follows_view_list() {
    for (visible_count, expected_start) in [(0_i64, 4_usize), (1, 5), (2, 6)] {
        let association = directory_target(1, 212);
        let view = directory_target(3, 410);
        let mut source = directory_target(5, 402);
        source.form = 6;
        let visible_1 = directory_target(7, 212);
        let visible_2 = directory_target(9, 212);
        let directory = BTreeMap::from([
            (1, &association),
            (3, &view),
            (5, &source),
            (7, &visible_1),
            (9, &visible_2),
        ]);
        let visible_count = usize::try_from(visible_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2] = i64::try_from(visible_count).unwrap();
        values[3] = 3;
        for (offset, sequence) in [7_i64, 9].into_iter().take(visible_count).enumerate() {
            values[4 + offset] = sequence;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
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
            double_precision_reals: Vec::new(),
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
            1,
            "N1={visible_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "N1={visible_count}");
        let groups = analysis.groups().expect("Type 402 Form 6 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form6_entity_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source)]);
    let values = [402, 1, 1, 3, 2, 1, 1, 0];
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
        double_precision_reals: Vec::new(),
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
    let groups = analysis.groups().expect("Type 402 Form 6 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form6_malformed_fields_do_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(5, 402);
    source.form = 6;
    let visible = directory_target(7, 212);
    let directory = BTreeMap::from([(1, &association), (3, &view), (5, &source), (7, &visible)]);
    let cases = [
        vec![402, 0, 1, 3, 7, 1, 1, 0],
        vec![402, 1, -1, 3, 7, 1, 1, 0],
        vec![402, 1, 1000, 3, 7, 1, 1, 0],
        vec![402, 1],
        vec![402, 1, 2, 3, 7],
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
            double_precision_reals: Vec::new(),
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

    let mut values = (0..8)
        .map(|_| Token {
            value: TokenValue::Integer(0),
            span: 0..0,
        })
        .collect::<Vec<_>>();
    values[0].value = TokenValue::Integer(402);
    values[1].value = TokenValue::Integer(1);
    values[2].value = TokenValue::String(b"1".to_vec());
    values[3].value = TokenValue::Integer(3);
    values[4].value = TokenValue::Integer(7);
    values[5].value = TokenValue::Integer(1);
    values[6].value = TokenValue::Integer(1);
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values,
        parameter_end: 8,
        comment: Vec::new(),
        double_precision_reals: Vec::new(),
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

#[test]
fn type402_form16_entity_table_boundary_follows_entity_count() {
    for (entity_count, expected_start) in [(1_i64, 5_usize), (2, 6)] {
        let association = directory_target(1, 212);
        let transform = directory_target(3, 124);
        let entity_1 = directory_target(7, 116);
        let entity_2 = directory_target(9, 116);
        let mut source = directory_target(5, 402);
        source.form = 16;
        let directory = BTreeMap::from([
            (1, &association),
            (3, &transform),
            (5, &source),
            (7, &entity_1),
            (9, &entity_2),
        ]);
        let entity_count = usize::try_from(entity_count).unwrap();
        let mut values = vec![0_i64; expected_start + 3];
        values[0] = 402;
        values[1] = 1;
        values[2] = i64::try_from(entity_count).unwrap();
        values[3] = 3;
        for (offset, sequence) in [7_i64, 9].into_iter().take(entity_count).enumerate() {
            values[4 + offset] = sequence;
        }
        values[expected_start] = 1;
        values[expected_start + 1] = 1;
        values[expected_start + 2] = 0;
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            parameter_end: values.len(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            comment: Vec::new(),
            double_precision_reals: Vec::new(),
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
            1,
            "N={entity_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "N={entity_count}");
        let groups = analysis.groups().expect("Type 402 Form 16 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form16_accepts_explicit_empty_pointer_groups() {
    let transform = directory_target(3, 124);
    let member = directory_target(341, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(3, &transform), (5, &source), (341, &member)]);
    let record = integer_parameter_record(5, &[402, 1, 1, 3, 341, 0, 0]);

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
    let groups = analysis
        .groups()
        .expect("Type 402 Form 16 empty pointer groups");
    assert_eq!(groups.token_start, 5);
    assert!(groups
        .associations()
        .copied()
        .collect::<Vec<_>>()
        .is_empty());
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form16_table_boundary_precedes_valid_generic_alternative() {
    let association = directory_target(1, 212);
    let transform = directory_target(3, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(1, &association), (3, &transform), (5, &source)]);
    let values = [402, 1, 1, 3, 2, 1, 1, 0];
    let record = ParameterRecord {
        directory_sequence: 5,
        line_range: 1..2,
        bytes: Vec::new(),
        parameter_end: values.len(),
        tokens: values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        comment: Vec::new(),
        double_precision_reals: Vec::new(),
    };
    let generic = crate::test_support::with_service_context(&[], |ctx| {
        structural_pointer_group_candidates_with_context(&record, ctx)
            .expect("test-only pointer candidate allocation")
    });
    assert!(generic.iter().any(|candidate| candidate.token_start == 4));
    assert!(generic.iter().any(|candidate| candidate.token_start == 5));

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
    let groups = analysis.groups().expect("Type 402 Form 16 table boundary");
    assert_eq!(groups.token_start, 5);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
}

#[test]
fn type402_form16_malformed_count_or_span_does_not_enable_generic_recovery() {
    let association = directory_target(1, 212);
    let transform = directory_target(3, 124);
    let mut source = directory_target(5, 402);
    source.form = 16;
    let directory = BTreeMap::from([(1, &association), (3, &transform), (5, &source)]);
    let cases = [
        vec![
            402.into(),
            0.into(),
            1.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            (-1).into(),
            1.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1).into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            i64::MAX.into(),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![
            402.into(),
            1.into(),
            TokenValue::real(1.0),
            3.into(),
            7.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
        vec![402.into(), 1.into()],
        vec![402.into(), 1.into(), 1.into(), 3.into()],
        vec![402.into(), 1.into(), 1.into(), 3.into(), 7.into(), 1.into()],
    ];

    for values in cases {
        let record = token_parameter_record(5, values);
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
fn type402_view_visibility_forms_follow_counted_view_blocks() {
    let cases = [
        (3, vec![402.into(), 1.into(), 0.into(), 3.into()], 4),
        (
            3,
            vec![402.into(), 1.into(), TokenValue::Omitted, 3.into()],
            4,
        ),
        (
            3,
            vec![402.into(), 1.into(), 2.into(), 3.into(), 5.into(), 7.into()],
            6,
        ),
        (
            4,
            vec![
                402.into(),
                1.into(),
                0.into(),
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
            ],
            8,
        ),
        (
            4,
            vec![
                402.into(),
                1.into(),
                TokenValue::Omitted,
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
            ],
            8,
        ),
        (
            4,
            vec![
                402.into(),
                2.into(),
                1.into(),
                3.into(),
                0.into(),
                1.into(),
                0.into(),
                1.into(),
                0.into(),
                5.into(),
                7.into(),
                1.into(),
                0.into(),
                7.into(),
            ],
            14,
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

#[test]
fn type402_view_visibility_entity_count_requirement_follows_dialect() {
    let association = directory_target(1, 212);
    let view = directory_target(3, 410);
    let mut source = directory_target(9, 402);
    source.form = 3;
    let directory = BTreeMap::from([(1, &association), (3, &view), (9, &source)]);
    let omitted_count = token_parameter_record(
        9,
        vec![
            402.into(),
            1.into(),
            TokenValue::Omitted,
            3.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
    );

    assert_eq!(
        crate::test_support::with_service_context(&[], |ctx| entity_primary_end_for_global_table(
            &omitted_count,
            &directory,
            GlobalTable::V4_0,
            ctx
        ))
        .unwrap(),
        Some(omitted_count.tokens.len())
    );
    let v4_analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &omitted_count,
            &directory,
            GlobalTable::V4_0,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    assert!(v4_analysis.groups().is_none());

    for global_table in [GlobalTable::V5_0, GlobalTable::V5Later] {
        assert_eq!(
            crate::test_support::with_service_context(&[], |ctx| {
                entity_primary_end_for_global_table(&omitted_count, &directory, global_table, ctx)
            })
            .unwrap(),
            Some(4),
            "global_table={global_table:?}"
        );
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &omitted_count,
                &directory,
                global_table,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        let groups = analysis
            .groups()
            .expect("optional Type 402 visible-entity list");
        assert_eq!(groups.token_start, 4, "global_table={global_table:?}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            vec![1],
            "global_table={global_table:?}"
        );
    }

    let explicit_zero = token_parameter_record(
        9,
        vec![
            402.into(),
            1.into(),
            0.into(),
            3.into(),
            1.into(),
            1.into(),
            0.into(),
        ],
    );
    assert_eq!(
        crate::test_support::with_service_context(&[], |ctx| entity_primary_end_for_global_table(
            &explicit_zero,
            &directory,
            GlobalTable::V4_0,
            ctx
        ))
        .unwrap(),
        Some(4)
    );
    let analysis = crate::test_support::with_service_context(&[], |ctx| {
        analyze_trailing_pointer_groups_for_global_table_with_context(
            &explicit_zero,
            &directory,
            GlobalTable::V4_0,
            ctx,
        )
        .expect("test-only trailing pointer analysis")
    });
    let groups = analysis
        .groups()
        .expect("explicit V4 Type 402 entity count");
    assert_eq!(groups.token_start, 4);
    assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn primary_layout_directory_lookup_is_admitted_and_keeps_missing_entry_fallback() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    let source = directory_target(1, 116);
    let directory = BTreeMap::from([(source.sequence, &source)]);
    let record = token_parameter_record(9, vec![116.into(), 0.into()]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges parameter primary layout directory lookup",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            entity_primary_end_for_global_table(
                &record,
                &directory,
                GlobalTable::V5Later,
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.additional > 0
                && limit.operation == "iges parameter primary layout directory lookup"
    ));

    let result = crate::test_support::with_service_context(&[], |ctx| {
        entity_primary_end_for_global_table(&record, &directory, GlobalTable::V5Later, ctx)
    })
    .unwrap();
    assert_eq!(result, None);
}

#[test]
fn type402_view_visibility_malformed_counts_do_not_enable_generic_recovery() {
    let mut source = directory_target(9, 402);
    source.form = 4;
    let directory = BTreeMap::from([(9, &source)]);
    for values in [
        vec![
            402.into(),
            0.into(),
            0.into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
        vec![
            402.into(),
            1.into(),
            (-1_i64).into(),
            1.into(),
            1.into(),
            1.into(),
            1.into(),
        ],
        vec![402.into(), 1.into(), 1.into(), 1.into(), 1.into()],
    ] {
        let record = token_parameter_record(9, values);
        let analysis = crate::test_support::with_service_context(&[], |ctx| {
            analyze_trailing_pointer_groups_for_global_table_with_context(
                &record,
                &directory,
                crate::global::GlobalTable::V5Later,
                ctx,
            )
            .expect("test-only trailing pointer analysis")
        });
        assert!(analysis.groups().is_none());
    }
}

#[test]
fn type402_external_reference_index_entity_table_boundary_follows_entry_pairs() {
    for form in [2, 12] {
        for (entry_count, expected_start) in [(1_usize, 4_usize), (2, 6)] {
            let internal_1 = directory_target(1, 116);
            let internal_2 = directory_target(5, 110);
            let association = directory_target(7, 212);
            let mut source = directory_target(3, 402);
            source.form = form;
            let directory = BTreeMap::from([
                (1, &internal_1),
                (3, &source),
                (5, &internal_2),
                (7, &association),
            ]);
            let mut tokens = (0..expected_start + 3)
                .map(|_| Token {
                    value: TokenValue::Integer(0),
                    span: 0..0,
                })
                .collect::<Vec<_>>();
            tokens[0].value = TokenValue::Integer(402);
            tokens[1].value = TokenValue::Integer(i64::try_from(entry_count).unwrap());
            for (offset, sequence) in [1_i64, 5].into_iter().take(entry_count).enumerate() {
                let start = 2 + offset * 2;
                tokens[start].value = TokenValue::String(format!("REF{}", offset + 1).into_bytes());
                tokens[start + 1].value = TokenValue::Integer(sequence);
            }
            tokens[expected_start].value = TokenValue::Integer(1);
            tokens[expected_start + 1].value = TokenValue::Integer(7);
            tokens[expected_start + 2].value = TokenValue::Integer(0);
            let parameter_end = tokens.len();
            let record = ParameterRecord {
                directory_sequence: 3,
                line_range: 1..2,
                bytes: Vec::new(),
                tokens,
                parameter_end,
                comment: Vec::new(),
                double_precision_reals: Vec::new(),
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
                1,
                "N={entry_count}"
            );
            assert_eq!(analysis.valid_candidate_count(), 1, "N={entry_count}");
            let groups = analysis
                .groups()
                .expect("Type 402 external-reference table boundary");
            assert_eq!(groups.token_start, expected_start);
            assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![7]);
            assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
        }
    }
}

#[test]
fn type402_external_reference_index_malformed_counts_or_pairs_do_not_enable_generic_recovery() {
    let cases = [
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(0),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(-1),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(i64::MAX),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::String(b"1".to_vec()),
            TokenValue::String(b"REF".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(7),
            TokenValue::Integer(0),
        ],
    ];

    for form in [2, 12] {
        let target = directory_target(1, 116);
        let association = directory_target(7, 212);
        let mut source = directory_target(3, 402);
        source.form = form;
        let directory = BTreeMap::from([(1, &target), (3, &source), (7, &association)]);
        for values in cases.iter().cloned() {
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
                double_precision_reals: Vec::new(),
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
}

#[test]
fn type402_form13_entity_table_boundary_follows_geometry_list() {
    for (geometry_count, expected_start) in [(1_usize, 5_usize), (2, 6)] {
        let dimension = directory_target(1, 216);
        let geometry_1 = directory_target(5, 116);
        let geometry_2 = directory_target(7, 110);
        let association = directory_target(9, 212);
        let mut source = directory_target(3, 402);
        source.form = 13;
        let directory = BTreeMap::from([
            (1, &dimension),
            (3, &source),
            (5, &geometry_1),
            (7, &geometry_2),
            (9, &association),
        ]);
        let mut tokens = (0..expected_start + 3)
            .map(|_| Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            })
            .collect::<Vec<_>>();
        tokens[0].value = TokenValue::Integer(402);
        tokens[1].value = TokenValue::Integer(1);
        tokens[2].value = TokenValue::Integer(i64::try_from(geometry_count).unwrap());
        tokens[3].value = TokenValue::Integer(1);
        tokens[4].value = TokenValue::Integer(5);
        if geometry_count == 2 {
            tokens[5].value = TokenValue::Integer(7);
        }
        tokens[expected_start].value = TokenValue::Integer(1);
        tokens[expected_start + 1].value = TokenValue::Integer(9);
        tokens[expected_start + 2].value = TokenValue::Integer(0);
        let parameter_end = tokens.len();
        let record = ParameterRecord {
            directory_sequence: 3,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens,
            parameter_end,
            comment: Vec::new(),
            double_precision_reals: Vec::new(),
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
            1,
            "NG={geometry_count}"
        );
        assert_eq!(analysis.valid_candidate_count(), 1, "NG={geometry_count}");
        let groups = analysis.groups().expect("Type 402 Form 13 table boundary");
        assert_eq!(groups.token_start, expected_start);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![9]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}

#[test]
fn type402_form13_malformed_fields_do_not_enable_generic_recovery() {
    let dimension = directory_target(1, 216);
    let geometry = directory_target(5, 116);
    let association = directory_target(9, 212);
    let mut source = directory_target(3, 402);
    source.form = 13;
    let directory = BTreeMap::from([
        (1, &dimension),
        (3, &source),
        (5, &geometry),
        (9, &association),
    ]);
    let cases = vec![
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(0),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(-1),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(i64::MAX),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::String(b"1".to_vec()),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
            TokenValue::Integer(0),
        ],
        vec![TokenValue::Integer(402), TokenValue::Integer(1)],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
        ],
        vec![
            TokenValue::Integer(402),
            TokenValue::Integer(2),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
            TokenValue::Integer(5),
            TokenValue::Integer(1),
            TokenValue::Integer(9),
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
            double_precision_reals: Vec::new(),
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
