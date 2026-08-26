//! Compact-legacy 96-byte profile-roster tests.

use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::*;
use crate::layout::compact_legacy_96_profile_roster_curve as legacy_96;
use crate::records::{SketchInputEntity, SketchInputKind, SketchRelationKind};

fn profile_roster_payload(endpoints: [u16; 2]) -> Vec<u8> {
    let mut payload = vec![0; legacy_96::LEN + LEGACY_SKETCH_MARKER.len()];
    payload[legacy_96::MARKER..legacy_96::HEADER].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[legacy_96::HEADER..legacy_96::SHARED_SELECTOR].fill(0xff);
    payload[legacy_96::SHARED_SELECTOR..legacy_96::NATIVE_KIND]
        .copy_from_slice(&legacy_96::SHARED_SELECTOR_VALUE.to_le_bytes());
    payload[legacy_96::NATIVE_KIND..legacy_96::NATIVE_KIND + std::mem::size_of::<u32>()]
        .copy_from_slice(&legacy_96::NATIVE_KIND_VALUE.to_le_bytes());
    payload[legacy_96::PROFILE_LOCUS..legacy_96::ROLE].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[legacy_96::ROLE..legacy_96::STATE_AT_29]
        .copy_from_slice(&legacy_96::ROLE_VALUE.to_le_bytes());
    payload[legacy_96::STATE_AT_29..legacy_96::SELECTOR]
        .copy_from_slice(&legacy_96::STATE_AT_29_VALUE.to_le_bytes());
    payload[legacy_96::SELECTOR..legacy_96::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[legacy_96::STATE_VALUE..legacy_96::ENDPOINT_FIRST]
        .copy_from_slice(&legacy_96::STATE_VALUE_VALUE.to_le_bytes());
    payload[legacy_96::ENDPOINT_FIRST..legacy_96::ENDPOINT_SECOND]
        .copy_from_slice(&endpoints[0].to_le_bytes());
    payload[legacy_96::ENDPOINT_SECOND..legacy_96::ZERO_ENDPOINT_PREFIX]
        .copy_from_slice(&endpoints[1].to_le_bytes());
    payload[legacy_96::ZERO_ENDPOINT_PREFIX..legacy_96::SIGNED_SELECTOR]
        .copy_from_slice(&legacy_96::ZERO_ENDPOINT_PREFIX_VALUE);
    payload[legacy_96::SIGNED_SELECTOR..legacy_96::ZERO_SELECTOR_TRAILER]
        .copy_from_slice(&legacy_96::SIGNED_SELECTOR_VALUE.to_le_bytes());
    payload[legacy_96::ZERO_SELECTOR_TRAILER..legacy_96::TAIL_STATE]
        .copy_from_slice(&legacy_96::ZERO_SELECTOR_TRAILER_VALUE);
    payload[legacy_96::TAIL_STATE..legacy_96::TAIL_STATE_PREFIX]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[legacy_96::TAIL_STATE_PREFIX..legacy_96::TAIL_STATE_MARKER]
        .copy_from_slice(&legacy_96::TAIL_STATE_PREFIX_VALUE.to_le_bytes());
    payload[legacy_96::TAIL_STATE_MARKER..legacy_96::ZERO_TAIL_IDENTITY]
        .copy_from_slice(&legacy_96::TAIL_STATE_MARKER_VALUE.to_le_bytes());
    payload[legacy_96::ZERO_TAIL_IDENTITY..legacy_96::ONE_TAIL_IDENTITY]
        .copy_from_slice(&legacy_96::ZERO_TAIL_IDENTITY_VALUE.to_le_bytes());
    payload[legacy_96::ONE_TAIL_IDENTITY..legacy_96::LEN]
        .copy_from_slice(&legacy_96::ONE_TAIL_IDENTITY_VALUE.to_le_bytes());
    payload[legacy_96::LEN..].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload
}

#[test]
fn compact_legacy_96_profile_roster_uses_coordinate_geometry_ordinals() {
    let entity = |id: &str, offset, coordinates_m, kind, object_index| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle, None);
    let first = entity(
        "first",
        10,
        Some([0.0, 0.0]),
        SketchInputKind::Point,
        Some(90),
    );
    let relation = entity(
        "relation",
        15,
        Some([0.5, 0.0]),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        Some(3),
    );
    let coordinate_line = entity(
        "coordinate-line",
        20,
        Some([1.0, 0.0]),
        SketchInputKind::LineOrCircle,
        Some(91),
    );
    let second = entity(
        "second",
        30,
        Some([1.0, 1.0]),
        SketchInputKind::Point,
        Some(92),
    );
    let first_arc = entity(
        "first-arc",
        40,
        Some([0.0, 1.0]),
        SketchInputKind::Arc,
        Some(3),
    );
    let third = entity(
        "third",
        50,
        Some([2.0, 1.0]),
        SketchInputKind::Point,
        Some(94),
    );
    let second_arc = entity(
        "second-arc",
        60,
        Some([2.0, 2.0]),
        SketchInputKind::Arc,
        Some(5),
    );
    let markers = [
        &curve,
        &first,
        &relation,
        &coordinate_line,
        &second,
        &first_arc,
        &third,
        &second_arc,
    ];
    let payload = profile_roster_payload([3, 5]);

    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &payload, 0
    ));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first-arc", "second-arc"]
    );

    let mut alternate_header = payload.clone();
    alternate_header[legacy_96::HEADER..legacy_96::SHARED_SELECTOR]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &alternate_header,
        0
    ));
    assert_eq!(
        roster_curve_endpoint_markers(&alternate_header, &curve, &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first-arc", "second-arc"]
    );

    let mut equal_endpoints = profile_roster_payload([3, 3]);
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&equal_endpoints, 0));
    assert!(roster_curve_endpoint_markers(&equal_endpoints, &curve, &markers).is_empty());

    equal_endpoints[legacy_96::TAIL_STATE..legacy_96::TAIL_STATE + 2]
        .copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&equal_endpoints, 0));

    let mut malformed_tail_marker = profile_roster_payload([3, 5]);
    malformed_tail_marker[legacy_96::TAIL_STATE_MARKER..legacy_96::ZERO_TAIL_IDENTITY]
        .copy_from_slice(&2u16.to_le_bytes());
    assert!(
        !compact_legacy_96_profile_roster_curve_uses_complete_roster(&malformed_tail_marker, 0)
    );

    let mut malformed_tail = profile_roster_payload([3, 5]);
    malformed_tail[legacy_96::ZERO_SELECTOR_TRAILER] = 1;
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&malformed_tail, 0));

    let mut missing_boundary = profile_roster_payload([3, 5]);
    missing_boundary[legacy_96::LEN..].fill(0);
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&missing_boundary, 0));

    let out_of_range = profile_roster_payload([3, 6]);
    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &out_of_range,
        0
    ));
    assert!(roster_curve_endpoint_markers(&out_of_range, &curve, &markers).is_empty());
}
