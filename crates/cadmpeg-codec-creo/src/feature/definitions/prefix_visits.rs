// SPDX-License-Identifier: Apache-2.0

use super::s2d_replay_starts;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], expected: &[usize]) {
    let windows = u64::try_from(payload.windows(4).len()).expect("fixture value fits u64");
    check_work(
        &[(windows, "creo replay marker traversal")],
        &expected.to_vec(),
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
    expected: &T,
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
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = run(&ctx);
        match result {
            Ok(value) => {
                assert_eq!(ctx.resource_refusal(), None);
                assert_eq!(&value, expected);
                let original = ctx
                    .charge_work_limit(1, "measure feature work")
                    .expect_err("measurement");
                assert_eq!((original.used, original.additional), (total, 1));
                Ok(())
            }
            Err(CodecError::ResourceLimit(original)) => {
                let mut used = 0;
                let (additional, operation) = fees.iter().find_map(|&(fee, operation)| {
                    if used + fee > cap {
                        Some((fee, operation))
                    } else {
                        used += fee;
                        None
                    }
                }).expect("first scheduled operation beyond cap");
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, used, additional, operation));

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
        check_work(&[], &expected, false, |_ctx| {
            Ok(super::depdb_section_name_id(name))
        });
    }
    let unterminated = [b'7'; 128];
    check_work(&[], &None, false, |_ctx| {
        Ok(super::depdb_section_name_id(&unterminated))
    });
    let mut leading_zeros = vec![b'0'; 127];
    leading_zeros.push(0);
    check_work(&[], &Some(0), false, |_ctx| {
        Ok(super::depdb_section_name_id(&leading_zeros))
    });
}

