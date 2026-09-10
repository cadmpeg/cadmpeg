//! Tests for the `typed_relations` module.

use super::super::markers::sketch_input_entities;
use super::super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

#[test]
fn compact_legacy_coordinate_line_ends_at_the_following_marker_coordinate() {
    let mut payload = vec![0; 268 + LEGACY_SKETCH_MARKER.len()];
    for (offset, code, coordinate) in [(0, 1u32, [1.25_f64, -2.5]), (134, 0, [3.0_f64, 4.0])] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[268..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let mut entities = sketch_input_entities(&payload, "lane");
    entities.truncate(2);
    for entity in &mut entities {
        entity.feature_ref = Some("sketch".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        consecutive_legacy_profile_line_endpoints(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.25, -2.5]), Some([3.0, 4.0])]
    );
    assert!(consecutive_legacy_profile_line_endpoints(&payload, &entities[1], &markers).is_empty());
}

#[test]
fn coordinate_lines_use_their_centered_endpoint_pairs() {
    let mut payload = vec![0; 147];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&2.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&3.0f64.to_le_bytes());
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
    payload[142..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = crate::records::SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            offset,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = Some(coordinates_m);
        constructed_marker.links = None;
        constructed_marker
    };
    let line = entity("line", 0, [2.0, 3.0]);
    let first = entity("first", 143, [1.0, 2.0]);
    let second = entity("second", 144, [3.0, 4.0]);
    let markers = [&line, &first, &second];
    assert_eq!(
        coordinate_centered_line_endpoints(&payload, &line, &markers),
        Some([&first, &second])
    );

    let mut extended = vec![0; 139];
    extended[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended[5..13].fill(0xff);
    extended[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    extended[17..21].copy_from_slice(&2u32.to_le_bytes());
    extended[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    extended[27..29].copy_from_slice(&1u16.to_le_bytes());
    extended[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    extended[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    extended[56..58].copy_from_slice(&[0x1e, 0x00]);
    extended[58..66].copy_from_slice(&2.0f64.to_le_bytes());
    extended[66..74].copy_from_slice(&3.0f64.to_le_bytes());
    extended[76..78].copy_from_slice(&1u16.to_le_bytes());
    extended[82..84].copy_from_slice(&1u16.to_le_bytes());
    extended[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    extended[130..134].copy_from_slice(&7u32.to_le_bytes());
    extended[134..].copy_from_slice(SKETCH_MARKER);
    let mut extended_line = line.clone();
    extended_line.coordinates_m = None;
    let markers = [&extended_line, &first, &second];
    assert_eq!(
        coordinate_centered_line_endpoints(&extended, &extended_line, &markers),
        Some([&first, &second])
    );
    extended[84] ^= 1;
    assert_eq!(
        coordinate_centered_line_endpoints(&extended, &extended_line, &markers),
        None
    );
}

#[test]
fn current_coordinate_line_uses_its_single_local_link() {
    let mut payload = vec![0; 157];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[86..88].copy_from_slice(&0xbc87u16.to_le_bytes());
    payload[88..90].copy_from_slice(&22u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[102..106].copy_from_slice(&(-2i32).to_le_bytes());
    payload[152..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, local_id, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            crate::records::SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, local_id);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let line = entity(
        "line",
        0,
        Some(1),
        SketchInputKind::LineOrCircle,
        Some([2.0, 3.0]),
    );
    let endpoint = entity(
        "endpoint",
        153,
        Some(22),
        SketchInputKind::Point,
        Some([4.0, 5.0]),
    );
    assert_eq!(
        current_coordinate_linked_line_endpoints(&payload, &line, &[&line, &endpoint]),
        Some([&line, &endpoint])
    );
}

#[test]
fn current_coordinate_line_accepts_a_coordinate_bearing_curve_vertex() {
    let mut payload = vec![0; 157];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[86..88].copy_from_slice(&0xbc87u16.to_le_bytes());
    payload[88..90].copy_from_slice(&22u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[102..106].copy_from_slice(&(-2i32).to_le_bytes());
    payload[152..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, local_id, kind, coordinates_m| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            crate::records::SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(None, local_id);
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let line = entity(
        "line",
        0,
        Some(1),
        SketchInputKind::LineOrCircle,
        Some([2.0, 3.0]),
    );
    let endpoint = entity(
        "curve-vertex",
        153,
        Some(22),
        SketchInputKind::LineOrCircle,
        Some([4.0, 5.0]),
    );
    assert_eq!(
        current_coordinate_linked_line_endpoints(&payload, &line, &[&line, &endpoint]),
        Some([&line, &endpoint])
    );
}

#[test]
fn extended_wide_selected_axis_uses_object_ids_then_one_based_point_roster() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[88..92].fill(0xff);
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity =
        |id: &str, offset: u64, object_index: Option<u32>, coordinates_m: Option<[f64; 2]>| {
            let marker_id: String = id.into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker = crate::records::SketchInputEntity::new(
                marker_id,
                marker_parent,
                0,
                offset,
                if coordinates_m.is_some() {
                    SketchInputKind::Point
                } else {
                    SketchInputKind::LineOrCircle
                },
            );
            constructed_marker.feature_ref = Some("sketch".into());
            constructed_marker = constructed_marker.with_test_identity(object_index, None);
            constructed_marker.state_value = Some(1.0);
            constructed_marker.coordinates_m = coordinates_m;
            constructed_marker.links = None;
            constructed_marker
        };
    let curve = entity("curve", 0, Some(8), None);
    let first = entity("first", 10, Some(1), Some([0.0, 0.0]));
    let second = entity("second", 20, Some(3), Some([1.0, 0.0]));
    let third = entity("third", 30, Some(20), Some([2.0, 0.0]));
    let markers = [&curve, &first, &second, &third];

    assert_eq!(
        extended_wide_selected_axis_endpoints(&payload, &curve, &markers)
            .expect("object-index endpoints")
            .map(crate::records::SketchInputEntity::id),
        ["first", "second"]
    );

    payload[64..66].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_wide_selected_axis_endpoints(&payload, &curve, &markers)
            .expect("one-based roster endpoints")
            .map(crate::records::SketchInputEntity::id),
        ["third", "second"]
    );
}

#[test]
fn current_line_resolves_one_based_point_roster_endpoints() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind: SketchInputKind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            crate::records::SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    let endpoints = one_based_point_roster_line_endpoint_markers(&payload, &curve, &markers)
        .expect("one-based point roster");
    assert_eq!(
        endpoints.map(crate::records::SketchInputEntity::id),
        ["second", "fourth"]
    );

    let arc = entity("arc", 50, None, SketchInputKind::Arc);
    let mixed = markers
        .iter()
        .copied()
        .chain(std::iter::once(&arc))
        .collect::<Vec<_>>();
    assert_eq!(
        one_based_point_roster_line_endpoint_markers(&payload, &curve, &mixed),
        None
    );

    payload[56..58].fill(0);
    assert_eq!(
        one_based_point_roster_line_endpoint_markers(&payload, &curve, &markers),
        None
    );
}

#[test]
fn legacy_geometry_locus_line_resolves_zero_based_point_roster_endpoints() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&5u32.to_le_bytes());
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind: SketchInputKind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            crate::records::SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    let endpoints = legacy_point_roster_line_endpoint_markers(&payload, &curve, &markers)
        .expect("zero-based point roster");
    assert_eq!(
        endpoints.map(crate::records::SketchInputEntity::id),
        ["second", "fourth"]
    );

    payload[80..84].fill(0xff);
    assert_eq!(
        legacy_point_roster_line_endpoint_markers(&payload, &curve, &markers),
        None
    );
}

#[test]
fn terminal_legacy_indexed_curve_retains_its_sibling_line_kind() {
    let detail = 84;
    let mut payload = vec![0; detail * 2];
    for offset in [0, detail] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let entity = |id: &str, offset, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            crate::records::SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = Some(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    let sibling = entity("sibling", 0, SketchInputKind::LineOrCircle);
    let terminal = entity("terminal", detail as u64, SketchInputKind::Arc);

    assert!(legacy_terminal_indexed_profile_line(
        &payload,
        &terminal,
        &[&sibling, &terminal],
    ));
    assert!(!legacy_terminal_indexed_profile_line(
        &payload,
        &terminal,
        &[&terminal],
    ));
}

#[test]
fn native_owner_operand_keeps_a_missing_source_index() {
    let relation = SketchInputEntity::new(
        "relation",
        "lane",
        0,
        0,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Horizontal),
    );
    let mut owner = SketchInputEntity::new("owner", "lane", 1, 1, SketchInputKind::LineOrCircle);
    owner.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 4,
            entity_ref: relation.id().to_string(),
        }],
    );
    for index in [None, Some(7)] {
        owner = owner.with_test_identity(owner.object_index(), index);
        let markers = HashMap::from([(relation.id(), &relation), (owner.id(), &owner)]);
        let Some(SketchConstraintDefinitionInput::Native { operands, .. }) =
            typed_marker_relation_definition(&relation, &markers, &HashMap::new())
        else {
            panic!("native relation");
        };
        assert_eq!(
            operands
                .iter()
                .map(|operand| operand.object_index)
                .collect::<Vec<_>>(),
            vec![index]
        );
    }
}
