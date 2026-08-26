//! Tests for the `curves` module.

use super::super::endpoints::{compact_legacy_code_one_line_endpoint_indices, minor_arc_geometry};
use super::super::markers::sketch_input_entities;
use super::super::typed_relations::compact_legacy_object_line_endpoints;
use super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::*;
use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};

#[test]
fn shared_endpoint_block_cycles_remain_profile_chains() {
    let sketch = SketchId("block-sketch".into());
    let line = |id: &str, start: &str, end: &str| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: vec![start.into(), end.into()],
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let entities = vec![
        line("bottom", "p0", "p1"),
        line("right", "p1", "p2"),
        line("top", "p2", "p3"),
        line("left", "p3", "p0"),
        line("diagonal", "p0", "p2"),
    ];

    assert!(super::closed_marker_profiles(&entities).is_empty());
    let profiles = closed_marker_profiles_allowing_shared_endpoints(&entities);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 4);
}

#[test]
fn compact_line_region_is_an_ordered_one_based_curve_roster() {
    let mut payload = b"moSketchRegion_c".to_vec();
    payload.extend(0x8060u16.to_le_bytes());
    payload.extend(4u16.to_le_bytes());
    for address in [2u16, 1, 4, 3] {
        payload.extend(0x80e1u16.to_le_bytes());
        payload.extend(address.to_le_bytes());
        payload.extend([0xff; 4]);
        payload.extend([0; 4]);
    }
    assert_eq!(
        compact_line_region_addresses(&payload),
        Some(vec![2, 1, 4, 3])
    );
    payload[22] = 1;
    assert_eq!(compact_line_region_addresses(&payload), None);
}

#[test]
fn compact_line_chain_is_an_ordered_one_based_vertex_roster() {
    let mut payload = Vec::new();
    payload.extend(4u16.to_le_bytes());
    for address in [3u32, 2, 1, 4] {
        payload.extend(address.to_le_bytes());
    }
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u16.to_le_bytes());
    payload.extend(6u32.to_le_bytes());
    payload.extend([0xff; 4]);
    payload.extend([0; 8]);
    payload.extend(5u32.to_le_bytes());
    payload.extend(5u32.to_le_bytes());
    payload.extend([0xff, 0xfe, 0xff, 0, 0, 0]);
    payload.extend([0xff; 4]);
    assert_eq!(
        compact_line_chain_addresses(&payload),
        Some(vec![3, 2, 1, 4])
    );
    payload[24] = 4;
    assert_eq!(compact_line_chain_addresses(&payload), None);
}

#[test]
fn compact_rectangle_requires_each_axis_corner_exactly_once() {
    let corners = [
        Point2::new(25.75, 14.15),
        Point2::new(-25.75, -14.15),
        Point2::new(-25.75, 14.15),
        Point2::new(25.75, -14.15),
    ];
    assert_eq!(
        ordered_rectangle_corners(&corners),
        Some([
            Point2::new(-25.75, -14.15),
            Point2::new(25.75, -14.15),
            Point2::new(25.75, 14.15),
            Point2::new(-25.75, 14.15),
        ])
    );

    let duplicate = [corners[0], corners[0], corners[2], corners[3]];
    assert_eq!(ordered_rectangle_corners(&duplicate), None);
    let non_rectangular = [
        corners[0],
        corners[1],
        corners[2],
        Point2::new(24.0, -14.15),
    ];
    assert_eq!(ordered_rectangle_corners(&non_rectangular), None);
}

