// SPDX-License-Identifier: Apache-2.0

use super::s2d_replay_starts;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], expected: &[usize]) {
    let windows = payload.windows(4).len() as u64;
    check_work(
        &[(windows, "creo replay marker traversal")],
        expected.to_vec(),
        !expected.is_empty(),
        |ctx| s2d_replay_starts(ctx, payload),
    );
}

#[test]
fn replay_prefix_keeps_bounded_digit_grammar_without_name_work() {
    for length in [0, 1, 2, 11, 12, 17] {
        let mut payload = b"\xe3S2D".to_vec();
        payload.extend(std::iter::repeat_n(b'7', length));
        check(&payload, &[]);
        payload.push(0);
        check(&payload, if (1..12).contains(&length) { &[0] } else { &[] });
    }
    check(b"\xe3S2DXignored\0", &[]);
    check(b"\xe3S2D12Xignored\0", &[]);
    check(b"", &[]);
    check(b"\xe3S2", &[]);
}

#[test]
fn replay_prefix_preserves_absolute_marker_offsets_and_the_twelve_byte_bound() {
    check(b"junk\xe3S2D123\0ignored", &[4]);
    check(b"\xe3S2D1\0\xe3S2D22\0", &[0, 6]);
    check(b"\xe3S2D12345678901\0", &[0]);
    check(b"\xe3S2D123456789012\0", &[]);
}

