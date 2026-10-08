//! Roster geometry tests.

use crate::records::SketchInputEntity;
use crate::records::SketchInputKind;
use crate::records::SketchRelationKind;
use crate::resolved_features::endpoints::coordinate_roster_arc_center;
use crate::resolved_features::endpoints::coordinate_roster_endpoint_offset;
use crate::resolved_features::endpoints::current_wide_arc_direct_markers;
use crate::resolved_features::endpoints::equal_index_coordinate_roster_full_circle;
use crate::resolved_features::endpoints::extended_profile_terminal_102_indexed_arc;
use crate::resolved_features::endpoints::indexed_arc_uses_coordinate_center;
use crate::resolved_features::endpoints::roster_curve_endpoint_markers;
use crate::resolved_features::endpoints::wide_direct_line_endpoint_markers;
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
use crate::resolved_features::SKETCH_MARKER;

const EPS_ROSTER_RADIUS: f64 = 1.0e-12;

#[test]
fn current_wide_arc_uses_direct_point_ids_with_an_arc_center_carrier() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&5u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&4u32.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity =
        |id: &str, object_index, coordinates_m: Option<[f64; 2]>, kind: SketchInputKind| {
            let marker_id: String = id.into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
            constructed_marker.feature_ref = Some("sketch".into());
            constructed_marker = constructed_marker.with_test_identity(object_index, None);
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m =
                coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
            constructed_marker.links = None;
            constructed_marker
        };
    let entities = [
        entity("curve", Some(2), None, SketchInputKind::Arc),
        entity("center", Some(4), Some([0.0, 0.0]), SketchInputKind::Arc),
        entity("start", Some(6), Some([0.0, 1.0]), SketchInputKind::Point),
        entity("end", Some(5), Some([0.0, -1.0]), SketchInputKind::Point),
        entity("shifted", Some(7), Some([2.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let crate::resolved_features::endpoints::CurrentWideArc(endpoints, center) = {
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = current_wide_arc_direct_markers(
            &ctx,
            &payload,
            &entities[0],
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &ctx, &payload,
                )
                .unwrap(),
            )
            .unwrap(),
        );
        result
    }
    .unwrap()
    .expect("direct endpoint IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &entities[0],
                [endpoints[0], endpoints[1]],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some(center)
    );
}

#[test]
fn wide_line_uses_direct_point_ids_after_one_based_resolution_fails() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity =
        |id: &str, object_index, coordinates_m: Option<[f64; 2]>, kind: SketchInputKind| {
            let marker_id: String = id.into();
            let marker_parent: String = "lane".into();
            let mut constructed_marker =
                SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
            constructed_marker.feature_ref = Some("sketch".into());
            constructed_marker = constructed_marker.with_test_identity(object_index, None);
            constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
            constructed_marker.coordinates_m =
                coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
            constructed_marker.links = None;
            constructed_marker
        };
    let entities = [
        entity("curve", Some(6), None, SketchInputKind::LineOrCircle),
        entity("start", Some(7), Some([-1.0, 0.0]), SketchInputKind::Point),
        entity("end", Some(8), Some([1.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let endpoints = {
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = wide_direct_line_endpoint_markers(
            &roster_ctx,
            &payload,
            &entities[0],
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &roster_ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &roster_ctx,
                    &payload,
                )
                .unwrap(),
            )
            .unwrap(),
        );
        result
    }
    .unwrap()
    .expect("direct point IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[64..66].fill(0);
    let zero = entity("zero", None, Some([0.0, 0.0]), SketchInputKind::Point);
    let extended = [&entities[0], &zero, &entities[2]];
    let endpoints = {
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = wide_direct_line_endpoint_markers(
            &roster_ctx,
            &payload,
            &entities[0],
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &roster_ctx,
                &extended,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &roster_ctx,
                    &payload,
                )
                .unwrap(),
            )
            .unwrap(),
        );
        result
    }
    .unwrap()
    .expect("unique zero-identity point");
    assert_eq!(endpoints[0].id(), "zero");

    let other_zero = entity("other-zero", None, Some([2.0, 0.0]), SketchInputKind::Point);
    let ambiguous = [&entities[0], &zero, &other_zero, &entities[2]];
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = wide_direct_line_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[0],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &roster_ctx,
                    &ambiguous,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &roster_ctx,
                        &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        None
    );
    payload[92] = 0;
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = wide_direct_line_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[0],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &roster_ctx,
                    &extended,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &roster_ctx,
                        &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        None
    );
}

