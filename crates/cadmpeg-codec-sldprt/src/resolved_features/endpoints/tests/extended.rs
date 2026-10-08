//! Extended and wide profile-curve endpoint tests.

use super::super::super::markers::marker_coordinates;
use super::super::super::markers::sketch_input_entities;
use super::super::super::typed_relations::current_undetailed_bounded_curve_is_line;
use super::super::super::typed_relations::extended_direct_object_line_endpoints;
use super::super::super::typed_relations::marker_curve_endpoint_markers;
use super::super::super::CLASS_MARKER;
use super::super::super::LEGACY_EXTENDED_SKETCH_MARKER;
use super::super::super::LEGACY_SKETCH_MARKER;
use super::super::super::SKETCH_MARKER;
use crate::records::SketchInputEntity;
use crate::records::SketchInputKind;
use crate::records::SketchInputLink;
use crate::resolved_features::endpoints::compact_indexed_curve_endpoint_indices;
use crate::resolved_features::endpoints::direct_indexed_curve_endpoint_indices;
use crate::resolved_features::endpoints::extended_declared_inline_line_endpoints;
use crate::resolved_features::endpoints::extended_direct_object_line_endpoint_ids;
use crate::resolved_features::endpoints::extended_identity_inline_line_endpoints;
use crate::resolved_features::endpoints::extended_linked_inline_line_endpoints;
use crate::resolved_features::endpoints::extended_tagged_indexed_curve_endpoint_indices;
use crate::resolved_features::endpoints::legacy_state_five_curve_endpoint_indices;
use crate::resolved_features::endpoints::marker_is_selected_construction_line;
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
    let mut external = {
        let marker_id: String = "external".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 1, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(3), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0075]);
        constructed_marker.links = None;
        constructed_marker
    };
    let mut curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            2,
            0,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(6), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };

    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_linked_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    external = external.with_test_identity(Some(1), external.local_id());
    curve = curve.with_test_identity(Some(4), curve.local_id());
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_linked_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[140] = 1;
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_linked_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
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
    let point = {
        let marker_id: String = "point".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 1, 200, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(5), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.01, 0.012]);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            2,
            0,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(6), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.007, 0.0075]);
        constructed_marker.links = None;
        constructed_marker
    };

    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&point, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    let chained_curve = {
        let mut constructed_marker = point.clone();
        constructed_marker.set_test_id("chained-curve");
        constructed_marker.reclassify(SketchInputKind::Arc);
        constructed_marker
    };
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&chained_curve, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&point, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::LineOrCircle
    );
    payload[74..84].copy_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let direct_curve = {
        let mut constructed_marker = curve.clone();
        constructed_marker.reclassify(SketchInputKind::Arc);
        constructed_marker
    };
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &direct_curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&chained_curve, &direct_curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind(),
        SketchInputKind::LineOrCircle
    );
    payload[126..130].fill(0);
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &direct_curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&chained_curve, &direct_curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        None
    );
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let duplicate = {
        let mut constructed_marker = point.clone();
        constructed_marker.set_test_id("duplicate");
        constructed_marker
    };
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&point, &duplicate, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        None
    );
    payload[130..134].fill(0);
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_identity_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&point, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
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
    let external = {
        let marker_id: String = "external".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 7, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(7), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.014, 0.016]);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let marker_id: String = "curve".into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = SketchInputEntity::new(
            marker_id,
            marker_parent,
            3,
            0,
            SketchInputKind::LineOrCircle,
        );
        constructed_marker.feature_ref = Some("sketch".into());
        constructed_marker = constructed_marker.with_test_identity(Some(3), None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };

    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_declared_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_declared_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].fill(0);
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_declared_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        None
    );
    payload[96..98].fill(0xff);
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_declared_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
        None
    );
    payload[96..98].copy_from_slice(&8u16.to_le_bytes());
    payload[110] = 0;
    assert_eq!(
        ({
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = extended_declared_inline_line_endpoints(
                &ctx,
                &payload,
                &curve,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &[&external, &curve],
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx, &payload,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            result
        })
        .map(|endpoints| endpoints.map(cadmpeg_ir::units::FiniteVector::get)),
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
        sketch_input_entities(&payload, "lane")[0].kind(),
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

    let entity = |id: &str, object_index, coordinates_m: Option<[f64; 2]>| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("profile".into());
        constructed_marker = constructed_marker.with_test_identity(object_index, None);
        constructed_marker.state_value = cadmpeg_ir::scalar::FiniteReal::new(1.0);
        constructed_marker.coordinates_m =
            coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        constructed_marker.links = None;
        constructed_marker
    };
    let curve = {
        let mut constructed_marker = entity("curve", Some(2), None);
        constructed_marker.reclassify(SketchInputKind::LineOrCircle);
        constructed_marker
    };
    let implicit = entity("implicit", None, Some([1.0, 2.0]));
    let explicit = entity("explicit", Some(4), Some([3.0, 4.0]));
    let markers = [&curve, &implicit, &explicit];
    assert_eq!(
        extended_direct_object_line_endpoints(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &curve,
            &markers
        )
        .unwrap()
        .map(|endpoints| endpoints.map(crate::records::SketchInputEntity::id)),
        Some(["implicit", "explicit"])
    );
    let arc = {
        let mut constructed_marker = curve.clone();
        constructed_marker.reclassify(SketchInputKind::Arc);
        constructed_marker
    };
    assert_eq!(
        extended_direct_object_line_endpoints(
            &cadmpeg_test_support::service_decode_context(),
            &payload,
            &arc,
            &markers
        )
        .unwrap(),
        None
    );
    let wrong_first = entity("wrong-first", Some(5), Some([5.0, 6.0]));
    let wrong_second = entity("wrong-second", Some(6), Some([7.0, 8.0]));
    let mut linked_curve = curve.clone();
    linked_curve.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 5,
                entity_ref: wrong_first.id().to_string(),
            },
            SketchInputLink {
                local_id: 6,
                entity_ref: wrong_second.id().to_string(),
            },
        ],
    );
    let markers = [
        &linked_curve,
        &implicit,
        &explicit,
        &wrong_first,
        &wrong_second,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id(), *marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = marker_curve_endpoint_markers(
                &ctx,
                &payload,
                &linked_curve,
                &markers_by_id,
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
            );
            result
        }
        .unwrap()
        .iter()
        .map(|endpoint| endpoint.id())
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

mod coordinate_rosters;