#[test]
fn indexed_line_cycle_carries_rectangle_from_known_vertices() {
    const CURVE_START: usize = 400;
    let mut payload = vec![0; CURVE_START + 4 * 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    let edges = [[0u16, 2u16], [0, 3], [3, 1], [2, 1]];
    for (index, edge) in edges.into_iter().enumerate() {
        let offset = CURVE_START + index * 84;
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31..offset + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&edge[1].to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    payload[CURVE_START + 3 * 84 + 74..CURVE_START + 3 * 84 + 76]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[CURVE_START + 4 * 84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>,
                  kind| SketchInputEntity {
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
    let markers = [
        marker(
            "first",
            0,
            None,
            Some([-0.025, -0.011]),
            SketchInputKind::Point,
        ),
        marker(
            "opposite",
            100,
            None,
            Some([0.025, 0.011]),
            SketchInputKind::Point,
        ),
        marker("third", 200, None, None, SketchInputKind::Point),
        marker("fourth", 300, None, None, SketchInputKind::Point),
        marker(
            "line-1",
            CURVE_START as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-2",
            (CURVE_START + 84) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-3",
            (CURVE_START + 168) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-4",
            (CURVE_START + 252) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();

    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &marker_refs),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    for index in 0..4 {
        let offset = CURVE_START + index * 84;
        payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &marker_refs),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    for index in 0..4 {
        let offset = CURVE_START + index * 84;
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    }
    let mut adjacent = markers.clone();
    adjacent[1].coordinates_m = None;
    adjacent[2].coordinates_m = Some([0.025, 0.011]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &adjacent.iter().collect::<Vec<_>>(),),
        None
    );

    let mut three_corners = markers;
    three_corners[2].coordinates_m = Some([0.025, -0.011]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    let mut current_payload = payload.clone();
    for index in 0..=4 {
        let offset = CURVE_START + index * 84;
        current_payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        if index < 4 {
            current_payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
        }
    }
    let mut current_corners = three_corners.clone();
    for (index, marker) in current_corners.iter_mut().take(4).enumerate() {
        marker.object_index = Some(index as u32 + 1);
    }
    for marker in current_corners.iter_mut().skip(4) {
        marker.kind = SketchInputKind::Arc;
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &current_payload,
            &current_corners.iter().collect::<Vec<_>>(),
        ),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    three_corners[2].coordinates_m = Some([0.024, -0.010]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        None
    );
    three_corners[0].coordinates_m = Some([0.013, -0.025]);
    three_corners[1].coordinates_m = Some([0.0, -0.03]);
    three_corners[2].coordinates_m = Some([0.01, 0.0]);
    three_corners[3].coordinates_m = Some([0.0, 0.0]);
    for (index, edge) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 84;
        payload[offset + 56..offset + 58].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&edge[1].to_le_bytes());
    }
    payload[CURVE_START + 3 * 84 + 72..CURVE_START + 4 * 84].fill(0);
    payload.truncate(CURVE_START + 4 * 84);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );

    let mut wide = vec![0; CURVE_START + 4 * 92 + SKETCH_MARKER.len()];
    for (index, edge) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 92;
        wide[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        wide[offset + 5..offset + 13].fill(0xff);
        wide[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        wide[offset + 17..offset + 21]
            .copy_from_slice(&(if index == 3 { 2u32 } else { 1 }).to_le_bytes());
        wide[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        wide[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
        wide[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        wide[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        wide[offset + 64..offset + 66].copy_from_slice(&edge[0].to_le_bytes());
        wide[offset + 66..offset + 68].copy_from_slice(&edge[1].to_le_bytes());
        wide[offset + 68..offset + 72].copy_from_slice(&1u32.to_le_bytes());
        wide[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
        wide[offset + 84..offset + 88]
            .copy_from_slice(&u32::try_from(index + 1).unwrap().to_le_bytes());
    }
    wide[CURVE_START + 4 * 92..].copy_from_slice(SKETCH_MARKER);
    let mut wide_markers = three_corners.to_vec();
    wide_markers.insert(
        0,
        marker("header", 0, None, None, SketchInputKind::LineOrCircle),
    );
    for (index, (marker, coordinates)) in wide_markers[1..5]
        .iter_mut()
        .zip([[0.01, -0.03], [0.0, -0.03], [0.01, 0.0], [0.0, 0.0]])
        .enumerate()
    {
        marker.offset = u64::try_from(index + 1).unwrap();
        marker.coordinates_m = Some(coordinates);
        marker.kind = SketchInputKind::Point;
    }
    for (index, marker) in wide_markers[5..].iter_mut().enumerate() {
        marker.offset = (CURVE_START + index * 92) as u64;
        marker.kind = if index == 3 {
            SketchInputKind::Arc
        } else {
            SketchInputKind::LineOrCircle
        };
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(&wide, &wide_markers.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );
    let mut three_sides = wide[..CURVE_START + 3 * 92 + SKETCH_MARKER.len()].to_vec();
    for (index, edge) in [[1u16, 2u16], [2, 4], [4, 3]].into_iter().enumerate() {
        let offset = CURVE_START + index * 92;
        three_sides[offset + 64..offset + 66].copy_from_slice(&edge[0].to_le_bytes());
        three_sides[offset + 66..offset + 68].copy_from_slice(&edge[1].to_le_bytes());
    }
    three_sides[CURVE_START + 92 + 23..CURVE_START + 92 + 27]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    three_sides[CURVE_START + 2 * 92 + 17..CURVE_START + 2 * 92 + 21]
        .copy_from_slice(&2u32.to_le_bytes());
    let mut three_side_markers = wide_markers[..8].to_vec();
    three_side_markers[4].coordinates_m = Some([1.0e-17, 0.0]);
    three_side_markers[7].kind = SketchInputKind::Arc;
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &three_sides,
            &three_side_markers.iter().collect::<Vec<_>>(),
        ),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );
    three_sides[CURVE_START + 92 + 64..CURVE_START + 92 + 68].copy_from_slice(&[1, 0, 4, 0]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &three_sides,
            &three_side_markers.iter().collect::<Vec<_>>(),
        ),
        None
    );
    wide[CURVE_START + 3 * 92 + 17..CURVE_START + 3 * 92 + 21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        indexed_rectangle_from_line_cycle(&wide, &wide_markers.iter().collect::<Vec<_>>(),),
        None
    );
}

#[test]
fn compact_legacy_object_index_cycle_carries_rectangle() {
    const CURVE_START: usize = 400;
    let mut payload = vec![0; CURVE_START + 4 * 68 + LEGACY_SKETCH_MARKER.len()];
    for (index, edge) in [[0u16, 3u16], [0, 2], [2, 1], [3, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 68;
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 19..offset + 25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 25..offset + 27].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31] = 4;
        payload[offset + 42..offset + 44].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 44..offset + 46].copy_from_slice(&edge[1].to_le_bytes());
        payload[offset + 46..offset + 50].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 50..offset + 58].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let terminal = CURVE_START + 3 * 68;
    payload.resize(terminal + 116, 0);
    payload[terminal + 58..terminal + 104].fill(0);
    payload[terminal + 104..terminal + 106].copy_from_slice(&4u16.to_le_bytes());
    payload[terminal + 106..terminal + 110].copy_from_slice(CLASS_MARKER);
    payload[terminal + 110..terminal + 112].copy_from_slice(&4u16.to_le_bytes());
    payload[terminal + 112..terminal + 116].copy_from_slice(b"line");
    let marker =
        |id: &str, offset: u64, object_index: u32, coordinates_m: Option<[f64; 2]>, kind| {
            SketchInputEntity {
                id: id.into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 0,
                offset,
                object_index: Some(object_index),
                local_id: None,
                kind,
                state_value: Some(1.0),
                coordinates_m,
                links: Vec::new(),
                link_selector: None,
            }
        };
    let markers = [
        marker("missing", 0, 1, None, SketchInputKind::Point),
        marker("top-left", 100, 2, Some([0.0, 1.0]), SketchInputKind::Point),
        marker(
            "top-right",
            200,
            3,
            Some([2.0, 1.0]),
            SketchInputKind::Point,
        ),
        marker(
            "bottom-left",
            300,
            4,
            Some([0.0, 0.0]),
            SketchInputKind::Point,
        ),
        marker(
            "line-1",
            CURVE_START as u64,
            1,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-2",
            (CURVE_START + 68) as u64,
            2,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-3",
            (CURVE_START + 136) as u64,
            3,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-4",
            (CURVE_START + 204) as u64,
            4,
            None,
            SketchInputKind::LineOrCircle,
        ),
    ];

    assert_eq!(
        (0..4)
            .map(|index| {
                compact_legacy_code_one_line_endpoint_indices(&payload, CURVE_START + index * 68)
            })
            .collect::<Vec<_>>(),
        [Some([1, 4]), Some([1, 3]), Some([3, 2]), Some([4, 2])]
    );
    assert_eq!(
        compact_legacy_object_line_endpoints(
            &payload,
            &markers[6],
            &markers.iter().collect::<Vec<_>>(),
        )
        .map(|endpoints| [endpoints[0].id.as_str(), endpoints[1].id.as_str()]),
        Some(["top-right", "top-left"])
    );
    payload[terminal + 106] = 0;
    assert_eq!(
        compact_legacy_code_one_line_endpoint_indices(&payload, terminal),
        None
    );
    assert_eq!(
        compact_legacy_rectangle_line_endpoints(&payload, terminal),
        Some([4, 2])
    );
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &markers.iter().collect::<Vec<_>>()),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );

    let mut geometry_locus = payload;
    for (index, endpoints) in [[0u16, 3u16], [0, 2], [2, 1], [3, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = index * 84;
        geometry_locus[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        geometry_locus[offset + 56..offset + 58].copy_from_slice(&endpoints[0].to_le_bytes());
        geometry_locus[offset + 58..offset + 60].copy_from_slice(&endpoints[1].to_le_bytes());
        geometry_locus[offset + 76..offset + 80]
            .copy_from_slice(&u32::try_from(index + 1).unwrap().to_le_bytes());
        geometry_locus[offset + 80..offset + 84]
            .copy_from_slice(&u32::try_from((index + 1) % 4 + 1).unwrap().to_le_bytes());
    }
    let mut diagonal = markers;
    diagonal[0].kind = SketchInputKind::Point;
    diagonal[0].coordinates_m = Some([0.0, 0.0]);
    diagonal[1].coordinates_m = Some([2.0, 1.0]);
    diagonal[2].coordinates_m = None;
    diagonal[3].coordinates_m = None;
    assert_eq!(
        indexed_rectangle_from_line_cycle(&geometry_locus, &diagonal.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );
}

#[test]
fn current_compact_line_cycle_infers_its_missing_rectangle_corner() {
    let mut payload = vec![0; 4 * 84 + SKETCH_MARKER.len()];
    for (index, endpoints) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = index * 84;
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 23..offset + 31]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&endpoints[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&endpoints[1].to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    payload[3 * 84 + 74..3 * 84 + 76].copy_from_slice(&2u16.to_le_bytes());
    payload[4 * 84..].copy_from_slice(SKETCH_MARKER);
    let marker =
        |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset,
            object_index,
            local_id: None,
            kind: if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
            state_value: Some(1.0),
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
    let markers = [
        marker("missing", 500, Some(1), None),
        marker("top-right", 510, Some(2), Some([2.0, 1.0])),
        marker("bottom-left", 520, Some(3), Some([0.0, 0.0])),
        marker("bottom-right", 530, Some(4), Some([2.0, 0.0])),
        marker("line-1", 0, Some(1), None),
        marker("line-2", 84, Some(2), None),
        marker("line-3", 168, Some(3), None),
        marker("line-4", 252, Some(4), None),
    ];

    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &markers.iter().collect::<Vec<_>>()),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );
}

#[test]
fn legacy_rectangle_diagonal_carries_one_endpoint_and_two_distinct_corner_links() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.025f64).to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.011f64).to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x03, 0x00]);
    payload[78..80].copy_from_slice(&0x80ecu16.to_le_bytes());
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[82..86].fill(0xff);
    payload[86..88].copy_from_slice(&0x80ecu16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = SketchInputEntity {
        id: "diagonal".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&payload, &marker),
        Some([-0.025, -0.011])
    );
    let mut terminal = payload.clone();
    terminal[136..142].fill(0);
    terminal[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&terminal, &marker),
        Some([-0.025, -0.011])
    );
    payload[88..90].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&payload, &marker),
        None
    );
}