#[test]
fn unresolved_guess_keeps_fixed_suffix_candidates_and_first_delimiter() {
    // Five delimiter visits, then candidate starts1..5. Only start2 has three fields.
    let short = [0x00, 0x55, 0x01, 0x02, 0x03, 0xe2];
    let fees = vec![(1, "creo variable guess delimiter"); 5];
    check_work(&fees, &Some(2), false, |ctx| {
        super::unresolved_variable_guess_end(ctx, &short, 0, short.len())
    });
    let mut long = short.to_vec();
    long.resize(65_536, 0x55);
    check_work(&fees, &Some(2), false, |ctx| {
        super::unresolved_variable_guess_end(ctx, &long, 0, long.len())
    });
    for body in [b"".as_slice(), b"\0", b"\0\xff", b"\0\xff\xff"] {
        let visits = body.windows(2).len();
        check_work(
            &vec![(1, "creo variable guess delimiter"); visits],
            &None,
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
    check_work(&fees, &Vec::<super::FeatureRelation>::new(), false, |ctx| {
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
        check_work(&[], &Vec::<super::FeatureRelation>::new(), false, |ctx| {
            super::positional_relation_rows(ctx, &body, 0, body.len(), rows)
        });
    }
    check_work(&[], &Vec::<super::FeatureRelation>::new(), false, |ctx| {
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
    check_work(&fees, &expected, true, |ctx| {
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
        let fees = vec![(
            u64::try_from(payload.len()).expect("fixture value fits u64"),
            "creo saved generated start traversal",
        )];
        check_work(&fees, &(), false, |ctx| {
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
            &expected,
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
            &expected,
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
            &None,
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
            u64::try_from(payload.len() + b"\xe0\x02params\0\xf8".len())
                .expect("fixture value fits u64"),
            "find Creo feature definition field",
        )];
        fees.extend(vec![(1, "creo saved spline parameters traversal"); visits]);
        if body_bytes != 0 {
            fees.push((body_bytes, "creo saved spline parameter body"));
        }
        check_work(&fees, &expected, true, |ctx| {
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
        check_work(&fees, &expected, true, |ctx| {
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
                u64::try_from(payload.len() + LABEL.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
            (
                u64::try_from(body_len + LABEL.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
            (
                u64::try_from(body_len + POINTS.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
        ];
        fees.extend(vec![
            (1, "creo saved spline entities traversal");
            usize::from(count)
        ]);
        fees.extend([
            (3 + 3 * u64::from(count), "creo saved spline point body"),
            (
                u64::try_from(TANGENTS.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
            (
                u64::try_from(PARAMETERS.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
            // The identifier uses named_compact_int over the empty pre-point range.
            (
                u64::try_from(b"\xe0\x01id\0".len()).expect("fixture value fits u64"),
                "find Creo named integer",
            ),
            (
                u64::try_from(body_len + LABEL.len()).expect("fixture value fits u64"),
                "find Creo feature definition field",
            ),
        ]);
        check_work(&fees, &usize::from(count), true, |ctx| {
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
                u64::try_from(payload.windows(b"gsec2d_ptr\0".len()).len())
                    .expect("fixture value fits u64"),
                "creo DEPDB section marker traversal",
            )],
            &(),
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

#[test]
fn named_section_dimension_ids_stop_at_definition_end() {
    let definition = b"\xe0\0gsec3d_ptr\0dim_id_tab\0\xf8\x01";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(b"feat_defs_2\0");
    crate::decode::with_test_decode_ctx(|ctx| {
        let section = super::section_3d(ctx, &payload, 0, definition.len())?.expect("section");
        assert!(section.dimension_ids.is_empty());
        Ok::<_, CodecError>(())
    }).expect("bounded section");
}

#[test]
fn named_relation_triples_stop_at_definition_end() {
    let definition = b"triples_ptr\0\xf8\x02\xf7\x58\xfb\xe2\
        \xe0\x01rel_id\0\x01\xe0\x01eqn_id\0\x02\xe0\x01skamp_id\0\x03\
        \xf1\xf7\x58\xe2";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[1, 2, 3, 0xe2]);
    crate::decode::with_test_decode_ctx(|ctx| {
        let rows = super::feature_relation_triples(ctx, &payload, 0, definition.len())?;
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].relation_id, rows[0].equation_id, rows[0].skamp_id),
            (Some(1), Some(2), Some(3)));
        Ok::<_, CodecError>(())
    }).expect("bounded triples");
}

#[test]
fn named_skamp_rows_do_not_visit_next_definition() {
    let definition = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
        \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
        \xe0\x01status\0\x04\xe0\0items\0\xf8\x01\xf7\x6c\xfb\xe2\
        \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
        \xf3\xf7\x6b\xe2";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(b"feat_defs_2\0");
    let mut work = [0; 2];
    for (index, bytes) in [definition.as_slice(), payload.as_slice()].into_iter().enumerate() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let rows = super::feature_skamps(&ctx, bytes, 0, definition.len()).expect("bounded skamps");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].id, rows[0].kind, rows[0].flags, rows[0].status), (5, 2, 3, 4));
        assert_eq!((rows[0].items[0].entity_id, rows[0].items[0].sense), (42, 1));
        work[index] = ctx.charge_work_limit(u64::MAX, "measure bounded named skamps")
            .expect_err("measurement").used;
    }
    assert_eq!(work[0], work[1]);
}

#[test]
fn saved_line_body_stops_at_segment_end() {
    let definition = b"\x01\xe2\x0f\x0f\x0f\x0f\x0f\x0f";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[0x0f, 0x0f]);
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut entities = Vec::new();
        super::saved_line_block(ctx, &payload, 0, definition.len(),
            &crate::scalar::ScalarCache::default(), &mut entities)?;
        assert_eq!(entities.len(), 1);
        let super::FeatureSavedEntity::Line(line) = &entities[0] else { panic!("line"); };
        assert_eq!(line.entity_id, 1);
        assert_eq!(line.endpoints, [[Some(0.0); 3]; 2]);
        assert_eq!(line.body, definition);
        Ok::<_, CodecError>(())
    }).expect("bounded line");
}

#[test]
fn named_order_rows_stop_at_definition_end() {
    let definition = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
        \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\xe0\x01bitmask\0\0\
        \xf1\xf7\x42\xe2\x0a";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[2, 1, 0xe2]);
    crate::decode::with_test_decode_ctx(|ctx| {
        let table = super::order_table(ctx, &payload, 0, definition.len())?.expect("header");
        assert!(table.has_prototype);
        assert!(table.rows.is_empty());
        assert!(!table.is_complete());
        Ok::<_, CodecError>(())
    }).expect("bounded order rows");
}

#[test]
fn positional_variable_header_stays_in_definition() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59";
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::positional_variable_table(ctx, payload, 0, 1, 88,
            &crate::scalar::ScalarCache::default())?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded variable header");
}

#[test]
fn named_dimension_header_reference_stays_in_definition() {
    let payload = b"dimtab_ptr\0\xf8\x01\xf7\x58";
    crate::decode::with_test_decode_ctx(|ctx| {
        let table = super::dimension_table(ctx, payload, 0, payload.len()-2,
            &crate::scalar::ScalarCache::default())?.expect("counted header");
        assert_eq!(table.declared_count, 1);
        assert_eq!(table.entity_ref, None);
        assert!(table.rows.is_empty());
        Ok::<_, CodecError>(())
    }).expect("bounded dimension header");
}

#[test]
fn named_dimension_reference_header_stays_in_definition() {
    let payload = b"dim_ref\0\xf8\x01\xf7\x58";
    crate::decode::with_test_decode_ctx(|ctx| {
        let table = super::dimension_reference_table(ctx, payload, 0, payload.len()-2)?
            .expect("counted header");
        assert_eq!(table.declared_count, 1);
        assert_eq!(table.entity_ref, None);
        assert!(table.rows.is_empty());
        Ok::<_, CodecError>(())
    }).expect("bounded dimension reference header");
}

#[test]
fn named_relation_header_reference_stays_in_definition() {
    let payload = b"relat_ptr\0\xf8\x01\xf7\x58";
    crate::decode::with_test_decode_ctx(|ctx| {
        let table = super::relation_table(ctx, payload, 0, payload.len()-2)?.expect("counted header");
        assert_eq!(table.declared_count, 1);
        assert_eq!(table.entity_ref, None);
        assert!(table.rows.is_empty());
        Ok::<_, CodecError>(())
    }).expect("bounded relation header");
}

#[test]
fn named_array_class_stays_in_definition() {
    let payload = b"array\0\xf8\x01\xf7\x58";
    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(super::named_array_class(ctx, payload, b"array\0", 0, 7)?, None);
        Ok::<_, CodecError>(())
    }).expect("bounded array class");
}

#[test]
fn named_solver_header_stays_in_definition() {
    let payload = b"triples_ptr\0\xf8\x01\xf7\x58";
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::named_solver_table_header(ctx, payload, b"triples_ptr\0", 0,
            payload.len()-1)?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded solver header");
}

#[test]
fn named_dimension_reference_point_stays_in_definition() {
    let definition = b"item_id\0\x01sense\0\x02point\0\xf8\x02";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[3, 4]);
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::named_dimension_reference(ctx, &payload, 0, definition.len())?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded dimension point");
}

#[test]
fn labeled_dimension_external_id_stays_in_definition() {
    let definition = b"type\0\x02value\0\xe4direct\0\0aux_value\0\x18ext_id\0";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(b"feat_defs_2\0");
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::labeled_dimension(ctx, &payload, 0, definition.len(),
            &crate::scalar::ScalarCache::default())?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded labeled dimension");
}

#[test]
fn segment_prototype_close_stays_in_definition() {
    let payload = b"\xf8\x01\xf7\x07\xf2\xf7\x07\xe2";
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::segment_table_body(ctx, payload, 0, 0, 6,
            super::PrototypeRow::Elided)?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded segment prototype");
}

#[test]
fn named_variable_key_stays_in_definition() {
    let definition = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\x01\x80";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[0, 0xe2]);
    crate::decode::with_test_decode_ctx(|ctx| {
        let table = super::variable_table(ctx, &payload, 0, definition.len(),
            &crate::scalar::ScalarCache::default())?.expect("declared header");
        assert_eq!(table.declared_count, 2);
        assert_eq!(table.entity_ref, Some(119));
        assert!(table.rows.is_empty());
        Ok::<_, CodecError>(())
    }).expect("bounded variable key");
}

#[test]
fn equation_prototype_body_stays_in_definition() {
    let definition = b"eqtn_arr\0\xf8\x01\xf7\x58\xfb\xe2schema\xf1\xf7";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[88, 0xe2]);
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::equation_table(ctx, &payload, 0, definition.len())?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded equation prototype");
}

#[test]
fn variable_dictionary_scalar_tail_stays_in_window() {
    let payload = [0x53, 0, 0, 0, 0, 0, 0];
    for end in 1..7 {
        check_work(&[], &(super::ScalarLane::Undefined, 1), false, |ctx| {
            super::decode_variable_scalar(ctx, &payload, 0, end,
                &crate::scalar::ScalarCache::default())
        });
    }
    check_work(&[], &(super::ScalarLane::Value(0.1875), 7), false, |ctx| {
        super::decode_variable_scalar(ctx, &payload, 0, payload.len(),
            &crate::scalar::ScalarCache::default())
    });
}

#[test]
fn segment_table_wrapper_does_not_visit_next_definition() {
    let definition = b"segtab_ptr\0";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(&[0xf1, 0xf2, 0xf3]);
    check_same_work([true, true], |ctx, appended| {
        let bytes = if appended { payload.as_slice() } else { definition.as_slice() };
        Ok(super::segment_table(ctx, bytes, 0, definition.len())?.is_none())
    });
}

#[test]
fn named_segment_integer_stays_in_prototype() {
    let payload = b"radius2\0\x80\xf2\xf7\x58\xe2";
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::named_segment_values(ctx, payload, 0, 9, b"radius2\0", 1)?.is_none());
        Ok::<_, CodecError>(())
    }).expect("bounded prototype integer");
}

#[test]
fn outline_empty_and_named_boundary_routes_are_free() {
    for payload in [b"".as_slice(), b"\xe0"] {
        check_work(&[], &(), false, |ctx| {
            let fields = super::outline_scalars(ctx, payload, &crate::scalar::ScalarCache::default())?;
            assert!(fields.iter().all(|field| field.value.is_none() && field.body.is_empty()));
            Ok(())
        });
    }
}

#[test]
fn trim_bucket_count_mismatch_is_free() {
    check_work(&[], &false, false, |ctx| super::complete_bucket_frame(ctx, Some(1), &[]));
}

#[test]
fn dimension_value_fixed_routes_are_free() {
    for (value, expected) in [(Some(1.0), super::DimensionValue::Resolved(1.0)),
        (None, super::DimensionValue::Undefined)] {
        check_work(&[], &expected, false, |ctx| super::DimensionValue::decoded(ctx, value, &[]));
    }
}

#[test]
fn variable_scalar_fixed_and_absent_routes_are_free() {
    for (payload, expected) in [(b"".as_slice(), (super::ScalarLane::Undefined, 0)),
        (b"\x0f".as_slice(), (super::ScalarLane::Value(0.0), 1)),
        (b"\x90\0\0\0\0\0\0".as_slice(), (super::ScalarLane::Value(2.625), 7))] {
        check_work(&[], &expected, false, |ctx| super::decode_variable_scalar(ctx, payload, 0,
            payload.len(), &crate::scalar::ScalarCache::default()));
    }
}

#[test]
fn coordinate_scalar_fixed_routes_are_free() {
    for (payload, expected) in [(b"\0\0\0".as_slice(), (super::ScalarLane::Undefined, 3)),
        (b"\x01\0\0\0".as_slice(), (super::ScalarLane::Undefined, 4)),
        (b"\x2d\0\0\0\0\0\0\0".as_slice(), (super::ScalarLane::Value(2.0), 8))] {
        check_work(&[], &expected, false, |ctx| super::decode_section_coordinate_scalar(ctx,
            payload, 0, payload.len(), &crate::scalar::ScalarCache::default()));
    }
}

#[test]
fn variable_guess_fixed_suffix_is_free() {
    let payload = b"\x18\x01\x02\x03";
    check_work(&[], &(super::ScalarLane::Value(0.0), 1), false, |ctx|
        super::decode_variable_guess(ctx, payload, 0, payload.len(), &crate::scalar::ScalarCache::default()));
}

#[test]
fn equation_invalid_range_is_free() {
    for (start, end) in [(1, 0), (0, 1)] {
        check_work(&[], &None, false, |ctx| super::equation_table(ctx, &[], start, end));
    }
}

#[test]
fn placement_absent_table_is_free() {
    check_work(&[], &None, false, |ctx| {
        let mut instructions = super::PlacementInstructions {
            payload: &[], definition_offset: 0, table_class: None, markers: 0..0,
        };
        instructions.next(ctx)
    });
}

#[test]
fn segment_absent_array_is_free() {
    for prototype in [super::PrototypeRow::Present, super::PrototypeRow::Elided] {
        check_work(&[], &None, false, |ctx|
            super::segment_table_body(ctx, &[], 0, 0, 0, prototype));
    }
}

#[test]
fn positional_dimension_absent_type_is_free() {
    check_work(&[], &None, false, |ctx| super::positional_dimension(ctx, &[], 0, 0,
        &crate::scalar::ScalarCache::default()));
}

#[test]
fn class_close_invalid_window_and_short_extent_are_free() {
    for (start, end) in [(1, 0), (0, 1), (0, 0)] {
        check_work(&[], &None, false, |ctx| super::find_class_close(ctx, &[], start, end, 0xf3, &[]));
    }
}

#[test]
fn saved_scalar_invalid_window_is_free() {
    for (start, end) in [(1, 0), (0, 1)] {
        check_work(&[], &None, false, |ctx| super::saved_named_scalars::<3>(ctx, &[], b"center",
            start, end, &crate::scalar::ScalarCache::default()));
    }
}

#[test]
fn saved_generated_absent_topology_is_free() {
    check_work(&[], &(), false, |ctx| {
        let mut entities = Vec::new();
        super::saved_positional_generated_entities(ctx, &[], 0, 0,
            &crate::scalar::ScalarCache::default(), None, &mut entities)?;
        assert!(entities.is_empty());
        Ok(())
    });
}

#[test]
fn trimmed_ids_absent_table_is_free() {
    let definition = super::FeatureDefinition {
        identity: super::DefinitionIdentity::Parsed { schema_id: None, owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: None, trim_entities: None, trim_vertices: None, order_table: None,
        section_3d: None, dimensions: None, relations: None, saved_section: None, offset: 0,
    };
    check_work(&[], &(&[] as &[u32]), false, |ctx| super::unique_trimmed_external_ids(ctx, &definition));
}

#[test]
fn named_skamp_prototype_does_not_visit_an_absent_item() {
    let payload = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
        \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
        \xe0\x01status\0\x04\xe0\0items\0\xf8\x02\xf7\x6c\xfb\xe2\
        \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
        \xf3\xf7\x6b\xe2";
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        match super::feature_skamps(&ctx, payload, 0, payload.len()) {
            Ok(rows) => {
                assert!(rows.is_empty());
                Ok(())
            }
            Err(CodecError::ResourceLimit(limit)) => {
                assert_ne!(limit.operation, "creo feature skamps cursor traversal");
                Err(CodecError::ResourceLimit(limit))
            }
            Err(error) => Err(error),
        }
    });
}