fn check_work<T: std::fmt::Debug + PartialEq>(
    fees: &[(u64, &'static str)],
    expected: T,
    allocating: bool,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let total = fees.iter().map(|(fee, _)| fee).sum::<u64>();
    let operations = fees
        .iter()
        .filter(|(fee, _)| *fee > 0)
        .map(|(_, operation)| *operation)
        .collect::<Vec<_>>();
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &operations, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        if !allocating {
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = run(&ctx);
        match result {
            Ok(value) => {
                assert_eq!(ctx.resource_refusal(), None);
                assert_eq!(value, expected);
                let original = ctx
                    .charge_work_limit(1, "measure feature work")
                    .expect_err("measurement");
                assert_eq!((original.used, original.additional), (total, 1));
                assert!(
                    matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                Ok(())
            }
            Err(CodecError::ResourceLimit(original)) => {
                assert_eq!(ctx.resource_refusal(), Some(original));
                assert!(
                    matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                Err(CodecError::ResourceLimit(original))
            }
            Err(error) => Err(error),
        }
    });
}

#[test]
fn depdb_decimal_name_is_bounded_and_free_without_owned_text() {
    for (name, expected) in [
        (b"".as_slice(), None),
        (b"\0ignored".as_slice(), None),
        (b"0\0ignored".as_slice(), Some(0)),
        (b"0002\0ignored".as_slice(), Some(2)),
        (b"4294967295\0".as_slice(), Some(u32::MAX)),
        (b"4294967296\0".as_slice(), None),
        (b"12Xignored\0".as_slice(), None),
        (b"123".as_slice(), None),
    ] {
        check_work(&[], expected, false, |ctx| {
            super::depdb_section_name_id(ctx, name)
        });
    }
    let unterminated = [b'7'; 128];
    check_work(&[], None, false, |ctx| {
        super::depdb_section_name_id(ctx, &unterminated)
    });
    let mut leading_zeros = vec![b'0'; 127];
    leading_zeros.push(0);
    check_work(&[], Some(0), false, |ctx| {
        super::depdb_section_name_id(ctx, &leading_zeros)
    });
}

#[test]
fn unresolved_guess_keeps_fixed_suffix_candidates_and_first_delimiter() {
    // Five delimiter visits, then candidate starts1..5. Only start2 has three fields.
    let short = [0x00, 0x55, 0x01, 0x02, 0x03, 0xe2];
    let fees = vec![(1, "creo variable guess delimiter"); 5];
    check_work(&fees, Some(2), false, |ctx| {
        super::unresolved_variable_guess_end(ctx, &short, 0, short.len())
    });
    let mut long = short.to_vec();
    long.resize(65_536, 0x55);
    check_work(&fees, Some(2), false, |ctx| {
        super::unresolved_variable_guess_end(ctx, &long, 0, long.len())
    });
    for body in [b"".as_slice(), b"\0", b"\0\xff", b"\0\xff\xff"] {
        let visits = body.windows(2).len();
        check_work(
            &vec![(1, "creo variable guess delimiter"); visits],
            None,
            false,
            |ctx| super::unresolved_variable_guess_end(ctx, body, 0, body.len()),
        );
    }
}

#[test]
fn relation_suffix_is_fixed_stops_at_ambiguity_and_skips_absent_rows() {
    let body = [1, 0, 0x80, 0x80, 1, 2, 3, 0xe2];
    // One present row, eight delimiter bytes, then starts2/3/4. Starts3/4 conflict.
    let mut fees = vec![(1, "creo positional relation rows traversal")];
    fees.extend([(1, "creo relation row end"); 8]);
    check_work(&fees, Vec::<super::FeatureRelation>::new(), false, |ctx| {
        super::positional_relation_rows(
            ctx,
            &body,
            0,
            body.len(),
            super::RelationBodyRows::Count(1),
        )
    });
    for rows in [
        super::RelationBodyRows::Count(0),
        super::RelationBodyRows::InvalidZero,
    ] {
        check_work(&[], Vec::<super::FeatureRelation>::new(), false, |ctx| {
            super::positional_relation_rows(ctx, &body, 0, body.len(), rows)
        });
    }
    check_work(&[], Vec::<super::FeatureRelation>::new(), false, |ctx| {
        super::positional_relation_rows(ctx, &[], 0, 0, super::RelationBodyRows::Count(1))
    });

    let body = [1, 0, 4, 1, 2, 3, 0xe2];
    let mut fees = vec![(1, "creo positional relation rows traversal")];
    fees.extend([(1, "creo relation row end"); 7]);
    fees.extend([(1, "creo relation operands"), (6, "creo relation row body")]);
    let expected = vec![super::FeatureRelation {
        relation_id: 1,
        used: 0,
        operands: vec![4],
        operand_vectors: None,
        sign: 1,
        dimension_id: 2,
        relation_type: 3,
        body: body[..6].to_vec(),
        offset: 0,
    }];
    check_work(&fees, expected, true, |ctx| {
        super::positional_relation_rows(
            ctx,
            &body,
            0,
            body.len(),
            super::RelationBodyRows::Count(1),
        )
    });
}

#[test]
fn saved_generated_header_is_free_within_twenty_four_byte_bound() {
    let order = super::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![super::FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }]
        .into(),
        offset: 0,
    };
    let segments = super::FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::segment_rows::SegmentRow::Ordinary(
            super::FeatureSegment {
                kind: super::FeatureSegmentKind::Arc([1, 2]),
                directions: [None; 3],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                body: Vec::new(),
                offset: 0,
            },
        )]
        .into_iter()
        .collect(),
        offset: 0,
    };
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 23, 24, 25] {
        let mut payload = vec![0xe3, 7];
        payload.extend(std::iter::repeat_n(0xff, length));
        let fees = vec![(payload.len() as u64, "creo saved generated start traversal")];
        check_work(&fees, (), false, |ctx| {
            let mut entities = Vec::new();
            super::saved_positional_generated_entities(
                ctx,
                &payload,
                0,
                payload.len(),
                &cache,
                super::SavedEntityTopology::from_tables(Some(&order), Some(&segments)),
                &mut entities,
            )?;
            assert!(entities.is_empty());
            Ok(())
        });
    }
}

