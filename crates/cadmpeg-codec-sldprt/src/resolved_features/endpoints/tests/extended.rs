//! Extended and wide profile-curve endpoint tests.

use super::super::super::markers::{marker_coordinates, sketch_input_entities};
use super::super::super::typed_relations::{
    current_undetailed_bounded_curve_is_line, extended_direct_object_line_endpoints,
    marker_curve_endpoint_markers,
};
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchRelationKind};
use std::collections::HashMap;

#[test]
fn linked_profile_curve_uses_its_two_typed_endpoint_cells() {
    let offset = 4;
    let mut payload = vec![0; offset + 146 + SKETCH_MARKER.len()];
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 3u16)] {
        payload[offset + relative..offset + relative + 2].copy_from_slice(&0x8137u16.to_le_bytes());
        payload[offset + relative + 2..offset + relative + 4]
            .copy_from_slice(&endpoint.to_le_bytes());
        payload[offset + relative + 4..offset + relative + 8].fill(0xff);
    }
    payload[offset + 94..offset + 100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 142..offset + 146].copy_from_slice(&5u32.to_le_bytes());
    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[offset..offset + prefix.len()].copy_from_slice(prefix);
        payload[offset + 146..offset + 146 + prefix.len()].copy_from_slice(prefix);
        assert_eq!(
            super::linked_profile_curve_endpoint_indices(&payload, offset),
            Some([2, 3])
        );
    }
}

#[test]
fn extended_linked_line_uses_inline_self_endpoint() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 5u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x810cu16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&endpoint.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[142..146].fill(0xff);
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let mut external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.0, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };
    let mut curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    external.object_index = Some(1);
    curve.object_index = Some(4);
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[140] = 1;
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}

#[test]
fn extended_identity_line_uses_inline_and_identified_point_endpoints() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&5u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let point = SketchInputEntity {
        id: "point".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 200,
        object_index: Some(5),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.01, 0.012]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: Some([0.007, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    let chained_curve = SketchInputEntity {
        id: "chained-curve".into(),
        kind: SketchInputKind::Arc,
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&chained_curve, &curve],),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[74..84].copy_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let direct_curve = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[126..130].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        None
    );
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let duplicate = SketchInputEntity {
        id: "duplicate".into(),
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &duplicate, &curve],),
        None
    );
    payload[130..134].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        None
    );
}

#[test]
fn extended_declared_line_uses_its_typed_point_selector() {
    let mut payload = vec![0; 170 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.0165f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.029f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106].copy_from_slice(&[0x08, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    payload[106..108].copy_from_slice(&0x8155u16.to_le_bytes());
    payload[108..110].copy_from_slice(&7u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    payload[170..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 7,
        offset: 0,
        object_index: Some(7),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.014, 0.016]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 3,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].fill(0);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].fill(0xff);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].copy_from_slice(&8u16.to_le_bytes());
    payload[110] = 0;
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}

#[test]
fn compact_indexed_curve_stores_endpoints_in_both_generations() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[80..84].copy_from_slice(&19u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[84..84 + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert!(!marker_is_selected_construction_line(&payload, 0));
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x45, 0x00]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert!(current_undetailed_bounded_curve_is_line(&payload, 0));
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[60..64].fill(0);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[56..58].copy_from_slice(&30u16.to_le_bytes());
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 32])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn direct_indexed_curve_stores_feature_local_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        direct_indexed_curve_endpoint_indices(&payload, 0),
        Some([6, 15])
    );
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn extended_direct_object_line_uses_exact_point_identities() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..84].copy_from_slice(&3u64.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].fill(0);
    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[37] = 0x04;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;

    let entity = |id: &str, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        kind: SketchInputKind::LineOrCircle,
        ..entity("curve", Some(2), None)
    };
    let implicit = entity("implicit", None, Some([1.0, 2.0]));
    let explicit = entity("explicit", Some(4), Some([3.0, 4.0]));
    let markers = [&curve, &implicit, &explicit];
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &curve, &markers)
            .map(|endpoints| endpoints.map(|endpoint| endpoint.id.as_str())),
        Some(["implicit", "explicit"])
    );
    let arc = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &arc, &markers),
        None
    );
    let wrong_first = entity("wrong-first", Some(5), Some([5.0, 6.0]));
    let wrong_second = entity("wrong-second", Some(6), Some([7.0, 8.0]));
    let mut linked_curve = curve.clone();
    linked_curve.links = vec![
        SketchInputLink {
            local_id: 5,
            entity_ref: wrong_first.id.clone(),
        },
        SketchInputLink {
            local_id: 6,
            entity_ref: wrong_second.id.clone(),
        },
    ];
    let markers = [
        &linked_curve,
        &implicit,
        &explicit,
        &wrong_first,
        &wrong_second,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_curve_endpoint_markers(&payload, &linked_curve, &markers_by_id, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["implicit", "explicit"]
    );

    payload[58..60].fill(0);
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[37] = 0x0c;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;
    payload[74] = 2;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
}

