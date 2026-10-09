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
