// SPDX-License-Identifier: Apache-2.0

#[test]
fn resolved_trim_scalar_preserves_missing_duplicate_and_conflict_rules() {
    use super::{FeatureVariableRow, FeatureVariableTable, ScalarLane, VariableType};

    let row = |value| FeatureVariableRow {
        variable_type: VariableType::Radius,
        key: 7,
        value,
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset: 0,
    };
    let table = |rows: Vec<FeatureVariableRow>| FeatureVariableTable {
        declared_count: u32::try_from(rows.len()).expect("row count fits"),
        entity_ref: None,
        rows,
        offset: 0,
    };
    let resolve = |rows| {
        crate::decode::with_test_decode_ctx(|ctx| {
            table(rows)
                .reconciled_trim_geometry(ctx)
                .map(|geometry| geometry.radius(7))
        })
        .expect("radius budget")
    };
    assert_eq!(resolve(Vec::new()), Ok(None));
    assert_eq!(resolve(vec![row(ScalarLane::Undefined)]), Ok(None));
    assert_eq!(resolve(vec![row(ScalarLane::DimensionDriven)]), Ok(None));
    assert_eq!(
        resolve(vec![
            row(ScalarLane::Value(2.0)),
            row(ScalarLane::Value(2.0))
        ])
        .expect("matching values")
        .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(2.0)
    );
    assert_eq!(
        resolve(vec![
            row(ScalarLane::Undefined),
            row(ScalarLane::Value(2.0))
        ]),
        Err(())
    );
    assert_eq!(
        resolve(vec![
            row(ScalarLane::Value(2.0)),
            row(ScalarLane::Undefined)
        ]),
        Err(())
    );
    assert_eq!(
        resolve(vec![
            row(ScalarLane::Value(2.0)),
            row(ScalarLane::Value(3.0))
        ]),
        Err(())
    );
    assert_eq!(resolve(vec![row(ScalarLane::Value(f64::NAN))]), Err(()));
}

fn assert_definition_limit(
    payload: &[u8],
    operation: &'static str,
    retained: bool,
    parse: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| parse(ctx)).is_ok(),
        "service input should parse"
    );
    let dimension = if retained {
        ResourceDimension::RetainedBytes
    } else {
        ResourceDimension::CollectionItems
    };
    let error = crate::test_support::last_refusal_at(payload, dimension, operation, parse);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
        if refusal.dimension == dimension && refusal.operation == operation),
        "expected a refusal at {operation}"
    );
}