#[test]
fn equation_arguments_admit_present_tokens_without_an_absent_source_visit() {
    for (payload, end, count, visits, expected) in [
        (b"".as_slice(), 0, Some(0), 0, Some(0)),
        (b"".as_slice(), 0, Some(1), 0, None),
        (b"".as_slice(), 1, None, 0, None),
        (b"\x01".as_slice(), 1, Some(1), 1, Some(1)),
        (b"\x01".as_slice(), 1, Some(2), 1, None),
        (b"\xe6".as_slice(), 1, Some(3), 1, Some(3)),
        (b"\xe6".as_slice(), 1, Some(4), 1, None),
        (b"\xf6".as_slice(), 1, None, 0, Some(0)),
    ] {
        check_work(
            &vec![(1, "creo equation argument traversal"); visits],
            expected,
            true,
            |ctx| {
                let mut offset = 0;
                Ok(
                    super::equation_arguments(ctx, payload, &mut offset, end, count)?
                        .map(|owned| owned.0.len()),
                )
            },
        );
    }
}

#[test]
fn skamp_boundary_admits_only_present_items_and_preserves_missing_field_recovery() {
    for (payload, count, visits, expected) in [
        (b"".as_slice(), 0, 0, Some(0)),
        (b"".as_slice(), 1, 0, None),
        (b"\x01".as_slice(), 1, 1, None),
        (b"\x01\x02".as_slice(), 1, 1, Some(2)),
        (b"\x01\x02".as_slice(), 2, 1, None),
        (b"\x01\x02\xe2".as_slice(), 2, 1, None),
        (b"\x01\x02\xe2\x03\x04".as_slice(), 2, 2, Some(5)),
    ] {
        check_work(
            &vec![(1, "creo skamp item boundary traversal"); visits],
            expected,
            false,
            |ctx| {
                super::positional_skamp_item_array_body_end(
                    ctx,
                    payload,
                    0,
                    count,
                    &[],
                    payload.len(),
                )
            },
        );
    }
}

#[test]
fn dimension_candidate_scan_admits_present_offsets_and_no_end_probe() {
    for payload in [b"".as_slice(), b"\xff", b"\xff\xff\xff"] {
        check_work(
            &vec![(1, "creo self described dimension traversal"); payload.len()],
            None,
            false,
            |ctx| {
                super::self_described_positional_dimension_table(
                    ctx,
                    payload,
                    0,
                    payload.len(),
                    &crate::scalar::ScalarCache::default(),
                )
            },
        );
    }
}

#[test]
fn spline_parameter_count_admits_present_values_and_preserves_incomplete_recovery() {
    for (suffix, count, visits, body_bytes, expected) in [
        (b"\x00".as_slice(), 0, 0, 2, Some(0)),
        (b"\x01".as_slice(), 1, 0, 0, None),
        (b"\x01\x0f".as_slice(), 1, 1, 3, Some(1)),
        (b"\x02\x0f".as_slice(), 2, 1, 0, None),
        (b"\x01\xff".as_slice(), 1, 1, 0, None),
    ] {
        let mut payload = b"\xe0\x02params\0\xf8".to_vec();
        payload.extend_from_slice(suffix);
        // The borrowed memmem search admits both extents; each scalar visit and copy follows.
        let mut fees = vec![(
            (payload.len() + b"\xe0\x02params\0\xf8".len()) as u64,
            "find Creo feature definition field",
        )];
        fees.extend(vec![(1, "creo saved spline parameters traversal"); visits]);
        if body_bytes != 0 {
            fees.push((body_bytes, "creo saved spline parameter body"));
        }
        check_work(&fees, expected, true, |ctx| {
            Ok(super::saved_spline_parameters(
                ctx,
                &payload,
                0,
                payload.len(),
                count,
                &crate::scalar::ScalarCache::default(),
            )?
            .map(|field| field.value.len()))
        });
    }
}

#[test]
fn saved_line_preamble_and_trailer_do_not_visit_absent_source() {
    let cache = crate::scalar::ScalarCache::default();
    for (payload, visits, body_bytes, expected) in [
        (b"".as_slice(), 0, 0, 0),
        (b"\xf7\x01".as_slice(), 2, 0, 0),
        (b"\x01\xe2".as_slice(), 2, 2, 1),
        (b"\x01\xe2\x0f\x0f\x0f\x0f\x0f\x0f".as_slice(), 8, 8, 1),
    ] {
        // One row visit, actual preamble probes and up to six scalar slots; EOF is free.
        let mut fees = vec![(1, "creo saved line block cursor traversal"); visits];
        if body_bytes != 0 {
            fees.push((body_bytes, "creo saved line body"));
        }
        check_work(&fees, expected, true, |ctx| {
            let mut entities = Vec::new();
            super::saved_line_block(ctx, payload, 0, payload.len(), &cache, &mut entities)?;
            Ok(entities.len())
        });
    }
}