#[test]
fn extended_marker104_arc_prefers_point_roster_endpoints() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, None, SketchInputKind::Arc);
    let object_indices = [1, 2, 4, 5, 6, 7, 8];
    let points = object_indices.map(|object_index| {
        entity(
            &format!("point-{object_index}"),
            u64::from(object_index) * 10,
            Some(object_index),
            Some([f64::from(object_index), 0.0]),
            SketchInputKind::Point,
        )
    });
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    let endpoints = {
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = roster_curve_endpoint_markers(
            &ctx,
            &payload,
            &curve,
            &markers,
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &ctx, &payload,
                )
                .unwrap(),
            )
            .unwrap(), None,
        );
        result
    }
    .unwrap();
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id())
            .collect::<Vec<_>>(),
        ["point-6", "point-8"]
    );
}

#[test]
fn extended_geometry_104_arc_uses_zero_based_roster_and_center_index() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&0u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..96].fill(0);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let center = entity("center", 10, Some([0.0, 0.0]), SketchInputKind::Point);
    let start = entity("start", 20, Some([1.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 30, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &center, &start, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &payload,
                &curve,
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(), None,
            );
            result
        }
        .unwrap()
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some([0.0, 0.0])
    );

    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some([0.0, 0.0])
    );

    payload[76..78].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        None
    );
}

#[test]
fn extended_compact_104_arc_uses_geometry_roster_for_center_index() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&3u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let relation = entity(
        "relation",
        1,
        Some([9.0, 9.0]),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    let first = entity("first", 10, Some([5.0, 5.0]), SketchInputKind::Point);
    let second = entity("second", 20, Some([6.0, 6.0]), SketchInputKind::Point);
    let center = entity("center", 30, Some([0.0, 0.0]), SketchInputKind::Point);
    let start = entity("start", 40, Some([1.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 50, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &relation, &first, &second, &center, &start, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some([0.0, 0.0])
    );
}

#[test]
fn extended_terminal_102_profile_arc_uses_object_center_fallback() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&5u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

    let entity = |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, None, SketchInputKind::Arc);
    let center = entity(
        "center",
        10,
        Some(4),
        Some([2.0, 0.0]),
        SketchInputKind::Point,
    );
    let start = entity(
        "start",
        20,
        Some(5),
        Some([0.0, 1.0]),
        SketchInputKind::Point,
    );
    let end = entity(
        "end",
        30,
        Some(7),
        Some([0.0, -1.0]),
        SketchInputKind::Point,
    );
    let markers = [&curve, &center, &start, &end];

    assert!(extended_profile_terminal_102_indexed_arc(&payload, 0));
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &payload,
                &curve,
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(), None,
            );
            result
        }
        .unwrap()
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["end", "start"]
    );
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&end, &start],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some([2.0, 0.0])
    );

    payload[94] = 1;
    assert!(!extended_profile_terminal_102_indexed_arc(&payload, 0));
    assert!(!indexed_arc_uses_coordinate_center(&payload, 0));
}

#[test]
fn coordinate_roster_arc_center_requires_matching_indexed_endpoints() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, kind, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, SketchInputKind::Arc, None);
    let relation = entity(
        "relation",
        1,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        Some([9.0, 9.0]),
    );
    let start = entity("start", 10, SketchInputKind::Point, Some([1.0, 0.0]));
    let end = entity("end", 20, SketchInputKind::Point, Some([0.0, 1.0]));
    let center = entity("center", 30, SketchInputKind::Point, Some([0.0, 0.0]));
    let distractor = entity("distractor", 40, SketchInputKind::Point, Some([4.0, 4.0]));
    let markers = [&curve, &relation, &start, &end, &center, &distractor];

    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        None
    );
}

#[test]
fn extended_geometry_116_arc_uses_relation_tail_and_center_index() {
    let mut payload = vec![0; 116 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..102].fill(0);
    payload[102..106].copy_from_slice(&4u32.to_le_bytes());
    payload[106..112].fill(0);
    payload[112..116].copy_from_slice(&3u32.to_le_bytes());
    payload[116..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", 0, None, SketchInputKind::Arc);
    let first = entity("first", 10, Some([9.0, 9.0]), SketchInputKind::Point);
    let start = entity("start", 20, Some([1.0, 0.0]), SketchInputKind::Point);
    let center = entity("center", 30, Some([0.0, 0.0]), SketchInputKind::Point);
    let end = entity("end", 40, Some([0.0, 1.0]), SketchInputKind::Point);
    let markers = [&curve, &first, &start, &center, &end];

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &curve,
                [&start, &end],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            result
        }
        .unwrap(),
        Some([0.0, 0.0])
    );

    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    let (circle_center, radius) = {
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = equal_index_coordinate_roster_full_circle(
            &payload,
            &curve,
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &roster_ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &roster_ctx,
                    &payload,
                )
                .unwrap(),
            )
            .unwrap(),
        );
        result
    }
    .unwrap()
    .expect("116-byte equal-index circle");
    assert_eq!(circle_center, [9.0, 9.0]);
    assert!((radius - 145.0_f64.sqrt()).abs() < EPS_ROSTER_RADIUS);
}