#[test]
fn feature_definition_start_vec_refuses_before_growth() {
    let payload = b"feat_defs_1\0";
    assert_definition_limit(payload, "creo feature definition starts", false, |ctx| {
        super::definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn contextual_definition_start_vec_refuses_before_growth() {
    let payload = b"feat_defs_1\0\xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let error = crate::test_support::last_refusal_at(
        payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo feature definition starts",
        |ctx| super::definition_starts(ctx, payload),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    policy.limits.max_collection_items = limit.limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("definition input admitted");
    assert!(matches!(super::definition_starts(&ctx, payload),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.operation == "creo feature definition starts"));
}

#[test]
fn retained_definition_offset_node_refuses_before_insertion() {
    let payload = b"feat_defs_1\0";
    assert_definition_limit(
        payload,
        "creo retained definition offset nodes",
        false,
        |ctx| super::definitions(ctx, payload).map(|_| ()),
    );
}

#[test]
fn replay_marker_vec_refuses_before_growth() {
    let payload = b"\xe3S2D0002\0";
    assert_definition_limit(payload, "creo S2D replay starts", false, |ctx| {
        super::positional_replay_definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn pending_replay_marker_node_refuses_before_insertion() {
    let payload = b"\xe3S2D0002\0";
    assert_definition_limit(payload, "creo pending S2D marker nodes", false, |ctx| {
        super::positional_replay_definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn definition_replay_start_vec_refuses_before_growth() {
    let payload = b"feat_defs_1\0\xe3S2D0002\0";
    assert_definition_limit(payload, "creo definition replay starts", false, |ctx| {
        super::definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn claimed_replay_marker_node_refuses_before_insertion() {
    let payload = b"feat_defs_1\0\xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0002\0";
    assert_definition_limit(payload, "creo claimed S2D marker nodes", false, |ctx| {
        super::positional_replay_definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn depdb_section_start_vec_refuses_before_growth() {
    let payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0";
    assert_definition_limit(payload, "creo DEPDB section starts", false, |ctx| {
        super::depdb_definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn depdb_definition_start_vec_refuses_before_growth() {
    let payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0";
    assert_definition_limit(payload, "creo DEPDB definition starts", false, |ctx| {
        super::depdb_definitions(ctx, payload).map(|_| ())
    });
}

#[test]
fn parsed_definition_vec_refuses_before_growth() {
    let payload = b"plain body";
    assert_definition_limit(payload, "creo parsed feature definitions", false, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn definition_scalar_cache_refuses_before_unique_image_insertion() {
    let payload = b"\x46\x08\0\0\0\0\0\0";
    assert_definition_limit(payload, "creo scalar cache unique images", false, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn definition_body_refuses_before_retained_copy() {
    let payload = b"plain body";
    assert_definition_limit(payload, "creo feature definition body", true, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn feature_parameter_frame_vec_refuses_before_growth() {
    let payload = b"local_sys\0\xf9\x04\x03\xe4";
    assert_definition_limit(payload, "creo feature parameter frames", false, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn feature_parameter_frame_body_refuses_before_retained_copy() {
    let payload = b"local_sys\0\xf9\x04\x03\xe4";
    assert_definition_limit(payload, "creo feature parameter frame body", true, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn feature_outline_vec_refuses_before_growth() {
    let payload = b"\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03\xe4";
    assert_definition_limit(payload, "creo feature outlines", false, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

#[test]
fn feature_outline_scalar_refuses_before_retained_copy() {
    let payload = b"\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03\xe4";
    assert_definition_limit(payload, "creo feature outline scalar body", true, |ctx| {
        super::definitions_in_ranges(
            ctx,
            payload,
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: None,
                owner_override: None,
                positional: false,
            }],
            None,
        )
        .map(|_| ())
    });
}

use super::{
    order_table, positional_order_table, segment_table_body, PrototypeRow, RelationBodyRows,
    VariableType,
};

fn one_segment_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<super::FeatureSegmentTable, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut payload = b"\xf8\x01\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)?;
    Ok(
        segment_table_body(&ctx, &payload, 0, 0, payload.len(), PrototypeRow::Present)?
            .expect("complete segment table"),
    )
}

#[test]
fn segment_rows_and_identity_indexes_refuse_before_growth() {
    let table = crate::test_support::assert_refusal_order(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        &[
            "creo segment rows",
            "creo segment identity nodes",
            "creo segment identity index",
        ],
        |cap| one_segment_with_limits(cap, u64::MAX),
    );
    assert_eq!(table.rows.len(), 1);
}

#[test]
fn segment_body_copy_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = one_segment_with_limits(u64::MAX, 0).expect_err("row body needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo segment row body"));
}

#[test]
fn equation_argument_tokens_expand_into_fixed_stack_slots() {
    for (token, expected, count) in [
        (0xe4, [Some(1), None, None], 1),
        (0xe5, [Some(0), Some(0), None], 2),
        (0xe6, [Some(0), Some(0), Some(0)], 3),
        (0xf6, [None, None, None], 1),
        (0x2a, [Some(42), None, None], 1),
    ] {
        let mut offset = 0;
        assert_eq!(
            super::equation_argument_slots(&[token], &mut offset),
            Some((expected, count))
        );
        assert_eq!(offset, 1);
    }
}

#[test]
fn variable_classes_normalize_known_codes_and_preserve_unknown_codes() {
    for (code, class) in [
        (0, VariableType::Dimension),
        (1, VariableType::U),
        (2, VariableType::V),
        (3, VariableType::Radius),
        (4, VariableType::Parameter),
        (5, VariableType::Selector),
        (6, VariableType::Result),
        (7, VariableType::Auxiliary),
    ] {
        assert_eq!(VariableType::from(code), class);
        assert_eq!(class.code(), code);
    }
    for code in [8, 255, u32::MAX] {
        let class = VariableType::from(code);
        assert!(matches!(class, VariableType::Unknown(_)));
        assert_eq!(class.code(), code);
    }
}

#[test]
fn elided_prototype_segment_table_refuses_a_declared_count_below_its_prototype_row() {
    let zero = b"\xf8\x00\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";
    let one = b"\xf8\x01\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";

    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        segment_table_body(ctx, zero, 0, 0, zero.len(), PrototypeRow::Elided)
    })
    .expect("segment table admitted")
    .is_none());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            segment_table_body(ctx, one, 0, 0, one.len(), PrototypeRow::Elided)
        })
        .expect("segment table admitted")
        .map(|table| (table.declared_count, table.rows.ordinary().count())),
        Some((1, 0))
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            segment_table_body(ctx, zero, 0, 0, zero.len(), PrototypeRow::Present)
        })
        .expect("segment table admitted")
        .map(|table| table.declared_count),
        Some(0)
    );
}

#[test]
fn order_table_refuses_a_zero_declared_count_with_a_prototype_row() {
    let prototype_row = b"\xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
        \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2";
    let table = |declared_count: u8| {
        let mut payload = b"order_table\0\xf8".to_vec();
        payload.push(declared_count);
        payload.extend_from_slice(b"\xf7\x42\xfb\xe2");
        payload.extend_from_slice(prototype_row);
        payload
    };

    let zero = table(0);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| order_table(ctx, &zero, 0, zero.len()))
            .expect("order table admitted")
            .is_none()
    );

    let one = table(1);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| order_table(ctx, &one, 0, one.len()))
            .expect("order table admitted")
            .map(|table| (table.declared_count, table.has_prototype, table.rows.len())),
        Some((1, true, 0))
    );
}

#[test]
fn positional_order_table_refuses_a_zero_declared_count_with_a_prototype_row() {
    let table = |declared_count: u8| {
        let mut payload = b"prefix\xf8".to_vec();
        payload.push(declared_count);
        payload.extend_from_slice(
            b"\xf7\x42\xfb\xe2\xf7\x43\x09\x01\x00\xf1\xf7\x42\xe2\x0a\x02\x01\xe2",
        );
        payload
    };

    let zero = table(0);
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        positional_order_table(ctx, &zero, 0, zero.len(), 66)
    })
    .expect("positional order table admitted")
    .is_none());

    let two = table(2);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            positional_order_table(ctx, &two, 0, two.len(), 66)
        })
        .expect("positional order table admitted")
        .map(|table| (table.declared_count, table.has_prototype, table.rows.len())),
        Some((2, true, 1))
    );
}

#[test]
fn relation_body_rows_state_preserves_invalid_zero_and_empty_counts() {
    assert_eq!(
        RelationBodyRows::from_declared(0),
        RelationBodyRows::InvalidZero
    );
    for declared_count in [1, 2] {
        assert_eq!(
            RelationBodyRows::from_declared(declared_count).get(),
            Some(0)
        );
    }
    assert_eq!(RelationBodyRows::from_declared(3).get(), Some(1));
    assert_eq!(
        RelationBodyRows::from_declared(u32::MAX).get(),
        Some(u32::MAX - 2)
    );
}

#[test]
fn a_solver_subtable_reports_the_shortfall_and_reports_an_over_run_as_none() {
    let table = |declared: u32, decoded: usize| {
        super::SolverSubtable::from_parts(
            Some(super::FeatureSolverTableHeader {
                declared_count: declared,
                entity_ref: 0,
                offset: 0,
            }),
            vec![(); decoded],
        )
        .expect("a declared header always frames a table")
    };

    assert_eq!(table(5, 2).missing_rows(), 3);
    assert!(!table(5, 2).is_complete());

    assert_eq!(table(2, 2).missing_rows(), 0);
    assert!(table(2, 2).is_complete());

    assert_eq!(table(2, 5).missing_rows(), 0);
    assert!(!table(2, 5).is_complete());

    assert_eq!(
        table(u32::MAX, 1).missing_rows(),
        usize::try_from(u32::MAX).expect("fixture index fits usize") - 1
    );
}

#[test]
fn a_saved_spline_admits_only_the_points_the_remaining_bytes_can_state() {
    use super::admitted_interpolation_point_count as admitted;

    // A point is three lanes and a lane consumes at least one byte.
    assert_eq!(admitted(1, 0), None);
    assert_eq!(admitted(1, 2), None);
    assert_eq!(admitted(1, 3), Some(1));
    assert_eq!(admitted(2, 5), None);
    assert_eq!(admitted(2, 6), Some(2));

    // A body with no bytes left states no point, and zero points need no
    // bytes.
    assert_eq!(admitted(0, 0), Some(0));
    assert_eq!(admitted(4, 0), None);
    assert_eq!(admitted(u32::MAX, 0), None);
}

#[test]
fn segment_slots_keep_zero_runs_and_nullable_positions() {
    let payload = [0xe5, 0xf6, 0xe6, 0xe4];
    let mut offset = 0;
    let slots = super::segment_slots(&payload, &mut offset, 7)
        .expect("seven slots fit the fixed segment frame");
    assert_eq!(
        slots,
        [Some(0), Some(0), None, Some(0), Some(0), Some(0), Some(1)]
    );
    assert_eq!(offset, payload.len());

    let mut offset = 0;
    assert_eq!(super::segment_slots(&payload, &mut offset, 1), None);
}
#[test]
fn definition_starts_deduplication_refuses_work() {
    let payload = b"feat_defs_1\0";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo definition starts starts deduplication",
        |ctx| super::definition_starts(ctx, payload),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
    if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        && resource.operation == "creo definition starts starts deduplication")
    );
}

#[test]
fn definitions_deduplication_refuses_work() {
    let payload = b"feat_defs_1\0";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo definitions starts deduplication",
        |ctx| super::definitions(ctx, payload),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
    if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        && resource.operation == "creo definitions starts deduplication")
    );
}

#[test]
fn depdb_definitions_deduplication_refuses_work() {
    let payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo depdb definitions starts deduplication",
        |ctx| super::depdb_definitions(ctx, payload),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
    if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        && resource.operation == "creo depdb definitions starts deduplication")
    );
}

#[test]
fn positional_replay_definitions_deduplication_refuses_work() {
    let payload = b"\xe3S2D0002\0";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo positional replay definitions starts deduplication",
        |ctx| super::positional_replay_definitions(ctx, payload),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
    if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
        && resource.operation == "creo positional replay definitions starts deduplication")
    );
}

mod cost;

#[test]
fn unresolved_guess_search_stops_at_the_first_delimiter() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let short = [0x00, 0x55, 0x01, 0x02, 0x03, 0xe2];
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo variable guess delimiter",
        |ctx| super::unresolved_variable_guess_end(ctx, &short, 0, short.len()),
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    let mut long = short.to_vec();
    long.resize(65_536, 0x55);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used.checked_add(limit.additional).expect("work need");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::unresolved_variable_guess_end(&ctx, &long, 0, long.len())
            .expect("first delimiter scan"),
        Some(2)
    );
}