#[test]
fn legacy_state_five_identity_curve_uses_coordinate_roster_indices() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&11u32.to_le_bytes());
    payload[80..84].copy_from_slice(&25u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(56)
    );

    payload[80..84].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
    payload[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
}

#[test]
fn extended_tagged_indexed_curve_uses_direct_point_ids() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[76..78].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );

    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    payload.resize(370, 0);
    payload[94..150].fill(0);
    payload[150..152].copy_from_slice(&[0x08, 0x80]);
    payload[152..162].fill(0);
    payload[162..166].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    for (relative, count) in [(166, 65u32), (170, 57), (174, 33), (178, 13)] {
        payload[relative..relative + 4].copy_from_slice(&count.to_le_bytes());
    }
    for relative in (182..230).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[230..258].copy_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x80,
        0xbf, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    ]);
    payload[258..282].fill(0);
    payload[282..286].copy_from_slice(&49u32.to_le_bytes());
    payload[286..338].fill(0);
    payload[338..342].copy_from_slice(&3u32.to_le_bytes());
    payload[342..346].copy_from_slice(&1u32.to_le_bytes());
    payload[346..353].fill(0);
    payload[353..357].copy_from_slice(&0x0001_86a5u32.to_le_bytes());
    payload[357..359].copy_from_slice(&5u16.to_le_bytes());
    payload[359..363].copy_from_slice(CLASS_MARKER);
    payload[363..365].copy_from_slice(&5u16.to_le_bytes());
    payload[365..370].copy_from_slice(b"class");
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    payload[338..342].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );
}

#[test]
fn extended_compact_curve_resolves_zero_based_point_object_ids() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&16u16.to_le_bytes());
    payload[58..60].copy_from_slice(&0u16.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", Some(8), None, SketchInputKind::LineOrCircle),
        entity(
            "explicit",
            Some(16),
            Some([0.0, 0.006]),
            SketchInputKind::Point,
        ),
        entity(
            "implicit-zero",
            None,
            Some([0.0, 0.0]),
            SketchInputKind::Point,
        ),
        entity(
            "explicit-fourteen",
            Some(14),
            Some([0.022, 0.0075]),
            SketchInputKind::Point,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    let duplicate = entity(
        "duplicate-zero",
        None,
        Some([1.0, 0.0]),
        SketchInputKind::Point,
    );
    let ambiguous = [&entities[0], &entities[1], &entities[2], &duplicate];
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &ambiguous).is_empty());

    payload.resize(96 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[82..84].fill(0);
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &markers).is_empty());

    payload.resize(102, 0);
    payload[56..58].copy_from_slice(&14u16.to_le_bytes());
    payload[58..60].copy_from_slice(&16u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..102].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    let mut roster_indexed = entities.clone();
    roster_indexed[1].object_index = None;
    roster_indexed[3].object_index = None;
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    let markers = roster_indexed.iter().collect::<Vec<_>>();
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "explicit-fourteen"]
    );

    payload.resize(116, 0);
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].fill(0);
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..116].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
}

#[test]
fn extended_geometry_locus_terminal_curve_resolves_point_object_ids() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
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
    let entities = [
        entity("curve", 0, Some(8), SketchInputKind::LineOrCircle, None),
        entity(
            "first",
            100,
            Some(7),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        entity(
            "second",
            200,
            Some(10),
            SketchInputKind::Point,
            Some([1.0, 0.0]),
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&[0; 2]);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&2u16.to_le_bytes());
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &markers).is_empty());
}

