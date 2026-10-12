//! Coordinate rosters tests.

use crate::records::SketchInputEntity;
use crate::records::SketchInputKind;
use crate::records::SketchRelationKind;
use crate::resolved_features::endpoints::coordinate_roster_arc_center;
use crate::resolved_features::endpoints::coordinate_roster_curve_endpoint_markers;
use crate::resolved_features::endpoints::current_direct_92_profile_line_endpoint_indices;
use crate::resolved_features::endpoints::current_identity_linked_wide_curve_uses_one_based_roster;
use crate::resolved_features::endpoints::extended_compact_endpoint_markers;
use crate::resolved_features::endpoints::legacy_undetailed_profile_line;
use crate::resolved_features::endpoints::roster_curve_endpoint_markers;
use crate::resolved_features::endpoints::wide_indexed_curve_endpoint_indices;
use crate::resolved_features::markers::sketch_input_entities;
use crate::resolved_features::typed_relations::current_undetailed_bounded_curve_is_line;
use crate::resolved_features::CLASS_MARKER;
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
use crate::resolved_features::LEGACY_SKETCH_MARKER;
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::u64_from_index;

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
    let entity = |id: &str, object_index, coordinates_m: Option<[f64; 2]>, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(marker_id, marker_parent, 0, 0, kind);
        constructed_marker.feature_ref = Some("profile".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
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
    assert!({
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = extended_compact_endpoint_markers(
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
    .unwrap()
    .is_empty());

    payload.resize(96 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[82..84].fill(0);
    assert!({
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = extended_compact_endpoint_markers(
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
    .is_empty());

    payload.resize(102, 0);
    payload[56..58].copy_from_slice(&14u16.to_le_bytes());
    payload[58..60].copy_from_slice(&16u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..102].fill(0);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    let mut roster_indexed = entities.clone();
    roster_indexed[1] = roster_indexed[1].with_test_identity(None, roster_indexed[1].local_id());
    roster_indexed[3] = roster_indexed[3].with_test_identity(None, roster_indexed[3].local_id());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    let markers = roster_indexed.iter().collect::<Vec<_>>();
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
                &roster_ctx,
                &payload,
                &roster_indexed[0],
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
        .iter()
        .map(|marker| marker.id())
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
                &roster_ctx,
                &payload,
                &roster_indexed[0],
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
        .iter()
        .map(|marker| marker.id())
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
    let entity = |id: &str, offset, object_index, kind, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&[0; 2]);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_compact_endpoint_markers(
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload[29..31].copy_from_slice(&2u16.to_le_bytes());
    assert!({
        let roster_ctx = cadmpeg_test_support::service_decode_context();
        let result = extended_compact_endpoint_markers(
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
    .is_empty());
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
        .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
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
    centered_entities[0].coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
    centered_entities[0].reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    centered_entities[1].coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);
    centered_entities[2].coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]);
    let centered_markers = centered_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &centered_entities[3],
                [&centered_entities[1], &centered_entities[2]],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &centered_markers,
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
    let mut hybrid_entities = centered_entities.clone();
    let mut additional_endpoint = hybrid_entities[2].clone();
    additional_endpoint.set_test_id(format!("{}:additional", additional_endpoint.id()));
    additional_endpoint = additional_endpoint.with_test_position(
        additional_endpoint.ordinal(),
        additional_endpoint.offset() + 1,
    );
    additional_endpoint.coordinates_m = cadmpeg_ir::units::FiniteVector::new([-1.0, 0.0]);
    hybrid_entities.insert(3, additional_endpoint);
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &hybrid_entities[4],
                [&hybrid_entities[3], &hybrid_entities[2]],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &hybrid_markers,
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
    hybrid_entities[0].coordinates_m = cadmpeg_ir::units::FiniteVector::new([4.0, 4.0]);
    hybrid_entities[1].coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
    hybrid_entities[1] =
        hybrid_entities[1].with_test_identity(Some(0), hybrid_entities[1].local_id());
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_arc_center(
                &ctx,
                &payload,
                &hybrid_entities[4],
                [&hybrid_entities[3], &hybrid_entities[2]],
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &hybrid_markers,
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
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
        .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    assert!(legacy_undetailed_profile_line(&payload, curve_offset));

    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        crate::resolved_features::endpoints::extended_compact_indexed_curve_endpoint_indices(
            &payload,
            curve_offset
        ),
        Some([2, 3])
    );
    assert_eq!(
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[3],
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
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
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
    complete_roster_entities[0]
        .reclassify(SketchInputKind::Relation(SketchRelationKind::Horizontal));
    let complete_roster_markers = complete_roster_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &payload,
                &complete_roster_entities[3],
                &complete_roster_markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &complete_roster_markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
                None,
            );
            result
        }
        .unwrap()
        .iter()
        .map(|marker| marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get))
        .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 56..curve_offset + 58].fill(0);
    assert!({
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = roster_curve_endpoint_markers(
            &ctx,
            &payload,
            &complete_roster_entities[3],
            &complete_roster_markers,
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &ctx,
                &complete_roster_markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &ctx, &payload,
                )
                .unwrap(),
            )
            .unwrap(),
            None,
        );
        result
    }
    .unwrap()
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

    let point = |id: &str, offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            u64_from_index(curve_offset),
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[4],
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
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["fourth", "second"]
    );
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &payload,
                &entities[4],
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
                None,
            );
            result
        }
        .unwrap()
        .iter()
        .map(|marker| marker.id())
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

    let point = |id: &str, offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            u64_from_index(curve_offset),
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[4],
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
        .iter()
        .map(|marker| marker.id())
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
        payload[offset + 58..offset + 66]
            .copy_from_slice(&(f64::from(u32::try_from(index).unwrap())).to_le_bytes());
        payload[offset + 66..offset + 74]
            .copy_from_slice(&(f64::from(u32::try_from(index + 1).unwrap())).to_le_bytes());
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

    let point = |id: &str, offset, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, offset, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            u64_from_index(curve_offset),
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    let mut entities = (0..8)
        .map(|index| {
            point(
                &format!("point{index}"),
                index * 134,
                Some([
                    f64::from(u32::try_from(index).unwrap()),
                    f64::from(u32::try_from(index + 1).unwrap()),
                ]),
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
        {
            let roster_ctx = cadmpeg_test_support::service_decode_context();
            let result = coordinate_roster_curve_endpoint_markers(
                &roster_ctx,
                &payload,
                &entities[8],
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
        .iter()
        .map(|marker| marker.id())
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