#[test]
fn dimensioned_rectangle_selects_one_complete_marker_product() {
    let marker = |id: &str, u, v| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([u, v]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker("center", -0.023, 0.0),
        marker("lower-left", -0.02575, -0.00425),
        marker("upper-right", -0.02025, 0.00425),
        marker("lower-right", -0.02025, -0.00425),
        marker("upper-left", -0.02575, 0.00425),
        marker("axis-top", -0.02575, 0.01415),
        marker("axis-bottom", -0.02575, -0.01415),
        marker("origin", 0.0, 0.0),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[8.5, 5.5])
            .map(|markers| markers.map(|marker| marker.id.as_str())),
        Some(["lower-left", "lower-right", "upper-right", "upper-left"])
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[8.5]),
        None
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[28.3, 5.5]),
        None
    );

    let second_rectangle = [
        marker("second-lower-left", 0.010, 0.020),
        marker("second-lower-right", 0.0155, 0.020),
        marker("second-upper-right", 0.0155, 0.0285),
        marker("second-upper-left", 0.010, 0.0285),
    ];
    let ambiguous = marker_refs
        .iter()
        .copied()
        .chain(second_rectangle.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ambiguous, &[8.5, 5.5]),
        None
    );
}

#[test]
fn compact_line_endpoint_pairs_form_one_oriented_cycle() {
    let marker = SketchInputEntity {
        id: "marker".into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let point = |u, v| Point2::new(u, v);
    let lines = vec![
        (
            SketchEntityId("top".into()),
            &marker,
            &marker,
            point(0.0, 1.0),
            point(1.0, 1.0),
        ),
        (
            SketchEntityId("bottom".into()),
            &marker,
            &marker,
            point(0.0, 0.0),
            point(1.0, 0.0),
        ),
        (
            SketchEntityId("right".into()),
            &marker,
            &marker,
            point(1.0, 0.0),
            point(1.0, 1.0),
        ),
        (
            SketchEntityId("left".into()),
            &marker,
            &marker,
            point(0.0, 1.0),
            point(0.0, 0.0),
        ),
    ];

    let profile = ordered_compact_line_profile(&lines).expect("closed line cycle");
    assert_eq!(
        profile
            .iter()
            .map(|use_| (use_.entity.0.as_str(), use_.reversed))
            .collect::<Vec<_>>(),
        [
            ("top", false),
            ("right", true),
            ("bottom", true),
            ("left", true)
        ]
    );
    assert_eq!(complete_ordered_compact_line_profile(&lines, 5), None);
}

#[test]
fn linked_semicircle_records_close_a_two_center_profile() {
    let mut payload = vec![0; 224];
    for (offset, addresses) in [(0, [1u16, 2]), (112, [3, 5])] {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
        payload[offset + 23..offset + 31]
            .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 64..offset + 66].copy_from_slice(&addresses[0].to_le_bytes());
        payload[offset + 66..offset + 68].copy_from_slice(&addresses[1].to_le_bytes());
        payload[offset + 68..offset + 72].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[offset + 80..offset + 84].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 86..offset + 102].copy_from_slice(&[
            0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
            0xff, 0xff,
        ]);
    }
    assert!(current_linked_semicircle_record(&payload, 0));
    assert!(current_linked_semicircle_record(&payload, 112));
    let marker = |id: &str, offset, center: &str| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: vec![SketchInputLink {
            entity_ref: center.into(),
            local_id: 1,
        }],
        link_selector: Some(1),
    };
    let records = [
        marker("curve-a", 0, "center-a"),
        marker("curve-b", 112, "center-b"),
    ];
    let markers = records.iter().collect::<Vec<_>>();
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let curve = |id: &str| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "sldprt:marker-geometry:1".into(),
        },
    };
    let mut entities = vec![
        point("center-a", Point2::new(0.0, 0.0)),
        point("a-plus", Point2::new(0.0, 2.0)),
        point("a-minus", Point2::new(0.0, -2.0)),
        point("center-b", Point2::new(3.0, 0.0)),
        point("b-plus", Point2::new(3.0, 2.0)),
        point("b-minus", Point2::new(3.0, -2.0)),
        curve("curve-a"),
        curve("curve-b"),
    ];

    resolve_two_center_semicircle_profile(&payload, &markers, &mut entities, 1.0e-9);

    assert_eq!(
        entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
            .count(),
        2
    );
    assert_eq!(
        entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .count(),
        2
    );
    assert!(entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Arc {
                radius: Length(radius),
                ..
            } => Some(radius),
            _ => None,
        })
        .all(|radius| (radius - 2.0).abs() < 1.0e-9));
}

