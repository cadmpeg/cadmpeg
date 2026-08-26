//! Legacy geometry-locus alternate profile-point marker tests.

use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::*;
use crate::records::SketchInputKind;

#[test]
fn legacy_geometry_locus_alternate_point_records_decode() {
    fn header(size: usize, code: u32, tag: [u8; 2], coordinates: [f64; 2]) -> Vec<u8> {
        let mut payload = vec![0; size + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&code.to_le_bytes());
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[29..31].copy_from_slice(&[1, 0]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&tag);
        payload[58..66].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[66..74].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[size..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    }

    for (code, tag) in [(0, [0x12, 0x00]), (1, [0x13, 0x00]), (2, [0x16, 0x00])] {
        let mut payload = header(134, code, tag, [1.25, -2.5]);
        payload[74..76].copy_from_slice(&8u16.to_le_bytes());
        payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
        payload[130..134].copy_from_slice(&9u32.to_le_bytes());

        assert_eq!(
            legacy_geometry_locus_alternate_profile_point_coordinates(&payload, 0),
            Some([1.25, -2.5])
        );
        assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::Point
        );
    }

    let mut profile_vertex = header(134, 1, [0x13, 0x00], [-1.5, 0.75]);
    profile_vertex[76..78].copy_from_slice(&1u16.to_le_bytes());
    profile_vertex[82..84].copy_from_slice(&1u16.to_le_bytes());
    profile_vertex[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    profile_vertex[130..134].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(marker_coordinates(&profile_vertex, 0), Some([-1.5, 0.75]));
    assert_eq!(
        sketch_input_entities(&profile_vertex, "lane")[0].kind,
        SketchInputKind::Point
    );

    let mut identity_variant = header(138, 1, [0x16, 0x00], [3.0, 4.0]);
    identity_variant[76..78].copy_from_slice(&1u16.to_le_bytes());
    identity_variant[82..84].copy_from_slice(&1u16.to_le_bytes());
    identity_variant[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    identity_variant[124..128].copy_from_slice(&5u32.to_le_bytes());
    identity_variant[130..134].copy_from_slice(&1u32.to_le_bytes());
    identity_variant[134..138].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(marker_coordinates(&identity_variant, 0), Some([3.0, 4.0]));
    assert_eq!(
        sketch_input_entities(&identity_variant, "lane")[0].kind,
        SketchInputKind::Point
    );

    let mut linked = header(154, 0, [0x12, 0x00], [5.0, 6.0]);
    linked[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (relative, selector, identifier) in [(78, 0x8148u16, 0u16), (90, 0x814cu16, 4u16)] {
        linked[relative..relative + 2].copy_from_slice(&selector.to_le_bytes());
        linked[relative + 2..relative + 4].copy_from_slice(&identifier.to_le_bytes());
        linked[relative + 4..relative + 8].fill(0xff);
        linked[relative + 8..relative + 12].fill(0);
    }
    linked[102..108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[150..154].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(marker_coordinates(&linked, 0), Some([5.0, 6.0]));
    assert_eq!(
        legacy_geometry_locus_alternate_linked_profile_point(&linked, 0),
        Some(([5.0, 6.0], [(0x8148, 0), (0x814c, 4)]))
    );
    assert_eq!(
        sketch_input_entities(&linked, "lane")[0].kind,
        SketchInputKind::Point
    );

    linked[102] = 1;
    assert_eq!(marker_coordinates(&linked, 0), None);
    assert_eq!(
        sketch_input_entities(&linked, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    let mut line_handle = header(170, 2, [0x13, 0x00], [7.0, 8.0]);
    line_handle[76..78].copy_from_slice(&2u16.to_le_bytes());
    line_handle[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    line_handle[84..96].copy_from_slice(b"sgLineHandle");
    line_handle[96..98].copy_from_slice(&3u16.to_le_bytes());
    line_handle[98..106].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    line_handle[110..114].fill(0xff);
    line_handle[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    line_handle[166..170].copy_from_slice(&12u32.to_le_bytes());
    assert_eq!(marker_coordinates(&line_handle, 0), Some([7.0, 8.0]));
    assert_eq!(
        sketch_input_entities(&line_handle, "lane")[0].kind,
        SketchInputKind::Point
    );

    let mut arc_handle = header(169, 1, [0x12, 0x00], [9.0, 10.0]);
    arc_handle[76..78].copy_from_slice(&2u16.to_le_bytes());
    arc_handle[78..82].copy_from_slice(&[0x48, 0x81, 0x00, 0x00]);
    arc_handle[82..90].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    arc_handle[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    arc_handle[96..107].copy_from_slice(b"sgArcHandle");
    arc_handle[109..117].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    arc_handle[117..123].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    arc_handle[165..169].copy_from_slice(&13u32.to_le_bytes());
    assert_eq!(marker_coordinates(&arc_handle, 0), Some([9.0, 10.0]));
    assert_eq!(
        sketch_input_entities(&arc_handle, "lane")[0].kind,
        SketchInputKind::Point
    );

    arc_handle[56..58].copy_from_slice(&[0x14, 0x00]);
    assert_eq!(legacy_declared_handle_coordinates(&arc_handle, 0), None);
    assert_eq!(marker_coordinates(&arc_handle, 0), None);
}
