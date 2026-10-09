//! Compact-legacy 96-byte profile-roster tests.

use super::super::super::LEGACY_SKETCH_MARKER;
use crate::layout::compact_legacy_96_profile_roster_curve as legacy_96;
use crate::records::{SketchInputEntity, SketchInputKind, SketchRelationKind};
use crate::resolved_features::endpoints::compact_legacy_96_profile_roster_curve_uses_complete_roster;
use crate::resolved_features::endpoints::coordinate_roster_endpoint_offset;
use crate::resolved_features::endpoints::roster_curve_endpoint_markers;
use crate::resolved_features::endpoints::geometry_index::{MarkerGeometryIndex, MarkerPrefixIndex};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn profile_roster_payload(endpoints: [u16; 2]) -> Vec<u8> {
    let mut payload = vec![0; legacy_96::LEN + LEGACY_SKETCH_MARKER.len()];
    payload[legacy_96::MARKER..legacy_96::HEADER].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[legacy_96::HEADER..legacy_96::SHARED_SELECTOR].fill(0xff);
    payload[legacy_96::SHARED_SELECTOR..legacy_96::NATIVE_KIND]
        .copy_from_slice(&legacy_96::SHARED_SELECTOR_VALUE.to_le_bytes());
    payload[legacy_96::NATIVE_KIND..legacy_96::NATIVE_KIND + std::mem::size_of::<u32>()]
        .copy_from_slice(&legacy_96::NATIVE_KIND_VALUE.to_le_bytes());
    payload[legacy_96::PROFILE_LOCUS..legacy_96::ROLE].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[legacy_96::ROLE..legacy_96::STATE_AT_29]
        .copy_from_slice(&legacy_96::ROLE_VALUE.to_le_bytes());
    payload[legacy_96::STATE_AT_29..legacy_96::SELECTOR]
        .copy_from_slice(&legacy_96::STATE_AT_29_VALUE.to_le_bytes());
    payload[legacy_96::SELECTOR..legacy_96::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[legacy_96::STATE_VALUE..legacy_96::ENDPOINT_FIRST]
        .copy_from_slice(&legacy_96::STATE_VALUE_VALUE.to_le_bytes());
    payload[legacy_96::ENDPOINT_FIRST..legacy_96::ENDPOINT_SECOND]
        .copy_from_slice(&endpoints[0].to_le_bytes());
    payload[legacy_96::ENDPOINT_SECOND..legacy_96::ZERO_ENDPOINT_PREFIX]
        .copy_from_slice(&endpoints[1].to_le_bytes());
    payload[legacy_96::ZERO_ENDPOINT_PREFIX..legacy_96::SIGNED_SELECTOR]
        .copy_from_slice(&legacy_96::ZERO_ENDPOINT_PREFIX_VALUE);
    payload[legacy_96::SIGNED_SELECTOR..legacy_96::ZERO_SELECTOR_TRAILER]
        .copy_from_slice(&legacy_96::SIGNED_SELECTOR_VALUE.to_le_bytes());
    payload[legacy_96::ZERO_SELECTOR_TRAILER..legacy_96::TAIL_STATE]
        .copy_from_slice(&legacy_96::ZERO_SELECTOR_TRAILER_VALUE);
    payload[legacy_96::TAIL_STATE..legacy_96::TAIL_STATE_PREFIX]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[legacy_96::TAIL_STATE_PREFIX..legacy_96::TAIL_STATE_MARKER]
        .copy_from_slice(&legacy_96::TAIL_STATE_PREFIX_VALUE.to_le_bytes());
    payload[legacy_96::TAIL_STATE_MARKER..legacy_96::ZERO_TAIL_IDENTITY]
        .copy_from_slice(&legacy_96::TAIL_STATE_MARKER_VALUE.to_le_bytes());
    payload[legacy_96::ZERO_TAIL_IDENTITY..legacy_96::ONE_TAIL_IDENTITY]
        .copy_from_slice(&legacy_96::ZERO_TAIL_IDENTITY_VALUE.to_le_bytes());
    payload[legacy_96::ONE_TAIL_IDENTITY..legacy_96::LEN]
        .copy_from_slice(&legacy_96::ONE_TAIL_IDENTITY_VALUE.to_le_bytes());
    payload[legacy_96::LEN..].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload
}

#[test]
fn compact_legacy_96_profile_roster_uses_coordinate_geometry_ordinals() {
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>, kind, object_index| {
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
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle, None);
    let first = entity(
        "first",
        10,
        Some([0.0, 0.0]),
        SketchInputKind::Point,
        Some(90),
    );
    let relation = entity(
        "relation",
        15,
        Some([0.5, 0.0]),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
        Some(3),
    );
    let coordinate_line = entity(
        "coordinate-line",
        20,
        Some([1.0, 0.0]),
        SketchInputKind::LineOrCircle,
        Some(91),
    );
    let second = entity(
        "second",
        30,
        Some([1.0, 1.0]),
        SketchInputKind::Point,
        Some(92),
    );
    let first_arc = entity(
        "first-arc",
        40,
        Some([0.0, 1.0]),
        SketchInputKind::Arc,
        Some(3),
    );
    let third = entity(
        "third",
        50,
        Some([2.0, 1.0]),
        SketchInputKind::Point,
        Some(94),
    );
    let second_arc = entity(
        "second-arc",
        60,
        Some([2.0, 2.0]),
        SketchInputKind::Arc,
        Some(5),
    );
    let markers = [
        &curve,
        &first,
        &relation,
        &coordinate_line,
        &second,
        &first_arc,
        &third,
        &second_arc,
    ];
    let payload = profile_roster_payload([3, 5]);

    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &payload, 0
    ));
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
                .unwrap(),
                None,
            );
            result
        }
        .unwrap()
        .iter()
        .map(|marker| marker.id())
        .collect::<Vec<_>>(),
        ["first-arc", "second-arc"]
    );

    let mut alternate_header = payload.clone();
    alternate_header[legacy_96::HEADER..legacy_96::SHARED_SELECTOR]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &alternate_header,
        0
    ));
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let result = roster_curve_endpoint_markers(
                &ctx,
                &alternate_header,
                &curve,
                &markers,
                &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    &ctx,
                    &markers,
                    crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                        &ctx,
                        &alternate_header,
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
        ["first-arc", "second-arc"]
    );

    let mut equal_endpoints = profile_roster_payload([3, 3]);
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&equal_endpoints, 0));
    assert!({
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = roster_curve_endpoint_markers(
            &ctx,
            &equal_endpoints,
            &curve,
            &markers,
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &ctx,
                    &equal_endpoints,
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

    equal_endpoints[legacy_96::TAIL_STATE..legacy_96::TAIL_STATE + 2]
        .copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&equal_endpoints, 0));

    let mut malformed_tail_marker = profile_roster_payload([3, 5]);
    malformed_tail_marker[legacy_96::TAIL_STATE_MARKER..legacy_96::ZERO_TAIL_IDENTITY]
        .copy_from_slice(&2u16.to_le_bytes());
    assert!(
        !compact_legacy_96_profile_roster_curve_uses_complete_roster(&malformed_tail_marker, 0)
    );

    let mut malformed_tail = profile_roster_payload([3, 5]);
    malformed_tail[legacy_96::ZERO_SELECTOR_TRAILER] = 1;
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&malformed_tail, 0));

    let mut missing_boundary = profile_roster_payload([3, 5]);
    missing_boundary[legacy_96::LEN..].fill(0);
    assert!(!compact_legacy_96_profile_roster_curve_uses_complete_roster(&missing_boundary, 0));

    let out_of_range = profile_roster_payload([3, 6]);
    assert!(compact_legacy_96_profile_roster_curve_uses_complete_roster(
        &out_of_range,
        0
    ));
    assert!({
        let ctx = cadmpeg_test_support::service_decode_context();
        let result = roster_curve_endpoint_markers(
            &ctx,
            &out_of_range,
            &curve,
            &markers,
            &crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                &ctx,
                &markers,
                crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
                    &ctx,
                    &out_of_range,
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

fn endpoint_storage_entities() -> [SketchInputEntity; 3] {
    let mut curve = SketchInputEntity::new("curve", "lane", 0, 0, SketchInputKind::LineOrCircle);
    curve.feature_ref = Some("feature".into());
    let mut first = SketchInputEntity::new("first", "lane", 1, 10, SketchInputKind::Point);
    first.feature_ref = Some("feature".into());
    first.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
    let mut second = SketchInputEntity::new("second", "lane", 2, 20, SketchInputKind::Point);
    second.feature_ref = Some("feature".into());
    second.coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);
    [curve, first, second]
}

// collect_vec uses the four-slot minimum for pointer-sized elements. The
// returned vector owns those slots; its markers remain borrowed native records.
const ENDPOINT_VECTOR_BYTES: u64 = 4 * std::mem::size_of::<&SketchInputEntity>() as u64;

#[test]
fn compact_legacy_96_profile_roster_publishes_only_endpoint_slots() {
    let payload = profile_roster_payload([0, 1]);
    let entities = endpoint_storage_entities();
    let markers = [&entities[0], &entities[1], &entities[2]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = ENDPOINT_VECTOR_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = MarkerGeometryIndex::new(
        &ctx, &markers, MarkerPrefixIndex::new(&ctx, &payload).unwrap(),
    ).unwrap();
    let endpoints = roster_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &markers, &geometry, None,
    ).unwrap();
    assert_eq!(endpoints.len(), 2);
    assert!(std::ptr::eq(endpoints[0], &entities[1]));
    assert!(std::ptr::eq(endpoints[1], &entities[2]));
    assert_eq!(endpoints[0].coordinates_m.unwrap().get(), [0.0, 0.0]);
    assert_eq!(endpoints[1].coordinates_m.unwrap().get(), [1.0, 0.0]);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn compact_legacy_96_profile_roster_charges_each_live_output_once() {
    let payload = profile_roster_payload([0, 1]);
    let entities = endpoint_storage_entities();
    let markers = [&entities[0], &entities[1], &entities[2]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2 * ENDPOINT_VECTOR_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = MarkerGeometryIndex::new(
        &ctx, &markers, MarkerPrefixIndex::new(&ctx, &payload).unwrap(),
    ).unwrap();
    let first = roster_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &markers, &geometry, None,
    ).unwrap();
    let second = roster_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &markers, &geometry, None,
    ).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    for endpoints in [&first, &second] {
        assert!(std::ptr::eq(endpoints[0], &entities[1]));
        assert!(std::ptr::eq(endpoints[1], &entities[2]));
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn compact_legacy_96_profile_roster_preserves_publication_refusal() {
    let payload = profile_roster_payload([0, 1]);
    let entities = endpoint_storage_entities();
    let markers = [&entities[0], &entities[1], &entities[2]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = ENDPOINT_VECTOR_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = MarkerGeometryIndex::new(
        &ctx, &markers, MarkerPrefixIndex::new(&ctx, &payload).unwrap(),
    ).unwrap();
    let decode = || roster_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &markers, &geometry, None,
    );
    let CodecError::ResourceLimit(limit) = decode().unwrap_err() else {
        panic!("expected endpoint publication refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "resolve SLDPRT compact legacy 96 profile roster endpoint markers");
    assert_eq!(limit.limit, ENDPOINT_VECTOR_BYTES - 1);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, ENDPOINT_VECTOR_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = decode().unwrap_err() else {
        panic!("expected sticky endpoint refusal");
    };
    assert_eq!(repeated, limit);
}
