//! Indexed and linked profile-point marker tests.
#![allow(unused_imports)]

use super::super::super::selections::coordinate_marker_local_links;
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchRelationKind,
};
use cadmpeg_ir::math::Point3;

#[test]
fn current_indexed_line_uses_its_unique_reverse_incidence_pair() {
    let first = 84;
    let second = first + 154;
    let end = second + 154;
    let mut payload = vec![0; end + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 5, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());

    for (offset, coordinates, other) in [
        (first, [1.0f64, 2.0], 11u16),
        (second, [3.0f64, 4.0], 12u16),
    ] {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
        for (start, selector, id) in [(78, 0x8178u16, 7u16), (90, 0x8132u16, other)] {
            payload[offset + start..offset + start + 2].copy_from_slice(&selector.to_le_bytes());
            payload[offset + start + 2..offset + start + 4].copy_from_slice(&id.to_le_bytes());
            payload[offset + start + 4..offset + start + 8].fill(0xff);
        }
        payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    }
    payload[end..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, object_index| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", 0, Some(7)),
        entity("first", first as u64, Some(20)),
        entity("second", second as u64, Some(21)),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        current_reverse_incidence_endpoint_offsets(&payload, &entities[0], &markers),
        Some([first as u64, second as u64])
    );
}

#[test]
fn extended_indexed_profile_point_decodes_compact_coordinates() {
    let offset = 4;
    for size in [134, 138, 140] {
        let mut payload = vec![0; offset + size + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&5u32.to_le_bytes());
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + size..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

        assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    }
}

#[test]
fn shifted_geometry_locus_coordinates_require_the_record_trailer() {
    let offset = 4;
    let record_len = 142;
    let mut payload = vec![0; offset + record_len + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&11u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x12, 0x00]);
    payload[offset + 66..offset + 74].copy_from_slice(&0.022_224_980_75f64.to_le_bytes());
    payload[offset + 74..offset + 82].copy_from_slice(&0.0f64.to_le_bytes());
    payload[offset + 92..offset + 96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 138..offset + 142].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + record_len..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        marker_coordinates(&payload, offset),
        Some([0.022_224_980_75, 0.0])
    );
    let entities = sketch_input_entities(&payload, "lane");
    let [entity] = entities.as_slice() else {
        panic!("expected one sketch marker");
    };
    assert_eq!(entity.kind, SketchInputKind::LineOrCircle);
    assert_eq!(entity.coordinates_m, Some([0.022_224_980_75, 0.0]));
    assert_eq!(entity.object_index, Some(11));
    assert_eq!(entity.local_id, Some(7));

    payload[offset + 92] = 0;
    assert_eq!(marker_coordinates(&payload, offset), None);
}

#[test]
fn shifted_geometry_handle_children_are_points() {
    for (record_len, code, tag, sentinel, declaration_offset, declaration) in [
        (178, 2u32, [0x13, 0x00], 128, 86, b"sgLineHandle".as_slice()),
        (177, 1u32, [0x12, 0x00], 127, 98, b"sgArcHandle".as_slice()),
    ] {
        let offset = 4;
        let mut payload = vec![0; offset + record_len + LEGACY_SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&11u32.to_le_bytes());
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
        payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 64..offset + 66].copy_from_slice(&tag);
        payload[offset + 66..offset + 74].copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + 74..offset + 82].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + 84..offset + 86].copy_from_slice(&2u16.to_le_bytes());
        let declaration_prefix = if declaration == b"sgLineHandle" {
            [0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]
        } else {
            [0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]
        };
        payload[offset + declaration_offset..offset + declaration_offset + 6]
            .copy_from_slice(&declaration_prefix);
        payload
            [offset + declaration_offset + 6..offset + declaration_offset + 6 + declaration.len()]
            .copy_from_slice(declaration);
        payload[offset + sentinel..offset + sentinel + 4]
            .copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
        payload[offset + record_len..].copy_from_slice(LEGACY_SKETCH_MARKER);

        let entities = sketch_input_entities(&payload, "lane");
        let [entity] = entities.as_slice() else {
            panic!("expected one sketch marker");
        };
        assert_eq!(entity.kind, SketchInputKind::Point);
        assert_eq!(entity.coordinates_m, Some([1.25, -2.5]));
    }
}