#[test]
fn compact_curve_detail_tangent_distinguishes_lines_and_arcs() {
    let detail = 84;
    let mut payload = vec![0; detail + 80];
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail..detail + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    payload[detail + 31..detail + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 35..detail + 39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );
    assert_eq!(
        tangent_bounded_curve(
            Point2::new(0.0, 2.0),
            Point2::new(0.0, 0.0),
            [-1.0, 0.0],
            1.0e-9,
        ),
        Some(SketchGeometry::Arc {
            center: Point2::new(0.0, 1.0),
            radius: Length(1.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(-std::f64::consts::FRAC_PI_2),
        })
    );
    assert_eq!(
        tangent_bounded_curve(
            Point2::new(0.0, 2.0),
            Point2::new(0.0, 0.0),
            [0.0, -1.0],
            1.0e-9,
        ),
        Some(SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(0.0, 0.0),
        })
    );
}

#[test]
fn bounded_arc_normalization_uses_angular_tolerance() {
    let Some(SketchGeometry::Arc {
        start_angle,
        end_angle,
        ..
    }) = minor_arc_geometry(
        Point2::new(10.0, 0.0),
        Point2::new(0.0, -10.0),
        Point2::new(0.0, 0.0),
        4.0,
    )
    else {
        panic!("valid bounded arc should resolve");
    };
    let sweep = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
    assert!(sweep <= std::f64::consts::PI + 1.0e-9);
    assert!((start_angle.0 + std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!(end_angle.0.abs() < 1.0e-12);
}

#[test]
fn unresolved_fillet_without_tangent_record_remains_native() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry, endpoint_refs: &[&str]| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: endpoint_refs.iter().map(|id| (*id).into()).collect(),
        geometry,
    };
    let mut entities = vec![
        entity(
            "start",
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
            &[],
        ),
        entity(
            "end",
            SketchGeometry::Point {
                position: Point2::new(0.0, 1.0),
            },
            &[],
        ),
        entity(
            "start-line",
            SketchGeometry::Line {
                start: Point2::new(1.0, -1.0),
                end: Point2::new(1.0, 0.0),
            },
            &["start-line-other", "start"],
        ),
        entity(
            "end-line",
            SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(-1.0, 1.0),
            },
            &["end", "end-line-other"],
        ),
        entity(
            "fillet",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            &["start", "end"],
        ),
    ];

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[4].geometry,
        SketchGeometry::Native { .. }
    ));
}