#[test]
fn wide_profile_curves_index_the_coordinate_roster() {
    let curve_offset = 402;
    let mut payload = vec![0; curve_offset + 92 + LEGACY_SKETCH_MARKER.len()];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
    ] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 27..curve_offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 92..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let mut entities = sketch_input_entities(&payload, "lane");
    entities.truncate(4);
    for entity in &mut entities {
        entity.feature_ref = Some("sketch".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&4u32.to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&7u32.to_le_bytes());
    assert!(current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].copy_from_slice(&1u16.to_le_bytes());
    assert!(current_direct_92_profile_line_endpoint_indices(&payload, curve_offset).is_some());
    assert!(!current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].fill(0);
    payload[curve_offset + 84..curve_offset + 92].fill(0);
    let mut centered_entities = entities.clone();
    centered_entities[0].coordinates_m = Some([0.0, 0.0]);
    centered_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    centered_entities[1].coordinates_m = Some([1.0, 0.0]);
    centered_entities[2].coordinates_m = Some([0.0, 1.0]);
    let centered_markers = centered_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &centered_entities[3],
            &centered_markers,
            [&centered_entities[1], &centered_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    let mut hybrid_entities = centered_entities.clone();
    let mut additional_endpoint = hybrid_entities[2].clone();
    additional_endpoint.id.push_str(":additional");
    additional_endpoint.offset += 1;
    additional_endpoint.coordinates_m = Some([-1.0, 0.0]);
    hybrid_entities.insert(3, additional_endpoint);
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        None
    );
    hybrid_entities[0].coordinates_m = Some([4.0, 4.0]);
    hybrid_entities[1].coordinates_m = Some([0.0, 0.0]);
    hybrid_entities[1].object_index = Some(0);
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);

    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    assert!(legacy_undetailed_profile_line(&payload, curve_offset));

    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([2, 3])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );

    payload.resize(curve_offset + 104 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[curve_offset + 84..].fill(0);
    payload[curve_offset + 60..curve_offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&1i32.to_le_bytes());
    for at in (curve_offset + 78..curve_offset + 94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[curve_offset + 104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let mut complete_roster_entities = entities.clone();
    complete_roster_entities[0].coordinates_m = None;
    complete_roster_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let complete_roster_markers = complete_roster_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        roster_curve_endpoint_markers(
            &payload,
            &complete_roster_entities[3],
            &complete_roster_markers,
        )
        .iter()
        .map(|marker| marker.coordinates_m)
        .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 56..curve_offset + 58].fill(0);
    assert!(roster_curve_endpoint_markers(
        &payload,
        &complete_roster_entities[3],
        &complete_roster_markers,
    )
    .is_empty());
}

#[test]
fn extended_terminal_wide_profile_curve_uses_coordinate_roster() {
    let curve_offset = 536;
    let mut payload = vec![0; curve_offset + 148];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
        (402, [7.0_f64, 8.0]),
    ] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 128..curve_offset + 130].copy_from_slice(&[0x0a, 0x00]);
    payload[curve_offset + 130..curve_offset + 134].copy_from_slice(CLASS_MARKER);
    payload[curve_offset + 134..curve_offset + 136].copy_from_slice(&12u16.to_le_bytes());
    payload[curve_offset + 136..curve_offset + 148].copy_from_slice(b"sgPntPntDist");

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        point("first", 0, Some([1.0, 2.0])),
        point("second", 134, Some([3.0, 4.0])),
        point("third", 268, Some([5.0, 6.0])),
        point("fourth", 402, Some([7.0, 8.0])),
        curve,
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([4, 2])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );
}

#[test]
fn extended_wide_104_profile_curve_uses_coordinate_roster() {
    let curve_offset = 536;
    let mut payload = vec![0; curve_offset + 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
        (402, [7.0_f64, 8.0]),
    ] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[curve_offset + 92..curve_offset + 96].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        point("first", 0, Some([1.0, 2.0])),
        point("second", 134, Some([3.0, 4.0])),
        point("third", 268, Some([5.0, 6.0])),
        point("fourth", 402, Some([7.0, 8.0])),
        curve,
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([4, 2])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[4], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "second"]
    );

    payload[curve_offset + 92..curve_offset + 96].fill(0);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        None
    );
}

#[test]
fn extended_terminal_164_wide_profile_curve_uses_coordinate_roster() {
    let curve_offset = 8 * 134;
    let mut payload = vec![0; curve_offset + 164];
    for (index, offset) in (0..8).map(|index| (index, index * 134)) {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&(f64::from(index as u32)).to_le_bytes());
        payload[offset + 66..offset + 74]
            .copy_from_slice(&(f64::from((index + 1) as u32)).to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 29]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&5u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&7u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 134..curve_offset + 136].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 144..curve_offset + 148].copy_from_slice(&u32::MAX.to_le_bytes());

    let point = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: curve_offset as u64,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let mut entities = (0..8)
        .map(|index| {
            point(
                &format!("point{index}"),
                index * 134,
                Some([f64::from(index as u32), f64::from((index + 1) as u32)]),
            )
        })
        .collect::<Vec<_>>();
    entities.push(curve);
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([6, 8])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[8], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["point5", "point7"]
    );
    assert!(current_undetailed_bounded_curve_is_line(
        &payload,
        curve_offset
    ));

    payload[curve_offset + 134..curve_offset + 136].fill(0);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, curve_offset),
        None
    );
}