#[test]
fn extended_linked_profile_vertex_decodes_as_a_point() {
    let offset = 4;
    let mut payload = vec![0; offset + 154 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    for (cell, link) in [(78, 1u16), (90, 2u16)] {
        payload[offset + cell..offset + cell + 2].copy_from_slice(&[0xe7, 0x81]);
        payload[offset + cell + 2..offset + cell + 4].copy_from_slice(&link.to_le_bytes());
        payload[offset + cell + 4..offset + cell + 8].fill(0xff);
    }
    payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 150..offset + 154].copy_from_slice(&9u32.to_le_bytes());
    payload[offset + 154..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert!(super::linked_profile_vertex(&payload, offset));
    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset + 102] = 1;
    assert!(!super::linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
}

#[test]
fn compact_linked_profile_vertex_decodes_legacy_and_extended_markers() {
    let offset = 4;
    let mut payload = vec![0; offset + 146 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 74..offset + 78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    for (cell, link) in [(78, 4u16), (86, 10u16)] {
        payload[offset + cell..offset + cell + 2].copy_from_slice(&[0x8b, 0x81]);
        payload[offset + cell + 2..offset + cell + 4].copy_from_slice(&link.to_le_bytes());
        payload[offset + cell + 4..offset + cell + 8].fill(0xff);
    }
    payload[offset + 94..offset + 100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 142..offset + 146].copy_from_slice(&11u32.to_le_bytes());
    payload[offset + 146..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset + 88..offset + 90].copy_from_slice(&4u16.to_le_bytes());
    assert!(!super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[offset + 78..offset + 82].copy_from_slice(&[0x06, 0x81, 0x00, 0x00]);
    payload[offset + 86..offset + 90].copy_from_slice(&[0x01, 0x83, 0x00, 0x00]);
    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
}

#[test]
fn current_indexed_profile_point_decodes_compact_coordinates() {
    let offset = 4;
    let mut payload = vec![0; offset + 134 + SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 134..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
}

#[test]
fn compact_legacy_linked_coordinate_uses_the_1a_pair() {
    let mut payload = vec![0; 154 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    payload[58..66].copy_from_slice(&0.0025f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.01f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (start, local_id) in [(78, 2u16), (90, 5u16)] {
        payload[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[102..108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[150..154].copy_from_slice(&29u32.to_le_bytes());
    payload[154..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(legacy_linked_coordinates(&payload, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&payload, 0), Some([0.0025, 0.01]));
    payload[90] ^= 1;
    assert_eq!(legacy_linked_coordinates(&payload, 0), None);
    assert_eq!(marker_coordinates(&payload, 0), None);

    let mut compact = vec![0; 146 + LEGACY_SKETCH_MARKER.len()];
    compact[..78].copy_from_slice(&payload[..78]);
    for (start, local_id) in [(78, 2u16), (86, 5u16)] {
        compact[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        compact[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        compact[start + 4..start + 8].fill(0xff);
    }
    compact[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    compact[138..142].copy_from_slice(&17u32.to_le_bytes());
    compact[142..146].copy_from_slice(&29u32.to_le_bytes());
    compact[146..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(legacy_linked_coordinates(&compact, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&compact, 0), Some([0.0025, 0.01]));

    let mut shifted = vec![0; 162 + LEGACY_SKETCH_MARKER.len()];
    shifted[..56].copy_from_slice(&payload[..56]);
    shifted[64..66].copy_from_slice(&[0x1a, 0x00]);
    shifted[66..74].copy_from_slice(&0.0025f64.to_le_bytes());
    shifted[74..82].copy_from_slice(&0.01f64.to_le_bytes());
    shifted[84..86].copy_from_slice(&2u16.to_le_bytes());
    for (start, local_id) in [(86, 2u16), (98, 5u16)] {
        shifted[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        shifted[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        shifted[start + 4..start + 8].fill(0xff);
    }
    shifted[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    shifted[158..162].copy_from_slice(&29u32.to_le_bytes());
    shifted[162..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(legacy_linked_coordinates(&shifted, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&shifted, 0), Some([0.0025, 0.01]));
    shifted[158..162].fill(0xff);
    assert_eq!(legacy_linked_coordinates(&shifted, 0), None);
}

#[test]
fn legacy_inline_arc_decodes_center_and_endpoints() {
    let mut payload = vec![0; 146 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1a, 0x00]);
    payload[66..74].copy_from_slice(&2.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&3.0f64.to_le_bytes());
    payload[92..94].copy_from_slice(&1u16.to_le_bytes());
    payload[96..104].copy_from_slice(&1.0f64.to_le_bytes());
    payload[104..112].copy_from_slice(&3.0f64.to_le_bytes());
    payload[112..120].copy_from_slice(&2.0f64.to_le_bytes());
    payload[120..128].copy_from_slice(&4.0f64.to_le_bytes());
    payload[132..136].copy_from_slice(&8u32.to_le_bytes());
    payload[142..146].copy_from_slice(&5u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&payload, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(marker_coordinates(&payload, 0), Some([2.0, 3.0]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[120..128].copy_from_slice(&5.0f64.to_le_bytes());
    assert_eq!(inline_arc_coordinates(&payload, 0), None);

    let mut corner = vec![0; 138 + LEGACY_SKETCH_MARKER.len()];
    corner[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    corner[5..13].fill(0xff);
    corner[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    corner[17..21].copy_from_slice(&2u32.to_le_bytes());
    corner[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    corner[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    corner[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    corner[56..58].copy_from_slice(&0x16u16.to_le_bytes());
    corner[58..66].copy_from_slice(&20.0f64.to_le_bytes());
    corner[66..74].copy_from_slice(&20.0f64.to_le_bytes());
    corner[74..76].copy_from_slice(&11u16.to_le_bytes());
    corner[84..88].copy_from_slice(&9u32.to_le_bytes());
    corner[88..96].copy_from_slice(&20.0f64.to_le_bytes());
    corner[96..104].copy_from_slice(&17.0f64.to_le_bytes());
    corner[104..112].copy_from_slice(&17.0f64.to_le_bytes());
    corner[112..120].copy_from_slice(&20.0f64.to_le_bytes());
    corner[124..128].copy_from_slice(&54u32.to_le_bytes());
    corner[128..132].copy_from_slice(&2u32.to_le_bytes());
    corner[134..138].copy_from_slice(&73u32.to_le_bytes());
    corner[138..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&corner, 0),
        Some([[17.0, 17.0], [20.0, 17.0], [17.0, 20.0]])
    );
    assert_eq!(marker_coordinates(&corner, 0), Some([17.0, 17.0]));
    assert_eq!(
        sketch_input_entities(&corner, "lane")[0].kind,
        SketchInputKind::Arc
    );

    let mut packed = vec![0; 126 + LEGACY_SKETCH_MARKER.len()];
    packed[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    packed[5..13].fill(0xff);
    packed[13..17].copy_from_slice(&2u32.to_le_bytes());
    packed[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    packed[29..33].copy_from_slice(&[0x04, 0x00, 0x00, 0x00]);
    packed[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    packed[48..50].copy_from_slice(&[0x16, 0x00]);
    packed[50..58].copy_from_slice(&2.0f64.to_le_bytes());
    packed[58..66].copy_from_slice(&3.0f64.to_le_bytes());
    packed[66..68].copy_from_slice(&11u16.to_le_bytes());
    packed[76..80].copy_from_slice(&6u32.to_le_bytes());
    packed[80..88].copy_from_slice(&1.0f64.to_le_bytes());
    packed[88..96].copy_from_slice(&3.0f64.to_le_bytes());
    packed[96..104].copy_from_slice(&2.0f64.to_le_bytes());
    packed[104..112].copy_from_slice(&4.0f64.to_le_bytes());
    packed[122..126].copy_from_slice(&9u32.to_le_bytes());
    packed[126..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        inline_arc_coordinates(&packed, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        sketch_input_entities(&packed, "lane")[0].kind,
        SketchInputKind::Arc
    );
    packed[48] = 0x12;
    assert_eq!(
        inline_arc_coordinates(&packed, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    packed[104..112].copy_from_slice(&5.0f64.to_le_bytes());
    assert_eq!(inline_arc_coordinates(&packed, 0), None);
}

#[test]
fn geometry_locus_inline_arcs_decode_direct_and_opposite_corner_centers() {
    for (prefix, code, tag, stored, tail, center) in [
        (
            LEGACY_EXTENDED_SKETCH_MARKER,
            2u32,
            [0x12, 0x00],
            [2.0f64, 3.0],
            [0x00, 0x00, 0x02, 0x00, 0x00, 0x00],
            [2.0f64, 3.0],
        ),
        (
            LEGACY_SKETCH_MARKER,
            1u32,
            [0x1a, 0x00],
            [2.0f64, 3.0],
            [0x01, 0x00, 0x00, 0x00, 0x00, 0x00],
            [1.0f64, 4.0],
        ),
    ] {
        let mut payload = vec![0; 138 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&code.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&tag);
        payload[58..66].copy_from_slice(&stored[0].to_le_bytes());
        payload[66..74].copy_from_slice(&stored[1].to_le_bytes());
        payload[74..76].copy_from_slice(&11u16.to_le_bytes());
        payload[84..88].copy_from_slice(&1u32.to_le_bytes());
        payload[88..96].copy_from_slice(&1.0f64.to_le_bytes());
        payload[96..104].copy_from_slice(&3.0f64.to_le_bytes());
        payload[104..112].copy_from_slice(&2.0f64.to_le_bytes());
        payload[112..120].copy_from_slice(&4.0f64.to_le_bytes());
        payload[124..128].copy_from_slice(&7u32.to_le_bytes());
        payload[128..134].copy_from_slice(&tail);
        payload[134..138].copy_from_slice(&5u32.to_le_bytes());
        payload[138..].copy_from_slice(prefix);

        assert_eq!(
            inline_arc_coordinates(&payload, 0),
            Some([center, [1.0, 3.0], [2.0, 4.0]])
        );
        assert_eq!(marker_coordinates(&payload, 0), Some(center));
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::Arc
        );

        payload[112..120].copy_from_slice(&5.0f64.to_le_bytes());
        assert_eq!(inline_arc_coordinates(&payload, 0), None);
        payload[112..120].copy_from_slice(&4.0f64.to_le_bytes());
        payload[128] ^= 1;
        assert_eq!(inline_arc_coordinates(&payload, 0), None);
    }

    let mut compact = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    compact[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact[5..13].fill(0xff);
    compact[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    compact[17..21].copy_from_slice(&2u32.to_le_bytes());
    compact[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    compact[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    compact[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    compact[56..58].copy_from_slice(&[0x1a, 0x00]);
    compact[58..66].copy_from_slice(&2.0f64.to_le_bytes());
    compact[66..74].copy_from_slice(&3.0f64.to_le_bytes());
    compact[74..76].copy_from_slice(&11u16.to_le_bytes());
    compact[88..96].copy_from_slice(&1.0f64.to_le_bytes());
    compact[96..104].copy_from_slice(&3.0f64.to_le_bytes());
    compact[104..112].copy_from_slice(&2.0f64.to_le_bytes());
    compact[112..120].copy_from_slice(&4.0f64.to_le_bytes());
    compact[130..134].copy_from_slice(&5u32.to_le_bytes());
    compact[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&compact, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        sketch_input_entities(&compact, "lane")[0].kind,
        SketchInputKind::Arc
    );
    compact[130..134].fill(0);
    assert_eq!(inline_arc_coordinates(&compact, 0), None);
}

#[test]
fn legacy_declared_handle_markers_decode_their_planar_coordinates() {
    let mut payload = vec![0; 170 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.045f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.0225f64).to_le_bytes());
    payload[74..84].copy_from_slice(&[0x00, 0x00, 0x03, 0x00, 0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106].copy_from_slice(&[0x03, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    payload[106..108].copy_from_slice(&[0x2d, 0x82]);
    payload[108..110].copy_from_slice(&4u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(marker_coordinates(&payload, 0), Some([0.045, -0.0225]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    let mut linked = payload.clone();
    linked[78..170].fill(0);
    linked[78..82].copy_from_slice(&[0x15, 0x84, 0x00, 0x00]);
    linked[82..86].fill(0xff);
    linked[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    linked[96..108].copy_from_slice(b"sgLineHandle");
    linked[110..114].fill(0xff);
    linked[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&linked, 0),
        Some([0.045, -0.0225])
    );
    linked[90] = 0;
    assert_eq!(legacy_declared_handle_coordinates(&linked, 0), None);

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[170..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[162..166].copy_from_slice(&3u32.to_le_bytes());
    payload[166..170].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[162..170].fill(0);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload.resize(177 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[96..177].fill(0);
    payload[96..108].copy_from_slice(&[
        0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x0b, 0x00,
    ]);
    payload[108..119].copy_from_slice(b"sgArcHandle");
    payload[119..121].copy_from_slice(&3u16.to_le_bytes());
    payload[121..125].fill(0xff);
    payload[125..131].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[173..177].copy_from_slice(&2u32.to_le_bytes());
    payload[177..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    let mut padded_handle = payload.clone();
    padded_handle.resize(185 + LEGACY_SKETCH_MARKER.len(), 0);
    padded_handle[96..98].copy_from_slice(&1u16.to_le_bytes());
    padded_handle[98..185].fill(0);
    padded_handle[98..106].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    padded_handle[106..112].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    padded_handle[112..123].copy_from_slice(b"sgArcHandle");
    padded_handle[125..129].fill(0xff);
    padded_handle[133..139].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    padded_handle[181..185].copy_from_slice(&5u32.to_le_bytes());
    padded_handle[185..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&padded_handle, 0),
        Some([0.045, -0.0225])
    );
    padded_handle[181..185].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&padded_handle, 0), None);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[96..98].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[119..121].fill(0xff);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    payload[84] = b'x';
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
}

#[test]
fn legacy_arc_handle_marker_decodes_its_planar_coordinate() {
    let mut payload = vec![0; 169 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.352f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.005f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[78..82].copy_from_slice(&[0x5e, 0x82, 0x03, 0x00]);
    payload[82..86].fill(0xff);
    payload[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    payload[96..107].copy_from_slice(b"sgArcHandle");
    payload[109..113].fill(0xff);
    payload[117..123].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[165..169].copy_from_slice(&7u32.to_le_bytes());
    payload[169..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].fill(0);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[169..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[169..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[80..82].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[80..82].copy_from_slice(&3u16.to_le_bytes());
    payload[165..169].fill(0xff);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
}

#[test]
fn linked_profile_point_146_decodes_prefix_specific_coordinate_tags() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.8f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0125f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[78..82].copy_from_slice(&[0x16, 0x81, 0x06, 0x00]);
    payload[82..86].fill(0xff);
    payload[86..90].copy_from_slice(&[0x16, 0x81, 0x07, 0x00]);
    payload[90..94].fill(0xff);
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[86..88].copy_from_slice(&0x8121u16.to_le_bytes());
    payload[88..90].copy_from_slice(&6u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[86..88].copy_from_slice(&0x8116u16.to_le_bytes());
    payload[88..90].copy_from_slice(&7u16.to_le_bytes());
    payload[100..146].fill(0);
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    payload[142..146].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[136..140].fill(0);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[134..136].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[134..136].fill(0);
    let mut continuation = vec![0; 150 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    continuation[..146].copy_from_slice(&payload[..146]);
    continuation[136..140].copy_from_slice(&6u32.to_le_bytes());
    continuation[140..142].fill(0);
    continuation[142..146].copy_from_slice(&1u32.to_le_bytes());
    continuation[146..150].copy_from_slice(&1u32.to_le_bytes());
    continuation[150..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&continuation, "lane")[0].kind,
        SketchInputKind::Point
    );
    continuation[142..146].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    continuation[142..146].copy_from_slice(&1u32.to_le_bytes());
    continuation[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    continuation[76..78].copy_from_slice(&2u16.to_le_bytes());
    continuation[136..140].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[80..82].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[88..90].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[88..90].copy_from_slice(&7u16.to_le_bytes());
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].fill(0);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[141] = 1;
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[141] = 0;
    payload[80..82].copy_from_slice(&6u16.to_le_bytes());
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[138..142].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );

    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );

    payload[88..90].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[80..82].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn extended_four_link_profile_point_decodes_coordinates() {
    let make_payload = |trailer_offset: usize| {
        let mut payload = vec![0; trailer_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&0u32.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&[0x1e, 0x00]);
        payload[58..66].copy_from_slice(&0.125f64.to_le_bytes());
        payload[66..74].copy_from_slice(&(-0.25f64).to_le_bytes());
        payload[74..78].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
        payload[78..80].copy_from_slice(&0x8116u16.to_le_bytes());
        payload[80..82].copy_from_slice(&3u16.to_le_bytes());
        payload[82..86].fill(0xff);
        payload[86..88].copy_from_slice(&0x8116u16.to_le_bytes());
        payload[88..90].copy_from_slice(&1u16.to_le_bytes());
        payload[90..94].fill(0xff);
        payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
        payload[134..136].copy_from_slice(&2u16.to_le_bytes());
        payload[trailer_offset..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload
    };

    for trailer_offset in [146, 150, 162, 174] {
        let payload = make_payload(trailer_offset);
        assert_eq!(
            extended_four_link_profile_point_coordinates(&payload, 0),
            Some([0.125, -0.25])
        );
    }

    let mut payload = make_payload(150);

    assert_eq!(
        extended_four_link_profile_point_coordinates(&payload, 0),
        Some([0.125, -0.25])
    );
    assert_eq!(marker_coordinates(&payload, 0), Some([0.125, -0.25]));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([0.125, -0.25]));

    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_four_link_profile_point_coordinates(&payload, 0),
        None
    );
    payload[76..78].copy_from_slice(&4u16.to_le_bytes());
    payload[134..136].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_four_link_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn compact_legacy_linked_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 132 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.004f64.to_le_bytes());
    payload[52..60].copy_from_slice(&0.006f64.to_le_bytes());
    payload[62..64].copy_from_slice(&2u16.to_le_bytes());
    for (relative, id) in [(64, 0u16), (72, 3u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x811au16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[80..86].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[128..132].copy_from_slice(&7u32.to_le_bytes());
    payload[132..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.004, 0.006])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[42] = 0x1a;
    payload[62..64].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.004, 0.006])
    );
    payload[74..76].fill(0);
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].copy_from_slice(&3u16.to_le_bytes());
    payload[128..132].fill(0xff);
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn compact_legacy_code_two_profile_point_and_embedded_geometry_have_distinct_layouts() {
    let mut point = vec![0; 132 + LEGACY_SKETCH_MARKER.len()];
    point[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    point[5..13].fill(0xff);
    point[13..17].copy_from_slice(&2u32.to_le_bytes());
    point[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    point[31..42].copy_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    point[42..44].copy_from_slice(&[0x1e, 0x00]);
    point[44..52].copy_from_slice(&0.03f64.to_le_bytes());
    point[52..60].copy_from_slice(&0.005f64.to_le_bytes());
    point[62..64].copy_from_slice(&4u16.to_le_bytes());
    for (relative, id) in [(64, 8u16), (72, 11u16)] {
        point[relative..relative + 2].copy_from_slice(&0x811au16.to_le_bytes());
        point[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        point[relative + 4..relative + 8].fill(0xff);
    }
    point[80..86].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    point[120..124].copy_from_slice(&2u32.to_le_bytes());
    point[128..132].copy_from_slice(&10u32.to_le_bytes());
    point[132..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_code_two_profile_point_coordinates(&point, 0),
        Some([0.03, 0.005])
    );
    let entity = &sketch_input_entities(&point, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([0.03, 0.005]));
    assert_eq!(entity.local_id, Some(10));
    assert_eq!(entity.state_value, None);

    let mut embedded = vec![0; 120 + LEGACY_SKETCH_MARKER.len()];
    embedded[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    embedded[5..13].fill(0xff);
    embedded[19..25].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    embedded[31..42].copy_from_slice(&[0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    embedded[42..44].copy_from_slice(&[0x1e, 0x00]);
    embedded[44..52].copy_from_slice(&0.03f64.to_le_bytes());
    embedded[52..60].copy_from_slice(&0.005f64.to_le_bytes());
    embedded[70..74].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    embedded[116..120].copy_from_slice(&12u32.to_le_bytes());
    embedded[120..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_embedded_geometry_coordinates(&embedded, 0),
        Some([0.03, 0.005])
    );
    embedded[60..64].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        compact_legacy_embedded_geometry_coordinates(&embedded, 0),
        Some([0.03, 0.005])
    );
    assert!(sketch_input_entities(&embedded, "lane").is_empty());
}

#[test]
fn legacy_single_incidence_profile_point_decodes_both_identity_trailers() {
    let mut payload = vec![0; 140 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.052f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.01f64).to_le_bytes());
    payload[76..78].copy_from_slice(&1u16.to_le_bytes());
    payload[78..82].copy_from_slice(&[0x29, 0x81, 0x0e, 0x00]);
    payload[82..86].fill(0xff);
    payload[90..96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00]);
    payload[136..140].copy_from_slice(&24u32.to_le_bytes());
    payload[140..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].fill(0);
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..140].fill(0);
    payload[128..132].copy_from_slice(&1u32.to_le_bytes());
    payload[136..140].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn legacy_140_profile_point_variant_decodes_link_state_and_shifted_trailers() {
    let mut payload = vec![0; 140 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.125f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.25f64).to_le_bytes());
    payload[78..80].copy_from_slice(&0x829eu16.to_le_bytes());
    payload[82..86].fill(0xff);
    payload[90..96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00]);
    payload[136..140].copy_from_slice(&3u32.to_le_bytes());
    payload[140..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[76..78].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_140_profile_point_variant_coordinates(&payload, 0),
        Some([0.125, -0.25])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[80..82].copy_from_slice(&2u16.to_le_bytes());
    payload[96..140].fill(0);
    payload[132..136].copy_from_slice(&25u32.to_le_bytes());
    payload[136..140].copy_from_slice(&28u32.to_le_bytes());
    payload[140..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_140_profile_point_variant_coordinates(&payload, 0),
        Some([0.125, -0.25])
    );

    payload[136..140].copy_from_slice(&25u32.to_le_bytes());
    assert_eq!(
        legacy_140_profile_point_variant_coordinates(&payload, 0),
        None
    );
}

#[test]
fn extended_scaled_incidence_profile_point_decodes_coordinates() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.052f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.01f64).to_le_bytes());
    payload[76..78].copy_from_slice(&4u16.to_le_bytes());
    payload[78..82].copy_from_slice(&[0x16, 0x87, 0x03, 0x00]);
    payload[82..86].fill(0xff);
    payload[86..90].copy_from_slice(&[0x10, 0x87, 0x02, 0x00]);
    payload[90..94].fill(0xff);
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[134..136].copy_from_slice(&2u16.to_le_bytes());
    payload[142..146].copy_from_slice(&24u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([0.052, -0.01]));

    payload[76..78].copy_from_slice(&8u16.to_le_bytes());
    payload[134..136].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );

    payload[134..136].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn packed_legacy_linked_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 138 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[27..31].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x1a, 0x00]);
    payload[50..58].copy_from_slice(&0.0021f64.to_le_bytes());
    payload[58..66].copy_from_slice(&0.0f64.to_le_bytes());
    payload[68..70].copy_from_slice(&2u16.to_le_bytes());
    for (relative, id) in [(70, 2u16), (78, 5u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x8181u16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[86..92].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[134..138].copy_from_slice(&29u32.to_le_bytes());
    payload[138..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.0021, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[80..82].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[80..82].copy_from_slice(&5u16.to_le_bytes());
    payload[72..74].fill(0);
    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn extended_profile_point_forms_decode_as_points() {
    let common = |length: usize| {
        let mut payload = vec![0; length + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&2u32.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&[0x1e, 0x00]);
        payload[58..66].copy_from_slice(&0.435f64.to_le_bytes());
        payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
        payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
        payload[length..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload
    };
    let mut declaration = common(170);
    declaration[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    declaration[84..96].copy_from_slice(b"sgLineHandle");
    declaration[96..106].copy_from_slice(&[0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    declaration[106..110].copy_from_slice(&[0x56, 0x81, 0x07, 0x00]);
    declaration[110..114].fill(0xff);
    declaration[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    declaration[76..78].copy_from_slice(&3u16.to_le_bytes());
    declaration[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    declaration[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(extended_profile_point_coordinates(&declaration, 0), None);

    let mut compact_declaration = common(162);
    compact_declaration[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    compact_declaration[84..96].copy_from_slice(b"sgLineHandle");
    compact_declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    compact_declaration[98..102].fill(0xff);
    compact_declaration[102..104].copy_from_slice(&0x8156u16.to_le_bytes());
    compact_declaration[104..106].copy_from_slice(&2u16.to_le_bytes());
    compact_declaration[106..110].fill(0xff);
    compact_declaration[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    compact_declaration[158..162].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&compact_declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    compact_declaration[96..98].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    compact_declaration[154..158].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[154..158].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[154..158].fill(0);
    compact_declaration[74..78].copy_from_slice(&[0x00, 0x00, 0x03, 0x00]);
    compact_declaration[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[102..104].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[102..104].copy_from_slice(&0x8156u16.to_le_bytes());
    compact_declaration[104..106].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[104..106].copy_from_slice(&2u16.to_le_bytes());
    compact_declaration[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    compact_declaration[96..98].fill(0);
    compact_declaration[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    compact_declaration[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact_declaration[17..21].copy_from_slice(&1u32.to_le_bytes());
    compact_declaration[96..98].copy_from_slice(&12u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&compact_declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    compact_declaration[96..98].copy_from_slice(&11u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );

    let mut linked = common(154);
    for (relative, id) in [(78, 1u16), (90, 2u16)] {
        linked[relative..relative + 2].copy_from_slice(&[0x56, 0x81]);
        linked[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        linked[relative + 4..relative + 8].fill(0xff);
    }
    linked[102..108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[150..154].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    linked[17..21].copy_from_slice(&0u32.to_le_bytes());
    linked[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&linked, "lane")[0].kind,
        SketchInputKind::Point
    );
    linked[92..94].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    linked[92..94].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(extended_profile_point_coordinates(&linked, 0), None);
    linked[92..94].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
}

#[test]
fn terminal_extended_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 180];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.19f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.0f64.to_le_bytes());
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[174..176].copy_from_slice(&6u16.to_le_bytes());
    payload[176..178].copy_from_slice(&0x81a3u16.to_le_bytes());

    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        Some([-0.19, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[174..176].fill(0);
    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        None
    );
    payload[174..176].copy_from_slice(&6u16.to_le_bytes());
    payload[176..178].fill(0);
    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        None
    );
}

#[test]
fn current_geometry_locus_profile_vertex_decodes_as_a_point() {
    let mut payload = vec![0; 146 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[74..82].copy_from_slice(&0.542f64.to_le_bytes());
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..98].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00]);
    payload[132..136].copy_from_slice(&7u32.to_le_bytes());
    payload[142..146].fill(0xff);
    payload[146..].copy_from_slice(SKETCH_MARKER);

    assert!(current_geometry_locus_profile_vertex(&payload, 0));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[132..136].fill(0);
    assert!(!current_geometry_locus_profile_vertex(&payload, 0));
}

#[test]
fn extended_geometry_locus_single_link_record_decodes_as_a_point() {
    let mut payload = vec![0; 138 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.0f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.019f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..86].copy_from_slice(&(-1i32).to_le_bytes());
    payload[124..128].copy_from_slice(&1u32.to_le_bytes());
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[132..138].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[138..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert!(extended_geometry_locus_single_link_point(&payload, 0));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([0.0, 0.019]));

    payload[128..132].copy_from_slice(&1u32.to_le_bytes());
    assert!(!extended_geometry_locus_single_link_point(&payload, 0));
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert!(!extended_geometry_locus_single_link_point(&payload, 0));
}

#[test]
fn current_compact_geometry_locus_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 134 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.542f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&7u32.to_le_bytes());
    payload[134..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_geometry_locus_point_coordinates(&payload, 0),
        Some([-1.125, 0.542])
    );
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[82..84].fill(0);
    assert_eq!(compact_geometry_locus_point_coordinates(&payload, 0), None);
}

#[test]
fn legacy_compact_geometry_locus_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 134 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.542f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&7u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_geometry_locus_point_coordinates(&payload, 0),
        Some([-1.125, 0.542])
    );
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[130..134].fill(0);
    assert_eq!(compact_geometry_locus_point_coordinates(&payload, 0), None);
}

#[test]
fn legacy_geometry_locus_value_two_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 134 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.542f64.to_le_bytes());
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[126..130].copy_from_slice(&6u32.to_le_bytes());
    payload[130..134].copy_from_slice(&2u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(marker_coordinates(&payload, 0), Some([-1.125, 0.542]));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[74..78].fill(0);
    assert!(!geometry_locus_profile_vertex(&payload, 0));
}

#[test]
fn geometry_locus_profile_vertex_decodes_compact_marker_bands() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.04f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.0045f64.to_le_bytes());
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[130..134].fill(0xff);
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert!(geometry_locus_profile_vertex(&payload, 0));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-0.04, 0.0045]));
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[134..].copy_from_slice(SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[37] = 0x05;
    assert!(geometry_locus_profile_vertex(&payload, 0));
    payload[37] = 0x06;
    assert!(!geometry_locus_profile_vertex(&payload, 0));
    payload[37] = 0x04;
    payload[74..78].fill(0);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    payload[74..78].copy_from_slice(&2u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[130..134].copy_from_slice(&13u32.to_le_bytes());
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[130..134].fill(0);
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[74..134].fill(0);
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[126..130].copy_from_slice(&3u32.to_le_bytes());
    payload[130..134].copy_from_slice(&10u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[130..134].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload.resize(138 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[74..138].fill(0);
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[124..128].copy_from_slice(&2u32.to_le_bytes());
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[132..138].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[138..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[128..132].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[17..21].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));
}

#[test]
fn indexed_profile_framing_distinguishes_vertices_lines_and_arcs() {
    let mut vertex = vec![0; 74];
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[5..13].fill(0xff);
    vertex[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    vertex[17..21].copy_from_slice(&1u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..29].copy_from_slice(&1u16.to_le_bytes());
    vertex[56..58].copy_from_slice(&[0x1e, 0x00]);
    vertex[58..66].copy_from_slice(&0.025f64.to_le_bytes());
    vertex[66..74].copy_from_slice(&0.01f64.to_le_bytes());
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex.resize(112 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[17..21].copy_from_slice(&4u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..31].copy_from_slice(&[1, 0, 1, 0]);
    vertex[64..66].copy_from_slice(&[0x1e, 0x00]);
    vertex[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    vertex[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    vertex[68..72].copy_from_slice(&1u32.to_le_bytes());
    vertex[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    vertex[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    vertex[84..86].copy_from_slice(&1u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        vertex[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    vertex[112..112 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(marker_coordinates(&vertex, 0), None);

    let mut curve = vec![0; 84 + 39];
    curve[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[5..13].fill(0xff);
    curve[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[27..29].copy_from_slice(&1u16.to_le_bytes());
    curve[60..64].copy_from_slice(&1u32.to_le_bytes());
    curve[84..84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[89..97].fill(0xff);
    curve[97..101].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[89..93].copy_from_slice(&[0xff, 0xff, 0x04, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::Arc)
    );
}

#[test]
fn selector44_terminal_indexed_curve_is_a_line() {
    let mut curve = vec![0; 170];
    curve[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[5..13].fill(0xff);
    curve[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    curve[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    curve[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    curve[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    curve[56..58].copy_from_slice(&0u16.to_le_bytes());
    curve[58..60].copy_from_slice(&1u16.to_le_bytes());
    curve[60..64].copy_from_slice(&1u32.to_le_bytes());
    curve[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    curve[142..144].copy_from_slice(&[0x08, 0x80]);
    curve[154..170].copy_from_slice(&[
        0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00,
    ]);

    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[37] = 0x04;
    assert_eq!(legacy_extended_profile_curve_kind(&curve, 0), None);
}

#[test]
fn geometry_locus_role_excludes_display_handles() {
    let mut payload = vec![0; 27];
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
}

#[test]
fn coordinate_marker_links_are_sentinel_terminated_reference_cells() {
    let mut payload = vec![0; 118];
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[84..86].copy_from_slice(&3u16.to_le_bytes());
    for (index, local_id) in [7u16, 11].into_iter().enumerate() {
        let start = 86 + index * 12;
        payload[start..start + 2].copy_from_slice(&0x8386u16.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0x8386))
    );
    for start in [86, 98] {
        payload[start..start + 2].copy_from_slice(&0xbc87u16.to_le_bytes());
    }
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0xbc87))
    );
    payload[98] ^= 1;
    assert_eq!(coordinate_marker_local_links(&payload, 0), None);
}