fn check_same_work<T: std::fmt::Debug + PartialEq>(
    expected: [T; 2],
    run: impl Fn(&DecodeContext<'_>, bool) -> Result<T, CodecError>,
) {
    let mut work = [0; 2];
    for (index, expected) in expected.into_iter().enumerate() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(run(&ctx, index != 0).expect("bounded recovery"), expected);
        let original = ctx
            .charge_work_limit(u64::MAX, "measure paired source work")
            .expect_err("measurement refuses after executed work");
        work[index] = original.used;
        assert!(matches!(run(&ctx, index != 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(work[0], work[1], "same source scans; no absent count visit");
}

#[test]
fn named_section_missing_reference_and_dimension_source_costs_no_count_visit() {
    for prefix in [
        b"\xe0\x00gsec3d_ptr\0\xe0\x00ref_planes\0\xf8".as_slice(),
        b"\xe0\x00gsec3d_ptr\0dim_id_tab\0\xf8".as_slice(),
    ] {
        check_same_work([0, 0], |ctx, nonzero| {
            let mut payload = prefix.to_vec();
            payload.push(if nonzero { 127 } else { 0 });
            let section = super::section_3d(ctx, &payload, 0, payload.len())?.expect("section");
            Ok(section.reference_planes.entity_ids().count() + section.dimension_ids.len())
        });
    }
}

#[test]
fn positional_section_missing_row_source_costs_no_count_visit() {
    check_same_work([0, 0], |ctx, nonzero| {
        let mut payload = b"\x07S2D1\0\x01\xf6\xe1\xf6\x02\x00\xf8".to_vec();
        payload.push(u8::from(nonzero));
        payload.extend_from_slice(b"\xf7\x39\xfb\xe2\xf7\x3a");
        let section =
            super::positional_section_3d(ctx, &payload, 0, payload.len())?.expect("section");
        assert_eq!(section.sketch_plane_entity_id, Some(2));
        Ok(section.reference_planes.entity_ids().count())
    });
}

#[test]
fn named_skamp_missing_row_and_item_source_costs_no_count_visit() {
    let prototype = b"\xf7\x6b\xfb\xe2\
        \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
        \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
        \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
        \xf3\xf7\x6b\xe2";
    check_same_work([1, 1], |ctx, extra_row| {
        let mut payload = b"skamp_ptr\0\xf3\xf8".to_vec();
        payload.push(if extra_row { 2 } else { 1 });
        payload.extend_from_slice(prototype);
        let rows = super::feature_skamps(ctx, &payload, 0, payload.len())?;
        assert_eq!(rows[0].id, 5);
        assert_eq!(rows[0].items[0].entity_id, 42);
        Ok(rows.len())
    });
    check_same_work([1, 1], |ctx, nonzero_items| {
        let mut payload = b"skamp_ptr\0\xf3\xf8\x02".to_vec();
        payload.extend_from_slice(prototype);
        payload.extend_from_slice(b"\x06\x02\x03\x04\xf8");
        payload.push(u8::from(nonzero_items));
        payload.extend_from_slice(b"\xf7\x6c\xfb\xe2");
        Ok(super::feature_skamps(ctx, &payload, 0, payload.len())?.len())
    });
}

#[test]
fn positional_skamp_missing_row_and_item_source_costs_no_count_visit() {
    check_same_work([0, 0], |ctx, nonzero| {
        let payload = [0xf8, u8::from(nonzero), 0xf7, 88, 0xfb, 0xe2, 0xf7, 89];
        let table =
            super::positional_feature_skamps(ctx, &payload, 0, payload.len(), 88)?.expect("table");
        Ok(table.rows().len())
    });
    check_same_work([1, 0], |ctx, nonzero_items| {
        let mut payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\x00\x00\x23\xf8".to_vec();
        payload.push(u8::from(nonzero_items));
        payload.extend_from_slice(b"\xf7\x60\xfb\xe2\xf7\x61");
        let table =
            super::positional_feature_skamps(ctx, &payload, 0, payload.len(), 88)?.expect("table");
        Ok(table.rows().len())
    });
}

#[test]
fn named_relation_triples_missing_row_source_costs_no_count_visit() {
    check_same_work([1, 1], |ctx, extra_row| {
        let mut payload = b"triples_ptr\0\xf4\x04\xf8".to_vec();
        payload.push(if extra_row { 2 } else { 1 });
        payload.extend_from_slice(b"\xf7\x64\xfb\xe2schema\xf1\xf7\x64\xe2");
        Ok(super::feature_relation_triples(ctx, &payload, 0, payload.len())?.len())
    });
}

#[test]
fn positional_relation_triples_missing_row_source_costs_no_count_visit() {
    check_same_work([0, 0], |ctx, nonzero| {
        let payload = [0xf8, u8::from(nonzero), 0xf7, 100, 0xfb, 0xe2, 0xf7, 101];
        let table = super::positional_relation_triples(ctx, &payload, 0, payload.len(), 100)?
            .expect("table");
        Ok(table.rows().len())
    });
}

#[test]
fn saved_spline_complete_point_count_has_no_end_probe() {
    const LABEL: &[u8] = b"\xe0\x00save_entity_ptr(spline)\0";
    const POINTS: &[u8] = b"\xe0\x02i_pnts\0\xf9";
    const TANGENTS: &[u8] = b"\xe0\x02end_tangts\0\xf9\x02\x03";
    const PARAMETERS: &[u8] = b"\xe0\x02params\0\xf8";
    for count in [0_u8, 1] {
        let mut payload = LABEL.to_vec();
        payload.extend_from_slice(POINTS);
        payload.extend_from_slice(&[count, 3]);
        if count != 0 {
            payload.extend_from_slice(&[0x0f; 3]);
        }
        let body_len = payload.len() - LABEL.len();
        let mut fees = vec![
            (
                (payload.len() + LABEL.len()) as u64,
                "find Creo feature definition field",
            ),
            (
                (body_len + LABEL.len()) as u64,
                "find Creo feature definition field",
            ),
            (
                (body_len + POINTS.len()) as u64,
                "find Creo feature definition field",
            ),
        ];
        fees.extend(vec![
            (1, "creo saved spline entities traversal");
            usize::from(count)
        ]);
        fees.extend([
            (3 + 3 * u64::from(count), "creo saved spline point body"),
            (TANGENTS.len() as u64, "find Creo feature definition field"),
            (
                PARAMETERS.len() as u64,
                "find Creo feature definition field",
            ),
            // The identifier uses named_compact_int over the empty pre-point range.
            (b"\xe0\x01id\0".len() as u64, "find Creo named integer"),
            (
                (body_len + LABEL.len()) as u64,
                "find Creo feature definition field",
            ),
        ]);
        check_work(&fees, usize::from(count), true, |ctx| {
            let mut entities = Vec::new();
            super::saved_spline_entities(
                ctx,
                &payload,
                0,
                payload.len(),
                &crate::scalar::ScalarCache::default(),
                &mut entities,
            )?;
            let [super::FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
                panic!("one spline");
            };
            assert_eq!(spline.declared_point_count, Some(u32::from(count)));
            Ok(spline.interpolation_points.len())
        });
    }
}

#[test]
fn outline_empty_and_named_boundary_routes_preserve_original_refusal() {
    for payload in [b"".as_slice(), b"\xe0"] {
        check_work(&[], (), false, |ctx| {
            let fields =
                super::outline_scalars(ctx, payload, &crate::scalar::ScalarCache::default())?;
            assert!(fields
                .iter()
                .all(|field| field.value.is_none() && field.body.is_empty()));
            Ok(())
        });
    }
}

#[test]
fn trim_bucket_count_mismatch_preserves_original_refusal() {
    check_work(&[], false, false, |ctx| {
        super::complete_bucket_frame(ctx, Some(1), &[])
    });
}

#[test]
fn dimension_value_fixed_routes_preserve_original_refusal() {
    for (value, expected) in [
        (Some(1.0), super::DimensionValue::Resolved(1.0)),
        (None, super::DimensionValue::Undefined),
    ] {
        check_work(&[], expected, false, |ctx| {
            super::DimensionValue::decoded(ctx, value, &[])
        });
    }
}

#[test]
fn solver_offset_refusal_preserves_header_and_rows_before_mutation() {
    let row = super::FeatureRelationTriple {
        relation_id: Some(1),
        equation_id: Some(2),
        skamp_id: Some(3),
        offset: 11,
    };
    for mut table in [
        super::SolverSubtable::Declared {
            header: super::FeatureSolverTableHeader {
                declared_count: 0,
                entity_ref: 7,
                offset: 5,
            },
            rows: Vec::new(),
        },
        super::SolverSubtable::Declared {
            header: super::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 7,
                offset: 5,
            },
            rows: vec![row.clone()],
        },
        super::SolverSubtable::Unframed(super::NonEmptySolverRows(vec![row])),
    ] {
        let original_table = table.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = ctx
            .charge_work_limit(1, "original solver refusal")
            .expect_err("fused");
        assert!(matches!(table.shift_offsets(&ctx, 3),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(table, original_table);
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn variable_scalar_fixed_and_absent_routes_preserve_original_refusal() {
    for (payload, expected) in [
        (b"".as_slice(), (super::ScalarLane::Undefined, 0)),
        (b"\x0f".as_slice(), (super::ScalarLane::Value(0.0), 1)),
        (
            b"\x90\0\0\0\0\0\0".as_slice(),
            (super::ScalarLane::Value(2.625), 7),
        ),
    ] {
        check_work(&[], expected, false, |ctx| {
            super::decode_variable_scalar(
                ctx,
                payload,
                0,
                payload.len(),
                &crate::scalar::ScalarCache::default(),
            )
        });
    }
}

#[test]
fn coordinate_scalar_fixed_routes_preserve_original_refusal() {
    for (payload, expected) in [
        (b"\0\0\0".as_slice(), (super::ScalarLane::Undefined, 3)),
        (b"\x01\0\0\0".as_slice(), (super::ScalarLane::Undefined, 4)),
        (
            b"\x2d\0\0\0\0\0\0\0".as_slice(),
            (super::ScalarLane::Value(2.0), 8),
        ),
    ] {
        check_work(&[], expected, false, |ctx| {
            super::decode_section_coordinate_scalar(
                ctx,
                payload,
                0,
                payload.len(),
                &crate::scalar::ScalarCache::default(),
            )
        });
    }
}

#[test]
fn variable_guess_fixed_suffix_preserves_original_refusal() {
    let payload = b"\x18\x01\x02\x03";
    check_work(&[], (super::ScalarLane::Value(0.0), 1), false, |ctx| {
        super::decode_variable_guess(
            ctx,
            payload,
            0,
            payload.len(),
            &crate::scalar::ScalarCache::default(),
        )
    });
}

#[test]
fn equation_invalid_range_preserves_original_refusal() {
    for (start, end) in [(1, 0), (0, 1)] {
        check_work(&[], None, false, |ctx| {
            super::equation_table(ctx, &[], start, end)
        });
    }
}

#[test]
fn placement_absent_table_preserves_original_refusal() {
    check_work(&[], None, false, |ctx| {
        let mut instructions = super::PlacementInstructions {
            payload: &[],
            definition_offset: 0,
            table_class: None,
            markers: 0..0,
        };
        instructions.next(ctx)
    });
}

#[test]
fn segment_absent_array_preserves_original_refusal() {
    for prototype in [super::PrototypeRow::Present, super::PrototypeRow::Elided] {
        check_work(&[], None, false, |ctx| {
            super::segment_table_body(ctx, &[], 0, 0, 0, prototype)
        });
    }
}

#[test]
fn positional_dimension_absent_type_preserves_original_refusal() {
    check_work(&[], None, false, |ctx| {
        super::positional_dimension(ctx, &[], 0, 0, &crate::scalar::ScalarCache::default())
    });
}

#[test]
fn class_close_invalid_window_and_short_extent_preserve_original_refusal() {
    for (start, end) in [(1, 0), (0, 1), (0, 0)] {
        check_work(&[], None, false, |ctx| {
            super::find_class_close(ctx, &[], start, end, 0xf3, &[])
        });
    }
}

#[test]
fn saved_scalar_invalid_window_preserves_original_refusal() {
    for (start, end) in [(1, 0), (0, 1)] {
        check_work(&[], None, false, |ctx| {
            super::saved_named_scalars::<3>(
                ctx,
                &[],
                b"center",
                start,
                end,
                &crate::scalar::ScalarCache::default(),
            )
        });
    }
}

#[test]
fn saved_generated_absent_topology_preserves_original_refusal() {
    check_work(&[], (), false, |ctx| {
        let mut entities = Vec::new();
        super::saved_positional_generated_entities(
            ctx,
            &[],
            0,
            0,
            &crate::scalar::ScalarCache::default(),
            None,
            &mut entities,
        )?;
        assert!(entities.is_empty());
        Ok(())
    });
}

#[test]
fn trimmed_ids_absent_table_preserves_original_refusal() {
    let definition = super::FeatureDefinition {
        identity: super::DefinitionIdentity::Parsed {
            schema_id: None,
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    check_work(&[], &[] as &[u32], false, |ctx| {
        super::unique_trimmed_external_ids(ctx, &definition)
    });
}

#[test]
fn positional_segment_marker_search_is_free_within_256_bytes() {
    for length in [0, 1, 254, 255, 256, 300] {
        let mut payload = vec![0xff; length];
        payload.extend_from_slice(b"S2D1\0");
        if length < 254 {
            payload.truncate(length);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            super::positional_segment_table(&ctx, &payload, 0, payload.len())
                .expect("missing bounded marker uses no work")
                .is_none()
        );
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn positional_segment_invalid_range_has_no_match_or_work() {
    let payload = b"S2D1\0";
    for (start, end) in [(1, 0), (payload.len() + 1, payload.len()), (0, usize::MAX)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(super::positional_segment_table(&ctx, payload, start, end)
            .expect("invalid range uses no work")
            .is_none());
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn depdb_name_marker_search_and_decimal_parse_add_no_bounded_work() {
    for payload in [
        b"gsec2d_ptr\0name\0S2D1\0".as_slice(),
        b"gsec2d_ptr\0missing",
        b"gsec2d_ptr\0name\0S2D4294967296\0",
    ] {
        check_work(
            &[(
                payload.windows(b"gsec2d_ptr\0".len()).len() as u64,
                "creo DEPDB section marker traversal",
            )],
            (),
            true,
            |ctx| {
                let starts = super::depdb_gsec2d_starts(ctx, payload)?;
                if payload.ends_with(b"S2D1\0") {
                    let [start] = starts.as_slice() else {
                        panic!("one DEPDB start");
                    };
                    assert_eq!(start.offset, 0);
                    assert_eq!(start.id.map(std::num::NonZeroU32::get), Some(1));
                    assert!(!start.positional);
                    assert_eq!(start.owner_override, None);
                } else {
                    assert!(starts.is_empty());
                }
                Ok(())
            },
        );
    }
}

#[test]
fn standalone_depdb_missing_name_has_only_section_search_work() {
    let payload = b"gsec2d_ptr\0missing";
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &[
            "creo standalone section search",
            "creo standalone section uniqueness",
        ],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            match super::depdb_section_definition(&ctx, payload, None) {
                Ok(definition) => {
                    assert!(definition.is_none());
                    Ok(())
                }
                Err(CodecError::ResourceLimit(refusal)) => {
                    assert!(matches!(
                        refusal.operation,
                        "creo standalone section search" | "creo standalone section uniqueness"
                    ));
                    Err(CodecError::ResourceLimit(refusal))
                }
                Err(error) => Err(error),
            }
        },
    );
}
