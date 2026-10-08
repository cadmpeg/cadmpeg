//! Legacy rosters tests.

use crate::records::SketchInputEntity;
use crate::records::SketchInputKind;
use crate::resolved_features::endpoints::coordinate_roster_endpoint_offset;
use crate::resolved_features::endpoints::extended_state_one_84_profile_line_uses_point_roster;
use crate::resolved_features::endpoints::legacy_104_profile_line_endpoint_indices;
use crate::resolved_features::endpoints::legacy_compact_104_profile_line_endpoint_indices;
use crate::resolved_features::endpoints::legacy_state_one_84_profile_line_uses_point_roster;
use crate::resolved_features::endpoints::legacy_state_one_profile_line_uses_point_roster;
use crate::resolved_features::endpoints::legacy_wide_profile_roster_curve;
use crate::resolved_features::endpoints::roster_curve_endpoint_markers;
use crate::resolved_features::endpoints::wide_indexed_curve_endpoint_indices;
use crate::resolved_features::markers::marker_local_id;
use crate::resolved_features::selections::marker_local_links;
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
use crate::resolved_features::LEGACY_SKETCH_MARKER;
use cadmpeg_core::decode::u64_from_index;

#[test]
fn legacy_compact_104_profile_line_uses_one_based_point_indices() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&2u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[6, 0, 8, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 100..offset + 104].copy_from_slice(&3u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        Some([7, 9])
    );
    payload[offset + 96..offset + 100].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
}

#[test]
fn legacy_104_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&29u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&0u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 94..offset + 96].copy_from_slice(&[0x04, 0x00]);
    payload[offset + 100..offset + 104].copy_from_slice(&30u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            entity_offset,
            if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", u64_from_index(offset), None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        Some([2, 3])
    );
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
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
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 94..offset + 96].copy_from_slice(&[0; 2]);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
    payload[offset + 94..offset + 96].copy_from_slice(&[0x04, 0x00]);
    payload[offset + 104..offset + 109].fill(0);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );

    payload[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 94..offset + 96].fill(0);
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 104..offset + 109].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
}

#[test]
fn legacy_state_one_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 94..offset + 96].copy_from_slice(&[0; 2]);
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 100..offset + 104].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            entity_offset,
            if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", u64_from_index(offset), None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
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
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 100..offset + 104].copy_from_slice(&43u32.to_le_bytes());
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 100..offset + 104].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 96..offset + 100].fill(0xff);
    assert!(!legacy_state_one_profile_line_uses_point_roster(
        &payload, offset
    ));
}

#[test]
fn legacy_wide_profile_roster_curves_use_zero_based_geometry_roster() {
    let make_payload = |length: usize, first: u16, second: u16| {
        let mut payload = vec![0; length + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&1u32.to_le_bytes());
        payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[64..66].copy_from_slice(&first.to_le_bytes());
        payload[66..68].copy_from_slice(&second.to_le_bytes());
        payload[68..72].copy_from_slice(&1u32.to_le_bytes());
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        if length == 104 {
            payload[88..92].copy_from_slice(&0x0008_0000u32.to_le_bytes());
            payload[96..100].copy_from_slice(&4u32.to_le_bytes());
            payload[100..104].copy_from_slice(&1u32.to_le_bytes());
        } else {
            payload[80..84].copy_from_slice(&(-1i32).to_le_bytes());
            for relative in (86..102).step_by(4) {
                payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
            }
            payload[104..108].copy_from_slice(&21u32.to_le_bytes());
            payload[108..112].copy_from_slice(&6u32.to_le_bytes());
        }
        payload[length..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    };
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
    let curve = entity("curve", 0, SketchInputKind::LineOrCircle, None);
    let center = entity("center", 10, SketchInputKind::Arc, Some([0.0, 0.0]));
    let first = entity("first", 20, SketchInputKind::Point, Some([1.0, 0.0]));
    let second = entity("second", 30, SketchInputKind::Point, Some([2.0, 0.0]));
    let markers = [&curve, &center, &first, &second];

    let short = make_payload(104, 0, 2);
    assert!(legacy_wide_profile_roster_curve(&short, 0));
    assert_eq!(wide_indexed_curve_endpoint_indices(&short, 0), None);
    assert_eq!(marker_local_links(&short, 0), None);
    assert_eq!(marker_local_id(&short, 0), Some(0x0008_0000));
    assert_eq!(coordinate_roster_endpoint_offset(&short, 0), Some(64));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &short,
                &curve,
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &short,
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
        ["center", "second"]
    );

    let long = make_payload(112, 1, 2);
    assert!(legacy_wide_profile_roster_curve(&long, 0));
    assert_eq!(wide_indexed_curve_endpoint_indices(&long, 0), None);
    assert_eq!(marker_local_links(&long, 0), None);
    assert_eq!(coordinate_roster_endpoint_offset(&long, 0), Some(64));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &long,
                &curve,
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &long,
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
        ["first", "second"]
    );

    let mut invalid_short = short;
    invalid_short[96..100].copy_from_slice(&5u32.to_le_bytes());
    assert!(!legacy_wide_profile_roster_curve(&invalid_short, 0));
    let mut invalid_long = long;
    invalid_long[84..86].copy_from_slice(&4u16.to_le_bytes());
    assert!(!legacy_wide_profile_roster_curve(&invalid_long, 0));
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&invalid_long, 0),
        Some([2, 3])
    );
}

#[test]
fn legacy_state_one_84_profile_line_uses_zero_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[1, 0, 2, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].fill(0);
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            entity_offset,
            if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", u64_from_index(offset), None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    for selector in [[0x00, 0x00, 0x44, 0x00], [0x00, 0x00, 0x84, 0x00]] {
        payload[offset + 35..offset + 39].copy_from_slice(&selector);
        assert!(legacy_state_one_84_profile_line_uses_point_roster(
            &payload, offset
        ));
    }
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
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
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    payload[offset + 76..offset + 80].fill(0);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&43u32.to_le_bytes());
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].fill(0);
    assert!(!legacy_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
}

#[test]
fn extended_state_one_84_profile_line_uses_one_based_point_roster() {
    let offset = 4;
    let mut payload = vec![0; offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&41u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 39..offset + 48].fill(0);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[2, 0, 3, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].fill(0);
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let entity = |id: &str, entity_offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            entity_offset,
            if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = entity("curve", u64_from_index(offset), None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    assert_eq!(
        coordinate_roster_endpoint_offset(&payload, offset),
        Some(56)
    );
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
        .map(|endpoint| endpoint.id())
        .collect::<Vec<_>>(),
        ["second", "third"]
    );

    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[offset + 76..offset + 80].fill(0);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 76..offset + 80].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 80..offset + 84].copy_from_slice(&43u32.to_le_bytes());
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 80..offset + 84].copy_from_slice(&42u32.to_le_bytes());
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 84..].fill(0);
    assert!(!extended_state_one_84_profile_line_uses_point_roster(
        &payload, offset
    ));
}
