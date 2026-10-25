// SPDX-License-Identifier: Apache-2.0

use super::first_marked_reference_offsets;
use super::named_parameter_scope_tail_is_valid;
use crate::test_support::lp_utf16;

fn named_scope_tail(lane_value: u64) -> Vec<u8> {
    let label = "Canvas";
    let label_code_units = label.encode_utf16().count();
    let marker = 19 + label_code_units * 2;
    let mut bytes = vec![0; marker + 59];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut label_bytes = Vec::new();
    lp_utf16(&mut label_bytes, label);
    bytes[8..8 + label_bytes.len()].copy_from_slice(&label_bytes);
    bytes[marker] = 1;
    bytes[marker + 1] = 0xd5;
    bytes[marker + 2..marker + 10].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 12..marker + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[marker + 16..marker + 20].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 20..marker + 28].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[marker + 28..marker + 32].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 32] = 1;
    bytes[marker + 33] = 0xd4;
    bytes[marker + 34..marker + 42].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 42..marker + 46].copy_from_slice(&[0, 1, 0, 0]);
    bytes[marker + 46] = 1;
    bytes[marker + 47] = 0xd3;
    bytes[marker + 48..marker + 56].copy_from_slice(&lane_value.to_le_bytes());
    bytes
}

#[test]
fn named_scope_tail_requires_one_repeated_binary_lane_value() {
    for lane_value in [0, 1] {
        let bytes = named_scope_tail(lane_value);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(true)
        );
    }

    let mut mismatched = named_scope_tail(0);
    let marker = mismatched.len() - 59;
    mismatched[marker + 34..marker + 42].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(&mismatched, 0, mismatched.len(), mismatched.len()),
        Some(false)
    );

    let outside_domain = named_scope_tail(2);
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &outside_domain,
            0,
            outside_domain.len(),
            outside_domain.len()
        ),
        Some(false)
    );
}

#[test]
fn sketch_scope_reference_offsets_refuse_second_unique_marker() {
    let mut frame = vec![0; 24];
    for (at, suffix) in [(0, 42u32), (12, 43)] {
        frame[at] = 1;
        frame[at + 1..at + 5].copy_from_slice(&suffix.to_le_bytes());
    }
    let admitted = first_marked_reference_offsets(&frame);
    assert_eq!(admitted.get(&42), Some(&0));
    assert_eq!(admitted.get(&43), Some(&12));
}

#[test]
fn discarded_envelope_source_draft_creates_no_capture() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"physical-envelope", &arena, &DecodePolicy::service())
            .unwrap();
    let drafts = std::collections::HashMap::from([(
        "discarded-owner".to_owned(),
        super::ScopeSource {
            extent: source,
            paths: Vec::new(),
        },
    )]);
    let pointers = super::capture_scope_layouts(&ctx, &[], drafts).unwrap();
    assert!(pointers.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn canvas_scope_admits_its_complete_98_byte_tail_and_rejects_broken_links() {
    use super::parse_parameter_scope;
    use crate::design::test_support::indexed_record_offsets_for_test;
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(b"300");
    prefix.extend_from_slice(&12u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 10]);
    prefix.push(1);
    prefix.extend_from_slice(&55u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 6]);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    for member in [55u32, 56] {
        prefix.push(1);
        prefix.extend_from_slice(&member.to_le_bytes());
        prefix.extend_from_slice(&[0; 6]);
    }
    prefix.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut prefix, "Canvas");
    let mut tail = [0; 98];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[12..16].copy_from_slice(&1u32.to_le_bytes());
    for (at, index) in [(16, 12u32), (30, 3), (62, 4), (75, 5), (86, 12)] {
        tail[at] = 1;
        tail[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    tail[42..46].copy_from_slice(&u32::MAX.to_le_bytes());
    tail[46..50].copy_from_slice(&8u32.to_le_bytes());
    tail[50..58].copy_from_slice(&0.25f64.to_le_bytes());
    tail[58..62].copy_from_slice(&8u32.to_le_bytes());
    let parse = |tail: &[u8]| {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"260");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        parse_parameter_scope(
            &bytes,
            &indexed_record_offsets_for_test(&bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&tail).expect("Canvas scope with reciprocal self links");
    assert_eq!(scope.kind().as_str(), "Canvas");
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    assert!(scope.history_state_id().is_none());
    for offset in [0, 4, 12, 17, 21, 27, 35, 41, 42, 58, 67, 73, 80, 87, 91, 97] {
        let mut broken = tail;
        broken[offset] ^= 1;
        assert!(parse(&broken).is_none(), "tail offset {offset}");
    }
    for offset in [31, 63, 76] {
        let mut broken = tail;
        broken[offset..offset + 4].fill(0);
        assert!(parse(&broken).is_none());
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut broken = tail;
        broken[50..58].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&broken).is_none());
    }
}

