// SPDX-License-Identifier: Apache-2.0

use super::s2d_replay_starts;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], visits: usize, expected: &[usize]) {
    // Every four-byte marker window is admitted once, followed by actual name bytes.
    let windows = payload.len().saturating_sub(3) as u64;
    let total = windows + visits as u64;
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        if expected.is_empty() {
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = s2d_replay_starts(&ctx, payload);
        if cap == total {
            assert_eq!(result.expect("exact marker and name visits"), expected);
            let original = ctx.charge_work_limit(1, "after replay prefix")
                .expect_err("all executed work is accounted");
            assert_eq!((original.used, original.additional), (total, 1));
        } else {
            let original = ctx.resource_refusal().expect("actual work refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            let expected = if cap < windows {
                (0, windows, "creo replay marker traversal")
            } else {
                (cap, 1, "creo replay name prefix scan")
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(s2d_replay_starts(&ctx, payload),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn replay_prefix_admits_present_digits_through_terminator_or_invalid_byte() {
    for length in [0, 1, 2, 11, 12, 17] {
        let mut payload = b"\xe3S2D".to_vec();
        payload.extend(std::iter::repeat_n(b'7', length));
        check(&payload, length.min(12), &[]);
        payload.push(0);
        check(&payload, (length + 1).min(12),
            if (1..12).contains(&length) { &[0] } else { &[] });
    }
    check(b"\xe3S2DXignored\0", 1, &[]);
    check(b"\xe3S2D12Xignored\0", 3, &[]);
    check(b"", 0, &[]);
    check(b"\xe3S2", 0, &[]);
}

#[test]
fn replay_prefix_preserves_absolute_marker_offsets_and_the_twelve_byte_bound() {
    check(b"junk\xe3S2D123\0ignored", 4, &[4]);
    check(b"\xe3S2D1\0\xe3S2D22\0", 5, &[0, 6]);
    check(b"\xe3S2D12345678901\0", 12, &[0]);
    check(b"\xe3S2D123456789012\0", 12, &[]);
}

fn check_work<T: std::fmt::Debug + PartialEq>(
    fees: &[(u64, &'static str)],
    expected: T,
    allocating: bool,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let total = fees.iter().map(|(fee, _)| fee).sum::<u64>();
    for cap in 0..=total {
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
        if cap == total {
            assert_eq!(result.expect("source-derived exact work"), expected);
            let original = ctx.charge_work_limit(1, "after bounded candidates").expect_err("exact work");
            assert_eq!((original.used, original.additional), (total, 1));
        } else {
            let mut used = 0;
            let (fee, operation) = fees.iter().find_map(|&(fee, operation)| {
                if used + fee > cap { Some((fee, operation)) }
                else { used += fee; None }
            }).expect("first operation beyond cap");
            let original = ctx.resource_refusal().expect("actual operation refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, used, fee, operation));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn depdb_decimal_name_admits_prefix_validation_and_parse_without_owned_text() {
    for (name, visits, digits, expected) in [
        (b"".as_slice(), 0, 0, None),
        (b"\0ignored".as_slice(), 1, 0, None),
        (b"0\0ignored".as_slice(), 2, 1, Some(0)),
        (b"0002\0ignored".as_slice(), 5, 4, Some(2)),
        (b"4294967295\0".as_slice(), 11, 10, Some(u32::MAX)),
        (b"4294967296\0".as_slice(), 11, 10, None),
        (b"12Xignored\0".as_slice(), 3, 0, None),
        (b"123".as_slice(), 3, 0, None),
    ] {
        let mut fees = vec![(1, "creo DEPDB section name scan"); visits];
        if digits != 0 {
            fees.extend([(digits, "creo UTF-8 validation"), (digits, "creo scalar text parsing")]);
        }
        check_work(&fees, expected, false, |ctx| super::depdb_section_name_id(ctx, name));
    }
    let unterminated = [b'7'; 128];
    check_work(&[(1, "creo DEPDB section name scan"); 128], None, false,
        |ctx| super::depdb_section_name_id(ctx, &unterminated));
}

#[test]
fn unresolved_guess_admits_actual_suffix_candidates_and_preserves_first_delimiter() {
    // Five delimiter visits, then candidate starts1..5. Only start2 has three fields.
    let short = [0x00, 0x55, 0x01, 0x02, 0x03, 0xe2];
    let mut fees = vec![(1, "creo variable guess delimiter"); 5];
    fees.extend([(1, "creo variable guess suffix scan"); 4]);
    check_work(&fees, Some(2), false,
        |ctx| super::unresolved_variable_guess_end(ctx, &short, 0, short.len()));
    let mut long = short.to_vec();
    long.resize(65_536, 0x55);
    check_work(&fees, Some(2), false,
        |ctx| super::unresolved_variable_guess_end(ctx, &long, 0, long.len()));
    for body in [b"".as_slice(), b"\0", b"\0\xff", b"\0\xff\xff"] {
        let visits = body.len().saturating_sub(1);
        check_work(&vec![(1, "creo variable guess delimiter"); visits], None, false,
            |ctx| super::unresolved_variable_guess_end(ctx, body, 0, body.len()));
    }
}

#[test]
fn relation_suffix_admits_candidates_stops_at_ambiguity_and_skips_absent_rows() {
    let body = [1, 0, 0x80, 0x80, 1, 2, 3, 0xe2];
    // One present row, eight delimiter bytes, then starts2/3/4. Starts3/4 conflict.
    let mut fees = vec![(1, "creo positional relation rows traversal")];
    fees.extend([(1, "creo relation row end"); 8]);
    fees.extend([(1, "creo positional relation suffix scan"); 3]);
    check_work(&fees, Vec::<super::FeatureRelation>::new(), false, |ctx|
        super::positional_relation_rows(ctx, &body, 0, body.len(), super::RelationBodyRows::Count(1)));
    for rows in [super::RelationBodyRows::Count(0), super::RelationBodyRows::InvalidZero] {
        check_work(&[], Vec::<super::FeatureRelation>::new(), false, |ctx|
            super::positional_relation_rows(ctx, &body, 0, body.len(), rows));
    }
    check_work(&[], Vec::<super::FeatureRelation>::new(), false, |ctx|
        super::positional_relation_rows(ctx, &[], 0, 0, super::RelationBodyRows::Count(1)));

    let body = [1, 0, 4, 1, 2, 3, 0xe2];
    let mut fees = vec![(1, "creo positional relation rows traversal")];
    fees.extend([(1, "creo relation row end"); 7]);
    fees.extend([(1, "creo positional relation suffix scan"); 4]);
    fees.extend([(1, "creo relation operands"), (6, "creo relation row body")]);
    let expected = vec![super::FeatureRelation {
        relation_id: 1, used: 0, operands: vec![4], operand_vectors: None,
        sign: 1, dimension_id: 2, relation_type: 3, body: body[..6].to_vec(), offset: 0,
    }];
    check_work(&fees, expected, true, |ctx|
        super::positional_relation_rows(ctx, &body, 0, body.len(), super::RelationBodyRows::Count(1)));
}

#[test]
fn saved_generated_header_admits_present_bytes_within_twenty_four_byte_bound() {
    let order = super::FeatureOrderTable {
        declared_count: 1, has_prototype: false, entity_ref: None,
        rows: vec![super::FeatureOrderRow { external_id: 42, internal_id: 7, bitmask: 0, offset: 0 }].into(),
        offset: 0,
    };
    let segments = super::FeatureSegmentTable {
        declared_count: 1, has_elided_prototype: false, entity_ref: None,
        rows: vec![crate::feature::segment_rows::SegmentRow::Ordinary(super::FeatureSegment {
            kind: super::FeatureSegmentKind::Arc([1, 2]), directions: [None; 3],
            center_id: Some(3), arc_orientation: Some(0), vertical_horizontal: None,
            radius_ref: None, radius2_ref: None, external_id: 42, body: Vec::new(), offset: 0,
        })].into_iter().collect(), offset: 0,
    };
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 23, 24, 25] {
        let mut payload = vec![0xe3, 7];
        payload.extend(std::iter::repeat_n(0xff, length));
        let mut fees = vec![(payload.len() as u64, "creo saved generated start traversal")];
        fees.extend(vec![(1, "creo saved generated header scan"); length.min(24)]);
        check_work(&fees, (), false, |ctx| {
            let mut entities = Vec::new();
            super::saved_positional_generated_entities(ctx, &payload, 0, payload.len(), &cache,
                super::SavedEntityTopology::from_tables(Some(&order), Some(&segments)), &mut entities)?;
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
        check_work(&vec![(1, "creo equation argument traversal"); visits], expected, true, |ctx| {
            let mut offset = 0;
            Ok(super::equation_arguments(ctx, payload, &mut offset, end, count)?
                .map(|owned| owned.0.len()))
        });
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
        check_work(&vec![(1, "creo skamp item boundary traversal"); visits], expected, false,
            |ctx| super::positional_skamp_item_array_body_end(ctx, payload, 0, count, &[], payload.len()));
    }
}

#[test]
fn dimension_candidate_scan_admits_present_offsets_and_no_end_probe() {
    for payload in [b"".as_slice(), b"\xff", b"\xff\xff\xff"] {
        check_work(&vec![(1, "creo self described dimension traversal"); payload.len()], None, false,
            |ctx| super::self_described_positional_dimension_table(ctx, payload, 0, payload.len(),
                &crate::scalar::ScalarCache::default()));
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
        let mut fees = vec![((payload.len() + b"\xe0\x02params\0\xf8".len()) as u64,
            "find Creo feature definition field")];
        fees.extend(vec![(1, "creo saved spline parameters traversal"); visits]);
        if body_bytes != 0 { fees.push((body_bytes, "creo saved spline parameter body")); }
        check_work(&fees, expected, true, |ctx| {
            Ok(super::saved_spline_parameters(ctx, &payload, 0, payload.len(), count,
                &crate::scalar::ScalarCache::default())?.map(|field| field.value.len()))
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
        if body_bytes != 0 { fees.push((body_bytes, "creo saved line body")); }
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
        let original = ctx.charge_work_limit(u64::MAX, "measure paired source work")
            .expect_err("measurement refuses after executed work");
        work[index] = original.used;
        assert!(matches!(run(&ctx, index != 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(work[0], work[1], "same source scans; no absent count visit");
}

#[test]
fn named_section_missing_reference_and_dimension_source_costs_no_count_visit() {
    for prefix in [b"\xe0\x00gsec3d_ptr\0\xe0\x00ref_planes\0\xf8".as_slice(),
        b"\xe0\x00gsec3d_ptr\0dim_id_tab\0\xf8".as_slice()] {
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
        let section = super::positional_section_3d(ctx, &payload, 0, payload.len())?
            .expect("section");
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
        let table = super::positional_feature_skamps(ctx, &payload, 0, payload.len(), 88)?
            .expect("table");
        Ok(table.rows().len())
    });
    check_same_work([1, 0], |ctx, nonzero_items| {
        let mut payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\x00\x00\x23\xf8".to_vec();
        payload.push(u8::from(nonzero_items));
        payload.extend_from_slice(b"\xf7\x60\xfb\xe2\xf7\x61");
        let table = super::positional_feature_skamps(ctx, &payload, 0, payload.len(), 88)?
            .expect("table");
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
        if count != 0 { payload.extend_from_slice(&[0x0f; 3]); }
        let body_len = payload.len() - LABEL.len();
        let mut fees = vec![
            ((payload.len() + LABEL.len()) as u64, "find Creo feature definition field"),
            ((body_len + LABEL.len()) as u64, "find Creo feature definition field"),
            ((body_len + POINTS.len()) as u64, "find Creo feature definition field"),
        ];
        fees.extend(vec![(1, "creo saved spline entities traversal"); usize::from(count)]);
        fees.extend([
            (3 + 3 * u64::from(count), "creo saved spline point body"),
            (TANGENTS.len() as u64, "find Creo feature definition field"),
            (PARAMETERS.len() as u64, "find Creo feature definition field"),
            // The identifier uses named_compact_int over the empty pre-point range.
            (b"\xe0\x01id\0".len() as u64, "find Creo named integer"),
            ((body_len + LABEL.len()) as u64, "find Creo feature definition field"),
        ]);
        check_work(&fees, usize::from(count), true, |ctx| {
            let mut entities = Vec::new();
            super::saved_spline_entities(ctx, &payload, 0, payload.len(),
                &crate::scalar::ScalarCache::default(), &mut entities)?;
            let [super::FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
                panic!("one spline");
            };
            assert_eq!(spline.declared_point_count, Some(u32::from(count)));
            Ok(spline.interpolation_points.len())
        });
    }
}