#[test]
fn unresolved_fillet_between_arcs_remains_native_without_tangent_relation() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry, endpoint_refs: &[&str]| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: endpoint_refs.iter().map(|id| (*id).into()).collect(),
        geometry,
    };
    let mut entities = vec![
        entity(
            "start",
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
            &[],
        ),
        entity(
            "end",
            SketchGeometry::Point {
                position: Point2::new(0.0, 1.0),
            },
            &[],
        ),
        entity(
            "start-arc",
            SketchGeometry::Arc {
                center: Point2::new(2.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::PI),
            },
            &["start", "start-other"],
        ),
        entity(
            "end-arc",
            SketchGeometry::Arc {
                center: Point2::new(0.0, 2.0),
                radius: Length(1.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::PI),
            },
            &["end", "end-other"],
        ),
        entity(
            "fillet",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            &["start", "end"],
        ),
    ];

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[4].geometry,
        SketchGeometry::Native { .. }
    ));
}

#[test]
fn connected_marker_arc_uses_unique_equidistant_point_witness() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry, native_ref: &str, endpoint_refs: &[&str]| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(native_ref.into()),
        geometry_ref: None,
        endpoint_refs: endpoint_refs
            .iter()
            .map(|reference| (*reference).into())
            .collect(),
        geometry,
    };
    let mut entities = vec![
        entity(
            "center",
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
            "point:10",
            &[],
        ),
        entity(
            "start",
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
            "point:100",
            &[],
        ),
        entity(
            "end",
            SketchGeometry::Point {
                position: Point2::new(0.0, 1.0),
            },
            "point:200",
            &[],
        ),
        entity(
            "arc",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            "curve:300",
            &["point:100", "point:200"],
        ),
    ];

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[3].geometry,
        SketchGeometry::Arc {
            center,
            radius: Length(radius),
            ..
        } if center == Point2::new(0.0, 0.0) && radius == 1.0
    ));
}