#[test]
fn typed_canvas_scope_validates_reference_types_and_fixed_tail() {
    use crate::design::test_support::indexed_record_offsets_for_test;
    fn reference(bytes: &mut [u8], at: usize, target: u32, guid: &str) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
        bytes[at + 9..at + 13].copy_from_slice(&36u32.to_le_bytes());
        bytes[at + 13..at + 49].copy_from_slice(guid.as_bytes());
    }
    let mut bytes = vec![0; 350];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"300");
    bytes[7..11].copy_from_slice(&12u32.to_le_bytes());
    bytes[11..15].copy_from_slice(&2u32.to_le_bytes());
    reference(&mut bytes, 15, 55, "5A1BF548-241F-46FD-9FB5-E4B05126EB9D");
    reference(&mut bytes, 66, 56, "7B6C3C2D-4096-4E6D-B225-699E8C2D9355");
    bytes[117..121].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut kind = Vec::new();
    lp_utf16(&mut kind, "Canvas");
    bytes[121..137].copy_from_slice(&kind);
    let tail = &mut bytes[137..333];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    reference(tail, 19, 3, "B7F34D7B-DAC4-41A5-B67D-D672537C3BFA");
    tail[70..74].copy_from_slice(&u32::MAX.to_le_bytes());
    tail[74..78].copy_from_slice(&8u32.to_le_bytes());
    tail[78..86].copy_from_slice(&0.25f64.to_le_bytes());
    tail[86..90].copy_from_slice(&8u32.to_le_bytes());
    reference(tail, 90, 4, "5FBE538E-139B-46D4-BCAF-CC796D1E1F78");
    reference(tail, 143, 5, "6CCF41D5-40BE-48ED-A834-18F3EAED6C57");
    bytes[333..337].copy_from_slice(&3u32.to_le_bytes());
    bytes[337..340].copy_from_slice(b"260");
    bytes[340..344].copy_from_slice(&12u32.to_le_bytes());
    let parse = |bytes: &[u8]| {
        super::parse_parameter_scope(
            bytes,
            &indexed_record_offsets_for_test(bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&bytes).expect("typed Canvas scope");
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    let wire = serde_json::to_string(&scope).unwrap();
    let decoded: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
    for offset in [
        11, 20, 28, 79, 137, 141, 161, 169, 207, 223, 240, 278, 285, 331,
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(parse(&invalid).is_none(), "offset {offset}");
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut invalid = bytes.clone();
        invalid[215..223].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
}

#[test]
fn thread_bound_hole_scope_requires_reciprocal_tail_and_history() {
    use crate::design::test_support::indexed_record_offsets_for_test;
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(b"300");
    prefix.extend_from_slice(&12u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 10]);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    for member in [55u32, 56] {
        prefix.push(1);
        prefix.extend_from_slice(&member.to_le_bytes());
        prefix.extend_from_slice(&[0; 6]);
    }
    prefix.extend_from_slice(&9u32.to_le_bytes());
    lp_utf16(&mut prefix, "Hole");
    let mut tail = [0; 109];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[12..16].copy_from_slice(&2u32.to_le_bytes());
    for (at, index) in [
        (16, 12u32),
        (27, 100),
        (41, 99),
        (73, 98),
        (86, 97),
        (97, 12),
    ] {
        tail[at] = 1;
        tail[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    tail[53..57].copy_from_slice(&8u32.to_le_bytes());
    tail[57..61].copy_from_slice(&4u32.to_le_bytes());
    tail[61..69].copy_from_slice(&0.25f64.to_le_bytes());
    tail[69..73].copy_from_slice(&4u32.to_le_bytes());
    let parse = |tail: &[u8]| {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"260");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        super::parse_parameter_scope(
            &bytes,
            &indexed_record_offsets_for_test(&bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&tail).expect("Thread-bound Hole scope");
    assert_eq!(scope.kind_name(), "Hole");
    assert_eq!(scope.history_state_id(), Some(9));
    assert_eq!(scope.previous_history_state_id(), Some(8));
    let wire = serde_json::to_string(&scope).unwrap();
    let decoded: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
    for offset in [
        0, 4, 12, 17, 21, 28, 32, 38, 42, 46, 52, 69, 74, 78, 84, 87, 91, 98, 102, 108,
    ] {
        let mut invalid = tail;
        invalid[offset] ^= 1;
        assert!(parse(&invalid).is_none(), "tail offset {offset}");
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut invalid = tail;
        invalid[61..69].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
}
