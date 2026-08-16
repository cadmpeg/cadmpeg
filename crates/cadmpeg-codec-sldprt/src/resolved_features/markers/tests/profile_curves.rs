//! Inline profile-curve marker tests.
#![allow(unused_imports)]

use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::*;
use crate::records::SketchInputKind;

fn compact_legacy_142_profile_curve_payload(
    tag: [u8; 2],
    auxiliary: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&(-1.0f32).to_le_bytes());
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..64].fill(0);
    payload[64..66].copy_from_slice(&tag);
    payload[66..74].copy_from_slice(&auxiliary[0].to_le_bytes());
    payload[74..82].copy_from_slice(&auxiliary[1].to_le_bytes());
    payload[82..86].copy_from_slice(&11u32.to_le_bytes());
    payload[92..96].copy_from_slice(&3u32.to_le_bytes());
    payload[96..104].copy_from_slice(&start[0].to_le_bytes());
    payload[104..112].copy_from_slice(&start[1].to_le_bytes());
    payload[112..120].copy_from_slice(&end[0].to_le_bytes());
    payload[120..128].copy_from_slice(&end[1].to_le_bytes());
    payload[138..142].copy_from_slice(&17u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload
}

#[test]
fn compact_legacy_142_profile_curve_selects_arc_or_line_from_radii() {
    let mut arc =
        compact_legacy_142_profile_curve_payload([0x1a, 0x00], [2.0, 3.0], [1.0, 3.0], [2.0, 4.0]);
    assert_eq!(
        compact_legacy_142_profile_curve_coordinates(&arc, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        inline_arc_coordinates(&arc, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(marker_coordinates(&arc, 0), Some([2.0, 3.0]));
    assert_eq!(
        sketch_input_entities(&arc, "lane")[0].kind,
        SketchInputKind::Arc
    );

    let mut separated = vec![0; 146 + LEGACY_SKETCH_MARKER.len()];
    separated[..142].copy_from_slice(&arc[..142]);
    separated[142..146].copy_from_slice(&[1, 0, 0, 0]);
    separated[146..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_legacy_142_profile_curve_coordinates(&separated, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        inline_arc_coordinates(&separated, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );

    let line = compact_legacy_142_profile_curve_payload(
        [0x12, 0x00],
        [10.0, 10.0],
        [0.0, 0.0],
        [1.0, 0.0],
    );
    assert_eq!(
        compact_legacy_142_profile_curve_coordinates(&line, 0),
        Some([[10.0, 10.0], [0.0, 0.0], [1.0, 0.0]])
    );
    assert_eq!(inline_arc_coordinates(&line, 0), None);
    assert_eq!(marker_coordinates(&line, 0), None);
    assert_eq!(
        sketch_input_entities(&line, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    arc[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(compact_legacy_142_profile_curve_coordinates(&arc, 0), None);
    arc[17..21].copy_from_slice(&2u32.to_le_bytes());
    arc[64..66].copy_from_slice(&[0x14, 0x00]);
    assert_eq!(compact_legacy_142_profile_curve_coordinates(&arc, 0), None);
}