#[test]
fn connected_marker_arc_with_mirror_centers_remains_native() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, native_ref: &str, position| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(native_ref.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let mut entities = vec![
        point("start", "point:100", Point2::new(1.0, 0.0)),
        point("between-center", "point:200", Point2::new(0.0, 0.0)),
        point("end", "point:300", Point2::new(0.0, 1.0)),
        point("outside-center", "point:400", Point2::new(1.0, 1.0)),
        SketchEntity {
            id: SketchEntityId("arc".into()),
            sketch,
            construction: false,
            native_ref: Some("curve:500".into()),
            geometry_ref: None,
            endpoint_refs: vec!["point:100".into(), "point:300".into()],
            geometry: SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
        },
    ];

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[4].geometry,
        SketchGeometry::Native { ref native_kind }
            if native_kind == "sldprt:marker-geometry:2"
    ));
}

#[test]
fn connected_marker_arc_uses_one_resolved_arc_in_a_closed_cycle() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let line = |id: &str, start: &str, end: &str, start_position, end_position| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: vec![start.into(), end.into()],
        geometry: SketchGeometry::Line {
            start: start_position,
            end: end_position,
        },
    };
    let center = Point2::new(9.5, 0.0);
    let radius = (9.5_f64.powi(2) + 2.0_f64.powi(2)).sqrt();
    let mut entities = vec![
        point("left-top", Point2::new(0.0, 2.0)),
        point("left-bottom", Point2::new(0.0, -2.0)),
        point("right-top", Point2::new(19.0, 2.0)),
        point("right-bottom", Point2::new(19.0, -2.0)),
        line(
            "top",
            "left-top",
            "right-top",
            Point2::new(0.0, 2.0),
            Point2::new(19.0, 2.0),
        ),
        line(
            "bottom",
            "right-bottom",
            "left-bottom",
            Point2::new(19.0, -2.0),
            Point2::new(0.0, -2.0),
        ),
        SketchEntity {
            id: SketchEntityId("left-arc".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: Some("left-arc".into()),
            geometry_ref: None,
            endpoint_refs: vec!["left-top".into(), "left-bottom".into()],
            geometry: SketchGeometry::Arc {
                center,
                radius: Length(radius),
                start_angle: Angle((2.0_f64).atan2(-9.5)),
                end_angle: Angle((-2.0_f64).atan2(-9.5)),
            },
        },
        SketchEntity {
            id: SketchEntityId("right-arc".into()),
            sketch,
            construction: false,
            native_ref: Some("right-arc".into()),
            geometry_ref: None,
            endpoint_refs: vec!["right-top".into(), "right-bottom".into()],
            geometry: SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
        },
    ];

    let mut ambiguous_entities = entities.clone();
    let mut duplicate_witness = ambiguous_entities[6].clone();
    duplicate_witness.id = SketchEntityId("left-arc-duplicate".into());
    duplicate_witness.native_ref = Some("left-arc-duplicate".into());
    ambiguous_entities.push(duplicate_witness);
    super::resolve_connected_marker_arcs(&mut ambiguous_entities, 1.0e-9);
    assert!(matches!(
        ambiguous_entities[7].geometry,
        SketchGeometry::Native { ref native_kind }
            if native_kind == "sldprt:marker-geometry:2"
    ));

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[7].geometry,
        SketchGeometry::Arc {
            center: actual_center,
            radius: Length(actual_radius),
            ..
        } if actual_center == center && actual_radius == radius
    ));
}

#[test]
fn packed_slot_descriptor_run_is_not_independent_geometry() {
    let slot_offset = 22;
    let mut payload = vec![0; slot_offset + 252];
    let declaration = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    payload[..slot_offset].copy_from_slice(declaration);
    payload[slot_offset..slot_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[slot_offset + 5..slot_offset + 13].fill(0xff);
    payload[slot_offset + 13..slot_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[slot_offset + 17..slot_offset + 21].copy_from_slice(&0_u32.to_le_bytes());
    payload[slot_offset + 23..slot_offset + 29]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[slot_offset + 31..slot_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[slot_offset + 48..slot_offset + 56].copy_from_slice(&1.0_f64.to_le_bytes());
    for (index, (tag, id)) in [
        (0x8156_u16, 0_u16),
        (0x814c, 3),
        (0x8156, 1),
        (0x8156, 2),
        (0x8294, 0),
        (0x8294, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let start = slot_offset + 64 + index * 8;
        payload[start..start + 2].copy_from_slice(&tag.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload.copy_within(slot_offset..slot_offset + 126, slot_offset + 126);

    assert_eq!(
        super::slot_curve_and_center_indices(&payload, slot_offset),
        Some(([0, 3, 1, 2], [0, 1]))
    );
    assert_eq!(
        super::slot_curve_and_center_indices(&payload, slot_offset + 126),
        Some(([0, 3, 1, 2], [0, 1]))
    );

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 2);
    assert!(entities
        .iter()
        .all(|entity| entity.kind == SketchInputKind::Native(0)));
}
